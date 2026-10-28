-- Raw archives are pinned by the store's ETag (create-once PUT + If-Match reads)
-- instead of an S3 object version. The partial purge index follows the rename.
ALTER TABLE archive_imports RENAME COLUMN raw_storage_version_id TO raw_storage_etag;
