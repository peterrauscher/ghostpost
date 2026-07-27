use crate::blob::{BlobError, BlobStore};
use crate::error::{AppError, AppResult};
use crate::import::{self, ArchivePlatform, FormatConfidence, FormatFamily, ImportError, ImportErrorCode};
use crate::repository::work_items::WorkItem;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;
const RANGE_CHUNK: u64 = 8 * 1024 * 1024;

fn worker_error(message: impl Into<String>) -> AppError { AppError::Worker(message.into()) }
fn blob_error(err: BlobError) -> AppError { worker_error(format!("archive storage operation failed: {err}")) }

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

async fn read_pinned(blob: &dyn BlobStore, key: &str, version: &str, expected: u64) -> AppResult<Vec<u8>> {
    if key.is_empty() || version.is_empty() { return Err(worker_error("import job is not version-pinned")); }
    let head = blob.head_object_version(key, version).await.map_err(blob_error)?;
    if head.version_id != version || head.content_length != expected || expected > import::limits::MAX_ARCHIVE_BYTES {
        return Err(worker_error(ImportErrorCode::IntegrityMismatch.as_str()));
    }
    let capacity = usize::try_from(expected).map_err(|_| worker_error("archive too large for worker"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut start = 0;
    while start < expected {
        let end = (start + RANGE_CHUNK).min(expected);
        let chunk = blob.get_range(key, version, start..end).await.map_err(blob_error)?;
        if chunk.len() as u64 != end - start { return Err(worker_error(ImportErrorCode::IntegrityMismatch.as_str())); }
        bytes.extend_from_slice(&chunk);
        start = end;
    }
    Ok(bytes)
}

pub async fn normalize(pool: &PgPool, blob: Arc<dyn BlobStore>, item: &WorkItem, lease_owner: &str) -> AppResult<()> {
    let import_id = item.payload.get("import_id").and_then(|v| v.as_str()).and_then(|v| Uuid::parse_str(v).ok()).ok_or_else(|| worker_error("invalid import.normalize import_id"))?;
    let payload_key = item.payload.get("key").and_then(|v| v.as_str()).ok_or_else(|| worker_error("import.normalize missing pinned key"))?;
    let payload_version = item.payload.get("version_id").and_then(|v| v.as_str()).ok_or_else(|| worker_error("import.normalize missing pinned version"))?;
    let row = sqlx::query("SELECT user_id,platform,raw_storage_key,raw_storage_version_id,byte_size,status FROM archive_imports WHERE tenant_id=$1 AND id=$2")
        .bind(item.tenant_id).bind(import_id).fetch_optional(pool).await?.ok_or_else(|| worker_error("archive import missing"))?;
    let user_id: Uuid = row.try_get("user_id")?;
    let declared: String = row.try_get("platform")?;
    let key: Option<String> = row.try_get("raw_storage_key")?;
    let version: Option<String> = row.try_get("raw_storage_version_id")?;
    let expected: Option<i64> = row.try_get("byte_size")?;
    let key = key.filter(|v| v == payload_key).ok_or_else(|| worker_error("import job key does not match pinned row"))?;
    let version = version.filter(|v| v == payload_version).ok_or_else(|| worker_error("import job version does not match pinned row"))?;
    let expected = expected.and_then(|v| u64::try_from(v).ok()).ok_or_else(|| worker_error("invalid pinned archive size"))?;
    sqlx::query("UPDATE archive_imports SET status='parsing',updated_at=now() WHERE tenant_id=$1 AND id=$2 AND status IN ('queued','parsing')")
        .bind(item.tenant_id).bind(import_id).execute(pool).await?;
    let bytes = read_pinned(blob.as_ref(), &key, &version, expected).await?;
    let digest = Sha256::digest(&bytes);
    let (key_version, secret) = fingerprint_key()?;
    let mut mac = HmacSha256::new_from_slice(&secret).map_err(|_| worker_error("invalid fingerprint key"))?;
    mac.update(item.tenant_id.as_bytes()); mac.update(user_id.as_bytes()); mac.update(&digest);
    let fingerprint = mac.finalize().into_bytes().to_vec();
    let declared_platform = platform(&declared).map_err(|e| worker_error(e.code))?;
    let archive = import::archive::SafeArchive::index(bytes, declared_platform).map_err(|e| worker_error(e.code))?;
    let selected_order: Vec<String> = archive.selected.iter().map(|entry| entry.name.clone()).collect();
    let selected = archive.read_selected().map_err(|e| worker_error(e.code))?;
    let mut records = Vec::new();
    let (family, confidence) = match declared_platform {
        ArchivePlatform::Reddit => {
            for (name, bytes) in &selected { if matches!(name.as_str(), "comments.csv" | "posts.csv") { records.extend(import::reddit::parse_csv(bytes.as_slice(), name, import_id).map_err(|e| worker_error(e.code))?); } }
            (FormatFamily::RedditGdprCsv, if selected.contains_key("posts.csv") { FormatConfidence::Provisional } else { FormatConfidence::Confirmed })
        }
        ArchivePlatform::X => {
            let has_manifest = selected.contains_key("data/manifest.js");
            let family = if selected_order.iter().any(|name| name.starts_with("data/tweets/")) { FormatFamily::XClassic } else { FormatFamily::XGdpr };
            let ordered: Vec<(String, Vec<u8>)> = selected_order.iter()
                .filter_map(|name| selected.get(name).map(|bytes| (name.clone(), bytes.clone())))
                .collect();
            records = import::x::parse_archive(pool, item.tenant_id, import_id, &ordered, family).await
                .map_err(|e| worker_error(e.code))?;
            (family, if has_manifest { FormatConfidence::Confirmed } else { FormatConfidence::Compatible })
        }
    };
    if records.is_empty() { return Err(worker_error(ImportErrorCode::NoSupportedContent.as_str())); }
    if records.len() > import::limits::MAX_NORMALIZED_RECORDS { return Err(worker_error(ImportErrorCode::ResourceLimit.as_str())); }
    sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2").bind(item.tenant_id).bind(import_id).execute(pool).await?;
    for record in records {
        let canonical = serde_json::to_vec(&record).map_err(|_| worker_error("record serialization failed"))?;
        let mut content_mac = HmacSha256::new_from_slice(&secret).map_err(|_| worker_error("invalid fingerprint key"))?;
        content_mac.update(item.tenant_id.as_bytes()); content_mac.update(&canonical);
        import::staging::stage(pool, item.tenant_id, import_id, &record, &content_mac.finalize().into_bytes()).await?;
    }
    sqlx::query("UPDATE archive_imports SET status='normalizing',archive_fingerprint=$3,fingerprint_key_version=$4,format_family=$5,format_confidence=$6,updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(item.tenant_id).bind(import_id).bind(fingerprint).bind(key_version).bind(format!("{family:?}")).bind(match confidence { FormatConfidence::Confirmed=>"confirmed",FormatConfidence::Compatible=>"compatible",FormatConfidence::Provisional=>"provisional" }).execute(pool).await?;
    if !import::commit::commit_staged(pool, item.tenant_id, import_id, lease_owner).await? { return Err(worker_error("lost import fence")); }
    purge_one(pool, blob, item.tenant_id, import_id).await
}

pub async fn purge_one(pool: &PgPool, blob: Arc<dyn BlobStore>, tenant_id: Uuid, import_id: Uuid) -> AppResult<()> {
    let row = sqlx::query("SELECT raw_storage_key,raw_storage_version_id,raw_deleted_at,status FROM archive_imports WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(import_id).fetch_optional(pool).await?;
    let Some(row)=row else { return Ok(()); };
    if row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("raw_deleted_at")?.is_some() { return Ok(()); }
    let key: Option<String> = row.try_get("raw_storage_key")?; let version: Option<String> = row.try_get("raw_storage_version_id")?;
    match (key, version) { (Some(k),Some(v)) => match blob.delete_version(&k,&v).await { Ok(())|Err(BlobError::NotFound)=>{}, Err(e)=>return Err(blob_error(e)) }, _=>return Ok(()) }
    sqlx::query("UPDATE archive_imports SET raw_deleted_at=now(),status=CASE WHEN status='deleting' THEN 'deleted' ELSE status END,updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(import_id).execute(pool).await?;
    Ok(())
}

pub async fn purge_job(pool:&PgPool, blob:Arc<dyn BlobStore>, item:&WorkItem)->AppResult<()> {
    let import_id=item.payload.get("import_id").and_then(|v|v.as_str()).and_then(|v|Uuid::parse_str(v).ok()).ok_or_else(||worker_error("invalid import.purge_raw import_id"))?;
    purge_one(pool,blob,item.tenant_id,import_id).await
}

pub async fn sweep_due(pool:&PgPool, blob:Arc<dyn BlobStore>)->AppResult<u64>{
    let rows=sqlx::query("SELECT tenant_id,id FROM archive_imports WHERE raw_deleted_at IS NULL AND raw_storage_key IS NOT NULL AND raw_storage_version_id IS NOT NULL AND raw_delete_after<=now() ORDER BY raw_delete_after LIMIT 100").fetch_all(pool).await?;
    let mut count=0; for row in rows { purge_one(pool,blob.clone(),row.try_get("tenant_id")?,row.try_get("id")?).await?; count+=1; } Ok(count)
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
    let Some(import_id) = item.payload.get("import_id").and_then(|v| v.as_str()).and_then(|v| Uuid::parse_str(v).ok()) else {
        return Ok(false);
    };
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2")
        .bind(item.tenant_id).bind(import_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2")
        .bind(item.tenant_id).bind(import_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE archive_imports SET status='rejected',error_code=$3,finished_at=now(),updated_at=now() WHERE tenant_id=$1 AND id=$2")
        .bind(item.tenant_id).bind(import_id).bind(code.as_str()).execute(&mut *tx).await?;
    tx.commit().await?;
    purge_one(pool, blob, item.tenant_id, import_id).await?;
    Ok(true)
}
