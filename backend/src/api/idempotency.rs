//! Tenant-scoped idempotency claim/complete for product mutations.
use crate::api::problem::Problem;
use axum::http::StatusCode;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug)]
pub enum IdemClaim {
    /// Caller holds the lock and must complete or abandon.
    Fresh,
    /// Prior successful response to replay.
    Replay { status: u16, body: Value },
    /// Another request holds the lock.
    InProgress,
    /// Same key, different body hash.
    Conflict,
}

pub fn request_hash(bytes: &[u8]) -> Vec<u8> {
    Sha256::digest(bytes).to_vec()
}

pub async fn claim(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    scope: &str,
    key: &str,
    hash: &[u8],
    lock_secs: i64,
) -> Result<IdemClaim, sqlx::Error> {
    // Try insert fresh claim.
    let inserted = sqlx::query(
        r#"
INSERT INTO idempotency_keys (tenant_id, scope, key, request_hash, locked_until)
VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))
ON CONFLICT (tenant_id, scope, key) DO NOTHING
"#,
    )
    .bind(tenant_id)
    .bind(scope)
    .bind(key)
    .bind(hash)
    .bind(lock_secs)
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if inserted > 0 {
        return Ok(IdemClaim::Fresh);
    }

    let row = sqlx::query!(
        r#"
SELECT request_hash, response_status, response_body, locked_until, resource_id
FROM idempotency_keys
WHERE tenant_id = $1 AND scope = $2 AND key = $3
FOR UPDATE
"#,
        tenant_id,
        scope,
        key,
    )
    .fetch_one(&mut **tx)
    .await?;

    if let (Some(status), Some(body)) = (row.response_status, row.response_body) {
        if row.request_hash.as_deref() != Some(hash) {
            return Ok(IdemClaim::Conflict);
        }
        return Ok(IdemClaim::Replay {
            status: status as u16,
            body,
        });
    }

    if row.request_hash.as_deref().is_some_and(|h| h != hash) {
        return Ok(IdemClaim::Conflict);
    }

    let locked = row
        .locked_until
        .map(|t| t > chrono::Utc::now())
        .unwrap_or(false);
    if locked {
        return Ok(IdemClaim::InProgress);
    }

    // Expired incomplete lock — take over.
    sqlx::query(
        r#"
UPDATE idempotency_keys
SET request_hash = $4,
    locked_until = now() + make_interval(secs => $5),
    response_status = NULL,
    response_body = NULL,
    resource_id = NULL,
    updated_at = now()
WHERE tenant_id = $1 AND scope = $2 AND key = $3
"#,
    )
    .bind(tenant_id)
    .bind(scope)
    .bind(key)
    .bind(hash)
    .bind(lock_secs)
    .execute(&mut **tx)
    .await?;

    Ok(IdemClaim::Fresh)
}

pub async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    scope: &str,
    key: &str,
    status: u16,
    body: &Value,
    resource_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE idempotency_keys
SET response_status = $4,
    response_body = $5,
    resource_id = $6,
    locked_until = NULL,
    updated_at = now()
WHERE tenant_id = $1 AND scope = $2 AND key = $3
"#,
    )
    .bind(tenant_id)
    .bind(scope)
    .bind(key)
    .bind(status as i32)
    .bind(body)
    .bind(resource_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub fn claim_problem(claim: IdemClaim) -> Result<(u16, Value), Problem> {
    match claim {
        IdemClaim::Fresh => unreachable!("caller must handle Fresh"),
        IdemClaim::Replay { status, body } => Ok((status, body)),
        IdemClaim::InProgress => Err(Problem {
            type_uri: "https://ghostpost.app/problems/request-in-progress".into(),
            title: "Conflict".into(),
            status: StatusCode::CONFLICT.as_u16(),
            detail: "A request with this Idempotency-Key is still in progress".into(),
            instance: None,
            code: Some("REQUEST_IN_PROGRESS".into()),
        }),
        IdemClaim::Conflict => Err(Problem {
            type_uri: "https://ghostpost.app/problems/idempotency-key-reused".into(),
            title: "Conflict".into(),
            status: StatusCode::CONFLICT.as_u16(),
            detail: "Idempotency-Key was reused with a different request body".into(),
            instance: None,
            code: Some("IDEMPOTENCY_KEY_REUSED".into()),
        }),
    }
}

#[allow(dead_code)]
pub async fn purge_expired(pool: &PgPool, older_than_hours: i64) -> Result<u64, sqlx::Error> {
    let n = sqlx::query(
        r#"
DELETE FROM idempotency_keys
WHERE created_at < now() - make_interval(hours => $1)
"#,
    )
    .bind(older_than_hours)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(n)
}
