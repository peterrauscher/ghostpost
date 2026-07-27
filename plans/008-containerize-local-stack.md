# Plan 008: Containerize local stack with Postgres, MinIO, migrator, and combined backend

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3625ed1..HEAD -- backend/ docker-compose.yml docker-compose*.yml Dockerfile backend/Dockerfile backend/.dockerignore .dockerignore plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md plans/004-archive-upload-ingestion.md app/package.json app/app.json app/README.md app/src/services/api/`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.
>
> **Canonical inputs**: Honor contracts from Plans 002, 003, and 004; use review
> artifacts DesignIntegrator, SecurityReview, and ProductFlowReview
> (`'/Users/peter/.omp/agent/sessions/-Code-ghostpost/2026-07-23T07-10-39-920Z_019f8dcf-e52f-7000-9275-dcdd40349fb1/'`). Do not invent parallel blob/auth
> env names, unlock routes, or local durable storage paths. SST/IaC is Plan 009
> only — this plan is Compose + Dockerfile packaging.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: plans/002-backend-kernel-schema.md, plans/003-workos-auth-profiles-onboarding.md, plans/004-archive-upload-ingestion.md
- **Category**: migration
- **Planned at**: commit `3625ed1`, 2026-07-23

## Why this matters

At commit `3625ed1` the repository is Expo-only: there is no `backend/`, no `Dockerfile`, and no Compose file. Archive ingestion requires a durable S3-compatible object store shared by API and worker processes, PostgreSQL 17 for schema and the work queue, and a one-shot migrator that runs before the combined `serve --role all` process. Without a reproducible local topology, developers cannot exercise the same presigned-upload, HEAD-verify, immutable version pin, worker-delete, and readiness contracts that production will run. This plan lands the multi-stage ARM64-capable image, Compose services (Postgres 17, MinIO, migrator, backend), and an optional host-native Cargo path against the same dependencies so Plans 003–004 durable-upload flows can be smoke-tested on a laptop. Plan 004 owns the BlobStore/S3 adapter contract; this plan only supplies the local durable MinIO service and wires finalized backend/auth/blob env names from Plans 002–004.

## Current state

Facts at `3625ed1` (and after Plans 002/003/004 have landed — those plans create the backend sources and env contracts this plan packages):

- Repository root contains only `app/`, `design/`, and `.cursor/`. No `backend/`, `docker-compose.yml`, `Dockerfile`, `sst.config.ts`, or root `package.json`.
- `app/package.json` is npm-based (`package-lock.json` present). Scripts:
  - `start` → `expo start`
  - `export:web` → `expo export --platform web`
  - `lint` → `expo lint`
  - No typecheck/deploy/docker scripts.
- `app/app.json` sets `expo.web.output: "static"` (export lands in `app/dist/`).
- `app/README.md` documents mock-by-default API; `EXPO_PUBLIC_API_URL` switches to HTTP.
- `app/src/services/api/index.ts` selects the adapter once at module init:

```ts
// app/src/services/api/index.ts
export function createApi(): GhostpostApi {
  if (process.env.EXPO_PUBLIC_API_URL) {
    return httpApi;
  }
  return mockApi;
}
export const api = createApi();
```

- `app/src/services/api/http.ts` reads `EXPO_PUBLIC_API_URL`, strips one trailing slash, and throws if unset when a request is made. It currently has no auth transport (Plan 007 supplies credentials).
- **Plan 002** is expected to have created (do not re-invent):
  - `backend/Cargo.toml` + `backend/Cargo.lock`
  - Binary name **`ghostpost-backend`**
  - CLI: `ghostpost-backend serve --role api|worker|all` and `ghostpost-backend migrate`
  - `GET /health/live` (process-only, 200) and `GET /health/ready` (DB/schema + role init; 200 or safe 503)
  - Embedded SQLx migrations via `sqlx::migrate!` under `backend/migrations/`
  - `BIND_ADDR` defaulting to `0.0.0.0:8080`
  - SIGTERM/SIGINT graceful shutdown that stops new accepts/claims and drains within 30s
- **Plan 003** is expected to have created auth env/runtime contracts Compose must wire (do not re-implement auth):
  - WorkOS + opaque Ghostpost sessions; `GET /v1/auth/authorize?client=native|web` returns JSON (`authorizationUrl`, `state`) with server-selected redirect — **no** client-supplied `redirect_uri`
  - One-time **exchange secret** (distinct from PKCE `code_verifier` and OAuth `state`): web receives it via HttpOnly pre-auth cookie (`gp_auth_init`); native receives it in the authorize JSON body; `POST /v1/auth/exchange` consumes it with WorkOS `code` + `state`
  - Session-bound rotating web CSRF token bootstrap
  - Env names such as `WORKOS_API_KEY`, `WORKOS_CLIENT_ID`, `WORKOS_COOKIE_PASSWORD` / session secrets, exact redirect/origin allowlists
- **Plan 004** is expected to have created:
  - `BlobStore` port + S3-compatible adapter under `backend/src/` (ingest owns parsers; storage port lives where Plan 004 placed it — do not create a parallel store)
  - Env: `ARCHIVE_BUCKET`, `ARCHIVE_S3_ENDPOINT` (optional for AWS default), `ARCHIVE_S3_REGION`, `ARCHIVE_S3_ACCESS_KEY_ID`, `ARCHIVE_S3_SECRET_ACCESS_KEY`, `ARCHIVE_S3_FORCE_PATH_STYLE` (true for MinIO)
  - Signed multipart POST policy, HEAD, ranged GET, delete-by-key+**version**
  - **Completion pins an immutable object version ID** after HEAD so a still-valid signed policy cannot replace worker input
  - No backend-mounted archive volume; scratch is ephemeral only
- Product APIs / `free_beta` (Plan 006) and scan/DeepSeek (Plan 005) are **not** hard packaging prerequisites for the image/Compose skeleton. Do not stub billing unlock routes or require scan env for this plan's focused durable-upload smoke.

Canonical design constraints (from DesignIntegrator + SecurityReview + ProductFlowReview — honor these):

- Local topology: Docker Compose with PostgreSQL 17 named volume, S3-compatible private object service with named volume/lifecycle, one-shot migration, optional combined backend. Host-native Cargo may use the same Compose dependencies.
- Direct presigned archive upload is canonical. **Never** proxy archive bytes through the API body as the durable handoff; **never** use container scratch or a backend-mounted volume as durable queue state.
- API/worker containers **never** mount an archive volume.
- Object keys are UUID-derived; **versioning enabled**; completion pins immutable version ID; workers and purge jobs address that version.
- Lifecycle backstop ≤ 24 hours for raw objects including noncurrent versions (MinIO best-effort locally; AWS authoritative in Plan 009).
- No secrets as Docker build args. No `EXPO_PUBLIC_*` secrets.
- Prefer SQLx `runtime-tokio-rustls` so OpenSSL is not a runtime dependency.
- Non-root runtime UID/GID `10001:10001`.
- `dumb-init` as PID 1 for correct signal forwarding.
- Auth exchange must remain client-bound; Compose only supplies env — do not weaken exchange/CSRF for local convenience.
- `free_beta` is server-issued when Plan 006 is present; Compose must not introduce unlock/grant/demo routes or priced CTA backends.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Drift check | `git diff --stat 3625ed1..HEAD -- backend/ docker-compose.yml docker-compose*.yml Dockerfile backend/Dockerfile backend/.dockerignore .dockerignore plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md plans/004-archive-upload-ingestion.md app/package.json app/app.json app/README.md app/src/services/api/` | Review only; empty or expected prior-plan diffs |
| Backend present | `test -f backend/Cargo.toml && test -f backend/Cargo.lock && test -d backend/migrations && test -f backend/src/main.rs` | exit 0 |
| Compose validate | `docker compose config` | exit 0; prints merged config |
| Build runtime image | `docker build --target runtime -t ghostpost-backend:local ./backend` | exit 0 |
| ARM64 image | `docker buildx build --platform linux/arm64 --target runtime -t ghostpost-backend:arm64 --load ./backend` | exit 0 |
| Inspect image | `docker image inspect ghostpost-backend:local --format '{{.Architecture}} {{.Config.User}} {{json .Config.Entrypoint}} {{json .Config.Cmd}}'` | non-root user, dumb-init entrypoint, default serve cmd |
| Compose up deps | `docker compose up -d --wait postgres minio minio-init` | minio-init completed; postgres healthy |
| Migrate | `docker compose run --rm migrate` | exit 0 |
| Backend ready | `docker compose up -d --wait --no-deps backend && curl -fsS http://127.0.0.1:8080/health/live && curl -fsS http://127.0.0.1:8080/health/ready` | both curl exit 0 |
| MinIO versioning | `docker compose run --rm --entrypoint /bin/sh minio-init -c 'mc version info local/ghostpost-archives'` | reports versioning enabled |
| Host migrate | `DATABASE_URL='postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost' cargo run --locked --manifest-path backend/Cargo.toml -- migrate` | exit 0 |
| Durable upload smoke | `bash scripts/local-upload-smoke.sh` | exit 0 |
| Expo web against API | `EXPO_PUBLIC_API_URL='http://localhost:8080' npm --prefix app start -- --web` | Metro starts; adapter is HTTP; local web callback remains same-site |

Do **not** run project-wide lint/typecheck/test suites as gates for this plan. Focused commands above only.

## Suggested executor toolkit

- Docker Engine + Buildx with multi-platform support.
- Docker Compose v2 (`docker compose`).
- Optional: `cargo` matching the backend MSRV from Plan 002 for the host-native path.
- npm (not pnpm/yarn) under `app/` because `app/package-lock.json` exists.

## Scope

**In scope** (the only files you should create or modify):

- `backend/Dockerfile` (create)
- `backend/.dockerignore` (create)
- `docker-compose.yml` (create at repo root)
- `docker/minio-init.sh` (create; one-shot bucket bootstrap for local MinIO)
- `backend/.env.example` (create; host-native env names and local non-secret defaults)
- `.env.example` (create at repo root; Compose WorkOS secret **names only** — values live in gitignored `.env`)
- `scripts/local-up.sh` (create; optional thin wrapper documenting the ordered start — no secrets)
- `scripts/local-upload-smoke.sh` (create; focused durable-upload E2E against Compose)

**Out of scope** (do NOT touch):

- `sst.config.ts`, SST/IaC, AWS resources, ECR, ALB — Plan 009.
- Backend application source under `backend/src/**` except if a Plan 002/003/004 contract is missing a documented env name required below; if missing, STOP and report rather than inventing a parallel config system.
- `app/**` source, mock removal, auth transport — Plan 007.
- Production Postgres vendor selection, DeepSeek legal approval — Plan 009 gates.
- Formatters, linters, `npx tsc`, full test suites.
- Committing real credentials, MinIO root passwords beyond the documented local-only defaults, or production URLs.

## Git workflow

- Branch: `advisor/008-containerize-local-stack` (repo history uses short imperative subjects; match that style).
- Commit per logical unit, e.g.:
  - `Add multi-stage backend Dockerfile and dockerignore`
  - `Add Compose Postgres MinIO migrator and backend`
  - `Add local smoke scripts and env example`
- Example existing subject style: `Add pricing paywall and expand platform support.`
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 0: Drift check

```bash
git rev-parse --short HEAD   # note SHA
git diff --stat 3625ed1..HEAD -- backend/ docker-compose.yml docker-compose*.yml Dockerfile backend/Dockerfile backend/.dockerignore .dockerignore plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md plans/004-archive-upload-ingestion.md app/package.json app/app.json app/README.md app/src/services/api/
```

**Verify**: If the diff is non-empty, re-read changed files and compare to the "Current state" excerpts. Material contract changes → STOP.

### Step 1: Confirm prerequisite backend contracts exist

Verify Plans **002/003/004** artifacts before packaging (auth env names from 003; blob/version pin from 004):

```bash
test -f backend/Cargo.toml
test -f backend/Cargo.lock
test -f backend/src/main.rs
test -d backend/migrations
rg -n "health/live|health/ready|serve|--role|migrate" backend/src
rg -n "ARCHIVE_BUCKET|BlobStore|presign|version" backend/src
rg -n "WORKOS_|CORS_ALLOWED" backend/src
```

Confirm binary package name produces `ghostpost-backend`:

```bash
rg -n '^name\s*=' backend/Cargo.toml
rg -n '\[\[bin\]\]|name = "ghostpost-backend"' backend/Cargo.toml
```

**Verify**: all `test` commands exit 0; health/CLI/blob/auth env symbols exist. If any are missing, STOP — do not stub a second backend.

### Step 2: Add `backend/.dockerignore`

Create `backend/.dockerignore` with at least:

```
target
.git
.gitignore
.env
.env.*
!.env.example
**/*.md
!prompts/
!prompts/**
**/tests/**
**/*_test.rs
Dockerfile*
.dockerignore
.cargo/config.toml
*.swp
.DS_Store
```

Rules:

- Never allow `.env` or credential files into the build context.
- Keep `Cargo.lock`, `migrations/`, and `src/` included (do not ignore them).
- Do not ignore `rust-toolchain.toml` / `rust-toolchain` if present.

**Verify**: `test -f backend/.dockerignore` and `rg -n '^\.env$' backend/.dockerignore` matches.

### Step 3: Write multi-stage `backend/Dockerfile`

Create `backend/Dockerfile` with **named stages** and these exact contracts.

#### Stage `builder-base`

- `FROM rust:<version>-bookworm` — pin by **digest** at implementation time (resolve current stable bookworm multi-arch digest; never `latest`). Record the chosen digest in a comment above the FROM line.
- Install only compile-time packages required by `backend/Cargo.lock`; prefer
  rustls so OpenSSL is unnecessary. If needed, install `pkg-config` and
  `ca-certificates`, then clear apt lists in the same layer.
- `WORKDIR /build`.
- Do not use a manifest-only stub build: Plan 005 may later make
  `backend/Cargo.toml` a local workspace with `crates/scan-text`, and a Dockerfile
  authored earlier must keep working. Rely on BuildKit registry/git/target
  caches around the real build instead.
- Use BuildKit cache mounts:
  - `--mount=type=cache,target=/usr/local/cargo/registry`
  - `--mount=type=cache,target=/usr/local/cargo/git`
  - `--mount=type=cache,target=/build/target`
- Do **not** pass `DATABASE_URL`, WorkOS, DeepSeek, or AWS keys as `ARG`/`ENV` build-time secrets.

#### Stage `builder`

- `FROM builder-base`.
- `COPY . .` after the dependency-cache layer so real `src/`, `migrations/`,
  `prompts/`, `build.rs`, and `.sqlx` data are available when present.
  `.dockerignore` keeps tests/secrets out; the `!prompts/**` exception is
  mandatory because Plan 005 embeds `backend/prompts/scan-v1.md` at compile
  time.
- Build exactly:

```bash
cargo build --locked --release --bin ghostpost-backend
```

- Produce a stable binary path, e.g. copy out of the cache-mounted target into `/build/ghostpost-backend` with:

```dockerfile
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --locked --release --bin ghostpost-backend \
    && cp /build/target/release/ghostpost-backend /build/ghostpost-backend
```

(Adjust paths if Plan 002 used a different target dir layout; keep `--locked`.)

#### Stage `runtime`

- `FROM debian:bookworm-slim` pinned by **digest** (multi-arch).
- Install **only**: `ca-certificates`, `dumb-init`, `curl`. Clean apt lists same layer.
- Create system user/group `10001:10001` (no login shell, no home writable by others).
- `COPY --from=builder --chown=root:root --chmod=0555 /build/ghostpost-backend /usr/local/bin/ghostpost-backend`
- `ENV BIND_ADDR=0.0.0.0:8080`
- `EXPOSE 8080`
- `USER 10001:10001`
- `ENTRYPOINT ["/usr/bin/dumb-init", "--", "/usr/local/bin/ghostpost-backend"]`
- `CMD ["serve", "--role", "all"]`
- `HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD curl -fsS http://127.0.0.1:8080/health/live || exit 1`
- No package installs, migrations, downloads, or code generation at runtime.
- Do **not** hard-code `FROM --platform=linux/arm64`; keep the Dockerfile multi-arch so the same image builds on laptop (amd64) and production arm64 (Plan 009).

**Verify**:

```bash
docker build --target runtime -t ghostpost-backend:local ./backend
docker image inspect ghostpost-backend:local --format '{{.Config.User}} {{json .Config.Entrypoint}} {{json .Config.Cmd}}'
docker run --rm --entrypoint /usr/local/bin/ghostpost-backend ghostpost-backend:local --help
```

Expected: User `10001:10001` (or `10001`); Entrypoint contains `dumb-init` and binary path; Cmd is `serve --role all` (JSON array); `--help` lists `serve` and `migrate`.

Architecture check (required on the executor machine when buildx is available):

```bash
docker buildx build --platform linux/arm64 --target runtime -t ghostpost-backend:arm64 --load ./backend
docker image inspect ghostpost-backend:arm64 --format '{{.Architecture}} {{.Config.User}}'
```

Expected: `arm64` and non-root user. If arm64 build fails due to a native dependency, STOP (do not silently ship x86_64-only).

### Step 4: Signal spot-checks (pre-Compose)

Run a throwaway container with a dummy DB URL to confirm the binary starts far enough to handle signals (full ready check comes after Compose Postgres):

```bash
CID=$(docker run -d --name gp-sigtest \
  -e BIND_ADDR=0.0.0.0:8080 \
  -e DATABASE_URL_APP=postgres://ghostpost_app:ghostpost_app@127.0.0.1:1/ghostpost \
  -e RUST_LOG=info \
  ghostpost-backend:local serve --role all)
sleep 2
docker kill --signal=SIGTERM "$CID"
docker wait "$CID"
docker rm "$CID" >/dev/null 2>&1 || true
```

**Verify**: container exits; logs show shutdown handling without panic loops. (Ready may be 503 without DB — that is OK here.)

Confirm runtime filesystem is not rebuilding:

```bash
docker run --rm --entrypoint /bin/sh ghostpost-backend:local -c 'test -x /usr/local/bin/ghostpost-backend && test ! -w /usr/local/bin/ghostpost-backend && which curl && test -f /etc/ssl/certs/ca-certificates.crt && id -u'
```

Expected: uid `10001`; binary present non-writable; curl and CA bundle present.

### Step 5: Create MinIO bootstrap script

Create `docker/minio-init.sh` (executable bit in git):

```bash
#!/bin/sh
set -eu
# Waits for MinIO, creates versioned private bucket used by local backend.
# Uses mc against the compose service DNS name "minio".
```

Implementation requirements:

- Image: use `minio/mc` in Compose (see Step 6), so this script is the container command **or** inline the `mc` commands in the Compose `minio-init` service. Prefer a checked-in script for readability.
- Wait loop: retry `mc alias set local http://minio:9000 "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD"` and `mc ready local` until MinIO accepts traffic (authoritative readiness gate when the server image lacks curl/wget).
- Alias MinIO with local root credentials (Compose-only defaults — never production values).
- `mc mb -p local/ghostpost-archives` (bucket name must match `ARCHIVE_BUCKET`).
- Enable versioning: `mc version enable local/ghostpost-archives`.
- Assert versioning is on: `mc version info local/ghostpost-archives` must report `status: Enabled` (exit nonzero otherwise).
- Do **not** set a public anonymous download policy.
- Optional local lifecycle rule approximating production ≤24h raw retention (MinIO lifecycle JSON with `Expiration.Days: 1` and `NoncurrentVersionExpiration`). If the pinned MinIO release cannot express noncurrent expiry the same way, document the limitation in the script comment and still enable versioning; Plan 009 enforces full AWS lifecycle.
- Exit 0 when idempotent re-run succeeds (bucket exists + versioning enabled).

**Verify**:

```bash
test -f docker/minio-init.sh
head -1 docker/minio-init.sh | rg '^#!/.*sh'
rg -n 'version enable|version info' docker/minio-init.sh
```

### Step 5b: Reuse Plan 002 local Postgres role bootstrap

Do not create a second init script. Confirm
`backend/postgres-init.sql` exists from Plan 002 and creates the local
`ghostpost_migrator` and `ghostpost_app` roles. The root Compose file mounts
that exact file. If an existing `postgres-data` volume predates it, stop with a
clear migration message; never delete the developer's volume automatically.

### Step 6: Write root `docker-compose.yml`

Create `docker-compose.yml` at repo root:

```yaml
name: ghostpost
```

Load operator secrets from a gitignored repo-root `.env` (copy from `.env.example`; Compose auto-loads it). Inline only **local-only** Postgres/MinIO defaults in the compose file itself.

#### Volumes

- `postgres-data` — Postgres data only.
- `minio-data` — object store data only.
- **Do not** add an archive volume mounted into `backend`, `migrate`, or any worker service.

#### Service `postgres`

- Image: `postgres:17-bookworm` pinned by digest at implementation time (comment the digest).
- Environment (local-only; never reuse in prod):
  - `POSTGRES_DB=ghostpost`
  - `POSTGRES_USER=ghostpost`
  - `POSTGRES_PASSWORD=ghostpost`
- Ports: `${POSTGRES_PUBLISH_HOST:-127.0.0.1}:5432:5432`
- Volumes: `postgres-data:/var/lib/postgresql/data`
- Mount `./backend/postgres-init.sql:/docker-entrypoint-initdb.d/001-roles.sql:ro`.
- The script creates local-only `ghostpost_migrator` and `ghostpost_app` login
  roles, grants the migrator required database/schema DDL capability, and
  grants the app role CONNECT only initially; Plan 002's migrate wrapper grants
  table/sequence DML. It contains no production credentials.
- Healthcheck: `pg_isready -U ghostpost -d ghostpost` — `interval: 2s`, `timeout: 5s`, `retries: 20`, `start_period: 5s`
- `restart: unless-stopped`
- `stop_grace_period: 30s`

#### Service `minio`

- Image: official MinIO server image pinned by **RELEASE tag and digest** (e.g. `minio/minio:RELEASE.2024-10-02T17-50-41Z@sha256:…`; comment both tag and digest; never `latest`).
- Command: `server /data --console-address ":9001"`
- Environment (local-only defaults):
  - `MINIO_ROOT_USER=ghostpost`
  - `MINIO_ROOT_PASSWORD=ghostpostsecret` (local dev only; not a production secret)
- Ports: `${MINIO_PUBLISH_HOST:-127.0.0.1}:9000:9000` and optionally `127.0.0.1:9001:9001` for console
- Volumes: `minio-data:/data`
- `restart: unless-stopped`
- Do **not** rely on a server-image HTTP health probe unless the pinned RELEASE documents one; `minio-init`'s `mc ready` wait is the authoritative gate.

#### Service `minio-init`

- Image: `minio/mc` pinned by digest (matching the MinIO RELEASE family above).
- Depends on `minio` with `condition: service_started` (plus script wait loop).
- `restart: "no"`
- Environment: pass through `MINIO_ROOT_USER` / `MINIO_ROOT_PASSWORD` local defaults.
- Entrypoint/command runs `docker/minio-init.sh` (mount `./docker/minio-init.sh` read-only) **or** equivalent inline `mc` script.
- Must be idempotent and exit 0 only after versioning is enabled.

#### Service `migrate`

- Build:
  - `context: ./backend`
  - `dockerfile: Dockerfile`
  - `target: runtime`
- `command: ["migrate"]` (keeps ENTRYPOINT)
- Environment:
  - `DATABASE_URL=postgres://ghostpost_migrator:ghostpost_migrator@postgres:5432/ghostpost` (DDL/migrator role per Plan 002)
  - `DATABASE_APP_ROLE=ghostpost_app`
  - `MIGRATION_MAX_CONNECTIONS=2`
  - `MIGRATION_LOCK_TIMEOUT_SECS=60`
  - `RUST_LOG=${RUST_LOG:-info}`
  - `LOG_FORMAT=${LOG_FORMAT:-json}`
- `depends_on`:
  - `postgres: { condition: service_healthy }`
- `restart: "no"`
- **Do not** inject WorkOS, DeepSeek, or S3 credentials into the migrator.

#### Service `backend`

- Same build as `migrate`.
- `command: ["serve", "--role", "all"]`
- Ports: `${API_PUBLISH_HOST:-127.0.0.1}:8080:8080`
- `depends_on` (strict startup order):
  - `postgres: { condition: service_healthy }`
  - `minio-init: { condition: service_completed_successfully }`
  - `migrate: { condition: service_completed_successfully }`
- Healthcheck (Compose override of readiness semantics is allowed; keep path `/health/ready`):

```yaml
healthcheck:
  test: ["CMD", "curl", "-fsS", "http://127.0.0.1:8080/health/ready"]
  interval: 5s
  timeout: 5s
  retries: 12
  start_period: 20s
```

- `restart: unless-stopped`
- `stop_grace_period: 30s`
- Environment (use Compose required interpolation `${VAR:?message}` for WorkOS secrets so missing values fail fast; MinIO/Postgres use local inline defaults):

| Variable | Value / source |
|----------|----------------|
| `DATABASE_URL_APP` | `postgres://ghostpost_app:ghostpost_app@postgres:5432/ghostpost` (DML only — no migrator URL on `backend`) |
| `DB_MAX_CONNECTIONS` | `10` |
| `BIND_ADDR` | `0.0.0.0:8080` |
| `RUST_LOG` | `${RUST_LOG:-info}` |
| `LOG_FORMAT` | `${LOG_FORMAT:-pretty}` local default |
| `CORS_ALLOWED_ORIGINS` | `http://localhost:8081,http://127.0.0.1:8081` (exact; never `*`) |
| `ARCHIVE_BUCKET` | `ghostpost-archives` |
| `ARCHIVE_S3_ENDPOINT` | `http://minio:9000` |
| `ARCHIVE_S3_REGION` | `us-east-1` |
| `ARCHIVE_S3_ACCESS_KEY_ID` | `ghostpost` |
| `ARCHIVE_S3_SECRET_ACCESS_KEY` | `ghostpostsecret` |
| `ARCHIVE_S3_FORCE_PATH_STYLE` | `true` |
| `ARCHIVE_FINGERPRINT_KEYS` | `${ARCHIVE_FINGERPRINT_KEYS:?ARCHIVE_FINGERPRINT_KEYS required}` |
| `WORKOS_API_KEY` | `${WORKOS_API_KEY:?WORKOS_API_KEY required — set in gitignored .env}` |
| `WORKOS_CLIENT_ID` | `${WORKOS_CLIENT_ID:?WORKOS_CLIENT_ID required}` |
| `WORKOS_WEBHOOK_SECRET` | `${WORKOS_WEBHOOK_SECRET:?WORKOS_WEBHOOK_SECRET required}` |
| `WORKOS_COOKIE_PASSWORD` | `${WORKOS_COOKIE_PASSWORD:?WORKOS_COOKIE_PASSWORD required}` |
| `APP_SESSION_KEYS` | `${APP_SESSION_KEYS:?APP_SESSION_KEYS required}` |
| `AUTH_WEB_REDIRECT_URI` | `http://localhost:8081/auth/callback` |
| `AUTH_NATIVE_REDIRECT_URI` | `ghostpost://auth/callback` |
| `AUTH_WEB_ORIGINS` | `http://localhost:8081,http://127.0.0.1:8081` |

Do **not** require `DEEPSEEK_API_KEY` for Compose packaging or the durable-upload
smoke in this plan. Plan 005 uses `stub-deterministic` locally; if it has
landed, include its non-secret provider selector and required local
`SCAN_BATCH_HMAC_KEYS`, but never add an approval bypass.

Document in comments: publishing with `API_PUBLISH_HOST=0.0.0.0` exposes the API on the LAN for physical devices; default stays loopback.

**No** bind-mount of host source or archive paths into the `backend` service for the default path (keeps image immutable). Host-native Cargo is the fast iteration path (Step 9).

**Verify**:

```bash
docker compose config >/tmp/gp-compose.yml
rg -n "postgres:17|minio|migrate|ghostpost-archives|health/ready|service_completed_successfully" /tmp/gp-compose.yml
rg -n 'backend:|migrate:' /tmp/gp-compose.yml | rg -v 'minio-data|postgres-data' || true
! rg -n 'backend:.*volumes:|/archives|archive-data' /tmp/gp-compose.yml
```

Expected: merged config references Postgres 17, MinIO, migrator, bucket name, readiness check, migrate/minio-init success dependencies, and **no** archive bind-mount on backend.

### Step 7: Add env examples and optional helper scripts

Create `backend/.env.example` with **names and local non-secret defaults only**:

```env
# Copy to backend/.env for host-native runs. Never commit real secrets.
DATABASE_URL=postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost
DATABASE_URL_APP=postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost
DATABASE_APP_ROLE=ghostpost_app
DB_MAX_CONNECTIONS=10
MIGRATION_MAX_CONNECTIONS=2
MIGRATION_LOCK_TIMEOUT_SECS=60
BIND_ADDR=0.0.0.0:8080
CORS_ALLOWED_ORIGINS=http://localhost:8081,http://127.0.0.1:8081
ARCHIVE_BUCKET=ghostpost-archives
ARCHIVE_S3_ENDPOINT=http://127.0.0.1:9000
ARCHIVE_S3_REGION=us-east-1
ARCHIVE_S3_ACCESS_KEY_ID=ghostpost
ARCHIVE_S3_SECRET_ACCESS_KEY=ghostpostsecret
ARCHIVE_S3_FORCE_PATH_STYLE=true
ARCHIVE_FINGERPRINT_KEYS=[{"version":1,"secret":"<base64-32-byte-local-secret>"}]
WORKOS_API_KEY=
WORKOS_CLIENT_ID=
WORKOS_WEBHOOK_SECRET=
WORKOS_COOKIE_PASSWORD=
APP_SESSION_KEYS=[{"id":"local_v1","secret":"<base64-32-byte-local-secret>"}]
AUTH_WEB_REDIRECT_URI=http://localhost:8081/auth/callback
AUTH_NATIVE_REDIRECT_URI=ghostpost://auth/callback
AUTH_WEB_ORIGINS=http://localhost:8081,http://127.0.0.1:8081
RUST_LOG=info
LOG_FORMAT=pretty
```

Create repo-root `.env.example` (Compose auto-loads `.env` from project dir):

```env
# Copy to .env at repo root for docker compose. Never commit real values.
ARCHIVE_FINGERPRINT_KEYS=
WORKOS_API_KEY=
WORKOS_CLIENT_ID=
WORKOS_WEBHOOK_SECRET=
WORKOS_COOKIE_PASSWORD=
APP_SESSION_KEYS=
```

Create `scripts/local-up.sh` with this **exact order** (matches Step 8):

1. `docker compose up -d --wait postgres minio`
2. `docker compose run --rm minio-init`
3. `docker compose run --rm migrate`
4. `docker compose up -d --wait --no-deps backend`
5. Print Expo command: `EXPO_PUBLIC_API_URL=http://localhost:8080 npm --prefix app start`

Create `scripts/local-upload-smoke.sh` — **focused durable-upload E2E only** (Plans 003–004 contracts; does not require scan/DeepSeek or import `ready` from Plan 005):

1. Assert `curl -fsS http://127.0.0.1:8080/health/ready`.
2. Obtain an authenticated session using the Plan 003 local test harness **or** a documented test login path if Plan 003 provides one. If auth is not available in the environment, exit with a clear message — do not bypass auth.
3. `POST /v1/archive-imports` with platform `x` or `reddit`, content length of a tiny valid ZIP fixture from Plan 004 tests, accepted content type.
4. Multipart `POST` the returned signed fields plus `file=@fixture.zip` directly to the object-store URL (no session `Authorization`, cookies, JSON/multipart header override, or API proxy).
5. `POST /v1/archive-imports/{id}/complete` with an empty body; capture only
   the sanitized import response and confirm the HTTP status is `202`.
6. Verify the private durable handoff without widening the public API: query
   local Postgres for that import's internal `raw_storage_key` and
   `raw_storage_version_id`, assert both are non-empty, then run
   `mc stat --version-id "$version" local/ghostpost-archives/"$key"` from the
   authenticated `minio-init` container. Never return key/version from an API
   response or log them outside this local smoke.
7. Assert raw object is **not** anonymously readable (`mc anonymous get` must not grant public access; unauthenticated GET to presigned host without sig fails).
8. **Do not** require import status `ready` or worker purge success in this smoke — those belong to Plan 004/005 integration tests. Optional: if ingest worker runs under `role=all` without DeepSeek, assert purge work enqueued or raw delete scheduled per Plan 004.

**Verify**: files exist; `bash -n scripts/local-up.sh scripts/local-upload-smoke.sh` exits 0.

### Step 8: Bring up Compose and prove migrate → ready → versioning

Copy `.env.example` → `.env` and fill WorkOS sandbox keys locally (never commit). Then:

```bash
docker compose build migrate backend
docker compose up -d --wait postgres minio
docker compose run --rm minio-init
docker compose run --rm migrate
docker compose up -d --wait --no-deps backend
curl -fsS http://127.0.0.1:8080/health/live
curl -fsS http://127.0.0.1:8080/health/ready
docker compose exec -T postgres psql -U ghostpost -d ghostpost -c \
  "SELECT version, success FROM _sqlx_migrations ORDER BY version;"
docker compose run --rm --entrypoint /bin/sh minio-init -c \
  'mc version info local/ghostpost-archives'
```

**Verify**:

- migrate exit code 0
- live and ready HTTP 200
- `_sqlx_migrations` shows success=true rows
- MinIO versioning reports `status: Enabled`
- Re-run `docker compose run --rm migrate` is idempotent (exit 0, no duplicate failure)
- Re-run `docker compose run --rm minio-init` is idempotent (exit 0)

SIGTERM drain:

```bash
docker compose kill --signal SIGTERM backend
docker compose logs --no-color backend | tail -n 50
docker compose up -d --wait --no-deps backend
```

**Verify**: logs show graceful shutdown within 30s; backend becomes ready again.

### Step 9: Host-native Cargo workflow against Compose deps

With Postgres + MinIO still up:

```bash
docker compose stop backend || true
export DATABASE_URL='postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost'
export DATABASE_URL_APP='postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost'
export ARCHIVE_BUCKET=ghostpost-archives
export ARCHIVE_S3_ENDPOINT='http://127.0.0.1:9000'
export ARCHIVE_S3_REGION=us-east-1
export ARCHIVE_S3_ACCESS_KEY_ID=ghostpost
export ARCHIVE_S3_SECRET_ACCESS_KEY=ghostpostsecret
export ARCHIVE_S3_FORCE_PATH_STYLE=true
export BIND_ADDR='0.0.0.0:8080'
export CORS_ALLOWED_ORIGINS='http://localhost:8081,http://127.0.0.1:8081'
# plus WorkOS vars from .env — do not echo values
cargo run --locked --manifest-path backend/Cargo.toml -- migrate
cargo run --locked --manifest-path backend/Cargo.toml -- serve --role all
```

In another terminal:

```bash
curl -fsS http://127.0.0.1:8080/health/live
curl -fsS http://127.0.0.1:8080/health/ready
```

**Verify**: both 200; host binary talks to Compose Postgres and MinIO. This path is the default fast iteration loop; Compose `backend` service remains the parity path for image/signal testing.

### Step 10: Local durable upload E2E

Run:

```bash
bash scripts/local-upload-smoke.sh
```

If Plan 004 already ships a Rust integration test for archive upload, you may run that **in addition** after discovering the actual test target name:

```bash
cargo test --locked --manifest-path backend/Cargo.toml -- --list | rg archive
# then run the listed test, e.g.:
# cargo test --locked --manifest-path backend/Cargo.toml <name_from_list> -- --nocapture
```

Do not invent a test module name that `--list` does not show.

**Verify**:

- Reserve → signed multipart POST → complete → pinned version ID matches MinIO object version; tampered/oversized form is rejected
- Raw object not readable anonymously from MinIO without credentials
- No backend bind-mount or container-local path used as durable queue state
- API logs contain no ZIP filename, raw bytes, or credentials

### Step 11: Expo wiring documentation check (no app code changes)

Confirm developers can point Expo at the local API without source edits:

```bash
# Do not leave this running as a gate; start briefly or document only if headless CI lacks UI
EXPO_PUBLIC_API_URL='http://localhost:8080' npm --prefix app run export:web
```

**Verify**: export exits 0 when dependencies installed; bundle was produced with the env inlined. Missing `EXPO_PUBLIC_API_URL` must still select mock at module init today — that is expected until Plan 007 makes missing URL a hard failure. This plan must **not** put secrets into any `EXPO_PUBLIC_*` variable.

Physical device note (document in `scripts/local-up.sh` comments only):

- Android emulator API base: `http://10.0.2.2:8080`
- Physical device: `API_PUBLISH_HOST=0.0.0.0` and `EXPO_PUBLIC_API_URL=http://<LAN-IP>:8080`

## Test plan

- **Image unit checks** (Steps 3–4): architecture, non-root user, entrypoint/cmd, CA certs, help text, SIGTERM.
- **Compose integration**: postgres healthy → minio-init (versioning on) → migrate → backend ready; idempotent migrate/minio-init; `_sqlx_migrations` rows.
- **Durable upload smoke** (`scripts/local-upload-smoke.sh`): presign path uses MinIO with pinned version ID; not local disk; no anonymous read.
- **Host-native path**: cargo migrate + serve against Compose deps.
- **Negative checks**:
  - `docker compose run -e DATABASE_URL=postgres://bad migrate` fails nonzero.
  - Backend without MinIO endpoint fails readiness or archive routes safely (no hang).
  - `docker compose config` shows no archive bind-mount on `backend`.

Model any Rust tests after existing Plan 002/004 integration test layout under `backend/tests/`.

Verification summary command set:

```bash
docker compose config >/dev/null
docker build --target runtime -t ghostpost-backend:local ./backend
docker compose up -d --wait postgres minio
docker compose run --rm minio-init
docker compose run --rm migrate
docker compose up -d --wait backend
curl -fsS http://127.0.0.1:8080/health/live
curl -fsS http://127.0.0.1:8080/health/ready
bash scripts/local-upload-smoke.sh
```

Expected: all exit 0.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `backend/Dockerfile` multi-stage with pinned builder/runtime digests, non-root `10001`, `dumb-init`, CA certs, curl healthcheck, `CMD serve --role all`, no secret build args
- [ ] `backend/.dockerignore` excludes `.env` and `target`
- [ ] `docker compose config` succeeds
- [ ] `docker compose run --rm migrate` exits 0 twice (idempotent)
- [ ] `curl -fsS http://127.0.0.1:8080/health/live` and `/health/ready` succeed with Compose backend up
- [ ] MinIO bucket `ghostpost-archives` exists with versioning enabled (`mc version info` shows Enabled)
- [ ] Internal DB assertion shows completion pinned the exact MinIO object version; sanitized HTTP response contains no storage key/version
- [ ] No archive volume mounted on `backend` (`docker compose config` shows none)
- [ ] Local durable upload smoke passes (reserve → signed POST → complete → version pin proof + oversize rejection)
- [ ] ARM64 image build/inspect succeeds when buildx available (`Architecture=arm64`)
- [ ] SIGTERM to backend container drains within 30s
- [ ] Host-native `cargo run --locked … migrate` and `serve --role all` work against Compose Postgres/MinIO
- [ ] No secrets committed; `.env.example` and `backend/.env.example` have empty or local-only placeholder values
- [ ] No files outside the in-scope list modified (`git status`)
- [ ] `plans/README.md` status row updated if you own the index

## STOP conditions

Stop and report back (do not improvise) if:

- Plans 002/003/004 backend artifacts are missing or use a different binary/CLI/health/blob/auth contract than this plan assumes.
- The code at locations in "Current state" does not match (drift).
- Any step verification fails twice after a reasonable fix attempt.
- ARM64 build fails or a native dependency is x86_64-only.
- Dockerfile would require secrets as build args to compile (e.g. private crate registry) without a documented non-secret alternative.
- Compose would need a backend-mounted archive volume for uploads to work.
- MinIO cannot enable versioning or the backend cannot address version IDs required by Plan 004 completion pinning.
- Health routes require WorkOS/DeepSeek calls to become ready (violates DesignIntegrator: ready = DB + role init only).
- You are asked to commit production credentials or reuse local Postgres/MinIO passwords in production configs.
- Fix appears to require editing out-of-scope app source, SST/IaC files, or backend business logic beyond env wiring.

## Maintenance notes

- When Plan 009 lands, keep Dockerfile stages and binary CLI stable — production migrate/API tasks should reuse `context: ./backend`, `target: runtime`, and the same digest pins.
- Bump base image digests deliberately; record digest changes in the PR body.
- If API and worker split into separate services later, Compose should add a second service with `--role worker` rather than mounting shared disks.
- Reviewers should scrutinize: non-root user, no secret build args, versioning enabled, no archive volume on backend, readiness vs liveness split, loopback publish defaults, immutable version pin proof in smoke.
- Deferred to Plan 009: AWS bucket policies, KMS, ALB, external Postgres vendor gates, provider-approval manifest, ordered cloud deploy.
- Local MinIO lifecycle is best-effort parity; production lifecycle rules in Plan 009 are authoritative.
