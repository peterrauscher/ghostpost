use crate::api::router::AppState;
use crate::auth::catalog::platforms_catalog;
use crate::auth::cookies::{
    clear_auth_init_cookie, clear_session_cookie, parse_cookie, set_auth_init_cookie,
    set_session_cookie, AUTH_INIT_COOKIE,
};
use crate::auth::extract::{enforce_web_mutation_guards, AuthSession};
use crate::auth::problem::AuthProblem;
use crate::auth::types::{
    AuthClient, ExchangeRequest, OnboardingState, WebhookOk,
};
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    pub client: String,
}

pub async fn authorize(
    State(state): State<AppState>,
    Query(query): Query<AuthorizeQuery>,
) -> Result<Response, AuthProblem> {
    let client = AuthClient::parse(&query.client).ok_or_else(|| {
        AuthProblem::bad_request("INVALID_CLIENT", "client must be web or native")
    })?;
    let (body, cookie_secret) = state.auth.authorize(client).await?;
    let mut res = (StatusCode::OK, Json(body)).into_response();
    if let Some(secret) = cookie_secret {
        set_auth_init_cookie(
            &mut res,
            &secret,
            state.auth.config.auth_init_cookie_max_age_secs,
            state.auth.config.secure_cookies,
        );
    }
    Ok(res)
}

pub async fn exchange(
    State(state): State<AppState>,
    parts: Parts,
    Json(body): Json<ExchangeRequest>,
) -> Result<Response, AuthProblem> {
    if body.client == AuthClient::Web && body.exchange_secret.is_some() {
        return Err(AuthProblem::forbidden(
            "web exchange must not include exchangeSecret body field",
        ));
    }
    let proof = match body.client {
        AuthClient::Web => {
            let cookie = parts
                .headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok());
            parse_cookie(cookie, AUTH_INIT_COOKIE).map(|s| s.to_string())
        }
        AuthClient::Native => body.exchange_secret.clone(),
    };
    let (native, web_cookie) = state
        .auth
        .exchange(body.client, &body.code, &body.state, proof.as_deref())
        .await?;

    let mut res = if let Some(payload) = native {
        (StatusCode::OK, Json(payload)).into_response()
    } else {
        StatusCode::NO_CONTENT.into_response()
    };
    clear_auth_init_cookie(&mut res, state.auth.config.secure_cookies);
    if let Some((token, max_age)) = web_cookie {
        set_session_cookie(
            &mut res,
            &token,
            max_age.num_seconds().max(0) as u64,
            state.auth.config.secure_cookies,
        );
    }
    Ok(res)
}

pub async fn refresh(
    State(state): State<AppState>,
    parts: Parts,
    AuthSession(session): AuthSession,
) -> Result<Response, AuthProblem> {
    enforce_web_mutation_guards(&parts, &state, &session).await?;
    let (native, web_cookie) = state.auth.refresh(&session).await?;
    let mut res = if let Some(payload) = native {
        (StatusCode::OK, Json(payload)).into_response()
    } else {
        StatusCode::NO_CONTENT.into_response()
    };
    if let Some((token, max_age)) = web_cookie {
        set_session_cookie(
            &mut res,
            &token,
            max_age.num_seconds().max(0) as u64,
            state.auth.config.secure_cookies,
        );
    }
    Ok(res)
}

pub async fn logout(
    State(state): State<AppState>,
    parts: Parts,
    AuthSession(session): AuthSession,
) -> Result<Response, AuthProblem> {
    enforce_web_mutation_guards(&parts, &state, &session).await?;
    state.auth.logout(&session).await?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    clear_session_cookie(&mut res, state.auth.config.secure_cookies);
    clear_auth_init_cookie(&mut res, state.auth.config.secure_cookies);
    Ok(res)
}

pub async fn csrf(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
) -> Result<impl IntoResponse, AuthProblem> {
    let body = state.auth.issue_csrf(&session).await?;
    Ok(Json(body))
}

pub async fn me(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
) -> Result<impl IntoResponse, AuthProblem> {
    Ok(Json(state.auth.me(&session).await?))
}

pub async fn get_onboarding(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
) -> Result<impl IntoResponse, AuthProblem> {
    Ok(Json(state.auth.get_onboarding(&session).await?))
}

pub async fn put_onboarding(
    State(state): State<AppState>,
    parts: Parts,
    AuthSession(session): AuthSession,
    Json(body): Json<OnboardingState>,
) -> Result<impl IntoResponse, AuthProblem> {
    enforce_web_mutation_guards(&parts, &state, &session).await?;
    Ok(Json(state.auth.put_onboarding(&session, body).await?))
}

pub async fn platforms() -> impl IntoResponse {
    Json(platforms_catalog())
}

pub async fn workos_webhook(
    State(state): State<AppState>,
    parts: Parts,
    body: Bytes,
) -> Result<impl IntoResponse, AuthProblem> {
    let signature = parts
        .headers
        .get("workos-signature")
        .or_else(|| parts.headers.get("WorkOS-Signature"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthProblem::forbidden("missing WorkOS-Signature"))?;
    state.auth.handle_webhook(signature, &body).await?;
    Ok(Json(WebhookOk { ok: true }))
}

pub async fn delete_me(
    State(state): State<AppState>,
    parts: Parts,
    AuthSession(session): AuthSession,
) -> Result<Response, AuthProblem> {
    enforce_web_mutation_guards(&parts, &state, &session).await?;
    let body = state.auth.schedule_deletion(&session).await?;
    let mut res = (StatusCode::ACCEPTED, Json(body)).into_response();
    clear_session_cookie(&mut res, state.auth.config.secure_cookies);
    clear_auth_init_cookie(&mut res, state.auth.config.secure_cookies);
    Ok(res)
}
