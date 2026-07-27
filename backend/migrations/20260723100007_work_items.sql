CREATE TABLE work_items (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  subject_user_id uuid,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  kind text NOT NULL,
  payload jsonb NOT NULL DEFAULT '{}',
  dedupe_key text,
  status text NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
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
  CONSTRAINT work_items_max_attempts_check CHECK (max_attempts >= 1),
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

CREATE TABLE work_item_attempts (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  work_item_id uuid NOT NULL,
  attempt_number int NOT NULL CHECK (attempt_number >= 1),
  lease_owner text NOT NULL,
  outcome text
    CHECK (outcome IS NULL OR outcome IN (
      'succeeded', 'failed', 'cancelled', 'lost_lease', 'retry'
    )),
  error_text text,
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  PRIMARY KEY (tenant_id, id),
  UNIQUE (tenant_id, work_item_id, attempt_number),
  FOREIGN KEY (tenant_id, work_item_id)
    REFERENCES work_items(tenant_id, id)
    ON DELETE CASCADE
);
