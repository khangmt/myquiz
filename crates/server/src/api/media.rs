//! `POST /media`: image upload for covers and questions (FR-QZ-01, FR-QZ-07,
//! SEC-11). Multipart with a `file` field and an optional `alt` field.

use axum::extract::{Multipart, State};
use axum::http::StatusCode;
use axum::Json;
use myquiz_protocol::quiz::MediaDoc;
use uuid::Uuid;

use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::{ratelimit, ApiState};
use crate::media::{self, MediaError, Processed};

/// The 5 MB image plus multipart overhead.
pub const MAX_BODY: usize = media::MAX_INPUT_BYTES + 64 * 1024;
pub const ALT_MAX: usize = 300;
const UPLOADS_PER_HOUR: u64 = 120;

pub fn media_url(storage_key: &str) -> String {
    format!("/media/{storage_key}")
}

fn refused(e: MediaError) -> AppError {
    let status = match e {
        MediaError::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        _ => StatusCode::UNPROCESSABLE_ENTITY,
    };
    AppError::new(status, e.code(), "the file is not an accepted image")
}

/// Runs the pipeline off the async threads, at most 2 at a time.
pub async fn process(state: &ApiState, bytes: Vec<u8>) -> ApiResult<Result<Processed, MediaError>> {
    let _permit = state.image_permits.acquire().await?;
    Ok(tokio::task::spawn_blocking(move || media::process(&bytes)).await?)
}

/// Stores a processed image and its row. Used by uploads and zip import.
pub async fn save(
    state: &ApiState,
    owner: Uuid,
    image: &Processed,
    alt: Option<&str>,
) -> ApiResult<MediaDoc> {
    let key = media::new_storage_key();
    media::store(&state.media_dir, &key, &image.webp).await?;
    let inserted: Result<Uuid, _> = sqlx::query_scalar(
        "INSERT INTO media (owner_id, storage_key, mime, bytes, width, height, alt_text) \
         VALUES ($1, $2, 'image/webp', $3, $4, $5, $6) RETURNING id",
    )
    .bind(owner)
    .bind(&key)
    .bind(image.webp.len() as i32)
    .bind(image.width as i32)
    .bind(image.height as i32)
    .bind(alt)
    .fetch_one(&state.app.db)
    .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            media::remove(&state.media_dir, &key).await;
            return Err(e.into());
        }
    };
    Ok(MediaDoc {
        id: id.to_string(),
        url: media_url(&key),
        width: image.width,
        height: image.height,
        alt_text: alt.map(str::to_string),
    })
}

pub async fn upload(
    State(state): State<ApiState>,
    user: AuthUser,
    mut form: Multipart,
) -> ApiResult<(StatusCode, Json<MediaDoc>)> {
    ratelimit::hit(
        &state.app.redis,
        &format!("media:{}", user.id),
        UPLOADS_PER_HOUR,
        3600,
    )
    .await?;
    let bad_form = || AppError::bad_request("BAD_UPLOAD", "send a multipart form with a file");
    let mut file: Option<Vec<u8>> = None;
    let mut alt: Option<String> = None;
    while let Some(field) = form.next_field().await.map_err(|_| bad_form())? {
        match field.name() {
            Some("file") => {
                let bytes = field.bytes().await.map_err(|_| bad_form())?;
                if bytes.len() > media::MAX_INPUT_BYTES {
                    return Err(refused(MediaError::TooLarge));
                }
                file = Some(bytes.to_vec());
            }
            Some("alt") => {
                let text = field.text().await.map_err(|_| bad_form())?;
                let text = text.trim().to_string();
                if text.chars().count() > ALT_MAX {
                    return Err(AppError::bad_request(
                        "ALT_TEXT_LENGTH",
                        "at most 300 characters",
                    ));
                }
                alt = Some(text).filter(|t| !t.is_empty());
            }
            _ => {}
        }
    }
    let file = file.ok_or_else(bad_form)?;
    let image = process(&state, file).await?.map_err(refused)?;
    let doc = save(&state, user.id, &image, alt.as_deref()).await?;
    Ok((StatusCode::CREATED, Json(doc)))
}
