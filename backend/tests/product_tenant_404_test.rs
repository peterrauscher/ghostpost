//! Plan 006: cross-tenant scan/flag GET → 404.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn product_tenant_404() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    // Owner A resources.
    let mut jar_a = CookieJar::new();
    let user_a = unique_workos_id("ten_a");
    login_web(&app, &mock, &mut jar_a, &user_a).await;
    let tenant_a = sqlx::query_scalar::<_, Uuid>(
        "SELECT tenant_id FROM users WHERE workos_user_id = $1",
    )
    .bind(&user_a)
    .fetch_one(&pool)
    .await
    .unwrap();
    let uid_a = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE workos_user_id = $1")
        .bind(&user_a)
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
    .bind(tenant_a)
    .bind(uid_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    let content_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO content_items (tenant_id, user_id, platform, kind, authorship, body)
VALUES ($1, $2, 'x', 'tweet', 'owner_authored', 'x')
RETURNING id
"#,
    )
    .bind(tenant_a)
    .bind(uid_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    let flag_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO flagged_posts (tenant_id, scan_id, content_item_id, user_id, risk_level)
VALUES ($1, $2, $3, $4, 'low')
RETURNING id
"#,
    )
    .bind(tenant_a)
    .bind(scan_id)
    .bind(content_id)
    .bind(uid_a)
    .fetch_one(&pool)
    .await
    .unwrap();

    // User B
    let mut jar_b = CookieJar::new();
    let user_b = unique_workos_id("ten_b");
    login_web(&app, &mock, &mut jar_b, &user_b).await;

    let scan_res = call(
        &app,
        Method::GET,
        &format!("/v1/scans/{scan_id}"),
        Some(&mut jar_b),
        &[],
        None,
    )
    .await;
    assert_eq!(scan_res.status, StatusCode::NOT_FOUND, "{}", scan_res.text());
    let scan_body = scan_res.json();
    let code = scan_body["code"].as_str().unwrap_or("");
    assert_eq!(code, "RESOURCE_NOT_FOUND");

    let flag_res = call(
        &app,
        Method::GET,
        &format!("/v1/flags/{flag_id}"),
        Some(&mut jar_b),
        &[],
        None,
    )
    .await;
    assert_eq!(flag_res.status, StatusCode::NOT_FOUND, "{}", flag_res.text());
}
