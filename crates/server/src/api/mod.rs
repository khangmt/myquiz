//! The REST API under `/api/v1` (design section 6). Served by `myquiz api`.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod error;
pub mod games;
pub mod media;
pub mod quizzes;
pub mod ratelimit;
pub mod reports;
pub mod signed;
pub mod zip;

#[cfg(test)]
mod quiz_tests;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use tokio::sync::Semaphore;

use axum::http::{header, HeaderValue};
use axum::routing::{get, post};
use axum::Router;
use tower_http::set_header::SetResponseHeaderLayer;
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
    pub media_dir: Arc<PathBuf>,
    /// Image decoding is CPU heavy: at most 2 at once per process, so a
    /// burst of uploads cannot starve the rest of the API.
    pub image_permits: Arc<Semaphore>,
    /// Zip imports at once per process; above it, 503 IMPORT_BUSY.
    pub import_permits: Arc<Semaphore>,
}

impl ApiState {
    pub fn new(app: AppState, cfg: &Config) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.media_dir)?;
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
            media_dir: Arc::new(PathBuf::from(&cfg.media_dir)),
            image_permits: Arc::new(Semaphore::new(2)),
            import_permits: Arc::new(Semaphore::new(2)),
        })
    }
}

/// `GET /health`: the public uptime check. Unlike the internal `/healthz`
/// it says nothing about versions or roles: just whether the API reaches
/// PostgreSQL and Redis.
async fn health(
    axum::extract::State(state): axum::extract::State<ApiState>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    use fred::prelude::*;
    let db = sqlx::query("SELECT 1").execute(&state.app.db).await.is_ok();
    let redis = state.app.redis.ping::<String>(None).await.is_ok();
    if db && redis {
        (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "status": "ok" })),
        )
    } else {
        (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({ "status": "down" })),
        )
    }
}

/// `GET /config`: public settings for the web app.
async fn client_config(
    axum::extract::State(state): axum::extract::State<ApiState>,
) -> axum::Json<myquiz_protocol::quiz::ClientConfig> {
    axum::Json(myquiz_protocol::quiz::ClientConfig {
        join_url: state.auth.join_base(),
    })
}

/// SEC-26, defence in depth: Caddy sets the page headers, but API answers
/// carry their own so they stay safe if something else ever serves them.
/// API data is per user, so nothing is cached. A handler that sets its own
/// header (a download) keeps it.
pub fn router(state: ApiState) -> Router {
    let v1 = Router::new()
        .route("/auth/google", get(auth::google_start))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/auth/logout", post(auth::logout))
        .route("/me", get(auth::me).patch(auth::update_me))
        .route("/config", get(client_config))
        .route("/health", get(health))
        .route("/quizzes", get(quizzes::list).post(quizzes::create))
        .route("/quizzes/expiring", get(quizzes::expiring))
        .route(
            "/quizzes/{id}",
            get(quizzes::get_one)
                .put(quizzes::save)
                .delete(quizzes::delete),
        )
        .route("/quizzes/{id}/duplicate", post(quizzes::duplicate))
        .route("/quizzes/{id}/publish", post(quizzes::publish))
        .route("/games", post(games::create))
        .route("/games/pin/{pin}", get(games::check_pin))
        .route("/quizzes/{id}/export", post(zip::export))
        .route("/exports/{token}", get(zip::download))
        .route("/admin/users", get(admin::users))
        .route("/admin/users/{id}/suspend", post(admin::suspend))
        .route("/admin/users/{id}/reactivate", post(admin::reactivate))
        .route("/admin/audit", get(admin::audit_log))
        .route("/sessions", get(reports::list))
        .route("/sessions/{id}", get(reports::get).delete(reports::delete))
        .route("/sessions/{id}/exports", post(reports::export))
        .route("/quiz-template.zip", get(zip::template))
        // SEC-21: 1 MB for everything but uploads.
        .layer(RequestBodyLimitLayer::new(1024 * 1024));
    let uploads = Router::new()
        .route("/media", post(media::upload))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(media::MAX_BODY));
    let imports = Router::new()
        .route("/quizzes/import", post(zip::import))
        .layer(DefaultBodyLimit::disable())
        .layer(RequestBodyLimitLayer::new(zip::MAX_IMPORT_BODY + 64 * 1024));
    Router::new()
        .nest("/api/v1", v1.merge(uploads).merge(imports))
        // SEC-26, defence in depth: Caddy sets the page headers, but API
        // answers carry their own too. API data is per user, so nothing is
        // cached. A handler that sets its own header (a download) keeps it.
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
