//! `import.normalize`: spool the pinned archive to an unlinked temp file,
//! claim its fingerprint (identical re-uploads become duplicates of the first
//! import), then stream entries through bounded parsers into staging. Peak
//! memory is independent of archive size: one spool chunk, one ZIP entry
//! reader, at most `PARSE_CHANNEL_CAPACITY` parsed records in flight, and
//! cross-record indexes that spill to Postgres.
use crate::blob::{BlobError, BlobStore};
use crate::error::{AppError, AppResult};
use crate::import::archive::SafeArchive;
use crate::import::pipeline::{self, ParsedItem};
use crate::import::staging::{self, SpillTarget};
use crate::import::x::{RecordSink, XItem, XResolver};
use crate::import::{self, ArchivePlatform, FormatConfidence, FormatFamily, ImportError, ImportErrorCode, NormalizedArchiveRecord};
use crate::repository::work_items::WorkItem;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Seconds before re-checking an import whose identical twin is still being processed.
pub const DUPLICATE_RECHECK_SECS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizeOutcome {
    /// Ready (parsed or resolved as a duplicate), or nothing left to do.
    Completed,
    /// Not an error: re-run after the delay without consuming an attempt.
    Deferred { seconds: i64 },
}

fn worker_error(message: impl Into<String>) -> AppError { AppError::Worker(message.into()) }
fn blob_error(err: BlobError) -> AppError { worker_error(format!("archive storage operation failed: {err}")) }
fn import_error(err: ImportError) -> AppError { worker_error(err.code) }

fn platform(value: &str) -> Result<ArchivePlatform, ImportError> {
    match value { "reddit" => Ok(ArchivePlatform::Reddit), "x" => Ok(ArchivePlatform::X), _ => Err(ImportErrorCode::PlatformMismatch.into()) }
}

fn fingerprint_key() -> Result<(i16, Vec<u8>), AppError> {
    let raw = std::env::var("ARCHIVE_FINGERPRINT_KEYS").map_err(|_| worker_error("ARCHIVE_FINGERPRINT_KEYS is required"))?;
    let values: serde_json::Value = serde_json::from_str(&raw).map_err(|_| worker_error("invalid ARCHIVE_FINGERPRINT_KEYS"))?;
    let current = values.as_array().and_then(|v| v.last()).ok_or_else(|| worker_error("ARCHIVE_FINGERPRINT_KEYS is empty"))?;
    let version = current.get("version").and_then(|v| v.as_i64()).and_then(|v| i16::try_from(v).ok()).ok_or_else(|| worker_error("invalid fingerprint key version"))?;
    let encoded = current.get("secret").and_then(|v| v.as_str()).ok_or_else(|| worker_error("invalid fingerprint key secret"))?;
    use base64::Engine;
    let secret = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|_| worker_error("invalid fingerprint key secret"))?;
    if secret.len() != 32 { return Err(worker_error("fingerprint key must be 32 bytes")); }
    Ok((version, secret))
}

fn spool_dir() -> PathBuf {
    std::env::var_os("IMPORT_SPOOL_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Streams the pinned object into an anonymous temp file (unlinked at
/// creation, so nothing survives a crash) and hashes it on the way. Every read
/// carries `If-Match: <etag>`; bytes replaced after completion surface as
/// `IntegrityMismatch`, a deterministic rejection, never a parse.
async fn spool_pinned(blob: &dyn BlobStore, key: &str, etag: &str, expected: u64) -> AppResult<(std::fs::File, [u8; 32])> {
    if key.is_empty() || etag.is_empty() { return Err(worker_error("import job is not etag-pinned")); }
    let pinned_error = |err: BlobError| match err { BlobError::Integrity => worker_error(ImportErrorCode::IntegrityMismatch.as_str()), other => blob_error(other) };
    let head = blob.head_object_if_match(key, etag).await.map_err(pinned_error)?;
    if head.etag != etag || head.content_length != expected || expected > import::limits::MAX_ARCHIVE_BYTES {
        return Err(worker_error(ImportErrorCode::IntegrityMismatch.as_str()));
    }
    let mut file = tempfile::tempfile_in(spool_dir()).map_err(|e| worker_error(format!("import spool unavailable: {e}")))?;
    let mut hasher = Sha256::new();
    let mut start = 0;
    while start < expected {
        let end = (start + import::limits::SPOOL_CHUNK_BYTES).min(expected);
        let chunk = blob.get_range(key, etag, start..end).await.map_err(pinned_error)?;
        if chunk.len() as u64 != end - start { return Err(worker_error(ImportErrorCode::IntegrityMismatch.as_str())); }
        hasher.update(&chunk);
        file.write_all(&chunk).map_err(|e| worker_error(format!("import spool write failed: {e}")))?;
        start = end;
    }
    file.flush().and_then(|_| file.seek(SeekFrom::Start(0)).map(|_| ()))
        .map_err(|e| worker_error(format!("import spool write failed: {e}")))?;
    Ok((file, hasher.finalize().into()))
}

enum Claim {
    Claimed,
    /// A ready import of this user already holds these exact bytes.
    DuplicateOf(Uuid),
    /// The holder is still being processed; check again later.
    HolderInProgress,
}

fn is_fingerprint_conflict(err: &sqlx::Error) -> bool {
    err.as_database_error().and_then(|e| e.constraint()) == Some("archive_imports_fingerprint_uq")
}

/// Claims the fingerprint before any parsing, so the unique index arbitrates
/// concurrent identical uploads instead of failing late as a database error.
async fn claim_fingerprint(pool: &PgPool, tenant_id: Uuid, import_id: Uuid, platform: &str, fingerprint: &[u8], key_version: i16) -> AppResult<Claim> {
    for _ in 0..3 {
        let claimed = sqlx::query("UPDATE archive_imports SET archive_fingerprint=$3,fingerprint_key_version=$4,updated_at=now() WHERE tenant_id=$1 AND id=$2")
            .bind(tenant_id).bind(import_id).bind(fingerprint).bind(key_version).execute(pool).await;
        match claimed {
            Ok(_) => return Ok(Claim::Claimed),
            Err(err) if is_fingerprint_conflict(&err) => {}
            Err(err) => return Err(err.into()),
        }
        let holder: Option<(Uuid, String)> = sqlx::query_as("SELECT id,status FROM archive_imports WHERE tenant_id=$1 AND platform=$2 AND archive_fingerprint=$3 AND id<>$4 AND status NOT IN ('deleted','cancelled')")
            .bind(tenant_id).bind(platform).bind(fingerprint).bind(import_id).fetch_optional(pool).await?;
        match holder {
            None => continue, // released between our UPDATE and SELECT; claim again
            Some((holder, status)) if status == "ready" => return Ok(Claim::DuplicateOf(holder)),
            Some((holder, status)) if status == "rejected" || status == "failed" => {
                // Terminal imports no longer own their bytes (legacy rows kept them).
                sqlx::query("UPDATE archive_imports SET archive_fingerprint=NULL,updated_at=now() WHERE tenant_id=$1 AND id=$2 AND status IN ('rejected','failed')")
                    .bind(tenant_id).bind(holder).execute(pool).await?;
            }
            Some(_) => return Ok(Claim::HolderInProgress),
        }
    }
    Ok(Claim::HolderInProgress)
}

/// Completes `import_id` as a duplicate of the ready `holder`: same content
/// links (so scans can select either import), same item count and format.
async fn finish_duplicate(pool: &PgPool, tenant_id: Uuid, import_id: Uuid, holder: Uuid, lease_owner: &str) -> AppResult<bool> {
    let mut tx = pool.begin().await?;
    let fenced = sqlx::query("SELECT 1 FROM work_items w WHERE w.tenant_id=$1 AND (w.payload->>'import_id')::uuid=$2 AND w.kind='import.normalize' AND w.lease_owner=$3 AND w.status='running' FOR UPDATE")
        .bind(tenant_id).bind(import_id).bind(lease_owner).fetch_optional(&mut *tx).await?.is_some();
    if !fenced { return Ok(false); }
    sqlx::query("INSERT INTO archive_import_content_items(tenant_id,import_id,content_item_id) SELECT tenant_id,$2,content_item_id FROM archive_import_content_items WHERE tenant_id=$1 AND import_id=$3 ON CONFLICT DO NOTHING")
        .bind(tenant_id).bind(import_id).bind(holder).execute(&mut *tx).await?;
    sqlx::query("UPDATE archive_imports a SET status='ready',duplicate_of_import_id=$3,archive_fingerprint=NULL,fingerprint_key_version=NULL,format_family=h.format_family,format_confidence=h.format_confidence,item_count=(SELECT count(*) FROM archive_import_content_items WHERE tenant_id=$1 AND import_id=$2),finished_at=now(),updated_at=now() FROM archive_imports h WHERE a.tenant_id=$1 AND a.id=$2 AND h.tenant_id=$1 AND h.id=$3")
        .bind(tenant_id).bind(import_id).bind(holder).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2").bind(tenant_id).bind(import_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2").bind(tenant_id).bind(import_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(true)
}

fn content_hmac(secret: &[u8], tenant_id: Uuid, record: &NormalizedArchiveRecord) -> Result<Vec<u8>, ImportError> {
    let canonical = serde_json::to_vec(record).map_err(|_| ImportError::from(ImportErrorCode::InvalidRecord))?;
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))?;
    mac.update(tenant_id.as_bytes());
    mac.update(&canonical);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn staging_error(_: sqlx::Error) -> ImportError { ImportError { code: "staging write failed" } }

/// X records resolved by `XResolver`, staged with overwrite semantics.
struct StagingSink<'a> {
    pool: &'a PgPool,
    tenant_id: Uuid,
    import_id: Uuid,
    secret: &'a [u8],
}

#[async_trait::async_trait]
impl RecordSink for StagingSink<'_> {
    async fn stage(&mut self, record: &NormalizedArchiveRecord) -> Result<(), ImportError> {
        let hmac = content_hmac(self.secret, self.tenant_id, record)?;
        staging::stage_replace(self.pool, self.tenant_id, self.import_id, record, &hmac).await.map_err(staging_error)
    }
    async fn unstage(&mut self, source_revision_id: &str) -> Result<(), ImportError> {
        staging::unstage(self.pool, self.tenant_id, self.import_id, source_revision_id).await.map_err(staging_error)
    }
}

pub async fn normalize(pool: &PgPool, blob: Arc<dyn BlobStore>, item: &WorkItem, lease_owner: &str) -> AppResult<NormalizeOutcome> {
    let import_id = item.payload.get("import_id").and_then(|v| v.as_str()).and_then(|v| Uuid::parse_str(v).ok()).ok_or_else(|| worker_error("invalid import.normalize import_id"))?;
    let payload_key = item.payload.get("key").and_then(|v| v.as_str()).ok_or_else(|| worker_error("import.normalize missing pinned key"))?;
    let payload_etag = item.payload.get("etag").and_then(|v| v.as_str()).ok_or_else(|| worker_error("import.normalize missing pinned etag"))?;
    let row = sqlx::query("SELECT user_id,platform,raw_storage_key,raw_storage_etag,byte_size,status FROM archive_imports WHERE tenant_id=$1 AND id=$2")
        .bind(item.tenant_id).bind(import_id).fetch_optional(pool).await?.ok_or_else(|| worker_error("archive import missing"))?;
    let user_id: Uuid = row.try_get("user_id")?;
    let declared: String = row.try_get("platform")?;
    let key: Option<String> = row.try_get("raw_storage_key")?;
    let etag: Option<String> = row.try_get("raw_storage_etag")?;
    let expected: Option<i64> = row.try_get("byte_size")?;
    let key = key.filter(|v| v == payload_key).ok_or_else(|| worker_error("import job key does not match pinned row"))?;
    let etag = etag.filter(|v| v == payload_etag).ok_or_else(|| worker_error("import job etag does not match pinned row"))?;
    let expected = expected.and_then(|v| u64::try_from(v).ok()).ok_or_else(|| worker_error("invalid pinned archive size"))?;
    sqlx::query("UPDATE archive_imports SET status='parsing',updated_at=now() WHERE tenant_id=$1 AND id=$2 AND status IN ('queued','parsing')")
        .bind(item.tenant_id).bind(import_id).execute(pool).await?;

    let (spool, digest) = spool_pinned(blob.as_ref(), &key, &etag, expected).await?;
    let (key_version, secret) = fingerprint_key()?;
    let mut mac = HmacSha256::new_from_slice(&secret).map_err(|_| worker_error("invalid fingerprint key"))?;
    mac.update(item.tenant_id.as_bytes()); mac.update(user_id.as_bytes()); mac.update(&digest);
    let fingerprint = mac.finalize().into_bytes().to_vec();
    let declared_platform = platform(&declared).map_err(import_error)?;

    match claim_fingerprint(pool, item.tenant_id, import_id, &declared, &fingerprint, key_version).await? {
        Claim::Claimed => {}
        Claim::HolderInProgress => return Ok(NormalizeOutcome::Deferred { seconds: DUPLICATE_RECHECK_SECS }),
        Claim::DuplicateOf(holder) => {
            if !finish_duplicate(pool, item.tenant_id, import_id, holder, lease_owner).await? { return Err(worker_error("lost import fence")); }
            purge_one(pool, blob, item.tenant_id, import_id).await?;
            return Ok(NormalizeOutcome::Completed);
        }
    }

    // A retry starts from a clean slate: earlier attempts' staging and spill rows are void.
    sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2").bind(item.tenant_id).bind(import_id).execute(pool).await?;
    sqlx::query("DELETE FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2").bind(item.tenant_id).bind(import_id).execute(pool).await?;

    let archive = tokio::task::spawn_blocking(move || SafeArchive::index(spool, declared_platform)).await
        .map_err(|e| worker_error(format!("archive index task failed: {e}")))?
        .map_err(import_error)?;
    let names: Vec<&str> = archive.selected.iter().map(|entry| entry.name.as_str()).collect();
    let (family, confidence) = match declared_platform {
        ArchivePlatform::Reddit => (FormatFamily::RedditGdprCsv, if names.contains(&"posts.csv") { FormatConfidence::Provisional } else { FormatConfidence::Confirmed }),
        ArchivePlatform::X => (
            if names.iter().any(|name| name.starts_with("data/tweets/")) { FormatFamily::XClassic } else { FormatFamily::XGdpr },
            if names.contains(&"data/manifest.js") { FormatConfidence::Confirmed } else { FormatConfidence::Compatible },
        ),
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel(import::limits::PARSE_CHANNEL_CAPACITY);
    let producer = tokio::task::spawn_blocking(move || pipeline::produce(archive, declared_platform, import_id, family, tx));
    let spill = SpillTarget { pool, tenant_id: item.tenant_id, import_id };
    let mut resolver = XResolver::new(Some(spill));
    let mut sink = StagingSink { pool, tenant_id: item.tenant_id, import_id, secret: &secret };
    let consumed: Result<(), ImportError> = async {
        while let Some(parsed) = rx.recv().await {
            match parsed {
                ParsedItem::Reddit(record) => {
                    let hmac = content_hmac(&secret, item.tenant_id, &record)?;
                    staging::stage(pool, item.tenant_id, import_id, &record, &hmac).await.map_err(staging_error)?;
                }
                ParsedItem::X(XItem::Note(note)) => resolver.add_note(note).await?,
                ParsedItem::X(XItem::Tweet(tweet)) => resolver.add_tweet(tweet, &mut sink).await?,
            }
        }
        Ok(())
    }.await;
    drop(rx); // unblocks the producer if we stopped early
    let produced = producer.await.map_err(|e| worker_error(format!("archive parse task failed: {e}")))?;
    let parsed: Result<(), ImportError> = async {
        consumed?;
        produced?;
        if declared_platform == ArchivePlatform::X {
            resolver.finish(&mut sink).await?;
        }
        let staged: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2")
            .bind(item.tenant_id).bind(import_id).fetch_one(pool).await.map_err(staging_error)?;
        if staged == 0 { return Err(ImportErrorCode::NoSupportedContent.into()); }
        if staged as usize > import::limits::MAX_NORMALIZED_RECORDS { return Err(ImportErrorCode::ResourceLimit.into()); }
        Ok(())
    }.await;
    if let Err(err) = parsed {
        // Streaming stages before the whole archive is validated; a failed
        // parse must not leave any of the user's text behind in staging.
        sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2").bind(item.tenant_id).bind(import_id).execute(pool).await?;
        sqlx::query("DELETE FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2").bind(item.tenant_id).bind(import_id).execute(pool).await?;
        return Err(import_error(err));
    }
    sqlx::query("UPDATE archive_imports SET status='normalizing',format_family=$3,format_confidence=$4,updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(item.tenant_id).bind(import_id).bind(format!("{family:?}")).bind(match confidence { FormatConfidence::Confirmed=>"confirmed",FormatConfidence::Compatible=>"compatible",FormatConfidence::Provisional=>"provisional" }).execute(pool).await?;
    if !import::commit::commit_staged(pool, item.tenant_id, import_id, lease_owner).await? { return Err(worker_error("lost import fence")); }
    purge_one(pool, blob, item.tenant_id, import_id).await?;
    Ok(NormalizeOutcome::Completed)
}

pub async fn purge_one(pool: &PgPool, blob: Arc<dyn BlobStore>, tenant_id: Uuid, import_id: Uuid) -> AppResult<()> {
    let row = sqlx::query("SELECT raw_storage_key,raw_deleted_at,status FROM archive_imports WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(import_id).fetch_optional(pool).await?;
    let Some(row)=row else { return Ok(()); };
    if row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("raw_deleted_at")?.is_some() { return Ok(()); }
    // Keys are create-once and unversioned, so deleting the key removes every byte,
    // including an upload that landed but was never completed.
    let Some(key) = row.try_get::<Option<String>, _>("raw_storage_key")? else { return Ok(()); };
    match blob.delete_object(&key).await { Ok(())|Err(BlobError::NotFound)=>{}, Err(e)=>return Err(blob_error(e)) }
    // The ETag is a content digest (MD5 for single-part uploads); keep it only while the bytes exist.
    sqlx::query("UPDATE archive_imports SET raw_deleted_at=now(),raw_storage_etag=NULL,status=CASE WHEN status='deleting' THEN 'deleted' ELSE status END,updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(import_id).execute(pool).await?;
    Ok(())
}

pub async fn purge_job(pool:&PgPool, blob:Arc<dyn BlobStore>, item:&WorkItem)->AppResult<()> {
    let import_id=item.payload.get("import_id").and_then(|v|v.as_str()).and_then(|v|Uuid::parse_str(v).ok()).ok_or_else(||worker_error("invalid import.purge_raw import_id"))?;
    purge_one(pool,blob,item.tenant_id,import_id).await
}

pub async fn sweep_due(pool:&PgPool, blob:Arc<dyn BlobStore>)->AppResult<u64>{
    let rows=sqlx::query("SELECT tenant_id,id FROM archive_imports WHERE raw_deleted_at IS NULL AND raw_storage_key IS NOT NULL AND raw_storage_etag IS NOT NULL AND raw_delete_after<=now() ORDER BY raw_delete_after LIMIT 100").fetch_all(pool).await?;
    let mut count=0; for row in rows { purge_one(pool,blob.clone(),row.try_get("tenant_id")?,row.try_get("id")?).await?; count+=1; } Ok(count)
}

/// Ends an import whose job will not run again: discards staging, releases
/// the fingerprint (a corrected re-upload must not be blocked), records the
/// error code, and purges the raw bytes.
async fn end_import(pool: &PgPool, blob: Arc<dyn BlobStore>, tenant_id: Uuid, import_id: Uuid, status: &str, code: &str) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2")
        .bind(tenant_id).bind(import_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2")
        .bind(tenant_id).bind(import_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE archive_imports SET status=$3,error_code=$4,archive_fingerprint=NULL,fingerprint_key_version=NULL,finished_at=now(),updated_at=now() WHERE tenant_id=$1 AND id=$2 AND status NOT IN ('ready','deleting','deleted','cancelled')")
        .bind(tenant_id).bind(import_id).bind(status).bind(code).execute(&mut *tx).await?;
    tx.commit().await?;
    purge_one(pool, blob, tenant_id, import_id).await
}

fn job_import_id(item: &WorkItem) -> Option<Uuid> {
    item.payload.get("import_id").and_then(|v| v.as_str()).and_then(|v| Uuid::parse_str(v).ok())
}

pub async fn reject_if_deterministic(
    pool: &PgPool,
    blob: Arc<dyn BlobStore>,
    item: &WorkItem,
    message: &str,
) -> AppResult<bool> {
    let code = [
        ImportErrorCode::UploadTooLarge, ImportErrorCode::NotZip,
        ImportErrorCode::UnsupportedZip, ImportErrorCode::EncryptedZip,
        ImportErrorCode::UnsafeZipEntry, ImportErrorCode::DuplicateZipEntry,
        ImportErrorCode::ZipLimitExceeded, ImportErrorCode::UnsupportedCompression,
        ImportErrorCode::IntegrityMismatch, ImportErrorCode::NoSupportedContent,
        ImportErrorCode::AmbiguousArchive, ImportErrorCode::PlatformMismatch,
        ImportErrorCode::UnsupportedFormatVersion, ImportErrorCode::InvalidEncoding,
        ImportErrorCode::InvalidSchema, ImportErrorCode::InvalidRecord,
        ImportErrorCode::DuplicateConflict, ImportErrorCode::InvalidEditChain,
        ImportErrorCode::AmbiguousNoteJoin, ImportErrorCode::AmbiguousRevisionOrder,
        ImportErrorCode::ResourceLimit,
    ].into_iter().find(|candidate| message.contains(candidate.as_str()));
    let Some(code) = code else { return Ok(false); };
    let Some(import_id) = job_import_id(item) else { return Ok(false); };
    end_import(pool, blob, item.tenant_id, import_id, "rejected", code.as_str()).await?;
    Ok(true)
}

/// The job exhausted its attempts on a non-deterministic error: the import
/// must not sit in `parsing` forever.
pub async fn fail_terminal(pool: &PgPool, blob: Arc<dyn BlobStore>, item: &WorkItem) -> AppResult<()> {
    let Some(import_id) = job_import_id(item) else { return Ok(()); };
    end_import(pool, blob, item.tenant_id, import_id, "failed", "processing_failed").await
}
