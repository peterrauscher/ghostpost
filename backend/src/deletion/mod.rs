use crate::auth::{store, WorkosIdentityProvider};
use crate::blob::{AccountDeletionMarker, BlobError, BlobStore, ProviderDeletionReceipt};
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use subtle::ConstantTimeEq;

fn worker(message: impl Into<String>) -> AppError {
    AppError::Worker(message.into())
}
fn blob_error(error: BlobError) -> AppError {
    worker(format!("deletion replay storage failure: {error}"))
}

pub async fn verify_restore_guard(pool: &PgPool, token: &str) -> AppResult<()> {
    if token.is_empty() {
        return Err(worker("RESTORE_GUARD_TOKEN is required"));
    }
    let rows: Vec<Vec<u8>> =
        sqlx::query_scalar("SELECT token_hash FROM ghostpost_restore_guard LIMIT 2")
            .fetch_all(pool)
            .await?;
    if rows.len() != 1 {
        return Err(worker("restore guard must contain exactly one token hash"));
    }
    let actual = Sha256::digest(token.as_bytes());
    if rows[0].as_slice().ct_eq(actual.as_slice()).unwrap_u8() != 1 {
        return Err(worker("restore guard token mismatch"));
    }
    Ok(())
}

async fn delete_archive_versions(
    pool: &PgPool,
    blob: &dyn BlobStore,
    marker: &AccountDeletionMarker,
) -> AppResult<()> {
    let rows = sqlx::query("SELECT raw_storage_key, raw_storage_version_id FROM archive_imports WHERE tenant_id=$1 AND user_id=$2")
        .bind(marker.tenant_id).bind(marker.user_id).fetch_all(pool).await?;
    for row in rows {
        let key: Option<String> = row.try_get("raw_storage_key")?;
        let version: Option<String> = row.try_get("raw_storage_version_id")?;
        if let (Some(key), Some(version)) = (key, version) {
            match blob.delete_version(&key, &version).await {
                Ok(()) | Err(BlobError::NotFound) => {}
                Err(error) => return Err(blob_error(error)),
            }
        }
    }
    Ok(())
}

pub async fn replay(
    pool: &PgPool,
    blob: Arc<dyn BlobStore>,
    provider: Arc<dyn WorkosIdentityProvider>,
    restore_point: DateTime<Utc>,
    guard_token: &str,
) -> AppResult<u64> {
    verify_restore_guard(pool, guard_token).await?;
    let markers = blob
        .list_deletion_markers(restore_point)
        .await
        .map_err(blob_error)?;
    let mut replayed = 0_u64;
    for marker in markers {
        if marker.schema_version != 1 {
            return Err(worker("unsupported deletion marker schema"));
        }
        let provider_complete = blob.provider_complete(&marker).await.map_err(blob_error)?;
        let user = store::get_user(pool, marker.tenant_id, marker.user_id).await?;
        if !provider_complete {
            let user = user.as_ref().ok_or_else(|| {
                worker("deletion marker has no provider receipt or restored user row")
            })?;
            let workos_user_id = user
                .workos_user_id
                .as_deref()
                .ok_or_else(|| worker("restored user is missing WorkOS identity"))?;
            provider
                .delete_user(workos_user_id)
                .await
                .map_err(|error| worker(format!("provider delete failed: {error}")))?;
            blob.record_provider_complete(
                &marker,
                &ProviderDeletionReceipt {
                    schema_version: 1,
                    tenant_id: marker.tenant_id,
                    user_id: marker.user_id,
                    completed_at: Utc::now(),
                },
            )
            .await
            .map_err(blob_error)?;
        }
        if user.is_some() {
            delete_archive_versions(pool, blob.as_ref(), &marker).await?;
            store::local_purge_profile(pool, marker.tenant_id, marker.user_id).await?;
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM work_items WHERE tenant_id=$1 AND subject_user_id=$2")
                .bind(marker.tenant_id)
                .bind(marker.user_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM users WHERE tenant_id=$1 AND id=$2")
                .bind(marker.tenant_id)
                .bind(marker.user_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        let remaining: i64 =
            sqlx::query_scalar("SELECT count(*) FROM users WHERE tenant_id=$1 AND id=$2")
                .bind(marker.tenant_id)
                .bind(marker.user_id)
                .fetch_one(pool)
                .await?;
        if remaining != 0 {
            return Err(worker("restored user remains readable after replay"));
        }
        replayed += 1;
    }
    Ok(replayed)
}
