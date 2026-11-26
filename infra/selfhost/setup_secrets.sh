#!/bin/bash
# Run once on the server: builds app.env from fresh secrets plus the Google
# client from infra.env (passed on stdin). Prints nothing secret.
set -euo pipefail
umask 077
D=/local/khangmt/myquiz
test -e $D/app.env && { echo "app.env exists, leaving it"; exit 0; }
src=$(cat)
get() { printf '%s\n' "$src" | grep -E "^$1=" | head -1 | cut -d= -f2-; }
PGPW=$(openssl rand -hex 24); RDPW=$(openssl rand -hex 24); SEC=$(openssl rand -hex 32)
printf '%s' "$PGPW" > $D/pg.pw
cat > $D/app.env <<E2
DATABASE_URL=postgres://myquiz:$PGPW@127.0.0.1:15432/myquiz
REDIS_URL=redis://:$RDPW@127.0.0.1:16379
REDIS_PASSWORD=$RDPW
LOG_JSON=true
RUST_LOG=info,sqlx=warn
PUBLIC_URL=https://myquiz.dev
JOIN_URL=https://myquiz.dev
APP_SECRET=$SEC
GOOGLE_CLIENT_ID=$(get GOOGLE_CLIENT_ID)
GOOGLE_CLIENT_SECRET=$(get GOOGLE_CLIENT_SECRET)
ALLOWED_EMAIL_DOMAINS=$(get ALLOWED_EMAIL_DOMAINS)
COOKIE_SECURE=true
RETENTION_DAYS=21
MAX_ACTIVE_GAMES=30
DB_MAX_CONNECTIONS=8
CLEANUP_ENABLED=true
CLEANUP_HOUR=2
MEDIA_DIR=$D/data/media
E2
echo "app.env written ($(wc -l < $D/app.env) lines); google client id set: $([ -n "$(get GOOGLE_CLIENT_ID)" ] && echo yes || echo no)"
