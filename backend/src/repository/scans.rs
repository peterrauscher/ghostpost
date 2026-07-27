use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl ScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    Connecting,
    Scanning,
    Flagging,
    Complete,
}

impl ScanPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Scanning => "scanning",
            Self::Flagging => "flagging",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Scan {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub status: String,
    pub phase: String,
    pub progress: i32,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

pub async fn create_scan(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Scan, sqlx::Error> {
    sqlx::query_as::<_, Scan>(
        r#"
INSERT INTO scans (tenant_id, user_id)
VALUES ($1, $2)
RETURNING tenant_id, id, user_id, status, phase, progress, cancel_requested_at,
          lease_owner, lease_expires_at, error_code, created_at, updated_at, finished_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
}

pub async fn request_cancel(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
UPDATE scans
SET cancel_requested_at = COALESCE(cancel_requested_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScanBatch {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub scan_id: Uuid,
    pub batch_index: i32,
    pub status: String,
    pub item_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ModelAttempt {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub scan_id: Uuid,
    pub batch_id: Uuid,
    pub attempt: i32,
    pub prompt_version: String,
    pub outcome: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn create_batch(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_index: i32,
    item_count: i32,
) -> Result<ScanBatch, sqlx::Error> {
    sqlx::query_as::<_, ScanBatch>(
        r#"
INSERT INTO scan_batches (tenant_id, scan_id, batch_index, item_count)
VALUES ($1, $2, $3, $4)
RETURNING tenant_id, id, scan_id, batch_index, status, item_count, created_at, updated_at, finished_at
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(batch_index)
    .bind(item_count)
    .fetch_one(pool)
    .await
}

pub async fn record_model_attempt(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_id: Uuid,
    attempt: i32,
    prompt_version: &str,
    outcome: Option<&str>,
) -> Result<ModelAttempt, sqlx::Error> {
    sqlx::query_as::<_, ModelAttempt>(
        r#"
INSERT INTO model_attempts (tenant_id, scan_id, batch_id, attempt, prompt_version, outcome)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING tenant_id, id, scan_id, batch_id, attempt, prompt_version, outcome, created_at
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(batch_id)
    .bind(attempt)
    .bind(prompt_version)
    .bind(outcome)
    .fetch_one(pool)
    .await
}


pub async fn record_model_attempt_fenced(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_id: Uuid,
    rec: &crate::scan::persist::ModelAttemptRecord,
) -> Result<Uuid, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO model_attempts (
  tenant_id, scan_id, batch_id, attempt, prompt_version,
  prompt_sha256, model_id, provider, request_sha256, result_sha256,
  outcome, error_code, input_tokens, output_tokens, cache_hit_tokens,
  cost_usd, latency_ms, provider_request_id
)
VALUES (
  $1,$2,$3,$4,$5,
  $6,$7,$8,$9,$10,
  $11,$12,$13,$14,$15,
  $16,$17,$18
)
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(batch_id)
    .bind(rec.attempt)
    .bind(&rec.prompt_version)
    .bind(&rec.prompt_sha256)
    .bind(&rec.model_id)
    .bind(&rec.provider)
    .bind(&rec.request_sha256)
    .bind(&rec.result_sha256)
    .bind(&rec.outcome)
    .bind(&rec.error_code)
    .bind(rec.input_tokens)
    .bind(rec.output_tokens)
    .bind(rec.cache_hit_tokens)
    .bind(rec.cost_usd)
    .bind(rec.latency_ms)
    .bind(&rec.provider_request_id)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn create_batch_with_key(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_index: i32,
    item_count: i32,
    batch_key_id: Option<&str>,
    external_batch_id: Option<&str>,
) -> Result<ScanBatch, sqlx::Error> {
    sqlx::query_as::<_, ScanBatch>(
        r#"
INSERT INTO scan_batches (tenant_id, scan_id, batch_index, item_count, batch_key_id, external_batch_id)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING tenant_id, id, scan_id, batch_index, status, item_count, created_at, updated_at, finished_at
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(batch_index)
    .bind(item_count)
    .bind(batch_key_id)
    .bind(external_batch_id)
    .fetch_one(pool)
    .await
}


pub async fn get_scan(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
) -> Result<Option<Scan>, sqlx::Error> {
    sqlx::query_as::<_, Scan>(
        r#"
SELECT tenant_id, id, user_id, status, phase, progress, cancel_requested_at,
       lease_owner, lease_expires_at, error_code, created_at, updated_at, finished_at
FROM scans
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_current_scan(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Scan>, sqlx::Error> {
    sqlx::query_as::<_, Scan>(
        r#"
SELECT tenant_id, id, user_id, status, phase, progress, cancel_requested_at,
       lease_owner, lease_expires_at, error_code, created_at, updated_at, finished_at
FROM scans
WHERE tenant_id = $1 AND user_id = $2
ORDER BY
  CASE WHEN status IN ('queued', 'running') THEN 0 ELSE 1 END,
  created_at DESC
LIMIT 1
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn latest_succeeded_scan(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<Scan>, sqlx::Error> {
    sqlx::query_as::<_, Scan>(
        r#"
SELECT tenant_id, id, user_id, status, phase, progress, cancel_requested_at,
       lease_owner, lease_expires_at, error_code, created_at, updated_at, finished_at
FROM scans
WHERE tenant_id = $1 AND user_id = $2 AND status = 'succeeded'
ORDER BY finished_at DESC NULLS LAST, created_at DESC
LIMIT 1
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn create_scan_tx<'e, E>(
    executor: E,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Scan, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_as::<_, Scan>(
        r#"
INSERT INTO scans (tenant_id, user_id)
VALUES ($1, $2)
RETURNING tenant_id, id, user_id, status, phase, progress, cancel_requested_at,
          lease_owner, lease_expires_at, error_code, created_at, updated_at, finished_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(executor)
    .await
}

pub async fn link_scan_imports_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    scan_id: Uuid,
    import_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    for import_id in import_ids {
        sqlx::query(
            r#"
INSERT INTO scan_archive_imports (tenant_id, scan_id, archive_import_id)
VALUES ($1, $2, $3)
"#,
        )
        .bind(tenant_id)
        .bind(scan_id)
        .bind(*import_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub async fn has_active_scan_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query_scalar::<_, Uuid>(
        r#"
SELECT id
FROM scans
WHERE tenant_id = $1 AND user_id = $2 AND status IN ('queued', 'running')
FOR UPDATE
LIMIT 1
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_enums_roundtrip_strings() {
        assert_eq!(ScanStatus::Queued.as_str(), "queued");
        assert_eq!(ScanPhase::Flagging.as_str(), "flagging");
    }
}
