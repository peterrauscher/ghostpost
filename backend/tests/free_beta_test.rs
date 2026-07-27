//! Plan 006: free_beta productId, platformLimit=2, unlimited rescans.
mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

async fn ids(pool: &sqlx::PgPool, workos: &str) -> (Uuid, Uuid) {
    let tenant_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT tenant_id FROM users WHERE workos_user_id = $1",
    )
    .bind(workos)
    .fetch_one(pool)
    .await
    .unwrap();
    let user_id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE workos_user_id = $1")
        .bind(workos)
        .fetch_one(pool)
        .await
        .unwrap();
    (tenant_id, user_id)
}

async fn ready_import(pool: &sqlx::PgPool, tenant_id: Uuid, user_id: Uuid, platform: &str) -> Uuid {
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
    .unwrap()
}

#[tokio::test]
async fn free_beta_entitlement_shape_and_limits() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("free_beta");
    login_web(&app, &mock, &mut jar, &user).await;

    let ent = call(
        &app,
        Method::GET,
        "/v1/me/entitlement",
        Some(&mut jar),
        &[],
        None,
    )
    .await;
    assert_eq!(ent.status, StatusCode::OK, "{}", ent.text());
    let body = ent.json();
    assert_eq!(body["productId"], "free_beta");
    assert_eq!(body["status"], "active");
    assert_eq!(body["capabilities"]["platformLimit"], 2);
    assert_eq!(body["capabilities"]["reviewAccess"], true);
    assert!(body["capabilities"]["rescansRemaining"].is_null());

    let (tenant_id, user_id) = ids(&pool, &user).await;
    let a = ready_import(&pool, tenant_id, user_id, "x").await;
    let b = ready_import(&pool, tenant_id, user_id, "reddit").await;
    // Third distinct platform exceeds limit — use facebook as third ready import.
    let c = ready_import(&pool, tenant_id, user_id, "facebook").await;

    let csrf = fetch_csrf(&app, &mut jar).await;
    let mut headers = web_mutation_headers(&csrf);
    headers.push(("idempotency-key", format!("fb-3plat-{}", Uuid::new_v4())));
    let too_many = call(
        &app,
        Method::POST,
        "/v1/scans",
        Some(&mut jar),
        &headers,
        Some(json!({ "archiveImportIds": [a, b, c] })),
    )
    .await;
    assert_eq!(
        too_many.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        too_many.text()
    );

    // Two platforms OK.
    let mut headers = web_mutation_headers(&csrf);
    headers.push(("idempotency-key", format!("fb-2plat-{}", Uuid::new_v4())));
    let ok = call(
        &app,
        Method::POST,
        "/v1/scans",
        Some(&mut jar),
        &headers,
        Some(json!({ "archiveImportIds": [a, b] })),
    )
    .await;
    assert_eq!(ok.status, StatusCode::ACCEPTED, "{}", ok.text());
    let scan_id = ok.json()["id"].as_str().unwrap().to_string();

    // Finish so rescan allowed.
    sqlx::query(
        "UPDATE scans SET status='succeeded', phase='complete', progress=100, finished_at=now() WHERE id=$1::uuid",
    )
    .bind(&scan_id)
    .execute(&pool)
    .await
    .unwrap();

    // Rescan still 202 (unlimited free_beta).
    let mut headers = web_mutation_headers(&csrf);
    headers.push(("idempotency-key", format!("fb-rescan-{}", Uuid::new_v4())));
    let rescan = call(
        &app,
        Method::POST,
        "/v1/scans",
        Some(&mut jar),
        &headers,
        Some(json!({ "archiveImportIds": [a] })),
    )
    .await;
    assert_eq!(rescan.status, StatusCode::ACCEPTED, "{}", rescan.text());
}
