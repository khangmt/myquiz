//! Is an answer correct? (FR-QZ-02, FR-QZ-03, FR-QZ-04)

use std::collections::BTreeSet;

use unicode_normalization::UnicodeNormalization;

use crate::answer::Submission;
use crate::quiz::{Question, QuestionKind};

/// Decides correctness for a submission that already passed
/// [`crate::answer::validate_submission`].
pub fn is_correct(question: &Question, submission: &Submission) -> bool {
    match question.kind {
        QuestionKind::Single | QuestionKind::TrueFalse | QuestionKind::Multi => {
            // Multi-correct needs exactly the correct set (FR-QZ-02); for
            // single and true/false the correct set has one element, so the
            // same rule applies.
            let chosen: BTreeSet<_> = submission.option_ids.iter().copied().collect();
            let correct: BTreeSet<_> = question.correct_option_ids().into_iter().collect();
            !chosen.is_empty() && chosen == correct
        }
        QuestionKind::FillBlank => {
            let Some(text) = submission.text.as_deref() else {
                return false;
            };
            let given = normalize(text, question.ignore_diacritics);
            !given.is_empty()
                && question
                    .accepted_answers
                    .iter()
                    .any(|a| normalize(a, question.ignore_diacritics) == given)
        }
    }
}

/// Normalises typed text for comparison: Unicode NFC, lower case, trimmed,
/// runs of whitespace collapsed to one space. With `strip_diacritics`,
/// Vietnamese tone and vowel marks are removed and đ becomes d, so
/// "ha noi" equals "Hà Nội".
pub fn normalize(text: &str, strip_diacritics: bool) -> String {
    let lowered = text.nfc().collect::<String>().to_lowercase();
    let folded: String = if strip_diacritics {
        lowered
            .nfd()
            .filter(|c| !is_combining_mark(*c))
            .map(|c| if c == 'đ' { 'd' } else { c })
            .collect()
    } else {
        lowered
    };
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_combining_mark(c: char) -> bool {
    // Combining Diacritical Marks, which covers every Vietnamese mark after NFD.
    ('\u{0300}'..='\u{036F}').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quiz::fixtures::*;

    fn choose(ids: &[u32]) -> Submission {
        Submission {
            option_ids: ids.to_vec(),
            text: None,
        }
    }

    fn typed(text: &str) -> Submission {
        Submission {
            option_ids: vec![],
            text: Some(text.into()),
        }
    }

    #[test]
    fn single_choice() {
        let q = single();
        assert!(is_correct(&q, &choose(&[1])));
        assert!(!is_correct(&q, &choose(&[2])));
        assert!(!is_correct(&q, &choose(&[])));
    }

    #[test]
    fn true_false() {
        let q = super::super::quiz::fixtures::true_false();
        assert!(is_correct(&q, &choose(&[2])));
        assert!(!is_correct(&q, &choose(&[1])));
    }

    #[test]
    fn multi_needs_the_exact_set() {
        let q = multi(); // correct: 1 and 2
        assert!(is_correct(&q, &choose(&[1, 2])));
        assert!(is_correct(&q, &choose(&[2, 1])), "order does not matter");
        assert!(!is_correct(&q, &choose(&[1])), "a subset is wrong");
        assert!(!is_correct(&q, &choose(&[1, 2, 3])), "a superset is wrong");
        assert!(!is_correct(&q, &choose(&[3, 4])));
    }

    #[test]
    fn fill_blank_ignores_case_and_spaces() {
        let q = fill_blank(); // accepts "Hà Nội" and "Ha Noi"
        assert!(is_correct(&q, &typed("hà nội")));
        assert!(is_correct(&q, &typed("  HÀ   NỘI ")));
        assert!(is_correct(&q, &typed("ha noi")), "listed as its own answer");
        assert!(!is_correct(&q, &typed("hà nọi")));
        assert!(!is_correct(&q, &typed("   ")));
        assert!(!is_correct(&q, &choose(&[1])));
    }

    #[test]
    fn fill_blank_diacritics_switch() {
        let mut q = fill_blank();
        q.accepted_answers = vec!["Đà Nẵng".into()];
        assert!(!is_correct(&q, &typed("da nang")));
        q.ignore_diacritics = true;
        assert!(is_correct(&q, &typed("da nang")));
        assert!(is_correct(&q, &typed("DA NẴNG")));
        assert!(is_correct(&q, &typed("Đà Nẵng")));
    }

    #[test]
    fn composed_and_decomposed_input_match() {
        // Phones may send "ộ" as one code point or as o + two combining marks.
        let decomposed = "Ha\u{0300} No\u{0323}\u{0302}i";
        assert_eq!(normalize(decomposed, false), normalize("Hà Nội", false));
        assert!(is_correct(&fill_blank(), &typed(decomposed)));
    }

    #[test]
    fn normalize_examples() {
        assert_eq!(normalize("  Xin   Chào\tBạn ", false), "xin chào bạn");
        assert_eq!(normalize("Xin Chào Bạn", true), "xin chao ban");
        assert_eq!(normalize("ĐƯỜNG", true), "duong");
        assert_eq!(normalize("Nguyễn", true), "nguyen");
    }
}
