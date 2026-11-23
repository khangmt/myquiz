//! The quiz editor API and POST /games against a real PostgreSQL and
//! Redis. Skipped unless DATABASE_URL and REDIS_URL are set.

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use fred::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use super::tests::{json_body, send, unique, Env};
use crate::live::keys;

/// A user with a login session, as the sign-in flow would leave it.
struct User {
    id: Uuid,
    cookie: String,
    csrf: String,
}

async fn user(env: &Env) -> User {
    let email = format!("{}@uit.edu.vn", unique());
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO users (email, display_name) VALUES ($1, 'Test') RETURNING id",
    )
    .bind(&email)
    .fetch_one(&env.db)
    .await
    .unwrap();
    let sid = format!("{}{}", unique(), unique());
    let csrf = unique();
    let _: () = env
        .redis
        .hset(
            keys::login_session(&sid),
            [("user_id", id.to_string()), ("csrf", csrf.clone())],
        )
        .await
        .unwrap();
    User {
        id,
        cookie: format!("sid={sid}"),
        csrf,
    }
}

impl User {
    fn req(&self, method: &str, uri: &str, body: Option<Value>) -> Request<Body> {
        let mut b = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::COOKIE, &self.cookie)
            .header("X-CSRF-Token", &self.csrf);
        let body = match body {
            Some(v) => {
                b = b.header(header::CONTENT_TYPE, "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        b.body(body).unwrap()
    }

    fn req_v(&self, method: &str, uri: &str, version: i64, body: Value) -> Request<Body> {
        let mut r = self.req(method, uri, Some(body));
        r.headers_mut()
            .insert(header::IF_MATCH, format!("\"{version}\"").parse().unwrap());
        r
    }
}

fn question(text: &str) -> Value {
    json!({
        "type": "single",
        "text": text,
        "timeLimitS": 20,
        "points": 1000,
        "options": [
            { "text": "Hà Nội", "correct": true },
            { "text": "Huế", "correct": false },
        ],
    })
}

fn quiz(title: &str, questions: Vec<Value>) -> Value {
    json!({ "title": title, "description": "Địa lý", "questions": questions })
}

#[tokio::test]
async fn create_save_publish_and_play() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;

    // Create a draft.
    let res = send(
        &env.app,
        lan.req("POST", "/api/v1/quizzes", Some(quiz("  Thủ đô  ", vec![]))),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(res.headers()[header::ETAG], "\"1\"");
    let doc = json_body(res).await;
    let id = doc["id"].as_str().unwrap().to_string();
    assert_eq!(doc["title"], "Thủ đô");
    assert_eq!(doc["status"], "draft");
    assert_eq!(doc["visibility"], "private");
    assert!(doc["expiresAt"].as_str().unwrap().len() >= 20);

    // A game needs a published quiz.
    let res = send(
        &env.app,
        lan.req("POST", "/api/v1/games", Some(json!({ "quizId": id }))),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(json_body(res).await["error"]["code"], "QUIZ_NOT_PUBLISHED");

    // Publishing an empty quiz lists the problem.
    let res = send(
        &env.app,
        lan.req("POST", &format!("/api/v1/quizzes/{id}/publish"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "QUIZ_INVALID");
    assert_eq!(body["problems"][0]["code"], "NO_QUESTIONS");

    // Save without If-Match is refused; with it, the version moves on.
    let path = format!("/api/v1/quizzes/{id}");
    let mut half = question("");
    half["options"][0]["text"] = json!("");
    let body = quiz("Thủ đô", vec![question("Thủ đô của Việt Nam?"), half]);
    let res = send(&env.app, lan.req("PUT", &path, Some(body.clone()))).await;
    assert_eq!(res.status(), StatusCode::PRECONDITION_REQUIRED);
    let res = send(&env.app, lan.req_v("PUT", &path, 1, body.clone())).await;
    assert_eq!(res.status(), StatusCode::OK);
    let doc = json_body(res).await;
    assert_eq!(doc["version"], 2);
    assert_eq!(doc["questions"].as_array().unwrap().len(), 2);
    assert_eq!(doc["questions"][0]["options"][0]["text"], "Hà Nội");
    assert_eq!(doc["questions"][1]["options"][0]["text"], "");

    // A second tab still on version 1 cannot overwrite it.
    let res = send(&env.app, lan.req_v("PUT", &path, 1, body)).await;
    assert_eq!(res.status(), StatusCode::PRECONDITION_FAILED);
    assert_eq!(json_body(res).await["error"]["code"], "VERSION_CONFLICT");

    // The half-written second question blocks publishing, by index.
    let res = send(&env.app, lan.req("POST", &format!("{path}/publish"), None)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let problems = json_body(res).await["problems"].clone();
    assert!(
        problems
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["question"] == 1),
        "{problems}"
    );

    // Fix it, publish, start a game.
    let fixed = quiz(
        "Thủ đô",
        vec![question("Thủ đô của Việt Nam?"), question("Lại?")],
    );
    let res = send(&env.app, lan.req_v("PUT", &path, 2, fixed)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(&env.app, lan.req("POST", &format!("{path}/publish"), None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_body(res).await["status"], "published");

    let res = send(
        &env.app,
        lan.req("POST", "/api/v1/games", Some(json!({ "quizId": id }))),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let game = json_body(res).await;
    let pin = game["pin"].as_str().unwrap().to_string();
    assert_eq!(pin.len(), 6);
    assert_eq!(
        game["joinUrl"],
        json!(format!("http://localhost:8080/join?pin={pin}"))
    );

    // Anyone can check the PIN; the snapshot is in Redis and the DB.
    let res = send(
        &env.app,
        Request::get(format!("/api/v1/games/pin/{pin}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let check = json_body(res).await;
    assert_eq!(check["title"], "Thủ đô");
    assert_eq!(check["locked"], false);
    let (host, snap_title): (Uuid, String) =
        sqlx::query_as("SELECT host_id, quiz_snapshot->>'title' FROM game_sessions WHERE id = $1")
            .bind(game["sessionId"].as_str().unwrap().parse::<Uuid>().unwrap())
            .fetch_one(&env.db)
            .await
            .unwrap();
    assert_eq!((host, snap_title.as_str()), (lan.id, "Thủ đô"));

    // Breaking a published quiz sends it back to draft.
    let res = send(&env.app, lan.req_v("PUT", &path, 3, quiz("Thủ đô", vec![]))).await;
    assert_eq!(json_body(res).await["status"], "draft");
}

#[tokio::test]
async fn only_the_owner_sees_a_quiz() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;
    let minh = user(&env).await;
    let res = send(
        &env.app,
        lan.req(
            "POST",
            "/api/v1/quizzes",
            Some(quiz("Riêng", vec![question("Q?")])),
        ),
    )
    .await;
    let id = json_body(res).await["id"].as_str().unwrap().to_string();
    let path = format!("/api/v1/quizzes/{id}");

    for req in [
        minh.req("GET", &path, None),
        minh.req_v("PUT", &path, 1, quiz("Của tôi", vec![])),
        minh.req("DELETE", &path, None),
        minh.req("POST", &format!("{path}/duplicate"), None),
        minh.req("POST", &format!("{path}/publish"), None),
        minh.req("POST", "/api/v1/games", Some(json!({ "quizId": id }))),
        minh.req("GET", "/api/v1/quizzes/not-a-uuid", None),
    ] {
        let res = send(&env.app, req).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_body(res).await["error"]["code"], "QUIZ_NOT_FOUND");
    }

    // An admin can read it but not change it.
    sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
        .bind(minh.id)
        .execute(&env.db)
        .await
        .unwrap();
    assert_eq!(
        send(&env.app, minh.req("GET", &path, None)).await.status(),
        StatusCode::OK
    );
    let res = send(
        &env.app,
        minh.req_v("PUT", &path, 1, quiz("Của tôi", vec![])),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Someone else's image cannot be attached.
    let media: Uuid = sqlx::query_scalar(
        "INSERT INTO media (owner_id, storage_key, mime, bytes, width, height) \
         VALUES ($1, $2, 'image/webp', 10, 1, 1) RETURNING id",
    )
    .bind(minh.id)
    .bind(unique())
    .fetch_one(&env.db)
    .await
    .unwrap();
    let mut q = question("Ảnh?");
    q["mediaId"] = json!(media.to_string());
    let res = send(&env.app, lan.req_v("PUT", &path, 1, quiz("Riêng", vec![q]))).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(res).await;
    assert_eq!(body["problems"][0]["code"], "MEDIA_NOT_FOUND");
    assert_eq!(body["problems"][0]["question"], 0);

    // CSRF is required on every change.
    let mut req = lan.req("DELETE", &path, None);
    req.headers_mut().remove("X-CSRF-Token");
    assert_eq!(send(&env.app, req).await.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn list_search_duplicate_delete() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;
    for title in ["Lịch sử 10%", "Toán lớp 5", "toán nâng cao"] {
        let res = send(
            &env.app,
            lan.req(
                "POST",
                "/api/v1/quizzes",
                Some(quiz(title, vec![question("Q?")])),
            ),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
    }
    let list = |q: &str| {
        let uri = format!("/api/v1/quizzes?q={}", urlencode(q));
        lan.req("GET", &uri, None)
    };
    let page = json_body(send(&env.app, list("")).await).await;
    assert_eq!(page["total"], 3);
    assert_eq!(page["items"][0]["title"], "toán nâng cao"); // newest first
    assert_eq!(page["items"][0]["questionCount"], 1);

    let page = json_body(send(&env.app, list("TOÁN")).await).await;
    assert_eq!(page["total"], 2);
    // % is a literal, not a wildcard.
    let page = json_body(send(&env.app, list("%")).await).await;
    assert_eq!(page["total"], 1);

    // Duplicate.
    let first = page["items"][0]["id"].as_str().unwrap().to_string();
    let res = send(
        &env.app,
        lan.req("POST", &format!("/api/v1/quizzes/{first}/duplicate"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let copy = json_body(res).await;
    assert_eq!(copy["title"], "Lịch sử 10% (copy)");
    assert_eq!(copy["questions"][0]["text"], "Q?");
    assert_ne!(copy["id"], json!(first));

    // Delete hides it and writes an audit entry.
    let res = send(
        &env.app,
        lan.req("DELETE", &format!("/api/v1/quizzes/{first}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &env.app,
        lan.req("GET", &format!("/api/v1/quizzes/{first}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let page = json_body(send(&env.app, list("")).await).await;
    assert_eq!(page["total"], 3);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'quiz_deleted' AND target_id = $1",
    )
    .bind(&first)
    .fetch_one(&env.db)
    .await
    .unwrap();
    assert_eq!(audits, 1);
}

#[tokio::test]
async fn pin_checks_are_rate_limited() {
    let env = crate::api_env_or_skip!("");
    let ip = format!("10.9.{}.{}", rand::random::<u8>(), rand::random::<u8>());
    let mut last = StatusCode::OK;
    for _ in 0..21 {
        let res = send(
            &env.app,
            Request::get("/api/v1/games/pin/000000")
                .header("x-forwarded-for", &ip)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        last = res.status();
        if last == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
        assert_eq!(last, StatusCode::NOT_FOUND);
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn multipart(user: &User, parts: &[(&str, &[u8])]) -> Request<Body> {
    multipart_to(user, "/api/v1/media", parts)
}

fn multipart_to(user: &User, uri: &str, parts: &[(&str, &[u8])]) -> Request<Body> {
    let boundary = "myquiz-test-boundary";
    let mut body = Vec::new();
    for (name, bytes) in parts {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; \
                 filename=\"x.png\"\r\nContent-Type: image/png\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Request::post(uri)
        .header(header::COOKIE, &user.cookie)
        .header("X-CSRF-Token", &user.csrf)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(w, h, |x, _| {
        image::Rgb([x as u8, 10, 200])
    }));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
async fn upload_an_image_and_use_it_in_a_quiz() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;

    let res = send(
        &env.app,
        multipart(
            &lan,
            &[("file", &png(2400, 1200)), ("alt", "Bản đồ".as_bytes())],
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let media = json_body(res).await;
    assert_eq!(
        (media["width"].as_u64(), media["height"].as_u64()),
        (Some(1920), Some(960))
    );
    assert_eq!(media["altText"], "Bản đồ");
    let url = media["url"].as_str().unwrap().to_string();
    let key = url.strip_prefix("/media/").unwrap();
    let dir = std::env::temp_dir().join("myquiz-test-media");
    let stored = std::fs::read(dir.join(key)).unwrap();
    assert_eq!(&stored[8..12], b"WEBP");

    // Attach it to a question; the quiz shows its URL, and so does a game.
    let mut q = question("Ở đâu?");
    q["mediaId"] = media["id"].clone();
    let mut body = quiz("Bản đồ", vec![q]);
    body["coverMediaId"] = media["id"].clone();
    let res = send(&env.app, lan.req("POST", "/api/v1/quizzes", Some(body))).await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let doc = json_body(res).await;
    assert_eq!(doc["questions"][0]["mediaUrl"], json!(url));
    assert_eq!(doc["coverMediaUrl"], json!(url));
    let id = doc["id"].as_str().unwrap();
    let res = send(
        &env.app,
        lan.req("POST", &format!("/api/v1/quizzes/{id}/publish"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let res = send(
        &env.app,
        lan.req("POST", "/api/v1/games", Some(json!({ "quizId": id }))),
    )
    .await;
    let session: Uuid = json_body(res).await["sessionId"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let snap_media: Option<String> = sqlx::query_scalar(
        "SELECT quiz_snapshot->'questions'->0->>'media' FROM game_sessions WHERE id = $1",
    )
    .bind(session)
    .fetch_one(&env.db)
    .await
    .unwrap();
    assert_eq!(snap_media.as_deref(), Some(url.as_str()));

    // Not an image, whatever the name says.
    let res = send(
        &env.app,
        multipart(&lan, &[("file", b"<svg onload=alert(1)>")]),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(res).await["error"]["code"], "NOT_AN_IMAGE");

    // Over 5 MB.
    let res = send(
        &env.app,
        multipart(&lan, &[("file", &vec![0u8; 5 * 1024 * 1024 + 10])]),
    )
    .await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // Changes need the CSRF token.
    let mut req = multipart(&lan, &[("file", &png(10, 10))]);
    req.headers_mut().remove("X-CSRF-Token");
    assert_eq!(send(&env.app, req).await.status(), StatusCode::FORBIDDEN);
}

async fn body_bytes(res: axum::response::Response) -> Vec<u8> {
    use http_body_util::BodyExt;
    res.into_body().collect().await.unwrap().to_bytes().to_vec()
}

#[tokio::test]
async fn download_a_quiz_as_zip_and_upload_it_again() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;
    let minh = user(&env).await;

    // A quiz with a cover and a question image.
    let res = send(
        &env.app,
        multipart(
            &lan,
            &[("file", &png(64, 32)), ("alt", "Bản đồ".as_bytes())],
        ),
    )
    .await;
    let media = json_body(res).await;
    let mut q = question("Ở đâu?");
    q["mediaId"] = media["id"].clone();
    let mut fill = json!({
        "type": "fill_blank", "text": "Thủ đô là ___", "timeLimitS": 30, "points": 2000,
        "acceptedAnswers": ["Hà Nội"], "ignoreDiacritics": true,
    });
    fill["options"] = json!([]);
    let mut body = quiz("Địa lý: thủ đô", vec![q, fill]);
    body["coverMediaId"] = media["id"].clone();
    let res = send(&env.app, lan.req("POST", "/api/v1/quizzes", Some(body))).await;
    let original = json_body(res).await;
    let id = original["id"].as_str().unwrap().to_string();

    // Someone else cannot export it.
    let res = send(
        &env.app,
        minh.req("POST", &format!("/api/v1/quizzes/{id}/export"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // The owner gets a link; the link alone downloads the zip.
    let res = send(
        &env.app,
        lan.req("POST", &format!("/api/v1/quizzes/{id}/export"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let link = json_body(res).await;
    let url = link["url"].as_str().unwrap().to_string();
    let res = send(&env.app, Request::get(&url).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "application/zip");
    let disposition = res.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        disposition.starts_with("attachment; filename=\"myquiz-dia-ly-thu-do-"),
        "{disposition}"
    );
    let zip = body_bytes(res).await;

    let bundle = myquiz_bundle::read_bundle(
        std::io::Cursor::new(&zip),
        &myquiz_bundle::Limits::default(),
    )
    .unwrap();
    assert_eq!(bundle.quiz.cover.as_deref(), Some("media/cover.webp"));
    assert_eq!(
        bundle.quiz.questions[0].image.as_deref(),
        Some("media/q1.webp")
    );
    assert_eq!(
        bundle.quiz.questions[0].image_alt.as_deref(),
        Some("Bản đồ")
    );
    assert_eq!(bundle.images.len(), 2);

    // A forged or altered link does nothing.
    let mut forged = url.clone();
    forged.push('0');
    let res = send(&env.app, Request::get(&forged).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::GONE);

    // Minh uploads the zip: a new private draft owned by Minh, same content.
    let res = send(
        &env.app,
        multipart_to(&minh, "/api/v1/quizzes/import", &[("file", &zip)]),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED);
    let new_id = json_body(res).await["quizId"].as_str().unwrap().to_string();
    assert_ne!(new_id, id);
    let res = send(
        &env.app,
        minh.req("GET", &format!("/api/v1/quizzes/{new_id}"), None),
    )
    .await;
    let copy = json_body(res).await;
    assert_eq!(copy["title"], "Địa lý: thủ đô");
    assert_eq!(copy["status"], "draft");
    assert_eq!(copy["visibility"], "private");
    assert_eq!(copy["questions"][1]["acceptedAnswers"], json!(["Hà Nội"]));
    assert_eq!(copy["questions"][1]["points"], 2000);
    assert!(copy["questions"][0]["mediaUrl"]
        .as_str()
        .unwrap()
        .starts_with("/media/"));
    assert_ne!(
        copy["questions"][0]["mediaId"],
        original["questions"][0]["mediaId"]
    );
    assert!(copy["coverMediaUrl"].is_string());
    let owner: Uuid = sqlx::query_scalar("SELECT owner_id FROM media WHERE id = $1")
        .bind(
            copy["questions"][0]["mediaId"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(owner, minh.id);
    // It can be published straight away.
    let res = send(
        &env.app,
        minh.req("POST", &format!("/api/v1/quizzes/{new_id}/publish"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);

    // An unfinished quiz is not downloadable, so every zip uploads again.
    let res = send(
        &env.app,
        lan.req("POST", "/api/v1/quizzes", Some(quiz("Nháp", vec![]))),
    )
    .await;
    let draft = json_body(res).await["id"].as_str().unwrap().to_string();
    let res = send(
        &env.app,
        lan.req("POST", &format!("/api/v1/quizzes/{draft}/export"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(json_body(res).await["problems"][0]["code"], "NO_QUESTIONS");
}

#[tokio::test]
async fn bad_zips_are_refused_with_every_problem() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;
    let import = |bytes: Vec<u8>| multipart_to(&lan, "/api/v1/quizzes/import", &[("file", &bytes)]);

    let res = send(&env.app, import(b"not a zip".to_vec())).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(res).await;
    assert_eq!(body["error"]["code"], "IMPORT_INVALID");
    assert_eq!(body["errors"][0]["problem"], "NOT_A_ZIP");

    // A quiz.json with a broken question and an image that is not one.
    let mut file = myquiz_bundle::template();
    file.quiz.questions[0].time_limit_s = 1;
    file.quiz.questions[1].image = Some("media/fake.png".into());
    let fake = b"\x89PNG\r\n\x1a\n but not really a png".to_vec();
    let zip = myquiz_bundle::write_bundle(
        std::io::Cursor::new(Vec::new()),
        &file,
        vec![("media/fake.png".to_string(), fake.clone())],
    )
    .unwrap()
    .into_inner();
    let res = send(&env.app, import(zip)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(res).await;
    assert_eq!(body["errors"][0]["problem"], "TIME_LIMIT");
    assert_eq!(body["errors"][0]["question"], 0);

    // The structure is fine but the image fails the media pipeline.
    let mut file = myquiz_bundle::template();
    file.quiz.questions[1].image = Some("media/fake.png".into());
    let zip = myquiz_bundle::write_bundle(
        std::io::Cursor::new(Vec::new()),
        &file,
        vec![("media/fake.png".to_string(), fake)],
    )
    .unwrap()
    .into_inner();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE owner_id = $1")
        .bind(lan.id)
        .fetch_one(&env.db)
        .await
        .unwrap();
    let res = send(&env.app, import(zip)).await;
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = json_body(res).await;
    assert_eq!(body["errors"][0]["file"], "media/fake.png");
    assert_eq!(body["errors"][0]["problem"], "NOT_AN_IMAGE");
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM quizzes WHERE owner_id = $1")
        .bind(lan.id)
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(before, after, "nothing is saved from a refused zip");

    // The template downloads without signing in and imports cleanly.
    let res = send(
        &env.app,
        Request::get("/api/v1/quiz-template.zip")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let template = body_bytes(res).await;
    let res = send(&env.app, import(template)).await;
    assert_eq!(res.status(), StatusCode::CREATED);
}

/// A finished game written straight to the database, as the game servers
/// leave it: two players ranked, one removed, one unanswered question.
async fn finished_game(env: &Env, host: &User) -> Uuid {
    let snapshot = json!({
        "title": "Địa lý",
        "questions": [
            { "kind": "single", "text": "Thủ đô?", "time_limit_s": 20, "points": 1000,
              "options": [ { "id": 1, "text": "Hà Nội", "is_correct": true },
                           { "id": 2, "text": "=Huế", "is_correct": false } ] },
            { "kind": "fill_blank", "text": "Sông ___", "time_limit_s": 30, "points": 1000,
              "accepted_answers": ["Hồng"] }
        ]
    });
    let session: Uuid = sqlx::query_scalar(
        "INSERT INTO game_sessions (host_id, quiz_snapshot, status, started_at, ended_at, player_count) \
         VALUES ($1, $2, 'ended', now() - interval '5 minutes', now(), 2) RETURNING id",
    )
    .bind(host.id)
    .bind(&snapshot)
    .fetch_one(&env.db)
    .await
    .unwrap();
    let mut ids = Vec::new();
    for (i, (nick, score, rank, kicked)) in [
        ("An", Some(1900), Some(1), false),
        ("Bình", Some(0), Some(2), false),
        ("Xấu", None, None, true),
    ]
    .into_iter()
    .enumerate()
    {
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO session_players (session_id, nickname, join_order, final_score, final_rank, kicked) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(session)
        .bind(nick)
        .bind(i as i32)
        .bind(score)
        .bind(rank)
        .bind(kicked)
        .fetch_one(&env.db)
        .await
        .unwrap();
        ids.push(id);
    }
    // An: right, right. Bình: wrong on Q1, nothing on Q2.
    for (player, q, opts, text, correct, ms, pts) in [
        (ids[0], 0, Some(vec![1]), None, true, 2000, 950),
        (ids[0], 1, None, Some("hong"), true, 6000, 950),
        (ids[1], 0, Some(vec![2]), None, false, 4000, 0),
    ] {
        sqlx::query(
            "INSERT INTO answers (session_id, player_id, question_index, option_ids, text_answer, \
             is_correct, response_ms, points, received_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8, now())",
        )
        .bind(session)
        .bind(player)
        .bind(q)
        .bind(opts)
        .bind(text)
        .bind(correct)
        .bind(ms)
        .bind(pts)
        .execute(&env.db)
        .await
        .unwrap();
    }
    session
}

#[tokio::test]
async fn reports_list_summary_exports_and_delete() {
    let env = crate::api_env_or_skip!("");
    let host = user(&env).await;
    let other = user(&env).await;
    let id = finished_game(&env, &host).await;

    // FR-RP-01: the host's list.
    let res = send(&env.app, host.req("GET", "/api/v1/sessions", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let page = json_body(res).await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["items"][0]["quizTitle"], "Địa lý");
    assert_eq!(page["items"][0]["playerCount"], 2);
    assert_eq!(page["items"][0]["averageScore"], 950.0);

    // Somebody else's report does not exist for them (SEC-23).
    let res = send(
        &env.app,
        other.req("GET", &format!("/api/v1/sessions/{id}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = send(
        &env.app,
        other.req(
            "POST",
            &format!("/api/v1/sessions/{id}/exports"),
            Some(json!({ "format": "csv" })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // FR-RP-02, 03.
    let res = send(
        &env.app,
        host.req("GET", &format!("/api/v1/sessions/{id}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let r = json_body(res).await;
    assert_eq!(r["participation"], 1.0);
    assert_eq!(r["accuracy"], 0.5);
    assert_eq!(r["questions"][0]["answered"], 2);
    assert_eq!(r["questions"][0]["accuracy"], 0.5);
    assert_eq!(r["questions"][0]["averageMs"], 3000);
    assert_eq!(r["questions"][1]["hard"], false);
    assert_eq!(r["players"][0]["nickname"], "An");
    assert_eq!(r["players"][0]["answers"][1]["answer"], "hong");
    assert_eq!(r["players"][1]["answers"][0]["answer"], "=Huế");
    assert_eq!(r["players"][1]["answers"][1]["answer"], Value::Null);
    assert_eq!(r["players"][2]["kicked"], true);
    assert_eq!(r["players"][2]["rank"], Value::Null);

    // FR-RP-04: CSV with a BOM, one row per player × question, formulas
    // neutralised; removed players left out.
    let res = send(
        &env.app,
        host.req(
            "POST",
            &format!("/api/v1/sessions/{id}/exports"),
            Some(json!({ "format": "csv" })),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let url = json_body(res).await["url"].as_str().unwrap().to_string();
    let res = send(&env.app, Request::get(&url).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers()[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .contains("myquiz-report-dia-ly-"));
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
    let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
    assert_eq!(text.lines().count(), 1 + 2 * 2, "{text}");
    assert!(text.contains("'=Huế"));
    assert!(text.contains("no answer"));
    assert!(!text.contains("Xấu"));

    // FR-RP-05: a real xlsx (a zip).
    let res = send(
        &env.app,
        host.req(
            "POST",
            &format!("/api/v1/sessions/{id}/exports"),
            Some(json!({ "format": "xlsx" })),
        ),
    )
    .await;
    let url = json_body(res).await["url"].as_str().unwrap().to_string();
    let res = send(&env.app, Request::get(&url).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = http_body_util::BodyExt::collect(res.into_body())
        .await
        .unwrap()
        .to_bytes();
    assert!(bytes.starts_with(b"PK"));

    // FR-RP-09: only the host deletes; then it is gone.
    let res = send(
        &env.app,
        other.req("DELETE", &format!("/api/v1/sessions/{id}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = send(
        &env.app,
        host.req("DELETE", &format!("/api/v1/sessions/{id}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &env.app,
        host.req("GET", &format!("/api/v1/sessions/{id}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let res = send(&env.app, Request::get(&url).body(Body::empty()).unwrap()).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admins_search_suspend_and_read_the_audit_log() {
    let env = crate::api_env_or_skip!("");
    let admin = user(&env).await;
    sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
        .bind(admin.id)
        .execute(&env.db)
        .await
        .unwrap();
    let target = user(&env).await;
    let target_email: String = sqlx::query_scalar("SELECT email FROM users WHERE id = $1")
        .bind(target.id)
        .fetch_one(&env.db)
        .await
        .unwrap();

    // Not an admin: the area does not exist.
    let res = send(&env.app, target.req("GET", "/api/v1/admin/users", None)).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    // Search by part of the email.
    let q = &target_email[..12];
    let res = send(
        &env.app,
        admin.req("GET", &format!("/api/v1/admin/users?q={q}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let page = json_body(res).await;
    assert_eq!(page["total"], 1, "{page}");
    assert_eq!(page["items"][0]["email"], json!(target_email));
    assert_eq!(page["items"][0]["status"], "active");

    // An admin cannot suspend themselves or another admin.
    let res = send(
        &env.app,
        admin.req(
            "POST",
            &format!("/api/v1/admin/users/{}/suspend", admin.id),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CONFLICT);

    // Suspend: the user's next request is refused and their session ends.
    let res = send(
        &env.app,
        admin.req(
            "POST",
            &format!("/api/v1/admin/users/{}/suspend", target.id),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(&env.app, target.req("GET", "/api/v1/me", None)).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // Reactivate, then both actions are in the audit log, filterable by
    // the target's email.
    let res = send(
        &env.app,
        admin.req(
            "POST",
            &format!("/api/v1/admin/users/{}/reactivate", target.id),
            None,
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &env.app,
        admin.req("GET", &format!("/api/v1/admin/audit?user={q}"), None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
    let log = json_body(res).await;
    let actions: Vec<&str> = log["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["action"].as_str().unwrap())
        .collect();
    assert_eq!(actions, ["user_reactivated", "user_suspended"], "{log}");
    assert!(log["items"][0]["actorEmail"].as_str().is_some());

    // Date filter: nothing tomorrow; a bad date is refused.
    let res = send(
        &env.app,
        admin.req("GET", "/api/v1/admin/audit?from=2999-01-01", None),
    )
    .await;
    assert_eq!(json_body(res).await["total"], 0);
    let res = send(
        &env.app,
        admin.req("GET", "/api/v1/admin/audit?from=nope", None),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// SEC-29 authorization review, as a test: every endpoint that is not
/// public refuses a request without a session, and every change refuses a
/// request without the CSRF token. API answers carry their own security
/// headers (SEC-26).
#[tokio::test]
async fn every_private_endpoint_needs_a_session_and_csrf() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;
    let id = Uuid::new_v4();
    let private: Vec<(&str, String)> = vec![
        ("GET", "/api/v1/me".into()),
        ("PATCH", "/api/v1/me".into()),
        ("POST", "/api/v1/auth/logout".into()),
        ("GET", "/api/v1/quizzes".into()),
        ("POST", "/api/v1/quizzes".into()),
        ("GET", "/api/v1/quizzes/expiring".into()),
        ("GET", format!("/api/v1/quizzes/{id}")),
        ("PUT", format!("/api/v1/quizzes/{id}")),
        ("DELETE", format!("/api/v1/quizzes/{id}")),
        ("POST", format!("/api/v1/quizzes/{id}/duplicate")),
        ("POST", format!("/api/v1/quizzes/{id}/publish")),
        ("POST", format!("/api/v1/quizzes/{id}/export")),
        ("POST", "/api/v1/quizzes/import".into()),
        ("POST", "/api/v1/media".into()),
        ("POST", "/api/v1/games".into()),
        ("GET", "/api/v1/sessions".into()),
        ("GET", format!("/api/v1/sessions/{id}")),
        ("DELETE", format!("/api/v1/sessions/{id}")),
        ("POST", format!("/api/v1/sessions/{id}/exports")),
        ("GET", "/api/v1/admin/users".into()),
        ("POST", format!("/api/v1/admin/users/{id}/suspend")),
        ("POST", format!("/api/v1/admin/users/{id}/reactivate")),
        ("GET", "/api/v1/admin/audit".into()),
    ];
    for (method, uri) in &private {
        let req = Request::builder()
            .method(*method)
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let res = send(&env.app, req).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(res.headers()["x-content-type-options"], "nosniff");
        assert_eq!(res.headers()["cache-control"], "no-store");

        if *method != "GET" {
            let mut req = lan.req(method, uri, None);
            req.headers_mut().remove("X-CSRF-Token");
            let res = send(&env.app, req).await;
            assert_eq!(
                res.status(),
                StatusCode::FORBIDDEN,
                "{method} {uri} without CSRF"
            );
        }
    }
}

#[tokio::test]
async fn quizzes_due_within_three_days_are_listed_for_the_warning() {
    let env = crate::api_env_or_skip!("");
    let lan = user(&env).await;
    for (title, hours_ago) in [("Sắp xoá", 20 * 24), ("Còn lâu", 24)] {
        sqlx::query(
            "INSERT INTO quizzes (owner_id, title, last_used_at) \
             VALUES ($1, $2, now() - make_interval(hours => $3))",
        )
        .bind(lan.id)
        .bind(title)
        .bind(hours_ago)
        .execute(&env.db)
        .await
        .unwrap();
    }
    let res = send(&env.app, lan.req("GET", "/api/v1/quizzes/expiring", None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let list = json_body(res).await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["title"], "Sắp xoá");
}
