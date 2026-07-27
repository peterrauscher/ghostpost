//! Cancellation propagation and recovery.
//!
//! - Setting `cancel_requested_at` must not delete rows inline.
//! - Worker checks cancel after claim and commits `cancelled`.
//! - Recovery: imports/scans stuck in running with expired lease and cancel → terminal cancelled.
//! - Recovery: imports/scans running with expired lease and no cancel → resumable
//!   (`queued` for imports, `queued` for scans).

use sqlx::PgPool;
use tracing::info;

pub async fn recover_once(pool: &PgPool) -> Result<(u64, u64, u64), sqlx::Error> {
    let mut tx = pool.begin().await?;

    // Cancel work items and close open attempts atomically.
    let work = sqlx::query_scalar::<_, i64>(
        r#"
WITH targets AS (
  SELECT tenant_id, id
  FROM work_items
  WHERE cancel_requested_at IS NOT NULL
    AND status IN ('pending', 'running')
    AND (
      status = 'pending'
      OR (status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at < now())
    )
  FOR UPDATE
),
updated AS (
  UPDATE work_items wi
  SET status = 'cancelled',
      finished_at = COALESCE(finished_at, now()),
      lease_owner = NULL,
      lease_expires_at = NULL,
      updated_at = now()
  FROM targets t
  WHERE wi.tenant_id = t.tenant_id AND wi.id = t.id
  RETURNING wi.tenant_id, wi.id
),
closed AS (
  UPDATE work_item_attempts a
  SET outcome = 'cancelled',
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
    .await? as u64;

    let scans_cancel = sqlx::query(
        r#"
UPDATE scans
SET status = 'cancelled',
    finished_at = COALESCE(finished_at, now()),
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now()
WHERE cancel_requested_at IS NOT NULL
  AND status IN ('queued', 'running')
  AND (
    status = 'queued'
    OR (status = 'running' AND lease_expires_at IS NOT NULL AND lease_expires_at < now())
  )
"#,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let scans_resume = sqlx::query(
        r#"
UPDATE scans
SET status = 'queued',
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now()
WHERE cancel_requested_at IS NULL
  AND status = 'running'
  AND lease_expires_at IS NOT NULL
  AND lease_expires_at < now()
"#,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let imports_cancel = sqlx::query(
        r#"
UPDATE archive_imports
SET status = 'cancelled',
    finished_at = COALESCE(finished_at, now()),
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now()
WHERE cancel_requested_at IS NOT NULL
  AND status IN ('queued', 'parsing', 'normalizing')
  AND (
    lease_expires_at IS NULL
    OR lease_expires_at < now()
  )
"#,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    let imports_resume = sqlx::query(
        r#"
UPDATE archive_imports
SET status = 'queued',
    lease_owner = NULL,
    lease_expires_at = NULL,
    updated_at = now()
WHERE cancel_requested_at IS NULL
  AND status IN ('parsing', 'normalizing')
  AND lease_expires_at IS NOT NULL
  AND lease_expires_at < now()
"#,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();

    tx.commit().await?;

    let total_scans = scans_cancel + scans_resume;
    let total_imports = imports_cancel + imports_resume;
    if work > 0 || total_scans > 0 || total_imports > 0 {
        info!(
            work,
            scans_cancel,
            scans_resume,
            imports_cancel,
            imports_resume,
            "cancellation recovery tick"
        );
    }
    Ok((work, total_scans, total_imports))
}
