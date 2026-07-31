-- Restore-only guard. UNLOGGED contents are intentionally absent after physical restore.
CREATE UNLOGGED TABLE IF NOT EXISTS ghostpost_restore_guard (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    token_hash bytea NOT NULL CHECK (octet_length(token_hash) = 32),
    updated_at timestamptz NOT NULL DEFAULT now()
);

REVOKE ALL ON ghostpost_restore_guard FROM PUBLIC;
