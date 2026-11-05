# Contexto de build: apps/api (directorio raíz del servicio en Railway; ver scripts/railway-configure.sh).
FROM rust:1-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Las dependencias van en su propia capa: solo se recompilan si cambia Cargo.toml o Cargo.lock.
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin spaceease-api

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/spaceease-api /usr/local/bin/spaceease-api
CMD ["spaceease-api"]
