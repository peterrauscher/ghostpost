//! Ghostpost backend kernel library (integration-test surface).

pub mod api;
pub mod auth;
pub mod blob;
pub mod cli;
pub mod config;
pub mod db;
pub mod deletion;
pub mod domain;
pub mod error;
pub mod r#import;
pub mod jobs;
pub mod repository;
pub mod scan;
pub mod shutdown;
pub mod telemetry;

pub use auth::{
    AuthClient, AuthConfig, AuthService, SdkWorkosProvider, TenantContext, WorkosIdentityProvider,
};
pub use config::Config;
pub use error::{AppError, AppResult};
