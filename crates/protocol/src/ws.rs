//! WebSocket messages (design section 7).
//!
//! Every frame is a JSON envelope. From the client:
//! `{ "t": "answer:submit", "id": 17, "d": { ... } }`, where `id` is optional
//! and asks for an ack. Back from the server:
//! `{ "t": "ack", "id": 17, "ok": true, "d": { ... } }` or
//! `{ "t": "ack", "id": 17, "ok": false, "code": "LATE" }`.
//! Pushes use `{ "t": "question:start", "d": { ... } }`.

use myquiz_engine::reveal::{OptionCount, TextCount};
use myquiz_engine::view::PublicQuestion;
use myquiz_engine::{GameState, OptionId, Submission};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The looks a host can give a game. Both use the same palette; each draws
/// the avatars in its own style.
pub const THEMES: &[&str] = &["cute", "anime"];

/// The player avatars, by id. The web client draws them
/// (`web/src/game/avatars.tsx`) and must list the same ids; each theme draws
/// its own character for an id.
pub const AVATARS: &[&str] = &[
    "bunny", "kitty", "froggy", "ducky", "bear", "fox", "panda", "chick", "piggy", "koala",
    "mouse", "seal", "owl", "penguin", "lion", "tiger", "sheep", "cow", "hedgehog", "unicorn",
];

/// The avatar to show: the one asked for when it is known, otherwise one
/// picked from `seed` so a player keeps the same face all game.
pub fn pick_avatar(asked: Option<&str>, seed: u32) -> String {
    match asked {
        Some(a) if AVATARS.contains(&a) => a.to_string(),
        _ => AVATARS[seed as usize % AVATARS.len()].to_string(),
    }
}

/// Who may chat (players' popups on the host screen), set by the host:
/// everyone, quick emoji only, or nobody.
pub const CHAT_MODES: &[&str] = &["open", "emoji", "off"];

/// The quick reactions players can send, by id. The web client draws them
/// (`web/src/game/chat.ts`) and must list the same ids. Ids rather than raw
/// emoji keep the host screen to a known, classroom-safe set.
pub const CHAT_EMOJIS: &[&str] = &[
    "thumbs", "heart", "laugh", "wow", "party", "fire", "clap", "think",
];

/// Longest chat text, in characters.
pub const CHAT_MAX_CHARS: usize = 60;

macro_rules! wire {
    ($($item:item)*) => {
        $(
            #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
            #[serde(rename_all = "camelCase")]
            #[ts(export, export_to = "../../../web/src/generated/")]
            $item
        )*
    };
}

/// A client frame: the optional request id plus the message.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Inbound {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub msg: ClientMessage,
}

/// Everything a client may send.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "t", content = "d")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum ClientMessage {
    #[serde(rename = "host:attach")]
    HostAttach(HostAttach),
    #[serde(rename = "host:lock")]
    HostLock(HostLock),
    #[serde(rename = "host:theme")]
    HostTheme(HostTheme),
    #[serde(rename = "host:kick")]
    HostKick(HostKick),
    #[serde(rename = "host:rejoin")]
    HostRejoin(HostRejoin),
    #[serde(rename = "host:start")]
    HostStart(Empty),
    #[serde(rename = "host:next")]
    HostNext(HostNext),
    #[serde(rename = "host:skip")]
    HostSkip(HostSkip),
    #[serde(rename = "host:end")]
    HostEnd(Empty),
    #[serde(rename = "host:chat")]
    HostChat(HostChat),
    #[serde(rename = "player:join")]
    PlayerJoin(PlayerJoin),
    #[serde(rename = "player:resume")]
    PlayerResume(PlayerResume),
    #[serde(rename = "answer:submit")]
    AnswerSubmit(AnswerSubmit),
    #[serde(rename = "player:chat")]
    PlayerChat(PlayerChat),
    #[serde(rename = "time:ping")]
    TimePing(TimePing),
}

/// A payload with no fields, `{}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Empty {}

wire! {
    pub struct HostAttach { pub pin: String }
    pub struct HostLock { pub locked: bool }
    /// The game's look, one of [`THEMES`]; only while in the lobby.
    pub struct HostTheme { pub theme: String }
    pub struct HostKick { pub player_id: String }
    /// Lets a player back in under their name from a new device.
    pub struct HostRejoin { pub player_id: String }
    pub struct HostNext { pub expect: GameState }
    pub struct HostSkip { pub index: usize }
    /// Who may chat, one of [`CHAT_MODES`]; any time during the game.
    pub struct HostChat { pub mode: String }

    pub struct PlayerJoin {
        pub pin: String,
        pub nickname: String,
        pub device_id: String,
        /// One of [`AVATARS`]. Older clients send none and get one assigned.
        #[serde(default)]
        #[ts(optional)]
        pub avatar: Option<String>,
    }
    pub struct PlayerResume { pub pin: String, pub token: String }

    /// A chat popup for the host screen: either a short text (at most
    /// [`CHAT_MAX_CHARS`], not while a question runs) or one of
    /// [`CHAT_EMOJIS`].
    pub struct PlayerChat {
        #[serde(default)]
        #[ts(optional)]
        pub text: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        pub emoji: Option<String>,
    }

    pub struct AnswerSubmit {
        pub index: usize,
        #[serde(flatten)]
        pub submission: Submission,
    }

    pub struct TimePing { pub client_sent_at: f64 }

    // ---- ack payloads ----

    pub struct TimePong { pub client_sent_at: f64, #[ts(type = "number")] pub server_now: u64 }
    pub struct AnswerAccepted { pub accepted: bool }

    /// Sent on host:attach, player:join and player:resume so a (re)connecting
    /// screen can draw itself without waiting for the next push.
    pub struct GameSnapshot {
        pub pin: String,
        pub state: GameState,
        pub locked: bool,
        /// The look the host picked, one of [`THEMES`].
        pub theme: String,
        /// Who may chat, one of [`CHAT_MODES`].
        pub chat: String,
        pub players: Vec<LobbyPlayer>,
        /// Set while a question is running.
        pub question: Option<QuestionStart>,
        /// For players: their own standing.
        pub me: Option<MyStanding>,
        /// For the host: the game's id, which names its report
        /// (`/reports/<id>`). Null for players.
        pub report_id: Option<String>,
        /// While answers are revealed, for the host: the correct answer and
        /// how players answered. Null otherwise.
        pub reveal: Option<QuestionReveal>,
        /// While answers are revealed, for a player: their own result.
        pub result: Option<QuestionResult>,
        /// On the leaderboard and podium: the rows on screen.
        pub top: Option<Vec<ScoreRow>>,
    }

    pub struct PlayerJoined { pub player_id: String, pub token: String, pub snapshot: GameSnapshot }

    pub struct MyStanding {
        pub player_id: String,
        pub nickname: String,
        pub avatar: String,
        #[ts(type = "number")]
        pub score: u64,
        pub rank: u32,
        /// Whether this player already answered the running question.
        pub answered: bool,
    }

    // ---- server pushes ----

    pub struct LobbyPlayer { pub id: String, pub nickname: String, pub avatar: String }
    pub struct LobbyUpdate { pub players: Vec<LobbyPlayer>, pub count: usize, pub locked: bool }
    pub struct LobbyJoined { pub nickname: String, pub player_count: usize }

    pub struct QuestionStart {
        #[serde(flatten)]
        pub question: PublicQuestion,
        /// Server clock, ms since the Unix epoch: when options show.
        #[ts(type = "number")]
        pub starts_at: u64,
        /// Server clock: when answers stop being accepted.
        #[ts(type = "number")]
        pub ends_at: u64,
    }

    pub struct QuestionProgress { pub answered: u32, pub connected: u32 }

    pub struct QuestionReveal {
        pub index: usize,
        pub correct_option_ids: Vec<OptionId>,
        pub accepted_answers: Vec<String>,
        pub distribution: Vec<OptionCount>,
        pub top_text_answers: Vec<TextCount>,
    }

    pub struct QuestionResult {
        pub index: usize,
        /// False when the player sent nothing before time ran out.
        pub answered: bool,
        pub correct: bool,
        pub points: u32,
        #[ts(type = "number")]
        pub total: u64,
        pub rank: u32,
        pub correct_option_ids: Vec<OptionId>,
    }

    pub struct ScoreRow { pub nickname: String, pub avatar: String, #[ts(type = "number")] pub score: u64 }
    pub struct Leaderboard { pub top: Vec<ScoreRow> }

    pub struct GameEnded { pub reason: EndReason }
    pub struct GameTheme { pub theme: String }
    /// The host changed who may chat, one of [`CHAT_MODES`].
    pub struct GameChat { pub mode: String }
    /// A player's chat popup, to the host only.
    pub struct ChatMessage {
        pub player_id: String,
        pub nickname: String,
        pub avatar: String,
        pub text: Option<String>,
        pub emoji: Option<String>,
    }
    /// To the host: someone on a new device asks to continue as this
    /// (offline) player. Sent again on each of their retries.
    pub struct RejoinRequest { pub player_id: String, pub nickname: String, pub avatar: String }
    pub struct ErrorPush { pub code: String }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum EndReason {
    Finished,
    HostEnded,
    HostTimeout,
    Suspended,
}

/// Everything the server pushes, with its event name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "t", content = "d")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum ServerPush {
    #[serde(rename = "lobby:update")]
    LobbyUpdate(LobbyUpdate),
    #[serde(rename = "lobby:joined")]
    LobbyJoined(LobbyJoined),
    #[serde(rename = "player:kicked")]
    PlayerKicked(Empty),
    #[serde(rename = "question:start")]
    QuestionStart(QuestionStart),
    #[serde(rename = "question:progress")]
    QuestionProgress(QuestionProgress),
    #[serde(rename = "question:reveal")]
    QuestionReveal(QuestionReveal),
    #[serde(rename = "question:result")]
    QuestionResult(QuestionResult),
    #[serde(rename = "leaderboard:show")]
    LeaderboardShow(Leaderboard),
    #[serde(rename = "game:podium")]
    GamePodium(Leaderboard),
    #[serde(rename = "game:ended")]
    GameEnded(GameEnded),
    #[serde(rename = "game:theme")]
    GameTheme(GameTheme),
    #[serde(rename = "game:chat")]
    GameChat(GameChat),
    #[serde(rename = "chat:message")]
    ChatMessage(ChatMessage),
    #[serde(rename = "rejoin:request")]
    RejoinRequest(RejoinRequest),
    #[serde(rename = "error")]
    Error(ErrorPush),
}

/// The ack frame for a request that carried an `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Ack {
    /// Always "ack".
    pub t: String,
    #[ts(type = "number")]
    pub id: u64,
    pub ok: bool,
    #[serde(default)]
    #[ts(type = "unknown")]
    pub d: Option<serde_json::Value>,
    #[serde(default)]
    pub code: Option<String>,
}

impl Ack {
    pub fn ok(id: u64, d: impl Serialize) -> Self {
        Ack {
            t: "ack".into(),
            id,
            ok: true,
            d: Some(serde_json::to_value(d).expect("serialisable ack")),
            code: None,
        }
    }

    pub fn err(id: u64, code: impl Into<String>) -> Self {
        Ack {
            t: "ack".into(),
            id,
            ok: false,
            d: None,
            code: Some(code.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_client_frames() {
        let f: Inbound = serde_json::from_str(
            r#"{"t":"answer:submit","id":17,"d":{"index":2,"optionIds":[1,3]}}"#,
        )
        .unwrap();
        assert_eq!(f.id, Some(17));
        let ClientMessage::AnswerSubmit(a) = f.msg else {
            panic!()
        };
        assert_eq!(a.index, 2);
        assert_eq!(a.submission.option_ids, vec![1, 3]);

        let f: Inbound = serde_json::from_str(r#"{"t":"host:start","d":{}}"#).unwrap();
        assert_eq!(f.id, None);
        assert_eq!(f.msg, ClientMessage::HostStart(Empty {}));

        let f: Inbound = serde_json::from_str(
            r#"{"t":"host:next","id":1,"d":{"expect":{"state":"reveal","index":0}}}"#,
        )
        .unwrap();
        assert_eq!(
            f.msg,
            ClientMessage::HostNext(HostNext {
                expect: GameState::Reveal { index: 0 }
            })
        );
    }

    #[test]
    fn parses_chat_frames() {
        let f: Inbound =
            serde_json::from_str(r#"{"t":"player:chat","id":3,"d":{"emoji":"clap"}}"#).unwrap();
        assert_eq!(
            f.msg,
            ClientMessage::PlayerChat(PlayerChat {
                text: None,
                emoji: Some("clap".into())
            })
        );
        let f: Inbound = serde_json::from_str(r#"{"t":"host:chat","d":{"mode":"emoji"}}"#).unwrap();
        assert_eq!(
            f.msg,
            ClientMessage::HostChat(HostChat {
                mode: "emoji".into()
            })
        );
    }

    #[test]
    fn rejects_unknown_events() {
        assert!(serde_json::from_str::<Inbound>(r#"{"t":"host:hack","d":{}}"#).is_err());
        assert!(serde_json::from_str::<Inbound>(r#"{"t":"player:join","d":{}}"#).is_err());
    }

    #[test]
    fn push_and_ack_shapes() {
        let p = ServerPush::QuestionProgress(QuestionProgress {
            answered: 3,
            connected: 10,
        });
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"t":"question:progress","d":{"answered":3,"connected":10}}"#
        );
        assert_eq!(
            serde_json::to_string(&Ack::err(4, "LATE")).unwrap(),
            r#"{"t":"ack","id":4,"ok":false,"d":null,"code":"LATE"}"#
        );
        assert_eq!(
            serde_json::to_string(&Ack::ok(5, AnswerAccepted { accepted: true })).unwrap(),
            r#"{"t":"ack","id":5,"ok":true,"d":{"accepted":true},"code":null}"#
        );
    }
}
