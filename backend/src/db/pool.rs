use crate::config::Config;
use crate::error::{AppError, AppResult};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Lazy pool so API can bind and serve `/health/live` even when Postgres is down;
/// `/health/ready` then returns 503 Problem+JSON.
pub async fn connect_app(config: &Config) -> AppResult<PgPool> {
    PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy(&config.database_url_app)
        .map_err(AppError::from)
}
