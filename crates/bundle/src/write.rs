use std::io::{Seek, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::format::{OptionV1, QuestionV1, QuizFileV1, QuizV1};

pub const README: &str = "\
This zip is a quiz exported from myquiz.

Keep it to reuse the quiz later: quizzes are deleted from the server 3 weeks
after they were last edited or played. To bring it back, open myquiz, go to
My quizzes and choose Import zip. You can edit quiz.json in a text editor;
images go in the media folder.

Zip này là một bộ câu hỏi được xuất từ myquiz.

Hãy giữ lại để dùng sau: bộ câu hỏi bị xoá khỏi máy chủ 3 tuần sau lần sửa
hoặc lần chơi gần nhất. Để khôi phục, mở myquiz, vào Bộ câu hỏi của tôi và
chọn Nhập tệp zip. Bạn có thể sửa quiz.json bằng trình soạn thảo văn bản;
hình ảnh nằm trong thư mục media.
";

/// Writes a bundle: `quiz.json` deflated, images stored as they are (they
/// are already compressed), and a README. `images` yields
/// `(path inside the zip, bytes)`, e.g. `("media/q1.webp", ...)`.
pub fn write_bundle<W, I, B>(out: W, file: &QuizFileV1, images: I) -> zip::result::ZipResult<W>
where
    W: Write + Seek,
    I: IntoIterator<Item = (String, B)>,
    B: AsRef<[u8]>,
{
    let mut zip = ZipWriter::new(out);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);

    zip.start_file("quiz.json", deflated)?;
    let json = serde_json::to_vec_pretty(file).expect("quiz.json serialises");
    zip.write_all(&json)?;

    for (path, bytes) in images {
        zip.start_file(path, stored)?;
        zip.write_all(bytes.as_ref())?;
    }

    zip.start_file("README.txt", deflated)?;
    zip.write_all(README.as_bytes())?;
    zip.finish()
}

/// `myquiz-<title-slug>-<yyyymmdd>.zip`. The slug keeps ASCII letters and
/// digits, drops Vietnamese marks (đ → d) and joins words with `-`.
pub fn file_name(title: &str, yyyymmdd: &str) -> String {
    let folded = myquiz_engine::matching::normalize(title, true);
    let mut slug = String::new();
    for c in folded.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_end_matches('-').chars().take(50).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        format!("myquiz-quiz-{yyyymmdd}.zip")
    } else {
        format!("myquiz-{slug}-{yyyymmdd}.zip")
    }
}

/// The downloadable example for `GET /quiz-template.zip`: one question of
/// each type, no images.
pub fn template() -> QuizFileV1 {
    use myquiz_engine::QuestionKind as K;
    let q = |kind, text: &str, options: Vec<(&str, bool)>, accepted: Vec<&str>| QuestionV1 {
        kind,
        text: text.into(),
        image: None,
        image_alt: None,
        time_limit_s: kind.default_time_limit_s(),
        points: 1000,
        options: options
            .into_iter()
            .map(|(t, c)| OptionV1 {
                text: t.into(),
                correct: c,
            })
            .collect(),
        accepted_answers: accepted.into_iter().map(Into::into).collect(),
        ignore_diacritics: kind == K::FillBlank,
    };
    QuizFileV1::new(
        QuizV1 {
            title: "Mẫu bộ câu hỏi / Quiz template".into(),
            description: "Sửa tệp này rồi nhập lại. Edit this file, then import it.".into(),
            cover: None,
            questions: vec![
                q(
                    K::Single,
                    "Tầng nào định tuyến gói tin? / Which layer routes packets?",
                    vec![
                        ("Network", true),
                        ("Transport", false),
                        ("Data link", false),
                    ],
                    vec![],
                ),
                q(
                    K::Multi,
                    "Chọn các số nguyên tố / Pick the prime numbers",
                    vec![("2", true), ("3", true), ("4", false), ("9", false)],
                    vec![],
                ),
                q(
                    K::TrueFalse,
                    "TCP đảm bảo thứ tự gói tin. / TCP keeps packets in order.",
                    vec![("Đúng / True", true), ("Sai / False", false)],
                    vec![],
                ),
                q(
                    K::FillBlank,
                    "Thủ đô của Việt Nam là ___. (The capital of Vietnam)",
                    vec![],
                    vec!["Hà Nội", "Hanoi"],
                ),
            ],
        },
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(
            file_name("Chương 3 – Mạng máy tính", "20261206"),
            "myquiz-chuong-3-mang-may-tinh-20261206.zip"
        );
        assert_eq!(
            file_name("Đề thi!!", "20261206"),
            "myquiz-de-thi-20261206.zip"
        );
        assert_eq!(file_name("ーー", "20261206"), "myquiz-quiz-20261206.zip");
        assert!(file_name(&"a ".repeat(100), "20261206").len() <= 70);
    }

    #[test]
    fn template_is_valid() {
        assert_eq!(template().quiz.to_snapshot().validate(), vec![]);
    }
}
