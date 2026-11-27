//! The quiz snapshot a game runs on.
//!
//! When a game starts the quiz is copied into the session as JSON
//! (design section 5), so editing the quiz later never changes a running game
//! or a past report. The snapshot includes the correct answers and must never
//! be sent to players as is; see [`crate::view`].

use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

/// Option ids are small integers, stable within one snapshot.
pub type OptionId = u32;

pub const TITLE_MAX: usize = 120;
pub const QUESTION_TEXT_MAX: usize = 300;
/// Longest Markdown block (code, lists) a question can show instead of an
/// image. Enough for a 60-line C program.
pub const MARKDOWN_MAX: usize = 4000;
pub const OPTION_TEXT_MAX: usize = 120;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 6;
pub const MAX_ACCEPTED_ANSWERS: usize = 5;
pub const TEXT_ANSWER_MAX: usize = 50;
pub const TIME_LIMIT_MIN_S: u32 = 5;
pub const TIME_LIMIT_MAX_S: u32 = 240;
pub const ALLOWED_POINTS: [u32; 3] = [0, 1000, 2000];
/// The marker a fill-in-the-blank question uses for its one blank (FR-QZ-04).
pub const BLANK_MARKER: &str = "___";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../../../web/src/generated/")]
pub enum QuestionKind {
    /// Multiple choice, exactly one correct option.
    Single,
    /// Multiple choice, one or more correct options; the player must pick exactly that set.
    Multi,
    /// Exactly two options, one correct.
    TrueFalse,
    /// The player types the missing word.
    FillBlank,
}

impl QuestionKind {
    /// Default time limit in seconds (FR-QZ-06).
    pub fn default_time_limit_s(self) -> u32 {
        match self {
            QuestionKind::FillBlank => 30,
            _ => 20,
        }
    }

    pub fn has_options(self) -> bool {
        !matches!(self, QuestionKind::FillBlank)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerOption {
    pub id: OptionId,
    pub text: String,
    pub is_correct: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    pub kind: QuestionKind,
    pub text: String,
    /// Stored media id, if the question has an image.
    #[serde(default)]
    pub media: Option<String>,
    /// Markdown shown in the media area instead of (or below) an image, e.g.
    /// a fenced C code block. Rendered and sanitised by the web client.
    #[serde(default)]
    pub markdown: Option<String>,
    pub time_limit_s: u32,
    /// 0, 1000 or 2000 (FR-QZ-06).
    pub points: u32,
    /// Empty for fill-in-the-blank.
    #[serde(default)]
    pub options: Vec<AnswerOption>,
    /// Fill-in-the-blank only, up to 5.
    #[serde(default)]
    pub accepted_answers: Vec<String>,
    /// Fill-in-the-blank only: "ha noi" also matches "Hà Nội".
    #[serde(default)]
    pub ignore_diacritics: bool,
}

impl Question {
    pub fn time_limit_ms(&self) -> u64 {
        u64::from(self.time_limit_s) * 1000
    }

    pub fn correct_option_ids(&self) -> Vec<OptionId> {
        self.options
            .iter()
            .filter(|o| o.is_correct)
            .map(|o| o.id)
            .collect()
    }

    pub fn has_option(&self, id: OptionId) -> bool {
        self.options.iter().any(|o| o.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuizSnapshot {
    pub title: String,
    pub questions: Vec<Question>,
}

impl QuizSnapshot {
    pub fn total(&self) -> usize {
        self.questions.len()
    }

    pub fn question(&self, index: usize) -> Option<&Question> {
        self.questions.get(index)
    }

    /// The publish rules (FR-QZ-10): every problem in the quiz, so the editor
    /// can show them all at once. An empty list means the quiz can be published.
    pub fn validate(&self) -> Vec<QuizProblem> {
        let mut problems = Vec::new();
        let title_len = self.title.trim().chars().count();
        if title_len == 0 || title_len > TITLE_MAX {
            problems.push(QuizProblem {
                question: None,
                error: ValidationError::TitleLength,
            });
        }
        if self.questions.is_empty() {
            problems.push(QuizProblem {
                question: None,
                error: ValidationError::NoQuestions,
            });
        }
        for (i, q) in self.questions.iter().enumerate() {
            problems.extend(validate_question(q).into_iter().map(|error| QuizProblem {
                question: Some(i),
                error,
            }));
        }
        problems
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QuizProblem {
    /// Index of the question, or `None` for a quiz-level problem.
    pub question: Option<usize>,
    pub error: ValidationError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValidationError {
    #[error("title must be 1 to 120 characters")]
    TitleLength,
    #[error("a quiz needs at least one question")]
    NoQuestions,
    #[error("question text must be 1 to 300 characters")]
    QuestionTextLength,
    #[error("time limit must be 5 to 240 seconds")]
    TimeLimit,
    #[error("points must be 0, 1000 or 2000")]
    Points,
    #[error("a choice question needs 2 to 6 options")]
    OptionCount,
    #[error("a true/false question needs exactly 2 options")]
    TrueFalseOptionCount,
    #[error("option text must be 1 to 120 characters")]
    OptionTextLength,
    #[error("option ids must be unique within a question")]
    DuplicateOptionId,
    #[error("mark at least one option as correct")]
    NoCorrectOption,
    #[error("this question type allows exactly one correct option")]
    TooManyCorrectOptions,
    #[error("fill-in-the-blank needs 1 to 5 accepted answers")]
    AcceptedAnswerCount,
    #[error("accepted answers must be 1 to 50 characters")]
    AcceptedAnswerLength,
    #[error("fill-in-the-blank text must contain exactly one blank (___)")]
    BlankMarker,
    #[error("fill-in-the-blank questions have no options")]
    UnexpectedOptions,
    #[error("the code/Markdown block must be at most 4000 characters")]
    MarkdownLength,
}

/// Every rule a single question breaks. Empty means valid.
pub fn validate_question(q: &Question) -> Vec<ValidationError> {
    use ValidationError as E;
    let mut errors = Vec::new();

    let text_len = q.text.trim().chars().count();
    if text_len == 0 || text_len > QUESTION_TEXT_MAX {
        errors.push(E::QuestionTextLength);
    }
    if !(TIME_LIMIT_MIN_S..=TIME_LIMIT_MAX_S).contains(&q.time_limit_s) {
        errors.push(E::TimeLimit);
    }
    if !ALLOWED_POINTS.contains(&q.points) {
        errors.push(E::Points);
    }
    if q.markdown
        .as_deref()
        .is_some_and(|m| m.chars().count() > MARKDOWN_MAX)
    {
        errors.push(E::MarkdownLength);
    }

    if q.kind.has_options() {
        let n = q.options.len();
        if q.kind == QuestionKind::TrueFalse {
            if n != 2 {
                errors.push(E::TrueFalseOptionCount);
            }
        } else if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&n) {
            errors.push(E::OptionCount);
        }
        if q.options.iter().any(|o| {
            let len = o.text.trim().chars().count();
            len == 0 || len > OPTION_TEXT_MAX
        }) {
            errors.push(E::OptionTextLength);
        }
        let mut ids: Vec<_> = q.options.iter().map(|o| o.id).collect();
        ids.sort_unstable();
        ids.dedup();
        if ids.len() != n {
            errors.push(E::DuplicateOptionId);
        }
        let correct = q.options.iter().filter(|o| o.is_correct).count();
        if correct == 0 {
            errors.push(E::NoCorrectOption);
        } else if correct > 1 && q.kind != QuestionKind::Multi {
            errors.push(E::TooManyCorrectOptions);
        }
    } else {
        if !q.options.is_empty() {
            errors.push(E::UnexpectedOptions);
        }
        let n = q.accepted_answers.len();
        if n == 0 || n > MAX_ACCEPTED_ANSWERS {
            errors.push(E::AcceptedAnswerCount);
        }
        if q.accepted_answers.iter().any(|a| {
            let len = a.trim().chars().count();
            len == 0 || len > TEXT_ANSWER_MAX
        }) {
            errors.push(E::AcceptedAnswerLength);
        }
        if q.text.matches(BLANK_MARKER).count() != 1 {
            errors.push(E::BlankMarker);
        }
    }
    errors
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn opt(id: OptionId, text: &str, is_correct: bool) -> AnswerOption {
        AnswerOption {
            id,
            text: text.into(),
            is_correct,
        }
    }

    pub fn single() -> Question {
        Question {
            kind: QuestionKind::Single,
            text: "Thủ đô của Việt Nam là?".into(),
            media: None,
            markdown: None,
            time_limit_s: 20,
            points: 1000,
            options: vec![
                opt(1, "Hà Nội", true),
                opt(2, "Huế", false),
                opt(3, "Đà Nẵng", false),
                opt(4, "TP.HCM", false),
            ],
            accepted_answers: vec![],
            ignore_diacritics: false,
        }
    }

    pub fn multi() -> Question {
        Question {
            kind: QuestionKind::Multi,
            text: "Which are prime?".into(),
            options: vec![
                opt(1, "2", true),
                opt(2, "3", true),
                opt(3, "4", false),
                opt(4, "9", false),
            ],
            ..single()
        }
    }

    pub fn true_false() -> Question {
        Question {
            kind: QuestionKind::TrueFalse,
            text: "Rust has a garbage collector.".into(),
            options: vec![opt(1, "True", false), opt(2, "False", true)],
            ..single()
        }
    }

    pub fn fill_blank() -> Question {
        Question {
            kind: QuestionKind::FillBlank,
            text: "Thủ đô của Việt Nam là ___.".into(),
            time_limit_s: 30,
            options: vec![],
            accepted_answers: vec!["Hà Nội".into(), "Ha Noi".into()],
            ignore_diacritics: false,
            ..single()
        }
    }

    pub fn quiz() -> QuizSnapshot {
        QuizSnapshot {
            title: "Demo".into(),
            questions: vec![single(), multi(), true_false(), fill_blank()],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use ValidationError as E;

    #[test]
    fn fixtures_are_valid() {
        assert_eq!(quiz().validate(), vec![]);
    }

    #[test]
    fn default_time_limits() {
        assert_eq!(QuestionKind::Single.default_time_limit_s(), 20);
        assert_eq!(QuestionKind::FillBlank.default_time_limit_s(), 30);
    }

    #[test]
    fn quiz_level_rules() {
        let q = QuizSnapshot {
            title: "  ".into(),
            questions: vec![],
        };
        let errors: Vec<_> = q.validate().into_iter().map(|p| p.error).collect();
        assert_eq!(errors, vec![E::TitleLength, E::NoQuestions]);

        let long = QuizSnapshot {
            title: "a".repeat(121),
            questions: vec![single()],
        };
        assert_eq!(long.validate()[0].error, E::TitleLength);
    }

    #[test]
    fn problems_name_the_question() {
        let mut quiz = quiz();
        quiz.questions[2].points = 500;
        assert_eq!(
            quiz.validate(),
            vec![QuizProblem {
                question: Some(2),
                error: E::Points
            }]
        );
    }

    #[test]
    fn text_time_and_points_limits() {
        let mut q = single();
        q.text = "x".repeat(301);
        q.time_limit_s = 4;
        q.points = 1500;
        assert_eq!(
            validate_question(&q),
            vec![E::QuestionTextLength, E::TimeLimit, E::Points]
        );
        q.text = "x".repeat(300);
        q.time_limit_s = 240;
        q.points = 0;
        assert_eq!(validate_question(&q), vec![]);
    }

    #[test]
    fn text_length_counts_characters_not_bytes() {
        let mut q = single();
        q.text = "ệ".repeat(300); // 900 bytes in UTF-8
        assert_eq!(validate_question(&q), vec![]);
    }

    #[test]
    fn option_count_rules() {
        let mut q = single();
        q.options.truncate(1);
        assert!(validate_question(&q).contains(&E::OptionCount));
        q.options = (1..=7).map(|i| opt(i, "x", i == 1)).collect();
        assert!(validate_question(&q).contains(&E::OptionCount));
        q.options.truncate(6);
        assert_eq!(validate_question(&q), vec![]);

        let mut tf = true_false();
        tf.options.push(opt(3, "Maybe", false));
        assert_eq!(validate_question(&tf), vec![E::TrueFalseOptionCount]);
    }

    #[test]
    fn correct_option_rules() {
        let mut q = single();
        for o in &mut q.options {
            o.is_correct = false;
        }
        assert_eq!(validate_question(&q), vec![E::NoCorrectOption]);
        q.options[0].is_correct = true;
        q.options[1].is_correct = true;
        assert_eq!(validate_question(&q), vec![E::TooManyCorrectOptions]);
        q.kind = QuestionKind::Multi;
        assert_eq!(validate_question(&q), vec![]);

        let mut tf = true_false();
        tf.options[0].is_correct = true;
        assert_eq!(validate_question(&tf), vec![E::TooManyCorrectOptions]);
    }

    #[test]
    fn duplicate_and_empty_options() {
        let mut q = single();
        q.options[1].id = 1;
        q.options[2].text = " ".into();
        assert_eq!(
            validate_question(&q),
            vec![E::OptionTextLength, E::DuplicateOptionId]
        );
    }

    #[test]
    fn fill_blank_rules() {
        let mut q = fill_blank();
        q.accepted_answers = vec![];
        q.text = "No blank here".into();
        q.options = vec![opt(1, "x", true)];
        assert_eq!(
            validate_question(&q),
            vec![E::UnexpectedOptions, E::AcceptedAnswerCount, E::BlankMarker]
        );

        let mut q = fill_blank();
        q.accepted_answers = (0..6).map(|i| i.to_string()).collect();
        assert_eq!(validate_question(&q), vec![E::AcceptedAnswerCount]);

        let mut q = fill_blank();
        q.accepted_answers = vec!["x".repeat(51)];
        assert_eq!(validate_question(&q), vec![E::AcceptedAnswerLength]);

        let mut q = fill_blank();
        q.text = "___ and ___".into();
        assert_eq!(validate_question(&q), vec![E::BlankMarker]);
    }

    #[test]
    fn snapshot_round_trips_as_json() {
        let quiz = quiz();
        let json = serde_json::to_string(&quiz).unwrap();
        assert!(json.contains("\"kind\":\"fill_blank\""));
        let back: QuizSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(back, quiz);
    }
}
