//! Plan 006: duplicate Idempotency-Key on review-actions ⇒ same 200; one audit row.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn product_disposition_idempotency() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("disp_idem");
    login_web(&app, &mock, &mut jar, &user).await;
    let csrf = fetch_csrf(&app, &mut jar).await;

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
VALUES ($1, $2, 'x', 'tweet', 'owner_authored', 'quote text')
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
VALUES ($1, $2, $3, $4, 'high', 'negativity', 'mean')
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

    let key = format!("review-{}", Uuid::new_v4());
    let mut headers = web_mutation_headers(&csrf);
    headers.push(("idempotency-key", key.clone()));
    let body = json!({ "action": "keep", "expectedStatus": "open" });

    let first = call(
        &app,
        Method::POST,
        &format!("/v1/flags/{flag_id}/review-actions"),
        Some(&mut jar),
        &headers,
        Some(body.clone()),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text());
    assert_eq!(first.json()["status"], "kept");

    let second = call(
        &app,
        Method::POST,
        &format!("/v1/flags/{flag_id}/review-actions"),
        Some(&mut jar),
        &headers,
        Some(body),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK, "{}", second.text());
    assert_eq!(second.json()["id"], first.json()["id"]);

    let n: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM review_actions WHERE tenant_id=$1 AND flagged_post_id=$2",
    )
    .bind(tenant_id)
    .bind(flag_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "exactly one audit row");
}
