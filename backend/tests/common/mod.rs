//! Shared Plan 003 auth integration harness (mock WorkOS + axum Router).

#![allow(dead_code)]

pub mod blob;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, Method, Request, StatusCode};
use axum::Router;
use ghostpost_backend::api::router::{self, AppState};
use ghostpost_backend::blob::BlobStore;
use ghostpost_backend::auth::crypto::derive_key32;
use ghostpost_backend::auth::keys::SessionKeyring;
use ghostpost_backend::auth::{
    AuthConfig, AuthService, AuthorizeUrlRequest, ProviderError, ProviderUser,
    VerifiedWebhookEvent, WorkosIdentityProvider,
};
pub use ghostpost_backend::auth::ProviderAuthSession;
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use uuid::Uuid;


pub const WEB_ORIGIN: &str = "http://localhost:8081";
pub const DISCLOSURE_VERSION: &str = "2026-07-23";

pub const KEY_K1_JSON: &str =
    r#"[{"id":"k1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#;
pub const KEY_K2_FRONT_JSON: &str = concat!(
    r#"[{"id":"k2","secret":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="},"#,
    r#"{"id":"k1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#
);

#[derive(Default)]
pub struct MockInner {
    pub sessions_by_code: HashMap<String, ProviderAuthSession>,
    pub refresh_by_token: HashMap<String, ProviderAuthSession>,
    pub force_exchange_err: Option<String>,
    pub force_refresh_err: Option<String>,
    pub force_verify_err: Option<String>,
    pub auth_url_calls: u64,
    pub exchange_calls: u64,
    pub refresh_calls: u64,
    pub revoke_calls: u64,
    pub delete_calls: u64,
    pub verify_calls: u64,
}

#[derive(Clone, Default)]
pub struct MockWorkos {
    pub inner: Arc<Mutex<MockInner>>,
}

impl MockWorkos {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_code(&self, code: &str, session: ProviderAuthSession) {
        self.inner
            .lock()
            .unwrap()
            .sessions_by_code
            .insert(code.to_string(), session);
    }

    pub fn register_refresh(&self, refresh_token: &str, session: ProviderAuthSession) {
        self.inner
            .lock()
            .unwrap()
            .refresh_by_token
            .insert(refresh_token.to_string(), session);
    }

    pub fn workos_call_count(&self) -> u64 {
        let g = self.inner.lock().unwrap();
        g.auth_url_calls
            + g.exchange_calls
            + g.refresh_calls
            + g.revoke_calls
            + g.delete_calls
            + g.verify_calls
    }
}

#[async_trait]
impl WorkosIdentityProvider for MockWorkos {
    async fn authorization_url(&self, req: AuthorizeUrlRequest) -> Result<String, ProviderError> {
        let mut g = self.inner.lock().unwrap();
        g.auth_url_calls += 1;
        Ok(format!(
            "https://api.workos.test/sso/authorize?state={}&redirect_uri={}&code_challenge={}",
            req.state, req.redirect_uri, req.code_challenge
        ))
    }

    async fn exchange_code(
        &self,
        code: &str,
        _code_verifier: &str,
    ) -> Result<ProviderAuthSession, ProviderError> {
        let mut g = self.inner.lock().unwrap();
        g.exchange_calls += 1;
        if let Some(msg) = g.force_exchange_err.clone() {
            return Err(ProviderError::Request(msg));
        }
        g.sessions_by_code
            .get(code)
            .cloned()
            .ok_or_else(|| ProviderError::Request(format!("unknown code {code}")))
    }

    async fn refresh_session(
        &self,
        refresh_token: &str,
    ) -> Result<ProviderAuthSession, ProviderError> {
        let mut g = self.inner.lock().unwrap();
        g.refresh_calls += 1;
        if let Some(msg) = g.force_refresh_err.clone() {
            return Err(ProviderError::Request(msg));
        }
        g.refresh_by_token
            .get(refresh_token)
            .cloned()
            .ok_or_else(|| ProviderError::Request(format!("unknown refresh {refresh_token}")))
    }

    async fn revoke_session(&self, _workos_session_id: &str) -> Result<(), ProviderError> {
        self.inner.lock().unwrap().revoke_calls += 1;
        Ok(())
    }

    async fn delete_user(&self, _workos_user_id: &str) -> Result<(), ProviderError> {
        self.inner.lock().unwrap().delete_calls += 1;
        Ok(())
    }

    fn verify_webhook(
        &self,
        signature_header: &str,
        body: &[u8],
    ) -> Result<VerifiedWebhookEvent, ProviderError> {
        let mut g = self.inner.lock().unwrap();
        g.verify_calls += 1;
        if let Some(msg) = g.force_verify_err.clone() {
            return Err(ProviderError::Webhook(msg));
        }
        if signature_header != "sig_test_ok" {
            return Err(ProviderError::Webhook("bad signature".into()));
        }
        let v: Value = serde_json::from_slice(body)
            .map_err(|err| ProviderError::Webhook(err.to_string()))?;
        Ok(VerifiedWebhookEvent {
            event_id: v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("evt_missing")
                .to_string(),
            event_type: v
                .get("event")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string(),
            data: v.get("data").cloned().unwrap_or(json!({})),
        })
    }
}

pub fn provider_user(workos_user_id: &str, email: &str, name: &str) -> ProviderUser {
    ProviderUser {
        workos_user_id: workos_user_id.to_string(),
        email: email.to_string(),
        display_name: name.to_string(),
        first_name: name.split_whitespace().next().map(|s| s.to_string()),
        avatar_url: None,
    }
}

pub fn provider_session(
    workos_user_id: &str,
    email: &str,
    name: &str,
    refresh_token: &str,
) -> ProviderAuthSession {
    ProviderAuthSession {
        user: provider_user(workos_user_id, email, name),
        access_token: format!("access_{workos_user_id}_{}", Uuid::new_v4()),
        refresh_token: refresh_token.to_string(),
        // Unique per mint so re-login / parallel flows never collide on
        // workos_sessions(workos_session_id) partial unique index.
        workos_session_id: Some(format!("wos_sess_{}", Uuid::new_v4())),
    }
}

pub fn test_env() {
    let _ = dotenvy::from_filename(".env");
    let _ = dotenvy::from_filename("backend/.env");
    if std::env::var("DATABASE_URL").is_err() {
        std::env::set_var(
            "DATABASE_URL",
            "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost",
        );
    }
    if std::env::var("DATABASE_URL_APP").is_err() {
        std::env::set_var(
            "DATABASE_URL_APP",
            "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost",
        );
    }
    if std::env::var("DATABASE_APP_ROLE").is_err() {
        std::env::set_var("DATABASE_APP_ROLE", "ghostpost_app");
    }
    if std::env::var("GHOSTPOST_ENV").is_err() {
        std::env::set_var("GHOSTPOST_ENV", "development");
    }
}

pub async fn ensure_migrated() {
    test_env();
    let config = Config::load_for_migrate().expect("config");
    migrate::run(&config, false).await.expect("migrate");
}

pub async fn app_pool() -> PgPool {
    test_env();
    let url = std::env::var("DATABASE_URL_APP").expect("DATABASE_URL_APP");
    PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await
        .expect("connect app pool")
}

pub const DEFAULT_WORKOS_COOKIE_PASSWORD: &str = "correct-horse-battery-staple-secret!!";

pub fn auth_config_with_keys(session_keys_json: &str) -> AuthConfig {
    auth_config_with_password(DEFAULT_WORKOS_COOKIE_PASSWORD, session_keys_json)
}

pub fn auth_config_with_password(password: &str, session_keys_json: &str) -> AuthConfig {
    assert!(
        password.len() >= 32,
        "WORKOS_COOKIE_PASSWORD must be at least 32 characters"
    );
    AuthConfig {
        workos_api_key: "sk_test_local".into(),
        workos_client_id: "client_local".into(),
        workos_webhook_secret: "whsec_local_test_secret".into(),
        workos_cookie_password: password.into(),
        workos_seal_key: derive_key32(password),
        workos_seal_key_version: "workos_v1".into(),
        session_keys: SessionKeyring::parse(session_keys_json).expect("session keys"),
        auth_web_redirect_uri: "http://localhost:8081/auth/callback".into(),
        auth_native_redirect_uri: "ghostpost://auth/callback".into(),
        auth_web_origins: vec![
            "http://localhost:8081".into(),
            "http://127.0.0.1:8081".into(),
        ],
        cors_allowed_origins: vec![
            "http://localhost:8081".into(),
            "http://127.0.0.1:8081".into(),
        ],
        auth_init_cookie_max_age_secs: 600,
        app_session_max_age_secs: 2_592_000,
        csrf_token_max_age_secs: 3600,
        secure_cookies: false,
    }
}

pub fn build_app(
    pool: PgPool,
    provider: Arc<dyn WorkosIdentityProvider>,
    keys_json: &str,
) -> Router {
    build_app_with_config(pool, provider, auth_config_with_keys(keys_json))
}

pub fn build_app_with_blob_store(
    pool: PgPool,
    provider: Arc<dyn WorkosIdentityProvider>,
    keys_json: &str,
    blob_store: Arc<dyn BlobStore>,
) -> Router {
    let config = auth_config_with_keys(keys_json);
    let auth = Arc::new(AuthService::new(pool.clone(), config, provider));
    router::build_with_blob_store(
        AppState {
            pool,
            restore_replay_pending: false,
            worker_ready: true,
            auth,
        },
        blob_store,
    )
}

pub fn build_app_with_config(
    pool: PgPool,
    provider: Arc<dyn WorkosIdentityProvider>,
    config: AuthConfig,
) -> Router {
    let auth = Arc::new(AuthService::new(pool.clone(), config, provider));
    router::build(AppState {
        pool,
        restore_replay_pending: false,
        worker_ready: true,
        auth,
    })
}

#[derive(Clone, Default)]
pub struct CookieJar {
    cookies: HashMap<String, String>,
}

impl CookieJar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn absorb_set_cookie(&mut self, headers: &HeaderMap) {
        for value in headers.get_all(header::SET_COOKIE) {
            let Ok(raw) = value.to_str() else {
                continue;
            };
            let Some(pair) = raw.split(';').next() else {
                continue;
            };
            let Some((name, val)) = pair.split_once('=') else {
                continue;
            };
            if val.is_empty() {
                self.cookies.remove(name);
            } else {
                self.cookies.insert(name.to_string(), val.to_string());
            }
        }
    }

    pub fn header_value(&self) -> Option<String> {
        if self.cookies.is_empty() {
            return None;
        }
        Some(
            self.cookies
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(|s| s.as_str())
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl TestResponse {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn problem_code(&self) -> Option<String> {
        self.json()
            .get("code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub fn problem_detail(&self) -> Option<String> {
        self.json()
            .get("detail")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    pub fn content_type(&self) -> Option<String> {
        self.headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    pub fn assert_problem_json(&self) {
        let ct = self.content_type().unwrap_or_default();
        assert!(
            ct.starts_with("application/problem+json"),
            "expected application/problem+json, got {ct:?}; body={}",
            self.text()
        );
        assert!(
            self.problem_code().is_some(),
            "problem response must include stable code; body={}",
            self.text()
        );
    }

    pub fn assert_no_leak(&self, canaries: &[&str]) {
        let body = self.text();
        let detail = self.problem_detail().unwrap_or_default();
        let code = self.problem_code().unwrap_or_default();
        for canary in canaries {
            assert!(
                !body.contains(canary),
                "response body leaked canary {canary:?}: {body}"
            );
            assert!(
                !detail.contains(canary),
                "problem detail leaked canary {canary:?}: {detail}"
            );
            assert!(
                !code.contains(canary),
                "problem code leaked canary {canary:?}: {code}"
            );
        }
    }
}

pub async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    jar: Option<&mut CookieJar>,
    extra_headers: &[(&str, String)],
    body: Option<Value>,
) -> TestResponse {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(jar) = jar.as_ref() {
        if let Some(cookie) = jar.header_value() {
            builder = builder.header(header::COOKIE, cookie);
        }
    }
    for (k, v) in extra_headers {
        let name = HeaderName::from_bytes(k.as_bytes()).expect("header name");
        let value = HeaderValue::from_str(v).expect("header value");
        builder = builder.header(name, value);
    }
    let req = if let Some(body) = body {
        builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };

    let response = app.clone().oneshot(req).await.expect("oneshot");
    let status = response.status();
    let headers = response.headers().clone();
    if let Some(jar) = jar {
        jar.absorb_set_cookie(&headers);
    }
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes()
        .to_vec();
    TestResponse {
        status,
        headers,
        body,
    }
}

pub fn web_mutation_headers(csrf: &str) -> Vec<(&str, String)> {
    vec![
        ("origin", WEB_ORIGIN.to_string()),
        ("sec-fetch-site", "same-origin".into()),
        ("sec-fetch-mode", "cors".into()),
        ("x-csrf-token", csrf.to_string()),
    ]
}

pub fn bearer_headers(token: &str) -> Vec<(&str, String)> {
    vec![("authorization", format!("Bearer {token}"))]
}

pub async fn authorize_web(app: &Router, jar: &mut CookieJar) -> Value {
    let res = call(
        app,
        Method::GET,
        "/v1/auth/authorize?client=web",
        Some(jar),
        &[],
        None,
    )
    .await;
    assert_eq!(res.status, StatusCode::OK, "web authorize: {}", res.text());
    assert!(
        !res.status.is_redirection(),
        "authorize must never redirect: {}",
        res.status
    );
    assert!(
        jar.get("gp_auth_init").is_some(),
        "web authorize must set gp_auth_init"
    );
    let body = res.json();
    assert!(body.get("authorizationUrl").is_some());
    assert!(body.get("state").is_some());
    assert!(
        body.get("exchangeSecret").is_none(),
        "web must not leak exchangeSecret in JSON"
    );
    body
}

pub async fn authorize_native(app: &Router) -> Value {
    let res = call(
        app,
        Method::GET,
        "/v1/auth/authorize?client=native",
        None,
        &[],
        None,
    )
    .await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "native authorize: {}",
        res.text()
    );
    assert!(!res.status.is_redirection());
    let body = res.json();
    assert!(body.get("authorizationUrl").is_some());
    assert!(body.get("state").is_some());
    assert!(
        body.get("exchangeSecret")
            .and_then(|v| v.as_str())
            .is_some(),
        "native must include exchangeSecret"
    );
    body
}

pub async fn exchange_web(
    app: &Router,
    jar: &mut CookieJar,
    code: &str,
    state: &str,
) -> TestResponse {
    call(
        app,
        Method::POST,
        "/v1/auth/exchange",
        Some(jar),
        &[],
        Some(json!({
            "client": "web",
            "code": code,
            "state": state
        })),
    )
    .await
}

pub async fn exchange_native(
    app: &Router,
    code: &str,
    state: &str,
    exchange_secret: &str,
) -> TestResponse {
    call(
        app,
        Method::POST,
        "/v1/auth/exchange",
        None,
        &[],
        Some(json!({
            "client": "native",
            "code": code,
            "state": state,
            "exchangeSecret": exchange_secret
        })),
    )
    .await
}

pub async fn fetch_csrf(app: &Router, jar: &mut CookieJar) -> String {
    let res = call(app, Method::GET, "/v1/auth/csrf", Some(jar), &[], None).await;
    assert_eq!(res.status, StatusCode::OK, "csrf: {}", res.text());
    res.json()
        .get("token")
        .and_then(|v| v.as_str())
        .expect("csrf token")
        .to_string()
}

pub async fn login_web(
    app: &Router,
    mock: &MockWorkos,
    jar: &mut CookieJar,
    workos_user_id: &str,
) {
    let authz = authorize_web(app, jar).await;
    let state = authz["state"].as_str().unwrap().to_string();
    let code = format!("code_{}", Uuid::new_v4());
    let refresh = format!("refresh_{workos_user_id}");
    let session = provider_session(
        workos_user_id,
        &format!("{workos_user_id}@example.test"),
        "Test User",
        &refresh,
    );
    mock.register_code(&code, session.clone());
    mock.register_refresh(
        &refresh,
        ProviderAuthSession {
            access_token: format!("access2_{workos_user_id}"),
            refresh_token: format!("refresh2_{workos_user_id}"),
            ..session
        },
    );
    let res = exchange_web(app, jar, &code, &state).await;
    assert_eq!(
        res.status,
        StatusCode::NO_CONTENT,
        "web exchange: {}",
        res.text()
    );
    assert!(res.body.is_empty(), "web exchange must have empty body");
    assert!(
        jar.get("gp_session").is_some(),
        "web exchange must set gp_session"
    );
    assert!(
        jar.get("gp_auth_init").is_none(),
        "gp_auth_init must be cleared"
    );
}

pub async fn login_native(
    app: &Router,
    mock: &MockWorkos,
    workos_user_id: &str,
) -> (Value, String) {
    let authz = authorize_native(app).await;
    let state = authz["state"].as_str().unwrap().to_string();
    let secret = authz["exchangeSecret"].as_str().unwrap().to_string();
    let code = format!("code_{}", Uuid::new_v4());
    let refresh = format!("refresh_{workos_user_id}");
    let session = provider_session(
        workos_user_id,
        &format!("{workos_user_id}@example.test"),
        "Native User",
        &refresh,
    );
    mock.register_code(&code, session.clone());
    mock.register_refresh(
        &refresh,
        ProviderAuthSession {
            access_token: format!("access2_{workos_user_id}"),
            refresh_token: format!("refresh2_{workos_user_id}"),
            ..session
        },
    );
    let res = exchange_native(app, &code, &state, &secret).await;
    assert_eq!(
        res.status,
        StatusCode::OK,
        "native exchange: {}",
        res.text()
    );
    let body = res.json();
    assert_eq!(body["session"]["kind"], "bearer");
    assert!(body["session"]["token"].as_str().is_some());
    assert!(body.get("user").is_some());
    assert!(body.get("accessToken").is_none());
    assert!(body.get("refreshToken").is_none());
    let token = body["session"]["token"].as_str().unwrap().to_string();
    (body, token)
}

pub async fn assert_session_hash_only(pool: &PgPool, raw_token: &str) {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    let digest = hasher.finalize();
    let row: (Vec<u8>,) = sqlx::query_as(
        r#"
SELECT token_hash
FROM auth_sessions
WHERE token_hash = $1 AND revoked_at IS NULL
"#,
    )
    .bind(digest.as_slice())
    .fetch_one(pool)
    .await
    .expect("session hash row");
    assert_eq!(row.0.as_slice(), digest.as_slice());

    let hits: i64 = sqlx::query_scalar(
        r#"
SELECT COUNT(*)::bigint FROM auth_sessions
WHERE key_id = $1 OR client = $1
"#,
    )
    .bind(raw_token)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(hits, 0, "raw session token must not be stored verbatim");
}

pub async fn assert_workos_sealed(pool: &PgPool, tenant_id: Uuid, user_id: Uuid) {
    let row: (Vec<u8>, String) = sqlx::query_as(
        r#"
SELECT sealed_session, seal_key_version
FROM workos_sessions
WHERE tenant_id = $1 AND user_id = $2 AND revoked_at IS NULL
ORDER BY created_at DESC
LIMIT 1
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(pool)
    .await
    .expect("sealed workos session");
    assert!(!row.0.is_empty());
    assert_eq!(row.1, "workos_v1");
    let as_str = String::from_utf8_lossy(&row.0);
    assert!(
        !as_str.contains("refresh_"),
        "sealed_session must not contain plaintext refresh token"
    );
}

pub fn unique_workos_id(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4())
}
