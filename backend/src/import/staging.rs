use super::{limits::*, ImportError, ImportErrorCode, NormalizedArchiveRecord};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

/// Where a `BoundedIndex` may spill. `None` keeps the index memory-only and
/// turns exceeding the in-memory budget into `ResourceLimit` (pure parsing).
#[derive(Clone, Copy)]
pub struct SpillTarget<'p> {
    pub pool: &'p PgPool,
    pub tenant_id: Uuid,
    pub import_id: Uuid,
}

/// Key/value index for cross-record import state (X notes, revisions, edit
/// chains). Holds up to `MAX_IN_MEMORY_INDEX_BYTES` of keys+payloads in
/// memory, then spills to `import_index_entries`. A key lives in exactly one
/// place: once the index has spilled, new keys go straight to disk, and an
/// in-memory key that outgrows the budget moves to disk.
pub struct BoundedIndex<'p> {
    spill: Option<SpillTarget<'p>>,
    kind: &'static str,
    memory_bytes: usize,
    disk_bytes: usize,
    spilled: bool,
    entries: HashMap<String, Vec<u8>>,
}

fn limit(_: sqlx::Error) -> ImportError {
    ImportErrorCode::ResourceLimit.into()
}

impl<'p> BoundedIndex<'p> {
    pub fn new(spill: Option<SpillTarget<'p>>, kind: &'static str) -> Self {
        Self { spill, kind, memory_bytes: 0, disk_bytes: 0, spilled: false, entries: HashMap::new() }
    }

    pub async fn insert(&mut self, key: String, payload: Vec<u8>) -> Result<(), ImportError> {
        let size = key.len().checked_add(payload.len()).ok_or(ImportErrorCode::ResourceLimit)?;
        if let Some(previous) = self.entries.get(&key) {
            let without = self.memory_bytes - (key.len() + previous.len());
            if without.checked_add(size).is_some_and(|total| total <= MAX_IN_MEMORY_INDEX_BYTES) {
                self.memory_bytes = without + size;
                self.entries.insert(key, payload);
                return Ok(());
            }
            self.entries.remove(&key);
            self.memory_bytes = without;
        } else if !self.spilled
            && self.memory_bytes.checked_add(size).is_some_and(|total| total <= MAX_IN_MEMORY_INDEX_BYTES)
        {
            self.memory_bytes += size;
            self.entries.insert(key, payload);
            return Ok(());
        }
        self.write_disk(key, payload, size).await
    }

    async fn write_disk(&mut self, key: String, payload: Vec<u8>, size: usize) -> Result<(), ImportError> {
        let target = self.spill.ok_or(ImportErrorCode::ResourceLimit)?;
        self.spilled = true;
        let previous_disk_size: i64 = sqlx::query_scalar("SELECT COALESCE(octet_length(entry_key)+octet_length(entry_payload),0)::bigint FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2 AND index_kind=$3 AND entry_key=$4")
            .bind(target.tenant_id).bind(target.import_id).bind(self.kind).bind(&key).fetch_optional(target.pool).await
            .map_err(limit)?.unwrap_or(0);
        let projected = self.disk_bytes.saturating_sub(previous_disk_size.max(0) as usize)
            .checked_add(size).ok_or(ImportErrorCode::ResourceLimit)?;
        if projected > MAX_DISK_INDEX_BYTES { return Err(ImportErrorCode::ResourceLimit.into()); }
        sqlx::query("INSERT INTO import_index_entries(tenant_id,import_id,index_kind,entry_key,entry_payload) VALUES($1,$2,$3,$4,$5) ON CONFLICT(tenant_id,import_id,index_kind,entry_key) DO UPDATE SET entry_payload=EXCLUDED.entry_payload")
            .bind(target.tenant_id).bind(target.import_id).bind(self.kind).bind(&key).bind(&payload).execute(target.pool).await
            .map_err(limit)?;
        self.disk_bytes = projected;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, ImportError> {
        if let Some(payload) = self.entries.get(key) { return Ok(Some(payload.clone())); }
        let Some(target) = self.spill.filter(|_| self.spilled) else { return Ok(None) };
        sqlx::query_scalar("SELECT entry_payload FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2 AND index_kind=$3 AND entry_key=$4")
            .bind(target.tenant_id).bind(target.import_id).bind(self.kind).bind(key).fetch_optional(target.pool).await
            .map_err(limit)
    }

    /// Pages through every entry exactly once: first the in-memory entries
    /// (bounded by `MAX_IN_MEMORY_INDEX_BYTES`), then spilled entries in
    /// keyset pages of 1,000. An empty page means done.
    pub async fn next_page(&self, cursor: &mut IndexCursor) -> Result<Vec<(String, Vec<u8>)>, ImportError> {
        if !cursor.memory_done {
            cursor.memory_done = true;
            if !self.entries.is_empty() {
                return Ok(self.entries.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
            }
        }
        let Some(target) = self.spill.filter(|_| self.spilled) else { return Ok(Vec::new()) };
        let page: Vec<(String, Vec<u8>)> = sqlx::query_as("SELECT entry_key,entry_payload FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2 AND index_kind=$3 AND entry_key>$4 ORDER BY entry_key LIMIT 1000")
            .bind(target.tenant_id).bind(target.import_id).bind(self.kind).bind(&cursor.after).fetch_all(target.pool).await
            .map_err(limit)?;
        if let Some((last, _)) = page.last() { cursor.after = last.clone(); }
        Ok(page)
    }
}

#[derive(Default)]
pub struct IndexCursor {
    memory_done: bool,
    after: String,
}

/// Generic staging: an identical re-stage keeps the row, a conflicting one
/// poisons it (record_json NULL) so commit cannot silently pick a side.
pub async fn stage(pool:&PgPool,tenant_id:Uuid,import_id:Uuid,record:&NormalizedArchiveRecord,content_hmac:&[u8])->Result<(),sqlx::Error>{let json:Value=serde_json::to_value(record).map_err(|e|sqlx::Error::Protocol(e.to_string()))?;sqlx::query("INSERT INTO import_staging_records(tenant_id,import_id,source_revision_id,record_json,content_hmac) VALUES($1,$2,$3,$4,$5) ON CONFLICT(tenant_id,import_id,source_revision_id) DO UPDATE SET record_json=CASE WHEN import_staging_records.content_hmac=EXCLUDED.content_hmac THEN import_staging_records.record_json ELSE NULL END,content_hmac=EXCLUDED.content_hmac").bind(tenant_id).bind(import_id).bind(&record.source_revision_id).bind(json).bind(content_hmac).execute(pool).await?;Ok(())}

/// Staging for callers that already resolved revision conflicts (X): the
/// latest decision overwrites.
pub async fn stage_replace(pool:&PgPool,tenant_id:Uuid,import_id:Uuid,record:&NormalizedArchiveRecord,content_hmac:&[u8])->Result<(),sqlx::Error>{let json:Value=serde_json::to_value(record).map_err(|e|sqlx::Error::Protocol(e.to_string()))?;sqlx::query("INSERT INTO import_staging_records(tenant_id,import_id,source_revision_id,record_json,content_hmac) VALUES($1,$2,$3,$4,$5) ON CONFLICT(tenant_id,import_id,source_revision_id) DO UPDATE SET record_json=EXCLUDED.record_json,content_hmac=EXCLUDED.content_hmac").bind(tenant_id).bind(import_id).bind(&record.source_revision_id).bind(json).bind(content_hmac).execute(pool).await?;Ok(())}

pub async fn unstage(pool:&PgPool,tenant_id:Uuid,import_id:Uuid,source_revision_id:&str)->Result<(),sqlx::Error>{sqlx::query("DELETE FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2 AND source_revision_id=$3").bind(tenant_id).bind(import_id).bind(source_revision_id).execute(pool).await?;Ok(())}
