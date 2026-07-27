//! Plan 003 Step 7 — two independent web cookie jars.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use std::sync::Arc;

#[tokio::test]
async fn two_cookie_jar_web_session_isolation() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar_a = CookieJar::new();
    let mut jar_b = CookieJar::new();

    // Parallel authorize flows for A and B.
    let authz_a = authorize_web(&app, &mut jar_a).await;
    let authz_b = authorize_web(&app, &mut jar_b).await;
    assert_ne!(authz_a["state"], authz_b["state"]);
    assert_ne!(
        jar_a.get("gp_auth_init").unwrap(),
        jar_b.get("gp_auth_init").unwrap()
    );

    // Complete exchange for A only.
    let user_a = unique_workos_id("web_a");
    let state_a = authz_a["state"].as_str().unwrap().to_string();
    let code_a = format!("code_{}", uuid::Uuid::new_v4());
    let refresh_a = format!("refresh_{user_a}");
    let session_a = provider_session(
        &user_a,
        &format!("{user_a}@example.test"),
        "Alice Web",
        &refresh_a,
    );
    mock.register_code(&code_a, session_a.clone());
    mock.register_refresh(
        &refresh_a,
        ProviderAuthSession {
            access_token: format!("access2_{user_a}"),
            refresh_token: format!("refresh2_{user_a}"),
            ..session_a
        },
    );
    let ex_a = exchange_web(&app, &mut jar_a, &code_a, &state_a).await;
    assert_eq!(ex_a.status, StatusCode::NO_CONTENT, "{}", ex_a.text());
    assert!(jar_a.get("gp_session").is_some());

    // B has no session → 401.
    let me_b = call(
        &app,
        Method::GET,
        "/v1/me",
        Some(&mut jar_b),
        &[],
        None,
    )
    .await;
    assert_eq!(me_b.status, StatusCode::UNAUTHORIZED, "{}", me_b.text());

    // A is authenticated → 200 profile only.
    let me_a = call(
        &app,
        Method::GET,
        "/v1/me",
        Some(&mut jar_a),
        &[],
        None,
    )
    .await;
    assert_eq!(me_a.status, StatusCode::OK, "{}", me_a.text());
    let me_json = me_a.json();
    assert!(me_json.get("user").is_some());
    assert!(me_json.get("lifecycle").is_none());
    assert!(me_json.get("onboarding").is_none());
    assert!(me_json.get("entitlements").is_none());

    let raw = jar_a.get("gp_session").unwrap().to_string();
    assert_session_hash_only(&pool, &raw).await;
    let user_id = uuid::Uuid::parse_str(me_json["user"]["id"].as_str().unwrap()).unwrap();
    let tenant_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT tenant_id FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_workos_sealed(&pool, tenant_id, user_id).await;

    // Wrong Origin on mutating route → 403.
    let csrf = fetch_csrf(&app, &mut jar_a).await;
    let mut bad_headers = web_mutation_headers(&csrf);
    for (k, v) in bad_headers.iter_mut() {
        if *k == "origin" {
            *v = "https://evil.example".into();
        }
    }
    let bad_origin = call(
        &app,
        Method::POST,
        "/v1/auth/logout",
        Some(&mut jar_a),
        &bad_headers,
        None,
    )
    .await;
    assert_eq!(bad_origin.status, StatusCode::FORBIDDEN, "{}", bad_origin.text());

    // Logout A with correct guards → subsequent /me is 401.
    let csrf = fetch_csrf(&app, &mut jar_a).await;
    let logout = call(
        &app,
        Method::POST,
        "/v1/auth/logout",
        Some(&mut jar_a),
        &web_mutation_headers(&csrf),
        None,
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT, "{}", logout.text());
    let me_after = call(
        &app,
        Method::GET,
        "/v1/me",
        Some(&mut jar_a),
        &[],
        None,
    )
    .await;
    assert_eq!(me_after.status, StatusCode::UNAUTHORIZED);

    // Replay consumed state → 410.
    let replay = exchange_web(&app, &mut jar_b, &code_a, &state_a).await;
    assert_eq!(replay.status, StatusCode::GONE, "{}", replay.text());
    assert_eq!(replay.problem_code().as_deref(), Some("AUTH_FLOW_GONE"));
}
