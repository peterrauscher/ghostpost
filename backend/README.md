# Ghostpost backend

Single-package Rust/Axum kernel: config, health, SQLx migrations, tenant-safe
schema, and a fenced Postgres work queue.

## Prerequisites

- Rust stable ≥ 1.80
- Docker Engine (Postgres 17)
- `sqlx-cli` for offline data: `cargo install sqlx-cli --no-default-features --features rustls,postgres`

## Local Postgres

```bash
docker compose -f docker-compose.postgres.yml up -d --wait
cp .env.example .env
cargo run --locked -- migrate
```

## Serve roles

```bash
cargo run --locked -- serve --role all    # API + worker (local default)
cargo run --locked -- serve --role api
cargo run --locked -- serve --role worker
```

Health:

- `GET /health/live` — process up
- `GET /health/ready` — DML pool + `SELECT 1` (503 Problem+JSON when not ready)

## Tests

```bash
cargo test --locked
cargo test --locked --test postgres_integration -- --ignored --test-threads=1
```

## Offline SQLx

```bash
cargo sqlx prepare -- --all-targets
```

CI builds with `SQLX_OFFLINE=true`.
