//! A small Vietnamese and English word list for nicknames (SEC-08).
//!
//! The aim is to stop the obvious cases on a classroom projector, not to be
//! a complete filter. Three checks, all after lower-casing:
//!
//! - whole words with their Vietnamese marks ("lồn" is blocked, "lớn" and
//!   "lợn" are not, which a mark-blind check would confuse);
//! - whole words and short abbreviations without marks ("dm", "vcl");
//! - longer English words anywhere once spaces, dots and look-alike digits
//!   are squeezed out ("f.u.c.k", "sh1t").

use crate::matching::normalize;

/// Matched as whole words, marks kept.
const WORDS_MARKED: &[&str] = &[
    "lồn", "cặc", "buồi", "địt", "đụ", "đéo", "đĩ", "cứt", "đếch", "nứng", "dái", "đcm", "đm",
    "đmm", "đkm", "đjt", "cave",
];

/// Matched as whole words after marks are removed.
const WORDS_PLAIN: &[&str] = &[
    "dit", "djt", "deo", "dech", "dm", "dcm", "dmm", "dkm", "vcl", "vkl", "vl", "clgt", "cl",
    "cmm", "cc", "loz", "buoi", "fuck", "fck", "fuk", "shit", "bitch", "cunt", "dick", "cock",
    "pussy", "ass", "asshole", "bastard", "whore", "slut", "porn", "rape", "retard", "nigger",
    "nigga", "faggot", "fag", "penis", "vagina", "sex", "hitler", "nazi",
];

/// Found anywhere in the squeezed text: long enough not to hit innocent
/// words.
const FRAGMENTS: &[&str] = &[
    "fuck",
    "shit",
    "bitch",
    "nigg",
    "fagg",
    "whore",
    "asshole",
    "pussy",
    "motherf",
    "dickhead",
    "dumamay",
    "ditme",
    "ditcon",
    "dume",
    "duma",
    "conchonao",
    "occho",
    "occac",
];

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
}

fn squeeze(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '0' => 'o',
            '1' | '!' | '|' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            _ => c,
        })
        .filter(|c| c.is_alphabetic())
        .collect()
}

/// True if the text should be refused as a nickname.
pub fn is_offensive(text: &str) -> bool {
    let marked = normalize(text, false);
    if words(&marked).any(|w| WORDS_MARKED.contains(&w)) {
        return true;
    }
    let plain = normalize(text, true);
    if words(&plain).any(|w| WORDS_PLAIN.contains(&w)) {
        return true;
    }
    let squeezed = squeeze(&plain);
    FRAGMENTS.iter().any(|f| squeezed.contains(f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_obvious_cases() {
        for bad in [
            "Lồn",
            "đm",
            "ĐCM",
            "dit me",
            "Địt",
            "vcl",
            "Fuck",
            "f.u.c.k",
            "sh1t",
            "BITCH",
            "a$$hole",
            "con cho nao",
            "Du Ma",
            "đụ",
            "nigga",
        ] {
            assert!(is_offensive(bad), "{bad:?} should be refused");
        }
    }

    #[test]
    fn lets_ordinary_names_through() {
        for ok in [
            "Lớn",
            "Lợn con",
            "Các bạn",
            "An",
            "Bình",
            "Đức",
            "Dũng",
            "Cường",
            "Dương",
            "Thủy",
            "Hà Nội",
            "Classic",
            "Cocktail",
            "Scunthorpe",
            "Passion",
            "Dickens fan",
            "Mai Lan",
            "Đỗ Minh",
            "Sơn",
            "Assam tea",
            "Kiều Oanh",
            "Duy",
            "Hoàng",
            "Bassist",
        ] {
            assert!(!is_offensive(ok), "{ok:?} should be allowed");
        }
    }
}
