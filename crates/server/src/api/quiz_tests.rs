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
