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

    /// "Get ready" countdown before each question's options show, in ms.
    #[arg(long, env = "COUNTDOWN_MS", default_value_t = 3000)]
    pub countdown_ms: u64,

    /// Log as JSON lines (production) instead of human-readable text.
    #[arg(long, env = "LOG_JSON", default_value_t = false)]
    pub log_json: bool,

    /// Quizzes are deleted this many days after they were last edited or
    /// played (FR-QZ-18). The API reports the date as expiresAt.
    #[arg(long, env = "RETENTION_DAYS", default_value_t = 21)]
    pub retention_days: u32,

    /// New games are refused with 503 CAPACITY_FULL above this many running
    /// games (NFR-03). Set to the load-tested figure.
    #[arg(long, env = "MAX_ACTIVE_GAMES", default_value_t = 200)]
    pub max_active_games: u32,

    #[command(flatten)]
    pub auth: AuthSettings,
}

/// Sign-in and session settings (design sections 6 and 9).
#[derive(Debug, Clone, Args)]
pub struct AuthSettings {
    /// The site's public origin, e.g. https://quiz.example.edu.vn. Used to
    /// build the Google redirect URI: <PUBLIC_URL>/api/v1/auth/google/callback.
    #[arg(long, env = "PUBLIC_URL", default_value = "http://localhost:8080")]
    pub public_url: String,

    /// Google OAuth client. Without it, Google sign-in answers 503.
    #[arg(long, env = "GOOGLE_CLIENT_ID")]
    pub google_client_id: Option<String>,
    #[arg(long, env = "GOOGLE_CLIENT_SECRET", hide_env_values = true)]
    pub google_client_secret: Option<String>,

    /// Overridable for tests; the defaults are Google's.
    #[arg(
        long,
        env = "GOOGLE_AUTH_URL",
        default_value = "https://accounts.google.com/o/oauth2/v2/auth"
    )]
    pub google_auth_url: String,
    #[arg(
        long,
        env = "GOOGLE_TOKEN_URL",
        default_value = "https://oauth2.googleapis.com/token"
    )]
    pub google_token_url: String,
    #[arg(
        long,
        env = "GOOGLE_ISSUER",
        default_value = "https://accounts.google.com"
    )]
    pub google_issuer: String,

    /// Comma-separated email domains allowed to sign in, e.g. "uit.edu.vn".
    /// Empty allows any verified Google account.
    #[arg(long, env = "ALLOWED_EMAIL_DOMAINS", default_value = "")]
    pub allowed_email_domains: String,

    /// Secret for hashing IPs in the audit log and signing download links.
    /// At least 32 characters in production.
    #[arg(long, env = "APP_SECRET", hide_env_values = true, default_value = "")]
    pub app_secret: String,

    /// Send the session cookie only over HTTPS. Turn off for plain-HTTP
    /// local development.
    #[arg(long, env = "COOKIE_SECURE", default_value_t = true, action = clap::ArgAction::Set)]
    pub cookie_secure: bool,

    /// Login sessions expire after this many days without use (SEC-04).
    #[arg(long, env = "SESSION_DAYS", default_value_t = 7)]
    pub session_days: u64,
}

impl AuthSettings {
    pub fn redirect_uri(&self) -> String {
        format!(
            "{}/api/v1/auth/google/callback",
            self.public_url.trim_end_matches('/')
        )
    }

    pub fn allowed_domains(&self) -> Vec<String> {
        self.allowed_email_domains
            .split(',')
            .map(|d| d.trim().trim_start_matches('@').to_lowercase())
            .filter(|d| !d.is_empty())
            .collect()
    }

    #[cfg(test)]
    pub fn for_tests() -> Self {
        AuthSettings {
            public_url: "http://localhost:8080".into(),
            google_client_id: None,
            google_client_secret: None,
            google_auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            google_token_url: "https://oauth2.googleapis.com/token".into(),
            google_issuer: "https://accounts.google.com".into(),
            allowed_email_domains: String::new(),
            app_secret: "test-secret-test-secret-test-secret".into(),
            cookie_secure: false,
            session_days: 7,
        }
    }
}
