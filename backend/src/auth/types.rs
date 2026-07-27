use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthClient {
    Web,
    Native,
}

impl AuthClient {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Native => "native",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "web" => Some(Self::Web),
            "native" => Some(Self::Native),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TenantContext {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct AppSession {
    pub tenant_id: Uuid,
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub client: AuthClient,
    pub key_id: String,
    pub workos_session_row_id: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub greeting_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub kind: &'static str,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizeResponse {
    pub authorization_url: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exchange_secret: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRequest {
    pub client: AuthClient,
    pub code: String,
    pub state: String,
    #[serde(default)]
    pub exchange_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeExchangeResponse {
    pub user: UserDto,
    pub session: SessionDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeRefreshResponse {
    pub session: SessionDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsrfResponse {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResponse {
    pub user: UserDto,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisclosureConsent {
    pub version: String,
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnboardingAnswersDto {
    pub coming_up: Vec<String>,
    pub concerns: Vec<String>,
    pub platforms: Vec<String>,
    pub disclosure_consent: DisclosureConsent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnboardingState {
    pub status: String,
    pub current_step: i16,
    pub revision: i64,
    pub answers: OnboardingAnswersDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionScheduled {
    pub status: &'static str,
    pub purge_deadline: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WebhookOk {
    pub ok: bool,
}
