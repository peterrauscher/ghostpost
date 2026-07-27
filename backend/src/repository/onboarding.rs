use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OnboardingProfile {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub coming_up: Vec<String>,
    pub concerns: Vec<String>,
    pub platforms: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn upsert(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    coming_up: &[String],
    concerns: &[String],
    platforms: &[String],
) -> Result<OnboardingProfile, sqlx::Error> {
    sqlx::query_as::<_, OnboardingProfile>(
        r#"
INSERT INTO onboarding_profiles (tenant_id, user_id, coming_up, concerns, platforms)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (tenant_id, user_id) DO UPDATE
SET coming_up = EXCLUDED.coming_up,
    concerns = EXCLUDED.concerns,
    platforms = EXCLUDED.platforms,
    updated_at = now()
RETURNING tenant_id, id, user_id, coming_up, concerns, platforms, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(coming_up)
    .bind(concerns)
    .bind(platforms)
    .fetch_one(pool)
    .await
}

pub async fn get(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<OnboardingProfile>, sqlx::Error> {
    sqlx::query_as::<_, OnboardingProfile>(
        r#"
SELECT tenant_id, id, user_id, coming_up, concerns, platforms, created_at, updated_at
FROM onboarding_profiles
WHERE tenant_id = $1 AND user_id = $2
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn get_by_id(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<OnboardingProfile>, sqlx::Error> {
    sqlx::query_as::<_, OnboardingProfile>(
        r#"
SELECT tenant_id, id, user_id, coming_up, concerns, platforms, created_at, updated_at
FROM onboarding_profiles
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}
