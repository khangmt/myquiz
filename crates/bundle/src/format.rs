//! `quiz.json`, version 1 (design section 8, "Quiz zip format").
//!
//! The server ignores any ids or owner in the file; it only ever creates a
//! new private draft from it.

use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "myquiz.quiz";
pub const CURRENT_VERSION: u32 = 1;

/// The outer envelope: read first, so an unknown version is refused before
/// the rest of the file is interpreted.
#[derive(Debug, Clone, Deserialize)]
pub struct Header {
    pub format: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuizFileV1 {
    pub format: String,
    pub version: u32,
    /// RFC 3339 time of export; informational only.
    #[serde(default)]
    pub exported_at: Option<String>,
    pub quiz: QuizV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuizV1 {
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Path inside the zip, e.g. `media/cover.webp`.
    #[serde(default)]
    pub cover: Option<String>,
    pub questions: Vec<QuestionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionV1 {
    #[serde(rename = "type")]
    pub kind: myquiz_engine::QuestionKind,
    pub text: String,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub image_alt: Option<String>,
    /// Markdown for the media area, e.g. a fenced C code block. Older
    /// files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    pub time_limit_s: u32,
    pub points: u32,
    #[serde(default)]
    pub options: Vec<OptionV1>,
    #[serde(default)]
    pub accepted_answers: Vec<String>,
    #[serde(default)]
    pub ignore_diacritics: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionV1 {
    pub text: String,
    pub correct: bool,
}

impl QuizFileV1 {
    pub fn new(quiz: QuizV1, exported_at: Option<String>) -> Self {
        QuizFileV1 {
            format: FORMAT.into(),
            version: CURRENT_VERSION,
            exported_at,
            quiz,
        }
    }
}

impl QuizV1 {
    /// The engine's view of this quiz, with option ids 1..=n per question,
    /// so it can go through the same validator as the editor.
    pub fn to_snapshot(&self) -> myquiz_engine::QuizSnapshot {
        myquiz_engine::QuizSnapshot {
            title: self.title.clone(),
            questions: self
                .questions
                .iter()
                .map(|q| myquiz_engine::Question {
                    kind: q.kind,
                    text: q.text.clone(),
                    media: q.image.clone(),
                    markdown: q.markdown.clone(),
                    time_limit_s: q.time_limit_s,
                    points: q.points,
                    options: q
                        .options
                        .iter()
                        .enumerate()
                        .map(|(i, o)| myquiz_engine::quiz::AnswerOption {
                            id: i as u32 + 1,
                            text: o.text.clone(),
                            is_correct: o.correct,
                        })
                        .collect(),
                    accepted_answers: q.accepted_answers.clone(),
                    ignore_diacritics: q.ignore_diacritics,
                })
                .collect(),
        }
    }

    /// Every image path the quiz refers to.
    pub fn image_refs(&self) -> impl Iterator<Item = &str> {
        self.cover
            .as_deref()
            .into_iter()
            .chain(self.questions.iter().filter_map(|q| q.image.as_deref()))
    }
}
