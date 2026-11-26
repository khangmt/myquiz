//! REST bodies for the quiz editor (design section 6, FR-QZ-01 to 11).

use myquiz_engine::QuestionKind;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One answer option. Its position in the list is its order on screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct OptionDoc {
    pub text: String,
    pub correct: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuestionDoc {
    #[serde(rename = "type")]
    pub kind: QuestionKind,
    pub text: String,
    /// An image uploaded with POST /media.
    #[serde(default)]
    pub media_id: Option<String>,
    /// Where to show the image. Filled in by the server; ignored on save.
    #[serde(default)]
    pub media_url: Option<String>,
    pub time_limit_s: u32,
    /// 0, 1000 or 2000.
    pub points: u32,
    #[serde(default)]
    pub options: Vec<OptionDoc>,
    /// Fill-in-the-blank only, up to 5.
    #[serde(default)]
    pub accepted_answers: Vec<String>,
    #[serde(default)]
    pub ignore_diacritics: bool,
}

/// Body of `POST /quizzes` and `PUT /quizzes/:id`: the whole quiz. A draft
/// may be incomplete; publishing checks every rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct SaveQuiz {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub cover_media_id: Option<String>,
    /// "private" (default) or "public".
    #[serde(default = "private")]
    pub visibility: String,
    #[serde(default)]
    pub questions: Vec<QuestionDoc>,
}

fn private() -> String {
    "private".into()
}

/// `GET /quizzes/:id`. Send `version` back as `If-Match` when saving.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuizDoc {
    pub id: String,
    pub title: String,
    pub description: String,
    pub cover_media_id: Option<String>,
    pub cover_media_url: Option<String>,
    pub visibility: String,
    /// "draft" or "published".
    pub status: String,
    pub version: i32,
    /// RFC 3339.
    pub updated_at: String,
    /// When the server deletes the quiz unless it is edited or played
    /// again (FR-QZ-18). RFC 3339.
    pub expires_at: String,
    pub questions: Vec<QuestionDoc>,
}

/// One row of `GET /quizzes`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuizSummary {
    pub id: String,
    pub title: String,
    pub status: String,
    pub visibility: String,
    #[ts(type = "number")]
    pub question_count: i64,
    pub updated_at: String,
    pub expires_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuizPage {
    pub items: Vec<QuizSummary>,
    /// 1-based.
    pub page: u32,
    pub page_size: u32,
    #[ts(type = "number")]
    pub total: i64,
}

/// One rule a quiz breaks, from publish (422 QUIZ_INVALID).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuizProblemDoc {
    /// 0-based question index, or null for the quiz itself.
    pub question: Option<u32>,
    /// e.g. NO_CORRECT_OPTION; the client translates it.
    pub code: String,
}

/// 422 body of `PUT /quizzes/:id` and `POST /quizzes/:id/publish`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct QuizInvalid {
    pub error: crate::ApiErrorBody,
    pub problems: Vec<QuizProblemDoc>,
}

/// Body of `POST /games`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct CreateGame {
    pub quiz_id: String,
    /// Optional name for this game, e.g. the class ("SE101.A"). 1 to 80
    /// characters; blank means no name.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct GameCreated {
    pub session_id: String,
    pub pin: String,
    pub join_url: String,
}

/// `GET /games/pin/:pin`, checked before showing the nickname screen.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct PinCheck {
    pub pin: String,
    pub title: String,
    /// The host locked the lobby: new players cannot join.
    pub locked: bool,
}

/// `GET /quizzes/expiring`: quizzes that will be deleted within 3 days,
/// for the warning shown after sign-in (FR-QZ-19).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ExpiringQuiz {
    pub id: String,
    pub title: String,
    /// RFC 3339.
    pub expires_at: String,
}

/// `GET /config`: settings the web app needs before sign-in.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ClientConfig {
    /// Origin players open, e.g. http://192.168.1.20:8080. The join page is
    /// `<joinUrl>/join?pin=<pin>`.
    pub join_url: String,
}

/// Response of `POST /media`. Put `id` in a quiz; show `url`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct MediaDoc {
    pub id: String,
    /// e.g. /media/3f9c….webp, served by Caddy.
    pub url: String,
    pub width: u32,
    pub height: u32,
    pub alt_text: Option<String>,
}

/// Response of `POST /quizzes/:id/export`: open `url` to download the zip.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ExportLink {
    pub url: String,
    /// The link stops working after this time (10 minutes). RFC 3339.
    pub expires_at: String,
}

/// 201 body of `POST /quizzes/import`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Imported {
    pub quiz_id: String,
}

/// One thing wrong with an uploaded zip.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ImportProblem {
    /// The file inside the zip, e.g. quiz.json or media/q1.png.
    pub file: String,
    /// 0-based question index, if the problem is in a question.
    pub question: Option<u32>,
    /// The quiz.json field, e.g. timeLimitS.
    pub field: Option<String>,
    /// e.g. MISSING_IMAGE; the client translates it.
    pub problem: String,
}

/// 422 body of `POST /quizzes/import`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct ImportRefused {
    pub error: crate::ApiErrorBody,
    pub errors: Vec<ImportProblem>,
}
