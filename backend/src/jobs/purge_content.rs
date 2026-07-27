//! `purge_content` — scrub body and hard-delete content + dependent flags.
//! Triggered by `delete_local`. No outbound HTTP.
use crate::error::{AppError, AppResult};
use crate::repository::work_items::WorkItem;
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurgeContentPayload {
    content_item_id: Uuid,
}

fn worker_err(msg: impl Into<String>) -> AppError {
    AppError::Worker(msg.into())
}

pub async fn handle(pool: &PgPool, item: &WorkItem) -> AppResult<()> {
    let payload: PurgeContentPayload = serde_json::from_value(item.payload.clone())
        .map_err(|e| worker_err(format!("invalid purge_content payload: {e}")))?;

    let tenant_id = item.tenant_id;
    let content_id = payload.content_item_id;

    let mut tx = pool.begin().await.map_err(AppError::from)?;

    // Scrub body text first (privacy), then hard-delete cascade.
    sqlx::query(
        r#"
UPDATE content_items
SET body = NULL,
    title = NULL,
    purged_at = COALESCE(purged_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(content_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    // Hide any remaining flags then delete them with the content row.
    sqlx::query(
        r#"
UPDATE flagged_posts
SET hidden_at = COALESCE(hidden_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND content_item_id = $2
"#,
    )
    .bind(tenant_id)
    .bind(content_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    sqlx::query(
        r#"
DELETE FROM flagged_posts
WHERE tenant_id = $1 AND content_item_id = $2
"#,
    )
    .bind(tenant_id)
    .bind(content_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    sqlx::query(
        r#"
DELETE FROM archive_import_content_items
WHERE tenant_id = $1 AND content_item_id = $2
"#,
    )
    .bind(tenant_id)
    .bind(content_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    sqlx::query(
        r#"
DELETE FROM content_items
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(content_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::from)?;

    tx.commit().await.map_err(AppError::from)?;
    Ok(())
}
