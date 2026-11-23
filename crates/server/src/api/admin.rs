//! Administration (FR-AD-01, FR-AD-04, design section 6).
//!
//! - `GET /admin/users?q=&page=`: search by email or name.
//! - `POST /admin/users/:id/suspend` and `/reactivate`. A suspended user
//!   cannot sign in or use the API (the session check refuses them), and
//!   the game servers end their running games.
//! - `GET /admin/audit?user=&action=&from=&to=&page=`: the audit log,
//!   read only.
//!
//! Every endpoint answers 404 to non-admins, so the area is not advertised.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use myquiz_protocol::admin::{AdminUser, AdminUserPage, AuditPage, AuditRow};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use super::audit::{audit, AuditEntry};
use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::quizzes::rfc3339;
use super::ApiState;

const PAGE_SIZE: u32 = 50;

fn require_admin(user: &AuthUser) -> ApiResult<()> {
    if user.is_admin() {
        Ok(())
    } else {
        Err(AppError::not_found("NOT_FOUND"))
    }
}

fn like(q: &str) -> String {
    format!(
        "%{}%",
        q.trim()
            .chars()
            .take(100)
            .collect::<String>()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

#[derive(Debug, Deserialize)]
pub struct UserQuery {
    #[serde(default)]
    q: String,
    page: Option<u32>,
}

pub async fn users(
    State(state): State<ApiState>,
    user: AuthUser,
    Query(query): Query<UserQuery>,
) -> ApiResult<Json<AdminUserPage>> {
    require_admin(&user)?;
    let page = query.page.unwrap_or(1).clamp(1, 10_000);
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, String, String, String, OffsetDateTime, i64, i64, i64)> =
        sqlx::query_as(
            "SELECT u.id, u.email, u.display_name, u.role::text, u.status::text, u.created_at, \
                    (SELECT count(*) FROM quizzes q WHERE q.owner_id = u.id AND q.deleted_at IS NULL), \
                    (SELECT count(*) FROM game_sessions s WHERE s.host_id = u.id AND s.started_at IS NOT NULL), \
                    count(*) OVER () \
             FROM users u \
             WHERE u.deleted_at IS NULL AND (u.email ILIKE $1 OR u.display_name ILIKE $1) \
             ORDER BY u.created_at DESC, u.id \
             LIMIT $2 OFFSET $3",
        )
        .bind(like(&query.q))
        .bind(PAGE_SIZE as i64)
        .bind(((page - 1) * PAGE_SIZE) as i64)
        .fetch_all(&state.app.db)
        .await?;
    let total = rows.first().map_or(0, |r| r.8);
    Ok(Json(AdminUserPage {
        items: rows
            .into_iter()
            .map(|r| AdminUser {
                id: r.0.to_string(),
                email: r.1,
                display_name: r.2,
                role: r.3,
                status: r.4,
                created_at: rfc3339(r.5),
                quiz_count: r.6 as u32,
                game_count: r.7 as u32,
            })
            .collect(),
        page,
        page_size: PAGE_SIZE,
        total,
    }))
}

async fn set_status(
    state: &ApiState,
    admin: &AuthUser,
    id: &str,
    headers: &HeaderMap,
    status: &str,
) -> ApiResult<StatusCode> {
    require_admin(admin)?;
    let id: Uuid = id
        .parse()
        .map_err(|_| AppError::not_found("USER_NOT_FOUND"))?;
    if id == admin.id {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "CANNOT_SUSPEND_SELF",
            "an admin cannot suspend their own account",
        ));
    }
    // Admins are changed with `myquiz admin`, not from the web.
    let changed: Option<String> = sqlx::query_scalar(
        "UPDATE users SET status = $2::user_status WHERE id = $1 AND deleted_at IS NULL \
           AND role = 'user' RETURNING email",
    )
    .bind(id)
    .bind(status)
    .fetch_optional(&state.app.db)
    .await?;
    let Some(email) = changed else {
        return Err(AppError::not_found("USER_NOT_FOUND"));
    };
    audit(
        &state.app.db,
        &state.auth.app_secret,
        headers,
        AuditEntry {
            actor: Some(admin.id),
            action: if status == "suspended" {
                "user_suspended"
            } else {
                "user_reactivated"
            },
            target_type: Some("user"),
            target_id: Some(id.to_string()),
            details: json!({ "email": email }),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn suspend(
    State(state): State<ApiState>,
    admin: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    set_status(&state, &admin, &id, &headers, "suspended").await
}

pub async fn reactivate(
    State(state): State<ApiState>,
    admin: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    set_status(&state, &admin, &id, &headers, "active").await
}

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    /// Email (or part of it) of the actor or the target user.
    #[serde(default)]
    user: String,
    #[serde(default)]
    action: String,
    /// YYYY-MM-DD, inclusive, Vietnam time.
    from: Option<String>,
    to: Option<String>,
    page: Option<u32>,
}

fn day(s: &Option<String>) -> ApiResult<Option<time::Date>> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    match s.as_deref().filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => time::Date::parse(s, fmt)
            .map(Some)
            .map_err(|_| AppError::bad_request("DATE_INVALID", "use YYYY-MM-DD")),
    }
}

pub async fn audit_log(
    State(state): State<ApiState>,
    user: AuthUser,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<AuditPage>> {
    require_admin(&user)?;
    let page = query.page.unwrap_or(1).clamp(1, 10_000);
    let ict = time::UtcOffset::from_hms(7, 0, 0).expect("valid offset");
    let from = day(&query.from)?.map(|d| d.midnight().assume_offset(ict));
    let to = day(&query.to)?.map(|d| (d.midnight() + time::Duration::days(1)).assume_offset(ict));
    let user_like = like(&query.user);
    let filter_user = !query.user.trim().is_empty();
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        i64,
        OffsetDateTime,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        serde_json::Value,
        i64,
    )> = sqlx::query_as(
        "SELECT a.id, a.at, u.email, a.action, a.target_type, a.target_id, a.ip_hash, a.details, \
                count(*) OVER () \
         FROM audit_log a LEFT JOIN users u ON u.id = a.actor_user_id \
         WHERE ($1::timestamptz IS NULL OR a.at >= $1) \
           AND ($2::timestamptz IS NULL OR a.at < $2) \
           AND ($3 = '' OR a.action = $3) \
           AND (NOT $4 OR u.email ILIKE $5 OR EXISTS ( \
                 SELECT 1 FROM users t WHERE a.target_type = 'user' \
                 AND t.id::text = a.target_id AND t.email ILIKE $5)) \
         ORDER BY a.id DESC LIMIT $6 OFFSET $7",
    )
    .bind(from)
    .bind(to)
    .bind(query.action.trim())
    .bind(filter_user)
    .bind(&user_like)
    .bind(PAGE_SIZE as i64)
    .bind(((page - 1) * PAGE_SIZE) as i64)
    .fetch_all(&state.app.db)
    .await?;
    let total = rows.first().map_or(0, |r| r.8);
    Ok(Json(AuditPage {
        items: rows
            .into_iter()
            .map(|r| AuditRow {
                id: r.0,
                at: rfc3339(r.1),
                actor_email: r.2,
                action: r.3,
                target_type: r.4,
                target_id: r.5,
                ip_hash: r.6,
                details: r.7,
            })
            .collect(),
        page,
        page_size: PAGE_SIZE,
        total,
    }))
}
