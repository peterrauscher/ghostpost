-- Plan 006: product APIs, free_beta grants, review dispositions, purge support.
-- Additive only — does not recreate Plan 002 entitlement/scan tables.

-- Expand review_status for local dispositions (open remains the only non-terminal).
ALTER TABLE flagged_posts DROP CONSTRAINT IF EXISTS flagged_posts_review_status_check;
ALTER TABLE flagged_posts
  ADD CONSTRAINT flagged_posts_review_status_check
  CHECK (review_status IN ('open', 'closed', 'resolved', 'deleted', 'archived', 'kept'));

-- Scan ↔ ready-import linkage (exact IDs chosen at create time).
CREATE TABLE IF NOT EXISTS scan_archive_imports (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  scan_id uuid NOT NULL,
  archive_import_id uuid NOT NULL,
  PRIMARY KEY (tenant_id, scan_id, archive_import_id),
  CONSTRAINT scan_archive_imports_scan_fk
    FOREIGN KEY (tenant_id, scan_id) REFERENCES scans(tenant_id, id) ON DELETE CASCADE,
  CONSTRAINT scan_archive_imports_import_fk
    FOREIGN KEY (tenant_id, archive_import_id) REFERENCES archive_imports(tenant_id, id)
);

CREATE INDEX IF NOT EXISTS scan_archive_imports_import_idx
  ON scan_archive_imports (tenant_id, archive_import_id);

-- At most one active (queued|running) scan per user.
CREATE UNIQUE INDEX IF NOT EXISTS scans_one_active_per_user_idx
  ON scans (tenant_id, user_id)
  WHERE status IN ('queued', 'running');

-- Local content purge markers (delete_local).
ALTER TABLE content_items
  ADD COLUMN IF NOT EXISTS deletion_pending_at timestamptz,
  ADD COLUMN IF NOT EXISTS purged_at timestamptz;

CREATE INDEX IF NOT EXISTS content_items_purge_pending_idx
  ON content_items (tenant_id, deletion_pending_at)
  WHERE deletion_pending_at IS NOT NULL AND purged_at IS NULL;

-- Soft-hide flags when content is scheduled for local purge.
ALTER TABLE flagged_posts
  ADD COLUMN IF NOT EXISTS hidden_at timestamptz;

CREATE INDEX IF NOT EXISTS flagged_posts_open_risk_idx
  ON flagged_posts (tenant_id, scan_id, risk_level)
  WHERE review_status = 'open' AND hidden_at IS NULL;

-- Idempotency resource pointer for scan_create / flag_review_action replay.
ALTER TABLE idempotency_keys
  ADD COLUMN IF NOT EXISTS resource_id uuid;

CREATE INDEX IF NOT EXISTS idempotency_keys_retention_idx
  ON idempotency_keys (created_at);

-- free_beta: at most one non-revoked grant per user+product (launch cohort).
CREATE UNIQUE INDEX IF NOT EXISTS entitlement_grants_one_active_product_idx
  ON entitlement_grants (tenant_id, user_id, product_key)
  WHERE revoked_at IS NULL;

-- Backfill free_beta for existing non-deleted users (idempotent via unique index).
INSERT INTO entitlement_grants (
  tenant_id, user_id, product_key, grant_source,
  review_access, rescan_limit, platform_limit, valid_from
)
SELECT
  u.tenant_id,
  u.id,
  'free_beta',
  'promotion',
  true,
  NULL,
  2,
  now()
FROM users u
WHERE u.deleted_at IS NULL
  AND NOT EXISTS (
    SELECT 1
    FROM entitlement_grants g
    WHERE g.tenant_id = u.tenant_id
      AND g.user_id = u.id
      AND g.product_key = 'free_beta'
      AND g.revoked_at IS NULL
  );
