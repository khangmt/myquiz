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
