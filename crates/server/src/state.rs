use fred::prelude::*;
use sqlx::PgPool;

use crate::Role;

/// Shared handles every request needs. Cheap to clone.
#[derive(Clone)]
pub struct AppState {
    pub role: Role,
    pub db: PgPool,
    pub redis: Pool,
}

impl AppState {
    pub async fn connect(role: Role, cfg: &crate::config::Config) -> anyhow::Result<Self> {
        let db = sqlx::postgres::PgPoolOptions::new()
            .max_connections(cfg.db_max_connections)
            .connect(&cfg.database_url)
            .await?;

        let redis_cfg = fred::types::config::Config::from_url(&cfg.redis_url)?;
        let redis = Builder::from_config(redis_cfg).build_pool(4)?;
        redis.init().await?;

        Ok(Self { role, db, redis })
    }
}
