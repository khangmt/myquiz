//! Checking a submitted answer before it is stored (FR-GM-06, SEC-16).
//!
//! The gateway calls [`validate_submission`] on the parsed payload, then the
//! Redis answer script repeats the state, index and deadline checks atomically
//! with first-answer-wins storage. [`check_timing`] is the same rule in Rust,
//! used by tests and as the reference for the Lua script.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::quiz::{OptionId, Question, QuestionKind, TEXT_ANSWER_MAX};
use crate::state::GameState;

/// What a player sends in `answer:submit`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct Submission {
    #[serde(default, rename = "optionIds")]
    pub option_ids: Vec<OptionId>,
    #[serde(default)]
    pub text: Option<String>,
}

/// Refusal codes sent back in the ack (design section 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum SubmitError {
    /// The payload does not fit the question (wrong ids, empty text, too long).
    #[error("answer does not fit the question")]
    Invalid,
    /// The game is not showing this question.
    #[error("not accepting answers for this question")]
    WrongState,
    /// Sent during the "get ready" countdown, before the options were shown.
    #[error("question has not started")]
    Early,
    /// Arrived at or after the server deadline.
    #[error("time is up")]
    Late,
    /// The player already answered this question; the first answer stands.
    #[error("already answered")]
    Duplicate,
}

impl SubmitError {
    pub fn code(self) -> &'static str {
        match self {
            SubmitError::Invalid => "INVALID",
            SubmitError::WrongState => "WRONG_STATE",
            SubmitError::Early => "EARLY",
            SubmitError::Late => "LATE",
            SubmitError::Duplicate => "DUPLICATE",
        }
    }
}

/// Does the payload's shape fit the question type?
pub fn validate_submission(question: &Question, s: &Submission) -> Result<(), SubmitError> {
    match question.kind {
        QuestionKind::Single | QuestionKind::TrueFalse => {
            if s.text.is_some() || s.option_ids.len() != 1 || !question.has_option(s.option_ids[0])
            {
                return Err(SubmitError::Invalid);
            }
        }
        QuestionKind::Multi => {
            if s.text.is_some()
                || s.option_ids.is_empty()
                || s.option_ids.len() > question.options.len()
                || !s.option_ids.iter().all(|id| question.has_option(*id))
            {
                return Err(SubmitError::Invalid);
            }
            let mut ids = s.option_ids.clone();
            ids.sort_unstable();
            ids.dedup();
            if ids.len() != s.option_ids.len() {
                return Err(SubmitError::Invalid);
            }
        }
        QuestionKind::FillBlank => {
            let Some(text) = s.text.as_deref() else {
                return Err(SubmitError::Invalid);
            };
            let len = text.trim().chars().count();
            if !s.option_ids.is_empty() || len == 0 || len > TEXT_ANSWER_MAX {
                return Err(SubmitError::Invalid);
            }
        }
    }
    Ok(())
}

/// Is the game accepting an answer for `index` at server time `now_ms`?
/// Answers are accepted from `starts_at_ms` (options shown) until strictly
/// before `ends_at_ms`.
pub fn check_timing(
    state: &GameState,
    index: usize,
    now_ms: u64,
    starts_at_ms: u64,
    ends_at_ms: u64,
) -> Result<(), SubmitError> {
    if *state != (GameState::Question { index }) {
        return Err(SubmitError::WrongState);
    }
    if now_ms < starts_at_ms {
        return Err(SubmitError::Early);
    }
    if now_ms >= ends_at_ms {
        return Err(SubmitError::Late);
    }
    Ok(())
}

/// Response time measured on the server from when the options were shown.
pub fn response_ms(starts_at_ms: u64, received_at_ms: u64) -> u64 {
    received_at_ms.saturating_sub(starts_at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quiz::fixtures::*;

    fn ids(v: &[u32]) -> Submission {
        Submission {
            option_ids: v.to_vec(),
            text: None,
        }
    }

    fn text(t: &str) -> Submission {
        Submission {
            option_ids: vec![],
            text: Some(t.into()),
        }
    }

    #[test]
    fn single_needs_one_known_option() {
        let q = single();
        assert_eq!(validate_submission(&q, &ids(&[2])), Ok(()));
        assert_eq!(
            validate_submission(&q, &ids(&[])),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &ids(&[1, 2])),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &ids(&[99])),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &text("Hà Nội")),
            Err(SubmitError::Invalid)
        );
    }

    #[test]
    fn multi_needs_distinct_known_options() {
        let q = multi();
        assert_eq!(validate_submission(&q, &ids(&[1])), Ok(()));
        assert_eq!(validate_submission(&q, &ids(&[1, 3, 4])), Ok(()));
        assert_eq!(
            validate_submission(&q, &ids(&[1, 1])),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &ids(&[1, 99])),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &ids(&[])),
            Err(SubmitError::Invalid)
        );
    }

    #[test]
    fn fill_blank_needs_short_text() {
        let q = fill_blank();
        assert_eq!(validate_submission(&q, &text("Hà Nội")), Ok(()));
        assert_eq!(
            validate_submission(&q, &text("  ")),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &text(&"a".repeat(51))),
            Err(SubmitError::Invalid)
        );
        assert_eq!(
            validate_submission(&q, &ids(&[1])),
            Err(SubmitError::Invalid)
        );
        assert_eq!(validate_submission(&q, &text(&"ộ".repeat(50))), Ok(()));
    }

    #[test]
    fn timing_window() {
        let q1 = GameState::Question { index: 1 };
        // shown at 10_000, deadline at 30_000
        assert_eq!(check_timing(&q1, 1, 10_000, 10_000, 30_000), Ok(()));
        assert_eq!(check_timing(&q1, 1, 29_999, 10_000, 30_000), Ok(()));
        assert_eq!(
            check_timing(&q1, 1, 30_000, 10_000, 30_000),
            Err(SubmitError::Late)
        );
        assert_eq!(
            check_timing(&q1, 1, 9_999, 10_000, 30_000),
            Err(SubmitError::Early)
        );
        assert_eq!(
            check_timing(&q1, 0, 15_000, 10_000, 30_000),
            Err(SubmitError::WrongState)
        );
        let reveal = GameState::Reveal { index: 1 };
        assert_eq!(
            check_timing(&reveal, 1, 15_000, 10_000, 30_000),
            Err(SubmitError::WrongState)
        );
    }

    #[test]
    fn response_time_never_negative() {
        assert_eq!(response_ms(10_000, 12_345), 2_345);
        assert_eq!(response_ms(10_000, 9_000), 0);
    }

    #[test]
    fn payload_uses_camel_case() {
        let s: Submission = serde_json::from_str(r#"{"optionIds":[1,2]}"#).unwrap();
        assert_eq!(s, ids(&[1, 2]));
        assert_eq!(SubmitError::Late.code(), "LATE");
        assert_eq!(
            serde_json::to_string(&SubmitError::WrongState).unwrap(),
            "\"WRONG_STATE\""
        );
    }
}
