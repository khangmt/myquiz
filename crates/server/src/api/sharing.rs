//! Sharing a quiz: making it public, or sharing it with specific people by
//! email. Others can host a shared quiz or copy it into their own quizzes;
//! only the owner can edit it. Only a published quiz is visible to others,
//! so a half-made draft never shows.
//!
//! A quiz the caller cannot use answers 404, like any other quiz id.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use myquiz_protocol::quiz::{LibraryPage, LibraryQuiz, QuizSharing};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use super::audit::{audit, AuditEntry};
use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::media::media_url;
use super::quizzes::{fetch_row, load_quiz, rfc3339, QuizRow};
use super::ApiState;

pub const MAX_SHARES: usize = 100;
const EMAIL_MAX: usize = 254;
const PAGE_SIZE: u32 = 20;

fn parse_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| AppError::not_found("QUIZ_NOT_FOUND"))
}

/// Loads a live quiz the user may host or copy: their own, or someone
/// else's published quiz that is public or shared with the user's email.
pub async fn load_usable(state: &ApiState, user: &AuthUser, id: Uuid) -> ApiResult<QuizRow> {
    let Some(row) = fetch_row(&state.app.db, state, id, false).await? else {
        return Err(AppError::not_found("QUIZ_NOT_FOUND"));
    };
    if row.owner_id == user.id {
        return Ok(row);
    }
    let usable: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM quizzes q JOIN users u ON u.id = q.owner_id \
         WHERE q.id = $1 AND q.deleted_at IS NULL AND q.status = 'published' \
           AND u.deleted_at IS NULL AND u.status = 'active' \
           AND (q.visibility = 'public' \
                OR EXISTS (SELECT 1 FROM quiz_shares s WHERE s.quiz_id = q.id AND s.email = lower($2))))",
    )
    .bind(id)
    .bind(&user.email)
    .fetch_one(&state.app.db)
    .await?;
    if usable {
        Ok(row)
    } else {
        Err(AppError::not_found("QUIZ_NOT_FOUND"))
    }
}

/// Lower case, trimmed, and shaped like an address, or None.
pub fn clean_email(raw: &str) -> Option<String> {
    let e = raw.trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && e.chars().count() <= EMAIL_MAX
        && !e
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == ',' || c == ';');
    ok.then_some(e)
}

async fn emails_of(state: &ApiState, quiz: Uuid) -> ApiResult<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT email FROM quiz_shares WHERE quiz_id = $1 ORDER BY email")
            .bind(quiz)
            .fetch_all(&state.app.db)
            .await?,
    )
}

/// `GET /quizzes/:id/sharing` (owner only).
pub async fn get(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Json<QuizSharing>> {
    let id = parse_id(&id)?;
    let row = load_quiz(&state.app.db, &state, &user, id, false, false).await?;
    Ok(Json(QuizSharing {
        visibility: row.visibility,
        emails: emails_of(&state, id).await?,
    }))
}

/// `PUT /quizzes/:id/sharing`: sets visibility and replaces the list of
/// people (owner only). A visibility change bumps the quiz version, so an
/// editor open elsewhere reloads instead of saving the old value back.
pub async fn put(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<QuizSharing>,
) -> ApiResult<Json<QuizSharing>> {
    let id = parse_id(&id)?;
    if body.visibility != "private" && body.visibility != "public" {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VISIBILITY",
            "visibility is private or public",
        ));
    }
    let own = user.email.to_lowercase();
    let mut emails = Vec::new();
    for raw in &body.emails {
        if raw.trim().is_empty() {
            continue;
        }
        let Some(e) = clean_email(raw) else {
            return Err(AppError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "EMAIL_INVALID",
                format!("not an email address: {}", raw.trim()),
            ));
        };
        if e != own && !emails.contains(&e) {
            emails.push(e);
        }
    }
    if emails.len() > MAX_SHARES {
        return Err(AppError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "TOO_MANY_SHARES",
            "a quiz can be shared with at most 100 people",
        ));
    }
    emails.sort();

    let mut tx = state.app.db.begin().await?;
    let row = load_quiz(&mut *tx, &state, &user, id, false, true).await?;
    if row.visibility != body.visibility {
        sqlx::query(
            "UPDATE quizzes SET visibility = $2::quiz_visibility, version = version + 1 \
             WHERE id = $1",
        )
        .bind(id)
        .bind(&body.visibility)
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query("DELETE FROM quiz_shares WHERE quiz_id = $1 AND NOT (email = ANY($2))")
        .bind(id)
        .bind(&emails)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO quiz_shares (quiz_id, email) SELECT $1, unnest($2::text[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(id)
    .bind(&emails)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    audit(
        &state.app.db,
        &state.auth.app_secret,
        &headers,
        AuditEntry {
            actor: Some(user.id),
            action: "quiz_sharing_changed",
            target_type: Some("quiz"),
            target_id: Some(id.to_string()),
            details: json!({ "visibility": body.visibility, "people": emails.len() }),
        },
    )
    .await;
    Ok(Json(QuizSharing {
        visibility: body.visibility,
        emails,
    }))
}

#[derive(Deserialize)]
pub struct LibraryQuery {
    #[serde(default)]
    scope: String,
    #[serde(default)]
    q: String,
    page: Option<u32>,
}

/// `GET /library?scope=public|shared&q=&page=`: other people's published
/// quizzes that are public, or shared with the caller. Newest edit first.
pub async fn library(
    State(state): State<ApiState>,
    user: AuthUser,
    Query(query): Query<LibraryQuery>,
) -> ApiResult<Json<LibraryPage>> {
    let filter = match query.scope.as_str() {
        "public" | "" => "q.visibility = 'public'",
        "shared" => {
            "EXISTS (SELECT 1 FROM quiz_shares s WHERE s.quiz_id = q.id AND s.email = lower($2))"
        }
        _ => {
            return Err(AppError::bad_request(
                "BAD_SCOPE",
                "scope is public or shared",
            ))
        }
    };
    let page = query.page.unwrap_or(1).clamp(1, 10_000);
    let search = query.q.trim().chars().take(100).collect::<String>();
    let pattern = format!(
        "%{}%",
        search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        Option<String>,
        i64,
        OffsetDateTime,
        i64,
    )> = sqlx::query_as(&format!(
        "SELECT q.id, q.title, q.description, u.display_name, \
                (SELECT storage_key FROM media WHERE media.id = q.cover_media_id), \
                (SELECT count(*) FROM questions WHERE quiz_id = q.id), \
                q.updated_at, count(*) OVER () \
         FROM quizzes q JOIN users u ON u.id = q.owner_id \
         WHERE q.owner_id <> $1 AND q.deleted_at IS NULL AND q.status = 'published' \
           AND u.deleted_at IS NULL AND u.status = 'active' \
           AND {filter} AND q.title ILIKE $3 \
         ORDER BY q.updated_at DESC, q.id \
         LIMIT $4 OFFSET $5"
    ))
    .bind(user.id)
    .bind(&user.email)
    .bind(&pattern)
    .bind(PAGE_SIZE as i64)
    .bind(((page - 1) * PAGE_SIZE) as i64)
    .fetch_all(&state.app.db)
    .await?;
    let total = rows.first().map_or(0, |r| r.7);
    Ok(Json(LibraryPage {
        items: rows
            .into_iter()
            .map(|r| LibraryQuiz {
                id: r.0.to_string(),
                title: r.1,
                description: r.2,
                owner_name: r.3,
                cover_media_url: r.4.as_deref().map(media_url),
                question_count: r.5,
                updated_at: rfc3339(r.6),
            })
            .collect(),
        page,
        page_size: PAGE_SIZE,
        total,
    }))
}

#[cfg(test)]
mod unit {
    use super::clean_email;

    #[test]
    fn emails_are_cleaned_or_refused() {
        assert_eq!(
            clean_email("  An@UIT.edu.vn ").as_deref(),
            Some("an@uit.edu.vn")
        );
        for bad in [
            "",
            "an",
            "@uit.edu.vn",
            "an@",
            "an@uit",
            "a b@x.vn",
            "a@b@x.vn",
            "a@x.vn,b@x.vn",
            "a@.vn",
        ] {
            assert_eq!(clean_email(bad), None, "{bad}");
        }
    }
}
