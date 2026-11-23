//! One binary, two roles (design section 2):
//!
//! - `myquiz api`  serves REST, uploads and exports, and runs migrations at start.
//! - `myquiz game` serves live games over WebSockets.
//! - `myquiz migrate` applies database migrations and exits.
//! - `myquiz healthcheck` is the Docker health check (no curl in the image).

mod config;
// Used by the WebSocket gateway, which arrives in the next PR.
#[allow(dead_code, unused_imports)]
mod live;
mod routes;
mod state;

use clap::{Parser, Subcommand};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use config::Config;
use state::AppState;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Parser)]
#[command(name = "myquiz", version, about = "Live quiz platform server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the REST API.
    Api(Config),
    /// Serve live games over WebSockets.
    Game(Config),
    /// Apply database migrations and exit.
    Migrate {
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
    /// Exit 0 if the local server's /healthz answers 200, else 1.
    Healthcheck {
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Api,
    Game,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Api => "api",
            Role::Game => "game",
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Api(cfg) => serve(Role::Api, cfg).await,
        Command::Game(cfg) => serve(Role::Game, cfg).await,
        Command::Migrate { database_url } => {
            init_tracing(false);
            let pool = sqlx::PgPool::connect(&database_url).await?;
            MIGRATOR.run(&pool).await?;
            tracing::info!("migrations applied");
            Ok(())
        }
        Command::Healthcheck { addr } => {
            if healthcheck(&addr).await {
                Ok(())
            } else {
                std::process::exit(1)
            }
        }
    }
}

/// A minimal HTTP/1.0 GET so the runtime image needs no curl.
async fn healthcheck(addr: &str) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let attempt = async {
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        stream
            .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .await?;
        let mut head = [0u8; 12];
        stream.read_exact(&mut head).await?;
        Ok::<bool, std::io::Error>(
            head.starts_with(b"HTTP/1.0 200") || head.starts_with(b"HTTP/1.1 200"),
        )
    };
    matches!(
        tokio::time::timeout(std::time::Duration::from_secs(3), attempt).await,
        Ok(Ok(true))
    )
}

async fn serve(role: Role, cfg: Config) -> anyhow::Result<()> {
    init_tracing(cfg.log_json);
    let state = AppState::connect(role, &cfg).await?;

    if role == Role::Api {
        // sqlx takes a PostgreSQL advisory lock, so two api replicas starting
        // together do not race.
        MIGRATOR.run(&state.db).await?;
    }

    let app = routes::router(state);
    let listener = tokio::net::TcpListener::bind(&cfg.bind_addr).await?;
    tracing::info!(role = role.as_str(), addr = %cfg.bind_addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into());
    let registry = tracing_subscriber::registry().with(filter);
    if json {
        registry.with(fmt::layer().json()).init();
    } else {
        registry.with(fmt::layer()).init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
