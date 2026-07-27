//! Plan 003 auth surface.
//!
//! Local smoke / Plan 008 session harness:
//! - Preferred automated path: `cargo test --locked --manifest-path backend/Cargo.toml auth::local_session_fixture -- --ignored --nocapture`
//!   (requires `DATABASE_URL`/`DATABASE_URL_APP` plus `WORKOS_*` / `APP_SESSION_KEYS` / `AUTH_*`).
//! - Manual path: `GET /v1/auth/authorize` → frontend WorkOS callback → `POST /v1/auth/exchange`,
//!   then export `GP_TEST_SESSION` from the native bearer token or web `gp_session` cookie.
//! - `scripts/local-upload-smoke.sh` should fail closed when auth env / session is missing.

pub mod catalog;
pub mod config;
pub mod cookies;
pub mod crypto;
pub mod extract;
pub mod keys;
#[cfg(test)]
pub mod local_fixture;
pub mod problem;
pub mod provider;
pub mod purge;
pub mod reseal;
pub mod routes;
pub mod service;
pub mod store;
pub mod types;

pub use catalog::platforms_catalog;
pub use config::AuthConfig;
pub use provider::{
    AuthorizeUrlRequest, ProviderAuthSession, ProviderError, ProviderUser, VerifiedWebhookEvent,
    WorkosIdentityProvider,
};
pub use provider::sdk::SdkWorkosProvider;
pub use service::AuthService;
pub use types::{AppSession, AuthClient, TenantContext};
