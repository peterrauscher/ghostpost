# Multi-stage build for ghostpost-backend.
# Digests recorded at Plan 008 implementation (multi-arch; do not hard-code --platform).

# rust:1.88-bookworm (stable bookworm; matches backend MSRV floor via rust-toolchain)
# Digest: sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0
FROM rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS builder-base

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

FROM builder-base AS builder

# Full context after .dockerignore: src/, migrations/, Cargo.*, .sqlx/, rust-toolchain,
# and prompts/ (Plan 005 compile-time embed). No secrets as build args.
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release --bin ghostpost-backend \
    && cp /build/target/release/ghostpost-backend /build/ghostpost-backend

# debian:bookworm-slim multi-arch
# Digest: sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl dumb-init \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 ghostpost \
    && useradd --system --uid 10001 --gid 10001 --home-dir /nonexistent --shell /usr/sbin/nologin ghostpost

COPY --from=builder --chown=root:root --chmod=0555 /build/ghostpost-backend /usr/local/bin/ghostpost-backend

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

USER 10001:10001

ENTRYPOINT ["/usr/bin/dumb-init", "--", "/usr/local/bin/ghostpost-backend"]
CMD ["serve", "--role", "all"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health/live || exit 1
