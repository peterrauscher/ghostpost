CREATE TABLE archive_imports (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  platform text NOT NULL
    CHECK (platform IN ('facebook', 'reddit', 'instagram', 'tiktok', 'x')),
  status text NOT NULL DEFAULT 'awaiting_upload'
    CHECK (status IN (
      'awaiting_upload', 'uploaded', 'queued', 'parsing', 'normalizing',
      'ready', 'failed', 'rejected', 'cancelled', 'deleting', 'deleted'
    )),
  item_count int,
  error_code varchar(80),
  cancel_requested_at timestamptz,
  lease_owner text,
  lease_expires_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, user_id)
    REFERENCES users(tenant_id, id)
    ON DELETE CASCADE
);

CREATE INDEX archive_imports_user_status_idx
  ON archive_imports (tenant_id, user_id, status);

CREATE INDEX archive_imports_stale_lease_idx
  ON archive_imports (status, lease_expires_at)
  WHERE status IN ('queued', 'parsing', 'normalizing') AND lease_expires_at IS NOT NULL;

CREATE TABLE content_items (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  platform text NOT NULL
    CHECK (platform IN ('facebook', 'reddit', 'instagram', 'tiktok', 'x')),
  kind text NOT NULL DEFAULT 'post',
  authorship text NOT NULL DEFAULT 'owner_authored'
    CHECK (authorship IN ('owner_authored', 'reshared')),
  title text,
  body text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, user_id)
    REFERENCES users(tenant_id, id)
    ON DELETE CASCADE
);

CREATE INDEX content_items_user_platform_idx
  ON content_items (tenant_id, user_id, platform);
