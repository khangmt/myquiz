use axum::{extract::State, http::StatusCode, routing::get, Json, Router};
use fred::prelude::*;
use myquiz_protocol::Health;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

use crate::state::AppState;

/// Routes shared by both roles. The REST API and the WebSocket gateway add
/// their own routes on top of this in later PRs.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .layer(RequestBodyLimitLayer::new(1024 * 1024)) // SEC-21: 1 MB body limit
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Internal health check (NFR-05): 200 when PostgreSQL and Redis answer,
/// 503 otherwise. Caddy does not route it to the public.
async fn healthz(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let redis_ok = state.redis.ping::<String>(None).await.is_ok();
    let ok = db_ok && redis_ok;

    let body = Health {
        status: if ok { "ok" } else { "degraded" }.to_string(),
        role: state.role.as_str().to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let code = if ok {
        StatusCode::OK
    } else {
        tracing::warn!(db_ok, redis_ok, "health check failed");
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Runs against the real PostgreSQL and Redis from CI (or docker compose).
    /// Skipped when DATABASE_URL is not set so `cargo test` works anywhere.
    #[tokio::test]
    async fn healthz_reports_ok_with_live_dependencies() {
        let (Ok(database_url), Ok(redis_url)) =
            (std::env::var("DATABASE_URL"), std::env::var("REDIS_URL"))
        else {
            eprintln!("skipped: DATABASE_URL and REDIS_URL not set");
            return;
        };
        let cfg = crate::config::Config {
            bind_addr: "127.0.0.1:0".into(),
            database_url,
            redis_url,
            db_max_connections: 2,
            countdown_ms: 3000,
            auth: crate::config::AuthSettings::for_tests(),
            log_json: false,
        };
        let state = AppState::connect(crate::Role::Api, &cfg).await.unwrap();
        crate::MIGRATOR.run(&state.db).await.unwrap();

        let res = router(state)
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body = res.into_body().collect().await.unwrap().to_bytes();
        let health: Health = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "ok");
        assert_eq!(health.role, "api");
    }
}
