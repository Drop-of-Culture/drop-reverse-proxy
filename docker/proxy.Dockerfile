# Build (from repo root): docker build -f docker/proxy.Dockerfile -t drop-reverse-proxy:latest .
# Reads ./app.toml from /app: mount it at runtime.

FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked --bin drop-reverse-proxy \
    && cp target/release/drop-reverse-proxy /usr/local/bin/

FROM debian:bookworm-slim
# reqwest uses native-tls (openssl)
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /app app
WORKDIR /app
COPY --from=builder /usr/local/bin/drop-reverse-proxy /usr/local/bin/
# owned by app so the data volume mounted here is writable (imports untar next to data/import)
RUN mkdir -p /app/data/import && chown -R app:app /app/data
USER app
ENTRYPOINT ["drop-reverse-proxy"]
