# Build (from repo root): docker build -f docker/admin.Dockerfile -t drop-admin:latest .
# Reads ./app.toml from /app: mount it at runtime.

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked --bin admin \
    && cp target/release/admin /usr/local/bin/

FROM debian:bookworm-slim
# reqwest uses native-tls (openssl)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /app app
WORKDIR /app
COPY --from=builder /usr/local/bin/admin /usr/local/bin/
USER app
ENTRYPOINT ["admin"]
