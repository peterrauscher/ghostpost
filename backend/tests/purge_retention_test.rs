//! Plan 006: retention sweeper + soft-deleted flags hidden from reads.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn purge_retention_hides_and_sweeps() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("purge_ret");
    login_web(&app, &mock, &mut jar, &user).await;

    let tenant_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT tenant_id FROM users WHERE workos_user_id = $1",
    )
    .bind(&user)
    .fetch_one(&pool)
    .await
    .unwrap();
    let user_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE workos_user_id = $1")
        .bind(&user)
        .fetch_one(&pool)
        .await
        .unwrap();

    let scan_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO scans (tenant_id, user_id, status, phase, progress, finished_at)
VALUES ($1, $2, 'succeeded', 'complete', 100, now())
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let content_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO content_items (tenant_id, user_id, platform, kind, authorship, body)
VALUES ($1, $2, 'x', 'tweet', 'owner_authored', 'hidden')
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let flag_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO flagged_posts (tenant_id, scan_id, content_item_id, user_id, risk_level, hidden_at)
VALUES ($1, $2, $3, $4, 'high', now())
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(content_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let res = call(
        &app,
        Method::GET,
        &format!("/v1/flags/{flag_id}"),
        Some(&mut jar),
        &[],
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND, "hidden flag not readable");

    // Seed expired idempotency row and sweep.
    sqlx::query(
        r#"
INSERT INTO idempotency_keys (tenant_id, scope, key, created_at, updated_at)
VALUES ($1, 'test', $2, now() - interval '48 hours', now() - interval '48 hours')
"#,
    )
    .bind(tenant_id)
    .bind(format!("old-{}", Uuid::new_v4()))
    .execute(&pool)
    .await
    .unwrap();

    let n1 = ghostpost_backend::jobs::purge_retention::sweep_once(&pool)
        .await
        .expect("sweep");
    let n2 = ghostpost_backend::jobs::purge_retention::sweep_once(&pool)
        .await
        .expect("sweep again");
    assert!(n1 >= 1);
    assert_eq!(n2, 0, "idempotent second sweep");
}
