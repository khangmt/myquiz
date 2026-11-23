# myquiz

A Kahoot-like quiz platform: create quizzes, host live games with a PIN, and download reports afterwards.

- Backend: Rust (Axum on tokio), PostgreSQL 16, Redis 7, plain WebSockets
- Frontend: React + TypeScript (Vite)
- Deployment: Docker Compose, memory-capped at about 9 GB in total

The work follows the requirements spec (v1.0) and the design document. Requirement IDs such as
`FR-GM-06` or `SEC-15` in code and PRs refer to the spec.

## Repository layout

| Path | What it holds |
| --- | --- |
| `crates/server` | The `myquiz` binary: `myquiz api` (REST) and `myquiz game` (WebSockets) |
| `crates/engine` | Game state machine, answer matching and scoring as pure functions |
| `crates/bundle` | The quiz zip format: safe import and export of a quiz with its images |
| `crates/protocol` | REST and WebSocket message types, exported to TypeScript with ts-rs |
| `migrations/` | PostgreSQL schema (sqlx migrations, applied by `myquiz api` at start) |
| `web/` | React + TypeScript app (Vite, pnpm, Tailwind, i18next with `vi` and `en`) |
| `infra/` | Dockerfiles, Compose files, Caddyfile |

## Running locally

You need Rust (stable), Node 22 with pnpm, and Docker.

```sh
# 1. PostgreSQL and Redis on localhost
docker compose -f infra/docker-compose.dev.yml up -d

# 2. The servers (two terminals). The api applies migrations at start.
cp .env.example .env && set -a && . ./.env && set +a
cargo run -p myquiz -- api
BIND_ADDR=127.0.0.1:8081 cargo run -p myquiz -- game

# 3. The web app on http://localhost:5173 (proxies /api and /ws to the servers)
cd web && pnpm install && pnpm dev
```

Tests: `cargo test --workspace` (database tests run when `DATABASE_URL` and `REDIS_URL` are set) and
`cd web && pnpm test`. Running `cargo test -p myquiz-protocol` regenerates the TypeScript types in
`web/src/generated/`; commit them with the Rust change.

## Running the whole stack

```sh
cd infra
cp .env.example .env   # set POSTGRES_PASSWORD and REDIS_PASSWORD
docker compose up -d --build
```

Memory limits in `infra/docker-compose.yml` add up to about 9 GB (game 2x1 GB, api 2x0.5 GB,
postgres 3 GB, redis 2 GB, caddy 0.5 GB, plus 0.5 GB reserved for backup and uptime later), under
the 16 GB cap on the production server. Only Caddy publishes ports; PostgreSQL and Redis sit on an
internal network.
