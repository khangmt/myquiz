# syntax=docker/dockerfile:1
# Builds the single `myquiz` binary used by both the api and game services.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p myquiz && cp target/release/myquiz /myquiz

# debian slim rather than distroless: libwebp (image uploads, later PR) needs libc.
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 myquiz
COPY --from=build /myquiz /usr/local/bin/myquiz
USER myquiz
EXPOSE 8080
ENTRYPOINT ["myquiz"]
