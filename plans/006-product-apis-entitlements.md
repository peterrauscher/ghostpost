# Plan 006: Product APIs, entitlements, and local review dispositions

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md plans/004-archive-upload-ingestion.md plans/005-scan-prompt-deepseek-evals.md app/src/domain/types.ts app/src/services/api/`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: MED
- **Depends on**: plans/002-backend-kernel-schema.md, plans/003-workos-auth-profiles-onboarding.md, plans/004-archive-upload-ingestion.md, plans/005-scan-prompt-deepseek-evals.md
- **Category**: feature
- **Planned at**: commit `3625ed1`, 2026-07-23
- **Canonical inputs**: the API, backend, security, and product contracts reproduced in this document

## Why this matters

Plans 003–005 land auth, archive ingestion, and the scan worker (DeepSeek `scan-v1`), but the Expo app still uses mock routes with client-side unlock, demo reset, and dishonest risk when zero flags exist. ProductFlowReview requires **server-truth** product APIs: create a scan and poll **`GET /v1/scans/{id}`** until a **terminal** `status`, an honest dashboard whose overall risk is **`none`** when there are zero open flags, **local-only** review dispositions (including **`delete_local`** with **no outbound network**), read-only entitlement via **`GET /v1/me/entitlement`**, and a trusted server-issued **`free_beta`** policy (**platform limit 2**, **rescans enabled**). Billing unlock, grant/demo reset, source-account listing, model-run exposure, scan cancel, and social mutation are explicitly forbidden. Plan 007 maps these contracts onto the Expo HTTP adapter; this plan implements the backend routes, entitlement resolver, retention purge workers, and focused integration tests only.

## Current state

Facts at `3625ed1` plus expected artifacts from Plans 002–005:

### Repository

- `app/` — Expo SDK 57; mock API by default (`app/src/services/api/mock.ts`, `http.ts`).
- Plan 002 creates kernel + schema (`scans`, `flagged_posts`, `review_actions`, `entitlement_grants`, `entitlement_usages`, `work_items`, idempotency).
- Plan 003 adds WorkOS auth, `GET /v1/me`, onboarding persistence, tenant isolation middleware.
- Plan 004 adds archive import routes and normalized `content_items`.
- Plan 005 adds scan worker, `model_attempts`, flag persistence from validated LLM output — **no public model-run routes**.

### App DTOs (Plan 007 extends; do not edit in this plan)

```ts
// app/src/domain/types.ts — compatibility targets (camelCase JSON at wire)
export type ReviewAction = 'delete' | 'archive' | 'keep' | 'resolve'; // Plan 007 adds delete_local mapping
export type ScanPhase = 'connecting' | 'scanning' | 'flagging' | 'complete';
export type RiskLevel = 'high' | 'medium' | 'low'; // dashboard adds overall `none`

export interface ScanStatus {
  phase: ScanPhase;
  progress: number;
  message: string;
  // Plan 006 wire adds: id, status, errorCode?, createdAt?, finishedAt?
}

export interface FlaggedPost {
  id: string;
  platform: PlatformId;
  platformLabel: string;
  date: string;
  quote: string;
  risk: RiskLevel;
  category: string;
  tags: string[];
  engagementLabel?: string; // optional when ingest lacks metrics
  likes?: number;
  comments?: number;
  explanation: string;
  whyFlagged: string;
  status: 'open' | 'resolved' | 'deleted' | 'archived' | 'kept';
}
```

### Legacy mock paths (must NOT be implemented)

```ts
// app/src/services/api/http.ts — Plan 007 retargets; Plan 006 must not add these
POST /scan, GET /scan, GET /dashboard, GET /review, GET /posts/:id,
POST /posts/:id/actions, POST /home/unlock, POST /demo/reset
```

### Canonical constraints (review artifacts)

| Source | Requirement |
|--------|-------------|
| ProductFlowReview | Poll created scan **by id**; terminal `failed` must not open next gate; `free_beta` server policy; honest **`none`** dashboard risk |
| SecurityReview | Tenant wrong-id → **404**; idempotency on mutations; no social HTTP clients |
| DesignIntegrator | No unlock/grant/billing routes; local dispositions only |
| DataModelApi | Exact `/v1` routes/DTOs below; retention/purge workers; optional engagement fields |

### Module layout

Implement handlers under existing kernel modules — **do not** create a parallel router tree:

| File | Routes |
|------|--------|
| `backend/src/api/scans.rs` | `POST /v1/scans`, `GET /v1/scans/current`, `GET /v1/scans/{id}` |
| `backend/src/api/profile.rs` | `GET /v1/dashboard`, `GET /v1/me/entitlement` |
| `backend/src/api/flags.rs` | `GET /v1/flags`, `GET /v1/flags/{id}`, `POST /v1/flags/{id}/review-actions` |
| `backend/src/jobs/handlers/purge_*.rs` | Retention/purge work kinds (no HTTP) |
| `backend/src/domain/entitlements/service.rs` | `require_review_access`, `consume_rescan`, `resolve_effective_grant` |

Wire routes once in `backend/src/api/router.rs`.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Drift check | `git diff --stat 3625ed1..HEAD -- backend/ plans/003-workos-auth-profiles-onboarding.md plans/004-archive-upload-ingestion.md plans/005-scan-prompt-deepseek-evals.md` | Review only |
| Prereq auth | `rg -n 'require_auth|/v1/me' backend/src/api` | Plan 003 middleware present |
| Prereq ingest | `rg -n 'archive_imports|/v1/archive-imports' backend/src` | Plan 004 present |
| Prereq scan worker | `rg -n 'scan_posts|ScanProvider|flagged_posts' backend/src` | Plan 005 present |
| Migrate | `cargo run --locked --manifest-path backend/Cargo.toml -- migrate` | exit 0, idempotent on repeat |
| Product tests | `cargo test --locked --manifest-path backend/Cargo.toml product_` | exit 0 |
| free_beta tests | `cargo test --locked --manifest-path backend/Cargo.toml free_beta` | exit 0 |
| Guessed routes | `cargo test --locked --manifest-path backend/Cargo.toml guessed_route` | all 404/405 |
| delete_local network | `cargo test --locked --manifest-path backend/Cargo.toml delete_local_no_network` | exit 0, zero outbound HTTP |
| Idempotency | `cargo test --locked --manifest-path backend/Cargo.toml idempotency` | exit 0 |

Do **not** run repo-wide formatters, clippy fix sweeps, or `app/` typecheck as gates for this plan.

## Suggested executor toolkit

- Rust stable ≥ 1.80 (MSRV recorded in Plan 002 `backend/Cargo.toml`).
- Disposable Postgres (Plan 008 Compose or local).
- Plan 003 test session factory (cookie + bearer + CSRF).
- Network guard test double (panic on any outbound HTTP during `delete_local` tests).

## Scope

**In scope**:

- `backend/migrations/*_product_apis_free_beta.sql` — additive indexes/constraints on Plan 002 `entitlement_grants` / `entitlement_usages` only if missing; **do not duplicate tables**.
- `backend/src/api/scans.rs`, `flags.rs`, extend `profile.rs` — handlers only for routes listed below.
- `backend/src/domain/entitlements/` — resolver + `free_beta` policy (`ENTITLEMENT_POLICY=free_beta` default in dev).
- `backend/src/repository/{scans,flags,entitlements}.rs` — tenant-scoped SQLx using Plan 002 composite-FK pattern — **do not duplicate tables**.
- `backend/src/jobs/handlers/purge_content.rs`, `purge_retention.rs` — enqueue from `delete_local` and scheduled retention.
- `backend/tests/product_*.rs`, `guessed_route_404_test.rs`, `delete_local_no_network_test.rs`, `free_beta_test.rs`.

**Out of scope**:

- `app/**` (Plan 007).
- Auth routes (Plan 003), archive routes (Plan 004), DeepSeek/prompt/evals (Plan 005).
- `GET /v1/source-accounts/**`, `POST /v1/scans/{id}/cancel`, `GET /v1/scans` list (optional later), model-run admin APIs.
- Billing, `POST /home/unlock`, `POST /v1/entitlement/grant`, `POST /demo/reset`, social platform clients.
- Docker/SST (Plans 008–009).

## Git workflow

- Branch: `advisor/006-product-apis-entitlements`
- Commits: migration → entitlement service → scan/dashboard routes → flags/review-actions → purge workers → tests.
- Do NOT push unless instructed.

---

## API contract (exact)

**Global rules**

- JSON **camelCase** (matches Expo DTOs).
- Protected routes: Plan 003 session extractor (401 if missing/revoked).
- Mutations: `Idempotency-Key` header (1–128 printable ASCII); web cookie auth also requires CSRF per Plan 003.
- Errors: `application/problem+json` with stable `code` (never leak storage keys, model output, or tokens).
- Owner scope: every lookup `WHERE id = $1 AND tenant_id = $session_tenant` (or `user_id` if that is the live convention). Wrong tenant → **404 `RESOURCE_NOT_FOUND`**.

### Scan DTOs

**ScanStatusResponse** (compatibility + durable fields):

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "running",
  "phase": "scanning",
  "progress": 0.42,
  "message": "scanning...",
  "errorCode": null,
  "createdAt": "2026-07-23T12:00:00Z",
  "finishedAt": null
}
```

| Field | Rules |
|-------|-------|
| `status` | `queued` \| `running` \| `succeeded` \| `failed` \| `cancelled` — polling stops on any terminal value |
| `phase` | `connecting` \| `scanning` \| `flagging` \| `complete` — **only** `succeeded` may project `phase=complete` and `progress=1` |
| `progress` | [0, 1]; monotonic non-decreasing while `running` |
| `message` | Safe localized string; table-driven keys matching mock phases (`fixtures.ts:171-182`) |
| `errorCode` | Non-null only when `status=failed`; short snake string (e.g. `provider_timeout`) |
| `finishedAt` | Non-null iff terminal |

**CreateScanRequest**:

```json
{ "archiveImportIds": ["uuid", "uuid"] }
```

`archiveImportIds` is required, contains 1–`platformLimit` unique IDs, and must
name exactly the tenant-owned terminal `ready` imports the user chose. Omitted
or empty arrays return `422`; the backend never silently scans other imports.

### Dashboard DTO

**DashboardResponse** (current `DashboardData` shape + honest none-risk):

```json
{
  "user": { "id": "…", "name": "Jordan Lee", "greetingName": "jordan", "avatarUrl": null },
  "auditHeadline": "no flagged posts found",
  "focusAreas": [{ "id": "colleges", "label": "colleges", "symbol": "🎓" }],
  "flaggedPreview": [],
  "risk": { "level": "none", "flaggedCount": 0, "gaugeSweep": 0 }
}
```

| Rule | Detail |
|------|--------|
| Overall `risk.level` | **`none`** iff open flag count for selected scan is 0; else highest open flag risk (`high` > `medium` > `low`) |
| `gaugeSweep` | **0** when `level=none`; else deterministic map: high=320, medium=245, low=180 (match mock medium sweep) |
| `flaggedPreview` | Max 3 open flags, highest risk first; empty when no open flags |
| Headline | Table-driven; must not imply hidden flags when count is 0 |

Optional query: `GET /v1/dashboard?scanId=<uuid>` — default latest **succeeded** scan owned by tenant.

### Flag DTOs

**FlaggedPostResponse** — maps DB rows to flat Expo shape; engagement fields **optional**:

```json
{
  "id": "660e8400-e29b-41d4-a716-446655440001",
  "platform": "x",
  "platformLabel": "X",
  "date": "Mar 12, 2024",
  "quote": "exact excerpt…",
  "risk": "high",
  "category": "inappropriate_language",
  "tags": ["targeted_insult"],
  "explanation": "Direct insult toward a group of people.",
  "whyFlagged": "Matches your inappropriate-language review concern.",
  "status": "open"
}
```

Plan 004 intentionally does not ingest likes/votes/comment counts. Omit
`engagementLabel`, `likes`, and `comments` at launch; never add columns or
fabricate zeroes to preserve the mock.

**ReviewListResponse**:

```json
{
  "posts": [],
  "filters": { "all": 5, "high": 2, "medium": 2, "low": 1 },
  "nextCursor": null,
  "hasMore": false
}
```

Query: `GET /v1/flags?scanId=&risk=all|high|medium|low&status=open&cursor=&limit=` (default scan = latest succeeded; default risk = all; default status = open).

**ReviewActionRequest**:

```json
{ "action": "delete_local", "expectedStatus": "open" }
```

Allowed `action`: `resolve` | `delete_local` | `archive` | `keep` only. **No** API action named `delete` (Plan 007 maps UI “Delete Ghostpost copy” → `delete_local`).

### Entitlement DTO (read-only)

**EntitlementResponse** — `GET /v1/me/entitlement?scanId=<optional>`:

```json
{
  "status": "active",
  "productId": "free_beta",
  "validFrom": "2026-07-23T12:00:00Z",
  "expiresAt": null,
  "scanId": null,
  "capabilities": {
    "reviewAccess": true,
    "rescansRemaining": null,
    "platformLimit": 2
  }
}
```

| Rule | Detail |
|------|--------|
| `productId` | **`free_beta`** for launch cohort |
| `platformLimit` | **2** (distinct Reddit/X ready imports per scan create) |
| `rescansRemaining` | **`null`** = unlimited rescans while active (ProductFlowReview) |
| `reviewAccess` | `true` permits dashboard/flag reads; Expo derives its route gate from this capability, never a dashboard `unlocked` field |
| Issuance | Server-only: on first authenticated profile upsert (Plan 003 hook) insert grant with `grantSource=promotion`, **never** from client POST |
| Env | `ENTITLEMENT_POLICY=free_beta` (default in dev/test); no public grant endpoint |

---

## Routes (exact)

| Method | Path | Auth | Success | Body |
|--------|------|------|---------|------|
| `POST` | `/v1/scans` | session + CSRF(web) + Idempotency-Key | **202** ScanStatusResponse | CreateScanRequest optional |
| `GET` | `/v1/scans/current` | session | **200** ScanStatusResponse or **404** | — |
| `GET` | `/v1/scans/{id}` | session | **200** ScanStatusResponse | Pure ID poll |
| `GET` | `/v1/dashboard` | session | **200** DashboardResponse | `?scanId=` optional |
| `GET` | `/v1/flags` | session + review entitlement | **200** ReviewListResponse | filters/cursor |
| `GET` | `/v1/flags/{id}` | session + entitlement | **200** FlaggedPostResponse | — |
| `POST` | `/v1/flags/{id}/review-actions` | session + CSRF(web) + Idempotency-Key | **200** FlaggedPostResponse | ReviewActionRequest |
| `GET` | `/v1/me/entitlement` | session | **200** EntitlementResponse | `?scanId=` optional |

### Status codes (per route)

**POST /v1/scans**

| Code | Condition |
|------|-----------|
| **202** | Scan queued; body `status=queued`, `phase=connecting`, `progress=0` |
| **401** | Unauthenticated |
| **403** | `ENTITLEMENT_REQUIRED` — rescan quota/policy denial |
| **404** | Any `archiveImportId` not owned by tenant |
| **409** | Active scan exists (`scan_already_running`); idempotency replay mismatch; concurrent quota race |
| **422** | No ready imports; platform limit exceeded; invalid UUID list |

**GET /v1/scans/current**

| Code | Condition |
|------|-----------|
| **200** | Active scan if any, else most recent terminal scan |
| **401** | Unauthenticated |
| **404** | No scans ever for tenant |

**GET /v1/scans/{id}**

| Code | Condition |
|------|-----------|
| **200** | ScanStatusResponse |
| **401** | Unauthenticated |
| **404** | Unknown id or other tenant |

**GET /v1/dashboard**

| Code | Condition |
|------|-----------|
| **200** | DashboardResponse |
| **401** | Unauthenticated |
| **404** | Optional `scanId` not found / wrong tenant |

**GET /v1/flags**

| Code | Condition |
|------|-----------|
| **200** | ReviewListResponse |
| **400** | Invalid cursor |
| **401** | Unauthenticated |
| **403** | No `reviewAccess` entitlement |
| **404** | Unknown `scanId` |
| **422** | Invalid `risk` filter |

**GET /v1/flags/{id}**

| Code | Condition |
|------|-----------|
| **200** | FlaggedPostResponse |
| **401** | Unauthenticated |
| **403** | Entitlement lacks review access for flag's scan |
| **404** | Unknown / wrong tenant / purged after `delete_local` |

**POST /v1/flags/{id}/review-actions**

| Code | Condition |
|------|-----------|
| **200** | Updated FlaggedPostResponse snapshot |
| **400** | Malformed JSON |
| **401** | Unauthenticated |
| **403** | Entitlement denial |
| **404** | Unknown flag / wrong tenant |
| **409** | Terminal flag (`FLAG_ALREADY_REVIEWED`); idempotency replay returns cached **200**; `expectedStatus` mismatch |
| **422** | Unknown `action` |

**GET /v1/me/entitlement**

| Code | Condition |
|------|-----------|
| **200** | EntitlementResponse |
| **401** | Unauthenticated |
| **404** | Optional `scanId` not owned |

### Routes that MUST NOT exist (404 or 405)

Register **no** handlers for:

```
POST /v1/scans/{id}/cancel
GET  /v1/scans/{id}/model-run
GET  /v1/model-runs/**
GET  /v1/source-accounts/**
POST /v1/source-accounts/**
POST /v1/billing/**
POST /v1/home/unlock
POST /home/unlock
POST /v1/entitlement/grant
POST /v1/me/entitlement/grant
POST /v1/demo/reset
POST /demo/reset
POST /v1/posts/{id}/actions
GET  /scan /dashboard /review /posts/**
```

---

## Transactions and idempotency

### POST /v1/scans

Single transaction:

1. Load effective entitlement (`free_beta`); verify rescan allowed (`rescansRemaining` null or > 0).
2. `SELECT archive_imports … FOR UPDATE` — tenant-owned, `status=ready`; enforce distinct platform count ≤ `platformLimit` (2).
3. Partial unique index / lock: reject second `queued|running` scan → **409**.
4. Insert `scans` row (`status=queued`, `phase=connecting`, `progress=0`).
5. Link imports via join table; insert `work_items` kind `scan_posts` with `subject_user_id=session.user_id` (Plan 005 handler).
6. If rescan consumes counted quota, insert `entitlement_usages` (skip for `free_beta` unlimited).
7. Commit; return **202**.

**Idempotency** (`operation=scan_create`): same `(tenant, key, request_hash)` → reload scan by `resource_id`; return original **202** body. Different hash → **409 `IDEMPOTENCY_KEY_REUSED`**. In-flight → **409 `REQUEST_IN_PROGRESS`** + `Retry-After`.

### POST /v1/flags/{id}/review-actions

Single transaction:

1. Idempotency claim (`operation=flag_review_action`).
2. `SELECT flagged_posts … FOR UPDATE` with tenant predicate — **404** if missing.
3. Verify `review_status=open` (and `expectedStatus` if provided).
4. Map action → terminal status: `delete_local`→`deleted`, `archive`→`archived`, `keep`→`kept`, `resolve`→`resolved`.
5. Insert `review_actions` audit row; update flag `review_status`, `closed_at=now()`.
6. If `delete_local`: mark linked `content_items` deletion pending, hide all dependent flags, enqueue `purge_content` with `subject_user_id=session.user_id` — **no HTTP client invocation**.
7. Store idempotency completion; commit; return **200**.

### Tenant 404

Never return **403** merely to hide cross-tenant resource existence on id-based reads. Use **403** only for authenticated capability denial (`ENTITLEMENT_REQUIRED`) on list/flag routes where the resource id is not yet known.

### Retention / purge (async)

| Work kind | Trigger | Behavior |
|-----------|---------|----------|
| `purge_content` | `delete_local` action | Scrub `body_text`, hard-delete content + dependent flags within 24h |
| `purge_raw_archive` | Plan 004 lifecycle | Delegate to existing blob port (do not duplicate) |
| `purge_retention_metadata` | Scheduled sweeper | Remove expired idempotency rows (≥24h), stale session/PKCE per DataModelApi retention table |

Workers run under `--role worker|all`; idempotent; no HTTP routes.

---

## free_beta policy (trusted server issuance)

1. Config: `ENTITLEMENT_POLICY=free_beta` (required in production launch).
2. On user/profile creation (call site in Plan 003 `AuthService` upsert — add hook, do not expose HTTP grant):
   - Insert `entitlement_grants` row: `product_key=free_beta`, `review_access=true`, `platform_limit=2`, `rescan_limit=NULL`, `grant_source=promotion`, `valid_from=now()`, `valid_until=NULL`.
3. `EntitlementService::resolve()` returns active grant for all authenticated users in launch cohort.
4. No Stripe, no `/home/unlock`, no client-supplied product id.

Plan 002 already includes `free_beta` in the `entitlement_grants.product_key` CHECK. Backfill existing users with one `free_beta` grant each if Plan 003 signup predates this migration.

---

## Steps

### Step 0: Drift check

```bash
git rev-parse --short HEAD
git diff --stat 3625ed1..HEAD -- backend/ plans/003-workos-auth-profiles-onboarding.md plans/004-archive-upload-ingestion.md plans/005-scan-prompt-deepseek-evals.md
rg -n 'scans|flagged_posts|entitlement_grants|require_auth' backend/src backend/migrations
```

**Verify**: Plans 003–005 landed. If product tables missing, STOP.

### Step 1: Migration — free_beta + retention indexes

Apply forward migration; backfill grants; hook Plan 003 signup (same transaction as profile insert).

**Verify**: `cargo run --locked --manifest-path backend/Cargo.toml -- migrate` twice → exit 0.

### Step 2: EntitlementService

Implement resolver + `require_review_access` + `consume_rescan` (no-op unlimited for `free_beta`).

**Verify**: `cargo test --locked free_beta` passes.

### Step 3: Scan handlers

Implement create/current/detail; map DB → ScanStatusResponse; enqueue worker.

**Verify**: `cargo test --locked product_scan_poll` — create → poll `{id}` until terminal.

### Step 4: Dashboard handler

Honest `none` risk; entitlement-gated preview; focus areas from onboarding projection.

**Verify**: `cargo test --locked product_dashboard_none_risk` — zero flags → `risk.level=none`, `gaugeSweep=0`.

### Step 5: Flag list/detail/review-actions

Pagination + filters; review-actions transaction; optional engagement omission.

**Verify**: `cargo test --locked product_flags` + `product_disposition_idempotency`.

### Step 6: GET /v1/me/entitlement

Read-only; no POST grant routes registered.

**Verify**: guessed grant paths return 404; entitlement test passes.

### Step 7: Purge workers

Implement handlers; wire into worker registry.

**Verify**: `cargo test --locked purge_retention` + delete_local hides content in integration test.

### Step 8: Negative route + network tests

```bash
cargo test --locked guessed_route
cargo test --locked delete_local_no_network
cargo test --locked product_tenant_404
```

**Verify**: all exit 0.

---

## Test plan

| Test file | Asserts |
|-----------|---------|
| `product_scan_poll_test.rs` | POST → 202 with `id`; GET `/{id}` until `succeeded\|failed`; terminal `succeeded` ⇒ `phase=complete`, `progress=1` |
| `product_dashboard_none_risk_test.rs` | Zero open flags ⇒ `risk.level=none`, empty preview, honest headline |
| `free_beta_test.rs` | Entitlement `productId=free_beta`, `platformLimit=2`, `rescansRemaining=null`; third platform on scan create → 422; rescan → 202 |
| `product_disposition_idempotency_test.rs` | Duplicate Idempotency-Key ⇒ same 200; one audit row |
| `product_tenant_404_test.rs` | Cross-tenant scan/flag GET → 404 |
| `guessed_route_404_test.rs` | cancel, source-accounts, model-runs, unlock, grant, demo, billing, legacy `/posts` → 404/405 |
| `delete_local_no_network_test.rs` | Global HTTP guard sees **zero** requests; action succeeds; content scheduled for purge |
| `product_no_engagement_fields_test.rs` | Launch flag JSON omits engagement fields because Plan 004 does not ingest them |
| `purge_retention_test.rs` | Sweeper idempotent; soft-deleted rows hidden from reads |

Install network guard in test harness:

```rust
// Pseudocode — fail test if any outbound HTTP attempted during review-action handler
static OUTBOUND: AtomicUsize = AtomicUsize::new(0);
// Wrap reqwest client or inject NoopHttpClient into AppState for tests
```

---

## Done criteria

Machine-checkable. **All** must hold:

- [ ] `free_beta` grant seeded server-side; `GET /v1/me/entitlement` read-only
- [ ] Routes: `POST/GET /v1/scans*`, `GET /v1/dashboard`, `GET/POST /v1/flags*`
- [ ] No cancel, source-account, model-run, billing, unlock, grant, demo routes
- [ ] Scan create + ID poll to terminal status in tests
- [ ] Dashboard honest `none` risk when zero open flags
- [ ] `platformLimit=2` enforced; rescans enabled (`free_beta`)
- [ ] Review-actions idempotency + tenant 404 tests pass
- [ ] `delete_local` no-network test passes
- [ ] Purge workers registered and tested
- [ ] No files outside scope (`git status`)
- [ ] `plans/README.md` status row updated if you own the index

---

## STOP conditions

Stop and report (do not improvise) if:

- Plans 003, 004, or 005 artifacts missing or schema incompatible with DataModelApi without a documented merge plan.
- Implementing scan create requires exposing **`POST /v1/scans/{id}/cancel`** or model-run HTTP — STOP; enqueue internal work only.
- ProductFlowReview `free_beta` policy cannot be seeded without a public grant route — report; do **not** add grant endpoint.
- Cross-tenant id access returns **403** instead of **404** on detail routes.
- `delete_local` handler invokes any HTTP client (guard triggers).
- Any guessed unlock/grant/billing route returns 200.
- Dashboard shows non-`none` risk with zero open flags.
- Step verification fails twice after reasonable fix.
- Fix requires `app/**` cutover, billing integration, or social API clients.
- Plan 002 uses different tenant column names — STOP if you would duplicate tables instead of extending repos.

---

## Maintenance notes

- Plan 007: map `http.ts` to these `/v1` paths; require exact ready import IDs,
  add `ScanStatus.id/status`, map UI delete to `delete_local`, consume
  `/v1/me/entitlement` for `reviewAccess`, and remove legacy dashboard
  `unlocked` gating.
- Plan 008 Compose: may smoke scan/entitlement when Plans 005–006 present; still no unlock env vars.
- Reviewers: scrutinize tenant SQL filters, idempotency txns, honest none-risk, absence of social HTTP, 404 policy, free_beta server issuance only.
- Deferred: paid SKUs (`single`, `sevenDay`, `thirtyDay`), Stripe webhooks, scan list pagination UI, scan cancel, source-account UI, real platform deletion.
