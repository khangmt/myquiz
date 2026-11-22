//! Scoring a whole question at reveal (FR-GM-07, FR-GM-08, FR-SC-01).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::answer::Submission;
use crate::matching::{is_correct, normalize};
use crate::quiz::{OptionId, Question, QuestionKind};
use crate::scoring::points;

/// How many distinct typed answers the host screen shows for fill-in-the-blank.
pub const TOP_TEXT_ANSWERS: usize = 5;

/// An answer as stored by the Redis answer script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAnswer<P> {
    pub player: P,
    pub submission: Submission,
    pub response_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerResult<P> {
    pub player: P,
    pub correct: bool,
    pub points: u32,
    pub response_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct OptionCount {
    #[serde(rename = "optionId")]
    pub option_id: OptionId,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../web/src/generated/")]
pub struct TextCount {
    pub text: String,
    pub count: u32,
    pub correct: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOutcome<P> {
    /// One result per stored answer. Players who did not answer have none and
    /// score 0; see [`time_charged_ms`] for their tie-break time.
    pub results: Vec<AnswerResult<P>>,
    /// Count per option in option order, including options nobody picked.
    pub distribution: Vec<OptionCount>,
    /// For fill-in-the-blank: the most common typed answers, normalised.
    pub top_text_answers: Vec<TextCount>,
}

/// Marks and scores every answer to one question and builds what the host's
/// reveal screen needs.
pub fn score_question<P: Clone>(
    question: &Question,
    answers: &[StoredAnswer<P>],
) -> QuestionOutcome<P> {
    let limit = question.time_limit_ms();
    let results = answers
        .iter()
        .map(|a| {
            let correct = is_correct(question, &a.submission);
            AnswerResult {
                player: a.player.clone(),
                correct,
                points: points(correct, question.points, a.response_ms, limit),
                response_ms: a.response_ms.min(limit),
            }
        })
        .collect();

    let distribution = question
        .options
        .iter()
        .map(|o| OptionCount {
            option_id: o.id,
            count: answers
                .iter()
                .filter(|a| a.submission.option_ids.contains(&o.id))
                .count() as u32,
        })
        .collect();

    let top_text_answers = if question.kind == QuestionKind::FillBlank {
        top_texts(question, answers)
    } else {
        Vec::new()
    };

    QuestionOutcome {
        results,
        distribution,
        top_text_answers,
    }
}

fn top_texts<P>(question: &Question, answers: &[StoredAnswer<P>]) -> Vec<TextCount> {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for a in answers {
        if let Some(t) = a.submission.text.as_deref() {
            let n = normalize(t, false);
            if !n.is_empty() {
                *counts.entry(n).or_default() += 1;
            }
        }
    }
    let mut list: Vec<TextCount> = counts
        .into_iter()
        .map(|(text, count)| {
            let correct = is_correct(
                question,
                &Submission {
                    option_ids: vec![],
                    text: Some(text.clone()),
                },
            );
            TextCount {
                text,
                count,
                correct,
            }
        })
        .collect();
    // Most common first; ties alphabetical (BTreeMap order, stable sort).
    list.sort_by_key(|t| std::cmp::Reverse(t.count));
    list.truncate(TOP_TEXT_ANSWERS);
    list
}

/// Time added to a player's tie-break total for one question: their response
/// time, or the full time limit if they did not answer, so staying silent
/// never beats answering.
pub fn time_charged_ms(question: &Question, response_ms: Option<u64>) -> u64 {
    let limit = question.time_limit_ms();
    response_ms.map_or(limit, |t| t.min(limit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quiz::fixtures::*;

    fn ans(
        player: &'static str,
        ids: &[u32],
        text: Option<&str>,
        response_ms: u64,
    ) -> StoredAnswer<&'static str> {
        StoredAnswer {
            player,
            submission: Submission {
                option_ids: ids.to_vec(),
                text: text.map(Into::into),
            },
            response_ms,
        }
    }

    #[test]
    fn scores_and_distribution_for_single_choice() {
        let q = single(); // correct option 1, 20 s, 1000 points
        let out = score_question(
            &q,
            &[
                ans("an", &[1], None, 0),
                ans("binh", &[1], None, 10_000),
                ans("chi", &[3], None, 1_000),
            ],
        );
        let pts: Vec<_> = out
            .results
            .iter()
            .map(|r| (r.player, r.correct, r.points))
            .collect();
        assert_eq!(
            pts,
            [("an", true, 1000), ("binh", true, 750), ("chi", false, 0)]
        );
        let dist: Vec<_> = out
            .distribution
            .iter()
            .map(|d| (d.option_id, d.count))
            .collect();
        assert_eq!(dist, [(1, 2), (2, 0), (3, 1), (4, 0)]);
        assert!(out.top_text_answers.is_empty());
    }

    #[test]
    fn multi_counts_each_chosen_option() {
        let out = score_question(
            &multi(),
            &[ans("a", &[1, 2], None, 0), ans("b", &[1], None, 0)],
        );
        let dist: Vec<_> = out.distribution.iter().map(|d| d.count).collect();
        assert_eq!(dist, [2, 1, 0, 0]);
        assert_eq!(out.results[0].points, 1000);
        assert_eq!(
            out.results[1].points, 0,
            "subset of the correct set scores nothing"
        );
    }

    #[test]
    fn fill_blank_top_answers() {
        let q = fill_blank();
        let out = score_question(
            &q,
            &[
                ans("a", &[], Some("Hà Nội"), 1_000),
                ans("b", &[], Some("hà  nội"), 2_000),
                ans("c", &[], Some("Huế"), 3_000),
                ans("d", &[], Some("HÀ NỘI"), 4_000),
                ans("e", &[], Some("huế"), 5_000),
                ans("f", &[], Some("Sài Gòn"), 6_000),
            ],
        );
        assert!(out.distribution.is_empty());
        let top: Vec<_> = out
            .top_text_answers
            .iter()
            .map(|t| (t.text.as_str(), t.count, t.correct))
            .collect();
        assert_eq!(
            top,
            [
                ("hà nội", 3, true),
                ("huế", 2, false),
                ("sài gòn", 1, false)
            ]
        );
        // 30 s limit: 1 s → round(1000 × (1 − 1/60)) = 983
        assert_eq!(out.results[0].points, 983);
    }

    #[test]
    fn response_time_is_capped_at_the_limit() {
        let out = score_question(&single(), &[ans("late", &[1], None, 99_000)]);
        assert_eq!(out.results[0].response_ms, 20_000);
        assert_eq!(out.results[0].points, 500);
    }

    #[test]
    fn silence_costs_the_full_time() {
        let q = single();
        assert_eq!(time_charged_ms(&q, None), 20_000);
        assert_eq!(time_charged_ms(&q, Some(3_000)), 3_000);
        assert_eq!(time_charged_ms(&q, Some(60_000)), 20_000);
    }
}
