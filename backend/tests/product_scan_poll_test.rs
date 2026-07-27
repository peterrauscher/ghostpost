//! Plan 006: POST /v1/scans → poll GET /v1/scans/{id} to terminal.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

async fn seed_ready_import(pool: &sqlx::PgPool, tenant_id: Uuid, user_id: Uuid, platform: &str) -> Uuid {
    sqlx::query_scalar(
        r#"
INSERT INTO archive_imports (tenant_id, user_id, platform, status)
VALUES ($1, $2, $3, 'ready')
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(platform)
    .fetch_one(pool)
    .await
    .expect("import")
}

#[tokio::test]
async fn product_scan_poll_create_and_terminal() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("scan_poll");
    login_web(&app, &mock, &mut jar, &user).await;
    let csrf = fetch_csrf(&app, &mut jar).await;

    let me = call(&app, Method::GET, "/v1/me", Some(&mut jar), &[], None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
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

    let import_id = seed_ready_import(&pool, tenant_id, user_id, "x").await;

    // Complete scan immediately via DB so poll sees terminal without worker.
    // First create via API.
    let mut headers = web_mutation_headers(&csrf);
    headers.push(("idempotency-key", format!("scan-{}", Uuid::new_v4())));
    let create = call(
        &app,
        Method::POST,
        "/v1/scans",
        Some(&mut jar),
        &headers,
        Some(json!({ "archiveImportIds": [import_id] })),
    )
    .await;
    assert_eq!(create.status, StatusCode::ACCEPTED, "{}", create.text());
    let body = create.json();
    let scan_id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["status"], "queued");
    assert_eq!(body["phase"], "connecting");
    assert_eq!(body["progress"], 0.0);

    // Simulate worker success.
    sqlx::query(
        r#"
UPDATE scans
SET status = 'succeeded', phase = 'complete', progress = 100, finished_at = now(), updated_at = now()
WHERE id = $1::uuid
"#,
    )
    .bind(&scan_id)
    .execute(&pool)
    .await
    .unwrap();

    let poll = call(
        &app,
        Method::GET,
        &format!("/v1/scans/{scan_id}"),
        Some(&mut jar),
        &[],
        None,
    )
    .await;
    assert_eq!(poll.status, StatusCode::OK, "{}", poll.text());
    let p = poll.json();
    assert_eq!(p["status"], "succeeded");
    assert_eq!(p["phase"], "complete");
    assert_eq!(p["progress"], 1.0);
    assert!(p.get("finishedAt").and_then(|v| v.as_str()).is_some());
}
