//! Game actions behind the WebSocket messages (design sections 7 and 8).
//!
//! Every method reads and writes Redis through [`LiveStore`] and announces
//! what happened through the [`Hub`]. Nothing here keeps game state in
//! memory except a cache of quiz snapshots, which never change during a game.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use myquiz_engine::answer::{validate_submission, Submission};
use myquiz_engine::ranking::{leaderboard, podium, rank, Ranked, Standing};
use myquiz_engine::reveal::{score_question, time_charged_ms};
use myquiz_engine::view::{PublicQuestion, RevealedAnswer};
use myquiz_engine::{Command, GameState, QuizSnapshot, TransitionError};
use myquiz_protocol::ws::*;
use sqlx::PgPool;
use uuid::Uuid;

use super::hub::{encode, Delivery, Hub};
use crate::live::{
    secrets, ChatSlot, GameMeta, JoinRefusal, Joined, LiveStore, NewPlayer, PlayerRecord,
    Transition,
};

/// Lobby and progress pushes are batched to at most one per this interval
/// per game and server (design section 7).
const BATCH_INTERVAL: Duration = Duration::from_millis(250);

/// Chat limits (players' popups on the host screen). A player may send one
/// text every few seconds and an emoji about once a second; the game as a
/// whole is capped too, so a full class cannot flood the projector.
pub const CHAT_TEXT_COOLDOWN_MS: u64 = 4_000;
pub const CHAT_EMOJI_COOLDOWN_MS: u64 = 1_000;
pub const CHAT_PER_GAME_PER_SEC: u32 = 12;

pub const NICK_MIN: usize = 2;
pub const NICK_MAX: usize = 20;

/// A refusal code for an ack, e.g. `PIN_INVALID` or `LATE`.
pub type Code = &'static str;

pub struct GameService {
    pub store: LiveStore,
    pub db: PgPool,
    pub hub: Arc<Hub>,
    quizzes: Mutex<HashMap<String, Arc<QuizSnapshot>>>,
    lobby_pending: Arc<Mutex<HashSet<String>>>,
    progress_pending: Arc<Mutex<HashSet<String>>>,
}

impl GameService {
    pub fn new(store: LiveStore, db: PgPool, hub: Arc<Hub>) -> Arc<Self> {
        Arc::new(Self {
            store,
            db,
            hub,
            quizzes: Mutex::new(HashMap::new()),
            lobby_pending: Arc::default(),
            progress_pending: Arc::default(),
        })
    }

    async fn quiz(&self, pin: &str) -> Result<Arc<QuizSnapshot>> {
        if let Some(q) = self.quizzes.lock().unwrap().get(pin) {
            return Ok(q.clone());
        }
        let q = Arc::new(
            self.store
                .quiz(pin)
                .await?
                .context("quiz snapshot missing")?,
        );
        self.quizzes
            .lock()
            .unwrap()
            .insert(pin.to_string(), q.clone());
        Ok(q)
    }

    fn forget(&self, pin: &str) {
        self.quizzes.lock().unwrap().remove(pin);
    }

    // ------------------------------------------------------------------
    // Snapshots

    pub async fn snapshot(&self, pin: &str, me: Option<&str>) -> Result<GameSnapshot> {
        let meta = self.store.meta(pin).await?.context("no such game")?;
        let players = self.store.players(pin).await?;
        let for_host = me.is_none();
        let question = match meta.state {
            GameState::Question { index } => {
                let quiz = self.quiz(pin).await?;
                let q = quiz.question(index).context("bad index")?;
                Some(QuestionStart {
                    question: PublicQuestion::from_question(index, quiz.total(), q),
                    starts_at: meta.starts_at_ms,
                    ends_at: meta.ends_at_ms,
                })
            }
            _ => None,
        };
        let me = match me {
            Some(id) => {
                let ranked = ranked(&players);
                match ranked.iter().find(|r| r.standing.player.0 == id) {
                    Some(r) => Some(MyStanding {
                        player_id: id.to_string(),
                        nickname: r.standing.player.1.clone(),
                        avatar: r.standing.player.2.clone(),
                        score: r.standing.score,
                        rank: r.rank,
                        answered: match meta.state {
                            GameState::Question { index } => {
                                self.store.has_answered(pin, index, id).await?
                            }
                            _ => false,
                        },
                        muted: players.iter().any(|(pid, p)| pid == id && p.muted),
                    }),
                    None => None,
                }
            }
            None => None,
        };
        // A screen that reconnects after the question ended must draw the
        // same view the pushes drew: the reveal, the leaderboard or the podium.
        let (mut reveal, mut result, mut top) = (None, None, None);
        match meta.state {
            GameState::Reveal { index } => {
                let quiz = self.quiz(pin).await?;
                let q = quiz.question(index).context("bad index")?;
                let answers = self.store.answers(pin, index).await?;
                let outcome = score_question(q, &answers);
                let revealed = RevealedAnswer::from_question(q);
                match &me {
                    None => {
                        reveal = Some(QuestionReveal {
                            index,
                            correct_option_ids: revealed.correct_option_ids,
                            accepted_answers: revealed.accepted_answers,
                            distribution: outcome.distribution,
                            top_text_answers: outcome.top_text_answers,
                        });
                    }
                    Some(mine) => {
                        let res = outcome.results.iter().find(|r| r.player == mine.player_id);
                        result = Some(QuestionResult {
                            index,
                            answered: res.is_some(),
                            correct: res.is_some_and(|r| r.correct),
                            points: res.map_or(0, |r| r.points),
                            total: mine.score,
                            rank: mine.rank,
                            correct_option_ids: revealed.correct_option_ids,
                        });
                    }
                }
            }
            GameState::Leaderboard { .. } => top = Some(rows(leaderboard(&ranked(&players)))),
            GameState::Podium => top = Some(rows(podium(&ranked(&players)))),
            _ => {}
        }
        // The host's reveal screen also shows the question it reveals.
        let question = match (question, meta.state) {
            (None, GameState::Reveal { index }) if for_host => {
                let quiz = self.quiz(pin).await?;
                let q = quiz.question(index).context("bad index")?;
                Some(QuestionStart {
                    question: PublicQuestion::from_question(index, quiz.total(), q),
                    starts_at: meta.starts_at_ms,
                    ends_at: meta.ends_at_ms,
                })
            }
            (q, _) => q,
        };
        Ok(GameSnapshot {
            pin: pin.to_string(),
            state: meta.state,
            locked: meta.locked,
            theme: meta.theme.clone(),
            chat: meta.chat.clone(),
            phones_show_question: meta.phones_show_question,
            players: lobby_list(&players),
            question,
            report_id: for_host.then(|| meta.session.clone()),
            me,
            reveal,
            result,
            top,
        })
    }

    // ------------------------------------------------------------------
    // Host

    /// `host:attach`: the cookie's user must be the game's host (SEC-07).
    pub async fn attach_host(
        &self,
        user_id: &str,
        pin: &str,
    ) -> Result<Result<GameSnapshot, Code>> {
        match self.store.meta(pin).await? {
            Some(m) if m.host == user_id && m.state != GameState::Ended => {
                Ok(Ok(self.snapshot(pin, None).await?))
            }
            Some(m) if m.host != user_id => Ok(Err("FORBIDDEN")),
            _ => Ok(Err("PIN_INVALID")),
        }
    }

    /// `host:theme`: the host picks the game's look in the lobby.
    pub async fn set_theme(&self, pin: &str, theme: &str) -> Result<Result<(), Code>> {
        if !THEMES.contains(&theme) {
            return Ok(Err("INVALID"));
        }
        let Some(meta) = self.store.meta(pin).await? else {
            return Ok(Err("PIN_INVALID"));
        };
        if meta.state != GameState::Lobby {
            return Ok(Err("WRONG_STATE"));
        }
        self.store.set_theme(pin, theme).await?;
        self.hub
            .to_all(
                pin,
                &ServerPush::GameTheme(GameTheme {
                    theme: theme.into(),
                }),
            )
            .await?;
        Ok(Ok(()))
    }

    /// `host:chat`: who may chat, at any point of the game.
    pub async fn set_chat(&self, pin: &str, mode: &str) -> Result<Result<(), Code>> {
        if !CHAT_MODES.contains(&mode) {
            return Ok(Err("INVALID"));
        }
        self.store.set_chat(pin, mode).await?;
        self.hub
            .to_all(pin, &ServerPush::GameChat(GameChat { mode: mode.into() }))
            .await?;
        Ok(Ok(()))
    }

    /// `host:phones`: whether phones show the question and its image/code
    /// or only the answer buttons, at any point of the game. Players still
    /// receive the question either way; this only changes what they draw.
    pub async fn set_phones(&self, pin: &str, show_question: bool) -> Result<()> {
        self.store.set_phones(pin, show_question).await?;
        self.hub
            .to_all(pin, &ServerPush::GamePhones(GamePhones { show_question }))
            .await
    }

    pub async fn lock(&self, pin: &str, locked: bool) -> Result<()> {
        self.store.set_locked(pin, locked).await?;
        self.push_lobby_now(pin).await
    }

    /// `host:kick` (`ban` false) and `host:ban`. Either way the player
    /// leaves the game and the report; only a ban keeps their device out.
    pub async fn kick(&self, pin: &str, player_id: &str, ban: bool) -> Result<Result<(), Code>> {
        if !self.store.kick(pin, player_id, ban).await? {
            return Ok(Err("NOT_FOUND"));
        }
        sqlx::query("UPDATE session_players SET kicked = true WHERE id = $1")
            .bind(parse_uuid(player_id)?)
            .execute(&self.db)
            .await?;
        self.hub
            .publish(
                pin,
                Delivery::Player {
                    id: player_id.to_string(),
                    text: encode(&ServerPush::PlayerKicked(PlayerKicked { banned: ban })),
                },
            )
            .await?;
        self.push_lobby_now(pin).await?;
        Ok(Ok(()))
    }

    /// `host:mute`: stops or allows again a player's chat.
    pub async fn mute(&self, pin: &str, player_id: &str, muted: bool) -> Result<Result<(), Code>> {
        if !self.store.set_muted(pin, player_id, muted).await? {
            return Ok(Err("NOT_FOUND"));
        }
        self.hub
            .publish(
                pin,
                Delivery::Player {
                    id: player_id.to_string(),
                    text: encode(&ServerPush::PlayerMuted(PlayerMuted { muted })),
                },
            )
            .await?;
        self.push_lobby_now(pin).await?;
        Ok(Ok(()))
    }

    /// `host:rejoin`: the host lets a returning player's new device in.
    pub async fn allow_rejoin(&self, pin: &str, player_id: &str) -> Result<Result<(), Code>> {
        if !self.store.allow_rejoin(pin, player_id).await? {
            return Ok(Err("NOT_FOUND"));
        }
        Ok(Ok(()))
    }

    pub async fn start(&self, pin: &str) -> Result<Result<(), Code>> {
        let r = self.store.command(pin, Command::Start).await?;
        self.after_command(pin, r).await
    }

    /// `host:next`. Podium → Ended goes through [`Self::end_game`] so the
    /// PIN is released and results saved.
    pub async fn next(&self, pin: &str, expect: GameState) -> Result<Result<(), Code>> {
        let meta = self.store.meta(pin).await?.context("no such game")?;
        if meta.state != expect {
            return Ok(Err("STALE"));
        }
        if meta.state == GameState::Podium {
            self.end_game(pin, EndReason::Finished).await?;
            return Ok(Ok(()));
        }
        let r = self.store.command(pin, Command::Next { expect }).await?;
        self.after_command(pin, r).await
    }

    pub async fn skip(&self, pin: &str, index: usize) -> Result<Result<(), Code>> {
        match self.store.meta(pin).await? {
            Some(m) if m.state == (GameState::Question { index }) => {
                self.finish_question(pin, index).await?;
                Ok(Ok(()))
            }
            _ => Ok(Err("STALE")),
        }
    }

    async fn after_command(
        &self,
        pin: &str,
        r: Result<(GameState, Transition), TransitionError>,
    ) -> Result<Result<(), Code>> {
        let (to, t) = match r {
            Ok(x) => x,
            Err(TransitionError::Stale) => return Ok(Err("STALE")),
            Err(TransitionError::WrongState) => return Ok(Err("WRONG_STATE")),
            Err(TransitionError::EmptyQuiz) => return Ok(Err("EMPTY_QUIZ")),
        };
        let Transition::Applied {
            starts_at_ms,
            ends_at_ms,
        } = t
        else {
            return Ok(Err("STALE"));
        };
        match to {
            GameState::Question { index } => {
                if index == 0 {
                    self.mark_started(pin).await?;
                }
                let quiz = self.quiz(pin).await?;
                let q = quiz.question(index).context("bad index")?;
                let push = ServerPush::QuestionStart(QuestionStart {
                    question: PublicQuestion::from_question(index, quiz.total(), q),
                    starts_at: starts_at_ms,
                    ends_at: ends_at_ms,
                });
                self.hub.to_all(pin, &push).await?;
            }
            GameState::Leaderboard { .. } => {
                let players = self.store.players(pin).await?;
                let top = rows(leaderboard(&ranked(&players)));
                self.hub
                    .to_all(pin, &ServerPush::LeaderboardShow(Leaderboard { top }))
                    .await?;
            }
            GameState::Podium => {
                // Save the results now: many hosts close the tab on the
                // podium without pressing Finish, and the report must not
                // stay "running" for ever.
                let meta = self.store.meta(pin).await?.context("no such game")?;
                self.save_results(pin, &meta).await?;
                let players = self.store.players(pin).await?;
                let top = rows(podium(&ranked(&players)));
                self.hub
                    .to_all(pin, &ServerPush::GamePodium(Leaderboard { top }))
                    .await?;
            }
            _ => {}
        }
        Ok(Ok(()))
    }

    async fn mark_started(&self, pin: &str) -> Result<()> {
        let meta = self.store.meta(pin).await?.context("no such game")?;
        sqlx::query(
            "UPDATE game_sessions SET status = 'running', started_at = now() WHERE id = $1",
        )
        .bind(parse_uuid(&meta.session)?)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Players

    /// `player:join` (FR-GM-02).
    pub async fn join(
        &self,
        req: &PlayerJoin,
        user_id: Option<String>,
    ) -> Result<Result<PlayerJoined, Code>> {
        let nickname = req
            .nickname
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let len = nickname.chars().count();
        if !(NICK_MIN..=NICK_MAX).contains(&len)
            || nickname.chars().any(char::is_control)
            || myquiz_engine::profanity::is_offensive(&nickname)
        {
            return Ok(Err("NICK_INVALID"));
        }
        if req.device_id.is_empty() || req.device_id.len() > 64 {
            return Ok(Err("INVALID"));
        }
        let Some(meta) = self.store.meta(&req.pin).await? else {
            return Ok(Err("PIN_INVALID"));
        };
        let player_id = Uuid::new_v4().to_string();
        let token = secrets::new_player_token();
        let joined = self
            .store
            .join(
                &req.pin,
                &player_id,
                &NewPlayer {
                    nickname: nickname.clone(),
                    avatar: pick_avatar(req.avatar.as_deref(), rand::random()),
                    device_id: req.device_id.clone(),
                    user_id: user_id.clone(),
                    token_hash: secrets::hash_token(&token),
                },
            )
            .await?;
        let join_order = match joined {
            Ok(Joined::New(o)) => o,
            Ok(Joined::Reclaimed { player_id }) => {
                tracing::info!(pin = req.pin, player_id, "player took their name back");
                self.schedule_lobby(&req.pin);
                return Ok(Ok(PlayerJoined {
                    snapshot: self.snapshot(&req.pin, Some(&player_id)).await?,
                    player_id,
                    token,
                }));
            }
            Err(JoinRefusal::RejoinWait { player_id }) => {
                let players = self.store.players(&req.pin).await?;
                if let Some((id, p)) = players.iter().find(|(id, _)| *id == player_id) {
                    self.hub
                        .to_host(
                            &req.pin,
                            &ServerPush::RejoinRequest(RejoinRequest {
                                player_id: id.clone(),
                                nickname: p.nickname.clone(),
                                avatar: avatar_of(id, p),
                            }),
                        )
                        .await?;
                }
                return Ok(Err("REJOIN_WAIT"));
            }
            Err(r) => return Ok(Err(refusal_code(r))),
        };

        let insert = sqlx::query(
            "INSERT INTO session_players (id, session_id, user_id, nickname, join_order) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(parse_uuid(&player_id)?)
        .bind(parse_uuid(&meta.session)?)
        .bind(user_id.as_deref().map(parse_uuid).transpose()?)
        .bind(&nickname)
        .bind(join_order as i32)
        .execute(&self.db)
        .await;
        if let Err(e) = insert {
            // Undo the Redis join so Redis and PostgreSQL agree.
            let _ = self.store.kick(&req.pin, &player_id, false).await;
            return Err(e.into());
        }

        self.schedule_lobby(&req.pin);
        Ok(Ok(PlayerJoined {
            snapshot: self.snapshot(&req.pin, Some(&player_id)).await?,
            player_id,
            token,
        }))
    }

    /// `player:resume` (FR-GM-09).
    pub async fn resume(&self, req: &PlayerResume) -> Result<Result<(String, GameSnapshot), Code>> {
        let hash = secrets::hash_token(&req.token);
        let Some(player_id) = self.store.player_for_token(&req.pin, &hash).await? else {
            return Ok(Err("TOKEN_INVALID"));
        };
        let snap = self.snapshot(&req.pin, Some(&player_id)).await?;
        if snap.state == GameState::Ended {
            return Ok(Err("PIN_INVALID"));
        }
        Ok(Ok((player_id, snap)))
    }

    pub async fn player_online(&self, pin: &str, player_id: &str, online: bool) -> Result<()> {
        self.store
            .set_online(pin, player_id, if online { 1 } else { -1 })
            .await?;
        if online {
            self.schedule_lobby(pin);
        } else {
            // Someone leaving can make "everyone answered" true.
            self.maybe_reveal_early(pin).await?;
        }
        Ok(())
    }

    /// `answer:submit` (FR-GM-06). Acked only after Redis stored it.
    pub async fn answer(
        self: &Arc<Self>,
        pin: &str,
        player_id: &str,
        index: usize,
        submission: &Submission,
    ) -> Result<Result<(), Code>> {
        let quiz = self.quiz(pin).await?;
        let Some(q) = quiz.question(index) else {
            return Ok(Err("WRONG_STATE"));
        };
        if validate_submission(q, submission).is_err() {
            return Ok(Err("INVALID"));
        }
        match self
            .store
            .submit_answer(pin, index, player_id, submission)
            .await?
        {
            Ok(_) => {
                self.schedule_progress(pin);
                if self.store.all_connected_answered(pin, index).await? {
                    // Reveal in the background so this player's ack is not delayed.
                    let this = self.clone();
                    let pin = pin.to_string();
                    tokio::spawn(async move {
                        if let Err(e) = this.finish_question(&pin, index).await {
                            tracing::error!(error = %e, pin, "early reveal failed");
                        }
                    });
                }
                Ok(Ok(()))
            }
            Err(e) => {
                tracing::info!(pin, player_id, index, code = e.code(), "answer refused");
                Ok(Err(e.code()))
            }
        }
    }

    /// `player:chat`: a short text or a quick emoji for the host screen.
    /// Nothing is stored; the host sees it as a popup that fades.
    pub async fn chat(
        &self,
        pin: &str,
        player_id: &str,
        msg: &PlayerChat,
    ) -> Result<Result<(), Code>> {
        let Some(meta) = self.store.meta(pin).await? else {
            return Ok(Err("PIN_INVALID"));
        };
        if meta.state == GameState::Ended {
            return Ok(Err("WRONG_STATE"));
        }
        let (text, emoji) = match (&msg.text, &msg.emoji) {
            (Some(_), Some(_)) | (None, None) => return Ok(Err("INVALID")),
            (None, Some(e)) => {
                if !CHAT_EMOJIS.contains(&e.as_str()) {
                    return Ok(Err("INVALID"));
                }
                (None, Some(e.clone()))
            }
            (Some(t), None) => match clean_chat(t) {
                Ok(t) => (Some(t), None),
                Err(c) => return Ok(Err(c)),
            },
        };
        match (meta.chat.as_str(), text.is_some()) {
            ("off", _) | ("emoji", true) => return Ok(Err("CHAT_OFF")),
            _ => {}
        }
        // Typing during a question would pull eyes off it, on the phone and
        // on the projector: only emoji then.
        if text.is_some() && matches!(meta.state, GameState::Question { .. }) {
            return Ok(Err("CHAT_BUSY"));
        }
        let Some(player) = self.store.player(pin, player_id).await? else {
            return Ok(Err("FORBIDDEN"));
        };
        if player.muted {
            return Ok(Err("MUTED"));
        }
        let (kind, cooldown) = if text.is_some() {
            ("text", CHAT_TEXT_COOLDOWN_MS)
        } else {
            ("emoji", CHAT_EMOJI_COOLDOWN_MS)
        };
        match self
            .store
            .chat_slot(pin, player_id, kind, cooldown, CHAT_PER_GAME_PER_SEC)
            .await?
        {
            ChatSlot::Granted => {}
            ChatSlot::TooSoon | ChatSlot::GameBusy => return Ok(Err("CHAT_SLOW")),
        }
        let avatar = avatar_of(player_id, &player);
        self.hub
            .to_host(
                pin,
                &ServerPush::ChatMessage(ChatMessage {
                    player_id: player_id.to_string(),
                    nickname: player.nickname,
                    avatar,
                    text,
                    emoji,
                }),
            )
            .await?;
        Ok(Ok(()))
    }

    async fn maybe_reveal_early(&self, pin: &str) -> Result<()> {
        let Some(meta) = self.store.meta(pin).await? else {
            return Ok(());
        };
        if let GameState::Question { index } = meta.state {
            if self.store.all_connected_answered(pin, index).await? {
                self.finish_question(pin, index).await?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Reveal and end

    /// Question → Reveal: score, save, and tell everyone (FR-GM-07, FR-GM-08,
    /// FR-SC-01). Safe to call from several places at once; only the caller
    /// that wins the Redis reveal does anything.
    pub async fn finish_question(&self, pin: &str, index: usize) -> Result<()> {
        let Some(answers) = self.store.reveal(pin, index).await? else {
            return Ok(());
        };
        let quiz = self.quiz(pin).await?;
        let q = quiz.question(index).context("bad index")?;
        let outcome = score_question(q, &answers);

        let by_player: HashMap<&str, _> = outcome
            .results
            .iter()
            .map(|r| (r.player.as_str(), r))
            .collect();
        let players = self.store.players(pin).await?;
        let updates: Vec<(String, u32, u64)> = players
            .iter()
            .map(|(id, _)| {
                let r = by_player.get(id.as_str());
                (
                    id.clone(),
                    r.map_or(0, |r| r.points),
                    time_charged_ms(q, r.map(|r| r.response_ms)),
                )
            })
            .collect();
        self.store.apply_scores(pin, index, &updates).await?;

        let meta = self.store.meta(pin).await?.context("no such game")?;
        self.save_answers(&meta.session, index, &answers, &outcome.results)
            .await?;

        let revealed = RevealedAnswer::from_question(q);
        self.hub
            .to_host(
                pin,
                &ServerPush::QuestionReveal(QuestionReveal {
                    index,
                    correct_option_ids: revealed.correct_option_ids.clone(),
                    accepted_answers: revealed.accepted_answers,
                    distribution: outcome.distribution,
                    top_text_answers: outcome.top_text_answers,
                }),
            )
            .await?;

        let players = self.store.players(pin).await?;
        let texts = ranked(&players)
            .into_iter()
            .map(|r| {
                let id = r.standing.player.0;
                let res = by_player.get(id.as_str());
                let push = ServerPush::QuestionResult(QuestionResult {
                    index,
                    answered: res.is_some(),
                    correct: res.is_some_and(|r| r.correct),
                    points: res.map_or(0, |r| r.points),
                    total: r.standing.score,
                    rank: r.rank,
                    correct_option_ids: revealed.correct_option_ids.clone(),
                });
                (id, encode(&push))
            })
            .collect();
        self.hub.publish(pin, Delivery::Each { texts }).await?;
        Ok(())
    }

    async fn save_answers(
        &self,
        session: &str,
        index: usize,
        answers: &[myquiz_engine::reveal::StoredAnswer<String>],
        results: &[myquiz_engine::reveal::AnswerResult<String>],
    ) -> Result<()> {
        if answers.is_empty() {
            return Ok(());
        }
        // One multi-row insert per question (design section 4).
        let mut qb = sqlx::QueryBuilder::new(
            "INSERT INTO answers (session_id, player_id, question_index, option_ids, \
             text_answer, is_correct, response_ms, points, received_at) ",
        );
        let session = parse_uuid(session)?;
        let rows: Vec<_> = answers.iter().zip(results).collect();
        let mut parsed = Vec::with_capacity(rows.len());
        for (a, r) in &rows {
            parsed.push((parse_uuid(&a.player)?, a, r));
        }
        qb.push_values(parsed, |mut b, (player, a, r)| {
            let ids: Vec<i32> = a.submission.option_ids.iter().map(|&i| i as i32).collect();
            b.push_bind(session)
                .push_bind(player)
                .push_bind(index as i32)
                .push_bind(if ids.is_empty() { None } else { Some(ids) })
                .push_bind(a.submission.text.clone())
                .push_bind(r.correct)
                .push_bind(r.response_ms as i32)
                .push_bind(r.points as i32)
                .push("now()");
        });
        qb.push(" ON CONFLICT (player_id, question_index) DO NOTHING");
        qb.build().execute(&self.db).await?;
        Ok(())
    }

    /// Ends the game now and saves final results (FR-GM-05 end early,
    /// FR-SC-05). A running question is scored first so no answer is lost.
    pub async fn end_game(&self, pin: &str, reason: EndReason) -> Result<()> {
        let Some(meta) = self.store.meta(pin).await? else {
            return Ok(());
        };
        if let GameState::Question { index } = meta.state {
            self.finish_question(pin, index).await?;
        }
        if !self.store.finish(pin).await? {
            return Ok(());
        }
        self.save_results(pin, &meta).await?;

        self.hub
            .to_all(pin, &ServerPush::GameEnded(GameEnded { reason }))
            .await?;
        self.forget(pin);
        Ok(())
    }

    /// Ends games created more than `hours` ago that never finished.
    /// Returns how many it ended.
    pub async fn end_stale_games(&self, hours: i32) -> Result<u32> {
        let rows: Vec<(Uuid, Option<String>)> = sqlx::query_as(
            "SELECT id, pin FROM game_sessions \
             WHERE status <> 'ended' AND created_at < now() - make_interval(hours => $1) \
             LIMIT 100",
        )
        .bind(hours)
        .fetch_all(&self.db)
        .await?;
        let mut ended = 0;
        for (session, pin) in rows {
            let pin = pin.map(|p| p.trim().to_string());
            let live =
                match &pin {
                    Some(pin) => self.store.meta(pin).await?.filter(|m| {
                        m.session == session.to_string() && m.state != GameState::Ended
                    }),
                    None => None,
                };
            match (pin, live) {
                (Some(pin), Some(_)) => {
                    // Someone is still on it: leave it alone.
                    if self.store.connected(&pin).await? > 0 {
                        continue;
                    }
                    self.end_game(&pin, EndReason::HostTimeout).await?;
                }
                _ => close_session(&self.db, session).await?,
            }
            ended += 1;
        }
        Ok(ended)
    }

    /// Writes final scores and ranks and marks the session ended in
    /// PostgreSQL. Runs when the podium shows and again when the game ends;
    /// writing the same results twice is harmless.
    async fn save_results(&self, pin: &str, meta: &GameMeta) -> Result<()> {
        let players = self.store.players(pin).await?;
        let ranked = ranked(&players);
        let mut tx = self.db.begin().await?;
        for r in &ranked {
            let record = &players
                .iter()
                .find(|(id, _)| *id == r.standing.player.0)
                .context("player vanished")?
                .1;
            sqlx::query(
                "UPDATE session_players SET final_score = $2, final_rank = $3, total_time_ms = $4 \
                 WHERE id = $1",
            )
            .bind(parse_uuid(&r.standing.player.0)?)
            .bind(r.standing.score as i32)
            .bind(r.rank as i32)
            .bind(record.time_ms as i64)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE game_sessions SET status = 'ended', ended_at = COALESCE(ended_at, now()), pin = NULL, \
             player_count = $2 WHERE id = $1",
        )
        .bind(parse_uuid(&meta.session)?)
        .bind(ranked.len() as i32)
        .execute(&mut *tx)
        .await?;
        // Playing a quiz restarts its 3-week retention clock (FR-QZ-18).
        sqlx::query(
            "UPDATE quizzes SET last_used_at = now() \
             WHERE id = (SELECT quiz_id FROM game_sessions WHERE id = $1)",
        )
        .bind(parse_uuid(&meta.session)?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Batched pushes

    fn schedule_lobby(&self, pin: &str) {
        if !self.lobby_pending.lock().unwrap().insert(pin.to_string()) {
            return;
        }
        let pin = pin.to_string();
        let store = self.store.clone();
        let hub = self.hub.clone();
        let pending = self.lobby_pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(BATCH_INTERVAL).await;
            pending.lock().unwrap().remove(&pin);
            if let Err(e) = push_lobby(&store, &hub, &pin).await {
                tracing::warn!(error = %e, pin, "lobby update failed");
            }
        });
    }

    async fn push_lobby_now(&self, pin: &str) -> Result<()> {
        push_lobby(&self.store, &self.hub, pin).await
    }

    fn schedule_progress(&self, pin: &str) {
        if !self
            .progress_pending
            .lock()
            .unwrap()
            .insert(pin.to_string())
        {
            return;
        }
        let pin = pin.to_string();
        let store = self.store.clone();
        let hub = self.hub.clone();
        let pending = self.progress_pending.clone();
        tokio::spawn(async move {
            tokio::time::sleep(BATCH_INTERVAL).await;
            pending.lock().unwrap().remove(&pin);
            let push = async {
                let meta = store.meta(&pin).await?.context("no such game")?;
                if !matches!(meta.state, GameState::Question { .. }) {
                    return Ok(());
                }
                let connected = store.connected(&pin).await?;
                hub.to_host(
                    &pin,
                    &ServerPush::QuestionProgress(QuestionProgress {
                        answered: meta.answered,
                        connected,
                    }),
                )
                .await
            };
            if let Err(e) = push.await {
                tracing::warn!(error = %e, pin, "progress update failed");
            }
        });
    }
}

async fn push_lobby(store: &LiveStore, hub: &Hub, pin: &str) -> Result<()> {
    let Some(meta) = store.meta(pin).await? else {
        return Ok(());
    };
    let players = store.players(pin).await?;
    let list = lobby_list(&players);
    hub.to_host(
        pin,
        &ServerPush::LobbyUpdate(LobbyUpdate {
            count: list.len(),
            players: list,
            locked: meta.locked,
        }),
    )
    .await
}

fn lobby_list(players: &[(String, PlayerRecord)]) -> Vec<LobbyPlayer> {
    players
        .iter()
        .map(|(id, p)| LobbyPlayer {
            id: id.clone(),
            nickname: p.nickname.clone(),
            avatar: avatar_of(id, p),
            muted: p.muted,
        })
        .collect()
}

/// The stored avatar, or a stable stand-in for players who joined before
/// avatars existed.
fn avatar_of(id: &str, p: &PlayerRecord) -> String {
    if p.avatar.is_empty() {
        pick_avatar(None, p.join_order.wrapping_add(id.len() as u32))
    } else {
        p.avatar.clone()
    }
}

/// Ranks players: score, then total time, then join order (FR-SC-03).
fn ranked(players: &[(String, PlayerRecord)]) -> Vec<Ranked<(String, String, String)>> {
    rank(
        players
            .iter()
            .map(|(id, p)| Standing {
                player: (id.clone(), p.nickname.clone(), avatar_of(id, p)),
                score: p.score,
                total_time_ms: p.time_ms,
                join_order: p.join_order,
            })
            .collect(),
    )
}

fn rows(ranked: &[Ranked<(String, String, String)>]) -> Vec<ScoreRow> {
    ranked
        .iter()
        .map(|r| ScoreRow {
            nickname: r.standing.player.1.clone(),
            avatar: r.standing.player.2.clone(),
            score: r.standing.score,
        })
        .collect()
}

/// Invisible characters that could hide a word from the filter or flip the
/// text's direction on screen.
fn is_hidden(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}')
}

/// A chat text as shown: invisible characters dropped, spaces collapsed,
/// at most [`CHAT_MAX_CHARS`], and through the nickname word filter.
pub fn clean_chat(raw: &str) -> Result<String, Code> {
    let visible: String = raw.chars().filter(|c| !is_hidden(*c)).collect();
    let text = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    let len = text.chars().count();
    if len == 0 || len > CHAT_MAX_CHARS || text.chars().any(char::is_control) {
        return Err("INVALID");
    }
    if myquiz_engine::profanity::is_offensive(&text) {
        return Err("CHAT_BLOCKED");
    }
    Ok(text)
}

fn refusal_code(r: JoinRefusal) -> Code {
    match r {
        JoinRefusal::PinInvalid => "PIN_INVALID",
        JoinRefusal::Locked => "LOCKED",
        JoinRefusal::Banned => "BANNED",
        JoinRefusal::Full => "FULL",
        JoinRefusal::NickTaken => "NICK_TAKEN",
        JoinRefusal::RejoinWait { .. } => "REJOIN_WAIT",
    }
}

/// Marks a session ended whose live state is gone: totals from its saved
/// answers; the report ranks by them.
async fn close_session(db: &PgPool, session: Uuid) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "UPDATE session_players p SET final_score = \
           (SELECT COALESCE(sum(a.points), 0) FROM answers a WHERE a.player_id = p.id) \
         WHERE p.session_id = $1 AND p.final_score IS NULL",
    )
    .bind(session)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE game_sessions SET status = 'ended', ended_at = COALESCE(ended_at, now()), \
           pin = NULL, \
           player_count = (SELECT count(*) FROM session_players \
                           WHERE session_id = $1 AND NOT kicked) \
         WHERE id = $1 AND status <> 'ended'",
    )
    .bind(session)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).with_context(|| format!("not a uuid: {s}"))
}

#[cfg(test)]
mod chat_tests {
    use super::clean_chat;

    #[test]
    fn chat_text_is_cleaned_and_filtered() {
        assert_eq!(clean_chat("  hay   quá!  "), Ok("hay quá!".into()));
        assert_eq!(clean_chat("   "), Err("INVALID"));
        assert_eq!(clean_chat(&"a".repeat(61)), Err("INVALID"));
        assert!(clean_chat(&"ă".repeat(60)).is_ok());
        assert_eq!(clean_chat("bell\u{7}"), Err("INVALID"));
        assert_eq!(clean_chat("đm"), Err("CHAT_BLOCKED"));
        // Zero-width characters cannot hide a word from the filter.
        assert_eq!(clean_chat("f\u{200B}uck"), Err("CHAT_BLOCKED"));
        assert_eq!(clean_chat("\u{202E}abc"), Ok("abc".into()));
    }
}
