ALTER TABLE archive_imports
    ADD COLUMN IF NOT EXISTS upload_reservation jsonb;
