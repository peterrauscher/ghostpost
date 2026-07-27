//! `purge_retention_metadata` — expire idempotency rows and related retention.
//! Idempotent; no HTTP routes.
use crate::error::{AppError, AppResult};
use crate::repository::work_items::WorkItem;
use sqlx::PgPool;

pub async fn handle(pool: &PgPool, _item: &WorkItem) -> AppResult<()> {
    sweep_once(pool).await.map(|_| ())
}

/// Shared sweeper used by scheduled ticks and the work-item handler.
pub async fn sweep_once(pool: &PgPool) -> AppResult<u64> {
    let idem = sqlx::query(
        r#"
DELETE FROM idempotency_keys
WHERE created_at < now() - interval '24 hours'
"#,
    )
    .execute(pool)
    .await
    .map_err(AppError::from)?
    .rows_affected();

    // Soft-deleted content older than 24h that still has pending marker — ensure purge.
    let pending = sqlx::query(
        r#"
UPDATE content_items
SET body = NULL,
    title = NULL,
    purged_at = COALESCE(purged_at, now()),
    updated_at = now()
WHERE deletion_pending_at IS NOT NULL
  AND purged_at IS NULL
  AND deletion_pending_at < now() - interval '24 hours'
"#,
    )
    .execute(pool)
    .await
    .map_err(AppError::from)?
    .rows_affected();

    Ok(idem + pending)
}

#[cfg(test)]
mod tests {
    #[test]
    fn retention_module_links() {
        assert!(true);
    }
}
