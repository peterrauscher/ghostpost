CREATE TABLE scans (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  status text NOT NULL DEFAULT 'queued'
    CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
  phase text NOT NULL DEFAULT 'connecting'
    CHECK (phase IN ('connecting', 'scanning', 'flagging', 'complete')),
  progress int NOT NULL DEFAULT 0 CHECK (progress >= 0 AND progress <= 100),
  cancel_requested_at timestamptz,
  lease_owner text,
  lease_expires_at timestamptz,
  error_code text,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, user_id)
    REFERENCES users(tenant_id, id)
    ON DELETE CASCADE
);

CREATE INDEX scans_user_status_idx
  ON scans (tenant_id, user_id, status);

CREATE INDEX scans_stale_lease_idx
  ON scans (status, lease_expires_at)
  WHERE status = 'running' AND lease_expires_at IS NOT NULL;

CREATE TABLE scan_batches (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  scan_id uuid NOT NULL,
  batch_index int NOT NULL CHECK (batch_index >= 0),
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
  item_count int NOT NULL DEFAULT 0 CHECK (item_count >= 0),
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  PRIMARY KEY (tenant_id, id),
  UNIQUE (tenant_id, scan_id, batch_index),
  FOREIGN KEY (tenant_id, scan_id)
    REFERENCES scans(tenant_id, id)
    ON DELETE CASCADE
);

CREATE TABLE model_attempts (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  scan_id uuid NOT NULL,
  batch_id uuid NOT NULL,
  attempt int NOT NULL CHECK (attempt >= 1),
  prompt_version text NOT NULL,
  prompt_sha256 text,
  model_id text,
  request_sha256 text,
  result_sha256 text,
  outcome text,
  error_code text,
  input_tokens int,
  output_tokens int,
  cache_hit_tokens int,
  cost_usd numeric,
  latency_ms int,
  provider_request_id text,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, scan_id)
    REFERENCES scans(tenant_id, id)
    ON DELETE CASCADE,
  FOREIGN KEY (tenant_id, batch_id)
    REFERENCES scan_batches(tenant_id, id)
    ON DELETE CASCADE
);

CREATE INDEX model_attempts_batch_idx
  ON model_attempts (tenant_id, batch_id, attempt);
