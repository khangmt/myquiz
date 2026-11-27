use std::collections::{BTreeMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::sync::LazyLock;

use myquiz_engine::quiz::{QuizProblem, ValidationError};
use regex_lite::Regex;
use serde::Serialize;
use zip::CompressionMethod;
use zip::ZipArchive;

use crate::format::{Header, QuizFileV1, QuizV1, CURRENT_VERSION, FORMAT};

/// SEC-31 and SEC-32 limits. The upload itself is capped at 25 MB before
/// this runs.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_entries: usize,
    /// Uncompressed bytes over all entries that are read.
    pub max_total: u64,
    /// Uncompressed : compressed, per entry.
    pub max_ratio: u64,
    pub max_quiz_json: u64,
    /// Per image, the same as a normal upload (SEC-11).
    pub max_image: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_entries: 250,
            max_total: 100 * 1024 * 1024,
            max_ratio: 100,
            max_quiz_json: 1024 * 1024,
            max_image: 5 * 1024 * 1024,
        }
    }
}

/// One thing wrong with an uploaded zip, shown to the user as a list in
/// their language (the client translates `problem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    /// The file inside the zip, e.g. `quiz.json` or `media/q1.png`.
    pub file: String,
    /// Question index (0-based) for problems inside a question.
    pub question: Option<usize>,
    /// The quiz.json field, e.g. `options` or `timeLimitS`.
    pub field: Option<String>,
    /// A code such as `MISSING_IMAGE` or `TIME_LIMIT`.
    pub problem: String,
    /// Extra detail for developers, e.g. the JSON parse error position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl Problem {
    fn file(file: &str, problem: &str) -> Self {
        Problem {
            file: file.into(),
            question: None,
            field: None,
            problem: problem.into(),
            detail: None,
        }
    }

    fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
}

/// A checked bundle. Images are raw bytes keyed by their path in quiz.json
/// and must still go through the media pipeline before they are stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    pub quiz: QuizV1,
    pub images: BTreeMap<String, Vec<u8>>,
}

static MEDIA_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^media/[a-z0-9_-]{1,64}\.(jpg|jpeg|png|webp)$").unwrap());

/// Files that zip tools add on their own; skipped without being read.
fn is_os_junk(name: &str) -> bool {
    name.starts_with("__MACOSX/")
        || name.contains("/__MACOSX/")
        || name.ends_with(".DS_Store")
        || name.ends_with("Thumbs.db")
        || name.ends_with("desktop.ini")
}

struct Entry {
    index: usize,
    /// Name with any single wrapping folder removed.
    name: String,
    size: u64,
    compressed: u64,
}

/// Reads and checks an uploaded quiz zip. Returns every problem found, or
/// the bundle if there are none. Nothing is partially accepted.
pub fn read_bundle<R: Read + Seek>(mut reader: R, limits: &Limits) -> Result<Bundle, Vec<Problem>> {
    let declared = declared_entry_count(&mut reader);
    let mut zip = ZipArchive::new(reader)
        .map_err(|e| vec![Problem::file("", "NOT_A_ZIP").detail(e.to_string())])?;

    // The zip reader keeps only the last of several entries with the same
    // name, so duplicates show up as fewer entries than the directory says.
    match declared {
        Some(n) if n as usize > limits.max_entries => {
            return Err(vec![Problem::file("", "TOO_MANY_FILES")]);
        }
        Some(n) if n as usize != zip.len() => {
            return Err(vec![Problem::file("", "DUPLICATE").detail(format!(
                "directory lists {n} entries, {} distinct",
                zip.len()
            ))]);
        }
        _ => {}
    }

    let entries = check_structure(&mut zip, limits)?;
    let mut budget = limits.max_total;

    // quiz.json
    let Some(manifest) = entries.iter().find(|e| e.name == "quiz.json") else {
        return Err(vec![Problem::file("quiz.json", "MISSING_QUIZ_JSON")]);
    };
    let raw = read_entry(
        &mut zip,
        manifest,
        limits.max_quiz_json,
        limits,
        &mut budget,
    )
    .map_err(|p| vec![p])?;
    let quiz = parse_manifest(&raw)?;

    // Content rules, then images.
    let mut problems = validate_quiz(&quiz);
    let mut images = BTreeMap::new();
    let mut wanted: Vec<&str> = quiz.image_refs().collect();
    wanted.sort_unstable();
    wanted.dedup();
    for path in wanted {
        if !MEDIA_NAME.is_match(path) {
            continue; // already reported by validate_quiz
        }
        let Some(entry) = entries.iter().find(|e| e.name == path) else {
            problems.push(Problem::file(path, "MISSING_IMAGE"));
            continue;
        };
        match read_entry(&mut zip, entry, limits.max_image, limits, &mut budget) {
            Ok(bytes) if !looks_like_image(&bytes) => {
                problems.push(Problem::file(path, "NOT_AN_IMAGE"));
            }
            Ok(bytes) => {
                images.insert(path.to_string(), bytes);
            }
            Err(p) => problems.push(p),
        }
    }

    if problems.is_empty() {
        Ok(Bundle { quiz, images })
    } else {
        Err(problems)
    }
}

/// The entry count from the end-of-central-directory record, read straight
/// from the bytes. `0xFFFF` (ZIP64) means more entries than we ever accept.
fn declared_entry_count<R: Read + Seek>(r: &mut R) -> Option<u64> {
    let len = r.seek(SeekFrom::End(0)).ok()?;
    let tail = len.min(22 + 65_535);
    r.seek(SeekFrom::Start(len - tail)).ok()?;
    let mut buf = vec![0u8; tail as usize];
    r.read_exact(&mut buf).ok()?;
    r.seek(SeekFrom::Start(0)).ok()?;
    let pos = buf.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let total = buf.get(pos + 10..pos + 12)?;
    Some(u16::from_le_bytes([total[0], total[1]]) as u64)
}

/// SEC-32: looks only at the central directory, never at content.
fn check_structure<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    limits: &Limits,
) -> Result<Vec<Entry>, Vec<Problem>> {
    if zip.len() > limits.max_entries {
        return Err(vec![Problem::file("", "TOO_MANY_FILES").detail(format!(
            "{} entries, limit {}",
            zip.len(),
            limits.max_entries
        ))]);
    }

    let mut problems = Vec::new();
    let mut raw = Vec::new();
    for index in 0..zip.len() {
        let f = match zip.by_index_raw(index) {
            Ok(f) => f,
            Err(e) => {
                problems.push(Problem::file("", "CORRUPT_ZIP").detail(e.to_string()));
                continue;
            }
        };
        let name = match f.name() {
            Ok(n) => n.replace('\\', "/"),
            Err(e) => {
                problems.push(Problem::file("", "UNSAFE_PATH").detail(e.to_string()));
                continue;
            }
        };
        if is_os_junk(&name) {
            continue;
        }
        if f.enclosed_name().is_none()
            || name.starts_with('/')
            || name.split('/').any(|p| p == "..")
        {
            problems.push(Problem::file(&name, "UNSAFE_PATH"));
            continue;
        }
        if f.is_symlink() {
            problems.push(Problem::file(&name, "SYMLINK"));
            continue;
        }
        if f.encrypted() {
            problems.push(Problem::file(&name, "ENCRYPTED"));
            continue;
        }
        if f.is_dir() {
            continue;
        }
        if !matches!(
            f.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            problems.push(Problem::file(&name, "UNSUPPORTED_COMPRESSION"));
            continue;
        }
        raw.push(Entry {
            index,
            name,
            size: f.size(),
            compressed: f.compressed_size(),
        });
    }

    // A zip made by right-clicking a folder wraps everything in that folder.
    if !raw.iter().any(|e| e.name == "quiz.json") {
        if let Some(prefix) = single_wrapping_folder(&raw) {
            for e in &mut raw {
                e.name = e.name[prefix.len()..].to_string();
            }
        }
    }

    let mut seen = HashSet::new();
    let mut declared_total: u64 = 0;
    let mut entries = Vec::new();
    for e in raw {
        let allowed =
            e.name == "quiz.json" || e.name == "README.txt" || MEDIA_NAME.is_match(&e.name);
        if !allowed {
            problems.push(Problem::file(&e.name, "NAME_NOT_ALLOWED"));
            continue;
        }
        if !seen.insert(e.name.to_lowercase()) {
            problems.push(Problem::file(&e.name, "DUPLICATE"));
            continue;
        }
        if e.size > limits.max_ratio.saturating_mul(e.compressed.max(1)) {
            problems.push(Problem::file(&e.name, "COMPRESSION_RATIO"));
            continue;
        }
        if e.name != "README.txt" {
            declared_total = declared_total.saturating_add(e.size);
        }
        entries.push(e);
    }
    if declared_total > limits.max_total {
        problems.push(Problem::file("", "TOO_LARGE"));
    }
    if problems.is_empty() {
        Ok(entries)
    } else {
        Err(problems)
    }
}

fn single_wrapping_folder(entries: &[Entry]) -> Option<String> {
    let first = entries.first()?.name.split_once('/')?.0.to_string();
    let prefix = format!("{first}/");
    let all_inside = entries.iter().all(|e| e.name.starts_with(&prefix));
    let has_manifest = entries
        .iter()
        .any(|e| e.name == format!("{prefix}quiz.json"));
    (all_inside && has_manifest).then_some(prefix)
}

/// SEC-31: reads through a counter capped at the limit + 1, so a header
/// that lies about the size cannot get more bytes past us.
fn read_entry<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    e: &Entry,
    file_limit: u64,
    limits: &Limits,
    budget: &mut u64,
) -> Result<Vec<u8>, Problem> {
    let ratio_limit = limits.max_ratio.saturating_mul(e.compressed.max(1));
    let limit = file_limit.min(ratio_limit).min(*budget);
    let f = zip
        .by_index(e.index)
        .map_err(|err| Problem::file(&e.name, "CORRUPT_ZIP").detail(err.to_string()))?;
    let mut buf = Vec::with_capacity(e.size.min(limit) as usize);
    f.take(limit + 1)
        .read_to_end(&mut buf)
        .map_err(|err| Problem::file(&e.name, "CORRUPT_ZIP").detail(err.to_string()))?;
    if buf.len() as u64 > limit {
        let code = if limit == *budget {
            "TOO_LARGE"
        } else if limit == ratio_limit {
            "COMPRESSION_RATIO"
        } else {
            "FILE_TOO_LARGE"
        };
        return Err(Problem::file(&e.name, code));
    }
    *budget -= buf.len() as u64;
    Ok(buf)
}

fn parse_manifest(raw: &[u8]) -> Result<QuizV1, Vec<Problem>> {
    let bad = |code: &str, detail: String| vec![Problem::file("quiz.json", code).detail(detail)];
    let header: Header =
        serde_json::from_slice(raw).map_err(|e| bad("INVALID_JSON", e.to_string()))?;
    if header.format != FORMAT || header.version == 0 {
        return Err(bad(
            "BAD_FORMAT",
            format!("{} v{}", header.format, header.version),
        ));
    }
    if header.version > CURRENT_VERSION {
        return Err(bad(
            "UNSUPPORTED_VERSION",
            format!("version {}", header.version),
        ));
    }
    // Version 1 is current; later versions add upgrade steps here.
    let file: QuizFileV1 =
        serde_json::from_slice(raw).map_err(|e| bad("INVALID_JSON", e.to_string()))?;
    Ok(file.quiz)
}

/// The editor's rules (shared validator in crates/engine), plus the fields
/// only a file can get wrong.
fn validate_quiz(quiz: &QuizV1) -> Vec<Problem> {
    let mut out: Vec<Problem> = quiz
        .to_snapshot()
        .validate()
        .into_iter()
        .map(|QuizProblem { question, error }| Problem {
            file: "quiz.json".into(),
            question,
            field: Some(field_of(error).into()),
            problem: format!("{error:?}").to_uppercase_snake(),
            detail: Some(error.to_string()),
        })
        .collect();
    if quiz.description.chars().count() > 1000 {
        out.push(field_problem(None, "description", "DESCRIPTION_LENGTH"));
    }
    if let Some(c) = &quiz.cover {
        if !MEDIA_NAME.is_match(c) {
            out.push(field_problem(None, "cover", "BAD_IMAGE_PATH"));
        }
    }
    for (i, q) in quiz.questions.iter().enumerate() {
        if let Some(img) = &q.image {
            if !MEDIA_NAME.is_match(img) {
                out.push(field_problem(Some(i), "image", "BAD_IMAGE_PATH"));
            }
        }
        if q.image_alt
            .as_ref()
            .is_some_and(|a| a.chars().count() > 300)
        {
            out.push(field_problem(Some(i), "imageAlt", "ALT_TEXT_LENGTH"));
        }
    }
    out
}

fn field_problem(question: Option<usize>, field: &str, code: &str) -> Problem {
    Problem {
        file: "quiz.json".into(),
        question,
        field: Some(field.into()),
        problem: code.into(),
        detail: None,
    }
}

fn field_of(e: ValidationError) -> &'static str {
    use ValidationError as E;
    match e {
        E::TitleLength => "title",
        E::NoQuestions => "questions",
        E::QuestionTextLength | E::BlankMarker => "text",
        E::TimeLimit => "timeLimitS",
        E::Points => "points",
        E::MarkdownLength => "markdown",
        E::OptionCount
        | E::TrueFalseOptionCount
        | E::OptionTextLength
        | E::DuplicateOptionId
        | E::NoCorrectOption
        | E::TooManyCorrectOptions
        | E::UnexpectedOptions => "options",
        E::AcceptedAnswerCount | E::AcceptedAnswerLength => "acceptedAnswers",
    }
}

trait UpperSnake {
    fn to_uppercase_snake(&self) -> String;
}

impl UpperSnake for String {
    /// `TimeLimit` → `TIME_LIMIT`, matching the editor's error codes.
    fn to_uppercase_snake(&self) -> String {
        let mut out = String::new();
        for (i, c) in self.chars().enumerate() {
            if c.is_uppercase() && i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_uppercase());
        }
        out
    }
}

/// Magic bytes only; the media pipeline does the real decode.
fn looks_like_image(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8, 0xFF])
        || b.starts_with(b"\x89PNG\r\n\x1a\n")
        || (b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP")
}
