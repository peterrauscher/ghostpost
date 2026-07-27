//! Plan 003 Step 9 — CSRF rotation on GET /csrf and refresh.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn csrf_rotation_rejects_old_token_after_get_and_refresh() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("csrfrot");
    login_web(&app, &mock, &mut jar, &user).await;

    let csrf1 = fetch_csrf(&app, &mut jar).await;
    let csrf2 = fetch_csrf(&app, &mut jar).await;
    assert_ne!(csrf1, csrf2);

    // Old CSRF rejected after rotation via GET /csrf.
    let stale = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf1),
        Some(json!({
            "status": "in_progress",
            "currentStep": 2,
            "revision": 0,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": [],
                "platforms": [],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": false }
            }
        })),
    )
    .await;
    assert_eq!(stale.status, StatusCode::FORBIDDEN, "{}", stale.text());

    // Current CSRF works.
    let ok = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf2),
        Some(json!({
            "status": "in_progress",
            "currentStep": 2,
            "revision": 0,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": [],
                "platforms": [],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": false }
            }
        })),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());

    // Refresh rotates CSRF; prior token fails.
    let csrf_before_refresh = fetch_csrf(&app, &mut jar).await;
    let refresh = call(
        &app,
        Method::POST,
        "/v1/auth/refresh",
        Some(&mut jar),
        &web_mutation_headers(&csrf_before_refresh),
        None,
    )
    .await;
    assert_eq!(refresh.status, StatusCode::NO_CONTENT, "{}", refresh.text());
    assert!(jar.get("gp_session").is_some());

    let after_refresh = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf_before_refresh),
        Some(json!({
            "status": "in_progress",
            "currentStep": 2,
            "revision": 1,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": ["public_image"],
                "platforms": [],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": false }
            }
        })),
    )
    .await;
    assert_eq!(
        after_refresh.status,
        StatusCode::FORBIDDEN,
        "old CSRF after refresh must fail: {}",
        after_refresh.text()
    );

    let csrf3 = fetch_csrf(&app, &mut jar).await;
    let ok2 = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf3),
        Some(json!({
            "status": "in_progress",
            "currentStep": 3,
            "revision": 1,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": ["public_image"],
                "platforms": ["x"],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": false }
            }
        })),
    )
    .await;
    assert_eq!(ok2.status, StatusCode::OK, "{}", ok2.text());
}
