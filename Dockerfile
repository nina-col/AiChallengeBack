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
# build.rs incrusta catalog/room.json (cook usó un build.rs vacío y borró lo compilado de él, así
# que este build corre el de verdad, ya con el catálogo copiado). Sin el catálogo la API arrancaría
# revisando solo la forma de los cuartos: en producción es obligatorio.
RUN grep -q '[^[:space:]]' catalog/room.json \
    || { echo "falta catalog/room.json (lo genera docs/brand/draw_room.py)" >&2; exit 1; }
RUN cargo build --release --locked --bin spaceease-api

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/spaceease-api /usr/local/bin/spaceease-api
CMD ["spaceease-api"]
