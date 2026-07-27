-- Plan 003: additive auth/profile/onboarding extensions (do not recreate Plan 002 tables).

ALTER TABLE users ADD COLUMN IF NOT EXISTS greeting_name text;
ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_url text;
ALTER TABLE users ADD COLUMN IF NOT EXISTS deleted_at timestamptz;

CREATE UNIQUE INDEX IF NOT EXISTS users_workos_user_id_uq
  ON users (workos_user_id)
  WHERE workos_user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS workos_sessions (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  id uuid NOT NULL DEFAULT gen_random_uuid(),
  user_id uuid NOT NULL,
  sealed_session bytea NOT NULL,
  seal_key_version text NOT NULL,
  workos_session_id text,
  expires_at timestamptz NOT NULL,
  revoked_at timestamptz,
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, id),
  FOREIGN KEY (tenant_id, user_id)
    REFERENCES users(tenant_id, id)
    ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS workos_sessions_active_user_idx
  ON workos_sessions (tenant_id, user_id)
  WHERE revoked_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS workos_sessions_workos_session_id_uq
  ON workos_sessions (workos_session_id)
  WHERE workos_session_id IS NOT NULL;

ALTER TABLE auth_sessions ADD COLUMN IF NOT EXISTS key_id text;
ALTER TABLE auth_sessions ADD COLUMN IF NOT EXISTS client text;
ALTER TABLE auth_sessions ADD COLUMN IF NOT EXISTS csrf_hash bytea;
ALTER TABLE auth_sessions ADD COLUMN IF NOT EXISTS csrf_rotated_at timestamptz;
ALTER TABLE auth_sessions ADD COLUMN IF NOT EXISTS workos_session_id uuid;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint WHERE conname = 'auth_sessions_client_check'
  ) THEN
    ALTER TABLE auth_sessions
      ADD CONSTRAINT auth_sessions_client_check
      CHECK (client IS NULL OR client IN ('web', 'native'));
  END IF;
END $$;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint WHERE conname = 'auth_sessions_workos_session_fk'
  ) THEN
    ALTER TABLE auth_sessions
      ADD CONSTRAINT auth_sessions_workos_session_fk
      FOREIGN KEY (tenant_id, workos_session_id)
      REFERENCES workos_sessions (tenant_id, id)
      ON DELETE SET NULL;
  END IF;
END $$;

-- Plan 002 already created auth_sessions_active_token_hash_uq; keep idempotent.
CREATE UNIQUE INDEX IF NOT EXISTS auth_sessions_active_token_hash_uq
  ON auth_sessions (token_hash)
  WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS auth_flows (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  state text UNIQUE NOT NULL,
  code_verifier_hash bytea NOT NULL,
  code_verifier_enc bytea NOT NULL,
  exchange_secret_hash bytea NOT NULL,
  client text NOT NULL CHECK (client IN ('web', 'native')),
  redirect_uri text NOT NULL,
  tenant_id uuid,
  user_id uuid,
  consumed_at timestamptz,
  expires_at timestamptz NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS auth_flows_expires_idx ON auth_flows (expires_at);

ALTER TABLE onboarding_profiles
  ADD COLUMN IF NOT EXISTS revision bigint NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS current_step smallint NOT NULL DEFAULT 1,
  ADD COLUMN IF NOT EXISTS consent_version text,
  ADD COLUMN IF NOT EXISTS consent_accepted_at timestamptz,
  ADD COLUMN IF NOT EXISTS completed_at timestamptz;

DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint WHERE conname = 'onboarding_profiles_current_step_check'
  ) THEN
    ALTER TABLE onboarding_profiles
      ADD CONSTRAINT onboarding_profiles_current_step_check
      CHECK (current_step BETWEEN 1 AND 4);
  END IF;
END $$;

CREATE TABLE IF NOT EXISTS webhook_events (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  provider text NOT NULL DEFAULT 'workos',
  event_id text NOT NULL,
  event_type text NOT NULL,
  payload_hash bytea NOT NULL,
  processed_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (provider, event_id)
);
