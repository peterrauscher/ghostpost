CREATE TABLE flagged_posts (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  scan_id uuid NOT NULL,
  content_item_id uuid NOT NULL,
  user_id uuid NOT NULL,
  risk_level text NOT NULL CHECK (risk_level IN ('high', 'medium', 'low')),
  category text,
  reason_summary text,
  evidence text,
  review_status text NOT NULL DEFAULT 'open'
    CHECK (review_status IN ('open', 'closed')),
  closed_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  CONSTRAINT flagged_posts_scan_fk
    FOREIGN KEY (tenant_id, scan_id) REFERENCES scans(tenant_id, id)
    ON DELETE CASCADE,
  CONSTRAINT flagged_posts_content_fk
    FOREIGN KEY (tenant_id, content_item_id) REFERENCES content_items(tenant_id, id)
    ON DELETE CASCADE,
  CONSTRAINT flagged_posts_user_fk
    FOREIGN KEY (tenant_id, user_id) REFERENCES users(tenant_id, id)
    ON DELETE CASCADE
);

CREATE INDEX flagged_posts_scan_idx
  ON flagged_posts (tenant_id, scan_id, review_status);

CREATE TABLE review_actions (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  flagged_post_id uuid NOT NULL,
  user_id uuid NOT NULL,
  action text NOT NULL
    CHECK (action IN ('delete', 'archive', 'keep', 'resolve', 'delete_local')),
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, flagged_post_id)
    REFERENCES flagged_posts(tenant_id, id)
    ON DELETE CASCADE,
  FOREIGN KEY (tenant_id, user_id)
    REFERENCES users(tenant_id, id)
    ON DELETE CASCADE
);

CREATE TABLE entitlement_grants (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  product_key text NOT NULL
    CHECK (product_key IN ('single', 'sevenDay', 'thirtyDay', 'free_beta')),
  grant_source text NOT NULL CHECK (grant_source IN ('admin', 'promotion', 'test')),
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

CREATE INDEX entitlement_grants_active_idx
  ON entitlement_grants (tenant_id, user_id, product_key)
  WHERE revoked_at IS NULL;

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
