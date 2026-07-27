//! Plan 003 security remediation regressions.
//!
//! Evidence-backed defects against the live auth implementation:
//! 1. Web mutation Fetch Metadata allows missing `Sec-Fetch-*` headers.
//! 2. Webhook idempotency claim commits before handler success (poison on failure).
//! 3. Exchange/refresh/logout unseal only the current `WORKOS_COOKIE_PASSWORD`.
//! 4. Provider/DB/webhook failures echo raw internal/secret strings in problem details.
//!
//! These tests are expected to fail until production remediation lands.

mod common;

use axum::http::{Method, StatusCode};
use common::*;
use ghostpost_backend::auth::crypto::{derive_key32, open_bytes};
use serde_json::json;
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, previous }
    }

    fn unset(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

const PASSWORD_PREVIOUS: &str = "previous-workos-cookie-password-aaa";
const PASSWORD_CURRENT: &str = "current-workos-cookie-password-bbbb";

fn origin_csrf_only(csrf: &str) -> Vec<(&'static str, String)> {
    vec![
        ("origin", WEB_ORIGIN.to_string()),
        ("x-csrf-token", csrf.to_string()),
    ]
}

async fn latest_sealed_session(pool: &sqlx::PgPool, user_id: Uuid) -> Vec<u8> {
    sqlx::query_scalar(
        r#"
SELECT sealed_session
FROM workos_sessions
WHERE user_id = $1 AND revoked_at IS NULL
ORDER BY created_at DESC
LIMIT 1
"#,
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("sealed session row")
}

async fn webhook_event_claimed(pool: &sqlx::PgPool, event_id: &str) -> bool {
    let count: i64 = sqlx::query_scalar(
        r#"
SELECT COUNT(*)::bigint
FROM webhook_events
WHERE provider = 'workos' AND event_id = $1
"#,
    )
    .bind(event_id)
    .fetch_one(pool)
    .await
    .expect("webhook_events count");
    count > 0
}

/// Defect 1: missing Sec-Fetch-Site and/or Sec-Fetch-Mode must reject authenticated
/// web mutations even when Origin + CSRF are valid.
#[tokio::test]
async fn web_mutations_reject_missing_sec_fetch_headers() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    login_web(&app, &mock, &mut jar, &unique_workos_id("secfetch")).await;
    let csrf = fetch_csrf(&app, &mut jar).await;

    let body = json!({
        "status": "in_progress",
        "currentStep": 2,
        "revision": 0,
        "answers": {
            "comingUp": ["college_apps"],
            "concerns": ["public_image"],
            "platforms": ["x"],
            "disclosureConsent": { "version": DISCLOSURE_VERSION, "accepted": false }
        }
    });

    let cases = [
        (
            "missing both Sec-Fetch-Site and Sec-Fetch-Mode",
            origin_csrf_only(&csrf),
        ),
        (
            "missing Sec-Fetch-Site only",
            vec![
                ("origin", WEB_ORIGIN.to_string()),
                ("sec-fetch-mode", "cors".into()),
                ("x-csrf-token", csrf.clone()),
            ],
        ),
        (
            "missing Sec-Fetch-Mode only",
            vec![
                ("origin", WEB_ORIGIN.to_string()),
                ("sec-fetch-site", "same-origin".into()),
                ("x-csrf-token", csrf.clone()),
            ],
        ),
    ];

    for (label, headers) in cases {
        let res = call(
            &app,
            Method::PUT,
            "/v1/me/onboarding",
            Some(&mut jar),
            &headers,
            Some(body.clone()),
        )
        .await;
        assert_eq!(
            res.status,
            StatusCode::FORBIDDEN,
            "{label}: expected 403, got {} body={}",
            res.status,
            res.text()
        );
        res.assert_problem_json();
        assert_eq!(res.problem_code().as_deref(), Some("FORBIDDEN"));
    }

    // Control: full Fetch Metadata still allowed.
    let ok = call(
        &app,
        Method::PUT,
        "/v1/me/onboarding",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        Some(body),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "control mutation: {}", ok.text());
}

/// Defect 2: handler failure after idempotency claim must not poison the event id;
/// a retry of the same event id must apply side effects exactly once.
#[tokio::test]
async fn webhook_failure_after_claim_allows_retry_exactly_once() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool.clone(), mock.clone(), KEY_K1_JSON);

    let mut jar = CookieJar::new();
    let workos_user = unique_workos_id("whpoison");
    login_web(&app, &mock, &mut jar, &workos_user).await;
    let me = call(&app, Method::GET, "/v1/me", Some(&mut jar), &[], None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let user_id = Uuid::parse_str(me.json()["user"]["id"].as_str().unwrap()).unwrap();

    let event_id = format!("evt_retry_{}", Uuid::new_v4());

    // Fail after claim: verified event, but handler cannot process (missing user id).
    let poisoned = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "sig_test_ok".into())],
        Some(json!({
            "id": event_id,
            "event": "user.updated",
            "data": {
                "first_name": "Should",
                "last_name": "NotApply"
            }
        })),
    )
    .await;
    assert!(
        poisoned.status.is_client_error() || poisoned.status.is_server_error(),
        "processing failure must not succeed: {} {}",
        poisoned.status,
        poisoned.text()
    );
    assert!(
        !webhook_event_claimed(&pool, &event_id).await,
        "failed webhook must roll back / not claim event_id={event_id}"
    );

    let success_payload = json!({
        "id": event_id,
        "event": "user.updated",
        "data": {
            "id": workos_user,
            "first_name": "Retry",
            "last_name": "Applied",
            "email": format!("{workos_user}@example.test"),
            "profile_picture_url": "https://cdn.example/retry.png"
        }
    });

    let ok = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "sig_test_ok".into())],
        Some(success_payload.clone()),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "retry must succeed: {}", ok.text());
    assert_eq!(ok.json()["ok"], true);
    assert!(
        webhook_event_claimed(&pool, &event_id).await,
        "successful processing claims event_id"
    );

    let display_name: Option<String> = sqlx::query_scalar(
        "SELECT display_name FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        display_name.as_deref(),
        Some("Retry Applied"),
        "retry must apply side effects"
    );

    let avatar: Option<String> = sqlx::query_scalar(
        "SELECT avatar_url FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(avatar.as_deref(), Some("https://cdn.example/retry.png"));

    // Replay: idempotent success, side effects remain exactly-once.
    let replay = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "sig_test_ok".into())],
        Some(json!({
            "id": event_id,
            "event": "user.updated",
            "data": {
                "id": workos_user,
                "first_name": "Second",
                "last_name": "Apply",
                "email": format!("{workos_user}@example.test"),
                "profile_picture_url": "https://cdn.example/second.png"
            }
        })),
    )
    .await;
    assert_eq!(replay.status, StatusCode::OK, "{}", replay.text());
    assert_eq!(replay.json()["ok"], true);

    let display_name_after: Option<String> = sqlx::query_scalar(
        "SELECT display_name FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        display_name_after.as_deref(),
        Some("Retry Applied"),
        "replay must not re-apply side effects"
    );
    let avatar_after: Option<String> = sqlx::query_scalar(
        "SELECT avatar_url FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(avatar_after.as_deref(), Some("https://cdn.example/retry.png"));
}

/// Defect 3a: exchange must unseal PKCE verifier encrypted under previous password.
#[tokio::test]
async fn cookie_password_rotation_exchange_unseals_previous() {
    ensure_migrated().await;
    let _env = lock_env();
    let _clear_previous = EnvVarGuard::unset("WORKOS_COOKIE_PASSWORD_PREVIOUS");

    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app_prev = build_app_with_config(
        pool.clone(),
        mock.clone(),
        auth_config_with_password(PASSWORD_PREVIOUS, KEY_K1_JSON),
    );
    let mut jar = CookieJar::new();
    let authz = authorize_web(&app_prev, &mut jar).await;
    let state = authz["state"].as_str().unwrap().to_string();
    let user = unique_workos_id("sealrot_ex");
    let code = format!("code_{}", Uuid::new_v4());
    let refresh_token = format!("refresh_{user}");
    let session = provider_session(
        &user,
        &format!("{user}@example.test"),
        "Seal Rot Exchange",
        &refresh_token,
    );
    mock.register_code(&code, session.clone());
    mock.register_refresh(
        &refresh_token,
        ProviderAuthSession {
            access_token: format!("access2_{user}"),
            refresh_token: format!("refresh2_{user}"),
            ..session
        },
    );

    let _previous = EnvVarGuard::set("WORKOS_COOKIE_PASSWORD_PREVIOUS", PASSWORD_PREVIOUS);
    let app_rot = build_app_with_config(
        pool.clone(),
        mock.clone(),
        auth_config_with_password(PASSWORD_CURRENT, KEY_K1_JSON),
    );
    let exchanged = exchange_web(&app_rot, &mut jar, &code, &state).await;
    assert_eq!(
        exchanged.status,
        StatusCode::NO_CONTENT,
        "exchange must unseal PKCE verifier under previous password: {}",
        exchanged.text()
    );
    assert!(jar.get("gp_session").is_some());

    let me = call(&app_rot, Method::GET, "/v1/me", Some(&mut jar), &[], None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text());
    let user_id = Uuid::parse_str(me.json()["user"]["id"].as_str().unwrap()).unwrap();
    let sealed = latest_sealed_session(&pool, user_id).await;
    assert!(
        open_bytes(&derive_key32(PASSWORD_CURRENT), &sealed).is_ok(),
        "new workos seal after exchange must use current password"
    );
    assert!(
        open_bytes(&derive_key32(PASSWORD_PREVIOUS), &sealed).is_err(),
        "new workos seal must not remain under previous password"
    );
}

/// Defect 3b: refresh must unseal workos session under previous and re-seal with current.
#[tokio::test]
async fn cookie_password_rotation_refresh_unseals_previous() {
    ensure_migrated().await;
    let _env = lock_env();
    let _clear_previous = EnvVarGuard::unset("WORKOS_COOKIE_PASSWORD_PREVIOUS");

    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app_prev = build_app_with_config(
        pool.clone(),
        mock.clone(),
        auth_config_with_password(PASSWORD_PREVIOUS, KEY_K1_JSON),
    );
    let mut jar = CookieJar::new();
    let user = unique_workos_id("sealrot_rf");
    login_web(&app_prev, &mock, &mut jar, &user).await;
    let me = call(&app_prev, Method::GET, "/v1/me", Some(&mut jar), &[], None).await;
    let user_id = Uuid::parse_str(me.json()["user"]["id"].as_str().unwrap()).unwrap();
    let sealed_before = latest_sealed_session(&pool, user_id).await;
    assert!(
        open_bytes(&derive_key32(PASSWORD_PREVIOUS), &sealed_before).is_ok(),
        "pre-rotation seal must open with previous password"
    );

    let csrf = fetch_csrf(&app_prev, &mut jar).await;
    let _previous = EnvVarGuard::set("WORKOS_COOKIE_PASSWORD_PREVIOUS", PASSWORD_PREVIOUS);
    let app_rot = build_app_with_config(
        pool.clone(),
        mock.clone(),
        auth_config_with_password(PASSWORD_CURRENT, KEY_K1_JSON),
    );
    let refreshed = call(
        &app_rot,
        Method::POST,
        "/v1/auth/refresh",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        None,
    )
    .await;
    assert_eq!(
        refreshed.status,
        StatusCode::NO_CONTENT,
        "refresh must unseal previous-password ciphertext: {}",
        refreshed.text()
    );
    let sealed_after = latest_sealed_session(&pool, user_id).await;
    assert!(
        open_bytes(&derive_key32(PASSWORD_CURRENT), &sealed_after).is_ok(),
        "refresh must re-seal under current password"
    );
    assert!(
        open_bytes(&derive_key32(PASSWORD_PREVIOUS), &sealed_after).is_err(),
        "refresh must not leave ciphertext under previous password"
    );
}

/// Defect 3c: logout must unseal previous-password ciphertext to revoke the provider session.
#[tokio::test]
async fn cookie_password_rotation_logout_unseals_previous() {
    ensure_migrated().await;
    let _env = lock_env();
    let _clear_previous = EnvVarGuard::unset("WORKOS_COOKIE_PASSWORD_PREVIOUS");

    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app_prev = build_app_with_config(
        pool.clone(),
        mock.clone(),
        auth_config_with_password(PASSWORD_PREVIOUS, KEY_K1_JSON),
    );
    let mut jar = CookieJar::new();
    let user = unique_workos_id("sealrot_lo");
    login_web(&app_prev, &mock, &mut jar, &user).await;
    let csrf = fetch_csrf(&app_prev, &mut jar).await;
    let revoke_before = mock.inner.lock().unwrap().revoke_calls;

    let _previous = EnvVarGuard::set("WORKOS_COOKIE_PASSWORD_PREVIOUS", PASSWORD_PREVIOUS);
    let app_rot = build_app_with_config(
        pool.clone(),
        mock.clone(),
        auth_config_with_password(PASSWORD_CURRENT, KEY_K1_JSON),
    );
    let logged_out = call(
        &app_rot,
        Method::POST,
        "/v1/auth/logout",
        Some(&mut jar),
        &web_mutation_headers(&csrf),
        None,
    )
    .await;
    assert_eq!(
        logged_out.status,
        StatusCode::NO_CONTENT,
        "logout: {}",
        logged_out.text()
    );
    let revoke_after = mock.inner.lock().unwrap().revoke_calls;
    assert!(
        revoke_after > revoke_before,
        "logout must unseal previous-password session to revoke provider session \
         (revoke_calls {revoke_before} -> {revoke_after})"
    );
}

/// Defect 4a: provider exchange errors must not echo raw provider/secret strings.
#[tokio::test]
async fn provider_exchange_errors_are_sanitized() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);

    let provider_canary = "sk_live_LEAKED_PROVIDER_SECRET_do_not_echo";
    mock.inner.lock().unwrap().force_exchange_err = Some(provider_canary.into());
    let authz = authorize_native(&app).await;
    let exchange_fail = exchange_native(
        &app,
        "nope",
        authz["state"].as_str().unwrap(),
        authz["exchangeSecret"].as_str().unwrap(),
    )
    .await;
    assert!(
        exchange_fail.status.is_server_error(),
        "provider exchange failure status: {} {}",
        exchange_fail.status,
        exchange_fail.text()
    );
    exchange_fail.assert_problem_json();
    exchange_fail.assert_no_leak(&[provider_canary, "sk_live_", "LEAKED_PROVIDER"]);
    let exchange_code = exchange_fail.problem_code().unwrap();
    assert!(
        matches!(
            exchange_code.as_str(),
            "INTERNAL" | "PROVIDER_ERROR" | "PROVIDER_UNAVAILABLE" | "AUTH_PROVIDER_ERROR"
        ),
        "exchange failure must use a stable sanitized code, got {exchange_code}"
    );
}

/// Defect 4b: refresh provider/DB-shaped errors must not echo internal strings.
#[tokio::test]
async fn provider_refresh_errors_are_sanitized() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);

    let db_canary = "password=supersecret relation \"auth_flows\" does not exist";
    let user = unique_workos_id("sanitize_rf");
    let (_body, token) = login_native(&app, &mock, &user).await;
    mock.inner.lock().unwrap().force_refresh_err = Some(db_canary.into());
    let refresh_fail = call(
        &app,
        Method::POST,
        "/v1/auth/refresh",
        None,
        &bearer_headers(&token),
        None,
    )
    .await;
    assert!(
        refresh_fail.status.is_client_error() || refresh_fail.status.is_server_error(),
        "refresh provider failure status: {} {}",
        refresh_fail.status,
        refresh_fail.text()
    );
    refresh_fail.assert_problem_json();
    refresh_fail.assert_no_leak(&[
        db_canary,
        "supersecret",
        "auth_flows",
        "password=",
        "does not exist",
    ]);
    let refresh_code = refresh_fail.problem_code().unwrap();
    assert!(
        matches!(
            refresh_code.as_str(),
            "UNAUTHORIZED"
                | "INTERNAL"
                | "PROVIDER_ERROR"
                | "PROVIDER_UNAVAILABLE"
                | "AUTH_PROVIDER_ERROR"
        ),
        "refresh failure must use a stable sanitized code, got {refresh_code}"
    );
}

/// Defect 4c: webhook verification errors must not echo raw verifier/secret strings.
#[tokio::test]
async fn webhook_verify_errors_are_sanitized() {
    ensure_migrated().await;
    let pool = app_pool().await;
    let mock = Arc::new(MockWorkos::new());
    let app = build_app(pool, mock.clone(), KEY_K1_JSON);

    let webhook_canary = "whsec_LEAKED_WEBHOOK_SECRET_raw_verifier_error";
    mock.inner.lock().unwrap().force_verify_err = Some(webhook_canary.into());
    let webhook_fail = call(
        &app,
        Method::POST,
        "/v1/webhooks/workos",
        None,
        &[("workos-signature", "sig_test_ok".into())],
        Some(json!({
            "id": format!("evt_sanitize_{}", Uuid::new_v4()),
            "event": "user.updated",
            "data": { "id": "user_unused" }
        })),
    )
    .await;
    assert_eq!(
        webhook_fail.status,
        StatusCode::FORBIDDEN,
        "webhook verify failure: {}",
        webhook_fail.text()
    );
    webhook_fail.assert_problem_json();
    webhook_fail.assert_no_leak(&[
        webhook_canary,
        "whsec_LEAKED",
        "LEAKED_WEBHOOK",
        "raw_verifier_error",
    ]);
    let webhook_code = webhook_fail.problem_code().unwrap();
    assert!(
        matches!(
            webhook_code.as_str(),
            "FORBIDDEN" | "WEBHOOK_INVALID" | "WEBHOOK_SIGNATURE_INVALID"
        ),
        "webhook verify failure must use a stable sanitized code, got {webhook_code}"
    );
}
