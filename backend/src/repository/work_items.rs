use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl WorkItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for WorkItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct WorkItem {
    pub tenant_id: Uuid,
    pub subject_user_id: Option<Uuid>,
    pub id: Uuid,
    pub kind: String,
    pub payload: JsonValue,
    pub dedupe_key: Option<String>,
    pub status: String,
    pub priority: i32,
    pub run_after: DateTime<Utc>,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub heartbeat_at: Option<DateTime<Utc>>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

macro_rules! map_work_item {
    ($row:expr) => {{
        let row = $row;
        WorkItem {
            tenant_id: row.tenant_id,
            subject_user_id: row.subject_user_id,
            id: row.id,
            kind: row.kind,
            payload: row.payload,
            dedupe_key: row.dedupe_key,
            status: row.status,
            priority: row.priority,
            run_after: row.run_after,
            attempt_count: row.attempt_count,
            max_attempts: row.max_attempts,
            lease_owner: row.lease_owner,
            lease_expires_at: row.lease_expires_at,
            heartbeat_at: row.heartbeat_at,
            cancel_requested_at: row.cancel_requested_at,
            last_error: row.last_error,
            created_at: row.created_at,
            updated_at: row.updated_at,
            finished_at: row.finished_at,
        }
    }};
}

pub async fn enqueue(
    pool: &PgPool,
    tenant_id: Uuid,
    subject_user_id: Option<Uuid>,
    kind: &str,
    payload: JsonValue,
    dedupe_key: Option<&str>,
    priority: i32,
    max_attempts: i32,
) -> Result<WorkItem, sqlx::Error> {
    let row = sqlx::query!(
        r#"
INSERT INTO work_items (
  tenant_id, subject_user_id, kind, payload, dedupe_key, priority, max_attempts
)
VALUES ($1, $2, $3, $4, $5, $6, $7)
RETURNING tenant_id, subject_user_id, id, kind, payload, dedupe_key, status, priority,
          run_after, attempt_count, max_attempts, lease_owner, lease_expires_at, heartbeat_at,
          cancel_requested_at, last_error, created_at, updated_at, finished_at
"#,
        tenant_id,
        subject_user_id,
        kind,
        payload,
        dedupe_key,
        priority,
        max_attempts,
    )
    .fetch_one(pool)
    .await?;
    Ok(map_work_item!(row))
}

pub async fn get(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<WorkItem>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
SELECT tenant_id, subject_user_id, id, kind, payload, dedupe_key, status, priority,
       run_after, attempt_count, max_attempts, lease_owner, lease_expires_at, heartbeat_at,
       cancel_requested_at, last_error, created_at, updated_at, finished_at
FROM work_items
WHERE tenant_id = $1 AND id = $2
"#,
        tenant_id,
        id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| map_work_item!(row)))
}

pub async fn request_cancel(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        r#"
UPDATE work_items
SET cancel_requested_at = COALESCE(cancel_requested_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
        tenant_id,
        id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_item_status_strings() {
        assert_eq!(WorkItemStatus::Pending.as_str(), "pending");
        assert_eq!(WorkItemStatus::Cancelled.to_string(), "cancelled");
    }
}
