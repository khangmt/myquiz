//! Google sign-in, login sessions and CSRF (design sections 6 and 9).
//!
//! Flow: `GET /auth/google?next=/quizzes` stores a one-time state with a
//! PKCE verifier and a nonce in Redis, binds it to this browser with a short
//! cookie, and redirects to Google. Google sends the browser back to
//! `/auth/google/callback`, where we swap the code for an ID token, check
//! it, find or create the user, and start a login session.
//!
//! Login sessions live in Redis as `session:<sid>` (fields `user_id`,
//! `csrf`) with a sliding expiry, so the game servers can read them too. The
//! `sid` cookie is HttpOnly and SameSite=Lax. Every request that changes
//! something must also send the session's CSRF token in `X-CSRF-Token`.

use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use fred::prelude::*;
use myquiz_protocol::{Me, UpdateMe};
use rand::TryRngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::audit::{audit, AuditEntry};
use super::error::{ApiResult, AppError};
use super::ApiState;
use crate::gateway::cookie;
use crate::live::keys;

const STATE_TTL_S: i64 = 600;
const STATE_COOKIE: &str = "oauth_state";
const SESSION_COOKIE: &str = "sid";
pub const CSRF_HEADER: &str = "x-csrf-token";

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .expect("OS random generator");
    hex::encode(buf)
}

fn random_b64(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rngs::OsRng
        .try_fill_bytes(&mut buf)
        .expect("OS random generator");
    URL_SAFE_NO_PAD.encode(buf)
}

fn oauth_key(state: &str) -> String {
    format!("oauth:{state}")
}

/// Only same-site relative paths, so the sign-in link cannot be used to
/// send people to another site (open redirect).
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n)
            if n.starts_with('/')
                && !n.starts_with("//")
                && !n.starts_with("/\\")
                && n.len() <= 512
                && !n.chars().any(|c| c.is_control() || c == '\\') =>
        {
            n.to_string()
        }
        _ => "/".to_string(),
    }
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn cookie_header(
    state: &ApiState,
    name: &str,
    value: &str,
    path: &str,
    max_age: i64,
) -> HeaderValue {
    let secure = if state.auth.cookie_secure {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{name}={value}; Max-Age={max_age}; Path={path}; HttpOnly; SameSite=Lax{secure}"
    ))
    .expect("cookie values are hex")
}

// ---------------------------------------------------------------------------
// Start

#[derive(Deserialize)]
pub struct StartQuery {
    next: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct PendingLogin {
    verifier: String,
    nonce: String,
    next: String,
}

pub async fn google_start(
    State(state): State<ApiState>,
    Query(q): Query<StartQuery>,
) -> ApiResult<Response> {
    let Some(client_id) = state.auth.google_client_id.as_deref() else {
        return Err(not_configured());
    };
    let login_state = random_hex(16);
    let pending = PendingLogin {
        verifier: random_b64(32),
        nonce: random_hex(16),
        next: safe_next(q.next.as_deref()),
    };
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes()));
    let _: () = state
        .app
        .redis
        .set(
            oauth_key(&login_state),
            serde_json::to_string(&pending)?,
            Some(Expiration::EX(STATE_TTL_S)),
            None,
            false,
        )
        .await?;

    let mut url = reqwest::Url::parse(&state.auth.google_auth_url)?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &state.auth.redirect_uri())
        .append_pair("response_type", "code")
        .append_pair("scope", "openid email profile")
        .append_pair("state", &login_state)
        .append_pair("nonce", &pending.nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("prompt", "select_account");

    let mut res = Redirect::to(url.as_str()).into_response();
    res.headers_mut().append(
        header::SET_COOKIE,
        cookie_header(
            &state,
            STATE_COOKIE,
            &login_state,
            "/api/v1/auth",
            STATE_TTL_S,
        ),
    );
    Ok(res)
}

fn not_configured() -> AppError {
    AppError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        "AUTH_NOT_CONFIGURED",
        "Google sign-in is not configured",
    )
}

// ---------------------------------------------------------------------------
// Callback

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

/// The ID token claims we use. `aud` may be a string or a list.
#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: serde_json::Value,
    exp: i64,
    sub: String,
    nonce: Option<String>,
    email: Option<String>,
    #[serde(default)]
    email_verified: serde_json::Value,
    name: Option<String>,
}

/// Why a sign-in was refused. The code goes to the login page as
/// `?error=CODE` and into the audit log.
#[derive(Debug, PartialEq, Eq)]
pub enum LoginRefusal {
    Denied,
    BadState,
    Exchange,
    BadToken,
    EmailUnverified,
    DomainNotAllowed,
    Suspended,
}

impl LoginRefusal {
    fn code(&self) -> &'static str {
        match self {
            LoginRefusal::Denied => "GOOGLE_DENIED",
            LoginRefusal::BadState => "LOGIN_EXPIRED",
            LoginRefusal::Exchange => "GOOGLE_UNAVAILABLE",
            LoginRefusal::BadToken => "GOOGLE_BAD_TOKEN",
            LoginRefusal::EmailUnverified => "EMAIL_UNVERIFIED",
            LoginRefusal::DomainNotAllowed => "DOMAIN_NOT_ALLOWED",
            LoginRefusal::Suspended => "ACCOUNT_SUSPENDED",
        }
    }
}

pub async fn google_callback(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> ApiResult<Response> {
    if state.auth.google_client_id.is_none() {
        return Err(not_configured());
    }
    let outcome = finish_login(&state, &headers, &q).await;
    let clear_state = cookie_header(&state, STATE_COOKIE, "", "/api/v1/auth", 0);
    let mut res = match outcome {
        Ok((user_id, next)) => {
            let sid = create_session(&state, user_id).await?;
            audit(
                &state.app.db,
                &state.auth.app_secret,
                &headers,
                AuditEntry {
                    actor: Some(user_id),
                    action: "login",
                    target_type: Some("user"),
                    target_id: Some(user_id.to_string()),
                    details: json!({ "provider": "google" }),
                },
            )
            .await;
            let mut res = Redirect::to(&next).into_response();
            let max_age = (state.auth.session_days * 86_400) as i64;
            res.headers_mut().append(
                header::SET_COOKIE,
                cookie_header(&state, SESSION_COOKIE, &sid, "/", max_age),
            );
            res
        }
        Err((refusal, email)) => {
            tracing::info!(reason = refusal.code(), "sign-in refused");
            audit(
                &state.app.db,
                &state.auth.app_secret,
                &headers,
                AuditEntry {
                    actor: None,
                    action: "login_failed",
                    target_type: None,
                    target_id: None,
                    details: json!({ "provider": "google", "reason": refusal.code(), "email": email }),
                },
            )
            .await;
            Redirect::to(&format!("/login?error={}", refusal.code())).into_response()
        }
    };
    res.headers_mut().append(header::SET_COOKIE, clear_state);
    Ok(res)
}

type Refused = (LoginRefusal, Option<String>);

async fn finish_login(
    state: &ApiState,
    headers: &HeaderMap,
    q: &CallbackQuery,
) -> Result<(Uuid, String), Refused> {
    let refuse = |r| (r, None);
    if q.error.is_some() {
        return Err(refuse(LoginRefusal::Denied));
    }
    let (Some(code), Some(login_state)) = (q.code.as_deref(), q.state.as_deref()) else {
        return Err(refuse(LoginRefusal::BadState));
    };
    // The state must be the one this browser started (login CSRF) and must
    // be used once.
    match cookie(headers, STATE_COOKIE) {
        Some(c) if constant_time_eq(&c, login_state) => {}
        _ => return Err(refuse(LoginRefusal::BadState)),
    }
    let pending: Option<String> = state
        .app
        .redis
        .getdel(oauth_key(login_state))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "redis getdel");
            refuse(LoginRefusal::Exchange)
        })?;
    let pending: PendingLogin = pending
        .and_then(|p| serde_json::from_str(&p).ok())
        .ok_or(refuse(LoginRefusal::BadState))?;

    let claims = exchange_code(state, code, &pending.verifier).await?;
    let email = check_claims(state, &claims, &pending.nonce)?;
    let user = find_or_create_user(state, &claims, &email)
        .await
        .map_err(|e| {
            tracing::error!(error = ?e, "find or create user");
            (LoginRefusal::Exchange, Some(email.clone()))
        })?;
    match user {
        Some(id) => Ok((id, pending.next)),
        None => Err((LoginRefusal::Suspended, Some(email))),
    }
}

async fn exchange_code(state: &ApiState, code: &str, verifier: &str) -> Result<Claims, Refused> {
    let (Some(id), Some(secret)) = (
        state.auth.google_client_id.as_deref(),
        state.auth.google_client_secret.as_deref(),
    ) else {
        return Err((LoginRefusal::Exchange, None));
    };
    let redirect_uri = state.auth.redirect_uri();
    let form = [
        ("code", code),
        ("client_id", id),
        ("client_secret", secret),
        ("redirect_uri", redirect_uri.as_str()),
        ("grant_type", "authorization_code"),
        ("code_verifier", verifier),
    ];
    let res = state
        .http
        .post(&state.auth.google_token_url)
        .form(&form)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| {
            tracing::warn!(error = %e, "google token exchange failed");
            (LoginRefusal::Exchange, None)
        })?;
    let token: TokenResponse = res
        .json()
        .await
        .map_err(|_| (LoginRefusal::BadToken, None))?;
    decode_id_token(&token.id_token).ok_or((LoginRefusal::BadToken, None))
}

/// Reads the ID token's claims. The signature is not checked: the token
/// came straight from Google's token endpoint over TLS in exchange for our
/// client secret, which OpenID Connect Core 3.1.3.7 allows in place of
/// signature validation. The claims are still checked below.
fn decode_id_token(token: &str) -> Option<Claims> {
    let mut parts = token.split('.');
    let (_header, payload) = (parts.next()?, parts.next()?);
    parts.next()?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn check_claims(state: &ApiState, c: &Claims, nonce: &str) -> Result<String, Refused> {
    let bad = |r| Err((r, None));
    let issuer = state.auth.google_issuer.trim_end_matches('/');
    let iss_ok = c.iss == issuer
        || (issuer == "https://accounts.google.com" && c.iss == "accounts.google.com");
    let client_id = state.auth.google_client_id.as_deref().unwrap_or_default();
    let aud_ok = match &c.aud {
        serde_json::Value::String(a) => a == client_id,
        serde_json::Value::Array(list) => list.iter().any(|a| a.as_str() == Some(client_id)),
        _ => false,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let nonce_ok = c
        .nonce
        .as_deref()
        .is_some_and(|n| constant_time_eq(n, nonce));
    if !iss_ok || !aud_ok || c.exp < now - 60 || !nonce_ok || c.sub.is_empty() {
        return bad(LoginRefusal::BadToken);
    }
    let Some(email) = c.email.as_deref().map(|e| e.trim().to_lowercase()) else {
        return bad(LoginRefusal::EmailUnverified);
    };
    let verified = matches!(&c.email_verified, serde_json::Value::Bool(true))
        || c.email_verified.as_str() == Some("true");
    if !verified {
        return Err((LoginRefusal::EmailUnverified, Some(email)));
    }
    let allowed = state.auth.allowed_domains();
    let domain = email.rsplit_once('@').map(|(_, d)| d).unwrap_or_default();
    if !allowed.is_empty() && !allowed.iter().any(|d| d == domain) {
        return Err((LoginRefusal::DomainNotAllowed, Some(email)));
    }
    Ok(email)
}

/// Returns the user's id, or None when the account is suspended or deleted.
/// The Google identity is matched first; otherwise a user with the same
/// verified email is linked (an email/password account from week 3);
/// otherwise a new user is created.
async fn find_or_create_user(
    state: &ApiState,
    c: &Claims,
    email: &str,
) -> anyhow::Result<Option<Uuid>> {
    let mut tx = state.app.db.begin().await?;
    let linked: Option<(Uuid, String, bool)> = sqlx::query_as(
        "SELECT u.id, u.status::text, u.deleted_at IS NOT NULL \
         FROM auth_identities i JOIN users u ON u.id = i.user_id \
         WHERE i.provider = 'google' AND i.subject = $1",
    )
    .bind(&c.sub)
    .fetch_optional(&mut *tx)
    .await?;

    let (id, status, deleted) = match linked {
        Some(row) => row,
        None => {
            let existing: Option<(Uuid, String, bool)> = sqlx::query_as(
                "SELECT id, status::text, deleted_at IS NOT NULL FROM users \
                 WHERE lower(email) = $1",
            )
            .bind(email)
            .fetch_optional(&mut *tx)
            .await?;
            let row = match existing {
                Some(row) => row,
                None => {
                    let name = display_name(c.name.as_deref(), email);
                    let id: Uuid = sqlx::query_scalar(
                        "INSERT INTO users (email, display_name, email_verified_at) \
                         VALUES ($1, $2, now()) RETURNING id",
                    )
                    .bind(email)
                    .bind(name)
                    .fetch_one(&mut *tx)
                    .await?;
                    (id, "active".to_string(), false)
                }
            };
            sqlx::query(
                "INSERT INTO auth_identities (user_id, provider, subject) VALUES ($1, 'google', $2)",
            )
            .bind(row.0)
            .bind(&c.sub)
            .execute(&mut *tx)
            .await?;
            row
        }
    };
    tx.commit().await?;
    Ok((status == "active" && !deleted).then_some(id))
}

/// Google's name, or the part of the email before '@', cut to 60 characters.
fn display_name(name: Option<&str>, email: &str) -> String {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    let fallback = email.split('@').next().unwrap_or("user");
    let chosen: String = name.unwrap_or(fallback).chars().take(60).collect();
    if chosen.trim().is_empty() {
        "user".to_string()
    } else {
        chosen
    }
}

async fn create_session(state: &ApiState, user_id: Uuid) -> ApiResult<String> {
    // A fresh id on every sign-in (no session fixation).
    let sid = random_hex(32);
    let key = keys::login_session(&sid);
    let ttl = (state.auth.session_days * 86_400) as i64;
    let pipe = state.app.redis.next().pipeline();
    let _: () = pipe
        .hset(
            &key,
            [("user_id", user_id.to_string()), ("csrf", random_hex(16))],
        )
        .await?;
    let _: () = pipe.expire(&key, ttl, None).await?;
    let _: Vec<Value> = pipe.all().await?;
    Ok(sid)
}

// ---------------------------------------------------------------------------
// The signed-in user

/// Extractor for endpoints that need a signed-in user. Refuses with 401 when
/// there is no valid session, and with 403 CSRF_INVALID when a request that
/// changes something lacks the session's CSRF token.
pub struct AuthUser {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub locale: String,
    pub sid: String,
    pub csrf: String,
}

impl AuthUser {
    #[allow(dead_code)] // admin endpoints, later
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

impl FromRequestParts<ApiState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> Result<Self, AppError> {
        let sid = cookie(&parts.headers, SESSION_COOKIE)
            .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(AppError::unauthenticated)?;
        let key = keys::login_session(&sid);
        let fields: std::collections::HashMap<String, String> =
            state.app.redis.hgetall(&key).await?;
        let (Some(user_id), Some(csrf)) = (fields.get("user_id"), fields.get("csrf")) else {
            return Err(AppError::unauthenticated());
        };
        let user_id: Uuid = user_id.parse().map_err(|_| AppError::unauthenticated())?;

        let safe = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
        if !safe {
            let sent = parts.headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok());
            if !sent.is_some_and(|s| constant_time_eq(s, csrf)) {
                return Err(AppError::forbidden("CSRF_INVALID"));
            }
        }

        let row: Option<(String, String, String, String)> = sqlx::query_as(
            "SELECT email, display_name, role::text, locale FROM users \
             WHERE id = $1 AND status = 'active' AND deleted_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(&state.app.db)
        .await?;
        let Some((email, display_name, role, locale)) = row else {
            // Suspended or deleted since signing in: end the session now.
            let _: i64 = state.app.redis.del(&key).await?;
            return Err(AppError::unauthenticated());
        };
        // Sliding expiry (SEC-04).
        let _: bool = state
            .app
            .redis
            .expire(&key, (state.auth.session_days * 86_400) as i64, None)
            .await?;
        Ok(AuthUser {
            id: user_id,
            email,
            display_name,
            role,
            locale,
            sid,
            csrf: csrf.clone(),
        })
    }
}

pub async fn logout(State(state): State<ApiState>, user: AuthUser) -> ApiResult<Response> {
    let _: i64 = state.app.redis.del(keys::login_session(&user.sid)).await?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().append(
        header::SET_COOKIE,
        cookie_header(&state, SESSION_COOKIE, "", "/", 0),
    );
    Ok(res)
}

fn me_body(u: &AuthUser) -> Me {
    Me {
        id: u.id.to_string(),
        email: u.email.clone(),
        display_name: u.display_name.clone(),
        role: u.role.clone(),
        locale: u.locale.clone(),
        csrf_token: u.csrf.clone(),
    }
}

pub async fn me(user: AuthUser) -> Json<Me> {
    Json(me_body(&user))
}

pub async fn update_me(
    State(state): State<ApiState>,
    mut user: AuthUser,
    Json(body): Json<UpdateMe>,
) -> ApiResult<Json<Me>> {
    if let Some(name) = body.display_name {
        let name = name.trim().to_string();
        let len = name.chars().count();
        if !(1..=60).contains(&len) || name.chars().any(char::is_control) {
            return Err(AppError::bad_request(
                "DISPLAY_NAME_LENGTH",
                "1 to 60 characters",
            ));
        }
        user.display_name = name;
    }
    if let Some(locale) = body.locale {
        if locale != "vi" && locale != "en" {
            return Err(AppError::bad_request("LOCALE_INVALID", "vi or en"));
        }
        user.locale = locale;
    }
    sqlx::query("UPDATE users SET display_name = $2, locale = $3 WHERE id = $1")
        .bind(user.id)
        .bind(&user.display_name)
        .bind(&user.locale)
        .execute(&state.app.db)
        .await?;
    Ok(Json(me_body(&user)))
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn next_must_be_a_local_path() {
        assert_eq!(safe_next(Some("/quizzes?x=1")), "/quizzes?x=1");
        assert_eq!(safe_next(Some("//evil.com")), "/");
        assert_eq!(safe_next(Some("/\\evil.com")), "/");
        assert_eq!(safe_next(Some("https://evil.com")), "/");
        assert_eq!(safe_next(Some("/a\nb")), "/");
        assert_eq!(safe_next(None), "/");
    }

    #[test]
    fn display_name_falls_back_and_truncates() {
        assert_eq!(display_name(Some("  Lan  "), "x@y.vn"), "Lan");
        assert_eq!(display_name(None, "khang@uit.edu.vn"), "khang");
        assert_eq!(
            display_name(Some(&"a".repeat(80)), "x@y").chars().count(),
            60
        );
    }

    #[test]
    fn id_token_payload_decodes() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"iss":"i","aud":"a","exp":1,"sub":"s"}"#);
        let c = decode_id_token(&format!("h.{payload}.sig")).unwrap();
        assert_eq!(c.sub, "s");
        assert!(decode_id_token("not-a-jwt").is_none());
    }
}
