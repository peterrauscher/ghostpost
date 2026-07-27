//! `account.purge` checkpointed workflow (Plan 003 baseline local hook).

use crate::auth::provider::WorkosIdentityProvider;
use crate::auth::store;
use crate::error::{AppError, AppResult};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PurgePayload {
    user_id: Uuid,
    deleted_at: chrono::DateTime<Utc>,
    #[serde(default)]
    local_purged_at: Option<chrono::DateTime<Utc>>,
    #[serde(default)]
    provider_purged_at: Option<chrono::DateTime<Utc>>,
}

pub async fn handle_account_purge(
    pool: &PgPool,
    provider: Arc<dyn WorkosIdentityProvider>,
    tenant_id: Uuid,
    work_item_id: Uuid,
    payload: JsonValue,
) -> AppResult<()> {
    let mut payload: PurgePayload = serde_json::from_value(payload)
        .map_err(|err| AppError::Worker(format!("invalid account.purge payload: {err}")))?;

    // Never log workos user id. Load for provider call only.
    let user = store::get_user(pool, tenant_id, payload.user_id)
        .await?
        .ok_or_else(|| AppError::Worker("purge user tombstone missing".into()))?;
    let workos_user_id = user.workos_user_id.clone();

    if payload.local_purged_at.is_none() {
        store::local_purge_profile(pool, tenant_id, payload.user_id).await?;
        payload.local_purged_at = Some(Utc::now());
        persist_payload(pool, tenant_id, work_item_id, &payload).await?;
        info!(user_id = %payload.user_id, "account.purge local checkpoint complete");
    }

    if payload.provider_purged_at.is_none() {
        if let Some(workos_user_id) = workos_user_id.as_deref() {
            provider
                .delete_user(workos_user_id)
                .await
                .map_err(|err| AppError::Worker(format!("provider delete failed: {err}")))?;
        } else {
            warn!(user_id = %payload.user_id, "account.purge missing provider id; treating as purged");
        }
        payload.provider_purged_at = Some(Utc::now());
        persist_payload(pool, tenant_id, work_item_id, &payload).await?;
        info!(user_id = %payload.user_id, "account.purge provider checkpoint complete");
    }

    // Scrub payload after both checkpoints.
    let scrubbed = serde_json::json!({
        "user_id": payload.user_id,
        "deleted_at": payload.deleted_at,
        "local_purged_at": payload.local_purged_at,
        "provider_purged_at": payload.provider_purged_at,
        "scrubbed": true,
    });
    sqlx::query(
        r#"
UPDATE work_items
SET payload = $3, updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(work_item_id)
    .bind(scrubbed)
    .execute(pool)
    .await?;

    Ok(())
}

async fn persist_payload(
    pool: &PgPool,
    tenant_id: Uuid,
    work_item_id: Uuid,
    payload: &PurgePayload,
) -> AppResult<()> {
    let value = serde_json::to_value(payload)
        .map_err(|err| AppError::Worker(format!("serialize purge payload: {err}")))?;
    sqlx::query(
        r#"
UPDATE work_items
SET payload = $3, updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(work_item_id)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub fn emit_purge_alarm(tenant_id: Uuid, work_item_id: Uuid, detail: &str) {
    error!(
        tenant_id = %tenant_id,
        work_item_id = %work_item_id,
        detail = %detail,
        alarm = "account_purge_terminal_failure",
        "account.purge terminal failure; manual requeue required"
    );
}
