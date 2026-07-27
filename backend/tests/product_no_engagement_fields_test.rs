//! Plan 006: flag JSON omits engagement fields at launch.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn product_no_engagement_fields() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("no_eng");
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
VALUES ($1, $2, 'reddit', 'post', 'owner_authored', 'hello world')
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
INSERT INTO flagged_posts (tenant_id, scan_id, content_item_id, user_id, risk_level, category, reason_summary)
VALUES ($1, $2, $3, $4, 'low', 'other', 'note')
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
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let v = res.json();
    assert!(v.get("likes").is_none());
    assert!(v.get("comments").is_none());
    assert!(v.get("engagementLabel").is_none());
    assert!(v.get("quote").is_some());
}
