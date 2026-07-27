use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FlaggedPost {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub scan_id: Uuid,
    pub content_item_id: Uuid,
    pub user_id: Uuid,
    pub risk_level: String,
    pub category: Option<String>,
    pub reason_summary: Option<String>,
    pub evidence: Option<String>,
    pub review_status: String,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hidden_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FlagListRow {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub scan_id: Uuid,
    pub content_item_id: Uuid,
    pub user_id: Uuid,
    pub risk_level: String,
    pub category: Option<String>,
    pub reason_summary: Option<String>,
    pub evidence: Option<String>,
    pub review_status: String,
    pub closed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub hidden_at: Option<DateTime<Utc>>,
    pub platform: String,
    pub body: Option<String>,
    pub content_created_at: DateTime<Utc>,
}

pub async fn insert_flag(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    content_item_id: Uuid,
    user_id: Uuid,
    risk_level: &str,
) -> Result<FlaggedPost, sqlx::Error> {
    sqlx::query_as::<_, FlaggedPost>(
        r#"
INSERT INTO flagged_posts (tenant_id, scan_id, content_item_id, user_id, risk_level)
VALUES ($1, $2, $3, $4, $5)
RETURNING tenant_id, id, scan_id, content_item_id, user_id, risk_level, category,
          reason_summary, evidence, review_status, closed_at, created_at, updated_at, hidden_at
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(content_item_id)
    .bind(user_id)
    .bind(risk_level)
    .fetch_one(pool)
    .await
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ReviewAction {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub flagged_post_id: Uuid,
    pub user_id: Uuid,
    pub action: String,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_review_action(
    pool: &PgPool,
    tenant_id: Uuid,
    flagged_post_id: Uuid,
    user_id: Uuid,
    action: &str,
) -> Result<ReviewAction, sqlx::Error> {
    sqlx::query_as::<_, ReviewAction>(
        r#"
INSERT INTO review_actions (tenant_id, flagged_post_id, user_id, action)
VALUES ($1, $2, $3, $4)
RETURNING tenant_id, id, flagged_post_id, user_id, action, created_at
"#,
    )
    .bind(tenant_id)
    .bind(flagged_post_id)
    .bind(user_id)
    .bind(action)
    .fetch_one(pool)
    .await
}

pub async fn insert_flag_full(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    content_item_id: Uuid,
    user_id: Uuid,
    risk_level: &str,
    category: Option<&str>,
    reason_summary: Option<&str>,
    evidence: Option<&str>,
) -> Result<FlaggedPost, sqlx::Error> {
    sqlx::query_as::<_, FlaggedPost>(
        r#"
INSERT INTO flagged_posts (
  tenant_id, scan_id, content_item_id, user_id, risk_level,
  category, reason_summary, evidence
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
RETURNING tenant_id, id, scan_id, content_item_id, user_id, risk_level, category,
          reason_summary, evidence, review_status, closed_at, created_at, updated_at, hidden_at
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(content_item_id)
    .bind(user_id)
    .bind(risk_level)
    .bind(category)
    .bind(reason_summary)
    .bind(evidence)
    .fetch_one(pool)
    .await
}

const FLAG_SELECT: &str = r#"
SELECT f.tenant_id, f.id, f.scan_id, f.content_item_id, f.user_id, f.risk_level, f.category,
       f.reason_summary, f.evidence, f.review_status, f.closed_at, f.created_at, f.updated_at,
       f.hidden_at, c.platform, c.body, c.created_at AS content_created_at
FROM flagged_posts f
JOIN content_items c
  ON c.tenant_id = f.tenant_id AND c.id = f.content_item_id
"#;

pub async fn get_flag_detail(
    pool: &PgPool,
    tenant_id: Uuid,
    flag_id: Uuid,
) -> Result<Option<FlagListRow>, sqlx::Error> {
    let sql = format!(
        "{FLAG_SELECT}
WHERE f.tenant_id = $1 AND f.id = $2 AND f.hidden_at IS NULL
  AND (c.purged_at IS NULL)
"
    );
    sqlx::query_as::<_, FlagListRow>(&sql)
        .bind(tenant_id)
        .bind(flag_id)
        .fetch_optional(pool)
        .await
}

pub async fn list_flags(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    risk: Option<&str>,
    status: &str,
    cursor: Option<Uuid>,
    limit: i64,
) -> Result<Vec<FlagListRow>, sqlx::Error> {
    let sql = format!(
        "{FLAG_SELECT}
WHERE f.tenant_id = $1
  AND f.scan_id = $2
  AND f.hidden_at IS NULL
  AND c.purged_at IS NULL
  AND ($3::text IS NULL OR f.risk_level = $3)
  AND f.review_status = $4
  AND ($5::uuid IS NULL OR f.id > $5)
ORDER BY
  CASE f.risk_level WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END,
  f.created_at DESC,
  f.id ASC
LIMIT $6
"
    );
    sqlx::query_as::<_, FlagListRow>(&sql)
        .bind(tenant_id)
        .bind(scan_id)
        .bind(risk)
        .bind(status)
        .bind(cursor)
        .bind(limit)
        .fetch_all(pool)
        .await
}

pub async fn count_flags_by_risk(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    status: &str,
) -> Result<(i64, i64, i64, i64), sqlx::Error> {
    let row = sqlx::query!(
        r#"
SELECT
  count(*) FILTER (WHERE true)::bigint AS "all!",
  count(*) FILTER (WHERE risk_level = 'high')::bigint AS "high!",
  count(*) FILTER (WHERE risk_level = 'medium')::bigint AS "medium!",
  count(*) FILTER (WHERE risk_level = 'low')::bigint AS "low!"
FROM flagged_posts
WHERE tenant_id = $1
  AND scan_id = $2
  AND review_status = $3
  AND hidden_at IS NULL
"#,
        tenant_id,
        scan_id,
        status,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.all, row.high, row.medium, row.low))
}

pub async fn list_open_preview(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    limit: i64,
) -> Result<Vec<FlagListRow>, sqlx::Error> {
    list_flags(pool, tenant_id, scan_id, None, "open", None, limit).await
}

pub async fn lock_flag_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    flag_id: Uuid,
) -> Result<Option<FlaggedPost>, sqlx::Error> {
    sqlx::query_as::<_, FlaggedPost>(
        r#"
SELECT tenant_id, id, scan_id, content_item_id, user_id, risk_level, category,
       reason_summary, evidence, review_status, closed_at, created_at, updated_at, hidden_at
FROM flagged_posts
WHERE tenant_id = $1 AND id = $2
FOR UPDATE
"#,
    )
    .bind(tenant_id)
    .bind(flag_id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn apply_review_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    flag_id: Uuid,
    user_id: Uuid,
    action: &str,
    new_status: &str,
) -> Result<FlaggedPost, sqlx::Error> {
    sqlx::query_as::<_, ReviewAction>(
        r#"
INSERT INTO review_actions (tenant_id, flagged_post_id, user_id, action)
VALUES ($1, $2, $3, $4)
RETURNING tenant_id, id, flagged_post_id, user_id, action, created_at
"#,
    )
    .bind(tenant_id)
    .bind(flag_id)
    .bind(user_id)
    .bind(action)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query_as::<_, FlaggedPost>(
        r#"
UPDATE flagged_posts
SET review_status = $3,
    closed_at = now(),
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
RETURNING tenant_id, id, scan_id, content_item_id, user_id, risk_level, category,
          reason_summary, evidence, review_status, closed_at, created_at, updated_at, hidden_at
"#,
    )
    .bind(tenant_id)
    .bind(flag_id)
    .bind(new_status)
    .fetch_one(&mut **tx)
    .await
}

/// Mark content deletion pending and hide all flags on that content (delete_local).
pub async fn mark_delete_local_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    content_item_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE content_items
SET deletion_pending_at = COALESCE(deletion_pending_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(content_item_id)
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        r#"
UPDATE flagged_posts
SET hidden_at = COALESCE(hidden_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND content_item_id = $2 AND hidden_at IS NULL
"#,
    )
    .bind(tenant_id)
    .bind(content_item_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn highest_open_risk(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
) -> Result<(i64, Option<String>), sqlx::Error> {
    let row = sqlx::query!(
        r#"
SELECT
  count(*)::bigint AS "count!",
  (array_agg(risk_level ORDER BY
     CASE risk_level WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END
   ))[1] AS top_risk
FROM flagged_posts
WHERE tenant_id = $1
  AND scan_id = $2
  AND review_status = 'open'
  AND hidden_at IS NULL
"#,
        tenant_id,
        scan_id,
    )
    .fetch_one(pool)
    .await?;
    Ok((row.count, row.top_risk))
}
