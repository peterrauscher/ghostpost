use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    AwaitingUpload,
    Uploaded,
    Queued,
    Parsing,
    Normalizing,
    Ready,
    Failed,
    Rejected,
    Cancelled,
    Deleting,
    Deleted,
}

impl ImportStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingUpload => "awaiting_upload",
            Self::Uploaded => "uploaded",
            Self::Queued => "queued",
            Self::Parsing => "parsing",
            Self::Normalizing => "normalizing",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
            Self::Deleting => "deleting",
            Self::Deleted => "deleted",
        }
    }
}

impl fmt::Display for ImportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArchiveImport {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub platform: String,
    pub status: String,
    pub item_count: Option<i32>,
    pub error_code: Option<String>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub lease_owner: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ContentItem {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub platform: String,
    pub kind: String,
    pub authorship: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn create_import(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    platform: &str,
) -> Result<ArchiveImport, sqlx::Error> {
    sqlx::query_as::<_, ArchiveImport>(
        r#"
INSERT INTO archive_imports (tenant_id, user_id, platform)
VALUES ($1, $2, $3)
RETURNING tenant_id, id, user_id, platform, status, item_count, error_code,
          cancel_requested_at, lease_owner, lease_expires_at, created_at, updated_at, finished_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(platform)
    .fetch_one(pool)
    .await
}

pub async fn request_cancel(
    pool: &PgPool,
    tenant_id: Uuid,
    import_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
UPDATE archive_imports
SET cancel_requested_at = COALESCE(cancel_requested_at, now()),
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(import_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn insert_content_item(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    platform: &str,
    kind: &str,
    authorship: &str,
    body: Option<&str>,
) -> Result<ContentItem, sqlx::Error> {
    sqlx::query_as::<_, ContentItem>(
        r#"
INSERT INTO content_items (tenant_id, user_id, platform, kind, authorship, body)
VALUES ($1, $2, $3, $4, $5, $6)
RETURNING tenant_id, id, user_id, platform, kind, authorship, title, body, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(platform)
    .bind(kind)
    .bind(authorship)
    .bind(body)
    .fetch_one(pool)
    .await
}
