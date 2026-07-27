//! Plan 002 remediation regressions for work-queue / schema invariants.
//!
//! These tests intentionally fail on the current defects. Run with:
//! `cargo test --locked --test plan002_remediation -- --ignored --test-threads=1`

use chrono::{Duration as ChronoDuration, Utc};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::jobs::{cancel, queue, sweeper};
use ghostpost_backend::repository::{users, work_items};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

fn test_env() {
    let _ = dotenvy::from_filename(".env");
    if std::env::var("DATABASE_URL").is_err() {
        std::env::set_var(
            "DATABASE_URL",
            "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost",
        );
    }
    if std::env::var("DATABASE_URL_APP").is_err() {
        std::env::set_var(
            "DATABASE_URL_APP",
            "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost",
        );
    }
    if std::env::var("DATABASE_APP_ROLE").is_err() {
        std::env::set_var("DATABASE_APP_ROLE", "ghostpost_app");
    }
    if std::env::var("GHOSTPOST_ENV").is_err() {
        std::env::set_var("GHOSTPOST_ENV", "development");
    }
}

async fn app_pool() -> PgPool {
    test_env();
    let url = std::env::var("DATABASE_URL_APP").expect("DATABASE_URL_APP");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("connect app pool")
}

async fn ensure_migrated() {
    test_env();
    let config = Config::load_for_migrate().expect("config");
    migrate::run(&config, false).await.expect("migrate");
}

async fn drain_pending(pool: &PgPool) {
    sqlx::query(
        r#"
UPDATE work_items
SET status = 'cancelled', finished_at = now(), updated_at = now()
WHERE status IN ('pending', 'running')
"#,
    )
    .execute(pool)
    .await
    .expect("drain pending");
}

async fn seed_tenant_user(pool: &PgPool) -> (Uuid, Uuid) {
    let tenant = users::create_tenant(pool).await.expect("tenant");
    let user = users::create_user(
        pool,
        tenant.id,
        None,
        Some("remediation@example.com"),
        Some("R"),
    )
    .await
    .expect("user");
    (tenant.id, user.id)
}

async fn latest_attempt(
    pool: &PgPool,
    tenant_id: Uuid,
    work_item_id: Uuid,
) -> (Option<String>, Option<chrono::DateTime<Utc>>) {
    sqlx::query_as::<_, (Option<String>, Option<chrono::DateTime<Utc>>)>(
        r#"
SELECT outcome, finished_at
FROM work_item_attempts
WHERE tenant_id = $1 AND work_item_id = $2
ORDER BY attempt_number DESC
LIMIT 1
"#,
    )
    .bind(tenant_id)
    .bind(work_item_id)
    .fetch_one(pool)
    .await
    .expect("latest attempt")
}

/// commit_failure fence miss must report lost_lease and close the open attempt.
#[tokio::test]
#[ignore]
async fn commit_failure_fence_miss_reports_lost_lease_and_closes_attempt() {
    ensure_migrated().await;
    let pool = app_pool().await;
    drain_pending(&pool).await;
    let (tenant_id, user_id) = seed_tenant_user(&pool).await;

    let item = work_items::enqueue(
        &pool,
        tenant_id,
        Some(user_id),
        "test.fail",
        json!({}),
        None,
        0,
        5,
    )
    .await
    .expect("enqueue");

    let claimed = queue::claim_one(&pool, "owner-a", Duration::from_secs(60))
        .await
        .expect("claim")
        .expect("item");
    assert_eq!(claimed.id, item.id);

    sqlx::query(
        r#"
UPDATE work_items
SET lease_owner = 'thief'
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(item.id)
    .execute(&pool)
    .await
    .expect("steal");

    let status = queue::commit_failure(
        &pool,
        tenant_id,
        item.id,
        "owner-a",
        "handler boom",
        claimed.attempt_count,
        claimed.max_attempts,
    )
    .await
    .expect("commit_failure");

    assert_eq!(
        status, "lost_lease",
        "fence miss must not be reported as pending/failed"
    );

    let (outcome, finished_at) = latest_attempt(&pool, tenant_id, item.id).await;
    assert_eq!(outcome.as_deref(), Some("lost_lease"));
    assert!(
        finished_at.is_some(),
        "lost_lease attempt must set finished_at"
    );

    // Work item fence must remain with the thief (no false retry/fail mutation).
    let row = work_items::get(&pool, tenant_id, item.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.lease_owner.as_deref(), Some("thief"));
    assert_eq!(row.status, "running");
}

/// Max-attempt sweep must not fail an in-flight unexpired running lease.
#[tokio::test]
#[ignore]
async fn max_attempt_sweep_skips_unexpired_running_lease() {
    ensure_migrated().await;
    let pool = app_pool().await;
    drain_pending(&pool).await;
    let (tenant_id, user_id) = seed_tenant_user(&pool).await;

    let item = work_items::enqueue(
        &pool,
        tenant_id,
        Some(user_id),
        "test.ping",
        json!({}),
        None,
        0,
        1,
    )
    .await
    .expect("enqueue");

    let claimed = queue::claim_one(&pool, "final-attempt-owner", Duration::from_secs(60))
        .await
        .expect("claim")
        .expect("item");
    assert_eq!(claimed.id, item.id);
    assert_eq!(claimed.attempt_count, 1);
    assert_eq!(claimed.max_attempts, 1);
    assert_eq!(claimed.status, "running");

    let (failed, stale) = sweeper::sweep_once(&pool).await.expect("sweep");
    assert_eq!(stale, 0, "lease is unexpired");
    assert_eq!(
        failed, 0,
        "max-attempt sweep must not preempt unexpired running lease (failed={failed})"
    );

    let row = work_items::get(&pool, tenant_id, item.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "running");
    assert_eq!(row.lease_owner.as_deref(), Some("final-attempt-owner"));
    assert!(row.finished_at.is_none());
}

/// Stale-lease recovery must close the open attempt with lost_lease + finished_at.
#[tokio::test]
#[ignore]
async fn stale_lease_recovery_closes_attempt_as_lost_lease() {
    ensure_migrated().await;
    let pool = app_pool().await;
    drain_pending(&pool).await;
    let (tenant_id, user_id) = seed_tenant_user(&pool).await;

    let item = work_items::enqueue(
        &pool,
        tenant_id,
        Some(user_id),
        "test.ping",
        json!({}),
        None,
        0,
        5,
    )
    .await
    .expect("enqueue");

    let claimed = queue::claim_one(&pool, "stale-owner", Duration::from_secs(1))
        .await
        .expect("claim")
        .expect("item");
    assert_eq!(claimed.id, item.id);

    sqlx::query(
        r#"
UPDATE work_items
SET lease_expires_at = $3
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(item.id)
    .bind(Utc::now() - ChronoDuration::minutes(1))
    .execute(&pool)
    .await
    .expect("expire");

    let (_, stale) = sweeper::sweep_once(&pool).await.expect("sweep");
    assert!(stale >= 1);

    let row = work_items::get(&pool, tenant_id, item.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "pending");
    assert!(row.lease_owner.is_none());

    let (outcome, finished_at) = latest_attempt(&pool, tenant_id, item.id).await;
    assert_eq!(
        outcome.as_deref(),
        Some("lost_lease"),
        "stale reclaim must close attempt outcome"
    );
    assert!(
        finished_at.is_some(),
        "stale reclaim must set attempt finished_at"
    );
}

/// Cancel recovery must close open attempts with cancelled + finished_at.
#[tokio::test]
#[ignore]
async fn cancel_recovery_closes_attempt_as_cancelled() {
    ensure_migrated().await;
    let pool = app_pool().await;
    drain_pending(&pool).await;
    let (tenant_id, user_id) = seed_tenant_user(&pool).await;

    let item = work_items::enqueue(
        &pool,
        tenant_id,
        Some(user_id),
        "test.ping",
        json!({}),
        None,
        0,
        5,
    )
    .await
    .expect("enqueue");

    let claimed = queue::claim_one(&pool, "cancel-owner", Duration::from_secs(60))
        .await
        .expect("claim")
        .expect("item");
    assert_eq!(claimed.id, item.id);

    work_items::request_cancel(&pool, tenant_id, item.id)
        .await
        .expect("cancel flag");

    // Expire lease so cancel recovery may take the running row.
    sqlx::query(
        r#"
UPDATE work_items
SET lease_expires_at = $3
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(item.id)
    .bind(Utc::now() - ChronoDuration::minutes(1))
    .execute(&pool)
    .await
    .expect("expire");

    let (work, _, _) = cancel::recover_once(&pool).await.expect("recover");
    assert!(work >= 1);

    let row = work_items::get(&pool, tenant_id, item.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "cancelled");
    assert!(row.finished_at.is_some());

    let (outcome, finished_at) = latest_attempt(&pool, tenant_id, item.id).await;
    assert_eq!(outcome.as_deref(), Some("cancelled"));
    assert!(
        finished_at.is_some(),
        "cancelled recovery must set attempt finished_at"
    );
}

/// Accepted schema correction: onboarding_profiles PK (tenant_id, id) + UNIQUE (tenant_id, user_id).
#[tokio::test]
#[ignore]
async fn onboarding_profiles_uses_tenant_id_pk_and_unique_user() {
    ensure_migrated().await;
    let pool = app_pool().await;

    let pk_cols: Vec<String> = sqlx::query_scalar(
        r#"
SELECT a.attname
FROM pg_index i
JOIN pg_attribute a
  ON a.attrelid = i.indrelid
 AND a.attnum = ANY (i.indkey)
WHERE i.indrelid = 'onboarding_profiles'::regclass
  AND i.indisprimary
ORDER BY array_position(i.indkey, a.attnum)
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("pk cols");

    assert_eq!(
        pk_cols,
        vec!["tenant_id".to_string(), "id".to_string()],
        "onboarding_profiles must use PRIMARY KEY (tenant_id, id)"
    );

    let has_unique_user: bool = sqlx::query_scalar(
        r#"
SELECT EXISTS (
  SELECT 1
  FROM pg_constraint c
  WHERE c.conrelid = 'onboarding_profiles'::regclass
    AND c.contype = 'u'
    AND (
      SELECT array_agg(a.attname::text ORDER BY u.ord)
      FROM unnest(c.conkey) WITH ORDINALITY AS u(attnum, ord)
      JOIN pg_attribute a
        ON a.attrelid = c.conrelid
       AND a.attnum = u.attnum
    ) = ARRAY['tenant_id','user_id']::text[]
)
"#,
    )
    .fetch_one(&pool)
    .await
    .expect("unique check");

    assert!(
        has_unique_user,
        "onboarding_profiles must UNIQUE (tenant_id, user_id)"
    );
}
