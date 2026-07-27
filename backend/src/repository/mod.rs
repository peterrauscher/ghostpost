pub mod archive_imports;
pub mod auth;
pub mod entitlements;
pub mod flags;
pub mod onboarding;
pub mod scans;
pub mod users;
pub mod work_items;

use sqlx::PgPool;

#[derive(Clone)]
pub struct Repositories {
    pub pool: PgPool,
}

impl Repositories {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn try_begin_idempotency(
        &self,
        tenant_id: uuid::Uuid,
        scope: &str,
        key: &str,
        lock_secs: i64,
    ) -> Result<bool, sqlx::Error> {
        let rows = sqlx::query(
            r#"
INSERT INTO idempotency_keys (tenant_id, scope, key, locked_until)
VALUES ($1, $2, $3, now() + make_interval(secs => $4))
ON CONFLICT (tenant_id, scope, key) DO UPDATE
SET locked_until = EXCLUDED.locked_until,
    updated_at = now()
WHERE idempotency_keys.locked_until IS NULL
   OR idempotency_keys.locked_until < now()
RETURNING id
"#,
        )
        .bind(tenant_id)
        .bind(scope)
        .bind(key)
        .bind(lock_secs)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(rows > 0)
    }
}
