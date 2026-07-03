# Build context is the repository root (see docker-compose.yml).
# Two stages: cargo build, then a slim runtime image.

FROM rust:bookworm AS builder
WORKDIR /workspace

COPY api/Cargo.toml api/Cargo.lock api/
COPY migrations migrations/
COPY api/src api/src

WORKDIR /workspace/api
RUN cargo build --release --bin ledger-api

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /workspace/api/target/release/ledger-api /usr/local/bin/ledger-api

RUN mkdir -p /app/logs
ENV LOG_FILE_PATH=/app/logs/ledger-api.log

EXPOSE 4000
CMD ["ledger-api"]
