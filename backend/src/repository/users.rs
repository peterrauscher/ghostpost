use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct User {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub workos_user_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn create_tenant(pool: &PgPool) -> Result<Tenant, sqlx::Error> {
    let row = sqlx::query!(
        r#"
INSERT INTO tenants DEFAULT VALUES
RETURNING id, created_at
"#
    )
    .fetch_one(pool)
    .await?;
    Ok(Tenant {
        id: row.id,
        created_at: row.created_at,
    })
}

pub async fn create_user(
    pool: &PgPool,
    tenant_id: Uuid,
    workos_user_id: Option<&str>,
    email: Option<&str>,
    display_name: Option<&str>,
) -> Result<User, sqlx::Error> {
    let row = sqlx::query!(
        r#"
INSERT INTO users (tenant_id, workos_user_id, email, display_name)
VALUES ($1, $2, $3, $4)
RETURNING tenant_id, id, workos_user_id, email, display_name, created_at, updated_at
"#,
        tenant_id,
        workos_user_id,
        email,
        display_name,
    )
    .fetch_one(pool)
    .await?;
    Ok(User {
        tenant_id: row.tenant_id,
        id: row.id,
        workos_user_id: row.workos_user_id,
        email: row.email.map(|e| e.to_string()),
        display_name: row.display_name,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

pub async fn get_user(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<User>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
SELECT tenant_id, id, workos_user_id, email, display_name, created_at, updated_at
FROM users
WHERE tenant_id = $1 AND id = $2
"#,
        tenant_id,
        user_id,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| User {
        tenant_id: row.tenant_id,
        id: row.id,
        workos_user_id: row.workos_user_id,
        email: row.email.map(|e| e.to_string()),
        display_name: row.display_name,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}
