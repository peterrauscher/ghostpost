-- Plan 005: fenced scan batch metadata (no raw prompt/completion columns)
ALTER TABLE scan_batches
  ADD COLUMN IF NOT EXISTS batch_key_id text;

ALTER TABLE scan_batches
  ADD COLUMN IF NOT EXISTS external_batch_id text;

-- model_attempts already has fenced hash/token/cost columns from 00005.
-- Ensure provider column exists for adapter id without retaining content.
ALTER TABLE model_attempts
  ADD COLUMN IF NOT EXISTS provider text;
