//! Persistence helpers for auth flows, sessions, and profiles.

use crate::auth::types::AuthClient;
use chrono::{DateTime, Utc};
use serde_json::Value as JsonValue;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuthFlowRow {
    pub id: Uuid,
    pub state: String,
    pub code_verifier_hash: Vec<u8>,
    pub code_verifier_enc: Vec<u8>,
    pub exchange_secret_hash: Vec<u8>,
    pub client: String,
    pub redirect_uri: String,
    pub tenant_id: Option<Uuid>,
    pub user_id: Option<Uuid>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UserRow {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub workos_user_id: Option<String>,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub greeting_name: Option<String>,
    pub avatar_url: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuthSessionRow {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: Vec<u8>,
    pub key_id: Option<String>,
    pub client: Option<String>,
    pub csrf_hash: Option<Vec<u8>>,
    pub csrf_rotated_at: Option<DateTime<Utc>>,
    pub workos_session_id: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WorkosSessionRow {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub sealed_session: Vec<u8>,
    pub seal_key_version: String,
    pub workos_session_id: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OnboardingRow {
    pub tenant_id: Uuid,
    pub id: Uuid,
    pub user_id: Uuid,
    pub coming_up: Vec<String>,
    pub concerns: Vec<String>,
    pub platforms: Vec<String>,
    pub revision: i64,
    pub current_step: i16,
    pub consent_version: Option<String>,
    pub consent_accepted_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub async fn insert_auth_flow(
    pool: &PgPool,
    state: &str,
    code_verifier_hash: &[u8],
    code_verifier_enc: &[u8],
    exchange_secret_hash: &[u8],
    client: AuthClient,
    redirect_uri: &str,
    expires_at: DateTime<Utc>,
) -> Result<AuthFlowRow, sqlx::Error> {
    sqlx::query_as::<_, AuthFlowRow>(
        r#"
INSERT INTO auth_flows (
  state, code_verifier_hash, code_verifier_enc, exchange_secret_hash,
  client, redirect_uri, expires_at
)
VALUES ($1,$2,$3,$4,$5,$6,$7)
RETURNING *
"#,
    )
    .bind(state)
    .bind(code_verifier_hash)
    .bind(code_verifier_enc)
    .bind(exchange_secret_hash)
    .bind(client.as_str())
    .bind(redirect_uri)
    .bind(expires_at)
    .fetch_one(pool)
    .await
}

pub async fn load_auth_flow_by_state(
    pool: &PgPool,
    state: &str,
) -> Result<Option<AuthFlowRow>, sqlx::Error> {
    sqlx::query_as::<_, AuthFlowRow>(
        r#"SELECT * FROM auth_flows WHERE state = $1"#,
    )
    .bind(state)
    .fetch_optional(pool)
    .await
}

pub async fn mark_flow_consumed_tx(
    tx: &mut Transaction<'_, Postgres>,
    flow_id: Uuid,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        r#"
UPDATE auth_flows
SET consumed_at = now(), tenant_id = $2, user_id = $3
WHERE id = $1 AND consumed_at IS NULL AND expires_at > now()
"#,
    )
    .bind(flow_id)
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn find_user_by_workos_id(
    pool: &PgPool,
    workos_user_id: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
SELECT tenant_id, id, workos_user_id, email::text AS email, display_name, greeting_name,
       avatar_url, deleted_at, created_at, updated_at
FROM users
WHERE workos_user_id = $1
"#,
    )
    .bind(workos_user_id)
    .fetch_optional(pool)
    .await
}

pub async fn find_user_by_workos_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    workos_user_id: &str,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
SELECT tenant_id, id, workos_user_id, email::text AS email, display_name, greeting_name,
       avatar_url, deleted_at, created_at, updated_at
FROM users
WHERE workos_user_id = $1
"#,
    )
    .bind(workos_user_id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn get_user(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
SELECT tenant_id, id, workos_user_id, email::text AS email, display_name, greeting_name,
       avatar_url, deleted_at, created_at, updated_at
FROM users
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn create_tenant_tx(tx: &mut Transaction<'_, Postgres>) -> Result<Uuid, sqlx::Error> {
    let row: (Uuid,) = sqlx::query_as(r#"INSERT INTO tenants DEFAULT VALUES RETURNING id"#)
        .fetch_one(&mut **tx)
        .await?;
    Ok(row.0)
}

pub async fn create_user_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workos_user_id: &str,
    email: &str,
    display_name: &str,
    greeting_name: &str,
    avatar_url: Option<&str>,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
INSERT INTO users (
  tenant_id, workos_user_id, email, display_name, greeting_name, avatar_url
)
VALUES ($1,$2,$3,$4,$5,$6)
RETURNING tenant_id, id, workos_user_id, email::text AS email, display_name, greeting_name,
          avatar_url, deleted_at, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(workos_user_id)
    .bind(email)
    .bind(display_name)
    .bind(greeting_name)
    .bind(avatar_url)
    .fetch_one(&mut **tx)
    .await
}

pub async fn update_user_profile_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    email: &str,
    display_name: &str,
    greeting_name: &str,
    avatar_url: Option<&str>,
) -> Result<UserRow, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
UPDATE users
SET email = $3,
    display_name = $4,
    greeting_name = $5,
    avatar_url = $6,
    updated_at = now()
WHERE tenant_id = $1 AND id = $2 AND deleted_at IS NULL
RETURNING tenant_id, id, workos_user_id, email::text AS email, display_name, greeting_name,
          avatar_url, deleted_at, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(email)
    .bind(display_name)
    .bind(greeting_name)
    .bind(avatar_url)
    .fetch_one(&mut **tx)
    .await
}

pub async fn insert_workos_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    sealed_session: &[u8],
    seal_key_version: &str,
    workos_session_id: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<WorkosSessionRow, sqlx::Error> {
    sqlx::query_as::<_, WorkosSessionRow>(
        r#"
INSERT INTO workos_sessions (
  tenant_id, user_id, sealed_session, seal_key_version, workos_session_id, expires_at
)
VALUES ($1,$2,$3,$4,$5,$6)
RETURNING *
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(sealed_session)
    .bind(seal_key_version)
    .bind(workos_session_id)
    .bind(expires_at)
    .fetch_one(&mut **tx)
    .await
}

pub async fn insert_app_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    token_hash: &[u8],
    key_id: &str,
    client: AuthClient,
    csrf_hash: Option<&[u8]>,
    workos_session_row_id: Uuid,
    expires_at: DateTime<Utc>,
) -> Result<AuthSessionRow, sqlx::Error> {
    sqlx::query_as::<_, AuthSessionRow>(
        r#"
INSERT INTO auth_sessions (
  tenant_id, user_id, token_hash, key_id, client, csrf_hash, csrf_rotated_at,
  workos_session_id, expires_at
)
VALUES ($1,$2,$3,$4,$5,$6, CASE WHEN $6::bytea IS NULL THEN NULL ELSE now() END, $7, $8)
RETURNING tenant_id, id, user_id, token_hash, key_id, client, csrf_hash, csrf_rotated_at,
          workos_session_id, expires_at, revoked_at, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(token_hash)
    .bind(key_id)
    .bind(client.as_str())
    .bind(csrf_hash)
    .bind(workos_session_row_id)
    .bind(expires_at)
    .fetch_one(&mut **tx)
    .await
}

pub async fn find_active_session_by_hash(
    pool: &PgPool,
    token_hash: &[u8],
) -> Result<Option<AuthSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, AuthSessionRow>(
        r#"
SELECT tenant_id, id, user_id, token_hash, key_id, client, csrf_hash, csrf_rotated_at,
       workos_session_id, expires_at, revoked_at, created_at, updated_at
FROM auth_sessions
WHERE token_hash = $1
  AND revoked_at IS NULL
  AND expires_at > now()
"#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
}

pub async fn lock_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    session_id: Uuid,
) -> Result<Option<AuthSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, AuthSessionRow>(
        r#"
SELECT tenant_id, id, user_id, token_hash, key_id, client, csrf_hash, csrf_rotated_at,
       workos_session_id, expires_at, revoked_at, created_at, updated_at
FROM auth_sessions
WHERE tenant_id = $1 AND id = $2
FOR UPDATE
"#,
    )
    .bind(tenant_id)
    .bind(session_id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn revoke_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    session_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE auth_sessions
SET revoked_at = COALESCE(revoked_at, now()), updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(session_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn revoke_all_user_sessions_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE auth_sessions
SET revoked_at = COALESCE(revoked_at, now()), updated_at = now()
WHERE tenant_id = $1 AND user_id = $2 AND revoked_at IS NULL
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
UPDATE workos_sessions
SET revoked_at = COALESCE(revoked_at, now())
WHERE tenant_id = $1 AND user_id = $2 AND revoked_at IS NULL
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn get_workos_session(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<WorkosSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, WorkosSessionRow>(
        r#"SELECT * FROM workos_sessions WHERE tenant_id = $1 AND id = $2"#,
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await
}

pub async fn get_workos_session_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<Option<WorkosSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, WorkosSessionRow>(
        r#"SELECT * FROM workos_sessions WHERE tenant_id = $1 AND id = $2 FOR UPDATE"#,
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn update_workos_session_sealed_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    id: Uuid,
    sealed_session: &[u8],
    seal_key_version: &str,
    workos_session_id: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE workos_sessions
SET sealed_session = $3,
    seal_key_version = $4,
    workos_session_id = COALESCE($5, workos_session_id),
    expires_at = $6
WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL
"#,
    )
    .bind(tenant_id)
    .bind(id)
    .bind(sealed_session)
    .bind(seal_key_version)
    .bind(workos_session_id)
    .bind(expires_at)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn rotate_csrf_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    session_id: Uuid,
    csrf_hash: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE auth_sessions
SET csrf_hash = $3, csrf_rotated_at = now(), updated_at = now()
WHERE tenant_id = $1 AND id = $2 AND revoked_at IS NULL
"#,
    )
    .bind(tenant_id)
    .bind(session_id)
    .bind(csrf_hash)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn get_onboarding(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<OnboardingRow>, sqlx::Error> {
    sqlx::query_as::<_, OnboardingRow>(
        r#"
SELECT tenant_id, id, user_id, coming_up, concerns, platforms, revision, current_step,
       consent_version, consent_accepted_at, completed_at, created_at, updated_at
FROM onboarding_profiles
WHERE tenant_id = $1 AND user_id = $2
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

pub async fn upsert_onboarding_revisioned(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    expected_revision: i64,
    coming_up: &[String],
    concerns: &[String],
    platforms: &[String],
    current_step: i16,
    consent_version: Option<&str>,
    consent_accepted_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
) -> Result<Option<OnboardingRow>, sqlx::Error> {
    // Insert empty row if missing, then conditional update on revision.
    sqlx::query(
        r#"
INSERT INTO onboarding_profiles (tenant_id, user_id)
VALUES ($1, $2)
ON CONFLICT (tenant_id, user_id) DO NOTHING
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(pool)
    .await?;

    sqlx::query_as::<_, OnboardingRow>(
        r#"
UPDATE onboarding_profiles
SET coming_up = $4,
    concerns = $5,
    platforms = $6,
    current_step = $7,
    consent_version = $8,
    consent_accepted_at = $9,
    completed_at = $10,
    revision = revision + 1,
    updated_at = now()
WHERE tenant_id = $1 AND user_id = $2 AND revision = $3
RETURNING tenant_id, id, user_id, coming_up, concerns, platforms, revision, current_step,
          consent_version, consent_accepted_at, completed_at, created_at, updated_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(expected_revision)
    .bind(coming_up)
    .bind(concerns)
    .bind(platforms)
    .bind(current_step)
    .bind(consent_version)
    .bind(consent_accepted_at)
    .bind(completed_at)
    .fetch_optional(pool)
    .await
}

pub async fn insert_webhook_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event_id: &str,
    event_type: &str,
    payload_hash: &[u8],
) -> Result<bool, sqlx::Error> {
    let res = sqlx::query(
        r#"
INSERT INTO webhook_events (provider, event_id, event_type, payload_hash)
VALUES ('workos', $1, $2, $3)
ON CONFLICT (provider, event_id) DO NOTHING
"#,
    )
    .bind(event_id)
    .bind(event_type)
    .bind(payload_hash)
    .execute(&mut **tx)
    .await?;
    Ok(res.rows_affected() == 1)
}

pub async fn insert_webhook_event(
    pool: &PgPool,
    event_id: &str,
    event_type: &str,
    payload_hash: &[u8],
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let inserted = insert_webhook_event_tx(&mut tx, event_id, event_type, payload_hash).await?;
    tx.commit().await?;
    Ok(inserted)
}

pub async fn tombstone_user_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    let row = sqlx::query_as::<_, (Option<DateTime<Utc>>,)>(
        r#"
UPDATE users
SET deleted_at = COALESCE(deleted_at, now()), updated_at = now()
WHERE tenant_id = $1 AND id = $2
RETURNING deleted_at
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.and_then(|r| r.0))
}

pub async fn cancel_non_purge_work_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE work_items
SET cancel_requested_at = COALESCE(cancel_requested_at, now()), updated_at = now()
WHERE tenant_id = $1
  AND subject_user_id = $2
  AND kind <> 'account.purge'
  AND status IN ('pending', 'running')
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn enqueue_account_purge_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    user_id: Uuid,
    payload: JsonValue,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
INSERT INTO work_items (
  tenant_id, subject_user_id, kind, payload, dedupe_key, priority, max_attempts
)
VALUES ($1, $2, 'account.purge', $3, $4, 100, 100)
ON CONFLICT (tenant_id, kind, dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(payload)
    .bind(user_id.to_string())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn sync_user_from_webhook_tx(
    tx: &mut Transaction<'_, Postgres>,
    workos_user_id: &str,
    display_name: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let res = sqlx::query(
        r#"
UPDATE users
SET display_name = COALESCE($2, display_name),
    avatar_url = COALESCE($3, avatar_url),
    updated_at = now()
WHERE workos_user_id = $1 AND deleted_at IS NULL
"#,
    )
    .bind(workos_user_id)
    .bind(display_name)
    .bind(avatar_url)
    .execute(&mut **tx)
    .await?;
    Ok(res.rows_affected())
}

pub async fn sync_user_from_webhook(
    pool: &PgPool,
    workos_user_id: &str,
    display_name: Option<&str>,
    avatar_url: Option<&str>,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let n = sync_user_from_webhook_tx(&mut tx, workos_user_id, display_name, avatar_url).await?;
    tx.commit().await?;
    Ok(n)
}

pub async fn find_workos_session_by_provider_id_tx(
    tx: &mut Transaction<'_, Postgres>,
    workos_session_id: &str,
) -> Result<Option<WorkosSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, WorkosSessionRow>(
        r#"SELECT * FROM workos_sessions WHERE workos_session_id = $1"#,
    )
    .bind(workos_session_id)
    .fetch_optional(&mut **tx)
    .await
}

pub async fn find_workos_session_by_provider_id(
    pool: &PgPool,
    workos_session_id: &str,
) -> Result<Option<WorkosSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, WorkosSessionRow>(
        r#"SELECT * FROM workos_sessions WHERE workos_session_id = $1"#,
    )
    .bind(workos_session_id)
    .fetch_optional(pool)
    .await
}

pub async fn revoke_workos_session_row_tx(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE workos_sessions
SET revoked_at = COALESCE(revoked_at, now())
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
UPDATE auth_sessions
SET revoked_at = COALESCE(revoked_at, now()), updated_at = now()
WHERE tenant_id = $1 AND workos_session_id = $2 AND revoked_at IS NULL
"#,
    )
    .bind(tenant_id)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn revoke_workos_session_row(
    pool: &PgPool,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    revoke_workos_session_row_tx(&mut tx, tenant_id, id).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn list_active_workos_sessions_for_reseal(
    pool: &PgPool,
) -> Result<Vec<WorkosSessionRow>, sqlx::Error> {
    sqlx::query_as::<_, WorkosSessionRow>(
        r#"
SELECT * FROM workos_sessions
WHERE revoked_at IS NULL
ORDER BY created_at ASC
"#,
    )
    .fetch_all(pool)
    .await
}

pub async fn local_purge_profile(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    // Archive-owned rows must be removed before the user is reduced to the
    // provider retry tombstone. Import/staging joins cascade from imports;
    // normalized content is user-owned and may otherwise outlive every import.
    sqlx::query(r#"DELETE FROM content_items WHERE tenant_id = $1 AND user_id = $2"#)
        .bind(tenant_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"DELETE FROM archive_imports WHERE tenant_id = $1 AND user_id = $2"#)
        .bind(tenant_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"DELETE FROM auth_sessions WHERE tenant_id = $1 AND user_id = $2"#)
        .bind(tenant_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"DELETE FROM workos_sessions WHERE tenant_id = $1 AND user_id = $2"#)
        .bind(tenant_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"DELETE FROM onboarding_profiles WHERE tenant_id = $1 AND user_id = $2"#)
        .bind(tenant_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        r#"
UPDATE users
SET email = NULL,
    display_name = NULL,
    greeting_name = NULL,
    avatar_url = NULL,
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
