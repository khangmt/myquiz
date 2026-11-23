//! The REST API under `/api/v1` (design section 6). Served by `myquiz api`.

pub mod audit;
pub mod auth;
pub mod error;
pub mod games;
pub mod quizzes;
pub mod ratelimit;

#[cfg(test)]
mod quiz_tests;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::config::{AuthSettings, Config};
use crate::live::LiveStore;
use crate::state::AppState;

#[derive(Clone)]
pub struct ApiState {
    pub app: AppState,
    pub auth: Arc<AuthSettings>,
    /// Outbound HTTP, for Google's token endpoint.
    pub http: reqwest::Client,
    pub live: LiveStore,
    pub retention_days: u32,
    pub max_active_games: u32,
}

impl ApiState {
    pub fn new(app: AppState, cfg: &Config) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(ApiState {
            live: LiveStore::new(app.redis.clone()),
            app,
            auth: Arc::new(cfg.auth.clone()),
            http,
            retention_days: cfg.retention_days,
            max_active_games: cfg.max_active_games,
        })
    }
}

pub fn router(state: ApiState) -> Router {
    let v1 = Router::new()
        .route("/auth/google", get(auth::google_start))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me).patch(auth::update_me))
        .route("/quizzes", get(quizzes::list).post(quizzes::create))
        .route(
            "/quizzes/{id}",
            get(quizzes::get_one)
                .put(quizzes::save)
                .delete(quizzes::delete),
        )
        .route("/quizzes/{id}/duplicate", post(quizzes::duplicate))
        .route("/quizzes/{id}/publish", post(quizzes::publish))
        .route("/games", post(games::create))
        .route("/games/pin/{pin}", get(games::check_pin));
    Router::new()
        .nest("/api/v1", v1)
        // SEC-21: 1 MB unless a route says otherwise (zip import, later).
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
