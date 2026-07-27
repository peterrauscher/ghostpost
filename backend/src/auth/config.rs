use crate::auth::crypto::derive_key32;
use crate::auth::keys::SessionKeyring;
use crate::error::{AppError, AppResult};
use std::env;

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub workos_api_key: String,
    pub workos_client_id: String,
    pub workos_webhook_secret: String,
    pub workos_cookie_password: String,
    pub workos_seal_key: [u8; 32],
    pub workos_seal_key_version: String,
    pub session_keys: SessionKeyring,
    pub auth_web_redirect_uri: String,
    pub auth_native_redirect_uri: String,
    pub auth_web_origins: Vec<String>,
    pub cors_allowed_origins: Vec<String>,
    pub auth_init_cookie_max_age_secs: u64,
    pub app_session_max_age_secs: u64,
    pub csrf_token_max_age_secs: u64,
    pub secure_cookies: bool,
}

fn require_env(key: &str) -> AppResult<String> {
    env::var(key).map_err(|_| AppError::Config(format!("missing required env {key}")))
}

fn parse_u64(key: &str, default: u64) -> AppResult<u64> {
    match env::var(key) {
        Ok(raw) => raw
            .parse::<u64>()
            .map_err(|err| AppError::Config(format!("{key} invalid: {err}"))),
        Err(_) => Ok(default),
    }
}

fn normalize_no_trailing_slash(key: &str, raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::Config(format!("{key} must not be empty")));
    }
    if trimmed.contains('*') {
        return Err(AppError::Config(format!(
            "{key} must not contain wildcard '*'"
        )));
    }
    let normalized = trimmed.trim_end_matches('/').to_string();
    if normalized != trimmed {
        // Reject trailing slashes rather than silently differing from registered URIs.
        return Err(AppError::Config(format!(
            "{key} must not have a trailing slash"
        )));
    }
    Ok(normalized)
}

fn parse_csv_origins(key: &str, raw: &str) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        out.push(normalize_no_trailing_slash(key, part)?);
    }
    if out.is_empty() {
        return Err(AppError::Config(format!("{key} must list at least one origin")));
    }
    Ok(out)
}

fn reject_backend_auth_redirect(key: &str, uri: &str) -> AppResult<()> {
    let lower = uri.to_ascii_lowercase();
    if lower.contains("/v1/auth/") {
        return Err(AppError::Config(format!(
            "{key} must be a frontend callback, not a backend /v1/auth/* path"
        )));
    }
    Ok(())
}

impl AuthConfig {
    pub fn load_from_env(_is_development: bool) -> AppResult<Self> {
        let workos_api_key = require_env("WORKOS_API_KEY")?;
        let workos_client_id = require_env("WORKOS_CLIENT_ID")?;
        let workos_webhook_secret = require_env("WORKOS_WEBHOOK_SECRET")?;
        let workos_cookie_password = require_env("WORKOS_COOKIE_PASSWORD")?;
        if workos_cookie_password.len() < 32 {
            return Err(AppError::Config(
                "WORKOS_COOKIE_PASSWORD must be at least 32 characters".into(),
            ));
        }
        let session_keys = SessionKeyring::parse(&require_env("APP_SESSION_KEYS")?)?;
        let auth_web_redirect_uri =
            normalize_no_trailing_slash("AUTH_WEB_REDIRECT_URI", &require_env("AUTH_WEB_REDIRECT_URI")?)?;
        let auth_native_redirect_uri = normalize_no_trailing_slash(
            "AUTH_NATIVE_REDIRECT_URI",
            &require_env("AUTH_NATIVE_REDIRECT_URI")?,
        )?;
        reject_backend_auth_redirect("AUTH_WEB_REDIRECT_URI", &auth_web_redirect_uri)?;
        reject_backend_auth_redirect("AUTH_NATIVE_REDIRECT_URI", &auth_native_redirect_uri)?;
        let auth_web_origins = parse_csv_origins("AUTH_WEB_ORIGINS", &require_env("AUTH_WEB_ORIGINS")?)?;
        let cors_allowed_origins =
            parse_csv_origins("CORS_ALLOWED_ORIGINS", &require_env("CORS_ALLOWED_ORIGINS")?)?;
        // Mirror check (order-insensitive).
        let mut a = auth_web_origins.clone();
        let mut b = cors_allowed_origins.clone();
        a.sort();
        b.sort();
        if a != b {
            return Err(AppError::Config(
                "CORS_ALLOWED_ORIGINS must mirror AUTH_WEB_ORIGINS".into(),
            ));
        }

        let seal_version =
            env::var("WORKOS_SEAL_KEY_VERSION").unwrap_or_else(|_| "workos_v1".into());

        Ok(Self {
            workos_api_key,
            workos_client_id,
            workos_webhook_secret,
            workos_seal_key: derive_key32(&workos_cookie_password),
            workos_cookie_password,
            workos_seal_key_version: seal_version,
            session_keys,
            auth_web_redirect_uri,
            auth_native_redirect_uri,
            auth_web_origins,
            cors_allowed_origins,
            auth_init_cookie_max_age_secs: parse_u64("AUTH_INIT_COOKIE_MAX_AGE_SECS", 600)?,
            app_session_max_age_secs: parse_u64("APP_SESSION_MAX_AGE_SECS", 2_592_000)?,
            csrf_token_max_age_secs: parse_u64("CSRF_TOKEN_MAX_AGE_SECS", 3600)?,
            // Default Secure=true per plan. Local HTTP test harness may set AUTH_SECURE_COOKIES=false.
            secure_cookies: env::var("AUTH_SECURE_COOKIES")
                .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "0" | "false" | "no"))
                .unwrap_or(true),
        })
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        self.auth_web_origins.iter().any(|o| o == origin)
    }
}
