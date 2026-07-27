use sqlx::PgPool;
use tracing::info;

/// Marks exhausted pending attempts as failed and requeues or fails stale leases.
///
/// Running rows are never failed by the max-attempt path while their lease is
/// still unexpired; only the stale-lease path may recover them after expiry.
pub async fn sweep_once(pool: &PgPool) -> Result<(u64, u64), sqlx::Error> {
    let mut tx = pool.begin().await?;

    let max_attempt = sqlx::query(
        r#"
UPDATE work_items
SET status = 'failed',
    finished_at = COALESCE(finished_at, now()),
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now(),
    last_error = COALESCE(last_error, 'max attempts exceeded')
WHERE attempt_count >= max_attempts
  AND status = 'pending'
"#,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // Stale running leases: revert to pending if budget remains, else failed.
    // Close open attempts atomically with the same outcome semantics.
    let stale = sqlx::query_scalar::<_, i64>(
        r#"
WITH stale AS (
  SELECT tenant_id, id
  FROM work_items
  WHERE status = 'running'
    AND lease_expires_at IS NOT NULL
    AND lease_expires_at < now()
  FOR UPDATE
),
updated AS (
  UPDATE work_items wi
  SET status = CASE
        WHEN attempt_count >= max_attempts THEN 'failed'
        ELSE 'pending'
      END,
      finished_at = CASE
        WHEN attempt_count >= max_attempts THEN COALESCE(finished_at, now())
        ELSE NULL
      END,
      lease_owner = NULL,
      lease_expires_at = NULL,
      heartbeat_at = NULL,
      run_after = CASE
        WHEN attempt_count >= max_attempts THEN run_after
        ELSE now()
      END,
      updated_at = now(),
      last_error = CASE
        WHEN attempt_count >= max_attempts THEN COALESCE(last_error, 'stale lease exhausted')
        ELSE COALESCE(last_error, 'stale lease recovered')
      END
  FROM stale s
  WHERE wi.tenant_id = s.tenant_id AND wi.id = s.id
  RETURNING wi.tenant_id, wi.id
),
closed AS (
  UPDATE work_item_attempts a
  SET outcome = 'lost_lease',
      finished_at = COALESCE(a.finished_at, now())
  FROM updated u
  WHERE a.tenant_id = u.tenant_id
    AND a.work_item_id = u.id
    AND a.finished_at IS NULL
)
SELECT count(*)::bigint FROM updated
"#,
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    let stale = stale as u64;
    if max_attempt > 0 || stale > 0 {
        info!(max_attempt, stale, "work item sweeper tick");
    }
    Ok((max_attempt, stale))
}
