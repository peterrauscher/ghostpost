//! Plan 003 Step 8 — two native contexts / exchange-proof binding.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn two_native_context_exchange_proof_binding() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let authz1 = authorize_native(&app).await;
    let authz2 = authorize_native(&app).await;
    let state1 = authz1["state"].as_str().unwrap().to_string();
    let secret1 = authz1["exchangeSecret"].as_str().unwrap().to_string();
    let state2 = authz2["state"].as_str().unwrap().to_string();
    let secret2 = authz2["exchangeSecret"].as_str().unwrap().to_string();
    assert_ne!(state1, state2);
    assert_ne!(secret1, secret2);

    let user1 = unique_workos_id("native_u1");
    let code1 = format!("code_{}", uuid::Uuid::new_v4());
    let refresh1 = format!("refresh_{user1}");
    let session1 = provider_session(
        &user1,
        &format!("{user1}@example.test"),
        "Native One",
        &refresh1,
    );
    mock.register_code(&code1, session1.clone());
    mock.register_refresh(
        &refresh1,
        ProviderAuthSession {
            access_token: format!("access2_{user1}"),
            refresh_token: format!("refresh2_{user1}"),
            ..session1
        },
    );

    // Wrong exchangeSecret for flow 1 → 403.
    let mismatch = exchange_native(&app, &code1, &state1, &secret2).await;
    assert_eq!(mismatch.status, StatusCode::FORBIDDEN, "{}", mismatch.text());

    // Correct binding → 200 + bearer T1.
    let ok = exchange_native(&app, &code1, &state1, &secret1).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text());
    let body1 = ok.json();
    assert_eq!(body1["session"]["kind"], "bearer");
    let t1 = body1["session"]["token"].as_str().unwrap().to_string();
    let user1_id = body1["user"]["id"].as_str().unwrap().to_string();
    assert_session_hash_only(&pool, &t1).await;

    // Start flow 2 for a different user; T1 cannot consume flow 2.
    let user2 = unique_workos_id("native_u2");
    let authz2b = authorize_native(&app).await;
    let state2b = authz2b["state"].as_str().unwrap().to_string();
    let secret2b = authz2b["exchangeSecret"].as_str().unwrap().to_string();
    let code2 = format!("code_{}", uuid::Uuid::new_v4());
    let refresh2 = format!("refresh_{user2}");
    let session2 = provider_session(
        &user2,
        &format!("{user2}@example.test"),
        "Native Two",
        &refresh2,
    );
    mock.register_code(&code2, session2.clone());
    mock.register_refresh(
        &refresh2,
        ProviderAuthSession {
            access_token: format!("access2_{user2}"),
            refresh_token: format!("refresh2_{user2}"),
            ..session2
        },
    );

    // Bearer token is irrelevant to exchange proof — wrong/missing secret still 403,
    // and T1 cannot "consume" another flow by presenting itself.
    let steal = call(
        &app,
        Method::POST,
        "/v1/auth/exchange",
        None,
        &bearer_headers(&t1),
        Some(json!({
            "client": "native",
            "code": code2,
            "state": state2b,
            "exchangeSecret": secret1
        })),
    )
    .await;
    assert_eq!(steal.status, StatusCode::FORBIDDEN, "{}", steal.text());

    let ok2 = exchange_native(&app, &code2, &state2b, &secret2b).await;
    assert_eq!(ok2.status, StatusCode::OK, "{}", ok2.text());
    let body2 = ok2.json();
    let t2 = body2["session"]["token"].as_str().unwrap().to_string();
    let user2_id = body2["user"]["id"].as_str().unwrap().to_string();
    assert_ne!(user1_id, user2_id);
    assert_ne!(t1, t2);

    // Tenant isolation: each bearer sees only its own profile.
    let me1 = call(
        &app,
        Method::GET,
        "/v1/me",
        None,
        &bearer_headers(&t1),
        None,
    )
    .await;
    let me2 = call(
        &app,
        Method::GET,
        "/v1/me",
        None,
        &bearer_headers(&t2),
        None,
    )
    .await;
    assert_eq!(me1.status, StatusCode::OK);
    assert_eq!(me2.status, StatusCode::OK);
    assert_eq!(me1.json()["user"]["id"], user1_id);
    assert_eq!(me2.json()["user"]["id"], user2_id);
    assert_ne!(me1.json()["user"]["email"], me2.json()["user"]["email"]);
}
