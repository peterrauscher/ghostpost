use crate::jobs::retry_backoff_secs;
use crate::repository::work_items::WorkItem;
use chrono::Duration as ChronoDuration;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use uuid::Uuid;

pub async fn claim_one(
    pool: &PgPool,
    lease_owner: &str,
    lease: Duration,
) -> Result<Option<WorkItem>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let item = claim_one_tx(&mut tx, lease_owner, lease).await?;
    tx.commit().await?;
    Ok(item)
}

pub async fn claim_one_tx(
    tx: &mut Transaction<'_, Postgres>,
    lease_owner: &str,
    lease: Duration,
) -> Result<Option<WorkItem>, sqlx::Error> {
    let lease_secs = lease.as_secs() as i64;
    let row = sqlx::query!(
        r#"
UPDATE work_items wi
SET status = 'running',
    lease_owner = $1,
    lease_expires_at = now() + make_interval(secs => $2),
    heartbeat_at = now(),
    attempt_count = attempt_count + 1,
    updated_at = now()
WHERE (tenant_id, id) = (
  SELECT tenant_id, id FROM work_items
  WHERE status = 'pending'
    AND run_after <= now()
    AND cancel_requested_at IS NULL
    AND attempt_count < max_attempts
  ORDER BY priority DESC, created_at ASC
  FOR UPDATE SKIP LOCKED
  LIMIT 1
)
RETURNING wi.tenant_id, wi.subject_user_id, wi.id, wi.kind, wi.payload, wi.dedupe_key, wi.status,
          wi.priority, wi.run_after, wi.attempt_count, wi.max_attempts, wi.lease_owner,
          wi.lease_expires_at, wi.heartbeat_at, wi.cancel_requested_at, wi.last_error,
          wi.created_at, wi.updated_at, wi.finished_at
"#,
        lease_owner,
        lease_secs as f64,
    )
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let item = WorkItem {
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
    };

    sqlx::query!(
        r#"
INSERT INTO work_item_attempts (tenant_id, work_item_id, attempt_number, lease_owner)
VALUES ($1, $2, $3, $4)
"#,
        item.tenant_id,
        item.id,
        item.attempt_count,
        lease_owner,
    )
    .execute(&mut **tx)
    .await?;

    Ok(Some(item))
}

pub async fn heartbeat(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
    lease_owner: &str,
    lease: Duration,
) -> Result<bool, sqlx::Error> {
    let lease_secs = lease.as_secs() as i64;
    let result = sqlx::query!(
        r#"
UPDATE work_items
SET heartbeat_at = now(),
    lease_expires_at = now() + make_interval(secs => $3),
    updated_at = now()
WHERE tenant_id = $1
  AND id = $2
  AND lease_owner = $4
  AND status = 'running'
"#,
        tenant_id,
        id,
        lease_secs as f64,
        lease_owner,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn commit_success(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
    lease_owner: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let ok = commit_success_tx(&mut tx, tenant_id, id, lease_owner).await?;
    tx.commit().await?;
    Ok(ok)
}

pub async fn commit_success_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    id: Uuid,
    lease_owner: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        r#"
UPDATE work_items
SET status = 'succeeded',
    finished_at = now(),
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now()
WHERE tenant_id = $1 AND id = $2 AND lease_owner = $3 AND status = 'running'
"#,
        tenant_id,
        id,
        lease_owner,
    )
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() == 1 {
        sqlx::query!(
            r#"
UPDATE work_item_attempts
SET outcome = 'succeeded', finished_at = now()
WHERE tenant_id = $1
  AND work_item_id = $2
  AND lease_owner = $3
  AND finished_at IS NULL
"#,
            tenant_id,
            id,
            lease_owner,
        )
        .execute(&mut **tx)
        .await?;
        Ok(true)
    } else {
        sqlx::query!(
            r#"
UPDATE work_item_attempts
SET outcome = 'lost_lease', finished_at = now()
WHERE tenant_id = $1
  AND work_item_id = $2
  AND lease_owner = $3
  AND finished_at IS NULL
"#,
            tenant_id,
            id,
            lease_owner,
        )
        .execute(&mut **tx)
        .await?;
        Ok(false)
    }
}

pub async fn commit_failure(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
    lease_owner: &str,
    error_text: &str,
    attempt_count: i32,
    max_attempts: i32,
) -> Result<String, sqlx::Error> {
    let truncated: String = error_text.chars().take(2000).collect();
    let mut tx = pool.begin().await?;

    let terminal = attempt_count >= max_attempts;
    let status = if terminal { "failed" } else { "pending" };
    let backoff = retry_backoff_secs(attempt_count);
    let run_after = chrono::Utc::now() + ChronoDuration::seconds(backoff);

    let result = sqlx::query!(
        r#"
UPDATE work_items
SET status = $4,
    finished_at = CASE WHEN $4 = 'failed' THEN now() ELSE NULL END,
    lease_owner = NULL,
    lease_expires_at = NULL,
    heartbeat_at = NULL,
    last_error = $5,
    run_after = CASE WHEN $4 = 'pending' THEN $6 ELSE run_after END,
    updated_at = now()
WHERE tenant_id = $1 AND id = $2 AND lease_owner = $3 AND status = 'running'
"#,
        tenant_id,
        id,
        lease_owner,
        status,
        &truncated,
        run_after,
    )
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 1 {
        let outcome = if terminal { "failed" } else { "retry" };
        sqlx::query!(
            r#"
UPDATE work_item_attempts
SET outcome = $4, error_text = $5, finished_at = now()
WHERE tenant_id = $1
  AND work_item_id = $2
  AND lease_owner = $3
  AND finished_at IS NULL
"#,
            tenant_id,
            id,
            lease_owner,
            outcome,
            &truncated,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(status.into())
    } else {
        // Fence miss: do not mutate the work item; close our open attempt as lost_lease.
        sqlx::query!(
            r#"
UPDATE work_item_attempts
SET outcome = 'lost_lease', error_text = $4, finished_at = now()
WHERE tenant_id = $1
  AND work_item_id = $2
  AND lease_owner = $3
  AND finished_at IS NULL
"#,
            tenant_id,
            id,
            lease_owner,
            &truncated,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok("lost_lease".into())
    }
}

pub async fn commit_cancelled(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
    lease_owner: &str,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        r#"
UPDATE work_items
SET status = 'cancelled',
    finished_at = now(),
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now()
WHERE tenant_id = $1 AND id = $2 AND lease_owner = $3 AND status = 'running'
"#,
        tenant_id,
        id,
        lease_owner,
    )
    .execute(&mut *tx)
    .await?;

    if result.rows_affected() == 1 {
        sqlx::query!(
            r#"
UPDATE work_item_attempts
SET outcome = 'cancelled', finished_at = now()
WHERE tenant_id = $1
  AND work_item_id = $2
  AND lease_owner = $3
  AND finished_at IS NULL
"#,
            tenant_id,
            id,
            lease_owner,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    } else {
        sqlx::query!(
            r#"
UPDATE work_item_attempts
SET outcome = 'lost_lease', finished_at = now()
WHERE tenant_id = $1
  AND work_item_id = $2
  AND lease_owner = $3
  AND finished_at IS NULL
"#,
            tenant_id,
            id,
            lease_owner,
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(false)
    }
}
