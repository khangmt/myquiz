# syntax=docker/dockerfile:1
# Caddy serving the built React app, plus the reverse proxy rules.
FROM node:26-bookworm-slim AS web
RUN corepack enable
WORKDIR /web
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web ./
RUN pnpm build

FROM caddy:2
COPY infra/Caddyfile /etc/caddy/Caddyfile
COPY --from=web /web/dist /srv
