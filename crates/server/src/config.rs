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
    /// games (NFR-03). 100 at launch: docs/load-test.md.
    #[arg(long, env = "MAX_ACTIVE_GAMES", default_value_t = 100)]
    pub max_active_games: u32,

    /// Where uploaded images are stored. Caddy serves this folder at
    /// /media/ (SEC-11).
    #[arg(long, env = "MEDIA_DIR", default_value = "/data/media")]
    pub media_dir: String,

    #[command(flatten)]
    pub auth: AuthSettings,

    #[command(flatten)]
    pub cleanup: CleanupSettings,
}

/// The daily clean-up in `myquiz api` (design section 8, FR-QZ-18 to 20).
#[derive(Debug, Clone, Args)]
pub struct CleanupSettings {
    /// Run the daily clean-up: reminder emails, deleting quizzes past their
    /// date, purging deleted quizzes after 30 days and reports after 12
    /// months.
    #[arg(long, env = "CLEANUP_ENABLED", default_value_t = true, action = clap::ArgAction::Set)]
    pub cleanup_enabled: bool,

    /// Hour of the day (Vietnam time, UTC+7) the clean-up runs.
    #[arg(long, env = "CLEANUP_HOUR", default_value_t = 2)]
    pub cleanup_hour: u8,

    /// Outgoing mail server for reminder emails, e.g.
    /// smtps://user:password@smtp.gmail.com:465. Without it, no emails are
    /// sent (the quiz-list and sign-in warnings still show).
    #[arg(long, env = "SMTP_URL")]
    pub smtp_url: Option<String>,

    /// Sender of reminder emails.
    #[arg(long, env = "MAIL_FROM", default_value = "myquiz <no-reply@localhost>")]
    pub mail_from: String,
}

impl CleanupSettings {
    #[cfg(test)]
    pub fn for_tests() -> Self {
        CleanupSettings {
            cleanup_enabled: false,
            cleanup_hour: 2,
            smtp_url: None,
            mail_from: "myquiz <no-reply@localhost>".into(),
        }
    }
}

/// Sign-in and session settings (design sections 6 and 9).
#[derive(Debug, Clone, Args)]
pub struct AuthSettings {
    /// The site's public origin, e.g. https://quiz.example.edu.vn. Used to
    /// build the Google redirect URI: <PUBLIC_URL>/api/v1/auth/google/callback.
    #[arg(long, env = "PUBLIC_URL", default_value = "http://localhost:8080")]
    pub public_url: String,

    /// Where players open the game, shown big on the host screen. Defaults
    /// to PUBLIC_URL. Set it to a LAN address (http://192.168.1.20:8080)
    /// when phones on the same Wi-Fi should join a laptop: players never
    /// sign in, so this address needs no Google redirect URI.
    #[arg(long, env = "JOIN_URL")]
    pub join_url: Option<String>,

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

    /// The origin players use, without a trailing slash.
    pub fn join_base(&self) -> String {
        self.join_url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .unwrap_or(&self.public_url)
            .trim_end_matches('/')
            .to_string()
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
            join_url: None,
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

#[cfg(test)]
mod tests {
    use super::AuthSettings;

    #[test]
    fn join_url_defaults_to_public_url() {
        let mut a = AuthSettings::for_tests();
        a.public_url = "https://quiz.example.edu.vn/".into();
        assert_eq!(a.join_base(), "https://quiz.example.edu.vn");
        a.join_url = Some("  ".into());
        assert_eq!(a.join_base(), "https://quiz.example.edu.vn");
        a.join_url = Some("http://192.168.1.20:8080/".into());
        assert_eq!(a.join_base(), "http://192.168.1.20:8080");
        // Sign-in keeps using PUBLIC_URL.
        assert!(a.redirect_uri().starts_with("https://quiz.example.edu.vn/"));
    }
}
