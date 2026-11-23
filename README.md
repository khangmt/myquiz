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

### Playing on phones on the same Wi-Fi

The host screen shows the address players should open. It comes from `JOIN_URL` in
`infra/.env`, or from `PUBLIC_URL` when `JOIN_URL` is empty. With `PUBLIC_URL=http://localhost:8080`,
phones are told to open `localhost`, which points at the phone itself, so set `JOIN_URL` to the laptop's
address on the Wi-Fi:

1. Find the laptop's address: `ipconfig getifaddr en0` on a Mac, `hostname -I` on Linux,
   `ipconfig` on Windows. Say it is `192.168.1.20`.
2. In `infra/.env` set `JOIN_URL=http://192.168.1.20:8080` (with `HTTP_PORT=8080`). Keep
   `PUBLIC_URL=http://localhost:8080` and `COOKIE_SECURE=false` for plain HTTP.
3. `docker compose up -d` to apply it, sign in on the laptop at `http://localhost:8080`, and host.

Only the host signs in, so only `PUBLIC_URL` needs a redirect URI in Google. Google does not accept a
private IP such as `192.168.1.20` as a redirect URI, which is why the two addresses are separate. Phones
must be on the same Wi-Fi, and the laptop's firewall must allow incoming connections on port 8080.

### If Docker cannot resolve deb.debian.org during a build

Some networks (often on macOS with Docker Desktop) give containers a DNS server that does not answer,
so `apt-get` fails with "Temporary failure resolving 'deb.debian.org'". Give Docker public DNS servers:
in Docker Desktop open Settings, then Docker Engine, add `"dns": ["8.8.8.8", "1.1.1.1"]` to the JSON,
and click Apply & restart. On Linux put the same line in `/etc/docker/daemon.json` and restart Docker.

## Admins

Admins can search accounts, suspend or reactivate them, and read the audit
log at `/admin`. A suspended account is signed out, cannot sign in again, and
its running games end within about 10 seconds.

There is no admin by default. After the person has signed in once, grant
the role from the server:

```sh
docker compose exec api myquiz admin someone@uit.edu.vn            # grant
docker compose exec api myquiz admin someone@uit.edu.vn --revoke   # back to a normal user
```

Both are written to the audit log.

## Security checks

The `Security` workflow runs on every pull request, on main and every week:

- **Semgrep** (SAST): Rust, TypeScript, React, secrets and Dockerfile rules.
  Findings rated ERROR fail the build.
- **cargo deny**: licences (permissive only), banned crates and crate
  sources. Known advisories are checked by `cargo audit` in CI.
- **Trivy**: both images. A critical issue that has a fix fails the build;
  high issues and the PostgreSQL and Redis images are reported.
- **OWASP ZAP baseline**: starts the whole stack with Docker Compose and
  scans it. Rules and the reason for each ignore are in `.zap/rules.tsv`.
  The HTML report is attached to the run.

Dependabot opens weekly update PRs for Cargo, npm, Docker and Actions.
