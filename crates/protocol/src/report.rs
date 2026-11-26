//! Game reports (FR-RP-01 to 05, FR-RP-09).

use myquiz_engine::QuestionKind;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One row of `GET /sessions?page=` (FR-RP-01).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct SessionSummary {
    pub id: String,
    /// The host's name for this game (e.g. the class), if any.
    pub name: Option<String>,
    pub quiz_title: String,
    /// RFC 3339.
    pub started_at: String,
    /// Null while the game is still running, or if it was abandoned.
    pub ended_at: Option<String>,
    pub ended: bool,
    pub player_count: u32,
    pub average_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 1-based.
    pub page: u32,
    pub page_size: u32,
    #[ts(type = "number")]
    pub total: i64,
}

/// Per-question figures for the summary (FR-RP-02).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuestionStat {
    pub index: u32,
    pub kind: QuestionKind,
    pub text: String,
    /// Players who sent an answer.
    pub answered: u32,
    pub correct: u32,
    /// Correct answers over players in the game, 0 to 1. Not answering
    /// counts as wrong.
    pub accuracy: f64,
    /// Average response time of the answers sent, in ms. Null if nobody
    /// answered.
    pub average_ms: Option<u32>,
    /// Accuracy under 35%: worth going over in class.
    pub hard: bool,
}

/// One player's answer to one question (FR-RP-03).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct PlayerAnswer {
    pub question_index: u32,
    /// The answer as text (option texts joined with "; "), or null when the
    /// player sent nothing.
    pub answer: Option<String>,
    pub correct: bool,
    pub points: u32,
    pub response_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct PlayerResult {
    pub id: String,
    pub nickname: String,
    /// Null for a removed player.
    pub rank: Option<u32>,
    #[ts(type = "number")]
    pub score: u64,
    pub correct_count: u32,
    pub kicked: bool,
    /// One entry per question, in order.
    pub answers: Vec<PlayerAnswer>,
}

/// `GET /sessions/:id`: the summary and every player's results.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct SessionReport {
    pub session: SessionSummary,
    /// Players who answered at least one question, over players in the game.
    pub participation: f64,
    /// Correct answers over all player × question pairs, 0 to 1.
    pub accuracy: f64,
    pub average_ms: Option<u32>,
    pub questions: Vec<QuestionStat>,
    /// Ranked players first, then removed players.
    pub players: Vec<PlayerResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum ReportFormat {
    /// One row per player × question (FR-RP-04).
    Csv,
    /// One row per player: the summary file of FR-RP-04.
    CsvSummary,
    /// Summary, players and questions sheets (FR-RP-05).
    Xlsx,
}

/// Body of `POST /sessions/:id/exports`. The answer is an `ExportLink`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ReportExport {
    pub format: ReportFormat,
}

/// Body of `PATCH /sessions/:id`: rename a game. Blank or null clears it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct RenameSession {
    pub name: Option<String>,
}
