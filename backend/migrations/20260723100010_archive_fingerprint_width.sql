DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conrelid = 'archive_imports'::regclass
          AND conname = 'archive_imports_fingerprint_width_check'
    ) THEN
        ALTER TABLE archive_imports
            ADD CONSTRAINT archive_imports_fingerprint_width_check
            CHECK (archive_fingerprint IS NULL OR octet_length(archive_fingerprint) = 32) NOT VALID;
    END IF;
END
$$;
