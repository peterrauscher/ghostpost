use crate::auth::cookies::{parse_cookie, SESSION_COOKIE};
use crate::auth::problem::AuthProblem;
use crate::auth::service::AuthService;
use crate::auth::types::{AppSession, AuthClient, TenantContext};
use crate::api::router::AppState;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, Method};
use std::sync::Arc;

pub struct AuthSession(pub AppSession);
pub struct Tenant(pub TenantContext);

fn extract_raw_token(parts: &Parts) -> Option<String> {
    if let Some(auth) = parts.headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            return Some(token.to_string());
        }
    }
    let cookie = parts
        .headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok());
    parse_cookie(cookie, SESSION_COOKIE).map(|s| s.to_string())
}

impl FromRequestParts<AppState> for AuthSession {
    type Rejection = AuthProblem;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let raw = extract_raw_token(parts)
            .ok_or_else(|| AuthProblem::unauthorized("missing session"))?;
        let session = state.auth.resolve_session_token(&raw).await?;
        Ok(AuthSession(session))
    }
}

impl FromRequestParts<AppState> for Tenant {
    type Rejection = AuthProblem;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthSession(session) = AuthSession::from_request_parts(parts, state).await?;
        Ok(Tenant(TenantContext {
            tenant_id: session.tenant_id,
            user_id: session.user_id,
        }))
    }
}

pub async fn enforce_web_mutation_guards(
    parts: &Parts,
    state: &AppState,
    session: &AppSession,
) -> Result<(), AuthProblem> {
    if session.client != AuthClient::Web {
        return Ok(());
    }
    if !matches!(
        parts.method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return Ok(());
    }

    let origin = parts
        .headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok());
    match origin {
        Some(origin) if state.auth.config.origin_allowed(origin) => {}
        _ => return Err(AuthProblem::forbidden("invalid origin")),
    }

    let site = parts
        .headers
        .get("sec-fetch-site")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthProblem::forbidden("missing sec-fetch-site"))?;
    if site != "same-origin" && site != "same-site" {
        return Err(AuthProblem::forbidden("invalid sec-fetch-site"));
    }

    let mode = parts
        .headers
        .get("sec-fetch-mode")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthProblem::forbidden("missing sec-fetch-mode"))?;
    if mode != "cors" && mode != "same-origin" {
        return Err(AuthProblem::forbidden("invalid sec-fetch-mode"));
    }

    let csrf = parts
        .headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthProblem::forbidden("missing CSRF token"))?;
    state.auth.validate_csrf(session, csrf).await
}

#[allow(dead_code)]
pub fn auth_service(state: &AppState) -> Arc<AuthService> {
    state.auth.clone()
}
