use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuthSession {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: Vec<u8>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn insert_session(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    token_hash: &[u8],
    expires_at: DateTime<Utc>,
) -> Result<AuthSession, sqlx::Error> {
    sqlx::query_as::<_, AuthSession>(
        r#"
INSERT INTO auth_sessions (tenant_id, user_id, token_hash, expires_at)
VALUES ($1, $2, $3, $4)
RETURNING tenant_id, id, user_id, token_hash, expires_at, revoked_at, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await
}

pub async fn find_active_by_hash(
    pool: &PgPool,
    tenant_id: Uuid,
    token_hash: &[u8],
) -> Result<Option<AuthSession>, sqlx::Error> {
    sqlx::query_as::<_, AuthSession>(
        r#"
SELECT tenant_id, id, user_id, token_hash, expires_at, revoked_at, created_at, updated_at
FROM auth_sessions
WHERE tenant_id = $1
  AND token_hash = $2
  AND revoked_at IS NULL
  AND expires_at > now()
"#,
    )
    .bind(tenant_id)
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}
