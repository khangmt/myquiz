//! Sign-in end to end against a fake Google token endpoint, with a real
//! PostgreSQL and Redis. Skipped unless DATABASE_URL and REDIS_URL are set.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::response::Response;
use axum::routing::post;
use axum::{Form, Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

use super::{router, ApiState};
use crate::config::AuthSettings;
use crate::state::AppState;

const CLIENT_ID: &str = "test-client.apps.googleusercontent.com";

/// The fake Google: answers the token exchange with whatever claims the
/// test put in `claims`, after checking the PKCE fields arrived.
#[derive(Clone, Default)]
struct FakeGoogle {
    claims: Arc<Mutex<Value>>,
    last_form: Arc<Mutex<Vec<(String, String)>>>,
}

async fn fake_token(
    axum::extract::State(g): axum::extract::State<FakeGoogle>,
    Form(form): Form<Vec<(String, String)>>,
) -> Result<Json<Value>, StatusCode> {
    *g.last_form.lock().unwrap() = form.clone();
    let get = |k: &str| form.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    if get("code").as_deref() != Some("good-code") || get("code_verifier").is_none() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let payload = URL_SAFE_NO_PAD.encode(g.claims.lock().unwrap().to_string());
    Ok(Json(
        json!({ "id_token": format!("eyJhbGciOiJSUzI1NiJ9.{payload}.c2ln") }),
    ))
}

struct Env {
    app: Router,
    google: FakeGoogle,
    db: sqlx::PgPool,
}

async fn env(domains: &str) -> Option<Env> {
    let database_url = std::env::var("DATABASE_URL").ok()?;
    let redis_url = std::env::var("REDIS_URL").ok()?;

    let google = FakeGoogle::default();
    let fake = Router::new()
        .route("/token", post(fake_token))
        .with_state(google.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, fake).await.unwrap() });

    let mut auth = AuthSettings::for_tests();
    auth.google_client_id = Some(CLIENT_ID.into());
    auth.google_client_secret = Some("shh".into());
    auth.google_token_url = format!("http://{addr}/token");
    auth.allowed_email_domains = domains.into();

    let cfg = crate::config::Config {
        bind_addr: "127.0.0.1:0".into(),
        database_url,
        redis_url,
        db_max_connections: 2,
        countdown_ms: 3000,
        auth: auth.clone(),
        log_json: false,
    };
    let app_state = AppState::connect(crate::Role::Api, &cfg).await.unwrap();
    crate::MIGRATOR.run(&app_state.db).await.unwrap();
    let db = app_state.db.clone();
    // The sandbox and some CI runners set HTTPS_PROXY; the fake is local.
    let mut state = ApiState::new(app_state, auth).unwrap();
    state.http = reqwest::Client::builder().no_proxy().build().unwrap();
    Some(Env {
        app: router(state),
        google,
        db,
    })
}

macro_rules! env_or_skip {
    ($domains:expr) => {
        match env($domains).await {
            Some(e) => e,
            None => {
                eprintln!("skipped: DATABASE_URL and REDIS_URL not set");
                return;
            }
        }
    };
}

fn query_param(url: &str, name: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()?
        .query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// `name=value` from the response's Set-Cookie headers.
fn set_cookie(res: &Response, name: &str) -> Option<String> {
    res.headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(&format!("{name}=")))
        .map(|v| v.split(';').next().unwrap().to_string())
}

fn location(res: &Response) -> String {
    res.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string()
}

async fn send(app: &Router, req: Request<Body>) -> Response {
    app.clone().oneshot(req).await.unwrap()
}

async fn json_body(res: Response) -> Value {
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

struct Login {
    callback: Response,
}

/// Runs the whole sign-in: start, "Google" redirect, callback.
async fn sign_in(env: &Env, claims: Value) -> Login {
    let start = send(
        &env.app,
        Request::get("/api/v1/auth/google?next=/quizzes")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(start.status(), StatusCode::SEE_OTHER);
    let to_google = location(&start);
    assert_eq!(
        query_param(&to_google, "client_id").as_deref(),
        Some(CLIENT_ID)
    );
    assert_eq!(
        query_param(&to_google, "code_challenge_method").as_deref(),
        Some("S256")
    );
    let state = query_param(&to_google, "state").unwrap();
    let nonce = query_param(&to_google, "nonce").unwrap();
    let state_cookie = set_cookie(&start, "oauth_state").unwrap();

    let mut claims = claims;
    if claims["nonce"].is_null() {
        claims["nonce"] = json!(nonce);
    }
    *env.google.claims.lock().unwrap() = claims;

    let callback = send(
        &env.app,
        Request::get(format!(
            "/api/v1/auth/google/callback?code=good-code&state={state}"
        ))
        .header(header::COOKIE, state_cookie)
        .body(Body::empty())
        .unwrap(),
    )
    .await;
    Login { callback }
}

fn claims(sub: &str, email: &str) -> Value {
    json!({
        "iss": "https://accounts.google.com",
        "aud": CLIENT_ID,
        "exp": 4_000_000_000i64,
        "sub": sub,
        "email": email,
        "email_verified": true,
        "name": "Nguyễn Văn Lan",
    })
}

fn unique() -> String {
    Uuid::new_v4().simple().to_string()
}

#[tokio::test]
async fn google_sign_in_session_csrf_and_logout() {
    let env = env_or_skip!("");
    let sub = unique();
    let email = format!("lan-{sub}@uit.edu.vn");
    let login = sign_in(&env, claims(&sub, &email)).await;
    assert_eq!(login.callback.status(), StatusCode::SEE_OTHER);
    assert_eq!(location(&login.callback), "/quizzes");
    let sid = set_cookie(&login.callback, "sid").unwrap();
    let raw = login
        .callback
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with("sid="))
        .unwrap();
    assert!(raw.contains("HttpOnly") && raw.contains("SameSite=Lax"));

    // The token exchange sent the PKCE verifier and our redirect URI.
    let form = env.google.last_form.lock().unwrap().clone();
    assert!(form
        .iter()
        .any(|(k, v)| k == "redirect_uri"
            && v == "http://localhost:8080/api/v1/auth/google/callback"));

    // GET /me works with the cookie alone.
    let me = send(
        &env.app,
        Request::get("/api/v1/me")
            .header(header::COOKIE, &sid)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(me.status(), StatusCode::OK);
    let me = json_body(me).await;
    assert_eq!(me["email"], json!(email));
    assert_eq!(me["displayName"], json!("Nguyễn Văn Lan"));
    assert_eq!(me["role"], json!("user"));
    let csrf = me["csrfToken"].as_str().unwrap().to_string();

    // PATCH without the CSRF header is refused; with it, it works.
    let patch = |with_csrf: bool| {
        let mut req = Request::patch("/api/v1/me")
            .header(header::COOKIE, &sid)
            .header(header::CONTENT_TYPE, "application/json");
        if with_csrf {
            req = req.header("X-CSRF-Token", &csrf);
        }
        req.body(Body::from(r#"{"displayName":"Lan","locale":"en"}"#))
            .unwrap()
    };
    let res = send(&env.app, patch(false)).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(res).await["error"]["code"], json!("CSRF_INVALID"));
    let res = send(&env.app, patch(true)).await;
    assert_eq!(res.status(), StatusCode::OK);
    let updated = json_body(res).await;
    assert_eq!(updated["displayName"], json!("Lan"));
    assert_eq!(updated["locale"], json!("en"));

    // Signing in again with the same Google account finds the same user.
    let again = sign_in(&env, claims(&sub, &email)).await;
    assert_eq!(again.callback.status(), StatusCode::SEE_OTHER);
    let users: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE lower(email) = $1")
        .bind(&email)
        .fetch_one(&env.db)
        .await
        .unwrap();
    assert_eq!(users, 1);
    let logins: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log a JOIN users u ON u.id = a.actor_user_id \
         WHERE a.action = 'login' AND lower(u.email) = $1",
    )
    .bind(&email)
    .fetch_one(&env.db)
    .await
    .unwrap();
    assert_eq!(logins, 2);

    // Logout ends the session.
    let res = send(
        &env.app,
        Request::post("/api/v1/auth/logout")
            .header(header::COOKIE, &sid)
            .header("X-CSRF-Token", &csrf)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let res = send(
        &env.app,
        Request::get("/api/v1/me")
            .header(header::COOKIE, &sid)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        json_body(res).await["error"]["code"],
        json!("UNAUTHENTICATED")
    );
}

#[tokio::test]
async fn refusals_redirect_to_the_login_page() {
    let env = env_or_skip!("uit.edu.vn");
    let refused = |l: &Login| {
        assert_eq!(l.callback.status(), StatusCode::SEE_OTHER);
        assert!(set_cookie(&l.callback, "sid").is_none());
        location(&l.callback)
    };

    let other_domain = sign_in(&env, claims(&unique(), "a@gmail.com")).await;
    assert_eq!(refused(&other_domain), "/login?error=DOMAIN_NOT_ALLOWED");

    let mut c = claims(&unique(), &format!("{}@uit.edu.vn", unique()));
    c["email_verified"] = json!(false);
    assert_eq!(
        refused(&sign_in(&env, c).await),
        "/login?error=EMAIL_UNVERIFIED"
    );

    let mut c = claims(&unique(), &format!("{}@uit.edu.vn", unique()));
    c["aud"] = json!("someone-else");
    assert_eq!(
        refused(&sign_in(&env, c).await),
        "/login?error=GOOGLE_BAD_TOKEN"
    );

    let mut c = claims(&unique(), &format!("{}@uit.edu.vn", unique()));
    c["nonce"] = json!("replayed");
    assert_eq!(
        refused(&sign_in(&env, c).await),
        "/login?error=GOOGLE_BAD_TOKEN"
    );

    let mut c = claims(&unique(), &format!("{}@uit.edu.vn", unique()));
    c["exp"] = json!(1);
    assert_eq!(
        refused(&sign_in(&env, c).await),
        "/login?error=GOOGLE_BAD_TOKEN"
    );

    // A callback without this browser's state cookie (login CSRF).
    let res = send(
        &env.app,
        Request::get("/api/v1/auth/google/callback?code=good-code&state=abc")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(location(&res), "/login?error=LOGIN_EXPIRED");

    // The user pressed Cancel at Google.
    let res = send(
        &env.app,
        Request::get("/api/v1/auth/google/callback?error=access_denied")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(location(&res), "/login?error=GOOGLE_DENIED");

    // A suspended user cannot sign in.
    let sub = unique();
    let email = format!("{sub}@uit.edu.vn");
    assert_eq!(
        sign_in(&env, claims(&sub, &email)).await.callback.status(),
        StatusCode::SEE_OTHER
    );
    sqlx::query("UPDATE users SET status = 'suspended' WHERE lower(email) = $1")
        .bind(&email)
        .execute(&env.db)
        .await
        .unwrap();
    assert_eq!(
        refused(&sign_in(&env, claims(&sub, &email)).await),
        "/login?error=ACCOUNT_SUSPENDED"
    );

    let failures: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE action = 'login_failed' AND details->>'email' = $1",
    )
    .bind(&email)
    .fetch_one(&env.db)
    .await
    .unwrap();
    assert_eq!(failures, 1);
}

#[tokio::test]
async fn a_used_state_cannot_be_replayed() {
    let env = env_or_skip!("");
    let start = send(
        &env.app,
        Request::get("/api/v1/auth/google")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let to_google = location(&start);
    let state = query_param(&to_google, "state").unwrap();
    let nonce = query_param(&to_google, "nonce").unwrap();
    let cookie = set_cookie(&start, "oauth_state").unwrap();
    let mut c = claims(&unique(), &format!("{}@uit.edu.vn", unique()));
    c["nonce"] = json!(nonce);
    *env.google.claims.lock().unwrap() = c;

    let callback = || {
        Request::get(format!(
            "/api/v1/auth/google/callback?code=good-code&state={state}"
        ))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap()
    };
    let first = send(&env.app, callback()).await;
    assert_eq!(location(&first), "/");
    assert!(set_cookie(&first, "sid").is_some());
    let second = send(&env.app, callback()).await;
    assert_eq!(location(&second), "/login?error=LOGIN_EXPIRED");
}

#[tokio::test]
async fn me_needs_a_session() {
    let env = env_or_skip!("");
    for cookie in ["", "sid=nothex", &format!("sid={}", "a".repeat(64))] {
        let res = send(
            &env.app,
            Request::get("/api/v1/me")
                .header(header::COOKIE, cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
