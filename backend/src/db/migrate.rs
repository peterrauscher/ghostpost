use crate::config::{quote_ident, Config};
use crate::db::ddl;
use crate::error::{AppError, AppResult};
use sqlx::postgres::PgAdvisoryLock;
use sqlx::Executor;
use tracing::info;

const MIGRATION_LOCK_KEY: &str = "ghostpost_migrations_v1";

pub async fn run(config: &Config, dry_run: bool) -> AppResult<()> {
    let pool = ddl::connect_migrator(config).await?;
    let lock_conn = pool.acquire().await.map_err(AppError::from)?;

    let lock = PgAdvisoryLock::new(MIGRATION_LOCK_KEY);
    let mut guard =
        tokio::time::timeout(config.migration_lock_timeout, lock.acquire(lock_conn))
            .await
            .map_err(|_| {
                AppError::Migration(format!(
                    "timed out waiting for advisory lock {MIGRATION_LOCK_KEY} after {}s",
                    config.migration_lock_timeout.as_secs()
                ))
            })?
            .map_err(|err| AppError::Migration(err.to_string()))?;

    let app_role = quote_ident(&config.database_app_role)?;

    // Ensure app role can use schema for DML (no CREATE). Schema USAGE may
    // already be granted by the operator/init script; ignore privilege notices.
    let pre_sql = format!("GRANT USAGE ON SCHEMA public TO {app_role};");
    if let Err(err) = (&mut *guard).execute(pre_sql.as_str()).await {
        tracing::warn!(error = %err, "GRANT USAGE ON SCHEMA public skipped/failed");
    }

    let migrator = sqlx::migrate!("./migrations");
    if dry_run {
        info!(pending = migrator.iter().count(), "dry-run: migrations embedded");
        let _conn = guard.release_now().await.map_err(AppError::from)?;
        pool.close().await;
        return Ok(());
    }

    // Second pool connection runs the migrator while the advisory lock is held.
    migrator
        .run(&pool)
        .await
        .map_err(|err| AppError::Migration(err.to_string()))?;

    let grant_sql = format!(
        r#"
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {app_role};
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {app_role};
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO {app_role};
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT USAGE, SELECT ON SEQUENCES TO {app_role};
REVOKE ALL ON TABLE ghostpost_restore_guard FROM {app_role};
"#
    );
    (&mut *guard)
        .execute(grant_sql.as_str())
        .await
        .map_err(|err| AppError::Migration(err.to_string()))?;

    info!(app_role = %config.database_app_role, "migrations applied; DML grants refreshed");
    let _conn = guard.release_now().await.map_err(AppError::from)?;
    pool.close().await;
    Ok(())
}
