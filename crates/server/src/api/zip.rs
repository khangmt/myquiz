//! Quiz zips over HTTP (FR-QZ-15 to 17, SEC-31 to 34, NFR-21, NFR-22).
//!
//! - `POST /quizzes/:id/export` gives the owner (or an admin) a signed
//!   download link valid 10 minutes; `GET /exports/:token` serves the zip.
//! - `POST /quizzes/import` turns an uploaded zip into a new private draft.
//! - `GET /quiz-template.zip` is a sample to start from.
//!
//! Zips are built and read in a temporary file, never fully in memory.

use std::collections::BTreeMap;
use std::io::{Seek, SeekFrom};

use axum::body::Body;
use axum::extract::{Multipart, Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use myquiz_bundle::format::{OptionV1, QuestionV1, QuizFileV1, QuizV1};
use myquiz_bundle::{read_bundle, Limits, Problem};
use myquiz_protocol::quiz::{
    ExportLink, ImportProblem, ImportRefused, Imported, QuestionDoc, SaveQuiz,
};
use myquiz_protocol::ApiErrorBody;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use super::audit::{audit, AuditEntry};
use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::quizzes::{insert_quiz, load_questions, load_quiz, publish_problems, rfc3339};
use super::{media as media_api, ratelimit, signed, ApiState};
use crate::media::{self, is_storage_key, MediaError};

/// Caddy caps the body at 25 MB too (design section 8).
pub const MAX_IMPORT_BODY: usize = 25 * 1024 * 1024;
const IMPORTS_PER_HOUR: u64 = 10;

fn zip_response(file: std::fs::File, name: &str) -> ApiResult<Response> {
    let len = file.metadata()?.len();
    let stream = tokio_util::io::ReaderStream::new(tokio::fs::File::from_std(file));
    let mut res = Body::from_stream(stream).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    // The name is an ASCII slug (write::file_name), safe in a header.
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))?,
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(res)
}

fn today_yyyymmdd() -> String {
    let d = time::OffsetDateTime::now_utc().date();
    format!("{:04}{:02}{:02}", d.year(), u8::from(d.month()), d.day())
}

// ---------------------------------------------------------------------------
// Export

/// `POST /quizzes/:id/export`. Refused with 422 QUIZ_INVALID when the quiz
/// would not import again (the zip must always be uploadable back).
pub async fn export(
    State(state): State<ApiState>,
    user: AuthUser,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let id: Uuid = id
        .parse()
        .map_err(|_| AppError::not_found("QUIZ_NOT_FOUND"))?;
    let row = load_quiz(&state.app.db, &state, &user, id, true, false).await?;
    let questions = load_questions(&state.app.db, id).await?;
    let problems = publish_problems(&row.title, &questions);
    if !problems.is_empty() {
        let body = myquiz_protocol::quiz::QuizInvalid {
            error: ApiErrorBody {
                code: "QUIZ_INVALID".into(),
                message: "fix the quiz before downloading it".into(),
            },
            problems,
        };
        return Ok((StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response());
    }
    let (token, exp) = signed::sign(&state.auth.app_secret, &format!("quiz:{id}"));
    audit(
        &state.app.db,
        &state.auth.app_secret,
        &headers,
        AuditEntry {
            actor: Some(user.id),
            action: "quiz_exported",
            target_type: Some("quiz"),
            target_id: Some(id.to_string()),
            details: json!({ "title": row.title }),
        },
    )
    .await;
    let expires = time::OffsetDateTime::from_unix_timestamp(exp as i64)?;
    Ok(Json(ExportLink {
        url: format!("/api/v1/exports/{token}"),
        expires_at: rfc3339(expires),
    })
    .into_response())
}

/// `GET /exports/:token`: whoever holds a valid link gets the zip.
pub async fn download(
    State(state): State<ApiState>,
    Path(token): Path<String>,
) -> ApiResult<Response> {
    let gone = || {
        AppError::new(
            StatusCode::GONE,
            "LINK_EXPIRED",
            "this download link has expired; ask for a new one",
        )
    };
    let subject = signed::verify(&state.auth.app_secret, &token).ok_or_else(gone)?;
    // One link endpoint serves quiz zips and game reports (SEC-23).
    if let Some(res) = super::reports::serve(&state.app.db, &subject).await? {
        return Ok(res);
    }
    let id: Uuid = subject
        .strip_prefix("quiz:")
        .and_then(|s| s.parse().ok())
        .ok_or_else(gone)?;

    #[allow(clippy::type_complexity)]
    let row: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT q.title, q.description, m.storage_key, m.alt_text FROM quizzes q \
         LEFT JOIN media m ON m.id = q.cover_media_id \
         WHERE q.id = $1 AND q.deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&state.app.db)
    .await?;
    let (title, description, cover_key, _) =
        row.ok_or_else(|| AppError::not_found("QUIZ_NOT_FOUND"))?;
    let questions = load_questions(&state.app.db, id).await?;
    let alts: BTreeMap<String, Option<String>> = media_alts(&state, &questions).await?;

    // Images keep their order: media/cover.webp, media/q1.webp, ...
    let mut images: Vec<(String, String)> = Vec::new();
    let cover = cover_key.filter(|k| is_storage_key(k)).map(|k| {
        images.push(("media/cover.webp".into(), k));
        "media/cover.webp".to_string()
    });
    let mut out = Vec::with_capacity(questions.len());
    for (i, q) in questions.iter().enumerate() {
        let key = q
            .media_url
            .as_deref()
            .and_then(|u| u.strip_prefix("/media/"))
            .filter(|k| is_storage_key(k));
        let image = key.map(|k| {
            let path = format!("media/q{}.webp", i + 1);
            images.push((path.clone(), k.to_string()));
            path
        });
        out.push(QuestionV1 {
            kind: q.kind,
            text: q.text.clone(),
            image_alt: q
                .media_id
                .as_ref()
                .and_then(|m| alts.get(m).cloned().flatten()),
            image,
            markdown: q.markdown.clone().filter(|m| !m.trim().is_empty()),
            time_limit_s: q.time_limit_s,
            points: q.points,
            options: q
                .options
                .iter()
                .map(|o| OptionV1 {
                    text: o.text.clone(),
                    correct: o.correct,
                })
                .collect(),
            accepted_answers: q.accepted_answers.clone(),
            ignore_diacritics: q.ignore_diacritics,
        });
    }
    let file = QuizFileV1::new(
        QuizV1 {
            title: title.clone(),
            description,
            cover,
            questions: out,
        },
        Some(rfc3339(time::OffsetDateTime::now_utc())),
    );

    let dir = state.media_dir.clone();
    let zip = tokio::task::spawn_blocking(move || -> anyhow::Result<std::fs::File> {
        let tmp = tempfile::tempfile()?;
        // Each image is read only when its turn comes, so memory stays flat.
        let entries = images.into_iter().filter_map(|(path, key)| {
            std::fs::read(media::path_for(&dir, &key))
                .map_err(|e| tracing::warn!(error = %e, key, "export: image missing"))
                .ok()
                .map(|bytes| (path, bytes))
        });
        let mut tmp = myquiz_bundle::write_bundle(tmp, &file, entries)?;
        tmp.seek(SeekFrom::Start(0))?;
        Ok(tmp)
    })
    .await??;
    zip_response(zip, &myquiz_bundle::file_name(&title, &today_yyyymmdd()))
}

async fn media_alts(
    state: &ApiState,
    questions: &[QuestionDoc],
) -> ApiResult<BTreeMap<String, Option<String>>> {
    let ids: Vec<Uuid> = questions
        .iter()
        .filter_map(|q| q.media_id.as_deref()?.parse().ok())
        .collect();
    let rows: Vec<(Uuid, Option<String>)> =
        sqlx::query_as("SELECT id, alt_text FROM media WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(&state.app.db)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, alt)| (id.to_string(), alt))
        .collect())
}

/// `GET /quiz-template.zip`: one question of each type, no images.
pub async fn template() -> ApiResult<Response> {
    let zip = tokio::task::spawn_blocking(|| -> anyhow::Result<std::fs::File> {
        let mut tmp = myquiz_bundle::write_bundle(
            tempfile::tempfile()?,
            &myquiz_bundle::template(),
            Vec::<(String, Vec<u8>)>::new(),
        )?;
        tmp.seek(SeekFrom::Start(0))?;
        Ok(tmp)
    })
    .await??;
    zip_response(zip, "myquiz-template.zip")
}

// ---------------------------------------------------------------------------
// Import

fn refused(status: StatusCode, code: &str, problems: Vec<Problem>) -> Response {
    let body = ImportRefused {
        error: ApiErrorBody {
            code: code.into(),
            message: "the zip cannot be imported".into(),
        },
        errors: problems
            .into_iter()
            .map(|p| ImportProblem {
                file: p.file,
                question: p.question.map(|q| q as u32),
                field: p.field,
                problem: p.problem,
            })
            .collect(),
    };
    (status, Json(body)).into_response()
}

/// `POST /quizzes/import` (multipart, field `file`). 201 `{ quizId }`, or
/// 422 with every problem found; nothing is saved unless all of it is.
pub async fn import(
    State(state): State<ApiState>,
    user: AuthUser,
    headers: HeaderMap,
    mut form: Multipart,
) -> ApiResult<Response> {
    ratelimit::hit(
        &state.app.redis,
        &format!("import:{}", user.id),
        IMPORTS_PER_HOUR,
        3600,
    )
    .await?;
    // Two imports at a time per process; refuse rather than queue.
    let Ok(_permit) = state.import_permits.clone().try_acquire_owned() else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "IMPORT_BUSY",
            "the server is busy importing; try again in a minute",
        ));
    };

    // Stream the upload to an unnamed temp file: the zip's index is at the
    // end, so it has to be on disk before it can be read.
    let bad_form = || AppError::bad_request("BAD_UPLOAD", "send a multipart form with a file");
    let mut saved: Option<std::fs::File> = None;
    while let Some(mut field) = form.next_field().await.map_err(|_| bad_form())? {
        if field.name() != Some("file") {
            continue;
        }
        let mut tmp = tokio::fs::File::from_std(tempfile::tempfile()?);
        let mut written = 0usize;
        while let Some(chunk) = field.chunk().await.map_err(|_| bad_form())? {
            written += chunk.len();
            if written > MAX_IMPORT_BODY {
                return Err(AppError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "FILE_TOO_LARGE",
                    "at most 25 MB",
                ));
            }
            tmp.write_all(&chunk).await?;
        }
        tmp.flush().await?;
        saved = Some(tmp.into_std().await);
    }
    let mut file = saved.ok_or_else(bad_form)?;
    file.seek(SeekFrom::Start(0))?;

    let bundle = tokio::task::spawn_blocking(move || read_bundle(file, &Limits::default())).await?;
    let bundle = match bundle {
        Ok(b) => b,
        Err(problems) => {
            return Ok(refused(
                StatusCode::UNPROCESSABLE_ENTITY,
                "IMPORT_INVALID",
                problems,
            ))
        }
    };

    // Every image through the normal pipeline; collect all failures.
    let mut processed = BTreeMap::new();
    let mut problems = Vec::new();
    for (path, bytes) in bundle.images {
        match media_api::process(&state, bytes).await? {
            Ok(img) => {
                processed.insert(path, img);
            }
            Err(e) => problems.push(Problem {
                file: path,
                question: None,
                field: None,
                problem: match e {
                    MediaError::TooLarge => "FILE_TOO_LARGE",
                    MediaError::TooManyPixels => "IMAGE_TOO_BIG",
                    MediaError::NotAnImage => "NOT_AN_IMAGE",
                }
                .into(),
                detail: None,
            }),
        }
    }
    if !problems.is_empty() {
        return Ok(refused(
            StatusCode::UNPROCESSABLE_ENTITY,
            "IMPORT_INVALID",
            problems,
        ));
    }

    // Files first (under fresh random names), then one transaction for
    // every row; on any failure the transaction rolls back and the files
    // are removed, so nothing partial is left (NFR-22).
    let quiz = bundle.quiz;
    let alt_for = |path: &str| -> Option<String> {
        quiz.questions
            .iter()
            .find(|q| q.image.as_deref() == Some(path))
            .and_then(|q| q.image_alt.clone())
    };
    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    let result = async {
        for (path, img) in &processed {
            let key = media::new_storage_key();
            media::store(&state.media_dir, &key, &img.webp).await?;
            keys.insert(path.clone(), key);
        }
        let mut tx = state.app.db.begin().await?;
        let mut ids: BTreeMap<String, String> = BTreeMap::new();
        for (path, img) in &processed {
            let id: Uuid = sqlx::query_scalar(
                "INSERT INTO media (owner_id, storage_key, mime, bytes, width, height, alt_text) \
                 VALUES ($1, $2, 'image/webp', $3, $4, $5, $6) RETURNING id",
            )
            .bind(user.id)
            .bind(&keys[path])
            .bind(img.webp.len() as i32)
            .bind(img.width as i32)
            .bind(img.height as i32)
            .bind(alt_for(path))
            .fetch_one(&mut *tx)
            .await?;
            ids.insert(path.clone(), id.to_string());
        }
        let body = SaveQuiz {
            title: quiz.title.trim().to_string(),
            description: quiz.description.clone(),
            cover_media_id: quiz.cover.as_ref().and_then(|c| ids.get(c).cloned()),
            visibility: "private".into(),
            questions: quiz
                .questions
                .iter()
                .map(|q| QuestionDoc {
                    kind: q.kind,
                    text: q.text.clone(),
                    media_id: q.image.as_ref().and_then(|p| ids.get(p).cloned()),
                    media_url: None,
                    markdown: q.markdown.clone(),
                    time_limit_s: q.time_limit_s,
                    points: q.points,
                    options: q
                        .options
                        .iter()
                        .map(|o| myquiz_protocol::quiz::OptionDoc {
                            text: o.text.clone(),
                            correct: o.correct,
                        })
                        .collect(),
                    accepted_answers: q.accepted_answers.clone(),
                    ignore_diacritics: q.ignore_diacritics,
                })
                .collect(),
        };
        let quiz_id = insert_quiz(&mut tx, user.id, &body, "draft").await?;
        tx.commit().await?;
        ApiResult::Ok(quiz_id)
    }
    .await;

    let quiz_id = match result {
        Ok(id) => id,
        Err(e) => {
            for key in keys.values() {
                media::remove(&state.media_dir, key).await;
            }
            return Err(e);
        }
    };
    audit(
        &state.app.db,
        &state.auth.app_secret,
        &headers,
        AuditEntry {
            actor: Some(user.id),
            action: "quiz_imported",
            target_type: Some("quiz"),
            target_id: Some(quiz_id.to_string()),
            details: json!({ "title": quiz.title, "images": processed.len() }),
        },
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(Imported {
            quiz_id: quiz_id.to_string(),
        }),
    )
        .into_response())
}
