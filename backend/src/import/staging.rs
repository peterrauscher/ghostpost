use super::{limits::*,ImportError,ImportErrorCode,NormalizedArchiveRecord};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

pub struct BoundedIndex {
    memory_bytes: usize,
    disk_bytes: usize,
    entries: HashMap<String, Vec<u8>>,
}

impl Default for BoundedIndex {
    fn default() -> Self {
        Self { memory_bytes: 0, disk_bytes: 0, entries: HashMap::new() }
    }
}

impl BoundedIndex {
    pub async fn insert(&mut self, pool: &PgPool, tenant_id: Uuid, import_id: Uuid, kind: &str, key: String, payload: Vec<u8>) -> Result<(), ImportError> {
        let size = key.len().checked_add(payload.len()).ok_or(ImportErrorCode::ResourceLimit)?;
        if let Some(previous) = self.entries.get(&key) {
            self.memory_bytes = self.memory_bytes.saturating_sub(key.len() + previous.len());
        }
        if self.memory_bytes.checked_add(size).is_some_and(|total| total <= MAX_IN_MEMORY_INDEX_BYTES) {
            self.memory_bytes += size;
            self.entries.insert(key, payload);
            return Ok(());
        }
        let previous_disk_size: i64 = sqlx::query_scalar("SELECT COALESCE(octet_length(entry_key)+octet_length(entry_payload),0)::bigint FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2 AND index_kind=$3 AND entry_key=$4")
            .bind(tenant_id).bind(import_id).bind(kind).bind(&key).fetch_optional(pool).await
            .map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))?.unwrap_or(0);
        let projected = self.disk_bytes.saturating_sub(previous_disk_size.max(0) as usize)
            .checked_add(size).ok_or(ImportErrorCode::ResourceLimit)?;
        if projected > MAX_DISK_INDEX_BYTES { return Err(ImportErrorCode::ResourceLimit.into()); }
        sqlx::query("INSERT INTO import_index_entries(tenant_id,import_id,index_kind,entry_key,entry_payload) VALUES($1,$2,$3,$4,$5) ON CONFLICT(tenant_id,import_id,index_kind,entry_key) DO UPDATE SET entry_payload=EXCLUDED.entry_payload")
            .bind(tenant_id).bind(import_id).bind(kind).bind(&key).bind(&payload).execute(pool).await
            .map_err(|_|ImportError::from(ImportErrorCode::ResourceLimit))?;
        self.disk_bytes = projected;
        Ok(())
    }

    pub async fn get(&self, pool: &PgPool, tenant_id: Uuid, import_id: Uuid, kind: &str, key: &str) -> Result<Option<Vec<u8>>, ImportError> {
        if let Some(payload) = self.entries.get(key) { return Ok(Some(payload.clone())); }
        sqlx::query_scalar("SELECT entry_payload FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2 AND index_kind=$3 AND entry_key=$4")
            .bind(tenant_id).bind(import_id).bind(kind).bind(key).fetch_optional(pool).await
            .map_err(|_| ImportError::from(ImportErrorCode::ResourceLimit))
    }
}
pub async fn stage(pool:&PgPool,tenant_id:Uuid,import_id:Uuid,record:&NormalizedArchiveRecord,content_hmac:&[u8])->Result<(),sqlx::Error>{let json:Value=serde_json::to_value(record).map_err(|e|sqlx::Error::Protocol(e.to_string()))?;sqlx::query("INSERT INTO import_staging_records(tenant_id,import_id,source_revision_id,record_json,content_hmac) VALUES($1,$2,$3,$4,$5) ON CONFLICT(tenant_id,import_id,source_revision_id) DO UPDATE SET record_json=CASE WHEN import_staging_records.content_hmac=EXCLUDED.content_hmac THEN import_staging_records.record_json ELSE NULL END,content_hmac=EXCLUDED.content_hmac").bind(tenant_id).bind(import_id).bind(&record.source_revision_id).bind(json).bind(content_hmac).execute(pool).await?;Ok(())}
