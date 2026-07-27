ALTER TABLE archive_imports
    ADD COLUMN IF NOT EXISTS reserve_idempotency_key text;

CREATE UNIQUE INDEX IF NOT EXISTS archive_imports_reserve_idempotency_uq
    ON archive_imports (tenant_id, user_id, reserve_idempotency_key)
    WHERE reserve_idempotency_key IS NOT NULL;
