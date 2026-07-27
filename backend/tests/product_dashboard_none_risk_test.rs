//! Plan 006: zero open flags ⇒ risk.level=none, gaugeSweep=0.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn product_dashboard_none_risk() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("dash_none");
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

    // Succeeded scan with zero flags.
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

    let res = call(
        &app,
        Method::GET,
        &format!("/v1/dashboard?scanId={scan_id}"),
        Some(&mut jar),
        &[],
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    let body = res.json();
    assert_eq!(body["risk"]["level"], "none");
    assert_eq!(body["risk"]["flaggedCount"], 0);
    assert_eq!(body["risk"]["gaugeSweep"], 0);
    assert!(body["flaggedPreview"].as_array().unwrap().is_empty());
    assert_eq!(body["auditHeadline"], "no flagged posts found");
}
