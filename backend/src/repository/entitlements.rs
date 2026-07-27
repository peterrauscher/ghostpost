use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EntitlementGrant {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub product_key: String,
    pub grant_source: String,
    pub review_access: bool,
    pub rescan_limit: Option<i32>,
    pub platform_limit: Option<i32>,
    pub scan_scope_id: Option<Uuid>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

fn map_grant(
    tenant_id: Uuid,
    id: Uuid,
    user_id: Uuid,
    product_key: String,
    grant_source: String,
    review_access: bool,
    rescan_limit: Option<i32>,
    platform_limit: Option<i32>,
    scan_scope_id: Option<Uuid>,
    valid_from: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
) -> EntitlementGrant {
    EntitlementGrant {
        tenant_id,
        id,
        user_id,
        product_key,
        grant_source,
        review_access,
        rescan_limit,
        platform_limit,
        scan_scope_id,
        valid_from,
        valid_until,
        revoked_at,
        created_at,
    }
}

/// Storage-only free_beta grant helper for tests and later plans. No HTTP.
pub async fn grant_free_beta(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<EntitlementGrant, sqlx::Error> {
    if let Some(existing) = get_active_grant(pool, tenant_id, user_id, "free_beta").await? {
        return Ok(existing);
    }
    let row = sqlx::query!(
        r#"
INSERT INTO entitlement_grants (
  tenant_id, user_id, product_key, grant_source, review_access, rescan_limit, platform_limit
)
VALUES ($1, $2, 'free_beta', 'test', true, NULL, 2)
RETURNING tenant_id, id, user_id, product_key, grant_source, review_access, rescan_limit,
          platform_limit, scan_scope_id, valid_from, valid_until, revoked_at, created_at
"#,
        tenant_id,
        user_id,
    )
    .fetch_one(pool)
    .await?;
    Ok(map_grant(
        row.tenant_id,
        row.id,
        row.user_id,
        row.product_key,
        row.grant_source,
        row.review_access,
        row.rescan_limit,
        row.platform_limit,
        row.scan_scope_id,
        row.valid_from,
        row.valid_until,
        row.revoked_at,
        row.created_at,
    ))
}

/// Signup hook: promotion-sourced free_beta (idempotent).
pub async fn grant_free_beta_promotion_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<EntitlementGrant, sqlx::Error> {
    if let Some(existing) = get_active_grant_tx(tx, tenant_id, user_id, "free_beta").await? {
        return Ok(existing);
    }
    let row = sqlx::query!(
        r#"
INSERT INTO entitlement_grants (
  tenant_id, user_id, product_key, grant_source, review_access, rescan_limit, platform_limit
)
VALUES ($1, $2, 'free_beta', 'promotion', true, NULL, 2)
RETURNING tenant_id, id, user_id, product_key, grant_source, review_access, rescan_limit,
          platform_limit, scan_scope_id, valid_from, valid_until, revoked_at, created_at
"#,
        tenant_id,
        user_id,
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(map_grant(
        row.tenant_id,
        row.id,
        row.user_id,
        row.product_key,
        row.grant_source,
        row.review_access,
        row.rescan_limit,
        row.platform_limit,
        row.scan_scope_id,
        row.valid_from,
        row.valid_until,
        row.revoked_at,
        row.created_at,
    ))
}

pub async fn get_active_grant(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    product_key: &str,
) -> Result<Option<EntitlementGrant>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
SELECT tenant_id, id, user_id, product_key, grant_source, review_access, rescan_limit,
       platform_limit, scan_scope_id, valid_from, valid_until, revoked_at, created_at
FROM entitlement_grants
WHERE tenant_id = $1
  AND user_id = $2
  AND product_key = $3
  AND revoked_at IS NULL
  AND (valid_until IS NULL OR valid_until > now())
ORDER BY created_at DESC
LIMIT 1
"#,
        tenant_id,
        user_id,
        product_key,
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| {
        map_grant(
            row.tenant_id,
            row.id,
            row.user_id,
            row.product_key,
            row.grant_source,
            row.review_access,
            row.rescan_limit,
            row.platform_limit,
            row.scan_scope_id,
            row.valid_from,
            row.valid_until,
            row.revoked_at,
            row.created_at,
        )
    }))
}

pub async fn get_active_grant_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    product_key: &str,
) -> Result<Option<EntitlementGrant>, sqlx::Error> {
    let row = sqlx::query!(
        r#"
SELECT tenant_id, id, user_id, product_key, grant_source, review_access, rescan_limit,
       platform_limit, scan_scope_id, valid_from, valid_until, revoked_at, created_at
FROM entitlement_grants
WHERE tenant_id = $1
  AND user_id = $2
  AND product_key = $3
  AND revoked_at IS NULL
  AND (valid_until IS NULL OR valid_until > now())
ORDER BY created_at DESC
LIMIT 1
"#,
        tenant_id,
        user_id,
        product_key,
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|row| {
        map_grant(
            row.tenant_id,
            row.id,
            row.user_id,
            row.product_key,
            row.grant_source,
            row.review_access,
            row.rescan_limit,
            row.platform_limit,
            row.scan_scope_id,
            row.valid_from,
            row.valid_until,
            row.revoked_at,
            row.created_at,
        )
    }))
}

pub async fn count_usages(
    pool: &PgPool,
    tenant_id: Uuid,
    entitlement_id: Uuid,
    usage_kind: &str,
) -> Result<i64, sqlx::Error> {
    let n = sqlx::query_scalar!(
        r#"
SELECT count(*)::bigint AS "count!"
FROM entitlement_usages
WHERE tenant_id = $1 AND entitlement_id = $2 AND usage_kind = $3
"#,
        tenant_id,
        entitlement_id,
        usage_kind,
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn count_usages_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    entitlement_id: Uuid,
    usage_kind: &str,
) -> Result<i64, sqlx::Error> {
    let n = sqlx::query_scalar!(
        r#"
SELECT count(*)::bigint AS "count!"
FROM entitlement_usages
WHERE tenant_id = $1 AND entitlement_id = $2 AND usage_kind = $3
"#,
        tenant_id,
        entitlement_id,
        usage_kind,
    )
    .fetch_one(&mut **tx)
    .await?;
    Ok(n)
}

pub async fn insert_usage_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    entitlement_id: Uuid,
    user_id: Uuid,
    scan_id: Uuid,
    usage_kind: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        r#"
INSERT INTO entitlement_usages (tenant_id, entitlement_id, user_id, scan_id, usage_kind)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (tenant_id, entitlement_id, scan_id, usage_kind) DO NOTHING
"#,
        tenant_id,
        entitlement_id,
        user_id,
        scan_id,
        usage_kind,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}
