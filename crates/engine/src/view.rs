//! What each audience may see of a question (SEC-15).
//!
//! Players receive [`PublicQuestion`] at `question:start`. It is built field
//! by field from an allow-list, never by serialising [`Question`] and removing
//! fields, so a new field on `Question` can never leak to players by accident.
//! The correct answers go out only at reveal, in [`RevealedAnswer`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::quiz::{OptionId, Question, QuestionKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct PublicOption {
    pub id: OptionId,
    pub text: String,
}

/// The payload of `question:start` (design section 7), minus the timing
/// fields the gateway adds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct PublicQuestion {
    pub index: usize,
    pub total: usize,
    #[serde(rename = "type")]
    pub kind: QuestionKind,
    pub text: String,
    pub media: Option<String>,
    pub options: Vec<PublicOption>,
    /// Seconds.
    pub time_limit: u32,
    pub points: u32,
}

impl PublicQuestion {
    pub fn from_question(index: usize, total: usize, q: &Question) -> Self {
        PublicQuestion {
            index,
            total,
            kind: q.kind,
            text: q.text.clone(),
            media: q.media.clone(),
            options: q
                .options
                .iter()
                .map(|o| PublicOption {
                    id: o.id,
                    text: o.text.clone(),
                })
                .collect(),
            time_limit: q.time_limit_s,
            points: q.points,
        }
    }
}

/// The correct answer, sent at reveal only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct RevealedAnswer {
    pub correct_option_ids: Vec<OptionId>,
    pub accepted_answers: Vec<String>,
}

impl RevealedAnswer {
    pub fn from_question(q: &Question) -> Self {
        RevealedAnswer {
            correct_option_ids: q.correct_option_ids(),
            accepted_answers: q.accepted_answers.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quiz::fixtures::*;

    /// SEC-15: fails if anything that reveals the answer reaches players.
    #[test]
    fn public_question_never_contains_answers() {
        let quiz = quiz();
        for (i, q) in quiz.questions.iter().enumerate() {
            let json =
                serde_json::to_string(&PublicQuestion::from_question(i, quiz.total(), q)).unwrap();
            for forbidden in [
                "is_correct",
                "isCorrect",
                "correct",
                "accepted",
                "ignore_diacritics",
            ] {
                assert!(
                    !json.contains(forbidden),
                    "question {i} leaks {forbidden}: {json}"
                );
            }
            for answer in &q.accepted_answers {
                assert!(!json.contains(answer.as_str()) || q.text.contains(answer.as_str()));
            }
        }
    }

    #[test]
    fn public_question_shape() {
        let q = single();
        let v = serde_json::to_value(PublicQuestion::from_question(0, 4, &q)).unwrap();
        assert_eq!(v["type"], "single");
        assert_eq!(v["timeLimit"], 20);
        assert_eq!(
            v["options"][0],
            serde_json::json!({"id": 1, "text": "Hà Nội"})
        );
        assert_eq!(v["options"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn fill_blank_has_no_options_to_leak() {
        let v = serde_json::to_value(PublicQuestion::from_question(3, 4, &fill_blank())).unwrap();
        assert_eq!(v["options"], serde_json::json!([]));
    }

    #[test]
    fn reveal_carries_the_answer() {
        assert_eq!(
            RevealedAnswer::from_question(&multi()).correct_option_ids,
            [1, 2]
        );
        assert_eq!(
            RevealedAnswer::from_question(&fill_blank()).accepted_answers,
            ["Hà Nội", "Ha Noi"]
        );
    }
}
