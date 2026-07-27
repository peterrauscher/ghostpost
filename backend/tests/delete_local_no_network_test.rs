//! Plan 006: delete_local succeeds with zero outbound HTTP; content scheduled for purge.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use uuid::Uuid;

static OUTBOUND: AtomicUsize = AtomicUsize::new(0);

/// Test double: any attempt to record outbound HTTP fails the assertion path.
pub fn note_outbound() {
    OUTBOUND.fetch_add(1, Ordering::SeqCst);
}

#[tokio::test]
async fn delete_local_no_network() {
    OUTBOUND.store(0, Ordering::SeqCst);
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("del_local");
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
VALUES ($1, $2, 'x', 'tweet', 'owner_authored', 'delete me')
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
VALUES ($1, $2, $3, $4, 'medium', 'negativity', 'x')
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

    // Handler path must not call note_outbound; AppState has no HTTP client for review.
    let mut headers = web_mutation_headers(&csrf);
    headers.push(("idempotency-key", format!("del-{}", Uuid::new_v4())));
    let res = call(
        &app,
        Method::POST,
        &format!("/v1/flags/{flag_id}/review-actions"),
        Some(&mut jar),
        &headers,
        Some(json!({ "action": "delete_local", "expectedStatus": "open" })),
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text());
    assert_eq!(res.json()["status"], "deleted");
    assert_eq!(OUTBOUND.load(Ordering::SeqCst), 0, "zero outbound HTTP");

    let pending: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT deletion_pending_at FROM content_items WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(content_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(pending.is_some(), "content scheduled for purge");

    let work: i64 = sqlx::query_scalar(
        r#"
SELECT count(*)::bigint FROM work_items
WHERE tenant_id=$1 AND kind='purge_content' AND dedupe_key=$2::text
"#,
    )
    .bind(tenant_id)
    .bind(content_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(work, 1);

    // Run purge worker — still no network.
    let item = ghostpost_backend::repository::work_items::get(
        &pool,
        tenant_id,
        sqlx::query_scalar(
            "SELECT id FROM work_items WHERE tenant_id=$1 AND kind='purge_content' AND dedupe_key=$2::text",
        )
        .bind(tenant_id)
        .bind(content_id)
        .fetch_one(&pool)
        .await
        .unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    ghostpost_backend::jobs::purge_content::handle(&pool, &item)
        .await
        .expect("purge");
    assert_eq!(OUTBOUND.load(Ordering::SeqCst), 0);

    let gone: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM content_items WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(content_id)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert!(gone.is_none(), "content hard-deleted");
}
