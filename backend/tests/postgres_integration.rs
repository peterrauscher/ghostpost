use chrono::{Duration as ChronoDuration, Utc};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::jobs::{cancel, queue, sweeper};
use ghostpost_backend::repository::{
    archive_imports, entitlements, users, work_items,
};
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
    let user = users::create_user(pool, tenant.id, None, Some("t@example.com"), Some("T"))
        .await
        .expect("user");
    (tenant.id, user.id)
}

#[tokio::test]
#[ignore]
async fn migrate_idempotent() {
    ensure_migrated().await;
    ensure_migrated().await;
}

#[tokio::test]
#[ignore]
async fn tenant_composite_fk_rejects_cross_tenant() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let (tenant_a, user_a) = seed_tenant_user(&pool).await;
    let (tenant_b, _user_b) = seed_tenant_user(&pool).await;

    let err = sqlx::query(
        r#"
INSERT INTO onboarding_profiles (tenant_id, user_id, coming_up, concerns, platforms)
VALUES ($1, $2, '{}', '{}', '{}')
"#,
    )
    .bind(tenant_b)
    .bind(user_a)
    .execute(&pool)
    .await
    .expect_err("cross-tenant FK must fail");

    let msg = err.to_string();
    assert!(
        msg.contains("foreign key") || msg.to_lowercase().contains("violates"),
        "unexpected error: {msg}"
    );
    let _ = (tenant_a,);
}

#[tokio::test]
#[ignore]
async fn work_item_claim_fence() {
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
        100,
        5,
    )
    .await
    .expect("enqueue");

    let claimed = queue::claim_one(&pool, "owner-a", Duration::from_secs(60))
        .await
        .expect("claim")
        .expect("should claim");
    assert_eq!(claimed.id, item.id);
    assert_eq!(claimed.lease_owner.as_deref(), Some("owner-a"));

    let hb_ok = queue::heartbeat(&pool, tenant_id, item.id, "owner-a", Duration::from_secs(60))
        .await
        .expect("hb");
    assert!(hb_ok);

    let hb_bad = queue::heartbeat(&pool, tenant_id, item.id, "owner-b", Duration::from_secs(60))
        .await
        .expect("hb bad");
    assert!(!hb_bad);

    let commit_bad = queue::commit_success(&pool, tenant_id, item.id, "owner-b")
        .await
        .expect("commit bad");
    assert!(!commit_bad);

    let commit_ok = queue::commit_success(&pool, tenant_id, item.id, "owner-a")
        .await
        .expect("commit ok");
    assert!(commit_ok);
}

#[tokio::test]
#[ignore]
async fn work_item_max_attempts_sweeper() {
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
        100,
        2,
    )
    .await
    .expect("enqueue");

    for owner in ["o1", "o2"] {
        let claimed = queue::claim_one(&pool, owner, Duration::from_secs(30))
            .await
            .expect("claim")
            .expect("item");
        assert_eq!(claimed.id, item.id);
        let status = queue::commit_failure(
            &pool,
            tenant_id,
            item.id,
            owner,
            "boom",
            claimed.attempt_count,
            claimed.max_attempts,
        )
        .await
        .expect("fail");
        if owner == "o1" {
            assert_eq!(status, "pending");
            // Make immediately claimable despite backoff.
            sqlx::query(
                r#"UPDATE work_items SET run_after = now() WHERE tenant_id = $1 AND id = $2"#,
            )
            .bind(tenant_id)
            .bind(item.id)
            .execute(&pool)
            .await
            .expect("reset run_after");
        } else {
            assert_eq!(status, "failed");
        }
    }

    // Force a pending exhausted row and sweep.
    sqlx::query(
        r#"
UPDATE work_items
SET status = 'pending', attempt_count = max_attempts, finished_at = NULL
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(item.id)
    .execute(&pool)
    .await
    .expect("force");

    let (failed, _) = sweeper::sweep_once(&pool).await.expect("sweep");
    assert!(failed >= 1);

    let row = work_items::get(&pool, tenant_id, item.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "failed");
}

#[tokio::test]
#[ignore]
async fn cancellation_marks_cancelled() {
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

    work_items::request_cancel(&pool, tenant_id, item.id)
        .await
        .expect("cancel flag");

    // Pending + cancel_requested should be recovered to cancelled.
    let (work, _, _) = cancel::recover_once(&pool).await.expect("recover");
    assert!(work >= 1);

    let row = work_items::get(&pool, tenant_id, item.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.status, "cancelled");
}

#[tokio::test]
#[ignore]
async fn stale_lease_recovery() {
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
}

#[tokio::test]
#[ignore]
async fn dml_role_cannot_create_table() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let err = sqlx::query("CREATE TABLE evil(id int)")
        .execute(&pool)
        .await
        .expect_err("app role must not CREATE");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("permission denied") || msg.contains("must be owner"),
        "unexpected: {msg}"
    );
}

#[tokio::test]
#[ignore]
async fn entitlement_grants_free_beta_storage() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let (tenant_id, user_id) = seed_tenant_user(&pool).await;
    let grant = entitlements::grant_free_beta(&pool, tenant_id, user_id)
        .await
        .expect("grant");
    assert_eq!(grant.product_key, "free_beta");
    assert_eq!(grant.platform_limit, Some(2));

    let active = entitlements::get_active_grant(&pool, tenant_id, user_id, "free_beta")
        .await
        .expect("get")
        .expect("active");
    assert_eq!(active.id, grant.id);

    // Ensure archive import helper works (tenant FK path) without HTTP.
    let import = archive_imports::create_import(&pool, tenant_id, user_id, "reddit")
        .await
        .expect("import");
    assert_eq!(import.status, "awaiting_upload");
}
