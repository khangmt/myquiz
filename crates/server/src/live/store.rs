use std::sync::LazyLock;

use anyhow::{anyhow, bail, Context, Result};
use fred::prelude::*;
use fred::types::scripts::Script;
use fred::types::Value;
use myquiz_engine::answer::{Submission, SubmitError};
use myquiz_engine::reveal::StoredAnswer;
use myquiz_engine::{Command, GameState, QuizSnapshot};
use serde::{Deserialize, Serialize};

use super::keys;

/// Keys of a game live 24 h after it ends, and at most 24 h before.
pub const GAME_TTL_S: u64 = 24 * 60 * 60;
/// The "get ready" countdown before options show (FR-GM-05, NFR-02).
#[allow(dead_code)] // default for LiveStore::new
pub const COUNTDOWN_MS: u64 = 3_000;
pub const MAX_PLAYERS: u32 = 200;

macro_rules! script {
    ($name:literal) => {
        LazyLock::new(|| {
            Script::from_lua(concat!(
                include_str!("lua/now.lua"),
                "\n",
                include_str!(concat!("lua/", $name, ".lua"))
            ))
        })
    };
}

#[allow(dead_code)] // POST /games, with the REST API
static CREATE: LazyLock<Script> = script!("create");
static JOIN: LazyLock<Script> = script!("join");
static KICK: LazyLock<Script> = script!("kick");
static TRANSITION: LazyLock<Script> = script!("transition");
static REVEAL: LazyLock<Script> = script!("reveal");
static ANSWER: LazyLock<Script> = script!("answer");
static DUE: LazyLock<Script> = script!("due");
static SCORES: LazyLock<Script> = script!("scores");
static FINISH: LazyLock<Script> = script!("finish");
static CLOCK: LazyLock<Script> = script!("clock");
static CHAT: LazyLock<Script> = script!("chat");

/// A player as stored in `game:<pin>:players`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerRecord {
    pub nickname: String,
    /// The player's avatar id; empty for records written before avatars.
    #[serde(default)]
    pub avatar: String,
    /// Normalised nickname used for uniqueness.
    pub nick_key: String,
    pub device_id: String,
    /// Set when the player is signed in (FR-RP-08).
    pub user_id: Option<String>,
    #[serde(default)]
    pub score: u64,
    #[serde(default)]
    pub time_ms: u64,
    #[serde(default)]
    pub join_order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlayer {
    pub nickname: String,
    pub avatar: String,
    pub device_id: String,
    pub user_id: Option<String>,
    pub token_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum JoinRefusal {
    PinInvalid,
    Locked,
    Kicked,
    Full,
    NickTaken,
}

/// The answer of [`LiveStore::chat_slot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSlot {
    Granted,
    /// This player sent one too recently.
    TooSoon,
    /// The game as a whole is over its rate.
    GameBusy,
}

/// The game hash, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMeta {
    pub state: GameState,
    pub session: String,
    pub host: String,
    pub total: usize,
    pub locked: bool,
    /// One of `ws::THEMES`; "cute" until the host picks another.
    pub theme: String,
    /// One of `ws::CHAT_MODES`; "open" until the host changes it.
    pub chat: String,
    pub starts_at_ms: u64,
    pub ends_at_ms: u64,
    pub answered: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Applied. For a new question, when its options show and when it ends
    /// (Redis server clock, ms).
    Applied { starts_at_ms: u64, ends_at_ms: u64 },
    /// The game had already moved on; nothing was written.
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accepted {
    pub response_ms: u64,
    /// Answers received so far for this question.
    pub answered: u32,
}

/// Uniqueness key for nicknames: "An", "an" and " AN " collide; "Ân" does not.
pub fn nick_key(nickname: &str) -> String {
    myquiz_engine::matching::normalize(nickname, false)
}

fn state_parts(s: GameState) -> (&'static str, String) {
    match s {
        GameState::Lobby => ("lobby", String::new()),
        GameState::Question { index } => ("question", index.to_string()),
        GameState::Reveal { index } => ("reveal", index.to_string()),
        GameState::Leaderboard { index } => ("leaderboard", index.to_string()),
        GameState::Podium => ("podium", String::new()),
        GameState::Ended => ("ended", String::new()),
    }
}

fn parse_state(name: &str, index: &str) -> Result<GameState> {
    let idx = || index.parse::<usize>().context("bad question index");
    Ok(match name {
        "lobby" => GameState::Lobby,
        "question" => GameState::Question { index: idx()? },
        "reveal" => GameState::Reveal { index: idx()? },
        "leaderboard" => GameState::Leaderboard { index: idx()? },
        "podium" => GameState::Podium,
        "ended" => GameState::Ended,
        other => bail!("unknown game state {other:?}"),
    })
}

fn array(v: Value) -> Result<Vec<Value>> {
    match v {
        Value::Array(a) => Ok(a),
        other => Err(anyhow!("expected an array from Redis, got {other:?}")),
    }
}

fn int(v: &Value) -> Result<i64> {
    v.as_i64()
        .ok_or_else(|| anyhow!("expected an integer from Redis, got {v:?}"))
}

fn text(v: &Value) -> Result<String> {
    v.as_string()
        .ok_or_else(|| anyhow!("expected a string from Redis, got {v:?}"))
}

/// Live game state for one Redis (or Redis pool).
#[derive(Clone)]
pub struct LiveStore {
    redis: Pool,
    countdown_ms: u64,
}

impl LiveStore {
    #[allow(dead_code)]
    pub fn new(redis: Pool) -> Self {
        Self::with_countdown(redis, COUNTDOWN_MS)
    }

    /// Tests shorten the "get ready" countdown.
    pub fn with_countdown(redis: Pool, countdown_ms: u64) -> Self {
        Self {
            redis,
            countdown_ms,
        }
    }

    #[allow(dead_code)]
    pub fn redis(&self) -> &Pool {
        &self.redis
    }

    /// The Redis server clock, which every deadline uses (ms since epoch).
    pub async fn now_ms(&self) -> Result<u64> {
        Ok(int(&self.run(&CLOCK, vec![], vec![]).await?)? as u64)
    }

    /// Counts a socket opening (+1) or closing (-1) for a player.
    pub async fn set_online(&self, pin: &str, player_id: &str, delta: i64) -> Result<()> {
        let n: i64 = self
            .redis
            .hincrby(keys::online(pin), player_id, delta)
            .await?;
        if n <= 0 {
            let _: i64 = self.redis.hdel(keys::online(pin), player_id).await?;
        } else {
            let _: bool = self
                .redis
                .expire(keys::online(pin), GAME_TTL_S as i64, None)
                .await?;
        }
        Ok(())
    }

    /// Players with at least one open socket.
    pub async fn connected(&self, pin: &str) -> Result<u32> {
        let vals: Vec<i64> = self.redis.hvals(keys::online(pin)).await?;
        Ok(vals.into_iter().filter(|v| *v > 0).count() as u32)
    }

    /// Whether a player has an answer stored for question `index`.
    pub async fn has_answered(&self, pin: &str, index: usize, player_id: &str) -> Result<bool> {
        Ok(self
            .redis
            .hexists(keys::answers(pin, index), player_id)
            .await?)
    }

    /// The user id behind a login session cookie, if the session exists.
    pub async fn login_user(&self, sid: &str) -> Result<Option<String>> {
        Ok(self.redis.hget(keys::login_session(sid), "user_id").await?)
    }

    async fn run(&self, script: &Script, keys: Vec<String>, args: Vec<String>) -> Result<Value> {
        Ok(script
            .evalsha_with_reload::<Value, _, _>(self.redis.next(), keys, args)
            .await?)
    }

    /// Claims a fresh PIN and creates the game in the lobby (FR-GM-01).
    /// Draws again on collision; returns the PIN.
    #[allow(dead_code)] // POST /games, with the REST API
    pub async fn create_game(
        &self,
        session: &str,
        host: &str,
        quiz: &QuizSnapshot,
    ) -> Result<String> {
        let snapshot = serde_json::to_string(quiz)?;
        for _ in 0..20 {
            let pin = super::secrets::new_pin();
            let created = self
                .run(
                    &CREATE,
                    vec![keys::pin(&pin), keys::game(&pin), keys::quiz(&pin)],
                    vec![
                        session.into(),
                        host.into(),
                        quiz.total().to_string(),
                        snapshot.clone(),
                        GAME_TTL_S.to_string(),
                    ],
                )
                .await?;
            if int(&created)? == 1 {
                return Ok(pin);
            }
        }
        bail!("could not find a free PIN after 20 tries")
    }

    pub async fn meta(&self, pin: &str) -> Result<Option<GameMeta>> {
        let h: std::collections::HashMap<String, String> =
            self.redis.hgetall(keys::game(pin)).await?;
        if h.is_empty() {
            return Ok(None);
        }
        let get = |k: &str| h.get(k).map(String::as_str).unwrap_or("");
        let num = |k: &str| get(k).parse::<u64>().unwrap_or(0);
        Ok(Some(GameMeta {
            state: parse_state(get("state"), get("index"))?,
            session: get("session").into(),
            host: get("host").into(),
            total: num("total") as usize,
            locked: get("locked") == "1",
            theme: match get("theme") {
                "" => "cute".into(),
                t => t.into(),
            },
            chat: match get("chat") {
                "" => "open".into(),
                c => c.into(),
            },
            starts_at_ms: num("startsAt"),
            ends_at_ms: num("endsAt"),
            answered: num("answered") as u32,
        }))
    }

    pub async fn quiz(&self, pin: &str) -> Result<Option<QuizSnapshot>> {
        let raw: Option<String> = self.redis.get(keys::quiz(pin)).await?;
        raw.map(|s| serde_json::from_str(&s).context("bad quiz snapshot"))
            .transpose()
    }

    pub async fn players(&self, pin: &str) -> Result<Vec<(String, PlayerRecord)>> {
        let h: std::collections::HashMap<String, String> =
            self.redis.hgetall(keys::players(pin)).await?;
        let mut players = h
            .into_iter()
            .map(|(id, raw)| Ok((id, serde_json::from_str(&raw)?)))
            .collect::<Result<Vec<(String, PlayerRecord)>>>()?;
        players.sort_by_key(|(_, p)| p.join_order);
        Ok(players)
    }

    /// The player a token belongs to, for `player:resume` (FR-GM-09).
    pub async fn player_for_token(&self, pin: &str, token_hash: &str) -> Result<Option<String>> {
        Ok(self.redis.hget(keys::tokens(pin), token_hash).await?)
    }

    /// Adds a player; returns their join order (FR-GM-02).
    pub async fn join(
        &self,
        pin: &str,
        player_id: &str,
        p: &NewPlayer,
    ) -> Result<std::result::Result<u32, JoinRefusal>> {
        let record = PlayerRecord {
            nickname: p.nickname.clone(),
            avatar: p.avatar.clone(),
            nick_key: nick_key(&p.nickname),
            device_id: p.device_id.clone(),
            user_id: p.user_id.clone(),
            score: 0,
            time_ms: 0,
            join_order: 0,
        };
        let out = array(
            self.run(
                &JOIN,
                vec![
                    keys::game(pin),
                    keys::players(pin),
                    keys::tokens(pin),
                    keys::nicks(pin),
                    keys::kicked(pin),
                ],
                vec![
                    player_id.into(),
                    serde_json::to_string(&record)?,
                    record.nick_key.clone(),
                    p.token_hash.clone(),
                    p.device_id.clone(),
                    MAX_PLAYERS.to_string(),
                ],
            )
            .await?,
        )?;
        Ok(match text(&out[0])?.as_str() {
            "OK" => Ok(int(&out[1])? as u32),
            "PIN_INVALID" => Err(JoinRefusal::PinInvalid),
            "LOCKED" => Err(JoinRefusal::Locked),
            "KICKED" => Err(JoinRefusal::Kicked),
            "FULL" => Err(JoinRefusal::Full),
            "NICK_TAKEN" => Err(JoinRefusal::NickTaken),
            other => bail!("unexpected join result {other}"),
        })
    }

    /// Removes a player and bars their device from this game (FR-GM-03).
    pub async fn kick(&self, pin: &str, player_id: &str) -> Result<bool> {
        let out = self
            .run(
                &KICK,
                vec![
                    keys::players(pin),
                    keys::tokens(pin),
                    keys::nicks(pin),
                    keys::kicked(pin),
                ],
                vec![player_id.into()],
            )
            .await?;
        Ok(int(&out)? == 1)
    }

    /// Opens or closes the lobby to new players (FR-GM-04).
    pub async fn set_locked(&self, pin: &str, locked: bool) -> Result<()> {
        let _: i64 = self
            .redis
            .hset(keys::game(pin), ("locked", if locked { "1" } else { "0" }))
            .await?;
        Ok(())
    }

    /// Sets the game's look.
    pub async fn set_theme(&self, pin: &str, theme: &str) -> Result<()> {
        let _: i64 = self.redis.hset(keys::game(pin), ("theme", theme)).await?;
        Ok(())
    }

    /// Sets who may chat: everyone, emoji only, or nobody.
    pub async fn set_chat(&self, pin: &str, mode: &str) -> Result<()> {
        let _: i64 = self.redis.hset(keys::game(pin), ("chat", mode)).await?;
        Ok(())
    }

    /// One player, if they are (still) in the game.
    pub async fn player(&self, pin: &str, player_id: &str) -> Result<Option<PlayerRecord>> {
        let raw: Option<String> = self.redis.hget(keys::players(pin), player_id).await?;
        raw.map(|s| serde_json::from_str(&s).context("bad player record"))
            .transpose()
    }

    /// Takes a chat slot: the player's cooldown for this kind of message and
    /// the game's messages-per-second cap (see `lua/chat.lua`).
    pub async fn chat_slot(
        &self,
        pin: &str,
        player_id: &str,
        kind: &str,
        cooldown_ms: u64,
        per_game_per_sec: u32,
    ) -> Result<ChatSlot> {
        let out = self
            .run(
                &CHAT,
                vec![
                    keys::chat_cooldown(pin, player_id, kind),
                    keys::chat_rate(pin),
                ],
                vec![cooldown_ms.to_string(), per_game_per_sec.to_string()],
            )
            .await?;
        Ok(match int(&out)? {
            1 => ChatSlot::Granted,
            0 => ChatSlot::TooSoon,
            _ => ChatSlot::GameBusy,
        })
    }

    /// Writes `to` if the game is still in `from` (compare-and-set). For a
    /// new question, `time_limit_ms` sets its deadline.
    pub async fn transition(
        &self,
        pin: &str,
        from: GameState,
        to: GameState,
        time_limit_ms: u64,
    ) -> Result<Transition> {
        let (from_s, from_i) = state_parts(from);
        let (to_s, to_i) = state_parts(to);
        let out = array(
            self.run(
                &TRANSITION,
                vec![keys::game(pin), keys::DEADLINES.into()],
                vec![
                    pin.into(),
                    from_s.into(),
                    from_i,
                    to_s.into(),
                    to_i,
                    self.countdown_ms.to_string(),
                    time_limit_ms.to_string(),
                ],
            )
            .await?,
        )?;
        if int(&out[0])? != 1 {
            return Ok(Transition::Stale);
        }
        Ok(Transition::Applied {
            starts_at_ms: int(&out[1])? as u64,
            ends_at_ms: int(&out[2])? as u64,
        })
    }

    /// Applies a host command or the all-answered trigger through the
    /// engine's state machine. Question → Reveal goes through [`Self::reveal`]
    /// instead, because it must also collect the answers.
    pub async fn command(
        &self,
        pin: &str,
        cmd: Command,
    ) -> Result<std::result::Result<(GameState, Transition), myquiz_engine::TransitionError>> {
        let meta = self.meta(pin).await?.context("no such game")?;
        let to = match meta.state.apply(cmd, meta.total) {
            Ok(to) => to,
            Err(e) => return Ok(Err(e)),
        };
        let limit = match to {
            GameState::Question { index } => self
                .quiz(pin)
                .await?
                .context("quiz snapshot missing")?
                .question(index)
                .context("question index out of range")?
                .time_limit_ms(),
            _ => 0,
        };
        let t = self.transition(pin, meta.state, to, limit).await?;
        Ok(Ok((to, t)))
    }

    /// Stores one answer (FR-GM-06, SEC-16). The caller acks the player only
    /// on `Ok(Ok(_))`.
    pub async fn submit_answer(
        &self,
        pin: &str,
        index: usize,
        player_id: &str,
        submission: &Submission,
    ) -> Result<std::result::Result<Accepted, SubmitError>> {
        let out = array(
            self.run(
                &ANSWER,
                vec![keys::game(pin), keys::answers(pin, index)],
                vec![
                    index.to_string(),
                    player_id.into(),
                    serde_json::to_string(submission)?,
                ],
            )
            .await?,
        )?;
        Ok(match text(&out[0])?.as_str() {
            "OK" => Ok(Accepted {
                response_ms: int(&out[1])? as u64,
                answered: int(&out[2])? as u32,
            }),
            "WRONG_STATE" => Err(SubmitError::WrongState),
            "EARLY" => Err(SubmitError::Early),
            "LATE" => Err(SubmitError::Late),
            "DUPLICATE" => Err(SubmitError::Duplicate),
            other => bail!("unexpected answer result {other}"),
        })
    }

    /// Moves question `index` to Reveal and returns its answers, or `None`
    /// if another caller already did (FR-GM-07).
    pub async fn reveal(
        &self,
        pin: &str,
        index: usize,
    ) -> Result<Option<Vec<StoredAnswer<String>>>> {
        let out = array(
            self.run(
                &REVEAL,
                vec![
                    keys::game(pin),
                    keys::DEADLINES.into(),
                    keys::answers(pin, index),
                ],
                vec![pin.into(), index.to_string()],
            )
            .await?,
        )?;
        if int(&out[0])? != 1 {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct Stored {
            r: u64,
            s: Submission,
        }
        out[1..]
            .chunks(2)
            .map(|pair| {
                let stored: Stored = serde_json::from_str(&text(&pair[1])?)?;
                Ok(StoredAnswer {
                    player: text(&pair[0])?,
                    submission: stored.s,
                    response_ms: stored.r,
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(Some)
    }

    /// The answers stored for question `index`, read only. A screen that
    /// reconnects during the reveal uses them to draw the result again.
    pub async fn answers(&self, pin: &str, index: usize) -> Result<Vec<StoredAnswer<String>>> {
        let pairs: std::collections::HashMap<String, String> =
            self.redis.hgetall(keys::answers(pin, index)).await?;
        #[derive(Deserialize)]
        struct Stored {
            r: u64,
            s: Submission,
        }
        pairs
            .into_iter()
            .map(|(player, json)| {
                let stored: Stored = serde_json::from_str(&json)?;
                Ok(StoredAnswer {
                    player,
                    submission: stored.s,
                    response_ms: stored.r,
                })
            })
            .collect()
    }

    /// PINs whose current question is past its deadline.
    pub async fn due(&self, limit: usize) -> Result<Vec<String>> {
        let out = array(
            self.run(&DUE, vec![keys::DEADLINES.into()], vec![limit.to_string()])
                .await?,
        )?;
        out.iter().map(text).collect()
    }

    /// Adds `(player, points, time charged)` to totals once per question.
    /// Returns false if this question was already scored.
    pub async fn apply_scores(
        &self,
        pin: &str,
        index: usize,
        updates: &[(String, u32, u64)],
    ) -> Result<bool> {
        let mut args = Vec::with_capacity(1 + updates.len() * 3);
        args.push(index.to_string());
        for (player, points, time_ms) in updates {
            args.extend([player.clone(), points.to_string(), time_ms.to_string()]);
        }
        let out = self
            .run(&SCORES, vec![keys::game(pin), keys::players(pin)], args)
            .await?;
        Ok(int(&out)? == 1)
    }

    /// Ends the game, releases its PIN and sets every key to expire in 24 h.
    pub async fn finish(&self, pin: &str) -> Result<bool> {
        let Some(meta) = self.meta(pin).await? else {
            return Ok(false);
        };
        let mut k = vec![
            keys::game(pin),
            keys::DEADLINES.into(),
            keys::pin(pin),
            keys::quiz(pin),
            keys::players(pin),
            keys::tokens(pin),
            keys::nicks(pin),
            keys::kicked(pin),
            keys::online(pin),
        ];
        k.extend((0..meta.total).map(|i| keys::answers(pin, i)));
        let out = self
            .run(
                &FINISH,
                k,
                vec![pin.into(), meta.session, GAME_TTL_S.to_string()],
            )
            .await?;
        Ok(int(&out)? == 1)
    }
}
