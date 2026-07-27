# Plan 004: Archive upload, presigned ingestion, and Reddit/X normalization

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md app/src/domain/types.ts`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: L
- **Risk**: HIGH
- **Depends on**: plans/002-backend-kernel-schema.md, plans/003-workos-auth-profiles-onboarding.md
- **Category**: feature
- **Planned at**: commit `3625ed1`, 2026-07-23
- **Canonical inputs**: the archive, Reddit, X, API, security, and product contracts reproduced in this document

## Why this matters

Plans 002–003 land Postgres, the fenced work queue, and authenticated tenant
isolation, but there is no way to ingest a user's Reddit or X **data-export
ZIP** into normalized `content_items` that Plans 005–006 can scan. The Expo
client today simulates scan progress with fixtures and has **no upload/import
API** (`app/src/services/api/http.ts` is JSON-only).

This plan lands the **canonical archive path**:

1. Authenticated user requests a presigned **direct POST policy** for a
   **private, versioned** object store (never through Axum body streaming).
2. Client streams the ZIP as multipart form data; the object store policy
   enforces the reserved content-length ceiling. Server then **HEAD-verifies**
   exact size/type and **pins an immutable object version ID** before queueing.
3. Worker **range-reads** the ZIP central directory, opens only an **exact
   allowlist** of text entries, streams Reddit CSV / X JS-wrapped JSON through
   platform parsers into **import-scoped staging**, then **atomically commits**
   canonical minimal records into **`content_items`** and sets import
   **`status=ready`**.
4. Raw objects are **purged immediately** on terminal outcomes with a **24-hour
   lifecycle backstop**.

**Hard boundary (non-negotiable)**: import commit **never** enqueues scan work,
**never** invokes an LLM, and **never** builds provider batch payloads.
`POST /v1/scans` (Plan 006) owns scan creation and `scan_posts` enqueue; Plan
005 owns Unicode segmentation, provider batching, and prompt assembly from a
**stable `content_items` snapshot**.

Without this plan, scan prompts would have no real normalized input, product
APIs would lie about platform coverage, and SecurityReview blockers (version
pinning, tenant-keyed fingerprints, memory-bounded X indexes, provenance-backed
fixtures) remain unresolved.

**Explicit non-goals**: live Reddit/X OAuth, scraping, outbound HTTP to
permalinks, media vision, likes/votes/messages ingestion, scan routes/workers
(Plans 005–006), frontend cutover (Plan 007), SST bucket provisioning (Plan
009 — Plan 004 defines the **application contract** Compose/009 must satisfy).

## Canonical sources

Honor these artifacts (read before implementing; do not contradict them):

| Source | Use |
|--------|-----|
| Archive-ingestion review | Shared ZIP limits, allowlist, canonical record schema, staging/commit/idempotency, fixture matrix, state machine |
| Reddit export research | GDPR CSV inventory, confirmed `comments.csv` header, provisional `posts.csv` schema, sentinels, zero-inference rules |
| X export research | Modern/classic layouts, JS wrapper unwrap, classification priority, Note join, edit chains, deleted tombstones, excluded files |
| API/data-model review | `/v1/archive-imports*` route shapes, idempotency, tenant 404 semantics, `archive_imports` lifecycle columns |
| Backend architecture review | `BlobStore` port location, worker fencing, `work_items.kind` for **import-only** jobs |
| DesignIntegrator + SecurityReview + ProductFlowReview | Version pin after complete, tenant HMAC not raw archive hash, disk-backed bounded indexes, real redacted fixtures per enabled family, honest product copy constraints (frontend migration deferred to Plan 007) |
| `plans/008-containerize-local-stack.md` | Env names: `ARCHIVE_BUCKET`, `ARCHIVE_S3_*`, MinIO versioning smoke expectations |
| `plans/005-scan-prompt-deepseek-evals.md` | Reads stable `content_items`; owns provider projection, Unicode segmentation, batching — **not implemented here** |
| `plans/006-product-apis-entitlements.md` | `POST /v1/scans` enqueues `scan_posts` against **ready** imports — **not implemented here** |

## Current state

Facts at `3625ed1` (after Plans 002–003 land):

- **No** `backend/src/import/` or `backend/src/blob/` yet.
- Plan 002 creates `archive_imports`, `content_items`, `work_items` tables and
  repositories with composite `(tenant_id, id)` FKs — extend, do not fork.
- Plan 003 provides authenticated session + CSRF + idempotency middleware and
  `TenantContext` — all import routes require it.
- Expo domain still lists five platforms equally; mocks invent engagement
  (`app/src/mocks/fixtures.ts`); onboarding claims live public scanning
  (`app/src/app/onboarding/platforms.tsx`, `how-it-helps.tsx`) — **do not edit
  `app/**` in this plan**; document contract deltas for Plan 007.

```ts
// app/src/domain/types.ts — parser output maps to normalized content_items, not wire DTO yet
export type PlatformId = 'facebook' | 'reddit' | 'instagram' | 'tiktok' | 'x';
export interface FlaggedPost {
  // engagementLabel/likes/comments are NOT reliably populated from archives
  engagementLabel?: string;
  likes?: number;
  comments?: number;
}
```

## Architecture summary

### End-to-end flow

```text
Client                          API (Axum)                     Object store              Import worker
  |                                |                                |                        |
  | POST /v1/archive-imports       |                                |                        |
  |------------------------------->| mint server key + signed POST policy |
  |<-------------------------------| { id, upload: { url, fields }, expiresAt } |
  |                                |                                |                        |
  | POST multipart fields + zip (direct)                            |                        |
  |--------------------------------------------------------------->|                        |
  |                                |                                |                        |
  | POST .../complete              | HEAD + pin versionId           |                        |
  |------------------------------->| enqueue import.normalize       |                        |
  |                                |                                |                        |
  |                                |                                | range GET + parse      |
  |                                |                                |<-----------------------|
  |                                |                                | staging -> commit      |
  |                                |                                | content_items + ready  |
  |                                |                                | delete raw (key,ver)   |
  |                                |                                |                        |
  | (later) POST /v1/scans         | Plan 006 — NOT this plan       |                        |
  |------------------------------->| enqueue scan_posts             |                        |
```

**Hard rules**

- Archive bytes **never** traverse JSON or Axum multipart as the durable handoff.
- Object keys are **server-generated UUID paths**; clients never supply keys.
- **Versioning enabled** on the bucket; completion persists **`raw_storage_version_id`**
  from HEAD/complete response; workers and purge jobs address **`(key, versionId)`**.
- A still-valid presign must **not** allow replacing worker input after complete
  (**overwrite-after-complete** test is mandatory).
- **No live APIs**: parsers must not `reqwest` Reddit/X, resolve t.co, or fetch
  permalinks/URLs from archive cells.
- **No scan side effects on commit**: successful import commit writes
  `content_items` (+ join rows) and sets `archive_imports.status=ready` only.

### Module layout

| Path | Responsibility |
|------|----------------|
| `backend/src/blob/mod.rs` | `BlobStore` trait |
| `backend/src/blob/s3.rs` | S3-compatible adapter: signed POST policy, HEAD, ranged GET, delete version |
| `backend/src/api/archive_imports.rs` | `/v1/archive-imports*` handlers |
| `backend/src/import/mod.rs` | Limits, errors, `NormalizedArchiveRecord`, parser trait, state helpers |
| `backend/src/import/archive.rs` | Safe ZIP central-directory index + counting entry reader |
| `backend/src/import/reddit.rs` | Streaming CSV parser |
| `backend/src/import/x.rs` | JS-wrapper strip + streaming JSON parser |
| `backend/src/import/staging.rs` | Import-scoped staging repo + disk/Postgres bounded indexes |
| `backend/src/import/commit.rs` | Atomic commit + dedupe (**no scan enqueue**) |
| `backend/src/worker/import.rs` | Fenced import worker driving state machine |
| `backend/tests/fixtures/**` | Synthetic + provenance-backed redacted ZIPs |
| `backend/tests/import_*.rs` | Golden, security, idempotency, privacy, boundary tests |

Use **`backend/src/import/`** as the sole parser namespace. If Plan 002 already
created `archive/parsers`, **STOP** and reconcile — do not keep both.

## Runtime limits (exact v1)

Define once in `backend/src/import/limits.rs`; presign policy, HEAD validation,
worker parser, and deployment config **must match**:

| Constant | Value | Enforcement |
|----------|------:|-------------|
| `MAX_ARCHIVE_BYTES` | `2_147_483_648` (2 GiB) | Presign + HEAD + worker |
| `MAX_ZIP_ENTRIES` | `100_000` | Central-directory scan |
| `MAX_ENTRY_NAME_BYTES` | `512` | Raw CD name |
| `MAX_PATH_SEGMENTS` | `8` | Canonical path |
| `MAX_SELECTED_ENTRIES` | `4_096` | Allowlist matches |
| `MAX_SELECTED_ENTRY_UNCOMPRESSED_BYTES` | `134_217_728` (128 MiB) | Per entry |
| `MAX_SELECTED_TOTAL_UNCOMPRESSED_BYTES` | `536_870_912` (512 MiB) | Aggregate selected |
| `MAX_SELECTED_ENTRY_RATIO` | `200:1` | Per entry |
| `MAX_SELECTED_AGGREGATE_RATIO` | `100:1` | Selected aggregate |
| `MAX_CSV_HEADER_BYTES` | `16_384` | Before header parse |
| `MAX_CSV_RECORD_BYTES` | `2_097_152` | Quoted multiline rows |
| `MAX_TEXT_FIELD_BYTES` | `1_048_576` | Any title/body/full_text |
| `MAX_JSON_NESTING` | `64` | Streaming JSON |
| `MAX_NORMALIZED_RECORDS` | `2_000_000` | Per import |
| `MAX_JS_PREFIX_BYTES` | `4_096` | Wrapper assignment scan |
| `MAX_IN_MEMORY_INDEX_BYTES` | `32_777_216` (32 MiB) | X Note/edit in-RAM budget |
| `MAX_DISK_INDEX_BYTES` | `268_435_456` (256 MiB) | Spill to import-scoped staging table |
| `WORKER_IMPORT_MEMORY_BUDGET_BYTES` | `805_306_368` (~768 MiB) | Admission control before claim |

**Memory ceiling (SecurityReview)**: X Note timestamp and edit-chain indexes
**must not** be unbounded `HashMap`s. Implement `BoundedIndex` that tracks byte
budget; spill to **`import_index_entries`** Postgres staging (preferred) or
disk-backed store keyed by `(tenant_id, import_id)`. Include index memory in
worker admission; reject with `resource_limit` before parsing when budget would
be exceeded.

Deployment **STOP** if presign max size, bucket policy, worker task memory, and
parser constants disagree.

## BlobStore and local S3-compatible service contract

Plan 008 wires MinIO locally; Plan 009 wires AWS S3 in production. Plan 004
defines the **application-level contract** both must satisfy.

### Trait

```rust
pub struct PresignedPost {
    pub url: String,
    pub fields: BTreeMap<String, String>,
    pub expires_at: DateTime<Utc>,
}

pub trait BlobStore: Send + Sync {
    async fn presign_post_archive(
        &self,
        key: &str,
        content_length: u64,
        content_type: &str,
        expires_in: Duration,
    ) -> Result<PresignedPost, BlobError>;

    async fn head_object(&self, key: &str) -> Result<ObjectHead, BlobError>;

    async fn head_object_version(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectHead, BlobError>;

    async fn get_range(
        &self,
        key: &str,
        version_id: &str,
        range: Range<u64>,
    ) -> Result<Bytes, BlobError>;

    async fn delete_version(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<(), BlobError>;
}

pub trait DeletionLedgerStore: Send + Sync {
    async fn record_if_absent(
        &self,
        marker: &AccountDeletionMarker,
    ) -> Result<DeletionLedgerReceipt, BlobError>;

    async fn record_provider_complete(
        &self,
        receipt: &ProviderDeletionReceipt,
    ) -> Result<(), BlobError>;
}

// Constructed only by the offline restore-replay command; never placed in AppState.
pub trait DeletionLedgerReader: Send + Sync {
    async fn list_since(
        &self,
        restore_point: DateTime<Utc>,
    ) -> Result<Vec<AccountDeletionMarker>, BlobError>;

    async fn provider_complete(
        &self,
        marker: &AccountDeletionMarker,
    ) -> Result<bool, BlobError>;
}
```

`AccountDeletionMarker` contains only `schema_version`, `tenant_id`, `user_id`,
and `deleted_at`; never email, WorkOS ID, archive keys, or content. Write it to
`deletion-ledger/v1/YYYY/MM/DD/<deleted_at_ms>-<tenant_id>-<user_id>.json`
with create-if-absent semantics, bucket-default encryption, and no public URL.
After WorkOS returns success or not-found, write the deterministic companion
`...<user_id>.provider-complete.json` containing only `{ schemaVersion,
tenantId, userId, completedAt }`. Both writes are idempotent. Serving API/worker
paths never list this prefix; the Plan 009 restore-replay CLI is the only reader.

### Local MinIO / Compose expectations (Plan 008)

| Requirement | Local (MinIO) | Production (Plan 009) |
|-------------|---------------|------------------------|
| Bucket name | `ghostpost-archives` (`ARCHIVE_BUCKET`) | same env name |
| Versioning | **Enabled** (`mc version enable`) | **Enabled** |
| Public access | **Denied** (no anonymous policy) | **Denied** |
| Path style | `ARCHIVE_S3_FORCE_PATH_STYLE=true` | `false` for AWS virtual-host |
| Endpoint | `http://minio:9000` in Compose; `http://127.0.0.1:9000` host-native | optional (AWS default) |
| Backend volume | **Never** mount archive data on API/worker containers | same |
| Lifecycle | Best-effort ≤24h raw + noncurrent (MinIO JSON) | authoritative AWS lifecycle |
| CORS | Exact `AUTH_WEB_ORIGINS`; POST; no exposed provider metadata | same |

Implementation: `backend/src/blob/s3.rs` using `aws-sdk-s3` with optional
custom endpoint. No parallel blob adapter.

The official AWS SDK for Rust does not expose a browser presigned-POST helper
([upstream request #1013](https://github.com/awslabs/aws-sdk-rust/issues/1013)).
Keep `aws-sdk-s3` as the sole storage client and isolate policy construction in
`blob/signed_post.rs`, using credentials from the same AWS config provider plus
`hmac`/`sha2`/base64. Follow AWS's
[SigV4 POST policy](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-HTTPPOSTConstructPolicy.html)
and [condition](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-HTTPPOSTConstructPolicy.html#sigv4-PolicyConditions)
specifications exactly. Do not add a second S3 client crate. Unit-test a fixed
clock/credential golden vector by decoding the policy JSON and independently
recomputing its signature; MinIO and staging AWS integration tests are release
gates.

### Presign policy

- Method: browser-style **multipart POST** directly to the object store; never API proxy upload.
- Policy conditions: exact server-minted `key`, exact `Content-Type`, exact
  bucket, required signing/SSE fields, `success_action_status=204`, and
  `content-length-range` capped at the reserved `contentLength` (which is
  already `<= MAX_ARCHIVE_BYTES`). A presigned PUT is forbidden because it
  cannot enforce an S3 content-length-range policy.
- SSE-KMS required in production; local MinIO may use bucket-default SSE.
- CORS (web): exact app origin(s) from Plan 003 `AUTH_WEB_ORIGINS`; allow POST.
- Policy TTL default **15 minutes** (`ARCHIVE_UPLOAD_TTL_SECS`).

### Completion and immutable version pin

`POST /v1/archive-imports/:id/complete` has an empty body. The client never
hashes or base64-encodes the archive in JavaScript.

1. Load the tenant-owned `awaiting_upload` row and its reserved `byte_size`,
   `content_type`, storage key, and expiry.
2. HEAD the exact server-owned key and reject unless the object exists,
   provider-reported content length exactly equals the reserved length and is
   `<= MAX_ARCHIVE_BYTES`, content type is the reserved allowed ZIP/octet value,
   and upload completed before expiry.
3. Read the provider object version ID and persist
   `raw_storage_version_id`; workers read only `(key, version_id)`.
4. Transition `awaiting_upload -> uploaded -> queued` and enqueue
   `work_items.kind='import.normalize'` with
   `subject_user_id=archive_imports.user_id` and private payload
   `{ import_id, key, version_id }`.
5. Set `raw_delete_after = now() + interval '24 hours'`.

The worker computes SHA-256 while streaming the pinned object for ZIP
validation and tenant-HMAC deduplication. The API never accepts, returns, or
persists a client-provided digest. A repeated empty-body completion after a
successful pin returns the same sanitized `202 ArchiveImportResponse` without
another enqueue.
### Transient checksum and tenant HMAC

During upload verification only:

- Compute/archive-stream SHA-256 of object bytes once (worker validation stage).

Persist **only**:

```text
archive_fingerprint = HMAC-SHA256(
  tenant_archive_key[version],
  tenant_id || user_id || archive_sha256
)
```

- Unique partial index: `(tenant_id, platform, archive_fingerprint)` for exact
  duplicate imports.
- **Do not persist** raw `archive_sha256`, object ETag, or client-visible digest
  after verification (SecurityReview). API responses expose `duplicateOfImportId`
  when fingerprint matches, never the hash.

### Purge and deletion SLA

| Trigger | Action | SLA |
|---------|--------|-----|
| Import reaches `ready`, `rejected`, or `failed` | `delete_version(key, version_id)` best-effort | immediate attempt |
| Upload never completes | abort incomplete multipart + mark `cancelled` | on `upload_expires_at` |
| User `DELETE /v1/archive-imports/:id` | `deleting` → purge raw + staging | enqueue within request; physical delete ≤ **24h** |
| User account deletion (Plan 003) | external ledger → pinned object versions → local content/PII purge; minimal provider tombstone until WorkOS success | local purge complete ≤ **24h** |
| Lifecycle rule (Plan 008/009) | expire raw + noncurrent versions | ≤ **24h** backstop |

Sweeper worker `import.purge_raw` runs every 5 minutes, selects
`raw_deleted_at IS NULL AND raw_delete_after <= now()`, deletes version, sets
`raw_deleted_at`.

Extend Plan 003's existing `account.purge` local checkpoint; do not add another
job kind. Its strict, retry-safe order is:

1. `DeletionLedgerStore::record_if_absent` and verify a durable receipt.
2. Read all user-owned import `(raw_storage_key, raw_storage_version_id)` pairs
   before deleting DB rows, and delete each exact version; `NotFound` is
   idempotent success.
3. Only after every external delete succeeds, hard-delete normalized, staging,
   profile, and session rows; scrub the user to Plan 003's minimal provider-ID
   tombstone; and persist `local_purged_at` on the surviving tenant-scoped purge
   item in one DB transaction.
4. Return to Plan 003's provider checkpoint. A WorkOS failure reads the retained
   tombstone identifier for retry but never restores or blocks already-purged
   local content.

Any ledger/object-store error retries without the database purge transaction.
Tests must inject failure after each boundary and prove no user content or PII
is deleted before the durable marker and all pinned-object deletes complete.
Only the minimal user tombstone, purge item, and empty personal tenant survive
until provider deletion succeeds.

## HTTP API (`/v1/archive-imports`)

All routes require Plan 003 authentication. Web mutations require CSRF + Origin +
Fetch Metadata. State-changing routes require `Idempotency-Key`.

| Method | Path | Body | Success | Notes |
|--------|------|------|---------|-------|
| `POST` | `/v1/archive-imports` | `{ "platform": "reddit"\|"x", "contentLength": int, "contentType": "application/zip"\|"application/octet-stream" }` | `201` `{ id, upload: { method: "POST", url, fields }, expiresAt }` | Validate positive length `<= MAX_ARCHIVE_BYTES`; reject future platforms with `422 PLATFORM_COMING_SOON`; server mints the key and returns only signed form fields. |
| `POST` | `/v1/archive-imports/:id/complete` | empty body | `202` `ArchiveImportResponse` | Server HEAD-verifies reserved length/type, pins version, and enqueues once; replay returns the same sanitized response. |
| `GET` | `/v1/archive-imports` | query: `platform?`, `status?`, `cursor?`, `limit?` | `200` `{ items, nextCursor }` | Tenant-scoped keyset pagination. |
| `GET` | `/v1/archive-imports/:id` | — | `200` | Sanitized `errorCode`; never storage key/version. |
| `DELETE` | `/v1/archive-imports/:id` | — | `202` `{ id, status: "deleting" }` | Does not delete committed content referenced by other imports. |

**Response shape** (`ArchiveImportResponse`):

```json
{
  "id": "uuid",
  "platform": "reddit",
  "status": "parsing",
  "itemCount": 1284,
  "errorCode": null,
  "parserVersion": "import-v1",
  "formatFamily": "RedditGdprCsv",
  "formatConfidence": "compatible",
  "createdAt": "2026-07-23T12:00:00Z",
  "finishedAt": null,
  "duplicateOfImportId": null
}
```

Wrong-tenant access → **404** (not 403).

## Import state machine

Worker DB status maps to internal phases:

```text
awaiting_upload -> uploaded -> queued -> parsing -> normalizing -> ready
                      |                       |
                      +------- cancelled -------+
                      |
                      v
                   failed | rejected (terminal)
                      |
                      v
                   deleting -> deleted
```

Internal worker phases (logged; map to `parsing`/`normalizing`):

```text
validating_container -> detecting_format -> parsing_sources
  -> normalizing_to_staging -> committing
```

Terminal:

- **`rejected`**: deterministic user/input/format (`error_code` stable).
- **`failed`**: retryable internal error (exhaust → `failed` terminal).
- **`cancelled`**: user cancel or upload expiry.

Rules:

- Compare-and-swap transitions guarded by `work_items.lease_owner` fence.
- Retries resume from last **committed** boundary; stale worker with wrong fence
  **cannot commit**.
- Only **`committing`** writes final `content_items` / provenance links and sets
  **`status=ready`**.
- Parsing writes **staging only**; terminal error deletes staging rows.
- **`ready` is the only success terminal** exposed to Plan 006 scan creation.
- **Ingestion states never invoke an LLM** — parsing, staging, and commit are deterministic only.

### Stable rejection codes

`upload_too_large`, `not_zip`, `unsupported_zip`, `encrypted_zip`,
`unsafe_zip_entry`, `duplicate_zip_entry`, `zip_limit_exceeded`,
`unsupported_compression`, `integrity_mismatch`, `no_supported_content`,
`ambiguous_archive`, `platform_mismatch`, `unsupported_format_version`,
`invalid_encoding`, `invalid_schema`, `invalid_record`, `duplicate_conflict`,
`invalid_edit_chain`, `ambiguous_note_join`, `ambiguous_revision_order`,
`resource_limit`.

Malformed/adversarial inputs are **non-retryable** terminal rejections.

## Database schema additions

Create migration `YYYYMMDDHHMMSS_archive_ingestion.sql` (additive to Plan 002).

### Extend `archive_imports`

| Column | Type | Notes |
|--------|------|-------|
| `raw_storage_key` | `text` | Server UUID path; never client-supplied |
| `raw_storage_version_id` | `text` | Pinned at complete; worker reads this only |
| `archive_fingerprint` | `bytea` | HMAC-SHA256 32 bytes; tenant-keyed |
| `fingerprint_key_version` | `smallint` | For rotation |
| `platform` | `text` | `reddit` \| `x` CHECK |
| `status` | `text` | See state machine |
| `byte_size` | `bigint` | Verified size |
| `content_type` | `text` | Reserved and HEAD-verified ZIP/octet type |
| `parser_version` | `text` | e.g. `import-v1` |
| `format_family` | `text` | `RedditGdprCsv`, `XGdpr`, `XClassic` |
| `format_confidence` | `text` | `confirmed` \| `compatible` \| `provisional` |
| `item_count` | `int` | After commit |
| `error_code` | `varchar(80)` | Stable code only |
| `upload_expires_at` | `timestamptz` | Presign expiry |
| `raw_delete_after` | `timestamptz` | now()+24h at complete |
| `raw_deleted_at` | `timestamptz` | NULL until purge |
| `duplicate_of_import_id` | `uuid` | Same fingerprint early return |

**Do not add** durable `content_sha256` column (SecurityReview).

Unique partial index:

```sql
CREATE UNIQUE INDEX archive_imports_fingerprint_uq
  ON archive_imports (tenant_id, platform, archive_fingerprint)
  WHERE status NOT IN ('deleted', 'cancelled') AND archive_fingerprint IS NOT NULL;
```

### `import_staging_records`

Import-scoped rows deleted on terminal or after successful commit:

| Column | Type |
|--------|------|
| `tenant_id`, `import_id` | FK composite |
| `source_revision_id` | `text` |
| `record_json` | `jsonb` | Canonical normalized record pre-commit |
| `content_hmac` | `bytea` | Tenant-keyed canonical content fingerprint |

### `import_index_entries` (disk-backed index spill)

| Column | Type |
|--------|------|
| `tenant_id`, `import_id` | FK |
| `index_kind` | `text` | `x_note_ts`, `x_edit_chain` |
| `entry_key` | `text` | Bounded length |
| `entry_payload` | `bytea` | Serialized minimal fields |

Index size sum per import must stay ≤ `MAX_DISK_INDEX_BYTES`.

### Extend `content_items`

Add columns if missing from Plan 002 kernel:

| Column | Notes |
|--------|-------|
| `source_logical_id` | Stable across X edits |
| `source_revision_id` | Selected revision |
| `record_type` | `post` \| `comment` \| `reply` \| `repost` \| `quote` |
| `text_format` | `standard` \| `long_form_note` |
| `authorship` | `owner_authored` \| `reshared` |
| `content_state` | `active` \| `deleted` \| `removed` |
| `content_hmac` | Tenant-keyed canonical hash |
| `relation_confidence` | `none` \| `explicit` \| `heuristic` |
| `title` | Reddit post title only |
| `body` | Active text (NFC, LF) |
| `created_at` | When known from export |

### `content_source_aliases` (X edit IDs)

Maps alternate revision IDs → `(tenant_id, source_logical_id)`.

### `archive_import_content_items` (join)

Links `(tenant_id, import_id)` → `(tenant_id, content_item_id)` at commit time.

## Safe ZIP / range parsing (`import/archive.rs`)

1. **Magic**: require ZIP local header at byte 0 (`PK\x03\x04`, empty `PK\x05\x06`,
   or valid ZIP64). Reject SFX preamble, spanned/split archives, trailing garbage.
2. **Central directory first**: parse CD without extracting; enforce
   `MAX_ZIP_ENTRIES`, name bytes, segment count.
3. **Canonical paths**: UTF-8, forward slash, no leading `/`, no `\`, no `:`, no
   NUL, no empty/`..`/`.` segments, max 512 bytes, max 8 segments. Case-sensitive
   allowlist match. Reject duplicate canonical names.
4. **Selected entry preflight**: sum declared uncompressed sizes and compression
   ratios for allowlisted entries before inflating any.
5. **Open allowlisted entries only** via counting reader; abort if actual bytes
   exceed declared or limits. Verify CRC.
6. Reject encrypted entries, symlinks, devices, FIFOs, unsupported compression,
   nested ZIPs.
7. **Range reads** via `BlobStore::get_range` — never download whole 2 GiB if
   CD is at tail.

### Exact read allowlist

**Reddit (decompress/read)**

| Path | Purpose |
|------|---------|
| `posts.csv` | Content (**provisional** schema) |
| `comments.csv` | Content (**confirmed** header) |
| `statistics.csv` | Export generation time only |
| `checkfile.csv` | Optional SHA-256 verify for selected paths |

**X (decompress/read)**

| Path | Purpose |
|------|---------|
| `data/manifest.js` | Generation time + file list cross-check |
| `data/tweets.js`, `data/tweet.js` | Tweet arrays |
| `data/tweets-partN.js`, `data/tweet-partN.js` | Sharded tweets (`N` 1–4 digits) |
| `data/tweets/YYYY_MM.js` | Classic monthly (`MM` 01–12) |
| `data/note-tweet.js` | Long-form Note join |
| `data/deleted-tweets.js` | Tombstones |

`data/js/tweet_index.js` — **directory inspect only** (classic detection), no
decompress unless allowlisted monthly files present.

**Never open**: media folders, `like.js`, `follower.js`, DMs, circles,
communities, articles, `account.js`, `profile.js`, nested archives, IP logs, votes,
saved items, messages, modmail, etc.

**Open-zero-excluded proof**: instrumented tests assert `open_count == 0` and
`decompressed_bytes == 0` for all non-allowlisted entries in
`x/excluded_sources.zip` and `zip/private_media_large_but_unopened.zip`.

## Canonical minimal record

Internal normalized shape (staging JSON + commit input). **Not** the provider
payload — Plan 005 builds that at scan time.

```rust
pub struct NormalizedArchiveRecord {
    pub schema_version: u16,              // exactly 1
    pub import_id: Uuid,
    pub platform: ArchivePlatform,        // Reddit | X
    pub source_logical_id: String,        // namespaced ASCII ≤96 bytes
    pub source_revision_id: String,
    pub record_type: RecordType,          // Post | Comment | Reply | Repost | Quote
    pub text_format: TextFormat,          // Standard | LongFormNote
    pub authorship: Authorship,           // OwnerAuthored | Reshared
    pub state: ContentState,              // Active | Deleted | Removed
    pub created_at: Option<DateTime<Utc>>,
    pub title: Option<String>,             // Reddit post only
    pub body: Option<String>,              // active only
    pub parent_source_id: Option<String>,
    pub quoted_source_id: Option<String>,
    pub relation_confidence: RelationConfidence,
    pub text_truncated: bool,
    pub provenance: RecordProvenance,
}

pub struct RecordProvenance {
    pub source_file: AllowedSourceFile,   // enum — internal only
    pub source_ordinal: u64,
    pub format_family: FormatFamily,
    pub format_confidence: FormatConfidence,
}
```

Invariants:

- Active rows require `created_at` + nonempty title/body (post) or body (comment).
- Tombstones: no title/body; stored with `content_state=deleted|removed`.
- `Comment` Reddit-only; `Repost` X-only with `authorship = Reshared`.
- Normalize NFC, LF line endings, decode minimal HTML entities in X text.
- Max 1 MiB per text field; no silent persistence truncation.

Identity:

- Reddit: logical = revision = row `id` (`^[a-z0-9]{1,20}$`).
- X: decimal string IDs only; never float. Edit chain: logical = min(edit IDs),
  revision = max; greatest revision must exist in archive.

Authorship at ingest (full provider mapping in **Archive-to-scan boundary** below):

| Internal enum | Stored (`content_items.authorship`) |
|---------------|-------------------------------------|
| `OwnerAuthored` | `owner_authored` |
| `Reshared` | `reshared` |

## Export research basis and confidence

- Reddit's official help documents the account data-request ZIP acquisition
  path but does **not** publish a versioned CSV schema:
  <https://support.reddithelp.com/hc/en-us/articles/360043048352-How-do-I-request-a-copy-of-my-Reddit-data-and-information>.
  Therefore `comments.csv` is confirmed only by the observed exact header;
  `posts.csv` remains provisional until the redacted-real provenance gate.
- X's official help documents archive acquisition:
  <https://help.x.com/en/managing-your-account/how-to-download-your-x-archive>.
  The maintained Rust schema reference
  <https://github.com/rust-utilities/twitter-archive> corroborates
  `data/tweets.js`, `window.YTD.tweets.part0`, `tweet.created_at`,
  `tweet.full_text`, and `data/manifest.js`. Treat it as corroboration, not a
  normative provider contract; the archive's own `README.txt`/manifest plus a
  redacted observed fixture wins.
  Do not add that AGPL crate or copy its implementation into Ghostpost; use it
  only as schema corroboration and implement from the observed export contract.
- Provider exports are mutable. Unknown wrappers, missing required headers, or
  a changed manifest version fail closed as `unsupported_format_version`; no
  fuzzy field guessing.

## Reddit parser rules (`import/reddit.rs`)

Streaming RFC 4180 CSV; comma; UTF-8 (+ optional BOM); doubled-quote escapes;
**no** formula interpretation.

### Header table

| File | Required columns | Optional (discard) | Confidence |
|------|------------------|--------------------|------------|
| `posts.csv` | `id,permalink,date,subreddit,title,url,body` | `ip,gildings,link,media` | **Provisional** — production STOP until redacted real export |
| `comments.csv` | `id,permalink,date,subreddit,parent,body` | `ip,gildings,link,media` | **Confirmed** if exact header; **Compatible** if extras |
| `statistics.csv` | `statistic,value` | — | metadata only |
| `checkfile.csv` | fixture-confirmed 2-col | — | optional SHA-256 64-hex only |

Missing required column → `unsupported_format_version`. Malformed selected file
→ whole import rejected (no silent comments-only fallback).

### Row classification

| Signal | Result |
|--------|--------|
| Post ID + permalink `t3` match | `Post` active |
| Comment ID; `parent` = `t1_<id>` | `Comment` on comment |
| Comment ID; `parent` = `t3_<id>` | `Comment` on post |
| Other parent syntax | `invalid_record` |
| Title `[deleted by user]` or sentinels | `Deleted` tombstone |
| Body `[deleted]` / `[removed]` | tombstone |
| Empty active text | `invalid_record` |

Dates: RFC 3339 with offset or `%Y-%m-%d %H:%M:%S UTC` only.

Discard before persist: `ip`, `subreddit`, `gildings`, `link`, `url`, `media`,
permalink after cross-check, account fields from statistics (except export time).

## X parser rules (`import/x.rs`)

- **No JS engine**. Strip wrapper: read ≤ `MAX_JS_PREFIX_BYTES`, find first
  `window.YTD.<id>.partN =`, parse JSON array after `=`.
- Envelope: `{ "tweet": {…} }` or bare tweet object only.
- IDs: prefer `id_str`; integer `id` only if exact u64; disagree → terminal.
- Text: prefer `full_text` then `text`.
- `created_at`: `%a %b %d %H:%M:%S %z %Y`; Note `createdAt`: RFC 3339.

### Classification priority

| P | Signal | Result |
|---|--------|--------|
| 1 | `retweeted_status` or `^RT @handle:` prefix | `Repost` / `Reshared` — strip prefix; ignore `retweeted:false` |
| 2 | `in_reply_to_status_id_str` | `Reply` / `Explicit` parent |
| 3 | Exactly one external `x.com`/`twitter.com` status URL in entities | `Quote` / `Heuristic` — remove matching t.co token only |
| 4 | else | `Post` |

Note join (strict):

1. Build bounded timestamp index from `note-tweet.js`.
2. Join only when `createdAt` **exactly equals** tweet `created_at`, preview ends
   with `…`, Note text extends preview prefix.
3. Ambiguous/unmatched → `ambiguous_note_join`.

Edit/deletion:

- Validate full `editTweetIds` chain; greatest revision must exist.
- `deleted-tweets.js` → tombstones; deletion wins over active only for explicit
  tombstone rows.
- Absence in later export **never** implies deletion.

Discard: account/profile, handles, counts, geo, media entities, manifest except
generation time + file list validation.

## Format detection

| Evidence | Family/confidence | Action |
|----------|-------------------|--------|
| Declared Reddit + root content CSV with exact known header | `RedditGdprCsv`; comments `Confirmed`, posts `Provisional` | Parse atomically |
| Declared X + valid manifest cross-check | `XGdpr`, `Confirmed` | Parse manifest-listed shards in numeric order |
| Declared X + allowlisted modern tweet file, no manifest | `XGdpr`, `Compatible` | Parse shards; unique contiguous part numbers |
| Declared X + classic monthly files | `XClassic`, `Compatible` | Parse months in lexical order |
| Reddit and X signatures both present | none | `ambiguous_archive` |
| Declared platform conflicts with detected family | none | `platform_mismatch` |
| Known path but unknown wrapper/schema | none | `unsupported_format_version` |
| Only non-allowlisted/private/media files | none | `no_supported_content` |

Unknown versions never partial-commit. Error messages must not echo adversarial
cell text, unknown headers, or arbitrary filenames.

## Atomic staging, fencing, idempotency

1. **`parsing_sources`**: stream records → `import_staging_records` only.
2. **`normalizing_to_staging`**: build bounded indexes (memory + spill table).
3. **`committing`**: single transaction:
   - upsert `content_items` on `(tenant_id, source_logical_id)`;
   - insert `content_source_aliases` for X edit IDs;
   - link `archive_import_content_items`;
   - delete staging rows + index spill rows;
   - set import **`status=ready`**, `item_count`, `finished_at`.
4. Worker heartbeat must match fence token (Plan 002). Stale worker commit attempt
   → 0 rows updated.

**Idempotency**

- Same tenant + platform + `archive_fingerprint` → return existing import (no
  duplicate staging).
- Within archive: same `(source_revision_id, content_hmac)` coalesces.
- Same revision ID + different content → `duplicate_conflict` (except valid edit
  chain or explicit deletion rules).
- `POST /complete` empty-body replay after a successful pin → same sanitized
  `202` body and no duplicate work item.

**Trusted chronology**

- Use export generation time from manifest/statistics when confirmed/compatible.
- Older trusted snapshot cannot overwrite newer revision; ambiguous order without
  trusted time → `ambiguous_revision_order` (upload time is **not** chronology).

## Archive-to-scan boundary (Plans 005–006)

**Ingestion never invokes an LLM.** Import commit persists normalized
`content_items`, sets `archive_imports.status=ready`, and stops. It does **not**
automatically enqueue scan work, create scan rows, call any completion provider,
or build provider batch payloads.

| Concern | Owner | Notes |
|---------|-------|-------|
| Presign upload + version pin | Plan 004 | This plan |
| Parse ZIP → normalized `content_items` | Plan 004 | Commit + `status=ready` only |
| User-initiated scan creation | Plan 006 | `POST /v1/scans` with `archiveImportIds` |
| Enqueue `scan_posts` work items | Plan 006 | **Only** after explicit scan create |
| Scan worker + batching | Plan 005 | Reads stable `content_items` at claim time |
| Unicode segmentation + chunking | Plan 005 | **Never** in import path |
| Provider batch JSON + prompt assembly | Plan 005 | Built per batch at scan time |

### Server-only fields (never in provider payload)

Plan 004 persists these for dedupe, provenance, and tenant isolation. They
**must remain server-only** — Plan 005 may read them for mapping but **never**
forwards them to the model:

- Internal IDs: `source_logical_id`, `source_revision_id`, parent/quote source IDs
- Provenance: `RecordProvenance`, `source_file`, `source_ordinal`, format metadata
- Fingerprints: `content_hmac`, `archive_fingerprint`
- Timestamps: `created_at` and all export/upload times
- Relations: `relation_confidence`, edit-chain metadata
- URLs: permalinks, profile links, media URLs, and any export-embedded links
- Engagement/count fields: likes, votes, comments, retweets, views, and similar metrics
- Storage: object keys, version IDs, raw digests

### Scan-time provider projection (Plan 005 only)

When Plan 005 assembles a provider batch, each item may include **only**:

1. **Sorted onboarding policy** (Plan 003 profile): `comingUp` and `concerns`,
   sorted in the canonical enum order defined by Plan 005.
2. **Batch-local item fields**:
   - `itemIndex` — 0-based within the batch (not stable across batches)
   - `platform` — `reddit` | `x`
   - `kind` — mapped from stored `record_type`
   - `authorship` — `authored` | `amplified` (see mapping below)
   - `text` — NFC body/title composite per Plan 005 segmentation rules

**Plan 005 owns all provider DTOs and Unicode segmentation.** Plan 004 **must
not** define, import, or build provider projection or segmentation helpers in
`backend/src/import/**` or `backend/src/worker/import.rs`.

### Authorship mapping (`OwnerAuthored` / `Reshared` → provider)

Parsers assign authorship at ingest time (e.g. X repost/RT → `Reshared`).
Plan 005 applies this table at projection time only:

| Internal enum | Stored (`content_items.authorship`) | Provider `authorship` |
|---------------|-------------------------------------|------------------------|
| `OwnerAuthored` | `owner_authored` | `authored` |
| `Reshared` | `reshared` | `amplified` |

### Explicit non-actions at commit (boundary tests must assert)

- **Do not** insert `work_items` with `kind` in (`scan_posts`, `scan.batch`, or
  any Plan 005 scan handler kind).
- **Do not** call DeepSeek, OpenAI, or any LLM/completion provider.
- **Do not** implement provider projection, batch assembly, text chunking, or
  provider-input structs in import modules.

Plan 004 tests must grep import modules and assert absence of scan enqueue, LLM
calls, and provider projection helpers.

## Exclusions (v1 mandatory)

No parser module or allowlist entry for:

- X likes/favorites, Reddit votes, saved items, bookmarks
- DMs, chats, modmail, drafts
- Circle / community / article tweets
- Account/profile/contact files, IP logs
- Media binaries and sidecar metadata
- Live API/scrape/fetch of any URL found in CSV/JSON

Product copy promising likes/tagged photos/live disconnect is **wrong** — Plan
007 must fix; do not expand scope here.

## Fixture matrix and provenance manifests

Fixtures live under `backend/tests/fixtures/`. **Never commit unredacted real
archives**, handles, emails, IPs, URLs, or authentic post text. A production
provenance fixture must first replace all text and pseudonymize identifiers
under the documented transform.

### Provenance manifest (required for production enablement)

For each **enabled format family**, commit
`backend/tests/fixtures/provenance/<family>.manifest.json`:

```json
{
  "family": "XGdpr",
  "fixture": "x/user_redacted_gdpr_v1.zip",
  "fixtureSha256": "…",
  "exportGenerationDate": "2026-03-15",
  "observedWrappers": ["window.YTD.tweets.part0"],
  "observedHeaders": null,
  "redactionTransform": "Replaced all text with lorem; hashed IDs with HMAC-test-key",
  "approvedForProduction": true
}
```

| Family | Production gate |
|--------|-------------------|
| `RedditGdprCsv` comments | provenance manifest + golden tests |
| `RedditGdprCsv` posts | **STOP** until `reddit/user_redacted_posts_v1.zip` manifest approved |
| `XGdpr` | provenance manifest + golden tests |
| `XClassic` | provenance manifest + golden tests |

Synthetic-only fixtures remain for adversarial/edge cases; they **do not** clear
production gates alone (SecurityReview).

### Reddit fixtures (minimum)

`gdpr_minimal.zip`, `comments_only.zip`, `header_only.zip`, `rfc4180_unicode.zip`,
`deleted_removed.zip`, `duplicate_exact.zip`, `duplicate_conflict.zip`,
`unknown_extra_columns.zip`, `missing_required_header.zip`, `malformed_csv.zip`,
`invalid_identity_date.zip`, `checkfile_mismatch.zip`, plus
`user_redacted_posts_v1.zip` (gated).

### X fixtures (minimum)

`gdpr_manifest_valid.zip`, `gdpr_singular_wrapper.zip`, `gdpr_parts_out_of_order.zip`,
`gdpr_part_gap.zip`, `classic_monthly.zip`, `reply.zip`, `retweet_modern.zip`,
`retweet_classic.zip`, `quote_single.zip`, `quote_ambiguous.zip`,
`note_join_valid.zip`, `note_join_ambiguous.zip`, `edit_chain.zip`,
`edit_chain_incomplete.zip`, `deleted.zip`, `wrapper_code_suffix.zip`,
`malformed_json.zip`, `excluded_sources.zip`, plus redacted production manifests.

### Adversarial ZIP fixtures (minimum)

`not_zip.bin`, `sfx_preamble.zip`, `trailing_payload.zip`, `absolute_path.zip`,
`dotdot.zip`, `backslash.zip`, `drive_unc.zip`, `nul_name.zip`, `non_utf8_name.zip`,
`duplicate_canonical_name.zip`, `symlink_device.zip`, `encrypted.zip`,
`unsupported_compression.zip`, `nested_zip.zip`, `entry_count_limit.zip`,
`declared_size_limit.zip`, `per_entry_ratio_bomb.zip`, `aggregate_ratio_bomb.zip`,
`actual_size_overrun.zip`, `ambiguous_reddit_x.zip`,
`private_media_large_but_unopened.zip`.

Generate huge bombs in tests via builders; committed blobs stay small.

## Environment variables

Add to `backend/.env.example` (fail fast in `api`/`worker` roles):

| Variable | Required | Purpose |
|----------|----------|---------|
| `ARCHIVE_BUCKET` | yes | Private bucket name |
| `ARCHIVE_S3_REGION` | yes | Region |
| `ARCHIVE_S3_ENDPOINT` | no | MinIO/local endpoint URL |
| `ARCHIVE_S3_ACCESS_KEY_ID` | local only | MinIO credential; omit in AWS and use ECS task-role credentials |
| `ARCHIVE_S3_SECRET_ACCESS_KEY` | local only | MinIO secret; forbidden in AWS runtime env |
| `ARCHIVE_S3_FORCE_PATH_STYLE` | no | `true` for MinIO |
| `ARCHIVE_UPLOAD_TTL_SECS` | no | Default `900` |
| `ARCHIVE_FINGERPRINT_KEYS` | yes | JSON `[{"version":1,"secret":"<base64 32 bytes>"}]` |
| `ARCHIVE_RAW_RETENTION_HOURS` | no | Default `24` (SLA backstop) |
| `ARCHIVE_MAX_BYTES` | no | Default `2147483648` — must match code constant |

Do **not** call Reddit/X API keys or OAuth secrets — none exist.

## Commands you will need

| Purpose | Command | Expected on success |
|---------|---------|---------------------|
| Drift check | `git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md` | review only |
| Plans 002–003 present | `test -f backend/Cargo.toml && rg -n 'TenantContext|health/ready' backend/src` | exit 0 |
| Migrations apply | `cargo run --locked --manifest-path backend/Cargo.toml -- migrate` | exit 0 |
| Unit tests — archive | `cargo test --locked --manifest-path backend/Cargo.toml import::archive` | exit 0 |
| Golden Reddit | `cargo test --locked --manifest-path backend/Cargo.toml import_reddit_golden -- --nocapture` | exit 0 |
| Golden X | `cargo test --locked --manifest-path backend/Cargo.toml import_x_golden -- --nocapture` | exit 0 |
| Security ZIP table | `cargo test --locked --manifest-path backend/Cargo.toml import_archive_security -- --nocapture` | each fixture → exact error code |
| Open-zero-excluded | `cargo test --locked --manifest-path backend/Cargo.toml import_no_open_excluded -- --nocapture` | exit 0 |
| Idempotency | `cargo test --locked --manifest-path backend/Cargo.toml import_idempotency -- --nocapture` | exit 0 |
| Overwrite-after-complete | `cargo test --locked --manifest-path backend/Cargo.toml import_overwrite_after_complete -- --nocapture` | pinned version unchanged |
| Memory bound | `cargo test --locked --manifest-path backend/Cargo.toml import_memory_ceiling -- --nocapture` | exit 0 |
| Privacy serialization | `cargo test --locked --manifest-path backend/Cargo.toml import_privacy -- --nocapture` | no forbidden keys |
| **No scan enqueue on commit** | `cargo test --locked --manifest-path backend/Cargo.toml import_no_scan_enqueue -- --nocapture` | zero `scan_posts` rows after commit |
| Provenance gate | `cargo test --locked --manifest-path backend/Cargo.toml fixture_provenance -- --nocapture` | all enabled families have approved manifest |
| No live HTTP | `rg -n 'reqwest|reddit\.com|twitter\.com|x\.com' backend/src/import` | **no outbound fetch** matches |
| No provider projection in import | `rg -n 'DeepSeek|scan_posts|scan\.batch' backend/src/import backend/src/worker/import.rs` | **no matches** |
| API integration | `cargo test --locked --manifest-path backend/Cargo.toml archive_imports_api -- --nocapture` | presign/complete/poll/delete |
| Purge sweeper | `cargo test --locked --manifest-path backend/Cargo.toml import_purge_sweeper -- --nocapture` | raw deleted ≤ SLA |

Do **not** run project-wide formatters, linters, or Expo gates as part of this plan.

## Suggested executor toolkit

- Plans 002–003 complete (Postgres + auth + idempotency middleware).
- MinIO or LocalStack with **versioning enabled** (Plan 008) for integration tests.
- `curl`, `aws` CLI or `mc` for manual presign smoke optional.

## Scope

**In scope** (only paths you should create or modify):

- `backend/src/blob/**`
- `backend/src/import/**`
- `backend/src/api/archive_imports.rs` + router wiring
- `backend/src/worker/import.rs` + worker dispatch for `import.normalize` / `import.purge_raw`, and extension of Plan 003's existing `account.purge` handler
- `backend/src/cli.rs` + `backend/src/cli/deletion.rs` for the offline restore-replay subcommand only
- `backend/migrations/*_archive_ingestion.sql`
- `backend/tests/import_*.rs`, `backend/tests/account_purge_*.rs`, `backend/tests/fixtures/**`
- `backend/tests/deletion_restore_replay.rs`
- `backend/.env.example` — archive env keys only (additive)
- `backend/Cargo.toml` / `backend/Cargo.lock` — add `aws-config`, `aws-credential-types`, `aws-sdk-s3`, `hmac`, `sha2`, `base64`, `zip`, `csv`, and required `serde_json` stream features

**Out of scope** (do NOT touch):

- `app/**` (Plan 007)
- `evals/**`, scan prompt, DeepSeek, `scan_posts` worker (Plan 005)
- `POST /v1/scans` and product routes (Plan 006)
- `docker-compose.yml`, `Dockerfile`, SST (Plans 008–009) — except documenting env vars above
- Live Reddit/X API clients, OAuth, scraping, media analysis
- Weakening ZIP limits, allowlist, or tenant isolation for convenience
- Persisting raw archive SHA-256, raw model prompts, or user content in logs
- Building provider batch payloads or Unicode chunking in import modules

## Git workflow

- Branch: `advisor/004-archive-upload-ingestion`
- Commits (examples):
  1. `feat(backend): add BlobStore and archive import schema migration`
  2. `feat(backend): presign upload and complete routes with version pin`
  3. `feat(backend): safe ZIP reader and shared import limits`
  4. `feat(backend): reddit and x parsers with staging and commit`
  5. `test(backend): fixtures, security table, idempotency, and provenance manifests`
- Do NOT push or open PR unless instructed.

## Steps

### Step 0: Drift check and dependency gate

```bash
git diff --stat 3625ed1..HEAD -- backend/ plans/002-backend-kernel-schema.md plans/003-workos-auth-profiles-onboarding.md
test -f backend/Cargo.toml
rg -n 'archive_imports|work_items|TenantContext' backend/src
```

**Verify**: Plan 002 tables/repos and Plan 003 auth middleware exist. If
`backend/src/providers/` also exists for parsers, **STOP** — pick one namespace.

### Step 1: BlobStore + config + migration

Implement `BlobStore` and S3 adapter with signed POST policy/HEAD/range/delete-version.
Add migration extending `archive_imports`, staging/index tables, content columns.
Wire env validation.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml blob::
cargo test --locked --manifest-path backend/Cargo.toml signed_post_policy
cargo run --locked --manifest-path backend/Cargo.toml -- migrate
```

`signed_post_policy` decodes the policy JSON and asserts exact key/type/SSE,
expiry, success status, and `content-length-range <= reserved contentLength`;
tampered fields and oversized multipart upload fail in the MinIO integration.

### Step 2: API routes — create, complete, list, get, delete

Implement `/v1/archive-imports*` with idempotency, CSRF (web), platform gating
(only `reddit`/`x`), server-side key minting, presign issuance.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml archive_imports_api
```

### Step 3: Safe ZIP index + limits

Implement central-directory parser, canonical path rules, selected-entry preflight,
counting inflate reader, stable error codes.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import::archive
cargo test --locked --manifest-path backend/Cargo.toml import_archive_security
```

Expected: each adversarial fixture returns its **exact** error code; ZIP bombs
and traversal paths never write staging rows.

### Step 4: Shared canonical schema + staging repos

Add `NormalizedArchiveRecord`, enums, `RecordSink`, staging repository,
`BoundedIndex` with spill table, fingerprint HMAC helper (no raw SHA persist).

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import::staging import::schema
```

### Step 5: Reddit parser + fixtures

Implement the streaming CSV parser per the rules. Commit synthetic Reddit ZIP
fixtures and golden snapshots. Add a redacted real-export fixture only if the
owner supplies it and explicitly approves committing it; otherwise keep the
posts family `provisional` and test it synthetically rather than naming a fake
fixture path.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import_reddit_golden
```

Comments path must pass; posts remain **`provisional`** until Step 10 manifest.

### Step 6: X parser + fixtures

Implement wrapper strip + streaming JSON parser, classification, Note join,
edit chain, tombstones. Enforce memory/disk index budgets.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import_x_golden import_memory_ceiling
cargo test --locked --manifest-path backend/Cargo.toml import_no_open_excluded
```

### Step 7: Import worker — state machine + atomic commit

Wire `import.normalize` worker: validate container → detect format → parse →
stage → commit; fencing; duplicate fingerprint short-circuit. Commit transaction
writes **`content_items` + `status=ready` only**.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import_worker_e2e -- --nocapture
cargo test --locked --manifest-path backend/Cargo.toml import_idempotency
cargo test --locked --manifest-path backend/Cargo.toml import_no_scan_enqueue -- --nocapture
```

Inject failures at each boundary; assert no final rows before commit; stale fence
cannot commit; **zero** `scan_posts` work items after successful import.

### Step 8: Purge sweeper + deletion SLA

Implement `import.purge_raw` worker and DELETE route transition to `deleting`.
Immediate delete on terminal success/failure/rejection plus `raw_delete_after`
sweeper.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import_purge_sweeper import_deletion_api
```

Simulate object store; assert delete called with `(key, version_id)` within SLA.

### Step 8b: Offline deletion-ledger restore replay

Add `ghostpost-backend deletion replay --restore-point <RFC3339>`. It runs only
when `RESTORE_REPLAY_PENDING=true`, reads the isolated
`RESTORE_DATABASE_URL`, and requires `RESTORE_GUARD_TOKEN`. Before any mutation,
query the one-row `ghostpost_restore_guard` table that the restore runbook
creates **after** restoring the isolated snapshot; constant-time compare its
token hash, and fail closed on absence/mismatch. This table is never created by
normal migrations, so production cannot accidentally satisfy the guard.
Construct `DeletionLedgerReader` only in this CLI path; never add ledger listing
to serving `AppState`.

For every marker returned in deterministic key order:

1. Validate key fields equal body fields and reject malformed/duplicate markers.
2. If no provider-complete companion exists, load the WorkOS ID from the
   restored minimal/full user row, call the existing Plan 003 delete adapter
   (`not_found` succeeds), and write the companion. No user row plus no
   companion is a hard failure.
3. Invoke the same exact-version archive and tenant/user purge repositories as
   `account.purge`.
4. Re-query and fail unless sessions, user-owned rows, and readable pinned
   object versions are all absent.

Never accept a database URL on the command line, log connection/object keys, or
clear the readiness gate itself.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml deletion_restore_replay -- --nocapture
```

Cover snapshots from before enqueue, after enqueue/before local purge, and after
local purge/before provider completion; malformed markers and missing recovery
identifiers fail closed.

### Step 9: Overwrite-after-complete + version pin integration

Integration test against versioned bucket (MinIO):

1. Complete import v1.
2. Reuse the still-valid signed POST policy for the same key to create version B.
3. Worker reads **pinned** `version_id`; import outcome deterministic;
   mismatch → `integrity_mismatch`, zero committed rows from corrupt pass.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml import_overwrite_after_complete -- --ignored --nocapture
```

### Step 10: Provenance manifests and production gate markers

Add `backend/tests/fixtures/provenance/*.manifest.json` for each enabled family.
Mark Reddit posts **`approvedForProduction: false`** until real redacted export
lands. CI test fails if production flag set without fixture file present.

**Verify**:

```bash
cargo test --locked --manifest-path backend/Cargo.toml fixture_provenance
cargo test --locked --manifest-path backend/Cargo.toml import_privacy
rg -n 'reqwest|oauth|reddit\.com/settings|api\.twitter|DeepSeek|scan_posts|scan\.batch' backend/src/import backend/src/worker/import.rs
```

Expected: **no live API**, **no scan enqueue**, **no provider projection** in
import modules.

## Test plan

| Layer | Cases |
|-------|-------|
| **Golden** | Every valid fixture → exact `NormalizedArchiveRecord` snapshot (types, state, IDs, provenance) |
| **Security table** | Each adversarial ZIP → exact stable `error_code`; zero staging/final rows |
| **ZIP bomb / traversal** | ratio bombs, `..` paths, symlink, nested zip, overrun declared size |
| **Open-zero-excluded** | media/likes/DM fixtures → zero opens on excluded entries |
| **Streaming** | 100k-row CSV / multi-shard X with small read buffer; peak memory bounded |
| **Idempotency** | duplicate archive, duplicate rows, conflicting rows, cross-tenant isolation |
| **Overwrite-after-complete** | pinned version immutability |
| **Memory ceiling** | index spill when Note/edit maps exceed RAM budget |
| **Atomicity/fencing** | fail at each state; stale lease cannot commit |
| **Deletion/SLA** | user delete + sweeper + 24h backstop fields |
| **Privacy** | API errors/logs/DB exclude raw hash, storage keys, archive text |
| **Provenance** | every enabled family has approved manifest before prod flag |
| **Scan boundary** | import commit leaves `work_items` free of scan kinds; no LLM calls |

## Done criteria

Machine-checkable. **All** must hold:

- [ ] `BlobStore` signed POST policy/HEAD/range/delete-version implemented; policy length bound tested; env documented
- [ ] `/v1/archive-imports` CRUD + complete routes match DataModelApi contract
- [ ] Version ID pinned at complete; worker reads `(key, versionId)` only
- [ ] Overwrite-after-complete integration test passes on versioned bucket
- [ ] Tenant-keyed `archive_fingerprint` persisted; raw SHA-256 **not** stored
- [ ] All runtime limits centralized and match presign/policy
- [ ] Safe ZIP: CD-first, allowlist-only inflate, stable rejection codes
- [ ] Open-zero-excluded tests pass for media/likes/DM fixtures
- [ ] Reddit comments parser golden tests pass; posts marked provisional
- [ ] X GDPR + classic parsers golden tests pass with bounded indexes
- [ ] Atomic staging → single commit transaction; fencing enforced
- [ ] Commit writes **`content_items` + `status=ready` only** — no scan enqueue
- [ ] `import_no_scan_enqueue` test passes
- [ ] No provider projection, segmentation, or provider-input code in import modules
- [ ] Idempotency + duplicate fingerprint + conflict rules covered
- [ ] Raw purge immediate + sweeper; `raw_delete_after` ≤ 24h SLA tested
- [ ] Provenance manifest present for each enabled family; Reddit posts gate documented
- [ ] No outbound HTTP/live API usage in import modules
- [ ] Only in-scope files modified
- [ ] `plans/README.md` status row updated if you own the index

## STOP conditions

Stop and report (do not improvise) if:

- **Dependency STOP**: Plan 002 `archive_imports`/`work_items` or Plan 003 auth
  middleware missing or incompatible.
- **Namespace STOP**: duplicate parser tree (`import/` vs `providers/`) — resolve
  before coding.
- **Version pin STOP**: object store lacks versioning or HEAD does not return
  version ID — cannot ship completion contract.
- **Overwrite STOP**: integration test proves a post-complete policy reuse
  changes worker input without detection — must fix immutable version pinning.
- **Scan boundary STOP**: import commit enqueues `scan_posts`, calls LLM, or
  builds provider batch payloads — remove immediately; that is Plan 005/006 scope.
- **Reddit posts STOP**: no provenance-backed redacted `posts.csv` fixture —
  do not set `approvedForProduction: true` for post ingestion.
- **Production fixture STOP** (SecurityReview): enabling `reddit` or `x` in prod
  without provenance manifest + real redacted export for each advertised family.
- **Memory STOP**: X parser retains unbounded in-memory indexes — must spill to
  bounded store before merge.
- **Privacy STOP**: raw archive SHA-256, storage keys, or post body appear in
  logs/API errors/migrations — halt.
- **Allowlist STOP**: any test opens excluded entry (media/likes/DM) — fix before
  proceeding.
- **Partial commit STOP**: malformed shard/Note allows some final rows — atomic
  rejection required.
- **Live API STOP**: any fetch to Reddit/X hosts from backend — remove immediately.
- **Limit mismatch STOP**: presign max size ≠ worker constant ≠ env default.
- **Drift STOP**: live Plan 002/003 schema differs from assumptions — reconcile
  with plan author.
- Any focused verification fails **twice** after reasonable fix.

## Maintenance notes

- Reddit `posts.csv` schema changes require new provenance manifest + golden
  snapshot bump; never guess columns from docs alone.
- X wrapper/filename additions require allowlist PR + adversarial tests; never
  glob `data/**`.
- Rotate `ARCHIVE_FINGERPRINT_KEYS` with dual-write migration; never decode old
  fingerprints to raw SHA.
- Plan 008 wires MinIO versioning smoke; Plan 009 owns KMS/lifecycle/IAM — keep
  application `(key, versionId)` contract stable.
- Plan 005 consumes `content_items` with authorship assigned here — coordinate
  schema changes via updated Plan 004/005 drift checks.
- Plan 006 owns `POST /v1/scans` and `scan_posts` enqueue — never reintroduce scan
  side effects into import commit.
- Plan 007 owns frontend upload UX, platform availability, and honest copy.
