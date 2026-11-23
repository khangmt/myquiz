//! The quiz editor API (design section 6, FR-QZ-01 to 11, FR-QZ-18).
//!
//! The editor saves the whole quiz on every autosave with `If-Match:
//! <version>`, so two tabs cannot silently overwrite each other. A draft may
//! be incomplete; only the storage limits are checked on save. Publishing
//! runs the engine's validator, the same one a game and a zip import use.
//!
//! Only the owner sees a quiz; anyone else gets 404, so ids reveal nothing.
//! Admins can read any quiz.

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myquiz_engine::quiz::{
    AnswerOption, ValidationError, MAX_ACCEPTED_ANSWERS, MAX_OPTIONS, OPTION_TEXT_MAX,
    QUESTION_TEXT_MAX, TEXT_ANSWER_MAX as ACCEPTED_ANSWER_MAX, TITLE_MAX,
};
use myquiz_engine::{Question, QuizSnapshot};
use myquiz_protocol::quiz::{
    QuestionDoc, QuizDoc, QuizInvalid, QuizPage, QuizProblemDoc, QuizSummary, SaveQuiz,
};
use myquiz_protocol::ApiErrorBody;
use serde::Deserialize;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use super::audit::{audit, AuditEntry};
use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::media::media_url;
use super::ApiState;

pub const DESCRIPTION_MAX: usize = 1000;
pub const MAX_QUESTIONS: usize = 200;
const PAGE_SIZE: u32 = 20;

pub fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_id(id: &str) -> ApiResult<Uuid> {
    id.parse()
        .map_err(|_| AppError::not_found("QUIZ_NOT_FOUND"))
}

/// A quiz row the caller may see.
pub struct QuizRow {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub title: String,
    pub description: String,
    pub cover_media_id: Option<Uuid>,
    pub cover_media_url: Option<String>,
    pub visibility: String,
    pub status: String,
    pub version: i32,
    pub updated_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

type RawRow = (
    Uuid,
    Uuid,
    String,
    String,
    Option<Uuid>,
    Option<String>,
    String,
    String,
    i32,
    OffsetDateTime,
    OffsetDateTime,
);

const ROW_COLUMNS: &str = "id, owner_id, title, description, cover_media_id, \
     (SELECT storage_key FROM media WHERE media.id = quizzes.cover_media_id), visibility::text, \
     status::text, version, updated_at, last_used_at + make_interval(days => $2)";

/// Loads a live (not deleted) quiz the user owns, or any quiz for an admin
/// when `admin_ok`. Locks the row when `for_update`.
pub async fn load_quiz<'e, E>(
    db: E,
    state: &ApiState,
    user: &AuthUser,
    id: Uuid,
    admin_ok: bool,
    for_update: bool,
) -> ApiResult<QuizRow>
where
    E: sqlx::PgExecutor<'e>,
{
    let lock = if for_update { " FOR UPDATE" } else { "" };
    let row: Option<RawRow> = sqlx::query_as(&format!(
        "SELECT {ROW_COLUMNS} FROM quizzes WHERE id = $1 AND deleted_at IS NULL{lock}"
    ))
    .bind(id)
    .bind(state.retention_days as i32)
    .fetch_optional(db)
    .await?;
    let Some(r) = row else {
        return Err(AppError::not_found("QUIZ_NOT_FOUND"));
    };
    if r.1 != user.id && !(admin_ok && user.is_admin()) {
        return Err(AppError::not_found("QUIZ_NOT_FOUND"));
    }
    Ok(QuizRow {
        id: r.0,
        owner_id: r.1,
        title: r.2,
        description: r.3,
        cover_media_id: r.4,
        cover_media_url: r.5.as_deref().map(media_url),
        visibility: r.6,
        status: r.7,
        version: r.8,
        updated_at: r.9,
        expires_at: r.10,
    })
}

/// The quiz's questions in order, in editor form.
pub async fn load_questions<'e, E>(db: E, quiz_id: Uuid) -> ApiResult<Vec<QuestionDoc>>
where
    E: sqlx::PgExecutor<'e>,
{
    // One query: options are aggregated per question in position order.
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        String,
        String,
        Option<Uuid>,
        Option<String>,
        i32,
        i32,
        Vec<String>,
        bool,
        serde_json::Value,
    )> = sqlx::query_as(
        "SELECT q.type::text, q.text, q.media_id, m.storage_key, q.time_limit_s, q.points, \
                q.accepted_answers, q.ignore_diacritics, \
                COALESCE((SELECT jsonb_agg(jsonb_build_object('text', o.text, 'correct', o.is_correct) \
                                           ORDER BY o.position) \
                          FROM question_options o WHERE o.question_id = q.id), '[]'::jsonb) \
         FROM questions q LEFT JOIN media m ON m.id = q.media_id \
         WHERE q.quiz_id = $1 ORDER BY q.position",
    )
    .bind(quiz_id)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(
            |(kind, text, media, key, time, points, accepted, ignore, options)| {
                Ok(QuestionDoc {
                    kind: serde_json::from_value(json!(kind))?,
                    text,
                    media_id: media.map(|m| m.to_string()),
                    media_url: key.as_deref().map(media_url),
                    time_limit_s: time as u32,
                    points: points as u32,
                    options: serde_json::from_value(options)?,
                    accepted_answers: accepted,
                    ignore_diacritics: ignore,
                })
            },
        )
        .collect::<Result<_, serde_json::Error>>()
        .map_err(AppError::from)
}

fn doc(row: QuizRow, questions: Vec<QuestionDoc>) -> QuizDoc {
    QuizDoc {
        id: row.id.to_string(),
        title: row.title,
        description: row.description,
        cover_media_id: row.cover_media_id.map(|m| m.to_string()),
        cover_media_url: row.cover_media_url,
        visibility: row.visibility,
        status: row.status,
        version: row.version,
        updated_at: rfc3339(row.updated_at),
        expires_at: rfc3339(row.expires_at),
        questions,
    }
}

/// The engine's view of an editor quiz: option ids 1..=n per question. The
/// image is its URL, which is what players' screens need.
pub fn to_snapshot(title: &str, questions: &[QuestionDoc]) -> QuizSnapshot {
    QuizSnapshot {
        title: title.to_string(),
        questions: questions
            .iter()
            .map(|q| Question {
                kind: q.kind,
                text: q.text.clone(),
                media: q.media_url.clone().or_else(|| q.media_id.clone()),
                time_limit_s: q.time_limit_s,
                points: q.points,
                options: q
                    .options
                    .iter()
                    .enumerate()
                    .map(|(i, o)| AnswerOption {
                        id: i as u32 + 1,
                        text: o.text.clone(),
                        is_correct: o.correct,
                    })
                    .collect(),
                accepted_answers: q.accepted_answers.clone(),
                ignore_diacritics: q.ignore_diacritics,
            })
            .collect(),
    }
}

fn code_of(e: ValidationError) -> String {
    serde_json::to_value(e)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn problem(question: Option<usize>, code: &str) -> QuizProblemDoc {
    QuizProblemDoc {
        question: question.map(|q| q as u32),
        code: code.to_string(),
    }
}

/// The publish rules, as editor problems.
pub fn publish_problems(title: &str, questions: &[QuestionDoc]) -> Vec<QuizProblemDoc> {
    to_snapshot(title, questions)
        .validate()
        .into_iter()
        .map(|p| problem(p.question, &code_of(p.error)))
        .collect()
}

/// What a draft must satisfy to be stored at all: the size limits, so a
/// draft can be half-written but never oversized (SEC-21).
pub fn draft_problems(q: &SaveQuiz) -> Vec<QuizProblemDoc> {
    let mut out = Vec::new();
    let title = q.title.trim().chars().count();
    if title == 0 || title > TITLE_MAX {
        out.push(problem(None, "TITLE_LENGTH"));
    }
    if q.description.chars().count() > DESCRIPTION_MAX {
        out.push(problem(None, "DESCRIPTION_LENGTH"));
    }
    if q.visibility != "private" && q.visibility != "public" {
        out.push(problem(None, "VISIBILITY"));
    }
    if q.questions.len() > MAX_QUESTIONS {
        out.push(problem(None, "TOO_MANY_QUESTIONS"));
        return out;
    }
    for (i, question) in q.questions.iter().enumerate() {
        let mut bad = |code: &str| out.push(problem(Some(i), code));
        if question.text.chars().count() > QUESTION_TEXT_MAX {
            bad("QUESTION_TEXT_LENGTH");
        }
        if !(myquiz_engine::quiz::TIME_LIMIT_MIN_S..=myquiz_engine::quiz::TIME_LIMIT_MAX_S)
            .contains(&question.time_limit_s)
        {
            bad("TIME_LIMIT");
        }
        if !myquiz_engine::quiz::ALLOWED_POINTS.contains(&question.points) {
            bad("POINTS");
        }
        if question.options.len() > MAX_OPTIONS {
            bad("OPTION_COUNT");
        }
        if question
            .options
            .iter()
            .any(|o| o.text.chars().count() > OPTION_TEXT_MAX)
        {
            bad("OPTION_TEXT_LENGTH");
        }
        if question.accepted_answers.len() > MAX_ACCEPTED_ANSWERS {
            bad("ACCEPTED_ANSWER_COUNT");
        }
        if question
            .accepted_answers
            .iter()
            .any(|a| a.chars().count() > ACCEPTED_ANSWER_MAX)
        {
            bad("ACCEPTED_ANSWER_LENGTH");
        }
        if question
            .media_id
            .as_deref()
            .is_some_and(|m| m.parse::<Uuid>().is_err())
        {
            bad("MEDIA_NOT_FOUND");
        }
    }
    if q.cover_media_id
        .as_deref()
        .is_some_and(|m| m.parse::<Uuid>().is_err())
    {
        out.push(problem(None, "MEDIA_NOT_FOUND"));
    }
    out
}

fn invalid(status: StatusCode, problems: Vec<QuizProblemDoc>) -> Response {
    let body = QuizInvalid {
        error: ApiErrorBody {
            code: "QUIZ_INVALID".into(),
            message: "the quiz breaks one or more rules".into(),
        },
        problems,
    };
    (status, Json(body)).into_response()
}

/// Every image the quiz refers to must be the user's own upload.
async fn check_media(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    q: &SaveQuiz,
) -> ApiResult<Vec<QuizProblemDoc>> {
    let mut wanted: Vec<(Option<usize>, Uuid)> = Vec::new();
    if let Some(Ok(id)) = q.cover_media_id.as_deref().map(str::parse) {
        wanted.push((None, id));
    }
    for (i, question) in q.questions.iter().enumerate() {
        if let Some(Ok(id)) = question.media_id.as_deref().map(str::parse) {
            wanted.push((Some(i), id));
        }
    }
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = wanted.iter().map(|(_, id)| *id).collect();
    let found: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM media WHERE id = ANY($1) AND owner_id = $2")
            .bind(&ids)
            .bind(owner)
            .fetch_all(&mut **tx)
            .await?;
    Ok(wanted
        .into_iter()
        .filter(|(_, id)| !found.contains(id))
        .map(|(question, _)| problem(question, "MEDIA_NOT_FOUND"))
        .collect())
}

/// Replaces all questions and options of a quiz in one statement each.
pub async fn write_questions(
    tx: &mut Transaction<'_, Postgres>,
    quiz_id: Uuid,
    questions: &[QuestionDoc],
) -> ApiResult<()> {
    sqlx::query("DELETE FROM questions WHERE quiz_id = $1")
        .bind(quiz_id)
        .execute(&mut **tx)
        .await?;
    if questions.is_empty() {
        return Ok(());
    }
    let mut qrows = Vec::with_capacity(questions.len());
    let mut orows = Vec::new();
    for (pos, q) in questions.iter().enumerate() {
        let id = Uuid::new_v4();
        qrows.push(json!({
            "id": id,
            "position": pos,
            "type": q.kind,
            "text": q.text,
            "media_id": q.media_id,
            "time_limit_s": q.time_limit_s,
            "points": q.points,
            "accepted_answers": q.accepted_answers,
            "ignore_diacritics": q.ignore_diacritics,
        }));
        for (opos, o) in q.options.iter().enumerate() {
            orows.push(json!({
                "question_id": id,
                "position": opos,
                "text": o.text,
                "is_correct": o.correct,
            }));
        }
    }
    sqlx::query(
        "INSERT INTO questions (id, quiz_id, position, type, text, media_id, time_limit_s, \
                                points, accepted_answers, ignore_diacritics) \
         SELECT x.id, $1, x.position, x.type, x.text, x.media_id, x.time_limit_s, x.points, \
                x.accepted_answers, x.ignore_diacritics \
         FROM jsonb_to_recordset($2) AS x(id uuid, position int, type question_type, text text, \
              media_id uuid, time_limit_s int, points int, accepted_answers text[], \
              ignore_diacritics boolean)",
    )
    .bind(quiz_id)
    .bind(serde_json::Value::Array(qrows))
    .execute(&mut **tx)
    .await?;
    if !orows.is_empty() {
        sqlx::query(
            "INSERT INTO question_options (question_id, position, text, is_correct) \
             SELECT x.question_id, x.position, x.text, x.is_correct \
             FROM jsonb_to_recordset($1) AS x(question_id uuid, position int, text text, \
                  is_correct boolean)",
        )
        .bind(serde_json::Value::Array(orows))
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn with_etag(status: StatusCode, d: QuizDoc) -> Response {
    let etag = HeaderValue::from_str(&format!("\"{}\"", d.version)).expect("digits");
    let mut res = (status, Json(d)).into_response();
    res.headers_mut().insert(header::ETAG, etag);
    res
}

async fn fetch_doc(state: &ApiState, user: &AuthUser, id: Uuid) -> ApiResult<QuizDoc> {
    let row = load_quiz(&state.app.db, state, user, id, true, false).await?;
    let questions = load_questions(&state.app.db, id).await?;
    Ok(doc(row, questions))
}

// ---------------------------------------------------------------------------
// Handlers

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    q: String,
    page: Option<u32>,
}

/// `GET /quizzes/expiring`: the user's quizzes due for deletion within 3
/// days, soonest first (FR-QZ-19).
pub async fn expiring(
    State(state): State<ApiState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<myquiz_protocol::quiz::ExpiringQuiz>>> {
    let rows: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(
        "SELECT id, title, last_used_at + make_interval(days => $2) AS expires FROM quizzes \
         WHERE owner_id = $1 AND deleted_at IS NULL \
           AND last_used_at + make_interval(days => $2) <= now() + interval '3 days' \
         ORDER BY expires LIMIT 20",
    )
    .bind(user.id)
    .bind(state.retention_days as i32)
    .fetch_all(&state.app.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, title, at)| myquiz_protocol::quiz::ExpiringQuiz {
                id: id.to_string(),
                title,
                expires_at: rfc3339(at),
            })
            .collect(),
    ))
}

/// `GET /quizzes?q=&page=`: the user's quizzes, newest edit first,
/// optionally filtered by title.
pub async fn list(
    State(state): State<ApiState>,
    user: AuthUser,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<QuizPage>> {
    let page = query.page.unwrap_or(1).clamp(1, 10_000);
    let search = query.q.trim().chars().take(100).collect::<String>();
    // ILIKE with the wildcards escaped; the trigram index serves it.
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
        i64,
        OffsetDateTime,
        OffsetDateTime,
        i64,
    )> = sqlx::query_as(
        "SELECT q.id, q.title, q.status::text, q.visibility::text, \
                    (SELECT count(*) FROM questions WHERE quiz_id = q.id), \
                    q.updated_at, q.last_used_at + make_interval(days => $3), \
                    count(*) OVER () \
             FROM quizzes q \
             WHERE q.owner_id = $1 AND q.deleted_at IS NULL AND q.title ILIKE $2 \
             ORDER BY q.updated_at DESC, q.id \
             LIMIT $4 OFFSET $5",
    )
    .bind(user.id)
    .bind(&pattern)
    .bind(state.retention_days as i32)
    .bind(PAGE_SIZE as i64)
    .bind(((page - 1) * PAGE_SIZE) as i64)
    .fetch_all(&state.app.db)
    .await?;
    let total = match rows.first() {
        Some(r) => r.7,
        None if page == 1 => 0,
        None => {
            sqlx::query_scalar(
                "SELECT count(*) FROM quizzes WHERE owner_id = $1 AND deleted_at IS NULL \
             AND title ILIKE $2",
            )
            .bind(user.id)
            .bind(&pattern)
            .fetch_one(&state.app.db)
            .await?
        }
    };
    Ok(Json(QuizPage {
        items: rows
            .into_iter()
            .map(|r| QuizSummary {
                id: r.0.to_string(),
                title: r.1,
                status: r.2,
                visibility: r.3,
                question_count: r.4,
                updated_at: rfc3339(r.5),
                expires_at: rfc3339(r.6),
            })
            .collect(),
        page,
        page_size: PAGE_SIZE,
        total,
    }))
}

/// `POST /quizzes`: a new private draft (FR-QZ-01).
pub async fn create(
    State(state): State<ApiState>,
    user: AuthUser,
    Json(mut body): Json<SaveQuiz>,
) -> ApiResult<Response> {
    body.title = body.title.trim().to_string();
    let problems = draft_problems(&body);
    if !problems.is_empty() {
        return Ok(invalid(StatusCode::UNPROCESSABLE_ENTITY, problems));
    }
    let mut tx = state.app.db.begin().await?;
    let problems = check_media(&mut tx, user.id, &body).await?;
    if !problems.is_empty() {
        return Ok(invalid(StatusCode::UNPROCESSABLE_ENTITY, problems));
    }
    let id = insert_quiz(&mut tx, user.id, &body, "draft").await?;
    tx.commit().await?;
    Ok(with_etag(
        StatusCode::CREATED,
        fetch_doc(&state, &user, id).await?,
    ))
}

/// Inserts a quiz with its questions; used by create, duplicate and import.
pub async fn insert_quiz(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    body: &SaveQuiz,
    status: &str,
) -> ApiResult<Uuid> {
    let cover: Option<Uuid> = body.cover_media_id.as_deref().and_then(|m| m.parse().ok());
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO quizzes (owner_id, title, description, cover_media_id, visibility, status) \
         VALUES ($1, $2, $3, $4, $5::quiz_visibility, $6::quiz_status) RETURNING id",
    )
    .bind(owner)
    .bind(&body.title)
    .bind(&body.description)
    .bind(cover)
    .bind(&body.visibility)
    .bind(status)
    .fetch_one(&mut **tx)
    .await?;
    write_questions(tx, id, &body.questions).await?;
    Ok(id)
}

/// `GET /quizzes/:id`, with the version as ETag.
pub async fn get_one(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let id = parse_id(&id)?;
    Ok(with_etag(
        StatusCode::OK,
        fetch_doc(&state, &user, id).await?,
    ))
}

fn if_match(headers: &HeaderMap) -> ApiResult<i32> {
    let raw = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            AppError::new(
                StatusCode::PRECONDITION_REQUIRED,
                "VERSION_REQUIRED",
                "send the quiz version in If-Match",
            )
        })?;
    raw.trim()
        .trim_start_matches("W/")
        .trim_matches('"')
        .parse()
        .map_err(|_| AppError::bad_request("VERSION_REQUIRED", "If-Match must be the version"))
}

/// `PUT /quizzes/:id`: saves the whole quiz (autosave). Refused with 412
/// VERSION_CONFLICT when someone saved in between. A published quiz that no
/// longer passes the publish rules goes back to draft.
pub async fn save(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut body): Json<SaveQuiz>,
) -> ApiResult<Response> {
    let id = parse_id(&id)?;
    let expected = if_match(&headers)?;
    body.title = body.title.trim().to_string();
    let problems = draft_problems(&body);
    if !problems.is_empty() {
        return Ok(invalid(StatusCode::UNPROCESSABLE_ENTITY, problems));
    }

    let mut tx = state.app.db.begin().await?;
    // Owner only: admins can read but not edit someone's quiz.
    let row = load_quiz(&mut *tx, &state, &user, id, false, true).await?;
    if row.version != expected {
        return Err(AppError::new(
            StatusCode::PRECONDITION_FAILED,
            "VERSION_CONFLICT",
            format!("the quiz is at version {}", row.version),
        ));
    }
    let problems = check_media(&mut tx, row.owner_id, &body).await?;
    if !problems.is_empty() {
        return Ok(invalid(StatusCode::UNPROCESSABLE_ENTITY, problems));
    }
    let status =
        if row.status == "published" && publish_problems(&body.title, &body.questions).is_empty() {
            "published"
        } else {
            "draft"
        };
    sqlx::query(
        "UPDATE quizzes SET title = $2, description = $3, cover_media_id = $4, \
             visibility = $5::quiz_visibility, status = $6::quiz_status, version = version + 1, \
             updated_at = now(), last_used_at = now() \
         WHERE id = $1",
    )
    .bind(id)
    .bind(&body.title)
    .bind(&body.description)
    .bind(
        body.cover_media_id
            .as_deref()
            .and_then(|m| m.parse::<Uuid>().ok()),
    )
    .bind(&body.visibility)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    write_questions(&mut tx, id, &body.questions).await?;
    tx.commit().await?;
    Ok(with_etag(
        StatusCode::OK,
        fetch_doc(&state, &user, id).await?,
    ))
}

/// `DELETE /quizzes/:id`: soft delete, purged after 30 days (FR-QZ-11).
pub async fn delete(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<StatusCode> {
    let id = parse_id(&id)?;
    let row = load_quiz(&state.app.db, &state, &user, id, false, false).await?;
    sqlx::query("UPDATE quizzes SET deleted_at = now() WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .execute(&state.app.db)
        .await?;
    audit(
        &state.app.db,
        &state.auth.app_secret,
        &headers,
        AuditEntry {
            actor: Some(user.id),
            action: "quiz_deleted",
            target_type: Some("quiz"),
            target_id: Some(id.to_string()),
            details: json!({ "title": row.title }),
        },
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /quizzes/:id/duplicate`: a private draft copy named "… (copy)".
pub async fn duplicate(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let id = parse_id(&id)?;
    let row = load_quiz(&state.app.db, &state, &user, id, false, false).await?;
    let questions = load_questions(&state.app.db, id).await?;
    let suffix = " (copy)";
    let keep = TITLE_MAX - suffix.chars().count();
    let title: String = row.title.chars().take(keep).collect::<String>() + suffix;
    let body = SaveQuiz {
        title,
        description: row.description,
        cover_media_id: row.cover_media_id.map(|m| m.to_string()),
        visibility: "private".into(),
        questions,
    };
    let mut tx = state.app.db.begin().await?;
    let new_id = insert_quiz(&mut tx, user.id, &body, "draft").await?;
    tx.commit().await?;
    Ok(with_etag(
        StatusCode::CREATED,
        fetch_doc(&state, &user, new_id).await?,
    ))
}

/// `POST /quizzes/:id/publish`: checks every rule (FR-QZ-10). 422 with the
/// full problem list, so the editor can mark every question at once.
pub async fn publish(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let id = parse_id(&id)?;
    let mut tx = state.app.db.begin().await?;
    let row = load_quiz(&mut *tx, &state, &user, id, false, true).await?;
    let questions = load_questions(&mut *tx, id).await?;
    let problems = publish_problems(&row.title, &questions);
    if !problems.is_empty() {
        return Ok(invalid(StatusCode::UNPROCESSABLE_ENTITY, problems));
    }
    sqlx::query("UPDATE quizzes SET status = 'published', last_used_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(with_etag(
        StatusCode::OK,
        fetch_doc(&state, &user, id).await?,
    ))
}

#[cfg(test)]
mod unit {
    use super::*;
    use myquiz_engine::QuestionKind;
    use myquiz_protocol::quiz::OptionDoc;

    fn q(kind: QuestionKind) -> QuestionDoc {
        QuestionDoc {
            kind,
            text: "2 + 2?".into(),
            media_id: None,
            media_url: None,
            time_limit_s: 20,
            points: 1000,
            options: vec![
                OptionDoc {
                    text: "4".into(),
                    correct: true,
                },
                OptionDoc {
                    text: "5".into(),
                    correct: false,
                },
            ],
            accepted_answers: vec![],
            ignore_diacritics: false,
        }
    }

    fn quiz(questions: Vec<QuestionDoc>) -> SaveQuiz {
        SaveQuiz {
            title: "Maths".into(),
            description: String::new(),
            cover_media_id: None,
            visibility: "private".into(),
            questions,
        }
    }

    #[test]
    fn drafts_may_be_incomplete_but_not_oversized() {
        let mut half = q(QuestionKind::Single);
        half.text.clear();
        half.options[0].text.clear();
        half.options[0].correct = false;
        assert!(draft_problems(&quiz(vec![half.clone()])).is_empty());
        // ...but it cannot be published.
        let codes: Vec<_> = publish_problems("Maths", &[half])
            .into_iter()
            .map(|p| p.code)
            .collect();
        assert!(
            codes.contains(&"QUESTION_TEXT_LENGTH".to_string()),
            "{codes:?}"
        );
        assert!(
            codes.contains(&"NO_CORRECT_OPTION".to_string()),
            "{codes:?}"
        );

        let mut big = q(QuestionKind::Single);
        big.text = "x".repeat(301);
        big.time_limit_s = 3;
        big.points = 500;
        let codes: Vec<_> = draft_problems(&quiz(vec![big]))
            .into_iter()
            .map(|p| p.code)
            .collect();
        assert_eq!(codes, ["QUESTION_TEXT_LENGTH", "TIME_LIMIT", "POINTS"]);

        let mut untitled = quiz(vec![]);
        untitled.title = "  ".into();
        untitled.visibility = "secret".into();
        let codes: Vec<_> = draft_problems(&untitled)
            .into_iter()
            .map(|p| p.code)
            .collect();
        assert_eq!(codes, ["TITLE_LENGTH", "VISIBILITY"]);
    }

    #[test]
    fn publish_codes_match_the_engine() {
        assert!(publish_problems("Maths", &[q(QuestionKind::Single)]).is_empty());
        let p = publish_problems("Maths", &[]);
        assert_eq!(p[0].code, "NO_QUESTIONS");
        assert_eq!(p[0].question, None);
    }
}
