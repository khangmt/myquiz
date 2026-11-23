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
use crate::live::{secrets, JoinRefusal, LiveStore, NewPlayer, PlayerRecord, Transition};

/// Lobby and progress pushes are batched to at most one per this interval
/// per game and server (design section 7).
const BATCH_INTERVAL: Duration = Duration::from_millis(250);

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
                        score: r.standing.score,
                        rank: r.rank,
                        answered: match meta.state {
                            GameState::Question { index } => {
                                self.store.has_answered(pin, index, id).await?
                            }
                            _ => false,
                        },
                    }),
                    None => None,
                }
            }
            None => None,
        };
        Ok(GameSnapshot {
            pin: pin.to_string(),
            state: meta.state,
            locked: meta.locked,
            players: lobby_list(&players),
            question,
            me,
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

    pub async fn lock(&self, pin: &str, locked: bool) -> Result<()> {
        self.store.set_locked(pin, locked).await?;
        self.push_lobby_now(pin).await
    }

    pub async fn kick(&self, pin: &str, player_id: &str) -> Result<Result<(), Code>> {
        if !self.store.kick(pin, player_id).await? {
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
                    text: encode(&ServerPush::PlayerKicked(Empty {})),
                },
            )
            .await?;
        self.push_lobby_now(pin).await?;
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
        if !(NICK_MIN..=NICK_MAX).contains(&len) {
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
                    device_id: req.device_id.clone(),
                    user_id: user_id.clone(),
                    token_hash: secrets::hash_token(&token),
                },
            )
            .await?;
        let join_order = match joined {
            Ok(o) => o,
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
            let _ = self.store.kick(&req.pin, &player_id).await;
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
            Ok(acc) => {
                self.schedule_progress(pin);
                let connected = self.store.connected(pin).await?;
                if connected > 0 && acc.answered >= connected {
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

    async fn maybe_reveal_early(&self, pin: &str) -> Result<()> {
        let Some(meta) = self.store.meta(pin).await? else {
            return Ok(());
        };
        if let GameState::Question { index } = meta.state {
            let connected = self.store.connected(pin).await?;
            if connected > 0 && meta.answered >= connected {
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
            "UPDATE game_sessions SET status = 'ended', ended_at = now(), pin = NULL, \
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

        self.hub
            .to_all(pin, &ServerPush::GameEnded(GameEnded { reason }))
            .await?;
        self.forget(pin);
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
        })
        .collect()
}

/// Ranks players: score, then total time, then join order (FR-SC-03).
fn ranked(players: &[(String, PlayerRecord)]) -> Vec<Ranked<(String, String)>> {
    rank(
        players
            .iter()
            .map(|(id, p)| Standing {
                player: (id.clone(), p.nickname.clone()),
                score: p.score,
                total_time_ms: p.time_ms,
                join_order: p.join_order,
            })
            .collect(),
    )
}

fn rows(ranked: &[Ranked<(String, String)>]) -> Vec<ScoreRow> {
    ranked
        .iter()
        .map(|r| ScoreRow {
            nickname: r.standing.player.1.clone(),
            score: r.standing.score,
        })
        .collect()
}

fn refusal_code(r: JoinRefusal) -> Code {
    match r {
        JoinRefusal::PinInvalid => "PIN_INVALID",
        JoinRefusal::Locked => "LOCKED",
        JoinRefusal::Kicked => "KICKED",
        JoinRefusal::Full => "FULL",
        JoinRefusal::NickTaken => "NICK_TAKEN",
    }
}

fn parse_uuid(s: &str) -> Result<Uuid> {
    Uuid::parse_str(s).with_context(|| format!("not a uuid: {s}"))
}
