//! The YAML form of a quiz: readable code blocks and a lossless round trip
//! with the JSON form.

use std::io::Cursor;

use myquiz_bundle::format::QuizFileV1;
use myquiz_bundle::{
    read_bundle, read_yaml, template, to_yaml, write_bundle_as, Bundle, Limits, Manifest, Problem,
};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR fake but magic is right";

fn codes(r: Result<Bundle, Vec<Problem>>) -> Vec<String> {
    r.expect_err("should be refused")
        .into_iter()
        .map(|p| p.problem)
        .collect()
}

#[test]
fn plain_yaml_round_trips_with_code_as_a_block() {
    let f = template();
    let yaml = to_yaml(&f);
    // The code question's markdown is a literal block, not one escaped line.
    assert!(yaml.contains("markdown: |"), "{yaml}");
    assert!(yaml.contains("\n    int sum = 0;\n") || yaml.contains("int sum = 0;\n"));
    assert!(!yaml.contains("\\n    int sum"), "{yaml}");
    let b = read_yaml(yaml.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(b.quiz, f.quiz);
    assert!(b.images.is_empty());
}

#[test]
fn awkward_strings_survive() {
    let mut f = template();
    let q = &mut f.quiz.questions[4];
    q.markdown =
        Some("```c\n\tchar *s = \"a: b # not a comment\";\n  \n  trailing  \n```\n\n".into());
    q.text = "yes".into(); // would be a boolean if left unquoted
    f.quiz.questions[0].options[0].text = "- 1".into();
    f.quiz.questions[1].options[0].text = "007".into();
    f.quiz.description = "Dòng 1\nDòng 2: ‘x’ & {y}".into();
    let b = read_yaml(to_yaml(&f).as_bytes(), &Limits::default()).unwrap();
    assert_eq!(b.quiz, f.quiz);
}

#[test]
fn yaml_inside_a_zip_round_trips_with_images() {
    let mut f = template();
    f.quiz.questions[0].image = Some("media/q1.png".into());
    let zip = write_bundle_as(
        Cursor::new(Vec::new()),
        &f,
        [("media/q1.png".to_string(), PNG)],
        Manifest::Yaml,
    )
    .unwrap()
    .into_inner();
    let mut a = zip::ZipArchive::new(Cursor::new(&zip)).unwrap();
    assert!(a.by_name("quiz.yaml").is_ok());
    assert!(a.by_name("quiz.json").is_err());
    let b = read_bundle(Cursor::new(&zip), &Limits::default()).unwrap();
    assert_eq!(b.quiz, f.quiz);
    assert_eq!(b.images["media/q1.png"], PNG);
}

#[test]
fn json_and_yaml_describe_the_same_quiz() {
    let f = template();
    let from_json: QuizFileV1 = serde_json::from_slice(&Manifest::Json.to_bytes(&f)).unwrap();
    let from_yaml = read_yaml(&Manifest::Yaml.to_bytes(&f), &Limits::default()).unwrap();
    assert_eq!(from_json.quiz, from_yaml.quiz);
}

#[test]
fn hand_written_yaml_is_accepted() {
    let yaml = r#"
format: myquiz.quiz
version: 1
quiz:
  title: Arrays
  questions:
    - type: single
      text: What does this print?
      markdown: |
        ```c
        int a[3] = {1, 2, 3};
        printf("%d", a[1]);
        ```
      timeLimitS: 20
      points: 1000
      options:
        - { text: "1", correct: false }
        - { text: "2", correct: true }
    - type: fill_blank
      text: The first index of a C array is ___.
      timeLimitS: 20
      points: 1000
      acceptedAnswers: ["0", zero]
"#;
    let b = read_yaml(yaml.as_bytes(), &Limits::default()).unwrap();
    assert_eq!(b.quiz.questions.len(), 2);
    assert_eq!(
        b.quiz.questions[0].markdown.as_deref(),
        Some("```c\nint a[3] = {1, 2, 3};\nprintf(\"%d\", a[1]);\n```\n")
    );
    assert_eq!(b.quiz.questions[1].accepted_answers, ["0", "zero"]);
}

#[test]
fn yaml_problems() {
    let l = Limits::default();
    assert_eq!(codes(read_yaml(b"quiz: [unclosed", &l)), ["INVALID_YAML"]);
    assert_eq!(codes(read_yaml(b"just text", &l)), ["INVALID_YAML"]);
    assert_eq!(
        codes(read_yaml(b"format: other\nversion: 1\n", &l)),
        ["BAD_FORMAT"]
    );
    assert_eq!(
        codes(read_yaml(b"format: myquiz.quiz\nversion: 9\n", &l)),
        ["UNSUPPORTED_VERSION"]
    );
    let big = vec![b'#'; (l.max_quiz_json + 1) as usize];
    assert_eq!(codes(read_yaml(&big, &l)), ["FILE_TOO_LARGE"]);

    // A plain YAML file has nowhere to keep images.
    let mut f = template();
    f.quiz.questions[0].image = Some("media/q1.png".into());
    let r = read_yaml(to_yaml(&f).as_bytes(), &l).unwrap_err();
    assert_eq!(r[0].problem, "IMAGE_NEEDS_ZIP");
    assert_eq!(r[0].file, "media/q1.png");

    // Content rules carry the YAML file name.
    let mut f = template();
    f.quiz.questions[0].time_limit_s = 0;
    let r = read_yaml(to_yaml(&f).as_bytes(), &l).unwrap_err();
    assert_eq!(r[0].file, "quiz.yaml");
    assert_eq!(r[0].question, Some(0));
}

#[test]
fn yaml_alias_bombs_are_refused() {
    let mut bomb = String::from("format: myquiz.quiz\nversion: 1\na: &a [x,x,x,x,x,x,x,x,x,x]\n");
    for i in 0..9 {
        let prev = if i == 0 {
            "a".to_string()
        } else {
            format!("b{}", i - 1)
        };
        bomb.push_str(&format!("b{i}: &b{i} [*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev},*{prev}]\n"));
    }
    bomb.push_str("quiz: *b8\n");
    assert!(read_yaml(bomb.as_bytes(), &Limits::default()).is_err());
}

#[test]
fn a_zip_with_two_quiz_files_is_refused() {
    let f = template();
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, m) in [("quiz.json", Manifest::Json), ("quiz.yaml", Manifest::Yaml)] {
        z.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut z, &m.to_bytes(&f)).unwrap();
    }
    let zip = z.finish().unwrap().into_inner();
    assert_eq!(
        codes(read_bundle(Cursor::new(zip), &Limits::default())),
        ["MULTIPLE_QUIZ_FILES"]
    );
}
