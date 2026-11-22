//! Import safety (SEC-31..33) and the export/import round trip (NFR-20).

use std::io::{Cursor, Write};

use myquiz_bundle::format::{QuizFileV1, CURRENT_VERSION};
use myquiz_bundle::{read_bundle, template, write_bundle, Bundle, Limits, Problem};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR fake but magic is right";
const WEBP: &[u8] = b"RIFF\x10\0\0\0WEBPVP8 rest";

fn quiz_with_images() -> QuizFileV1 {
    let mut f = template();
    f.quiz.cover = Some("media/cover.webp".into());
    f.quiz.questions[0].image = Some("media/q1.png".into());
    f.quiz.questions[0].image_alt = Some("Sơ đồ mạng".into());
    f
}

fn export(f: &QuizFileV1, images: &[(&str, &[u8])]) -> Vec<u8> {
    let imgs = images.iter().map(|(p, b)| (p.to_string(), b.to_vec()));
    write_bundle(Cursor::new(Vec::new()), f, imgs)
        .unwrap()
        .into_inner()
}

/// Builds a zip by hand: (name, bytes, deflate?).
fn raw_zip(files: &[(&str, Vec<u8>, bool)]) -> Vec<u8> {
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, deflate) in files {
        let m = if *deflate {
            CompressionMethod::Deflated
        } else {
            CompressionMethod::Stored
        };
        z.start_file(
            *name,
            SimpleFileOptions::default()
                .compression_method(m)
                .large_file(bytes.len() > u32::MAX as usize),
        )
        .unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn json(f: &QuizFileV1) -> Vec<u8> {
    serde_json::to_vec(f).unwrap()
}

fn import(bytes: &[u8]) -> Result<Bundle, Vec<Problem>> {
    read_bundle(Cursor::new(bytes), &Limits::default())
}

fn codes(r: Result<Bundle, Vec<Problem>>) -> Vec<String> {
    r.expect_err("should be refused")
        .into_iter()
        .map(|p| p.problem)
        .collect()
}

#[test]
fn export_then_import_gives_the_same_quiz() {
    let f = quiz_with_images();
    let zip = export(&f, &[("media/cover.webp", WEBP), ("media/q1.png", PNG)]);
    let b = import(&zip).unwrap();
    assert_eq!(b.quiz, f.quiz);
    assert_eq!(b.images["media/cover.webp"], WEBP);
    assert_eq!(b.images["media/q1.png"], PNG);
    assert_eq!(b.quiz.to_snapshot().validate(), vec![]);

    // Exporting the imported quiz again gives the same quiz.json content.
    let again = export(
        &QuizFileV1::new(b.quiz.clone(), None),
        &[("media/cover.webp", WEBP), ("media/q1.png", PNG)],
    );
    assert_eq!(import(&again).unwrap(), b);
}

#[test]
fn images_are_stored_and_json_is_deflated() {
    let zip = export(
        &quiz_with_images(),
        &[("media/cover.webp", WEBP), ("media/q1.png", PNG)],
    );
    let mut a = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
    assert_eq!(
        a.by_name("quiz.json").unwrap().compression(),
        CompressionMethod::Deflated
    );
    assert_eq!(
        a.by_name("media/q1.png").unwrap().compression(),
        CompressionMethod::Stored
    );
    assert!(a.by_name("README.txt").is_ok());
}

#[test]
fn folder_zips_and_os_junk_are_accepted() {
    // Right-click "Compress" on macOS: everything inside one folder, plus
    // __MACOSX and .DS_Store files.
    let f = quiz_with_images();
    let zip = raw_zip(&[
        ("my quiz/quiz.json", json(&f), true),
        ("my quiz/media/cover.webp", WEBP.to_vec(), false),
        ("my quiz/media/q1.png", PNG.to_vec(), false),
        ("my quiz/.DS_Store", vec![0; 10], false),
        ("__MACOSX/my quiz/._quiz.json", vec![0; 10], false),
    ]);
    assert_eq!(import(&zip).unwrap().quiz, f.quiz);
}

#[test]
fn unreferenced_files_are_ignored_but_must_be_allowed_names() {
    let f = template();
    let ok = raw_zip(&[
        ("quiz.json", json(&f), true),
        ("media/unused.png", PNG.to_vec(), false),
    ]);
    assert!(import(&ok).unwrap().images.is_empty());

    for bad in [
        "evil.exe",
        "media/x.svg",
        "media/inner.zip",
        "media/UPPER.png",
        "other/a.png",
        "media/sub/a.png",
    ] {
        let zip = raw_zip(&[("quiz.json", json(&f), true), (bad, PNG.to_vec(), false)]);
        assert_eq!(codes(import(&zip)), ["NAME_NOT_ALLOWED"], "{bad}");
    }
}

#[test]
fn unsafe_paths_and_symlinks_are_refused() {
    let f = template();
    for bad in [
        "../quiz.json",
        "media/../../etc/passwd",
        "/etc/passwd",
        "..\\..\\x.png",
    ] {
        let zip = raw_zip(&[("quiz.json", json(&f), true), (bad, b"x".to_vec(), false)]);
        assert_eq!(codes(import(&zip)), ["UNSAFE_PATH"], "{bad}");
    }

    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    z.start_file("quiz.json", SimpleFileOptions::default())
        .unwrap();
    z.write_all(&json(&f)).unwrap();
    z.add_symlink(
        "media/link.png",
        "/etc/passwd",
        SimpleFileOptions::default(),
    )
    .unwrap();
    let zip = z.finish().unwrap().into_inner();
    assert_eq!(codes(import(&zip)), ["SYMLINK"]);
}

#[test]
fn duplicate_names_are_refused() {
    // Zip writers refuse duplicates, so patch a second name into a copy.
    let f = template();
    let mut zip = raw_zip(&[("quiz.json", json(&f), true), ("quiz.jsoo", json(&f), true)]);
    let from = b"quiz.jsoo";
    let mut i = 0;
    while let Some(pos) = zip[i..].windows(from.len()).position(|w| w == from) {
        zip[i + pos..i + pos + from.len()].copy_from_slice(b"quiz.json");
        i += pos + from.len();
    }
    assert_eq!(codes(import(&zip)), ["DUPLICATE"]);
}

#[test]
fn zip_bombs_are_refused_before_inflating() {
    let f = template();
    // 50 MB of zeros deflates to about 50 KB: a ratio of about 1000:1.
    let zeros = vec![0u8; 50 * 1024 * 1024];
    let mut q = f.clone();
    q.quiz.questions[0].image = Some("media/bomb.png".into());
    let zip = raw_zip(&[
        ("quiz.json", json(&q), true),
        ("media/bomb.png", zeros, true),
    ]);
    assert!(zip.len() < 1024 * 1024, "the bomb itself is small");
    assert_eq!(codes(import(&zip)), ["COMPRESSION_RATIO"]);
}

#[test]
fn size_limits() {
    let f = template();
    // Too many entries.
    let mut files = vec![("quiz.json".to_string(), json(&f))];
    for i in 0..250 {
        files.push((format!("media/f{i}.png"), PNG.to_vec()));
    }
    let refs: Vec<_> = files
        .iter()
        .map(|(n, b)| (n.as_str(), b.clone(), false))
        .collect();
    assert_eq!(codes(import(&raw_zip(&refs))), ["TOO_MANY_FILES"]);

    // quiz.json over 1 MB (incompressible, so not a ratio problem).
    let mut big = f.clone();
    let mut seed: u32 = 1;
    big.quiz.description = (0..1_200_000)
        .map(|_| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (b'a' + (seed >> 16) as u8 % 26) as char
        })
        .collect();
    let zip = raw_zip(&[("quiz.json", json(&big), true)]);
    assert_eq!(codes(import(&zip)), ["FILE_TOO_LARGE"]);

    // An image over 5 MB.
    let mut q = f.clone();
    q.quiz.questions[0].image = Some("media/huge.png".into());
    let mut huge = PNG.to_vec();
    huge.resize(6 * 1024 * 1024, 7);
    let zip = raw_zip(&[
        ("quiz.json", json(&q), true),
        ("media/huge.png", huge, false),
    ]);
    assert_eq!(codes(import(&zip)), ["FILE_TOO_LARGE"]);

    // Total budget: tighten the limit to show the counter works across files.
    let limits = Limits {
        max_total: 4 * 1024,
        ..Limits::default()
    };
    let mut q = f.clone();
    q.quiz.questions[0].image = Some("media/a.png".into());
    let mut img = PNG.to_vec();
    img.resize(3 * 1024, 1);
    let zip = raw_zip(&[("quiz.json", json(&q), true), ("media/a.png", img, false)]);
    let problems = read_bundle(Cursor::new(zip), &limits).unwrap_err();
    assert_eq!(problems[0].problem, "TOO_LARGE");
}

#[test]
fn not_a_zip_and_missing_manifest() {
    assert_eq!(
        codes(import(b"PK\x03\x04 definitely not a zip")),
        ["NOT_A_ZIP"]
    );
    assert_eq!(codes(import(b"")), ["NOT_A_ZIP"]);
    let zip = raw_zip(&[("README.txt", b"hi".to_vec(), true)]);
    assert_eq!(codes(import(&zip)), ["MISSING_QUIZ_JSON"]);
}

#[test]
fn versions_and_format() {
    let mut f = template();
    f.version = CURRENT_VERSION + 1;
    assert_eq!(
        codes(import(&raw_zip(&[("quiz.json", json(&f), true)]))),
        ["UNSUPPORTED_VERSION"]
    );
    let mut f = template();
    f.format = "kahoot".into();
    assert_eq!(
        codes(import(&raw_zip(&[("quiz.json", json(&f), true)]))),
        ["BAD_FORMAT"]
    );
    let zip = raw_zip(&[("quiz.json", b"{ not json".to_vec(), true)]);
    let p = import(&zip).unwrap_err();
    assert_eq!(p[0].problem, "INVALID_JSON");
    assert!(p[0].detail.as_ref().unwrap().contains("line 1"));
}

#[test]
fn every_problem_is_reported_at_once() {
    let mut f = template();
    f.quiz.questions[0].time_limit_s = 3;
    f.quiz.questions[1]
        .options
        .iter_mut()
        .for_each(|o| o.correct = false);
    f.quiz.questions[2].image = Some("media/missing.png".into());
    f.quiz.questions[3].image = Some("../../etc/passwd".into());
    f.quiz.cover = Some("media/notimage.png".into());
    let zip = raw_zip(&[
        ("quiz.json", json(&f), true),
        ("media/notimage.png", b"GIF89a".to_vec(), false),
    ]);
    let problems = import(&zip).unwrap_err();
    let got: Vec<_> = problems
        .iter()
        .map(|p| {
            (
                p.file.as_str(),
                p.question,
                p.field.as_deref(),
                p.problem.as_str(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("quiz.json", Some(0), Some("timeLimitS"), "TIME_LIMIT"),
            ("quiz.json", Some(1), Some("options"), "NO_CORRECT_OPTION"),
            ("quiz.json", Some(3), Some("image"), "BAD_IMAGE_PATH"),
            ("media/missing.png", None, None, "MISSING_IMAGE"),
            ("media/notimage.png", None, None, "NOT_AN_IMAGE"),
        ]
    );
}

#[test]
fn problems_serialise_for_the_api() {
    let p = import(b"nope").unwrap_err();
    let v = serde_json::to_value(&p[0]).unwrap();
    assert_eq!(v["problem"], "NOT_A_ZIP");
    assert_eq!(v["file"], "");
}

#[test]
fn corrupted_zips_never_panic() {
    // A small fuzz pass: flip bytes in a valid export many times; the reader
    // must return Ok or a problem list, never panic or hang.
    let zip = export(
        &quiz_with_images(),
        &[("media/cover.webp", WEBP), ("media/q1.png", PNG)],
    );
    let mut seed: u64 = 42;
    let mut next = || {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) as usize
    };
    for _ in 0..3000 {
        let mut z = zip.clone();
        for _ in 0..1 + next() % 8 {
            let i = next() % z.len();
            z[i] = next() as u8;
        }
        if next() % 10 == 0 {
            z.truncate(next() % z.len());
        }
        let _ = import(&z);
    }
}
