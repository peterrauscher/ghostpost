# Plan 003: WorkOS auth, profiles, onboarding, and tenant isolation

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md app/src/domain/types.ts app/src/services/api/ app/app.json`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: plans/002-backend-kernel-schema.md
- **Category**: feature
- **Planned at**: commit `3625ed1`, 2026-07-23

## Why this matters

Ghostpost must authenticate real users before archive ingestion, scan, or
review data can be tenant-scoped. At `3625ed1` the Expo app talks to a mock
adapter or an uncredentialed HTTP shim (`/me`, `POST /onboarding`) with no
server-side identity, no onboarding persistence, and no isolation between
users. Plan 002 lands the Rust kernel, PostgreSQL, health probes, and
migration harness; this plan adds **additive backend-only** WorkOS AuthKit
integration, durable profiles/onboarding, platform catalog, webhook handling,
and account deletion — without changing Expo source (frontend cutover is Plan
007).

WorkOSAuth and SecurityReview canonical constraints require:

- Server-selected redirect URIs (clients never supply arbitrary redirect URLs).
- PKCE + one-use `state` for every authorization attempt; **server generates and stores** the PKCE verifier (never client-supplied).
- A **second client-bound exchange proof** independent of OAuth `state` and PKCE:
  web receives it in an HttpOnly pre-auth cookie; native receives it once in the
  authorize JSON body; both present the proof at `POST /v1/auth/exchange`.
- WorkOS refresh material sealed at rest; Ghostpost API authorization uses a
  separate **hashed opaque app session** never stored verbatim in Postgres.
- Web mutations require a **session-bound rotating CSRF token** plus exact
  `Origin` / Fetch Metadata checks.
- Tenant isolation on every read/write keyed by authenticated `tenant_id` + `user_id`.
- Readiness remains DB + role init only — WorkOS availability must **not**
  block `/health/ready`.

## Current state

Facts at `3625ed1` (and after Plan 002 has landed):

- No `backend/` exists at the planned-at commit; Plan 002 creates
  `backend/Cargo.toml`, `backend/migrations/`, `ghostpost-backend` CLI
  (`serve --role api|worker|all`, `migrate`), and health routes.
- Expo app scheme: `ghostpost` (`app/app.json`).
- Web dev origin expected locally: `http://localhost:8081` and
  `http://127.0.0.1:8081` (Plan 008 Compose wiring).
- Frontend domain types (behavior unchanged by this plan):

```ts
// app/src/domain/types.ts
export interface UserProfile {
  id: string;
  name: string;
  greetingName: string;
  avatarUrl?: string;
}

export interface OnboardingAnswers {
  comingUp: ComingUpOption[];
  concerns: ConcernOption[];
  platforms: PlatformId[];
}
```

- Frontend HTTP adapter today (Plan 007 will cut over paths/auth transport):

```ts
// app/src/services/api/http.ts
getProfile: () => request<UserProfile>('/me'),
submitOnboarding: (answers) =>
  request<{ ok: true }>('/onboarding', { method: 'POST', body: JSON.stringify(answers) }),
```

- App gate/onboarding UI state is device-local (`app/src/providers/app-state.tsx`);
  server onboarding is new.
- Plan 002 is expected to provide baseline tables (`tenants`, `users`,
  `auth_sessions`, `onboarding_profiles`, `_sqlx_migrations`, etc.); **this
  plan adds auth/onboarding columns and auth-only tables only when absent** —
  do not recreate or contradict Plan 002 DDL.

### Canonical contracts (WorkOSAuth + DataModelApi + SecurityReview)

Honor these exactly; Compose (Plan 008) wires env names from this plan:

| Topic | Contract |
|-------|----------|
| Authorize entry | `GET /v1/auth/authorize?client=web\|native` — **no** `redirect_uri` query param; **always JSON** (never `302`) |
| PKCE storage | Server generates the S256 PKCE verifier and stores it encrypted in `auth_flows`; clients never receive or send the verifier |
| Pre-auth binding | Second one-time secret **distinct from** `state` and PKCE: web → `Set-Cookie: gp_auth_init=…`; native → `exchangeSecret` field in authorize JSON |
| Redirect selection | Server maps `client` → exact allowlisted **frontend** URI from env |
| OAuth hardening | One-use `state`, S256 PKCE, 10-minute flow TTL |
| Login completion | **`POST /v1/auth/exchange`** consumes `code`, `state`, `client`, and exchange proof — **no backend OAuth callback route** |
| WorkOS session | Encrypted/sealed blob in `workos_sessions`; decrypted only in auth service |
| App session | Random 256-bit token → `gp_session` cookie (web) or JSON body (native); store SHA-256 hash in Plan 002 `auth_sessions` |
| CSRF | `GET /v1/auth/csrf` returns rotating token bound to app session; required on web POST/PUT/PATCH/DELETE |
| Fetch Metadata | Reject web mutations when `Sec-Fetch-Site` ∉ `{same-origin, same-site}` or `Sec-Fetch-Mode` ∉ `{cors, same-origin}` |
| Origin | Exact match against per-env allowlist (no subdomain wildcards) |
| Profile | `GET /v1/me` — authenticated profile projection only; lifecycle is derived from onboarding/import/scan/entitlement resources in Plan 007 |
| Onboarding | `GET\|PUT /v1/me/onboarding` — revisioned snapshot; PUT persists answers + consent |
| Platforms | `GET /v1/platforms` — static availability catalog aligned with `PLATFORM_OPTIONS` ids |
| Webhook | `POST /v1/webhooks/workos` with signature verification + idempotency |
| Deletion | `DELETE /v1/me` — authenticated account delete; cascade tenant data; revoke WorkOS + app sessions |
| Readiness | `/health/ready` checks Postgres (+ role init), **not** WorkOS |

**Forbidden route aliases** (must not register): `POST /v1/auth/authorize`,
`GET /v1/auth/callback`, `POST /v1/auth/callback`, `GET /v1/lifecycle`,
`POST /v1/onboarding`, `GET /v1/onboarding`, `GET /v1/platforms/catalog`,
`DELETE /v1/account`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Drift check | `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md app/src/domain/types.ts app/src/services/api/` | Review only |
| Plan 002 present | `test -f backend/Cargo.toml && test -d backend/migrations && rg -n 'health/live\|health/ready\|migrate' backend/src` | exit 0 |
| Migrations apply | `cargo run --locked --manifest-path backend/Cargo.toml -- migrate` | exit 0 |
| Unit + integration tests | `cargo test --locked --manifest-path backend/Cargo.toml auth::` | exit 0 |
| Two cookie jars (web) | `cargo test --locked --manifest-path backend/Cargo.toml two_cookie_jar` | exit 0 |
| Two native contexts | `cargo test --locked --manifest-path backend/Cargo.toml two_native_context` | exit 0 |
| CSRF rotation | `cargo test --locked --manifest-path backend/Cargo.toml csrf_rotation` | exit 0 |
| Key rotation | `cargo test --locked --manifest-path backend/Cargo.toml session_key_rotation` | exit 0 |
| Local authorize (web) | `curl -s -c /tmp/gp-jar.txt 'http://127.0.0.1:8080/v1/auth/authorize?client=web' \| jq -e '.authorizationUrl and .state'` | JSON + `Set-Cookie: gp_auth_init=…` |
| Local authorize (native) | `curl -s 'http://127.0.0.1:8080/v1/auth/authorize?client=native' \| jq -e '.authorizationUrl and .state and .exchangeSecret'` | JSON with `exchangeSecret` |
| Ready unaffected | WorkOS unreachable → `curl -fsS http://127.0.0.1:8080/health/ready` | still `200` when DB up |

Do **not** run repo-wide formatters, linters, or Expo gates as part of this plan.

## Suggested executor toolkit

- WorkOS staging application with AuthKit enabled.
- `curl`, `jq`, and a cookie jar file for web flow tests.
- PostgreSQL reachable via `DATABASE_URL` (Compose from Plan 008 or local).
- Rust toolchain per Plan 002 MSRV.

## Scope

**In scope** (the only paths you should create or modify):

- `backend/src/**` — auth module, profile/onboarding handlers, middleware, config
- `backend/migrations/*` — additive SQL extending Plan 002 tables; auth-only tables when absent
- `backend/tests/**` — integration tests including two-cookie-jar and two-native-context suites
- `backend/.env.example` — extend with auth env names (if file exists from Plan 002/008; additive keys only)
- `backend/Cargo.toml` / `backend/Cargo.lock` — add the official `workos` Rust SDK (pin the current `3.0.x` selected by Cargo.lock), plus only sealing/sha2/middleware crates not supplied by it

**Out of scope** (do NOT touch):

- `app/**` — frontend auth transport, `/v1` path cutover, SecureStore wiring (Plan 007)
- Archive upload, scan, entitlement, billing (Plans 004–006)
- Docker/Compose/SST (Plans 008–009)
- Changing Expo mock fixtures or `GhostpostApi` interface
- Weakening PKCE, exchange proof, CSRF, or Origin checks for local convenience
- Storing WorkOS refresh tokens or app session tokens in plaintext
- `/health/ready` calling WorkOS
- Any forbidden route alias listed in canonical contracts above

## Git workflow

- Branch: `advisor/003-workos-auth-profiles-onboarding`
- Commits (examples):
  1. `feat(backend): add auth schema migrations`
  2. `feat(backend): implement WorkOS authorize exchange refresh logout`
  3. `feat(backend): add profiles onboarding platforms and webhooks`
  4. `test(backend): two cookie jar and native context auth isolation`
- Do NOT push or open a PR unless instructed.

## Environment variables

Add to config loading (fail fast at startup if required vars missing in `api` role):

| Variable | Required | Purpose |
|----------|----------|---------|
| `WORKOS_API_KEY` | yes | WorkOS API authentication |
| `WORKOS_CLIENT_ID` | yes | OAuth client id |
| `WORKOS_WEBHOOK_SECRET` | yes | Webhook signature verification |
| `WORKOS_COOKIE_PASSWORD` | yes | 32+ char secret for sealing WorkOS session payloads (AES-256-GCM) |
| `APP_SESSION_KEYS` | yes | JSON array `[{"id":"k1","secret":"<base64 32 bytes>"}]` — newest first; supports rotation |
| `AUTH_WEB_REDIRECT_URI` | yes | Exact **frontend** web callback registered in WorkOS, e.g. `http://localhost:8081/auth/callback` |
| `AUTH_NATIVE_REDIRECT_URI` | yes | Exact **frontend** native callback, e.g. `ghostpost://auth/callback` |
| `AUTH_WEB_ORIGINS` | yes | Comma-separated exact origins, e.g. `http://localhost:8081,http://127.0.0.1:8081` |
| `AUTH_INIT_COOKIE_MAX_AGE_SECS` | no | Default `600` (10 minutes) |
| `APP_SESSION_MAX_AGE_SECS` | no | Default `2592000` (30 days) |
| `CSRF_TOKEN_MAX_AGE_SECS` | no | Default `3600` (1 hour) |
| `CORS_ALLOWED_ORIGINS` | yes | Must mirror `AUTH_WEB_ORIGINS` for credentialed fetches |

Startup validation:

- Reject `AUTH_*` values with trailing slashes (normalize internally).
- Reject `*` anywhere in origin/redirect lists.
- Reject redirect URIs pointing at backend `/v1/auth/*` paths — WorkOS must redirect to frontend callbacks only.
- Require at least one `APP_SESSION_KEYS` entry with `id` matching `^[a-z0-9_]+$`.

## Database schema additions

Create a new migration (do not edit previously applied Plan 002 migrations).
**Reuse Plan 002 tables**; add columns and auth-only tables **only when absent**.

### Plan 002 tables reused (do not recreate)

| Table | Plan 002 role | Plan 003 changes |
|-------|---------------|------------------|
| `tenants` | Root tenant row | unchanged |
| `users` | `(tenant_id, id)` composite PK; optional `workos_user_id`, `email`, `display_name` | `ALTER TABLE … ADD COLUMN IF NOT EXISTS` for columns below |
| `auth_sessions` | Opaque session hash storage (`token_hash` only) | `ALTER TABLE … ADD COLUMN IF NOT EXISTS` for WorkOS/CSRF columns below |
| `onboarding_profiles` | `coming_up`, `concerns`, `platforms` text arrays | `ALTER TABLE … ADD COLUMN IF NOT EXISTS` for consent columns below |

Use `CREATE TABLE IF NOT EXISTS` only for auth-only tables not present in Plan 002.

### `users` — additive columns only

Plan 002 already defines `(tenant_id, id)` composite PK, `workos_user_id`, `email`,
`display_name`, timestamps. Add when absent:

| Column | Type | Notes |
|--------|------|-------|
| `greeting_name` | `text` | lowercased first token or email local-part |
| `avatar_url` | `text` NULL | |
| `deleted_at` | `timestamptz` NULL | soft-delete marker; hard purge async |

After first login, enforce NOT NULL on `workos_user_id`, `email`, `display_name`,
`greeting_name` at the application layer (Plan 002 allows NULL until Plan 003
fills them).
Ghostpost v1 uses a **personal-tenant invariant**: one WorkOS user maps to one
tenant and one active user row. Add a partial global unique index on
`users(workos_user_id) WHERE workos_user_id IS NOT NULL`; login reuses that
tenant and never provisions a second tenant for the same WorkOS user.

Example migration fragment:

```sql
ALTER TABLE users ADD COLUMN IF NOT EXISTS greeting_name text;
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_url text;
ALTER TABLE users ADD COLUMN IF NOT EXISTS deleted_at timestamptz;
```

### `auth_sessions` — additive columns only

Extend Plan 002 `auth_sessions` (composite PK `(tenant_id, id)`; existing
`token_hash`, expiry, revocation columns). Add when absent:

| Column | Type | Notes |
|--------|------|-------|
| `key_id` | `text` | from `APP_SESSION_KEYS` |
| `client` | `text` CHECK (client IN ('web','native')) | |
| `csrf_hash` | `bytea` NULL | SHA-256(current CSRF); NULL for native |
| `csrf_rotated_at` | `timestamptz` NULL | |
| `workos_session_id` | `uuid` NULL | FK → `workos_sessions(tenant_id, id)` |

Add partial unique index on `(token_hash)` where `revoked_at IS NULL` if Plan 002
did not already create it.

### `workos_sessions` (new — auth-only)

| Column | Type | Notes |
|--------|------|-------|
| `tenant_id` | `uuid` NOT NULL | FK → `tenants(id)` ON DELETE CASCADE |
| `id` | `uuid` NOT NULL DEFAULT gen_random_uuid() | |
| `user_id` | `uuid` NOT NULL | FK `(tenant_id, user_id)` → `users(tenant_id, id)` ON DELETE CASCADE |
| `sealed_session` | `bytea` NOT NULL | AES-256-GCM ciphertext (nonce + ct + tag) |
| `seal_key_version` | `text` NOT NULL | e.g. `workos_v1` — rotate `WORKOS_COOKIE_PASSWORD` with re-seal job |
| `workos_session_id` | `text` NULL | WorkOS session id when exposed |
| `expires_at` | `timestamptz` NOT NULL | |
| `revoked_at` | `timestamptz` NULL | |
| `created_at` | `timestamptz` NOT NULL DEFAULT now() | |

Primary key: `(tenant_id, id)`.

Partial index: `(tenant_id, user_id)` where `revoked_at IS NULL`.

Unique (when `workos_session_id` present): `(workos_session_id)`.

### `auth_flows` (new — auth-only)

Pre-login rows are global (no tenant until user is known).

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | |
| `state` | `text` UNIQUE NOT NULL | 128-bit random, base64url |
| `code_verifier_hash` | `bytea` NOT NULL | SHA-256(PKCE verifier) |
| `code_verifier_enc` | `bytea` NOT NULL | AES-256-GCM encrypted verifier for token exchange |
| `exchange_secret_hash` | `bytea` NOT NULL | SHA-256(client-bound secret — distinct from PKCE and `state`) |
| `client` | `text` NOT NULL CHECK (client IN ('web','native')) | |
| `redirect_uri` | `text` NOT NULL | server-selected frontend callback copy |
| `tenant_id` | `uuid` NULL | set after exchange when known |
| `user_id` | `uuid` NULL | set after exchange when known |
| `consumed_at` | `timestamptz` NULL | one-use |
| `expires_at` | `timestamptz` NOT NULL | |
| `created_at` | `timestamptz` NOT NULL DEFAULT now() | |

Index: `(expires_at)` for sweeper.

### `onboarding_profiles` — reuse Plan 002 shape

Plan 002 already stores `coming_up`, `concerns`, and `platforms` arrays under
the composite `(tenant_id, user_id)` owner key. Add when absent:

| Column | Type | Notes |
|--------|------|-------|
| `revision` | `bigint NOT NULL DEFAULT 0` | optimistic-concurrency token |
| `current_step` | `smallint NOT NULL DEFAULT 1` | 1–4, CHECK constrained |
| `consent_version` | `text` NULL | disclosure version accepted |
| `consent_accepted_at` | `timestamptz` NULL | server timestamp |
| `completed_at` | `timestamptz` NULL | server-derived completion |

Do not store a parallel answers JSON blob. API code validates enum IDs and
derives `status` as `not_started`, `in_progress`, or `completed`; clients never
write timestamps or `completed_at`.

```sql
ALTER TABLE onboarding_profiles
  ADD COLUMN IF NOT EXISTS revision bigint NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS current_step smallint NOT NULL DEFAULT 1,
  ADD COLUMN IF NOT EXISTS consent_version text,
  ADD COLUMN IF NOT EXISTS consent_accepted_at timestamptz,
  ADD COLUMN IF NOT EXISTS completed_at timestamptz;
```
### `webhook_events` (new — auth-only)

| Column | Type | Notes |
|--------|------|-------|
| `id` | `uuid` PK | |
| `provider` | `text` NOT NULL DEFAULT 'workos' | |
| `event_id` | `text` NOT NULL | WorkOS event id |
| `event_type` | `text` NOT NULL | |
| `payload_hash` | `bytea` NOT NULL | SHA-256(raw body) |
| `processed_at` | `timestamptz` NOT NULL DEFAULT now() | |

Unique: `(provider, event_id)`.

### Tenant isolation rule

Every query on tenant-owned rows must filter by authenticated session's
`(tenant_id, user_id)`. Provide a reusable `TenantContext { tenant_id, user_id }`
extractor in this plan. Plan 004+ tables already follow Plan 002 composite FK
pattern — do not introduce single-column `user_id` FKs that bypass `tenant_id`.

## HTTP API specification

Successful JSON responses use `application/json`; `204` responses have no body.
All errors reuse Plan 002 RFC Problem Details:

```json
{
  "type": "https://ghostpost.app/problems/onboarding-revision-conflict",
  "title": "Onboarding revision conflict",
  "status": 409,
  "detail": "Refresh onboarding state and retry.",
  "instance": "/v1/me/onboarding",
  "code": "ONBOARDING_REVISION_CONFLICT"
}
```

Use `application/problem+json`, stable `code`, and sanitized `detail`. Common
statuses: `400`, `401`, `403`, `409`, `410`, `422`, `429`, `500`.

### `GET /v1/auth/authorize`

**Query**

| Param | Required | Values |
|-------|----------|--------|
| `client` | yes | `web` \| `native` |

**Behavior**

1. Validate `client`.
2. Select `redirect_uri` from env (`AUTH_WEB_REDIRECT_URI` or `AUTH_NATIVE_REDIRECT_URI`).
3. Generate `state` (128-bit), PKCE `code_verifier` (256-bit), and `exchange_secret` (256-bit) **on the server**.
4. Insert `auth_flows` row; store `code_verifier_hash`, encrypted `code_verifier_enc`, and `exchange_secret_hash` only.
5. Build WorkOS authorize URL with `response_type=code`, `client_id`, `redirect_uri`, `state`, `code_challenge` (S256), `code_challenge_method=S256`.
6. **Web**: `Set-Cookie: gp_auth_init=<exchange_secret>; HttpOnly; Secure; SameSite=Lax; Path=/v1/auth; Max-Age=600`
7. Return **`200` JSON** for both clients (never `302` from this handler):

**Response `200` (web)**

```json
{
  "authorizationUrl": "https://api.workos.com/sso/authorize?...",
  "state": "…",
  "expiresAt": "2026-07-23T08:32:00Z"
}
```

Plus `Set-Cookie: gp_auth_init=…` as above. Do **not** include `exchangeSecret` in the JSON body for web.

**Response `200` (native)**

```json
{
  "authorizationUrl": "https://api.workos.com/sso/authorize?...",
  "state": "…",
  "exchangeSecret": "base64url-256-bit-one-time-secret",
  "expiresAt": "2026-07-23T08:32:00Z"
}
```

Client opens `authorizationUrl` (browser / `expo-web-browser`). WorkOS redirects to the **frontend** callback URI; there is **no** backend OAuth callback route.

### `POST /v1/auth/exchange`

The frontend callback forwards WorkOS `code` and `state` to this route. There
is no backend OAuth callback route.

Web body:

```json
{ "client": "web", "code": "opaque", "state": "opaque" }
```

Web exchange proof is only the HttpOnly `gp_auth_init` cookie set by the
authorize response. Never accept `exchangeSecret` in a web request body.

Native body:

```json
{
  "client": "native",
  "code": "opaque",
  "state": "opaque",
  "exchangeSecret": "server-issued-one-time-secret"
}
```

Native exchange proof is the `exchangeSecret` returned by the matching native
authorize response.

Validation and transaction:

1. Load flow by `state`; missing, expired, consumed, or replayed → `410`.
2. Require body `client` to equal stored flow client.
3. Web: constant-time compare `SHA-256(gp_auth_init cookie)` to stored
   `exchange_secret_hash`; missing/mismatch → `403`.
4. Native: constant-time compare `SHA-256(exchangeSecret)` to stored hash;
   missing/mismatch → `403`.
5. Exchange `code` plus the decrypted server-held S256 PKCE verifier with
   WorkOS. The client never receives or submits the verifier.
6. Transactionally resolve the globally unique `workos_user_id` under the
   partial unique index. Concurrent first exchanges may create at most one
   personal tenant/user; on unique conflict roll back any candidate tenant and
   load the winner. If its `deleted_at` is non-null, return
   `410 ACCOUNT_DELETION_PENDING` without rehydrating fields or minting a
   session. Otherwise update the active profile, seal the WorkOS session, mint
   a random Ghostpost app session, store only its SHA-256 hash, mark the flow
   consumed, and initialize CSRF state for web.
7. Clear `gp_auth_init`. Any failure before commit creates no app session.

Web success: **`204 No Content`** plus HttpOnly `gp_session`; no JSON body.

Native success: **`200 application/json`**:

```json
{
  "user": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "email": "user@example.test",
    "name": "Jordan Lee",
    "greetingName": "jordan",
    "avatarUrl": null
  },
  "session": {
    "kind": "bearer",
    "token": "base64url-256-bit",
    "expiresAt": "2026-08-22T08:22:00Z"
  }
}
```

Native API calls use `Authorization: Bearer <session.token>`. Neither response
contains a WorkOS access, refresh, or ID token.
### `POST /v1/auth/refresh`

Web requires the `gp_session` cookie plus valid CSRF and exact Origin. Native
requires the current Ghostpost bearer session. Refresh is single-flight at the
client and single-use at the server:

1. lock and validate the app session hash and expiry;
2. refresh the sealed WorkOS session server-side;
3. atomically revoke the old app-session hash and insert the new hash;
4. rotate CSRF state for web.

Replaying the old session after successful rotation returns `401`.

Web success: `204 No Content` plus rotated `gp_session` cookie.

Native success:

```json
{
  "session": {
    "kind": "bearer",
    "token": "new-base64url-token",
    "expiresAt": "2026-08-22T09:22:00Z"
  }
}
```
### `POST /v1/auth/logout`

Same auth transport as refresh.

**Behavior**: revoke app session row, revoke linked WorkOS session, clear cookies (`gp_session`, `gp_auth_init`).

**Response**: `204 No Content`.

### `GET /v1/auth/csrf`

**Auth**: web session cookie only.

**Response `200`**

```json
{
  "token": "base64url-128-bit",
  "expiresAt": "2026-07-23T09:22:00Z"
}
```

Rotates CSRF hash on every GET; invalidates previous token. Native clients omit CSRF.

### `GET /v1/me`

Requires session cookie or Ghostpost bearer token. Return the authenticated
profile projection only:

```json
{
  "user": {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "email": "user@example.test",
    "name": "Jordan Lee",
    "greetingName": "jordan",
    "avatarUrl": null
  }
}
```

Do not embed lifecycle, onboarding, import, scan, or entitlement state. Plan
007 derives navigation from their authoritative resource routes. Invalid,
expired, revoked, or deleted-user sessions return `401`.
### `GET /v1/me/onboarding`

Requires authentication. Return one revisioned `OnboardingState`:

```json
{
  "status": "in_progress",
  "currentStep": 3,
  "revision": 7,
  "answers": {
    "comingUp": ["college_apps"],
    "concerns": ["public_image"],
    "platforms": ["x"],
    "disclosureConsent": {
      "version": "2026-07-23",
      "accepted": false
    }
  }
}
```

No row returns `status=not_started`, `currentStep=1`, `revision=0`, empty
arrays, and consent `{ version: CURRENT_DISCLOSURE_VERSION, accepted: false }`.
The response contains no `completed` boolean and no lifecycle/gate projection.
### `PUT /v1/me/onboarding`

Requires authentication; web additionally requires the rotating CSRF token,
exact allowlisted Origin, and accepted Fetch Metadata. The request is the full
`OnboardingState` snapshot last read plus the user's merged edits:

```json
{
  "status": "completed",
  "currentStep": 4,
  "revision": 7,
  "answers": {
    "comingUp": ["college_apps"],
    "concerns": ["public_image"],
    "platforms": ["x"],
    "disclosureConsent": {
      "version": "2026-07-23",
      "accepted": true
    }
  }
}
```

Rules:

- `revision` must equal the current DB revision; mismatch returns `409
  ONBOARDING_REVISION_CONFLICT` and changes nothing.
- Reject unknown/duplicate enum IDs and unknown fields. `comingUp` and
  `concerns` use the app unions; selected platforms must currently be
  archive-enabled (`reddit` or `x`), while the catalog still lists disabled
  future platforms.
- `currentStep` is 1–4 and cannot move backward after persisted progress.
- Server derives/validates `status`: `not_started` only for the empty initial
  state; `completed` requires non-empty coming-up, concerns, platforms,
  `currentStep=4`, and the current disclosure version accepted.
- The first false→true consent transition records server `now()` in
  `consent_accepted_at`; clients never submit a timestamp. Changing disclosure
  version requires a new acceptance.
- One transaction updates arrays/current step/consent, sets or clears
  `completed_at` according to validated status, and increments `revision`.

Return `200` with the complete persisted `OnboardingState` and new revision.
Never return a lifecycle object. Missing consent/inconsistent status or
disabled platform returns `422`; CSRF/Origin failures return `403`.
### `GET /v1/platforms`

**Auth**: optional (public read).

**Response `200`**

```json
{
  "revision": "2026-07-23",
  "platforms": [
    { "id": "facebook", "label": "Facebook", "glyph": "f", "color": "#0866FF", "archiveEnabled": false },
    { "id": "reddit", "label": "Reddit", "glyph": "●", "color": "#FF4500", "archiveEnabled": true },
    { "id": "instagram", "label": "Instagram", "glyph": "ig", "color": "#E1306C", "useImage": true, "archiveEnabled": false },
    { "id": "tiktok", "label": "TikTok", "glyph": "♪", "color": "#111111", "useImage": true, "archiveEnabled": false },
    { "id": "x", "label": "X", "glyph": "𝕏", "color": "#111111", "useImage": true, "archiveEnabled": true }
  ],
  "comingUpOptions": [
    { "id": "rush", "label": "Rush" },
    { "id": "college_apps", "label": "College apps" },
    { "id": "job_interviews", "label": "Job interviews" },
    { "id": "friends_family", "label": "Friends or family" },
    { "id": "just_concerned", "label": "Just concerned" },
    { "id": "something_else", "label": "Something else" }
  ],
  "concernOptions": [
    { "id": "inappropriate_language", "label": "Inappropriate language" },
    { "id": "drinking_drugs", "label": "Drinking / drugs" },
    { "id": "political_takes", "label": "Political takes" },
    { "id": "controversial_topics", "label": "Controversial topics" },
    { "id": "negativity", "label": "Negativity" },
    { "id": "public_image", "label": "Public image" },
    { "id": "other", "label": "Other" }
  ]
}
```

Ids must match `app/src/domain/types.ts` unions exactly. Reddit and X have
`archiveEnabled: true` at launch; others are `coming_soon` / `archiveEnabled: false`.

### `POST /v1/webhooks/workos`

**Auth**: WorkOS signature header (not session).

**Headers**: `WorkOS-Signature` (per WorkOS docs).

**Behavior**

1. Verify signature with `WORKOS_WEBHOOK_SECRET` and raw body.
2. Insert `webhook_events` idempotently; duplicate `event_id` → `200` no-op.
3. Handle at minimum:
   - `user.updated` → sync `users.display_name`, `avatar_url` only where `deleted_at IS NULL`; never rehydrate a tombstone
   - `user.deleted` → in one transaction tombstone the user, revoke app/WorkOS
     sessions, and idempotently enqueue the same `account.purge` workflow as
     `DELETE /v1/me`; WorkOS `404/not_found` is success at the provider checkpoint
   - `session.revoked` → revoke matching `workos_sessions`

**Response**: `200 { "ok": true }`.

### `DELETE /v1/me`

**Auth**: required + CSRF (web).

**Behavior**

1. In one transaction, compare-and-set `users.deleted_at`, revoke all app and
   known WorkOS sessions for `(tenant_id, user_id)`, set
   `cancel_requested_at` on this user's non-purge work selected by
   `subject_user_id`, and enqueue exactly one
   `work_items.kind='account.purge'` with `subject_user_id=user_id` and
   `dedupe_key=user_id.to_string()`. The private payload is
   `{ user_id, deleted_at, local_purged_at?, provider_purged_at? }`; never store
   the WorkOS user ID in queue JSON or logs.
2. Clear auth/pre-auth cookies and return immediately. Invalid, expired,
   revoked, and tombstoned-user sessions can no longer refresh. Login/exchange
   must reject a tombstoned WorkOS user while its purge exists; never recreate
   the account or a second personal tenant.
3. `account.purge` is a checkpointed, idempotent workflow. First run the local
   purge hook, then call the WorkOS delete-user API. Plan 003's baseline local
   hook deletes sessions/profile data and scrubs email, display name, avatar,
   and greeting, while retaining only a minimal user tombstone
   `{ tenant_id, id, workos_user_id, deleted_at }` plus the purge item. That
   provider identifier is retained solely so retries and login rejection remain
   possible. Provider `404/not_found` counts as success. Provider/network
   failure retries with jittered backoff capped at one hour without restoring
   local data. Use `max_attempts=100`; a terminal failure must emit the
   account-purge alarm and remain manually requeueable rather than disappearing.
4. Before archive upload ships, Plan 004 extends the local hook in this strict
   order: durable external deletion-ledger receipt → delete every pinned object
   version → delete normalized/staging rows → auth/profile cascade. Persist
   each checkpoint transactionally. A retry skips only checkpoints whose
   durable postcondition is re-verified; do not create a second purge job kind.
5. Mark success only when local and provider checkpoints both hold. When Plan
   004 is present, durably write its provider-complete companion receipt before
   deleting the minimal user tombstone. Then scrub the work-item payload and
   let a tombstone sweeper delete the succeeded item plus now-empty personal
   tenant. Alert on any local purge older than 24 hours or provider deletion
   beyond its retry SLO.

**Response**: `202 Accepted`

```json
{ "status": "deletion_scheduled", "purgeDeadline": "2026-07-24T08:22:00Z" }
```

## Cookie reference (web)

| Name | Purpose | Flags |
|------|---------|-------|
| `gp_auth_init` | Client-bound exchange secret (distinct from PKCE/state) | Host-only (omit `Domain`), HttpOnly, Secure, SameSite=Lax, Path=/v1/auth, Max-Age=600 |
| `gp_session` | Opaque app session | Host-only (omit `Domain`), HttpOnly, Secure, SameSite=Lax, Path=/, Max-Age configurable |

Never store CSRF in a cookie; use header `X-CSRF-Token` only.

## Key rotation

### App session keys (`APP_SESSION_KEYS`)

- JSON array ordered newest-first.
- New sessions use `keys[0]`; verification tries all entries.
- Rotation procedure:
  1. Append new key to front of JSON env var.
  2. Deploy; new sessions use new key.
  3. After max session TTL elapsed, remove old key entry.
- Integration test `session_key_rotation` must prove an old-key session still validates until revoked while new logins use the new key.

### WorkOS seal password

- `seal_key_version` column supports `workos_v1`, `workos_v2`, …
- Admin CLI: `ghostpost-backend auth reseal-workos-sessions` re-encrypts active `workos_sessions` before the old password is retired; account-purge items contain no provider secret or ciphertext.
- STOP if re-seal cannot run idempotently.

## Middleware layout

Implement in `backend/src/auth/`:

- `session_extractor` — cookie or bearer → `AppSession`
- `csrf_guard` — web mutating methods only
- `origin_guard` — exact match against `AUTH_WEB_ORIGINS`
- `fetch_metadata_guard` — rejects `cross-site` navigations and `navigate` mode
- `tenant_guard` — injects `TenantContext { tenant_id, user_id }`

Apply guards to all `/v1/*` routes except:

- `GET /v1/auth/authorize`
- `POST /v1/auth/exchange`
- `POST /v1/webhooks/workos`
- `GET /v1/platforms`
- `GET /health/*`

## Steps

### Step 0: Drift check

Run drift command from header. Confirm Plan 002 backend exists.

```bash
test -f backend/Cargo.toml
test -d backend/migrations
rg -n 'health/live|health/ready|migrate' backend/src
```

**Verify**: kernel health routes present; no unexpected auth code already landed with forbidden routes (`POST /v1/auth/authorize`, `GET /v1/auth/callback`, `POST /v1/auth/callback`, `/v1/lifecycle`, bare `/v1/onboarding`, `/v1/platforms/catalog`, `/v1/account`).

### Step 1: Add migrations

Add migration file extending Plan 002 tables per schema section. Use
`ADD COLUMN IF NOT EXISTS` / `CREATE TABLE IF NOT EXISTS` only.

```bash
cargo run --locked --manifest-path backend/Cargo.toml -- migrate
```

**Verify**:

```bash
psql "$DATABASE_URL" -c "\dt"
cargo run --locked --manifest-path backend/Cargo.toml -- migrate
```

Expected: `workos_sessions`, `auth_flows`, `webhook_events` present; Plan 002 tables unchanged in shape; second migrate exit 0 (idempotent).

### Step 2: Config + crypto primitives

Implement env parsing and helpers:

- `seal_bytes(plaintext) -> Vec<u8>` / `open_bytes`
- `hash_token(raw) -> [u8; 32]` (SHA-256)
- `constant_time_eq`

Add unit tests under `backend/src/auth/crypto.rs` (or equivalent).

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml crypto::
```

Expected: seal/open roundtrip passes; wrong password fails.

### Step 3: Authorize + exchange

Implement a `WorkosIdentityProvider` adapter with the official WorkOS Rust SDK
for authorization URL generation, code exchange, user/session operations, and
`WebhookVerifier::construct_event`. Do not hand-roll provider HTTP when the SDK
supports the operation. If one required endpoint is absent from the pinned
SDK, isolate only that call behind the same adapter using the documented WorkOS
API, add a contract test, and record the exact SDK gap; never expose WorkOS
types outside the adapter.

**Verify**:

```bash
curl -s -c /tmp/gp-auth-jar.txt 'http://127.0.0.1:8080/v1/auth/authorize?client=web' | jq -e '.authorizationUrl and .state'
# expect 200 JSON + Set-Cookie gp_auth_init (check -c jar or -i once)

curl -s 'http://127.0.0.1:8080/v1/auth/authorize?client=native' | jq -e '.authorizationUrl and .state and .exchangeSecret'
# expect 200 JSON with exchangeSecret; must NOT 302
```

Complete manual exchange with WorkOS staging credentials: frontend receives WorkOS redirect, then `POST /v1/auth/exchange` completes session.

### Step 4: Refresh rotation and CSRF

Wire rotation and revocation.

**Verify**:

```bash
JAR=/tmp/gp-auth-jar.txt
ORIGIN='http://localhost:8081'
# after frontend callback exchanges the WorkOS code into this cookie jar:
TOKEN=$(curl -s -b "$JAR" -c "$JAR" "http://127.0.0.1:8080/v1/auth/csrf" | jq -r .token)
curl -s -o /dev/null -w '%{http_code}' -b "$JAR" -c "$JAR" \
  -X POST http://127.0.0.1:8080/v1/auth/refresh \
  -H "X-CSRF-Token: $TOKEN" -H "Origin: $ORIGIN"
# expect 204; old cookie and old CSRF token are unusable
TOKEN=$(curl -s -b "$JAR" -c "$JAR" "http://127.0.0.1:8080/v1/auth/csrf" | jq -r .token)
```

### Step 5: `/me`, onboarding, platforms

**Verify**:

```bash
STATE=$(curl -s -b "$JAR" http://127.0.0.1:8080/v1/me/onboarding)
echo "$STATE" | jq -e '.revision >= 0 and .status and .answers'
BODY=$(echo "$STATE" | jq -c '
  .status = "completed"
  | .currentStep = 4
  | .answers.comingUp = ["rush"]
  | .answers.concerns = ["public_image"]
  | .answers.platforms = ["x"]
  | .answers.disclosureConsent.accepted = true')
curl -s -b "$JAR" -X PUT http://127.0.0.1:8080/v1/me/onboarding \
  -H 'Content-Type: application/json' \
  -H "Origin: $ORIGIN" \
  -H "X-CSRF-Token: $TOKEN" \
  -d "$BODY" | jq -e '.status == "completed" and .revision > 0'
curl -s -b "$JAR" http://127.0.0.1:8080/v1/me \
  | jq -e '.user.id and .user.greetingName and (has("lifecycle") | not)'
curl -s http://127.0.0.1:8080/v1/platforms \
  | jq -e '(.platforms | length == 5) and ([.platforms[] | select(.archiveEnabled)] | map(.id) == ["reddit","x"])'
```

Submitting the stale pre-update `STATE` again must return `409`; omitting or
declining current disclosure consent while claiming `status=completed` must
return `422`.

### Step 6: Webhook + account deletion

**Verify**: WorkOS webhook test vector passes signature check; duplicate `event_id` returns `200` without double mutation.

```bash
curl -s -o /dev/null -w '%{http_code}' -X DELETE http://127.0.0.1:8080/v1/me \
  -b "$JAR" -H "X-CSRF-Token: $TOKEN" -H "Origin: $ORIGIN"
# expect 202
curl -s -o /dev/null -w '%{http_code}' -b "$JAR" http://127.0.0.1:8080/v1/me
# expect 401
```

### Step 7: Integration tests — two cookie jars

Add `backend/tests/two_cookie_jar.rs`:

1. Spin up test app + DB (follow Plan 002 integration harness).
2. Start two authorize flows in parallel cookie jars **A** and **B** (web client).
3. Complete WorkOS exchange simulation for jar **A** only via `POST /v1/auth/exchange` (test helper mocks WorkOS token exchange).
4. Assert jar **B** `GET /v1/me` → `401`.
5. Assert jar **A** `GET /v1/me` → `200`.
6. Assert jar **A** mutating request with wrong `Origin` → `403`.
7. Logout jar **A**; subsequent `/v1/me` → `401`.
8. Replay consumed `state` at `POST /v1/auth/exchange` → `410`.

```bash
cargo test --locked --manifest-path backend/Cargo.toml two_cookie_jar -- --nocapture
```

**Verify**: exit 0.

### Step 8: Integration tests — two native contexts

Add `backend/tests/two_native_context.rs`:

1. `GET /v1/auth/authorize?client=native` twice → distinct `state` values and distinct `exchangeSecret` values.
2. Complete exchange for flow 1; obtain `code`/`state`.
3. `POST /v1/auth/exchange` with flow-1 `state` but flow-2 `exchangeSecret` → `403`.
4. `POST /v1/auth/exchange` with flow-1 `state` and flow-1 `exchangeSecret` → `200` + bearer token **T1**.
5. Start flow 2; assert **T1** cannot consume flow 2 `state`/secret.
6. Bearer **T1** accesses only user 1 tenant rows.

```bash
cargo test --locked --manifest-path backend/Cargo.toml two_native_context -- --nocapture
```

**Verify**: exit 0.

### Step 9: Key rotation + CSRF tests

Add:

- `backend/tests/session_key_rotation.rs` — session minted under key `k1` validates after adding `k2` to front; new login uses `k2`.
- `backend/tests/csrf_rotation.rs` — old CSRF rejected after `GET /v1/auth/csrf` or refresh.

```bash
cargo test --locked --manifest-path backend/Cargo.toml --test session_key_rotation --test csrf_rotation
```

**Verify**: exit 0.

### Step 10: Readiness isolation

With WorkOS credentials invalid or network blocked:

```bash
curl -fsS http://127.0.0.1:8080/health/ready
```

**Verify**: `200` when Postgres reachable; no outbound WorkOS call in logs for ready handler.

### Step 11: Extend `.env.example`

Add auth env keys documented above with empty values for secrets.

```bash
rg 'WORKOS_|APP_SESSION_KEYS|AUTH_' backend/.env.example
```

**Verify**: all required auth env names present; redirect URIs are frontend callbacks.

### Step 12: Local test harness for Plan 008 smoke script

Document (in test module README comment or `backend/tests/auth/README.md` **only if** Plan 002 already uses test README pattern; otherwise inline module docs):

- How `scripts/local-upload-smoke.sh` obtains a session: run `cargo test auth::local_session_fixture -- --ignored` or export `GP_TEST_SESSION=…` from documented curl flow (`POST /v1/auth/exchange`).
- Ignored test may mint a short-lived session when `WORKOS_*` env present.

**Verify**: `./scripts/local-upload-smoke.sh` STOP message references this harness when auth env missing (once Plan 008 script exists).

### Step 13: Final scope hygiene

```bash
git diff --stat 3625ed1 -- app/
git status
```

**Verify**: no `app/**` changes; only in-scope backend paths modified.

## Test plan

- **Unit**: seal/open, hash/compare, DTO validation, enum parsing, redirect allowlist parsing.
- **Integration — two cookie jars**: web session isolation + CSRF + Origin + Fetch Metadata enforcement.
- **Integration — two native contexts**: exchange secret binding + bearer isolation across parallel flows.
- **Integration — CSRF rotation**: old token rejected after refresh or new `GET /v1/auth/csrf`.
- **Integration — session key rotation**: old key verifies existing sessions; new sessions use new key.
- **Integration — onboarding PUT**: full snapshot required; stale revision → `409`; missing/current-version consent or disabled platform → `422`; success increments revision and GET matches.
- **Integration — webhook idempotency**: duplicate `event_id` safe.
- **Integration — account deletion**: sessions revoked; `/v1/me` → `401`; deleted user cannot refresh.
- **Integration — one-use state/PKCE**: replay `POST /v1/auth/exchange` → `410`.
- **Integration — authorize JSON**: web and native both return `200` JSON (never `302` from authorize handler).
- **Manual**: WorkOS staging login web + native happy path: authorize JSON → frontend redirect → `POST /v1/auth/exchange`.
- **Integration — exchange DTOs**: web success is `204` + cookie with no body; native success contains only `{ user, session: { kind, token, expiresAt } }`; web body `exchangeSecret` is rejected.
- **Integration — profile projection**: `GET /v1/me` has `user` only and no lifecycle/onboarding/entitlement keys.

Verification summary:

```bash
cargo test --locked --manifest-path backend/Cargo.toml auth::
cargo test --locked --manifest-path backend/Cargo.toml two_cookie_jar
cargo test --locked --manifest-path backend/Cargo.toml two_native_context
cargo test --locked --manifest-path backend/Cargo.toml --test session_key_rotation --test csrf_rotation
curl -fsS http://127.0.0.1:8080/health/ready
```

Expected: all exit 0.

## Done criteria

Machine-checkable. ALL must hold:

- [ ] Migrations applied idempotently; Plan 002 tables extended (not replaced); auth-only tables match schema section
- [ ] `GET /v1/auth/authorize?client=web|native` returns **200 JSON** with `authorizationUrl` + `state`; server stores PKCE verifier; never `302`
- [ ] Web authorize sets `gp_auth_init` cookie; native authorize JSON includes one-time `exchangeSecret` (distinct from PKCE and `state`)
- [ ] **No** `POST /v1/auth/authorize`, `GET /v1/auth/callback`, `POST /v1/auth/callback`, `GET /v1/lifecycle`, bare `/v1/onboarding`, `/v1/platforms/catalog`, or `DELETE /v1/account` routes registered
- [ ] One-use `state`/PKCE enforced; replay at `POST /v1/auth/exchange` returns `410`
- [ ] `POST /v1/auth/exchange` validates `code`, `state`, `client`, and exchange proof (cookie or `exchangeSecret`)
- [ ] WorkOS session sealed at rest in `workos_sessions`; app session stored as SHA-256 hash in `auth_sessions` only
- [ ] Web exchange is `204` + HttpOnly `gp_session` and no body; native exchange returns only `{ user, session: { kind: "bearer", token, expiresAt } }`
- [ ] Refresh atomically rotates the app session; web returns `204` + cookie and native returns `{ session: { kind, token, expiresAt } }`
- [ ] `POST /v1/auth/refresh`, `POST /v1/auth/logout`, `GET /v1/auth/csrf` implemented with exact status/cookies above
- [ ] `GET /v1/me`, `GET|PUT /v1/me/onboarding`, `GET /v1/platforms`, `POST /v1/webhooks/workos`, `DELETE /v1/me` implemented with exact DTOs/status codes above
- [ ] Onboarding uses the full `{ status, currentStep, revision, answers }` snapshot; stale revisions return `409`; no `completed` boolean or lifecycle projection exists
- [ ] `GET /v1/me` returns the user profile only; client lifecycle is derived from separate resources
- [ ] Web mutating routes reject missing/wrong CSRF, Origin, or Fetch Metadata
- [ ] `two_cookie_jar` and `two_native_context` integration tests pass
- [ ] `session_key_rotation` and `csrf_rotation` tests pass
- [ ] `/health/ready` does not call WorkOS
- [ ] `TenantContext { tenant_id, user_id }` extractor enforces composite tenant scoping on auth-owned tables
- [ ] No `app/**` files modified
- [ ] `backend/.env.example` lists auth env names with frontend redirect URIs
- [ ] `plans/README.md` status row updated if you own the index

## STOP conditions

Stop and report back (do not improvise) if:

- Plan 002 backend kernel is missing or uses non-additive migration patterns that conflict with this schema.
- Drift check shows frontend types changed such that platform/onboarding enums no longer match excerpts — reconcile with operator before inventing new ids.
- WorkOS staging app cannot be configured with exact **frontend** redirect URIs from env (no wildcard workaround; no backend OAuth callback route).
- Implementing exchange proof appears to require storing raw app session tokens or PKCE verifiers in plaintext in Postgres.
- `/health/ready` would need WorkOS to return `200`.
- Any step verification fails twice after a reasonable fix attempt.
- Tests cannot isolate two cookie jars or two native contexts without disabling CSRF, Origin, or exchange guards.
- You are asked to modify `app/**` for cutover, weaken PKCE/Origin/CSRF for local dev, or commit real secrets.
- WorkOS webhook signature verification cannot be implemented without disabling body parsing — fix parser, do not skip verification.
- Account deletion cannot revoke sessions without touching Plan 004 archive tables that do not exist yet — limit to auth-owned tables and document FK contract for Plan 004 instead of inventing purge logic.
- WorkOS API surface differs materially from AuthKit authorize/code exchange documented here — stop and update this plan with operator approval rather than inventing a parallel auth stack.
- Migration would recreate Plan 002 `users`/`auth_sessions`/`onboarding_profiles` instead of altering in place.
- Authorize handler would need to `302` redirect instead of returning JSON — stop; JSON authorize is canonical.

## Maintenance notes

- Plan 007 switches Expo to `/v1/*`, SecureStore bearer, credentialed web fetches, frontend `/auth/callback` routes, and `POST /v1/auth/exchange`; legacy unversioned `/onboarding` remains unimplemented.
- Plan 008 Compose must inject `WORKOS_*`, `APP_SESSION_KEYS`, and `AUTH_*` values with **frontend** redirect URIs; never weaken auth for Compose convenience.
- Plan 004+ tables must use Plan 002 composite `(tenant_id, …)` FK pattern and `TenantContext` extractor from this plan.
- Reviewers should scrutinize: hash-only session storage, one-use flows, CSRF rotation, exact Origin list, readiness isolation, JSON authorize + frontend-only WorkOS redirects, and cross-session integration tests.
- Deferred: frontend auth UX, legacy route shims, rate limiting at edge (Plan 009), social-login provider expansion beyond WorkOS AuthKit defaults.
