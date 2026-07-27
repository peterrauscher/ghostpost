use crate::config::Config;
use crate::error::{AppError, AppResult};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn connect_migrator(config: &Config) -> AppResult<PgPool> {
    PgPoolOptions::new()
        .max_connections(config.migration_max_connections)
        .connect(&config.database_url)
        .await
        .map_err(AppError::from)
}
