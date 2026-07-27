# Plan 002: Backend kernel, schema, and Postgres work queue

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md app/package.json app/package-lock.json`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: none
- **Category**: migration
- **Planned at**: commit `3625ed1`, 2026-07-23

## Why this matters

At `3625ed1` the repository is Expo-only: there is no `backend/`, no durable
Postgres schema, and no shared kernel for config, errors, telemetry, health, or
async work. Plans 003–007 need a single Rust binary with role separation, SQLx
migrations, tenant-scoped composite foreign keys, a fenced work queue, and
canonical tables for auth storage, onboarding, idempotency, import staging,
content, scans, batches, model attempts, flags, reviews, entitlement grants, and work
items — before WorkOS routes, archive parsers, model transport, product APIs,
Docker packaging, or SST land.

Without this kernel, later plans would invent parallel config systems,
inconsistent error shapes, and incompatible queue semantics. This plan lands the
**schema + repository + jobs primitives + health/migrate CLI only**;
product HTTP routes, WorkOS, blob adapters, DeepSeek transport, Dockerfile, and
SST remain explicitly out of scope.

## Canonical sources

Honor these artifacts (read before implementing; do not contradict them):

| Source | Use |
|--------|-----|
| Backend architecture review | Single package/binary, role model, kernel module boundaries, shutdown/health contracts, DDL/DML split, work-queue fencing |
| API/data-model review | Table names, tenant composite FK pattern, column semantics aligned to future `/v1` APIs and `app/src/domain/types.ts` |
| DesignIntegrator + SecurityReview + ProductFlowReview | Tenant isolation, no unlock/grant product routes in kernel, presign/immutable-version constraints deferred to Plan 004, `free_beta` stored not exposed |
| `plans/008-containerize-local-stack.md` | Downstream packaging expects binary `ghostpost-backend`, `serve --role api\|worker\|all`, `migrate`, `/health/live`, `/health/ready`, embedded SQLx migrations |

## Current state

Facts at `3625ed1`:

- Repository root contains `app/`, `design/`, `.cursor/`. **No** `backend/`,
  `docker-compose.yml`, `Dockerfile`, or `sst.config.ts`.
- `app/src/domain/types.ts` defines client DTOs (`UserProfile`, `OnboardingAnswers`,
  `FlaggedPost`, `ScanStatus`, `ReviewAction`, `PlatformId`, `RiskLevel`) that
  later product APIs must map to — kernel tables store canonical server shapes,
  not camelCase JSON.
- `app/src/services/api/http.ts` lists future product paths (`/me`, `/dashboard`,
  `/onboarding`, `/scan`, `/review`, `/posts/:id/actions`, `/home/unlock`) — **do
  not implement these routes in Plan 002**.
- This backend plan is independent of the Expo TypeScript repair and may execute
  in parallel with Plan 001. It does not modify `app/**`.

### Domain alignment excerpts (storage targets, not routes)

```ts
// app/src/domain/types.ts — enums the schema must accommodate
export type RiskLevel = 'high' | 'medium' | 'low';
export type PlatformId = 'facebook' | 'reddit' | 'instagram' | 'tiktok' | 'x';
export type ReviewAction = 'delete' | 'archive' | 'keep' | 'resolve';
export type ScanPhase = 'connecting' | 'scanning' | 'flagging' | 'complete';
```

```ts
// app/src/services/api/http.ts — OUT OF SCOPE for Plan 002 handlers
// getProfile → /me
// submitOnboarding → POST /onboarding
// startScan/getScanStatus → /scan
// applyReviewAction → POST /posts/:id/actions
// unlockHome → POST /home/unlock  (Plan 006 entitlement routes; NOT in kernel)
```

## Architecture summary

### Single package / binary

- One Cargo package at `backend/` (no workspace members).
- Binary name: **`ghostpost-backend`** (`[[bin]]` in `backend/Cargo.toml`).
- Library crate optional (`backend/src/lib.rs`) for integration tests; production
  ships one binary.

### Role model

| Role | Responsibilities in Plan 002 |
|------|------------------------------|
| `api` | Axum router: `/health/live`, `/health/ready` only; opens DML pool; **no product routes** |
| `worker` | Work-item claim loop, heartbeat renewer, max-attempt sweeper, cancellation recovery; **no HTTP** |
| `all` | Runs API + worker tasks in one Tokio runtime (local dev default for Plan 008) |
| `migrate` | Separate subcommand (not a serve role): applies SQLx migrations using **DDL credentials** |

CLI surface:

```text
ghostpost-backend migrate [--dry-run]
ghostpost-backend serve --role api|worker|all [--bind ADDR]
```

### Kernel modules (canonical map)

| Module path | Responsibility |
|-------------|----------------|
| `backend/src/main.rs` | CLI entry, role dispatch |
| `backend/src/cli.rs` | Clap parsing for `migrate` / `serve` |
| `backend/src/config/mod.rs` | Env loading, validation, typed config |
| `backend/src/error/mod.rs` | `AppError` taxonomy, `Result` alias |
| `backend/src/api/problem.rs` | RFC 7807 Problem+JSON mapping (health + future APIs) |
| `backend/src/api/health.rs` | Live/ready handlers and readiness checks |
| `backend/src/api/router.rs` | Axum router wiring (health only in Plan 002) |
| `backend/src/telemetry/mod.rs` | `tracing` subscriber, `RUST_LOG`, optional `LOG_FORMAT=json\|pretty` |
| `backend/src/shutdown/mod.rs` | SIGTERM/SIGINT → cancellation token; 30s drain budget |
| `backend/src/db/pool.rs` | DML pool factory |
| `backend/src/db/ddl.rs` | DDL connection for migrate only |
| `backend/src/db/migrate.rs` | `sqlx::migrate!` runner |
| `backend/src/repository/*.rs` | One repo per aggregate; SQLx queries only |
| `backend/src/jobs/queue.rs` | Fenced claim, heartbeat, and commit SQL |
| `backend/src/jobs/runner.rs` | Worker claim loop + shutdown integration |
| `backend/src/jobs/sweeper.rs` | Max-attempt / stale-lease sweeper |
| `backend/src/jobs/cancel.rs` | Cancellation propagation + recovery |

**Out of scope modules** (later plans): `api/auth.rs`, `api/profile.rs`,
`api/archive_imports.rs`, `api/scans.rs`, `api/flags.rs`, `archive/parsers`,
`model/deepseek`, `jobs/handlers/*`, `blob/s3`, `Dockerfile`, `sst`.

## Canonical table map

All tenant-owned rows use **`PRIMARY KEY (tenant_id, id)`** and child FKs
reference **`FOREIGN KEY (tenant_id, parent_id) REFERENCES parent(tenant_id, id)`**
to prevent cross-tenant joins.

| Table | Purpose | Owning repo |
|-------|---------|-------------|
| `tenants` | Root tenant row | `repository/users.rs` |
| `users` | Ghostpost user; optional `workos_user_id` (filled by Plan 003) | `repository/users.rs` |
| `auth_sessions` | Opaque session storage (hash only; no plaintext tokens) | `repository/auth.rs` |
| `onboarding_profiles` | `coming_up`, `concerns`, `platforms` arrays | `repository/onboarding.rs` |
| `idempotency_keys` | Request dedupe + lock | `repository/mod.rs` (Repositories aggregate) |
| `archive_imports` | Import staging lifecycle | `repository/archive_imports.rs` |
| `content_items` | Normalized posts from imports | `repository/archive_imports.rs` |
| `scans` | Scan lifecycle + cancellation | `repository/scans.rs` |
| `scan_batches` | Chunked scan work units | `repository/scans.rs` |
| `model_attempts` | Per-batch LLM call audit | `repository/scans.rs` |
| `flagged_posts` | Risk flags linked to content | `repository/flags.rs` |
| `review_actions` | User review decisions | `repository/flags.rs` |
| `entitlement_grants` | Trusted grants (`free_beta`, future SKUs) — **storage only** | `repository/entitlements.rs` |
| `entitlement_usages` | Rescan/quota consumption audit | `repository/entitlements.rs` |
| `work_items` | Async queue | `repository/work_items.rs` |
| `work_item_attempts` | Attempt audit trail | `repository/work_items.rs` |

### Representative DDL invariants

Migration `001_init.sql` (name may vary) must establish:

```sql
-- Extensions
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS citext;

-- Roles (DDL/DML split) — passwords supplied via migrate env, not checked in
DO $$ BEGIN
  CREATE ROLE ghostpost_migrator LOGIN;
  CREATE ROLE ghostpost_app LOGIN;
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- Tenant root
CREATE TABLE tenants (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  created_at timestamptz NOT NULL DEFAULT now()
);

-- Composite PK pattern
CREATE TABLE users (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  workos_user_id text,
  email citext,
  display_name text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  UNIQUE (tenant_id, workos_user_id)
);

CREATE TABLE work_items (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  subject_user_id uuid,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  kind text NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}',
  dedupe_key text,
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','running','succeeded','failed','cancelled')),
  priority int NOT NULL DEFAULT 0,
  run_after timestamptz NOT NULL DEFAULT now(),
  attempt_count int NOT NULL DEFAULT 0,
  max_attempts int NOT NULL DEFAULT 5,
  lease_owner text,
  lease_expires_at timestamptz,
  heartbeat_at timestamptz,
  cancel_requested_at timestamptz,
  last_error text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  PRIMARY KEY (tenant_id, id),
  CONSTRAINT work_items_attempt_count_check CHECK (attempt_count >= 0),
  FOREIGN KEY (tenant_id, subject_user_id)
    REFERENCES users(tenant_id, id)
    ON DELETE SET NULL (subject_user_id)
);

CREATE INDEX work_items_claim_idx ON work_items (status, run_after, priority DESC)
  WHERE status = 'pending' AND cancel_requested_at IS NULL;
CREATE UNIQUE INDEX work_items_dedupe_idx
  ON work_items (tenant_id, kind, dedupe_key)
  WHERE dedupe_key IS NOT NULL;

CREATE INDEX work_items_stale_lease_idx ON work_items (status, lease_expires_at)
  WHERE status = 'running';
CREATE INDEX work_items_subject_idx
  ON work_items (tenant_id, subject_user_id, status)
  WHERE subject_user_id IS NOT NULL AND status IN ('pending', 'running');
```

Additional migrations in the same plan must create the remaining tables with the
same composite-FK rule. Example child FK:

```sql
ALTER TABLE flagged_posts
  ADD CONSTRAINT flagged_posts_scan_fk
  FOREIGN KEY (tenant_id, scan_id) REFERENCES scans(tenant_id, id);
```

**Entitlement grants/usages** (storage only — no HTTP grant/unlock in this plan):

```sql
CREATE TABLE entitlement_grants (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  product_key text NOT NULL
    CHECK (product_key IN ('single','sevenDay','thirtyDay','free_beta')),
  grant_source text NOT NULL CHECK (grant_source IN ('admin','promotion','test')),
  review_access boolean NOT NULL DEFAULT true,
  rescan_limit int,
  platform_limit int,
  scan_scope_id uuid,
  valid_from timestamptz NOT NULL DEFAULT now(),
  valid_until timestamptz,
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, user_id) REFERENCES users(tenant_id, id)
);

CREATE TABLE entitlement_usages (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  entitlement_id uuid NOT NULL,
  user_id uuid NOT NULL,
  scan_id uuid NOT NULL,
  usage_kind text NOT NULL CHECK (usage_kind = 'rescan'),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, entitlement_id) REFERENCES entitlement_grants(tenant_id, id),
  FOREIGN KEY (tenant_id, user_id) REFERENCES users(tenant_id, id),
  UNIQUE (tenant_id, entitlement_id, scan_id, usage_kind)
);
-- Plan 006 resolves/consumes via repository; kernel does NOT expose /home/unlock
```

After DDL, grant DML privileges only:

```sql
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO ghostpost_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO ghostpost_app;
-- ghostpost_migrator owns migrations; ghostpost_app cannot CREATE/ALTER/DROP
```

### Work queue: fenced claim / heartbeat / commit

All worker mutations run in transactions.

**Claim** (jobs `queue.rs`):

```sql
-- $1 = lease_owner (instance id), $2 = lease_duration
UPDATE work_items wi
SET status = 'running',
    lease_owner = $1,
    lease_expires_at = now() + $2::interval,
    heartbeat_at = now(),
    attempt_count = attempt_count + 1,
    updated_at = now()
WHERE (tenant_id, id) = (
  SELECT tenant_id, id FROM work_items
  WHERE status = 'pending'
    AND run_after <= now()
    AND cancel_requested_at IS NULL
    AND attempt_count < max_attempts
  ORDER BY priority DESC, created_at ASC
  FOR UPDATE SKIP LOCKED
  LIMIT 1
)
RETURNING wi.*;
```

Insert matching `work_item_attempts` row with `attempt_number = attempt_count`.

**Heartbeat** (must verify fence):

```sql
WHERE tenant_id = $1 AND id = $2
  AND lease_owner = $4
  AND status = 'running'
RETURNING id;
```

**Commit success**:

```sql
UPDATE work_items
SET status = 'succeeded', finished_at = now(), lease_owner = NULL,
    lease_expires_at = NULL, updated_at = now()
WHERE tenant_id = $1 AND id = $2 AND lease_owner = $3 AND status = 'running';
```

**Commit failure** (retryable): set `status = 'pending'`, clear lease, set
`run_after = now() + backoff`, store `last_error` (truncated), unless
`attempt_count >= max_attempts` → `failed`.

**Max-attempt sweeper** (`jobs/sweeper.rs`): periodic task marks
`status = 'failed'` where `attempt_count >= max_attempts AND status IN ('pending','running')`,
and clears stale leases where `status = 'running' AND lease_expires_at < now()`
→ revert to `pending` or `failed` per attempt budget.

**Cancellation recovery** (`jobs/cancel.rs`):

- Setting `cancel_requested_at` on `work_items`, `scans`, or `archive_imports`
  must not delete rows inline.
- Worker checks `cancel_requested_at` after claim; if set, commit
  `status = 'cancelled'` and write attempt outcome `cancelled`.
- Recovery job: imports/scans stuck in `running` with expired lease and
  `cancel_requested_at IS NOT NULL` → terminal `cancelled`.
- Recovery job: imports/scans in `running` with expired lease and no cancel →
  revert to resumable state (`pending` / `verifying`) per state machine table in
  repo docstring.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Drift check | `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md` | review only |
| Start Postgres 17 | `docker compose -f backend/docker-compose.postgres.yml up -d --wait` | healthy |
| Copy env | `cp backend/.env.example backend/.env` then fill URLs | files exist |
| SQLx offline prep | `cd backend && cargo sqlx prepare -- --all-targets` | updates `backend/.sqlx/` |
| Migrate (DDL) | `cd backend && cargo run --locked -- migrate` | exit 0; `_sqlx_migrations` rows |
| Idempotent migrate | `cd backend && cargo run --locked -- migrate` | exit 0; no error |
| Unit tests | `cd backend && cargo test --locked` | exit 0 |
| Postgres integration | `cd backend && cargo test --locked --test postgres_integration -- --ignored --test-threads=1` | exit 0 (requires running Postgres) |
| Serve all (dev) | `cd backend && cargo run --locked -- serve --role all` | listens on `8080` |
| Live health | `curl -fsS http://127.0.0.1:8080/health/live` | HTTP 200 JSON |
| Ready health | `curl -fsS http://127.0.0.1:8080/health/ready` | HTTP 200 when DB up |
| Ready without DB | `DATABASE_URL=postgres://bad@127.0.0.1:1/x cargo run -- serve --role api` | `/health/ready` → 503 Problem+JSON, `/health/live` → 200 |
| Worker-only | `cargo run --locked -- serve --role worker` | logs claim loop; no HTTP bind required |
| DML cannot DDL | `psql "$DATABASE_URL_APP" -c 'CREATE TABLE evil(id int)'` | permission denied |
| Shutdown | `kill -TERM <pid>` | stops within 30s; no panic |

All `cargo` commands use `backend/` as cwd. Use `--locked` everywhere.

Do **not** run repo-wide formatters, `npm run lint`, or full app test suites as
gates for this plan.

## Suggested executor toolkit

- Rust stable ≥ 1.80 (record MSRV in `backend/Cargo.toml` `rust-version`).
- `cargo-sqlx` CLI for offline data (`cargo install sqlx-cli --no-default-features --features rustls,postgres`).
- Docker Engine for Postgres 17 compose file shipped in this plan.
- `curl` for health checks.

## Scope

**In scope** (only these paths may be created or modified):

- `backend/Cargo.toml`, `backend/Cargo.lock`
- `backend/src/**` — kernel, `repository/*`, `jobs/*`, health HTTP only
- `backend/migrations/**` — SQLx schema migrations; no login creation or passwords
- `backend/.sqlx/**` — offline query data for `--locked` CI builds
- `backend/tests/**` — focused Postgres integration tests
- `backend/docker-compose.postgres.yml` — **Postgres 17 only** (no MinIO/backend)
- `backend/postgres-init.sql` — local-only DDL/DML login bootstrap
- `backend/.env.example` — variable names only
- `backend/README.md` — local dev: compose up, migrate, serve roles, test commands
- `backend/.gitignore` — `target/`, `.env`
- Optional: `backend/rust-toolchain.toml` pinning stable

**Out of scope** (do NOT touch):

- WorkOS SDK, auth routes, session cookies, CSRF (`plans/003-*`)
- Archive parsers, presigned upload routes, `BlobStore` (`plans/004-*`)
- DeepSeek / model HTTP transport (`plans/005-*`)
- Product routes: `/me`, `/dashboard`, `/onboarding`, `/scan`, `/review`,
  `/posts/*`, `/home/unlock`, `/demo/reset`
- Entitlement **product** behavior (unlock/grant/demo); **`entitlement_grants` /
  `entitlement_usages` storage is in scope**, HTTP is not
- `Dockerfile`, root `docker-compose.yml`, MinIO (`plans/008-*`)
- `sst.config.ts`, AWS (`plans/009-*`)
- `app/**` source (except reading types for alignment)
- Any file outside the in-scope list above

## Environment variables

Document in `backend/.env.example` (no secret values committed):

| Variable | Used by | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | migrate | Migrator/DDL role URL only |
| `DATABASE_APP_ROLE` | migrate | Existing DML role identifier receiving grants; default `ghostpost_app` only in local dev |
| `DATABASE_URL_APP` | serve (api/worker/all) | App/DML role URL; falls back to `DATABASE_URL` in local dev only |
| `DB_MAX_CONNECTIONS` | serve | Per-process app pool maximum; default `10`, validate `>=1` |
| `MIGRATION_MAX_CONNECTIONS` | migrate | DDL pool maximum; exactly `2` in v1 (one advisory-lock session + one migrator session) |
| `MIGRATION_LOCK_TIMEOUT_SECS` | migrate | Advisory migration lock wait; default `60`, fail rather than run concurrently |
| `BIND_ADDR` | api/all | Default `0.0.0.0:8080` |
| `RUST_LOG` | all | e.g. `info,sqlx=warn` |
| `LOG_FORMAT` | all | `pretty` (dev) or `json` |
| `INSTANCE_ID` | worker | Lease owner token; default hostname+pid |
| `WORKER_LEASE_SECS` | worker | Default `60` |
| `WORKER_HEARTBEAT_SECS` | worker | Default `20` |
| `WORKER_SWEEPER_SECS` | worker | Default `30` |
| `SHUTDOWN_DEADLINE_SECS` | all | Default `30` |
| `RESTORE_REPLAY_PENDING` | api/all | When `true`, `/health/ready` returns 503 until deletion-ledger replay completes (Plan 009 ops gate; hook lives in health module) |

Do **not** add WorkOS, S3, or DeepSeek env vars in this plan.

## Git workflow

- Branch: `advisor/002-backend-kernel-schema`
- Commits (small, logical):
  1. `Add backend crate skeleton and CLI roles`
  2. `Add kernel config error problem telemetry shutdown`
  3. `Add SQLx migrations and repositories`
  4. `Add work queue claim heartbeat sweeper cancellation`
  5. `Add postgres integration tests and compose dev dependency`
- Match repo imperative subject style (`Add …`, `Fix …`).
- Do NOT push or open a PR unless the operator instructed it.

## Steps

### Step 0: Drift check

```bash
cd /path/to/ghostpost
git rev-parse --short HEAD
git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md app/package.json
test ! -d backend || echo "WARN: backend/ already exists — compare to plan"
```

**Verify**: If `backend/` already exists with divergent layout, STOP. Do not run
or gate on Expo checks; Plan 001 owns them independently.

### Step 1: Scaffold single-package backend

Create `backend/Cargo.toml`:

- `name = "ghostpost-backend"` (package name may differ; binary name must be
  `ghostpost-backend`)
- `[lib]` + `[[bin]] name = "ghostpost-backend"`
- Dependencies (pin in lockfile): `tokio`, `axum`, `clap`, `sqlx` with
  `runtime-tokio-rustls`, `postgres`, `migrate`, `uuid`, `serde`,
  `serde_json`, `thiserror`, `anyhow`, `tracing`, `tracing-subscriber`,
  `tower`, `tower-http`, `chrono`

Create `backend/src/main.rs` + `cli.rs` with `migrate` and `serve --role`.

**Verify**:

```bash
cd backend
cargo build --locked
./target/debug/ghostpost-backend --help
```

Expected: subcommands `migrate`, `serve`; `serve` accepts `--role`.

### Step 2: Kernel — config, error, problem, telemetry, shutdown

Implement modules per canonical map:

- **config**: load dotenv only in dev (`#[cfg(debug_assertions)]` or explicit
  `GHOSTPOST_ENV=development`); require `DATABASE_URL` for migrate; require
  `DATABASE_URL_APP` (or fallback) for serve.
- **error**: variants `Config`, `Database`, `Migration`, `Worker`, `Shutdown`,
  `NotFound`, `Conflict`, `InvalidInput`.
- **problem**: map to `application/problem+json` with `type`, `title`, `status`,
  `detail` (no stack traces in `detail` when `LOG_FORMAT=json` prod).
- **telemetry**: init once at startup; filter via `RUST_LOG`.
- **shutdown**: `tokio::signal` ctrl-c + SIGTERM (unix); propagate
  `CancellationToken` to worker loops; API stops accepting new connections.

**Verify**:

```bash
cd backend
cargo test --locked config error problem
```

### Step 3: Postgres 17 compose dependency

Create `backend/docker-compose.postgres.yml`:

```yaml
services:
  postgres:
    image: postgres:17-alpine
    environment:
      POSTGRES_USER: ghostpost
      POSTGRES_PASSWORD: ghostpost
      POSTGRES_DB: ghostpost
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U ghostpost -d ghostpost"]
      interval: 5s
      timeout: 5s
      retries: 5
    volumes:
      - ghostpost_pg_data:/var/lib/postgresql/data
      - ./postgres-init.sql:/docker-entrypoint-initdb.d/001-roles.sql:ro
volumes:
  ghostpost_pg_data:
```

Create `backend/postgres-init.sql` with idempotent local-only
`ghostpost_migrator` / `ghostpost_app` login creation, `CONNECT` for both, and
database/schema `CREATE` only for the migrator. It runs as the local
`POSTGRES_USER` only when the volume is first initialized. If a pre-existing
volume lacks either role, stop with a clear message; never delete the volume
automatically.

```sql
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ghostpost_migrator') THEN
    CREATE ROLE ghostpost_migrator LOGIN PASSWORD 'ghostpost_migrator';
  END IF;
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'ghostpost_app') THEN
    CREATE ROLE ghostpost_app LOGIN PASSWORD 'ghostpost_app';
  END IF;
END
$$;
GRANT CONNECT, CREATE ON DATABASE ghostpost TO ghostpost_migrator;
GRANT USAGE, CREATE ON SCHEMA public TO ghostpost_migrator;
GRANT CONNECT ON DATABASE ghostpost TO ghostpost_app;
```

Create `backend/.env.example`:

```bash
DATABASE_URL=postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost
DATABASE_URL_APP=postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost
DATABASE_APP_ROLE=ghostpost_app
DB_MAX_CONNECTIONS=10
MIGRATION_MAX_CONNECTIONS=2
MIGRATION_LOCK_TIMEOUT_SECS=60
BIND_ADDR=0.0.0.0:8080
RUST_LOG=info,ghostpost_backend=debug,sqlx=warn
LOG_FORMAT=pretty
INSTANCE_ID=dev-local
```

**Verify**:

```bash
docker compose -f backend/docker-compose.postgres.yml up -d --wait
docker compose -f backend/docker-compose.postgres.yml ps
```

Expected: `postgres` healthy.

### Step 4: SQLx migrations — full canonical schema + DDL/DML split

Add migrations under `backend/migrations/`:

1. `YYYYMMDDHHMMSS_extensions.sql` — required extensions only; no role creation or passwords
2. `YYYYMMDDHHMMSS_core_tenant_user_auth.sql` — tenants, users, auth_sessions
3. `YYYYMMDDHHMMSS_onboarding_idempotency.sql`
4. `YYYYMMDDHHMMSS_import_content.sql` — archive_imports, content_items
5. `YYYYMMDDHHMMSS_scan_batch_model.sql` — scans, scan_batches, model_attempts
6. `YYYYMMDDHHMMSS_flags_reviews_entitlements.sql` — entitlement_grants, entitlement_usages
7. `YYYYMMDDHHMMSS_work_items.sql` — work_items, work_item_attempts, indexes

Database roles are infrastructure prerequisites, never SQLx migration
artifacts. Local Postgres init creates `ghostpost_migrator` and
`ghostpost_app`; the production vendor/operator creates equivalent DDL and DML
roles and supplies `DATABASE_APP_ROLE`. Validate that value as an unquoted
Postgres identifier before safely quoting it. Do not create a production login,
accept a role password, or interpolate secrets in a migration file.

Implement `db/migrate.rs` using `sqlx::migrate!("./migrations")`. Build a DDL
pool capped by `MIGRATION_MAX_CONNECTIONS`, hold one dedicated connection with
the fixed Postgres advisory lock key `ghostpost_migrations_v1`, and run the
embedded migrator while that connection remains alive. Before and after the
migrator, use safely quoted `DATABASE_APP_ROLE` to set default privileges and
grant only required DML/sequence/schema access; never grant CREATE or DDL.
Wrap lock acquisition in `MIGRATION_LOCK_TIMEOUT_SECS`; timeout exits nonzero
without running any migration. Release the advisory lock on success/error.

**Verify**:

```bash
cd backend
cp .env.example .env   # local only; never commit .env
cargo run --locked -- migrate
docker compose -f docker-compose.postgres.yml exec -T postgres \
  psql -U ghostpost -d ghostpost -c "\dt"
docker compose -f docker-compose.postgres.yml exec -T postgres \
  psql -U ghostpost -d ghostpost -c "SELECT version, success FROM _sqlx_migrations ORDER BY version;"
cargo run --locked -- migrate
```

Expected: all tables exist; second migrate idempotent.

### Step 5: Repositories (DML pool only)

Implement repository modules with SQLx **checked against offline data**:

- Each repo method takes `tenant_id` explicitly.
- No `SELECT *` in hot paths; name columns.
- Insert/update return structured types mapped to Rust enums mirroring CHECK
  constraints (`WorkItemStatus`, `ScanPhase`, `ImportStatus`, etc.).

Entitlement repo exposes `grant_free_beta(tenant_id, user_id)` and
`get_active_grant(...)` for **tests and later plans** — no HTTP wiring.

**Verify**:

```bash
cd backend
cargo sqlx prepare -- --all-targets
cargo test --locked repositories
git add .sqlx
```

### Step 6: Worker — claim, heartbeat, commit, sweeper, cancellation

Wire worker loops for `serve --role worker|all`:

- Claim loop respects shutdown token (stop claiming; allow in-flight commit).
- Heartbeat task runs every `WORKER_HEARTBEAT_SECS` for owned leases.
- Sweeper task runs every `WORKER_SWEEPER_SECS`.
- Cancel recovery scans `scans` + `archive_imports` + `work_items` for
  stale/cancelled states.

No real job handlers in Plan 002 — integration tests inject `kind =
'test.ping'` handler inside test-only module or process payload echo.

**Verify**:

```bash
cd backend
cargo test --locked worker
```

### Step 7: Health HTTP (api role only)

Routes:

- `GET /health/live` → 200 `{"status":"live"}` always (process up).
- `GET /health/ready` → 200 when DML pool connects and `SELECT 1` succeeds;
  worker role initialized if `--role all`; 503 Problem+JSON on failure or when
  `RESTORE_REPLAY_PENDING=true`.

Bind `BIND_ADDR`. No other routes.

**Verify**:

```bash
cd backend
cargo run --locked -- serve --role all &
sleep 2
curl -fsS http://127.0.0.1:8080/health/live
curl -fsS http://127.0.0.1:8080/health/ready
kill -TERM $!
```

### Step 8: Focused real-Postgres integration tests

Create `backend/tests/postgres_integration.rs` with `#[ignore]` tests requiring
live Postgres (compose file). Use `--test-threads=1` to reduce migration races.

Required cases:

| Test | Asserts |
|------|---------|
| `migrate_idempotent` | double migrate OK |
| `tenant_composite_fk_rejects_cross_tenant` | insert with wrong tenant FK fails |
| `work_item_claim_fence` | two owners; heartbeat/commit only with matching `lease_owner` |
| `work_item_max_attempts_sweeper` | after forced failures → `failed` |
| `cancellation_marks_cancelled` | cancel_requested → worker yields cancelled terminal state |
| `stale_lease_recovery` | expired lease → item returns to pending or fails per budget |
| `dml_role_cannot_create_table` | `DATABASE_URL_APP` user lacks CREATE |
| `entitlement_grants_free_beta_storage` | repo can store/retrieve `free_beta`; no HTTP test |

Run:

```bash
docker compose -f backend/docker-compose.postgres.yml up -d --wait
cd backend
cargo run --locked -- migrate
cargo test --locked --test postgres_integration -- --ignored --test-threads=1
```

**Verify**: all ignored tests pass when Postgres is up.

### Step 9: Graceful shutdown proof

```bash
cd backend
cargo run --locked -- serve --role all &
PID=$!
sleep 2
kill -TERM $PID
wait $PID
echo EXIT:$?
```

**Verify**: exit code 0 or 130; logs show shutdown within 30s; no panic stack.

### Step 10: Final diff guard

```bash
cd /path/to/ghostpost
git status
git diff --stat
git diff --stat 3625ed1 -- app/
```

**Verify**:

- Only in-scope backend paths changed.
- No `app/` source changes.
- No Dockerfile, root compose, SST, WorkOS, S3, or product routes added.

## Test plan

- **Unit**: config parsing, problem JSON serialization, status enums, backoff
  calculation, cancellation state transitions (pure functions).
- **Integration (real Postgres)** — `backend/tests/postgres_integration.rs`:
  - migrations apply idempotently
  - composite tenant FK enforcement
  - fenced claim / heartbeat / commit
  - max-attempt sweeper
  - cancellation recovery
  - DDL/DML role split
  - `entitlement_grants` `free_beta` row storage via repository
- **Manual smoke**: `serve --role all` + health curls + SIGTERM drain.
- **Explicitly not required**: HTTP product routes, WorkOS, MinIO, model calls,
  Expo E2E, Docker image build.

## Security review amendments

These are **mandatory** adjustments from SecurityReview; implement in Plan 002:

1. **Tenant isolation**: every query includes `tenant_id` predicate; composite
   FKs on all child tables; repos must not expose cross-tenant lookups by bare
   `id` UUID alone.
2. **DDL/DML split**: migrator role holds DDL; runtime uses app role without
   CREATE/ALTER/DROP. CI integration test must prove CREATE fails on app URL.
3. **Session storage**: `auth_sessions.token_hash` stores SHA-256 or Argon2 hash
   only; never log or persist raw tokens.
4. **Idempotency locking**: `idempotency_keys.locked_until` prevents concurrent
   duplicate POST replay; unique on `(tenant_id, scope, key)`.
5. **Lease fencing**: commit/heartbeat must match `lease_owner`; stale owner
   commits are rejected (0 rows updated → treat as lost lease).
6. **Problem responses**: `/health/ready` 503 bodies use Problem+JSON without
   internal DB errors in `detail` when `LOG_FORMAT=json`.
7. **No product bypass**: do not add `/home/unlock`, `/demo/reset`, or admin
   entitlement grant routes — `free_beta` is repository/storage only until Plan 006.
8. **Secrets**: no secrets in `Cargo.toml`, migrations committed to git, or
   tracing spans; `.env` gitignored.
9. **SQL injection**: SQLx parameterized queries only; no dynamic SQL string
   concat for user input.
10. **Shutdown hygiene**: on SIGTERM, stop claiming work; do not leave
    `running` rows with refreshed leases unless heartbeat task still owned by
    draining worker (then cancel heartbeat first).

## Done criteria

Machine-checkable. ALL must hold:

- [ ] `backend/Cargo.toml` + `backend/Cargo.lock` exist; binary `ghostpost-backend`
- [ ] `ghostpost-backend migrate` applies embedded migrations idempotently
- [ ] `ghostpost-backend serve --role api|worker|all` parses and runs
- [ ] `GET /health/live` → 200; `GET /health/ready` → 200 with Postgres up, 503 without
- [ ] All canonical tables exist with tenant composite PK/FK pattern
- [ ] DDL/DML roles enforced; app role cannot CREATE TABLE (integration test)
- [ ] Work queue implements fenced claim, heartbeat, commit, sweeper, cancellation recovery
- [ ] `entitlement_grants` supports `free_beta` storage; **no** product unlock/grant routes
- [ ] `backend/docker-compose.postgres.yml` runs Postgres 17 locally
- [ ] `backend/.sqlx/` committed; `cargo build --locked` succeeds without live DB
- [ ] `cargo test --locked --test postgres_integration -- --ignored --test-threads=1` passes with compose Postgres up
- [ ] SIGTERM shutdown completes within 30s without panic
- [ ] No files outside in-scope list modified (`git status`)
- [ ] `plans/README.md` status row for 002 updated (unless reviewer owns index)

## STOP conditions

Stop and report back (do not improvise) if:

- Drift check shows unexpected `backend/` content incompatible with single-package
  layout.
- `cargo sqlx prepare` cannot run because migrations are invalid on Postgres 17.
- Composite FK pattern cannot be applied to a table without breaking canonical
  model — escalate rather than dropping tenant scope.
- Integration tests require product routes, WorkOS, or MinIO to pass — they must
  not; simplify the test.
- Executor is tempted to add `/home/unlock`, `/demo/*`, or entitlement grant HTTP
  to make manual testing easier — forbidden; use repository tests instead.
- A step's verification fails twice after a reasonable fix attempt.
- Docker Postgres 17 image unavailable on executor platform — report verbatim error.
- Adding Dockerfile or root compose feels "required" for local dev — defer to Plan 008.

## Maintenance notes

- Plan 008 packages this binary; keep CLI flags and health paths stable.
- Plan 003 adds WorkOS on top of `users` + `auth_sessions` without schema breaks.
- Plan 004 adds import routes using `archive_imports` + `work_items` kinds.
- Plan 005 adds model transport writing `model_attempts`.
- Plan 006 adds entitlement **routes** reading/writing `entitlement_grants` —
  kernel storage already exists.
- Keep `backend/.sqlx/` updated whenever repository SQL changes; CI uses
  `SQLX_OFFLINE=true`.
- Reviewers should scrutinize: (1) any HTTP route beyond health, (2) missing
  `tenant_id` predicates, (3) lease fencing on commit, (4) accidental DML grants
  to migrator role, (5) secrets in logs.
