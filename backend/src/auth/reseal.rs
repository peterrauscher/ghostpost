//! Admin reseal of active WorkOS sessions under the current seal key version.

use crate::auth::config::AuthConfig;
use crate::auth::crypto::{open_online_sealed, seal_bytes};
use crate::auth::service::SealedWorkosPayload;
use crate::auth::store;
use crate::error::{AppError, AppResult};
use sqlx::PgPool;
use tracing::info;

pub async fn reseal_workos_sessions(pool: &PgPool, config: &AuthConfig) -> AppResult<u64> {
    let rows = store::list_active_workos_sessions_for_reseal(pool).await?;
    let mut updated = 0u64;
    for row in rows {
        if row.seal_key_version == config.workos_seal_key_version {
            continue;
        }
        // Shared online unseal: current WORKOS_COOKIE_PASSWORD, then PREVIOUS.
        let plaintext = open_online_sealed(&config.workos_seal_key, &row.sealed_session).map_err(|err| {
            AppError::Config(format!(
                "cannot open workos_session {}: {err}; set WORKOS_COOKIE_PASSWORD_PREVIOUS if rotating",
                row.id
            ))
        })?;
        // Validate payload shape.
        let _: SealedWorkosPayload = serde_json::from_slice(&plaintext).map_err(|err| {
            AppError::Config(format!("invalid sealed payload for {}: {err}", row.id))
        })?;
        let resealed = seal_bytes(&config.workos_seal_key, &plaintext)
            .map_err(|err| AppError::Config(format!("reseal failed: {err}")))?;
        let mut tx = pool.begin().await?;
        store::update_workos_session_sealed_tx(
            &mut tx,
            row.tenant_id,
            row.id,
            &resealed,
            &config.workos_seal_key_version,
            row.workos_session_id.as_deref(),
            row.expires_at,
        )
        .await?;
        // Force version update even if same expires.
        sqlx::query(
            r#"
UPDATE workos_sessions
SET seal_key_version = $3, sealed_session = $4
WHERE tenant_id = $1 AND id = $2
"#,
        )
        .bind(row.tenant_id)
        .bind(row.id)
        .bind(&config.workos_seal_key_version)
        .bind(&resealed)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        updated += 1;
    }
    info!(updated, "resealed workos sessions");
    Ok(updated)
}
