//! WorkOS identity provider boundary. WorkOS SDK types stay inside the adapter.

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct AuthorizeUrlRequest {
    pub redirect_uri: String,
    pub state: String,
    pub code_challenge: String,
}

#[derive(Debug, Clone)]
pub struct ProviderUser {
    pub workos_user_id: String,
    pub email: String,
    pub display_name: String,
    pub first_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderAuthSession {
    pub user: ProviderUser,
    pub access_token: String,
    pub refresh_token: String,
    pub workos_session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VerifiedWebhookEvent {
    pub event_id: String,
    pub event_type: String,
    pub data: Value,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider configuration error: {0}")]
    Config(String),
    #[error("provider request failed: {0}")]
    Request(String),
    /// Provider rejected the credential itself (wrong/expired one-time code).
    /// Never echo the inner message to clients; map to AUTH_CODE_INVALID.
    #[error("provider rejected credentials")]
    Auth,
    /// Provider rate limit hit while issuing one-time codes.
    #[error("provider rate limited")]
    RateLimited,
    #[error("provider not found")]
    NotFound,
    #[error("webhook verification failed: {0}")]
    Webhook(String),
}

#[async_trait]
pub trait WorkosIdentityProvider: Send + Sync {
    async fn authorization_url(&self, req: AuthorizeUrlRequest) -> Result<String, ProviderError>;

    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
    ) -> Result<ProviderAuthSession, ProviderError>;

    /// Headless Magic Auth: issue (and email) a 6-digit one-time code.
    /// The provider email-delivers the code; callers must never handle it.
    async fn send_magic_code(&self, email: &str) -> Result<(), ProviderError>;

    /// Verify a Magic Auth code. Signs in an existing user or signs up a new
    /// one, mirroring the hosted AuthKit email-code path.
    async fn authenticate_with_magic_code(
        &self,
        email: &str,
        code: &str,
    ) -> Result<ProviderAuthSession, ProviderError>;

    async fn refresh_session(
        &self,
        refresh_token: &str,
    ) -> Result<ProviderAuthSession, ProviderError>;

    async fn revoke_session(&self, workos_session_id: &str) -> Result<(), ProviderError>;

    /// Provider 404/not_found must map to `Ok(())`.
    async fn delete_user(&self, workos_user_id: &str) -> Result<(), ProviderError>;

    fn verify_webhook(
        &self,
        signature_header: &str,
        body: &[u8],
    ) -> Result<VerifiedWebhookEvent, ProviderError>;
}

pub mod sdk {
    use super::*;
    use base64::Engine;
    use workos::helpers::{
        AuthKitAuthorizationUrlParams, AuthKitPkceCodeExchangeParams, WebhookVerifier,
    };
    use workos::user_management::{
        AuthenticateWithCodeParams, AuthenticateWithRefreshTokenParams, RevokeSessionParams,
    };
    use workos::{Client, RevokeSession, SecretString};

    pub struct SdkWorkosProvider {
        client: Client,
        webhook: WebhookVerifier,
    }

    impl SdkWorkosProvider {
        pub fn new(api_key: &str, client_id: &str, webhook_secret: &str) -> Result<Self, ProviderError> {
            if api_key.trim().is_empty() || client_id.trim().is_empty() {
                return Err(ProviderError::Config(
                    "WORKOS_API_KEY and WORKOS_CLIENT_ID are required".into(),
                ));
            }
            let client = Client::builder()
                .api_key(api_key)
                .client_id(client_id)
                .build();
            Ok(Self {
                client,
                webhook: WebhookVerifier::new(webhook_secret),
            })
        }

        pub fn for_deletion(api_key: &str) -> Result<Self, ProviderError> {
            if api_key.trim().is_empty() {
                return Err(ProviderError::Config("WORKOS_API_KEY is required".into()));
            }
            Ok(Self {
                client: Client::builder().api_key(api_key).client_id("restore-replay").build(),
                webhook: WebhookVerifier::new("restore-replay-unused"),
            })
        }

        fn map_auth_response(
            resp: workos::AuthenticateResponse,
        ) -> Result<ProviderAuthSession, ProviderError> {
            let access = resp.access_token.expose().to_string();
            let refresh = resp.refresh_token.expose().to_string();
            let sid = extract_sid_from_jwt(&access);
            let display_name = display_name_from_user(&resp.user);
            Ok(ProviderAuthSession {
                user: ProviderUser {
                    workos_user_id: resp.user.id,
                    email: resp.user.email,
                    display_name,
                    first_name: resp.user.first_name,
                    avatar_url: resp.user.profile_picture_url,
                },
                access_token: access,
                refresh_token: refresh,
                workos_session_id: sid,
            })
        }
    }

    /// Classify a WorkOS API failure. Credential rejections (wrong/expired one-time
    /// code, unknown user for the grant) are surfaced as [`ProviderError::Auth`]
    /// without the provider message; everything else stays a generic request error.
    fn map_workos_error(err: workos::Error) -> ProviderError {
        match &err {
            workos::Error::Api(api) if api.status == 429 => ProviderError::RateLimited,
            workos::Error::Api(api) if (400..500).contains(&api.status) => {
                // 400/401/403/404 on authenticate grants: wrong or expired code,
                // disabled user, unknown email. All client-retryable rejections.
                ProviderError::Auth
            }
            _ => ProviderError::Request(err.to_string()),
        }
    }

    fn display_name_from_user(user: &workos::User) -> String {
        if let Some(name) = user.name.as_ref().filter(|s| !s.trim().is_empty()) {
            return name.clone();
        }
        let first = user.first_name.clone().unwrap_or_default();
        let last = user.last_name.clone().unwrap_or_default();
        let joined = format!("{first} {last}").trim().to_string();
        if !joined.is_empty() {
            joined
        } else {
            user.email.clone()
        }
    }

    fn extract_sid_from_jwt(token: &str) -> Option<String> {
        let payload = token.split('.').nth(1)?;
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
            .ok()?;
        let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
        value
            .get("sid")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    #[async_trait]
    impl WorkosIdentityProvider for SdkWorkosProvider {
        async fn authorization_url(
            &self,
            req: AuthorizeUrlRequest,
        ) -> Result<String, ProviderError> {
            self.client
                .authkit()
                .authorization_url(AuthKitAuthorizationUrlParams {
                    redirect_uri: req.redirect_uri,
                    state: Some(req.state),
                    code_challenge: Some(req.code_challenge),
                    code_challenge_method: Some("S256".into()),
                    provider: Some("authkit".into()),
                    ..Default::default()
                })
                .map_err(|err| ProviderError::Request(err.to_string()))
        }

        async fn exchange_code(
            &self,
            code: &str,
            code_verifier: &str,
        ) -> Result<ProviderAuthSession, ProviderError> {
            // Prefer AuthKit PKCE helper; fall back to typed user_management API.
            let via_helper = self
                .client
                .authkit()
                .pkce_code_exchange(AuthKitPkceCodeExchangeParams {
                    code: code.to_string(),
                    code_verifier: code_verifier.to_string(),
                })
                .await;
            let resp = match via_helper {
                Ok(r) => r,
                Err(_) => {
                    let mut params = AuthenticateWithCodeParams::new(code.to_string());
                    params.code_verifier = Some(code_verifier.to_string());
                    self.client
                        .user_management()
                        .authenticate_with_code(params)
                        .await
                        .map_err(map_workos_error)?
                }
            };
            Self::map_auth_response(resp)
        }

        async fn send_magic_code(&self, email: &str) -> Result<(), ProviderError> {
            // POST /user_management/magic_auth: creates the 6-digit code and
            // emails it to the user. The response body contains the raw code;
            // drop it so it can never leak into logs or responses.
            let params = workos::user_management::CreateMagicAuthParams::new(
                workos::CreateMagicCodeAndReturn {
                    email: email.to_string(),
                    invitation_token: None,
                    ip_address: None,
                    user_agent: None,
                    radar_auth_attempt_id: None,
                    signals_id: None,
                },
            );
            self.client
                .user_management()
                .create_magic_auth(params)
                .await
                .map(|_| ())
                .map_err(map_workos_error)
        }

        async fn authenticate_with_magic_code(
            &self,
            email: &str,
            code: &str,
        ) -> Result<ProviderAuthSession, ProviderError> {
            let resp = self
                .client
                .user_management()
                .authenticate_with_magic_auth(
                    workos::user_management::AuthenticateWithMagicAuthParams {
                        code: code.to_string(),
                        email: email.to_string(),
                        invitation_token: None,
                        ip_address: None,
                        device_id: None,
                        user_agent: None,
                        radar_auth_attempt_id: None,
                    },
                )
                .await
                .map_err(map_workos_error)?;
            Self::map_auth_response(resp)
        }

        async fn refresh_session(
            &self,
            refresh_token: &str,
        ) -> Result<ProviderAuthSession, ProviderError> {
            let params =
                AuthenticateWithRefreshTokenParams::new(SecretString::from(refresh_token.to_string()));
            let resp = self
                .client
                .user_management()
                .authenticate_with_refresh_token(params)
                .await
                .map_err(|err| ProviderError::Request(err.to_string()))?;
            Self::map_auth_response(resp)
        }

        async fn revoke_session(&self, workos_session_id: &str) -> Result<(), ProviderError> {
            let params = RevokeSessionParams::new(RevokeSession {
                session_id: workos_session_id.to_string(),
            });
            self.client
                .user_management()
                .revoke_session(params)
                .await
                .map_err(|err| ProviderError::Request(err.to_string()))
        }

        async fn delete_user(&self, workos_user_id: &str) -> Result<(), ProviderError> {
            match self
                .client
                .user_management()
                .delete_user(workos_user_id)
                .await
            {
                Ok(()) => Ok(()),
                Err(err) if err.is_not_found() => Ok(()),
                Err(err) => Err(ProviderError::Request(err.to_string())),
            }
        }

        fn verify_webhook(
            &self,
            signature_header: &str,
            body: &[u8],
        ) -> Result<VerifiedWebhookEvent, ProviderError> {
            let body_str = std::str::from_utf8(body)
                .map_err(|err| ProviderError::Webhook(err.to_string()))?;
            let event = self
                .webhook
                .construct_event(signature_header, body_str)
                .map_err(|err| ProviderError::Webhook(err.to_string()))?;
            Ok(VerifiedWebhookEvent {
                event_id: event.id,
                event_type: event.event,
                data: serde_json::to_value(event.data)
                    .map_err(|err| ProviderError::Webhook(err.to_string()))?,
            })
        }
    }
}
