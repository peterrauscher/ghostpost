//! Remaining Plan 003 HTTP/DB contracts (mock WorkOS only).

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::test]
async fn authorize_shapes_and_forbidden_routes() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock, KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let web = authorize_web(&app, &mut jar).await;
    assert!(web.get("authorizationUrl").and_then(|v| v.as_str()).unwrap().contains("code_challenge"));
    assert!(web.get("expiresAt").is_some());

    let native = authorize_native(&app).await;
    assert!(native.get("exchangeSecret").is_some());
    assert!(native.get("expiresAt").is_some());

    // Aliases that must not be registered. Axum returns 405 when a path exists
    // for another method (GET /v1/auth/authorize) and 404 when the path is absent.
    let cases = [
        ("POST", "/v1/auth/authorize", true), // GET-only path → 405
        ("GET", "/v1/auth/callback", false),
        ("POST", "/v1/auth/callback", false),
        ("GET", "/v1/lifecycle", false),
        ("POST", "/v1/onboarding", false),
        ("GET", "/v1/onboarding", false),
        ("GET", "/v1/platforms/catalog", false),
        ("DELETE", "/v1/account", false),
    ];
    for (method, path, allow_method_not_allowed) in cases {
        let m = match method {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "DELETE" => Method::DELETE,
            _ => unreachable!(),
        };
        let res = call(&app, m, path, None, &[], None).await;
        let ok = res.status == StatusCode::NOT_FOUND
            || (allow_method_not_allowed && res.status == StatusCode::METHOD_NOT_ALLOWED);
        assert!(
            ok,
            "{method} {path} must not be registered as a usable route, got {}",
            res.status
        );
        assert!(
            !res.status.is_success(),
            "{method} {path} must not succeed"
        );
    }
}

#[tokio::test]
async fn web_exchange_rejects_body_secret_and_enforces_one_use() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let authz = authorize_web(&app, &mut jar).await;
    let state = authz["state"].as_str().unwrap().to_string();
    let init = jar.get("gp_auth_init").unwrap().to_string();

    let body_secret = call(
        &app,
        Method::POST,
        "/v1/auth/exchange",
        Some(&mut jar),
        &[],
        Some(json!({
            "client": "web",
            "code": "x",
            "state": state,
            "exchangeSecret": init
        })),
    )
    .await;
    assert_eq!(body_secret.status, StatusCode::FORBIDDEN, "{}", body_secret.text());

    // Happy path then replay.
    let mut jar2 = CookieJar::new();
    let user = unique_workos_id("oneuse");
    login_web(&app, &mock, &mut jar2, &user).await;
    // Capture a fresh flow and replay after consume via second authorize.
    let mut jar3 = CookieJar::new();
    let authz3 = authorize_web(&app, &mut jar3).await;
    let state3 = authz3["state"].as_str().unwrap().to_string();
    let code3 = format!("code_{}", Uuid::new_v4());
    let refresh = format!("refresh_{user}_b");
    let session = provider_session(&user, &format!("{user}b@example.test"), "Reuse", &refresh);
    // Same workos user reuses tenant; still one-use on flow.
    mock.register_code(&code3, session);
    let first = exchange_web(&app, &mut jar3, &code3, &state3).await;
    assert_eq!(first.status, StatusCode::NO_CONTENT, "{}", first.text());
    let replay = exchange_web(&app, &mut jar3, &code3, &state3).await;
    assert_eq!(replay.status, StatusCode::GONE);
    assert_eq!(replay.problem_code().as_deref(), Some("AUTH_FLOW_GONE"));
}

#[tokio::test]
async fn native_refresh_logout_and_provider_failure() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let user = unique_workos_id("natref");
    let (body, token) = login_native(&app, &mock, &user).await;
    assert_eq!(body["session"]["kind"], "bearer");

    let refresh = call(
        &app,
        Method::POST,
        "/v1/auth/refresh",
        None,
        &bearer_headers(&token),
        None,
    )
    .await;
    assert_eq!(refresh.status, StatusCode::OK, "{}", refresh.text());
    let refreshed = refresh.json();
    assert_eq!(refreshed["session"]["kind"], "bearer");
    let token2 = refreshed["session"]["token"].as_str().unwrap().to_string();
    assert_ne!(token, token2);

    // Old bearer revoked.
    let old = call(&app, Method::GET, "/v1/me", None, &bearer_headers(&token), None).await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);

    let logout = call(
        &app,
        Method::POST,
        "/v1/auth/logout",
        None,
        &bearer_headers(&token2),
        None,
    )
    .await;
    assert_eq!(logout.status, StatusCode::NO_CONTENT);
    let me = call(&app, Method::GET, "/v1/me", None, &bearer_headers(&token2), None).await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);

    // Provider exchange failure path.
    mock.inner.lock().unwrap().force_exchange_err = Some("workos down".into());
    let authz = authorize_native(&app).await;
    let fail = exchange_native(
        &app,
        "nope",
        authz["state"].as_str().unwrap(),
        authz["exchangeSecret"].as_str().unwrap(),
    )
    .await;
    assert!(
        fail.status.is_server_error() || fail.status == StatusCode::INTERNAL_SERVER_ERROR,
        "provider failure should surface as error, got {}",
        fail.status
    );
}

#[tokio::test]
async fn onboarding_revision_conflict_and_platform_catalog() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    login_web(&app, &mock, &mut jar, &unique_workos_id("onboard")).await;
    let csrf = fetch_csrf(&app, &mut jar).await;

    let empty = call(&app, Method::GET, "/v1/me/onboarding", Some(&mut jar), &[], None).await;
    assert_eq!(empty.status, StatusCode::OK);
    let e = empty.json();
    assert_eq!(e["status"], "not_started");
    assert_eq!(e["revision"], 0);
    assert_eq!(e["currentStep"], 1);
    assert!(e.get("completed").is_none());

    let put_ok = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        Some(json!({
            "status": "completed",
            "currentStep": 4,
            "revision": 0,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": ["public_image"],
                "platforms": ["x"],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": true }
            }
        })),
    )
    .await;
    assert_eq!(put_ok.status, StatusCode::OK, "{}", put_ok.text());
    let saved = put_ok.json();
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["status"], "completed");

    let conflict = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        Some(json!({
            "status": "completed",
            "currentStep": 4,
            "revision": 0,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": ["public_image"],
                "platforms": ["reddit"],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": true }
            }
        })),
    )
    .await;
    assert_eq!(conflict.status, StatusCode::CONFLICT);
    assert_eq!(
        conflict.problem_code().as_deref(),
        Some("ONBOARDING_REVISION_CONFLICT")
    );

    let disabled = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        Some(json!({
            "status": "in_progress",
            "currentStep": 3,
            "revision": 1,
            "answers": {
                "comingUp": ["college_apps"],
                "concerns": ["public_image"],
                "platforms": ["facebook"],
                "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": false }
            }
        })),
    )
    .await;
    assert_eq!(disabled.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", disabled.text());

    let platforms = call(&app, Method::GET, "/v1/platforms", None, &[], None).await;
    assert_eq!(platforms.status, StatusCode::OK);
    let cat = platforms.json();
    assert_eq!(cat["revision"], DISCLOSURE_VERSION);
    let ids: Vec<&str> = cat["platforms"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["facebook", "reddit", "instagram", "tiktok", "x"]);
    let reddit = cat["platforms"].as_array().unwrap().iter().find(|p| p["id"] == "reddit").unwrap();
    let facebook = cat["platforms"].as_array().unwrap().iter().find(|p| p["id"] == "facebook").unwrap();
    assert_eq!(reddit["archiveEnabled"], true);
    assert_eq!(facebook["archiveEnabled"], false);
}

#[tokio::test]
async fn webhook_signature_idempotency_and_account_deletion() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let user = unique_workos_id("del");
    login_web(&app, &mock, &mut jar, &user).await;
    let me = call(&app, Method::GET, "/v1/me", Some(&mut jar), &[], None).await;
    let user_id = Uuid::parse_str(me.json()["user"]["id"].as_str().unwrap()).unwrap();

    // Bad webhook signature → 403.
    let bad = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "bad".into())],
        Some(json!({"id":"evt_1","event":"user.updated","data":{"id": user}})),
    )
    .await;
    assert_eq!(bad.status, StatusCode::FORBIDDEN);

    let payload = json!({
        "id": format!("evt_{}", Uuid::new_v4()),
        "event": "user.updated",
        "data": {
            "id": user,
            "first_name": "Updated",
            "last_name": "Name",
            "email": format!("{user}@example.test"),
            "profile_picture_url": "https://cdn.example/a.png"
        }
    });
    let ok1 = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "sig_test_ok".into())],
        Some(payload.clone()),
    )
    .await;
    assert_eq!(ok1.status, StatusCode::OK, "{}", ok1.text());
    assert_eq!(ok1.json()["ok"], true);

    let ok2 = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "sig_test_ok".into())],
        Some(payload),
    )
    .await;
    assert_eq!(ok2.status, StatusCode::OK);
    assert_eq!(ok2.json()["ok"], true);
    let verify_calls = mock.inner.lock().unwrap().verify_calls;
    assert!(verify_calls >= 2);

    // Account deletion enqueues purge + tombstone + revokes session.
    let csrf = fetch_csrf(&app, &mut jar).await;
    let del = call(
        &app,
        Method::DELETE,
        "/v1/me",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        None,
    )
    .await;
    assert_eq!(del.status, StatusCode::ACCEPTED, "{}", del.text());
    assert_eq!(del.json()["status"], "deletion_scheduled");
    assert!(del.json().get("purgeDeadline").is_some());

    let me_after = call(&app, Method::GET, "/v1/me", Some(&mut jar), &[], None).await;
    assert_eq!(me_after.status, StatusCode::UNAUTHORIZED);

    let deleted_at: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
        "SELECT deleted_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(deleted_at.is_some());

    let purge_count: i64 = sqlx::query_scalar(
        r#"
SELECT COUNT(*)::bigint FROM work_items
WHERE kind = 'account.purge' AND subject_user_id = $1 AND dedupe_key = $2
"#,
    )
    .bind(user_id)
    .bind(user_id.to_string())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(purge_count, 1);

    // Tombstoned user cannot exchange again.
    let mut jar2 = CookieJar::new();
    let authz = authorize_web(&app, &mut jar2).await;
    let code = format!("code_{}", Uuid::new_v4());
    let refresh = format!("refresh_tombstone_{user}");
    mock.register_code(
        &code,
        provider_session(&user, &format!("{user}@example.test"), "Gone", &refresh),
    );
    let blocked = exchange_web(
        &app,
        &mut jar2,
        &code,
        authz["state"].as_str().unwrap(),
    )
    .await;
    assert_eq!(blocked.status, StatusCode::GONE, "{}", blocked.text());
    assert_eq!(
        blocked.problem_code().as_deref(),
        Some("ACCOUNT_DELETION_PENDING")
    );
}

#[tokio::test]
async fn readiness_does_not_call_workos() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);
    // Poison provider — ready must still succeed without touching it.
    mock.inner.lock().unwrap().force_exchange_err = Some("should not be called".into());
    mock.inner.lock().unwrap().force_refresh_err = Some("should not be called".into());
    mock.inner.lock().unwrap().force_verify_err = Some("should not be called".into());

    let before = mock.workos_call_count();
    let ready = call(&app, Method::GET, "/health/ready", None, &[], None).await;
    assert_eq!(ready.status, StatusCode::OK, "{}", ready.text());
    assert_eq!(ready.json()["status"], "ready");
    assert_eq!(mock.workos_call_count(), before);
}

#[tokio::test]
async fn concurrent_workos_user_provisions_single_tenant() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = Arc::new(build_app(pool.clone(), mock.clone(), KEY_K1_JSON));

    let workos_user = unique_workos_id("concurrent");
    let authz_a = authorize_native(&app).await;
    let authz_b = authorize_native(&app).await;

    let code_a = format!("code_a_{}", Uuid::new_v4());
    let code_b = format!("code_b_{}", Uuid::new_v4());
    let session_a = provider_session(
        &workos_user,
        &format!("{workos_user}@example.test"),
        "Concurrent User",
        &format!("refresh_a_{workos_user}"),
    );
    let session_b = provider_session(
        &workos_user,
        &format!("{workos_user}@example.test"),
        "Concurrent User",
        &format!("refresh_b_{workos_user}"),
    );
    mock.register_code(&code_a, session_a);
    mock.register_code(&code_b, session_b);

    let app_a = app.clone();
    let app_b = app.clone();
    let state_a = authz_a["state"].as_str().unwrap().to_string();
    let secret_a = authz_a["exchangeSecret"].as_str().unwrap().to_string();
    let state_b = authz_b["state"].as_str().unwrap().to_string();
    let secret_b = authz_b["exchangeSecret"].as_str().unwrap().to_string();

    let (ra, rb) = tokio::join!(
        async move { exchange_native(&app_a, &code_a, &state_a, &secret_a).await },
        async move { exchange_native(&app_b, &code_b, &state_b, &secret_b).await },
    );

    assert!(
        ra.status == StatusCode::OK || rb.status == StatusCode::OK,
        "at least one exchange must succeed: {} / {}",
        ra.text(),
        rb.text()
    );
    // Unique index may cause one to fail transiently; both OK is ideal after retry path.
    let mut user_ids = Vec::new();
    for res in [&ra, &rb] {
        if res.status == StatusCode::OK {
            user_ids.push(res.json()["user"]["id"].as_str().unwrap().to_string());
        }
    }
    user_ids.sort();
    user_ids.dedup();
    assert_eq!(user_ids.len(), 1, "must resolve to one user id: {user_ids:?}");

    let tenants: i64 = sqlx::query_scalar(
        r#"
SELECT COUNT(DISTINCT tenant_id)::bigint
FROM users
WHERE workos_user_id = $1 AND deleted_at IS NULL
"#,
    )
    .bind(&workos_user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tenants, 1);

    let users: i64 = sqlx::query_scalar(
        r#"
SELECT COUNT(*)::bigint FROM users
WHERE workos_user_id = $1 AND deleted_at IS NULL
"#,
    )
    .bind(&workos_user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(users, 1);
}

#[tokio::test]
async fn fetch_metadata_and_missing_csrf_rejected() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);
    let mut jar = CookieJar::new();
    login_web(&app, &mock, &mut jar, &unique_workos_id("fetchmeta")).await;
    let csrf = fetch_csrf(&app, &mut jar).await;

    let missing_csrf = call(
        &app,
        Method::POST,
        "/v1/auth/logout",
        Some(&mut jar),
        &[
            ("origin", WEB_ORIGIN.into()),
            ("sec-fetch-site", "same-origin".into()),
            ("sec-fetch-mode", "cors".into()),
        ],
        None,
    )
    .await;
    assert_eq!(missing_csrf.status, StatusCode::FORBIDDEN);

    let bad_site = call(
        &app,
        Method::POST,
        "/v1/auth/logout",
        Some(&mut jar),
        &[
            ("origin", WEB_ORIGIN.into()),
            ("sec-fetch-site", "cross-site".into()),
            ("sec-fetch-mode", "cors".into()),
            ("x-csrf-token", csrf.clone()),
        ],
        None,
    )
    .await;
    assert_eq!(bad_site.status, StatusCode::FORBIDDEN);

    let bad_mode = call(
        &app,
        Method::POST,
        "/v1/auth/logout",
        Some(&mut jar),
        &[
            ("origin", WEB_ORIGIN.into()),
            ("sec-fetch-site", "same-origin".into()),
            ("sec-fetch-mode", "navigate".into()),
            ("x-csrf-token", csrf),
        ],
        None,
    )
    .await;
    assert_eq!(bad_mode.status, StatusCode::FORBIDDEN);
}
