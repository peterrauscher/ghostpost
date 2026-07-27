use crate::api::{archive_imports, flags, health, profile, scans};
use crate::auth::routes as auth_routes;
use crate::auth::AuthService;
use crate::blob::{BlobStore, DeterministicBlobStore};
use axum::routing::{get, post};
use axum::Router;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use axum::http::{HeaderName, HeaderValue, Method};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub restore_replay_pending: bool,
    /// True when process also runs the worker (`--role all`).
    pub worker_ready: bool,
    pub auth: Arc<AuthService>,
}

pub fn build(state: AppState) -> Router {
    build_with_blob_store(state, Arc::new(DeterministicBlobStore))
}

pub fn build_with_blob_store(state: AppState, blob_store: Arc<dyn BlobStore>) -> Router {
    let origins = state
        .auth
        .config
        .cors_allowed_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect::<Vec<_>>();
    let cors = CorsLayer::new()
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("authorization"),
            HeaderName::from_static("x-csrf-token"),
            HeaderName::from_static("idempotency-key"),
        ])
        .allow_origin(AllowOrigin::list(origins));

    Router::new()
        .route("/health/live", get(health::live))
        .route("/health/ready", get(health::ready))
        .route("/v1/auth/authorize", get(auth_routes::authorize))
        .route("/v1/auth/exchange", post(auth_routes::exchange))
        .route("/v1/auth/refresh", post(auth_routes::refresh))
        .route("/v1/auth/logout", post(auth_routes::logout))
        .route("/v1/auth/csrf", get(auth_routes::csrf))
        .route("/v1/me", get(auth_routes::me).delete(auth_routes::delete_me))
        .route(
            "/v1/me/onboarding",
            get(auth_routes::get_onboarding).put(auth_routes::put_onboarding),
        )
        .route("/v1/platforms", get(auth_routes::platforms))
        .route("/v1/webhooks/workos", post(auth_routes::workos_webhook))
        .route("/v1/archive-imports", get(archive_imports::list).post(archive_imports::reserve))
        .route("/v1/archive-imports/{id}", get(archive_imports::get_one).delete(archive_imports::delete_one))
        .route("/v1/archive-imports/{id}/complete", post(archive_imports::complete))
        .route("/v1/scans", post(scans::create_scan))
        .route("/v1/scans/current", get(scans::get_current))
        .route("/v1/scans/{id}", get(scans::get_one))
        .route("/v1/dashboard", get(profile::dashboard))
        .route("/v1/me/entitlement", get(profile::entitlement))
        .route("/v1/flags", get(flags::list_flags))
        .route("/v1/flags/{id}", get(flags::get_flag))
        .route("/v1/flags/{id}/review-actions", post(flags::review_action))
        .layer(axum::Extension(blob_store))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
