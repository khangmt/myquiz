//! The REST API under `/api/v1` (design section 6). Served by `myquiz api`.

pub mod audit;
pub mod auth;
pub mod error;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::config::AuthSettings;
use crate::state::AppState;

#[derive(Clone)]
pub struct ApiState {
    pub app: AppState,
    pub auth: Arc<AuthSettings>,
    /// Outbound HTTP, for Google's token endpoint.
    pub http: reqwest::Client,
}

impl ApiState {
    pub fn new(app: AppState, auth: AuthSettings) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;
        Ok(ApiState {
            app,
            auth: Arc::new(auth),
            http,
        })
    }
}

pub fn router(state: ApiState) -> Router {
    let v1 = Router::new()
        .route("/auth/google", get(auth::google_start))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me).patch(auth::update_me));
    Router::new()
        .nest("/api/v1", v1)
        // SEC-21: 1 MB unless a route says otherwise (zip import, later).
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
