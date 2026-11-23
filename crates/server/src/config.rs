//! All configuration comes from environment variables (NFR-17).

use clap::Args;

#[derive(Debug, Clone, Args)]
pub struct Config {
    /// Address to listen on, e.g. 0.0.0.0:8080.
    #[arg(long, env = "BIND_ADDR", default_value = "0.0.0.0:8080")]
    pub bind_addr: String,

    /// PostgreSQL connection string.
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,

    /// Redis connection string, e.g. redis://:password@redis:6379.
    #[arg(long, env = "REDIS_URL")]
    pub redis_url: String,

    /// Maximum PostgreSQL connections per process.
    #[arg(long, env = "DB_MAX_CONNECTIONS", default_value_t = 10)]
    pub db_max_connections: u32,

    /// Log as JSON lines (production) instead of human-readable text.
    #[arg(long, env = "LOG_JSON", default_value_t = false)]
    pub log_json: bool,
}
