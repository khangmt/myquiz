//! Starting a game and checking a PIN (design section 6, FR-GM-01, FR-GM-02).

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use fred::prelude::*;
use myquiz_engine::GameState;
use myquiz_protocol::quiz::{CreateGame, GameCreated, PinCheck};
use uuid::Uuid;

use super::audit::{client_ip, hash_ip};
use super::auth::AuthUser;
use super::error::{ApiResult, AppError};
use super::quizzes::{load_questions, load_quiz, to_snapshot};
use super::{ratelimit, ApiState};
use crate::live::keys;

/// `POST /games { quizId }`: the owner starts a lobby for a published quiz.
/// The quiz is copied into the session, so later edits never change this
/// game or its report.
pub async fn create(
    State(state): State<ApiState>,
    user: AuthUser,
    Json(body): Json<CreateGame>,
) -> ApiResult<(StatusCode, Json<GameCreated>)> {
    let quiz_id: Uuid = body
        .quiz_id
        .parse()
        .map_err(|_| AppError::not_found("QUIZ_NOT_FOUND"))?;
    let name = super::reports::clean_name(body.name.as_deref())?;
    let row = load_quiz(&state.app.db, &state, &user, quiz_id, false, false).await?;
    if row.status != "published" {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "QUIZ_NOT_PUBLISHED",
            "publish the quiz before starting a game",
        ));
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_sessions WHERE status <> 'ended' AND pin IS NOT NULL",
    )
    .fetch_one(&state.app.db)
    .await?;
    if active >= i64::from(state.max_active_games) {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "CAPACITY_FULL",
            "the server is running as many games as it can; try again in a few minutes",
        ));
    }

    let snapshot = to_snapshot(&row.title, &load_questions(&state.app.db, quiz_id).await?);
    let session = Uuid::new_v4();
    let pin = state
        .live
        .create_game(&session.to_string(), &user.id.to_string(), &snapshot)
        .await?;
    let inserted = sqlx::query(
        "INSERT INTO game_sessions (id, quiz_id, host_id, pin, quiz_snapshot, name) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(session)
    .bind(quiz_id)
    .bind(user.id)
    .bind(&pin)
    .bind(serde_json::to_value(&snapshot)?)
    .bind(&name)
    .execute(&state.app.db)
    .await;
    if let Err(e) = inserted {
        // Give the PIN back so no half-made game lingers in Redis.
        let _: Result<i64, _> = state
            .app
            .redis
            .del(vec![keys::pin(&pin), keys::game(&pin), keys::quiz(&pin)])
            .await;
        return Err(e.into());
    }
    sqlx::query("UPDATE quizzes SET last_used_at = now() WHERE id = $1")
        .bind(quiz_id)
        .execute(&state.app.db)
        .await?;

    let join_url = format!(
        "{}/join?pin={pin}",
        state.auth.public_url.trim_end_matches('/')
    );
    Ok((
        StatusCode::CREATED,
        Json(GameCreated {
            session_id: session.to_string(),
            pin,
            join_url,
        }),
    ))
}

/// `GET /games/pin/:pin`: anyone may check a PIN before choosing a
/// nickname. 20 checks a minute per IP keeps PIN guessing slow (SEC-17).
pub async fn check_pin(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(pin): Path<String>,
) -> ApiResult<Json<PinCheck>> {
    let who = client_ip(&headers)
        .map(|ip| hash_ip(&state.auth.app_secret, &ip))
        .unwrap_or_else(|| "local".into());
    ratelimit::hit(&state.app.redis, &format!("pin:{who}"), 20, 60).await?;

    let not_found = || AppError::not_found("PIN_NOT_FOUND");
    if pin.len() != 6 || !pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err(not_found());
    }
    let meta = state.live.meta(&pin).await?.ok_or_else(not_found)?;
    // Late joins are allowed until the podium.
    if matches!(meta.state, GameState::Podium | GameState::Ended) {
        return Err(not_found());
    }
    let title = state
        .live
        .quiz(&pin)
        .await?
        .map(|q| q.title)
        .unwrap_or_default();
    Ok(Json(PinCheck {
        pin,
        title,
        locked: meta.locked,
    }))
}
