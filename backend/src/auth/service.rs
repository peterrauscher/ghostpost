//! Auth domain services: authorize, exchange, refresh, logout, profile, onboarding, purge.

use crate::auth::catalog::{
    is_archive_enabled_platform, is_known_coming_up, is_known_concern, is_known_platform,
    CURRENT_DISCLOSURE_VERSION,
};
use crate::auth::config::AuthConfig;
use crate::auth::crypto::{
    constant_time_eq, generate_pkce_pair, hash_token, open_online_sealed, random_token_b64,
    seal_bytes,
};
use crate::auth::problem::AuthProblem;
use crate::auth::provider::{
    AuthorizeUrlRequest, ProviderAuthSession, WorkosIdentityProvider,
};
use crate::auth::store::{self, AuthSessionRow, OnboardingRow, UserRow};
use crate::auth::types::{
    AppSession, AuthClient, AuthorizeResponse, CsrfResponse, DeletionScheduled, DisclosureConsent,
    MeResponse, NativeExchangeResponse, NativeRefreshResponse, OnboardingAnswersDto,
    OnboardingState, SessionDto, UserDto,
};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedWorkosPayload {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub workos_session_id: Option<String>,
}

pub struct AuthService {
    pub pool: PgPool,
    pub config: AuthConfig,
    pub provider: Arc<dyn WorkosIdentityProvider>,
}

fn greeting_name(display_name: &str, email: &str) -> String {
    let token = display_name
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user"));
    token.to_ascii_lowercase()
}

fn user_dto(row: &UserRow) -> Result<UserDto, AuthProblem> {
    Ok(UserDto {
        id: row.id,
        email: row
            .email
            .clone()
            .ok_or_else(|| AuthProblem::unauthorized("profile incomplete"))?,
        name: row
            .display_name
            .clone()
            .ok_or_else(|| AuthProblem::unauthorized("profile incomplete"))?,
        greeting_name: row
            .greeting_name
            .clone()
            .ok_or_else(|| AuthProblem::unauthorized("profile incomplete"))?,
        avatar_url: row.avatar_url.clone(),
    })
}

fn onboarding_status(row: &OnboardingRow) -> String {
    if row.completed_at.is_some() {
        "completed".into()
    } else if row.coming_up.is_empty()
        && row.concerns.is_empty()
        && row.platforms.is_empty()
        && row.revision == 0
        && row.current_step <= 1
    {
        "not_started".into()
    } else {
        "in_progress".into()
    }
}

fn onboarding_state_from_row(row: &OnboardingRow) -> OnboardingState {
    let accepted = row
        .consent_version
        .as_deref()
        .map(|v| v == CURRENT_DISCLOSURE_VERSION && row.consent_accepted_at.is_some())
        .unwrap_or(false);
    OnboardingState {
        status: onboarding_status(row),
        current_step: row.current_step,
        revision: row.revision,
        answers: OnboardingAnswersDto {
            coming_up: row.coming_up.clone(),
            concerns: row.concerns.clone(),
            platforms: row.platforms.clone(),
            disclosure_consent: DisclosureConsent {
                version: row
                    .consent_version
                    .clone()
                    .unwrap_or_else(|| CURRENT_DISCLOSURE_VERSION.to_string()),
                accepted,
            },
        },
    }
}

fn empty_onboarding() -> OnboardingState {
    OnboardingState {
        status: "not_started".into(),
        current_step: 1,
        revision: 0,
        answers: OnboardingAnswersDto {
            coming_up: vec![],
            concerns: vec![],
            platforms: vec![],
            disclosure_consent: DisclosureConsent {
                version: CURRENT_DISCLOSURE_VERSION.into(),
                accepted: false,
            },
        },
    }
}

impl AuthService {
    pub fn new(
        pool: PgPool,
        config: AuthConfig,
        provider: Arc<dyn WorkosIdentityProvider>,
    ) -> Self {
        Self {
            pool,
            config,
            provider,
        }
    }

    pub async fn authorize(
        &self,
        client: AuthClient,
    ) -> Result<(AuthorizeResponse, Option<String>), AuthProblem> {
        let redirect_uri = match client {
            AuthClient::Web => self.config.auth_web_redirect_uri.clone(),
            AuthClient::Native => self.config.auth_native_redirect_uri.clone(),
        };
        let pkce = generate_pkce_pair();
        let state = random_token_b64(16);
        let exchange_secret = random_token_b64(32);
        let verifier_hash = hash_token(pkce.code_verifier.as_bytes());
        let verifier_enc = seal_bytes(&self.config.workos_seal_key, pkce.code_verifier.as_bytes())
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let exchange_hash = hash_token(exchange_secret.as_bytes());
        let expires_at = Utc::now()
            + ChronoDuration::seconds(self.config.auth_init_cookie_max_age_secs as i64);

        store::insert_auth_flow(
            &self.pool,
            &state,
            &verifier_hash,
            &verifier_enc,
            &exchange_hash,
            client,
            &redirect_uri,
            expires_at,
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let authorization_url = self
            .provider
            .authorization_url(AuthorizeUrlRequest {
                redirect_uri,
                state: state.clone(),
                code_challenge: pkce.code_challenge,
            })
            .await
            .map_err(AuthProblem::provider_error)?;

        let body = AuthorizeResponse {
            authorization_url,
            state,
            exchange_secret: match client {
                AuthClient::Native => Some(exchange_secret.clone()),
                AuthClient::Web => None,
            },
            expires_at,
        };
        let cookie_secret = match client {
            AuthClient::Web => Some(exchange_secret),
            AuthClient::Native => None,
        };
        Ok((body, cookie_secret))
    }

    pub async fn exchange(
        &self,
        req_client: AuthClient,
        code: &str,
        state: &str,
        exchange_proof: Option<&str>,
    ) -> Result<(Option<NativeExchangeResponse>, Option<(String, ChronoDuration)>), AuthProblem>
    {
        let flow = store::load_auth_flow_by_state(&self.pool, state)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| {
                AuthProblem::gone("AUTH_FLOW_GONE", "Authorization flow missing or consumed")
            })?;

        if flow.consumed_at.is_some() || flow.expires_at <= Utc::now() {
            return Err(AuthProblem::gone(
                "AUTH_FLOW_GONE",
                "Authorization flow expired or already used",
            ));
        }
        let flow_client = AuthClient::parse(&flow.client).ok_or_else(|| {
            AuthProblem::internal("stored auth flow client invalid")
        })?;
        if flow_client != req_client {
            return Err(AuthProblem::forbidden("client mismatch"));
        }

        let proof = exchange_proof.ok_or_else(|| {
            AuthProblem::forbidden("missing exchange proof")
        })?;
        let proof_hash = hash_token(proof.as_bytes());
        if !constant_time_eq(&proof_hash, &flow.exchange_secret_hash) {
            return Err(AuthProblem::forbidden("invalid exchange proof"));
        }

        let verifier = open_online_sealed(&self.config.workos_seal_key, &flow.code_verifier_enc)
            .map_err(|_| AuthProblem::internal("failed to open PKCE verifier"))?;
        let verifier = String::from_utf8(verifier)
            .map_err(|_| AuthProblem::internal("invalid PKCE verifier encoding"))?;
        if !constant_time_eq(&hash_token(verifier.as_bytes()), &flow.code_verifier_hash) {
            return Err(AuthProblem::internal("PKCE verifier integrity check failed"));
        }

        let provider_session = self
            .provider
            .exchange_code(code, &verifier)
            .await
            .map_err(AuthProblem::provider_error)?;

        self.complete_login(flow.id, flow_client, provider_session)
            .await
    }

    async fn complete_login(
        &self,
        flow_id: Uuid,
        client: AuthClient,
        provider_session: ProviderAuthSession,
    ) -> Result<(Option<NativeExchangeResponse>, Option<(String, ChronoDuration)>), AuthProblem>
    {
        let workos_user_id = provider_session.user.workos_user_id.clone();
        let email = provider_session.user.email.clone();
        let display_name = provider_session.user.display_name.clone();
        let greeting = greeting_name(&display_name, &email);
        let avatar = provider_session.user.avatar_url.clone();

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let existing = store::find_user_by_workos_id_tx(&mut tx, &workos_user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let user = if let Some(user) = existing {
            if user.deleted_at.is_some() {
                return Err(AuthProblem::gone(
                    "ACCOUNT_DELETION_PENDING",
                    "Account deletion is pending",
                ));
            }
            store::update_user_profile_tx(
                &mut tx,
                user.tenant_id,
                user.id,
                &email,
                &display_name,
                &greeting,
                avatar.as_deref(),
            )
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
        } else {
            let tenant_id = store::create_tenant_tx(&mut tx)
                .await
                .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
            match store::create_user_tx(
                &mut tx,
                tenant_id,
                &workos_user_id,
                &email,
                &display_name,
                &greeting,
                avatar.as_deref(),
            )
            .await
            {
                Ok(user) => {
                    crate::domain::entitlements::EntitlementService::seed_free_beta_tx(
                        &mut tx,
                        user.tenant_id,
                        user.id,
                    )
                    .await
                    .map_err(|err| AuthProblem::internal_sanitized(err, "free_beta grant failed"))?;
                    user
                }
                Err(err) if is_unique_violation(&err) => {
                    tx.rollback()
                        .await
                        .map_err(|e| AuthProblem::internal_sanitized(e, "database or crypto error"))?;
                    // Winner exists — reopen and load.
                    let mut tx2 = self
                        .pool
                        .begin()
                        .await
                        .map_err(|e| AuthProblem::internal_sanitized(e, "database or crypto error"))?;
                    let winner = store::find_user_by_workos_id_tx(&mut tx2, &workos_user_id)
                        .await
                        .map_err(|e| AuthProblem::internal_sanitized(e, "database or crypto error"))?
                        .ok_or_else(|| AuthProblem::internal("unique conflict without winner"))?;
                    if winner.deleted_at.is_some() {
                        return Err(AuthProblem::gone(
                            "ACCOUNT_DELETION_PENDING",
                            "Account deletion is pending",
                        ));
                    }
                    let user = store::update_user_profile_tx(
                        &mut tx2,
                        winner.tenant_id,
                        winner.id,
                        &email,
                        &display_name,
                        &greeting,
                        avatar.as_deref(),
                    )
                    .await
                    .map_err(|e| AuthProblem::internal_sanitized(e, "database or crypto error"))?;
                    return self
                        .mint_session_in_tx(tx2, flow_id, client, user, provider_session)
                        .await;
                }
                Err(err) => return Err(AuthProblem::internal_sanitized(err, "database or crypto error")),
            }
        };

        self.mint_session_in_tx(tx, flow_id, client, user, provider_session)
            .await
    }

    async fn mint_session_in_tx(
        &self,
        mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
        flow_id: Uuid,
        client: AuthClient,
        user: UserRow,
        provider_session: ProviderAuthSession,
    ) -> Result<(Option<NativeExchangeResponse>, Option<(String, ChronoDuration)>), AuthProblem>
    {
        let consumed = store::mark_flow_consumed_tx(&mut tx, flow_id, user.tenant_id, user.id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        if !consumed {
            return Err(AuthProblem::gone(
                "AUTH_FLOW_GONE",
                "Authorization flow expired or already used",
            ));
        }

        let sealed_payload = SealedWorkosPayload {
            access_token: provider_session.access_token,
            refresh_token: provider_session.refresh_token,
            workos_session_id: provider_session.workos_session_id.clone(),
        };
        let sealed = seal_bytes(
            &self.config.workos_seal_key,
            &serde_json::to_vec(&sealed_payload)
                .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?,
        )
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let session_expires = Utc::now()
            + ChronoDuration::seconds(self.config.app_session_max_age_secs as i64);

        let workos_row = store::insert_workos_session_tx(
            &mut tx,
            user.tenant_id,
            user.id,
            &sealed,
            &self.config.workos_seal_key_version,
            provider_session.workos_session_id.as_deref(),
            session_expires,
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let raw_token = random_token_b64(32);
        let token_hash = hash_token(raw_token.as_bytes());
        let key = self.config.session_keys.current();
        let csrf_raw = match client {
            AuthClient::Web => Some(random_token_b64(16)),
            AuthClient::Native => None,
        };
        let csrf_hash = csrf_raw.as_ref().map(|t| hash_token(t.as_bytes()));

        store::insert_app_session_tx(
            &mut tx,
            user.tenant_id,
            user.id,
            &token_hash,
            &key.id,
            client,
            csrf_hash.as_ref().map(|h| h.as_slice()),
            workos_row.id,
            session_expires,
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let max_age = ChronoDuration::seconds(self.config.app_session_max_age_secs as i64);
        match client {
            AuthClient::Web => Ok((None, Some((raw_token, max_age)))),
            AuthClient::Native => Ok((
                Some(NativeExchangeResponse {
                    user: user_dto(&user)?,
                    session: SessionDto {
                        kind: "bearer",
                        token: raw_token,
                        expires_at: session_expires,
                    },
                }),
                None,
            )),
        }
    }

    pub async fn resolve_session_token(
        &self,
        raw_token: &str,
    ) -> Result<AppSession, AuthProblem> {
        let token_hash = hash_token(raw_token.as_bytes());
        let row = store::find_active_session_by_hash(&self.pool, &token_hash)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| AuthProblem::unauthorized("invalid session"))?;
        let key_id = row
            .key_id
            .as_deref()
            .ok_or_else(|| AuthProblem::unauthorized("session key missing"))?;
        if !self.config.session_keys.contains_id(key_id) {
            return Err(AuthProblem::unauthorized("session key retired"));
        }
        let user = store::get_user(&self.pool, row.tenant_id, row.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| AuthProblem::unauthorized("user missing"))?;
        if user.deleted_at.is_some() {
            return Err(AuthProblem::unauthorized("account deleted"));
        }
        let client = AuthClient::parse(row.client.as_deref().unwrap_or(""))
            .ok_or_else(|| AuthProblem::unauthorized("session client missing"))?;
        Ok(AppSession {
            tenant_id: row.tenant_id,
            user_id: row.user_id,
            session_id: row.id,
            client,
            key_id: key_id.to_string(),
            workos_session_row_id: row.workos_session_id,
            expires_at: row.expires_at,
        })
    }

    pub async fn issue_csrf(&self, session: &AppSession) -> Result<CsrfResponse, AuthProblem> {
        if session.client != AuthClient::Web {
            return Err(AuthProblem::forbidden("CSRF is web-only"));
        }
        let token = random_token_b64(16);
        let hash = hash_token(token.as_bytes());
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        store::rotate_csrf_tx(&mut tx, session.tenant_id, session.session_id, &hash)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        Ok(CsrfResponse {
            token,
            expires_at: Utc::now()
                + ChronoDuration::seconds(self.config.csrf_token_max_age_secs as i64),
        })
    }

    pub async fn validate_csrf(
        &self,
        session: &AppSession,
        token: &str,
    ) -> Result<(), AuthProblem> {
        if session.client != AuthClient::Web {
            return Ok(());
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let row = store::lock_session_tx(&mut tx, session.tenant_id, session.session_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| AuthProblem::unauthorized("invalid session"))?;
        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let Some(csrf_hash) = row.csrf_hash.as_ref() else {
            return Err(AuthProblem::forbidden("missing CSRF state"));
        };
        if let Some(rotated_at) = row.csrf_rotated_at {
            let age = Utc::now() - rotated_at;
            if age > ChronoDuration::seconds(self.config.csrf_token_max_age_secs as i64) {
                return Err(AuthProblem::forbidden("CSRF token expired"));
            }
        } else {
            return Err(AuthProblem::forbidden("missing CSRF rotation timestamp"));
        }
        if !constant_time_eq(&hash_token(token.as_bytes()), csrf_hash) {
            return Err(AuthProblem::forbidden("invalid CSRF token"));
        }
        Ok(())
    }

    pub async fn refresh(
        &self,
        session: &AppSession,
    ) -> Result<(Option<NativeRefreshResponse>, Option<(String, ChronoDuration)>), AuthProblem>
    {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let row = store::lock_session_tx(&mut tx, session.tenant_id, session.session_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| AuthProblem::unauthorized("invalid session"))?;
        if row.revoked_at.is_some() || row.expires_at <= Utc::now() {
            return Err(AuthProblem::unauthorized("session expired"));
        }
        let workos_id = row
            .workos_session_id
            .ok_or_else(|| AuthProblem::unauthorized("missing workos session"))?;
        let workos_row = store::get_workos_session_tx(&mut tx, session.tenant_id, workos_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| AuthProblem::unauthorized("missing workos session"))?;
        if workos_row.revoked_at.is_some() {
            return Err(AuthProblem::unauthorized("workos session revoked"));
        }
        let opened = open_online_sealed(&self.config.workos_seal_key, &workos_row.sealed_session)
            .map_err(|_| AuthProblem::internal("failed to open sealed session"))?;
        let sealed: SealedWorkosPayload = serde_json::from_slice(&opened)
            .map_err(|err| AuthProblem::internal_sanitized(err, "sealed session decode failed"))?;

        // Commit lock before external call? Keep lock through refresh for single-flight.
        let refreshed = self
            .provider
            .refresh_session(&sealed.refresh_token)
            .await
            .map_err(AuthProblem::provider_unauthorized)?;

        let new_payload = SealedWorkosPayload {
            access_token: refreshed.access_token,
            refresh_token: refreshed.refresh_token,
            workos_session_id: refreshed
                .workos_session_id
                .or(sealed.workos_session_id),
        };
        let sealed_bytes = seal_bytes(
            &self.config.workos_seal_key,
            &serde_json::to_vec(&new_payload)
                .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?,
        )
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let session_expires = Utc::now()
            + ChronoDuration::seconds(self.config.app_session_max_age_secs as i64);
        store::update_workos_session_sealed_tx(
            &mut tx,
            session.tenant_id,
            workos_id,
            &sealed_bytes,
            &self.config.workos_seal_key_version,
            new_payload.workos_session_id.as_deref(),
            session_expires,
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        store::revoke_session_tx(&mut tx, session.tenant_id, session.session_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let raw_token = random_token_b64(32);
        let token_hash = hash_token(raw_token.as_bytes());
        let key = self.config.session_keys.current();
        let csrf_raw = match session.client {
            AuthClient::Web => Some(random_token_b64(16)),
            AuthClient::Native => None,
        };
        let csrf_hash = csrf_raw.as_ref().map(|t| hash_token(t.as_bytes()));
        store::insert_app_session_tx(
            &mut tx,
            session.tenant_id,
            session.user_id,
            &token_hash,
            &key.id,
            session.client,
            csrf_hash.as_ref().map(|h| h.as_slice()),
            workos_id,
            session_expires,
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let max_age = ChronoDuration::seconds(self.config.app_session_max_age_secs as i64);
        match session.client {
            AuthClient::Web => Ok((None, Some((raw_token, max_age)))),
            AuthClient::Native => Ok((
                Some(NativeRefreshResponse {
                    session: SessionDto {
                        kind: "bearer",
                        token: raw_token,
                        expires_at: session_expires,
                    },
                }),
                None,
            )),
        }
    }

    pub async fn logout(&self, session: &AppSession) -> Result<(), AuthProblem> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let row = store::lock_session_tx(&mut tx, session.tenant_id, session.session_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        if let Some(row) = row {
            store::revoke_session_tx(&mut tx, row.tenant_id, row.id)
                .await
                .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
            if let Some(ws_id) = row.workos_session_id {
                if let Some(ws) = store::get_workos_session_tx(&mut tx, row.tenant_id, ws_id)
                    .await
                    .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
                {
                    sqlx::query(
                        r#"UPDATE workos_sessions SET revoked_at = COALESCE(revoked_at, now())
                           WHERE tenant_id = $1 AND id = $2"#,
                    )
                    .bind(row.tenant_id)
                    .bind(ws.id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
                    if let Ok(opened) =
                        open_online_sealed(&self.config.workos_seal_key, &ws.sealed_session)
                    {
                        if let Ok(payload) =
                            serde_json::from_slice::<SealedWorkosPayload>(&opened)
                        {
                            if let Some(sid) = payload.workos_session_id.or(ws.workos_session_id) {
                                let _ = self.provider.revoke_session(&sid).await;
                            }
                        }
                    }
                }
            }
        }
        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        Ok(())
    }

    pub async fn me(&self, session: &AppSession) -> Result<MeResponse, AuthProblem> {
        let user = store::get_user(&self.pool, session.tenant_id, session.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .ok_or_else(|| AuthProblem::unauthorized("user missing"))?;
        if user.deleted_at.is_some() {
            return Err(AuthProblem::unauthorized("account deleted"));
        }
        Ok(MeResponse {
            user: user_dto(&user)?,
        })
    }

    pub async fn get_onboarding(
        &self,
        session: &AppSession,
    ) -> Result<OnboardingState, AuthProblem> {
        let row = store::get_onboarding(&self.pool, session.tenant_id, session.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        Ok(row.map(|r| onboarding_state_from_row(&r)).unwrap_or_else(empty_onboarding))
    }

    pub async fn put_onboarding(
        &self,
        session: &AppSession,
        body: OnboardingState,
    ) -> Result<OnboardingState, AuthProblem> {
        validate_onboarding_put(&body)?;
        let existing = store::get_onboarding(&self.pool, session.tenant_id, session.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let prev_step = existing.as_ref().map(|r| r.current_step).unwrap_or(1);
        if body.current_step < prev_step {
            return Err(AuthProblem::unprocessable(
                "ONBOARDING_STEP_REGRESSION",
                "currentStep cannot move backward",
            ));
        }

        let prev_accepted = existing
            .as_ref()
            .map(|r| {
                r.consent_version.as_deref() == Some(CURRENT_DISCLOSURE_VERSION)
                    && r.consent_accepted_at.is_some()
            })
            .unwrap_or(false);
        let mut consent_accepted_at = existing.and_then(|r| r.consent_accepted_at);
        let consent_version = if body.answers.disclosure_consent.accepted {
            if body.answers.disclosure_consent.version != CURRENT_DISCLOSURE_VERSION {
                return Err(AuthProblem::unprocessable(
                    "ONBOARDING_CONSENT_VERSION",
                    "disclosure consent version must be current",
                ));
            }
            if !prev_accepted {
                consent_accepted_at = Some(Utc::now());
            }
            Some(CURRENT_DISCLOSURE_VERSION)
        } else {
            // Changing version requires new acceptance; declining clears acceptance for new version.
            if body.answers.disclosure_consent.version != CURRENT_DISCLOSURE_VERSION {
                consent_accepted_at = None;
            }
            Some(body.answers.disclosure_consent.version.as_str())
        };

        let completed_at = if body.status == "completed" {
            Some(Utc::now())
        } else {
            None
        };

        let updated = store::upsert_onboarding_revisioned(
            &self.pool,
            session.tenant_id,
            session.user_id,
            body.revision,
            &body.answers.coming_up,
            &body.answers.concerns,
            &body.answers.platforms,
            body.current_step,
            consent_version,
            consent_accepted_at,
            completed_at,
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;

        let Some(row) = updated else {
            return Err(AuthProblem::conflict(
                "ONBOARDING_REVISION_CONFLICT",
                "Onboarding revision conflict",
                "Refresh onboarding state and retry.",
            )
            .with_instance("/v1/me/onboarding"));
        };
        Ok(onboarding_state_from_row(&row))
    }

    pub async fn schedule_deletion(
        &self,
        session: &AppSession,
    ) -> Result<DeletionScheduled, AuthProblem> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let deleted_at = store::tombstone_user_tx(&mut tx, session.tenant_id, session.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?
            .unwrap_or_else(Utc::now);
        store::revoke_all_user_sessions_tx(&mut tx, session.tenant_id, session.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        store::cancel_non_purge_work_tx(&mut tx, session.tenant_id, session.user_id)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        let payload = json!({
            "user_id": session.user_id,
            "deleted_at": deleted_at,
            "local_purged_at": null,
            "provider_purged_at": null,
        });
        store::enqueue_account_purge_tx(&mut tx, session.tenant_id, session.user_id, payload)
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "database or crypto error"))?;
        Ok(DeletionScheduled {
            status: "deletion_scheduled",
            purge_deadline: deleted_at + ChronoDuration::hours(24),
        })
    }

    pub async fn handle_webhook(
        &self,
        signature: &str,
        body: &[u8],
    ) -> Result<(), AuthProblem> {
        let event = self
            .provider
            .verify_webhook(signature, body)
            .map_err(AuthProblem::webhook_verification_failed)?;
        let mut hasher = Sha256::new();
        hasher.update(body);
        let payload_hash = hasher.finalize();

        // Idempotency claim + handler side effects share one transaction.
        // Handler failure rolls back the claim so WorkOS can retry the same event id.
        // Only a fully processed (committed) event short-circuits duplicate delivery.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "webhook begin failed"))?;
        let inserted = store::insert_webhook_event_tx(
            &mut tx,
            &event.event_id,
            &event.event_type,
            payload_hash.as_slice(),
        )
        .await
        .map_err(|err| AuthProblem::internal_sanitized(err, "webhook idempotency insert failed"))?;
        if !inserted {
            // Already committed previously — duplicate delivery no-op.
            return Ok(());
        }

        match event.event_type.as_str() {
            "user.updated" => {
                let id = event
                    .data
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AuthProblem::bad_request("WEBHOOK_PAYLOAD", "missing user id"))?;
                let display_name = event
                    .data
                    .get("first_name")
                    .and_then(|v| v.as_str())
                    .map(|first| {
                        let last = event
                            .data
                            .get("last_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let joined = format!("{first} {last}").trim().to_string();
                        if joined.is_empty() {
                            event
                                .data
                                .get("email")
                                .and_then(|v| v.as_str())
                                .unwrap_or(first)
                                .to_string()
                        } else {
                            joined
                        }
                    });
                let avatar = event
                    .data
                    .get("profile_picture_url")
                    .and_then(|v| v.as_str());
                let _ = store::sync_user_from_webhook_tx(
                    &mut tx,
                    id,
                    display_name.as_deref(),
                    avatar,
                )
                .await
                .map_err(|err| AuthProblem::internal_sanitized(err, "webhook user.updated failed"))?;
            }
            "user.deleted" => {
                let id = event
                    .data
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AuthProblem::bad_request("WEBHOOK_PAYLOAD", "missing user id"))?;
                if let Some(user) = store::find_user_by_workos_id_tx(&mut tx, id)
                    .await
                    .map_err(|err| AuthProblem::internal_sanitized(err, "webhook user.deleted lookup failed"))?
                {
                    let deleted_at =
                        store::tombstone_user_tx(&mut tx, user.tenant_id, user.id)
                            .await
                            .map_err(|err| AuthProblem::internal_sanitized(err, "webhook tombstone failed"))?
                            .unwrap_or_else(Utc::now);
                    store::revoke_all_user_sessions_tx(&mut tx, user.tenant_id, user.id)
                        .await
                        .map_err(|err| AuthProblem::internal_sanitized(err, "webhook revoke sessions failed"))?;
                    store::cancel_non_purge_work_tx(&mut tx, user.tenant_id, user.id)
                        .await
                        .map_err(|err| AuthProblem::internal_sanitized(err, "webhook cancel work failed"))?;
                    let payload = json!({
                        "user_id": user.id,
                        "deleted_at": deleted_at,
                        "local_purged_at": null,
                        "provider_purged_at": null,
                    });
                    store::enqueue_account_purge_tx(&mut tx, user.tenant_id, user.id, payload)
                        .await
                        .map_err(|err| AuthProblem::internal_sanitized(err, "webhook enqueue purge failed"))?;
                }
            }
            "session.revoked" => {
                let sid = event
                    .data
                    .get("id")
                    .or_else(|| event.data.get("session_id"))
                    .and_then(|v| v.as_str());
                if let Some(sid) = sid {
                    if let Some(row) =
                        store::find_workos_session_by_provider_id_tx(&mut tx, sid)
                            .await
                            .map_err(|err| AuthProblem::internal_sanitized(err, "webhook session lookup failed"))?
                    {
                        store::revoke_workos_session_row_tx(&mut tx, row.tenant_id, row.id)
                            .await
                            .map_err(|err| AuthProblem::internal_sanitized(err, "webhook session revoke failed"))?;
                    }
                }
            }
            _ => {}
        }

        tx.commit()
            .await
            .map_err(|err| AuthProblem::internal_sanitized(err, "webhook commit failed"))?;
        Ok(())
    }
}

fn validate_onboarding_put(body: &OnboardingState) -> Result<(), AuthProblem> {
    fn uniq_known(ids: &[String], known: fn(&str) -> bool, label: &str) -> Result<(), AuthProblem> {
        let mut seen = std::collections::HashSet::new();
        for id in ids {
            if !known(id) {
                return Err(AuthProblem::unprocessable(
                    "ONBOARDING_INVALID_ENUM",
                    format!("unknown {label} id: {id}"),
                ));
            }
            if !seen.insert(id.clone()) {
                return Err(AuthProblem::unprocessable(
                    "ONBOARDING_DUPLICATE_ENUM",
                    format!("duplicate {label} id: {id}"),
                ));
            }
        }
        Ok(())
    }
    uniq_known(&body.answers.coming_up, is_known_coming_up, "comingUp")?;
    uniq_known(&body.answers.concerns, is_known_concern, "concerns")?;
    let mut seen = std::collections::HashSet::new();
    for id in &body.answers.platforms {
        if !is_known_platform(id) {
            return Err(AuthProblem::unprocessable(
                "ONBOARDING_INVALID_ENUM",
                format!("unknown platform id: {id}"),
            ));
        }
        if !is_archive_enabled_platform(id) {
            return Err(AuthProblem::unprocessable(
                "ONBOARDING_PLATFORM_DISABLED",
                format!("platform not archive-enabled: {id}"),
            ));
        }
        if !seen.insert(id.clone()) {
            return Err(AuthProblem::unprocessable(
                "ONBOARDING_DUPLICATE_ENUM",
                format!("duplicate platform id: {id}"),
            ));
        }
    }
    if !(1..=4).contains(&body.current_step) {
        return Err(AuthProblem::unprocessable(
            "ONBOARDING_STEP",
            "currentStep must be 1..=4",
        ));
    }

    let empty = body.answers.coming_up.is_empty()
        && body.answers.concerns.is_empty()
        && body.answers.platforms.is_empty();
    match body.status.as_str() {
        "not_started" => {
            if !(empty && body.current_step == 1 && !body.answers.disclosure_consent.accepted) {
                return Err(AuthProblem::unprocessable(
                    "ONBOARDING_STATUS",
                    "not_started only valid for empty initial state",
                ));
            }
        }
        "in_progress" => {}
        "completed" => {
            if body.answers.coming_up.is_empty()
                || body.answers.concerns.is_empty()
                || body.answers.platforms.is_empty()
                || body.current_step != 4
                || !body.answers.disclosure_consent.accepted
                || body.answers.disclosure_consent.version != CURRENT_DISCLOSURE_VERSION
            {
                return Err(AuthProblem::unprocessable(
                    "ONBOARDING_INCOMPLETE",
                    "completed requires answers, step 4, and current disclosure consent",
                ));
            }
        }
        other => {
            return Err(AuthProblem::unprocessable(
                "ONBOARDING_STATUS",
                format!("invalid status: {other}"),
            ));
        }
    }
    Ok(())
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    match err {
        sqlx::Error::Database(db) => db.code().as_deref() == Some("23505"),
        _ => false,
    }
}

#[allow(dead_code)]
pub fn session_row_client(row: &AuthSessionRow) -> Option<AuthClient> {
    row.client.as_deref().and_then(AuthClient::parse)
}
