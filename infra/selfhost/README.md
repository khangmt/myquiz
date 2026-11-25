# Self-hosted deploy (no Docker, no sudo)

How myquiz runs on the faculty server (cnpm-01, Ubuntu 24.04) under one
unprivileged account, with no Docker access. Public traffic arrives through
a Cloudflare tunnel (`cloudflared`, run as the same user), so the server
needs no open port.

Everything lives in `/local/khangmt/myquiz`:

| Path | What |
| --- | --- |
| `current` → `releases/<tag>` | the running app: `myquiz` binary and `web/` |
| `releases/` | the three newest releases, for rollback |
| `bin/` | `caddy`, `myquizctl`, `myquiz-deploy` |
| `env/` | PostgreSQL 16 and Redis 7 from conda-forge (micromamba) |
| `data/` | database, Redis AOF, uploaded media, Caddy state |
| `app.env` | secrets (mode 600), written once by `setup_secrets.sh` |
| `tunnel.token` | Cloudflare tunnel token (mode 600) |
| `deploy/deploy.log` | one line per deploy decision |

`myquizctl start|stop|status|logs <unit>` runs each piece as a user systemd
service in `myquiz.slice`, capped together at 1.7 GB RAM and 1.8 CPUs, with
no GPU (`CUDA_VISIBLE_DEVICES` empty). Everything listens on 127.0.0.1 only.

## Continuous delivery

The server is only reachable through the UIT VPN, so GitHub cannot push to
it. Instead:

1. `.github/workflows/release.yml` runs after CI passes on `main`. It builds
   the binary (in Debian bookworm, for an old enough glibc) and the web app,
   and publishes them as a GitHub release `deploy-<date>-<time>-<sha>` with a
   `SHA256SUMS` file.
2. `myquiz-deploy.timer` runs `myquiz-deploy` every 5 minutes on the server.
   It takes the latest release, waits while any game with connected players
   is running, verifies the checksum, unpacks it under `releases/`, points
   `current` at it and restarts `mq-api` and `mq-game`. If `/healthz` on
   either server or the site through Caddy is not healthy within a minute,
   it switches back and marks the release bad in `deploy/bad`.

Database migrations run when the api starts and are not undone by a
rollback, so keep migrations backwards compatible.

Install or update the scripts and the timer:

```sh
scp infra/selfhost/{myquiz-deploy,myquizctl} gpu:/local/khangmt/myquiz/bin/
scp infra/selfhost/systemd/myquiz-deploy.{service,timer} gpu:.config/systemd/user/
ssh gpu 'systemctl --user daemon-reload && systemctl --user enable --now myquiz-deploy.timer'
```

Deploy now instead of waiting: `ssh gpu /local/khangmt/myquiz/bin/myquiz-deploy`.
Roll back by hand: point `current` at an older folder in `releases/`, then
`systemctl --user restart mq-api mq-game`.

The user's systemd manager only runs while the account has a session, so a
tmux session (`myquiz-anchor`) is kept open. After a reboot, run
`myquizctl start` and `systemctl --user start myquiz-deploy.timer`, or ask
the admin to enable linger for the account.
