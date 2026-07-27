use crate::blob::BlobStore;
use crate::auth::purge::{emit_purge_alarm, handle_account_purge};
use crate::auth::AuthService;
use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::jobs::{archive_import, cancel, queue, sweeper};
use crate::repository::work_items::WorkItem;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// In-flight leases owned by this worker instance (for heartbeat).
type OwnedLeases = Arc<Mutex<Vec<(uuid::Uuid, uuid::Uuid)>>>;

/// Aborts a spawned task if dropped before being disarmed (prevents detached aux tasks).
struct AbortOnDrop(Option<AbortHandle>);

impl AbortOnDrop {
    fn new(handle: AbortHandle) -> Self {
        Self(Some(handle))
    }

    fn disarm(mut self) {
        self.0.take();
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Test-only releaseable hang gate for `test.hang` work items.
#[cfg(test)]
pub mod test_hang {
    use std::sync::Arc;
    use tokio::sync::{Mutex, Notify};

    static GATE: Mutex<Option<Arc<Notify>>> = Mutex::const_new(None);

    pub async fn arm() -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        *GATE.lock().await = Some(notify.clone());
        notify
    }

    pub async fn wait() {
        let notify = GATE
            .lock()
            .await
            .clone()
            .expect("test.hang gate must be armed before claim");
        notify.notified().await;
    }

    pub fn release(notify: &Notify) {
        notify.notify_waiters();
    }

    pub async fn clear() {
        *GATE.lock().await = None;
    }
}

pub async fn run(pool: PgPool, config: Config, auth: Arc<AuthService>, blob: Arc<dyn BlobStore>, token: CancellationToken) -> AppResult<()> {
    let owned: OwnedLeases = Arc::new(Mutex::new(Vec::new()));
    let lease_owner = config.instance_id.clone();

    let heartbeat_pool = pool.clone();
    let heartbeat_token = token.clone();
    let heartbeat_owned = owned.clone();
    let heartbeat_owner = lease_owner.clone();
    let heartbeat_lease = config.worker_lease;
    let heartbeat_every = config.worker_heartbeat;
    let heartbeat_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = heartbeat_token.cancelled() => break,
                _ = tokio::time::sleep(heartbeat_every) => {
                    let leases = heartbeat_owned.lock().await.clone();
                    for (tenant_id, id) in leases {
                        match queue::heartbeat(
                            &heartbeat_pool,
                            tenant_id,
                            id,
                            &heartbeat_owner,
                            heartbeat_lease,
                        ).await {
                            Ok(true) => {}
                            Ok(false) => {
                                warn!(%tenant_id, %id, "lost lease on heartbeat");
                                let mut guard = heartbeat_owned.lock().await;
                                guard.retain(|(t, i)| !(*t == tenant_id && *i == id));
                            }
                            Err(err) => warn!(error = %err, "heartbeat error"),
                        }
                    }
                }
            }
        }
    });
    let heartbeat_guard = AbortOnDrop::new(heartbeat_task.abort_handle());

    let sweeper_pool = pool.clone();
    let sweeper_token = token.clone();
    let sweeper_blob = blob.clone();
    let sweeper_every = config.worker_sweeper;
    let sweeper_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = sweeper_token.cancelled() => break,
                _ = tokio::time::sleep(sweeper_every) => {
                    if let Err(err) = sweeper::sweep_once(&sweeper_pool).await {
                        warn!(error = %err, "sweeper error");
                    }
                    if let Err(err) = cancel::recover_once(&sweeper_pool).await {
                        warn!(error = %err, "cancel recovery error");
                    }
                    if let Err(err) = archive_import::sweep_due(&sweeper_pool, sweeper_blob.clone()).await {
                        warn!(error = %err, "archive raw sweeper error");
                    }
                }
            }
        }
    });
    let sweeper_guard = AbortOnDrop::new(sweeper_task.abort_handle());

    info!(instance_id = %lease_owner, "worker claim loop started");
    while !token.is_cancelled() {
        match queue::claim_one(&pool, &lease_owner, config.worker_lease).await {
            Ok(Some(item)) => {
                {
                    let mut guard = owned.lock().await;
                    guard.push((item.tenant_id, item.id));
                }
                // Process errors (lost lease, DB) must not terminate future claiming.
                if let Err(err) = process_item(&pool, &item, &lease_owner, auth.clone(), blob.clone()).await {
                    warn!(
                        kind = %item.kind,
                        id = %item.id,
                        error = %err,
                        "work item processing error; continuing claim loop"
                    );
                }
                // Every item exit removes owned lease state (success or error).
                {
                    let mut guard = owned.lock().await;
                    guard.retain(|(t, i)| !(*t == item.tenant_id && *i == item.id));
                }
            }
            Ok(None) => {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {}
                }
            }
            Err(err) => {
                warn!(error = %err, "claim error");
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                }
            }
        }
    }

    info!("claim loop stopped; waiting auxiliary tasks");
    // Ensure aux loops observe cancellation even if we exited for other reasons.
    token.cancel();
    let _ = heartbeat_task.await;
    heartbeat_guard.disarm();
    let _ = sweeper_task.await;
    sweeper_guard.disarm();
    Ok(())
}

async fn process_item(pool: &PgPool, item: &WorkItem, lease_owner: &str, auth: Arc<AuthService>, blob: Arc<dyn BlobStore>) -> AppResult<()> {
    // Re-read cancel flag after claim so mid-flight cancel_requested_at is honored
    // (claim SQL excludes already-cancelled pending rows; this covers the race).
    let cancel_requested = sqlx::query_scalar!(
        r#"
SELECT cancel_requested_at
FROM work_items
WHERE tenant_id = $1 AND id = $2 AND lease_owner = $3 AND status = 'running'
"#,
        item.tenant_id,
        item.id,
        lease_owner,
    )
    .fetch_optional(pool)
    .await
    .map_err(AppError::from)?
    .flatten();

    if cancel_requested.is_some() || item.cancel_requested_at.is_some() {
        let ok = queue::commit_cancelled(pool, item.tenant_id, item.id, lease_owner)
            .await
            .map_err(AppError::from)?;
        if !ok {
            warn!(
                kind = %item.kind,
                id = %item.id,
                "lost lease on cancel commit"
            );
        }
        return Ok(());
    }

    #[cfg(test)]
    if item.kind == "test.hang" {
        test_hang::wait().await;
    }

    if item.kind == "import.normalize" || item.kind == "import.purge_raw" {
        let result = if item.kind == "import.normalize" {
            archive_import::normalize(pool, blob.clone(), item, lease_owner).await
        } else {
            archive_import::purge_job(pool, blob.clone(), item).await
        };
        match result {
            Ok(()) => {
                if !queue::commit_success(pool, item.tenant_id, item.id, lease_owner).await? {
                    warn!(kind = %item.kind, id = %item.id, "lost lease on archive success commit");
                }
            }
            Err(err) => {
                let message = err.to_string();
                let rejected = item.kind == "import.normalize"
                    && archive_import::reject_if_deterministic(pool, blob.clone(), item, &message).await?;
                if rejected {
                    queue::commit_success(pool, item.tenant_id, item.id, lease_owner).await?;
                } else {
                    queue::commit_failure(pool, item.tenant_id, item.id, lease_owner, &message, item.attempt_count, item.max_attempts).await?;
                }
            }
        }
        return Ok(());
    }
    if item.kind == "scan_posts" {
        let provider = match crate::jobs::scan_posts::build_scan_provider() {
            Ok(p) => p,
            Err(err) => {
                let msg = err.to_string();
                queue::commit_failure(
                    pool,
                    item.tenant_id,
                    item.id,
                    lease_owner,
                    &msg,
                    item.attempt_count,
                    item.max_attempts,
                )
                .await
                .map_err(AppError::from)?;
                return Ok(());
            }
        };
        match crate::jobs::scan_posts::handle(pool, item, provider).await {
            Ok(()) => {
                if !queue::commit_success(pool, item.tenant_id, item.id, lease_owner).await? {
                    warn!(kind = %item.kind, id = %item.id, "lost lease on scan success commit");
                }
            }
            Err(err) => {
                let message = err.to_string();
                queue::commit_failure(
                    pool,
                    item.tenant_id,
                    item.id,
                    lease_owner,
                    &message,
                    item.attempt_count,
                    item.max_attempts,
                )
                .await
                .map_err(AppError::from)?;
                warn!(kind = %item.kind, id = %item.id, error = %message, "scan_posts failed");
            }
        }
        return Ok(());
    }

    if item.kind == "purge_content" {
        match crate::jobs::purge_content::handle(pool, item).await {
            Ok(()) => {
                if !queue::commit_success(pool, item.tenant_id, item.id, lease_owner).await? {
                    warn!(kind = %item.kind, id = %item.id, "lost lease on purge_content success");
                }
            }
            Err(err) => {
                let message = err.to_string();
                queue::commit_failure(
                    pool,
                    item.tenant_id,
                    item.id,
                    lease_owner,
                    &message,
                    item.attempt_count,
                    item.max_attempts,
                )
                .await
                .map_err(AppError::from)?;
                warn!(kind = %item.kind, id = %item.id, error = %message, "purge_content failed");
            }
        }
        return Ok(());
    }
    if item.kind == "purge_retention_metadata" {
        match crate::jobs::purge_retention::handle(pool, item).await {
            Ok(()) => {
                if !queue::commit_success(pool, item.tenant_id, item.id, lease_owner).await? {
                    warn!(kind = %item.kind, id = %item.id, "lost lease on purge_retention success");
                }
            }
            Err(err) => {
                let message = err.to_string();
                queue::commit_failure(
                    pool,
                    item.tenant_id,
                    item.id,
                    lease_owner,
                    &message,
                    item.attempt_count,
                    item.max_attempts,
                )
                .await
                .map_err(AppError::from)?;
                warn!(kind = %item.kind, id = %item.id, error = %message, "purge_retention failed");
            }
        }
        return Ok(());
    }

    if item.kind == "account.purge" {
        match handle_account_purge(
            pool,
            auth.provider.clone(),
            item.tenant_id,
            item.id,
            item.payload.clone(),
        )
        .await
        {
            Ok(()) => {
                let ok = queue::commit_success(pool, item.tenant_id, item.id, lease_owner)
                    .await
                    .map_err(AppError::from)?;
                if !ok {
                    warn!(kind = %item.kind, id = %item.id, "lost lease on success commit");
                } else {
                    debug!(kind = %item.kind, id = %item.id, "work item succeeded");
                }
            }
            Err(err) => {
                let msg = err.to_string();
                let status = queue::commit_failure(
                    pool,
                    item.tenant_id,
                    item.id,
                    lease_owner,
                    &msg,
                    item.attempt_count,
                    item.max_attempts,
                )
                .await
                .map_err(AppError::from)?;
                if status == "failed" {
                    emit_purge_alarm(item.tenant_id, item.id, &msg);
                }
                warn!(kind = %item.kind, id = %item.id, %status, error = %msg, "work item failed");
            }
        }
        return Ok(());
    }

    match handle_kind(item) {
        Ok(()) => {
            let ok = queue::commit_success(pool, item.tenant_id, item.id, lease_owner)
                .await
                .map_err(AppError::from)?;
            if !ok {
                warn!(
                    kind = %item.kind,
                    id = %item.id,
                    "lost lease on success commit"
                );
            } else {
                debug!(kind = %item.kind, id = %item.id, "work item succeeded");
            }
        }
        Err(err) => {
            let status = queue::commit_failure(
                pool,
                item.tenant_id,
                item.id,
                lease_owner,
                &err,
                item.attempt_count,
                item.max_attempts,
            )
            .await
            .map_err(AppError::from)?;
            if status == "lost_lease" {
                warn!(
                    kind = %item.kind,
                    id = %item.id,
                    error = %err,
                    "lost lease on failure commit"
                );
            } else {
                warn!(kind = %item.kind, id = %item.id, %status, error = %err, "work item failed");
            }
        }
    }
    Ok(())
}

/// Plan 002 ships no product handlers. `test.ping` echoes for integration tests.
fn handle_kind(item: &WorkItem) -> Result<(), String> {
    match item.kind.as_str() {
        "test.ping" => Ok(()),
        "test.fail" => Err("forced failure".into()),
        #[cfg(test)]
        "test.hang" => Ok(()),
        other => Err(format!("no handler registered for kind={other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LogFormat;
    use crate::db::migrate;
    use crate::repository::{users, work_items};
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    fn test_env() {
        let _ = dotenvy::from_filename(".env");
        if std::env::var("DATABASE_URL").is_err() {
            std::env::set_var(
                "DATABASE_URL",
                "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost",
            );
        }
        if std::env::var("DATABASE_URL_APP").is_err() {
            std::env::set_var(
                "DATABASE_URL_APP",
                "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost",
            );
        }
        if std::env::var("DATABASE_APP_ROLE").is_err() {
            std::env::set_var("DATABASE_APP_ROLE", "ghostpost_app");
        }
        if std::env::var("GHOSTPOST_ENV").is_err() {
            std::env::set_var("GHOSTPOST_ENV", "development");
        }
    }


    fn test_auth(pool: PgPool) -> Arc<AuthService> {
        use crate::auth::provider::{
            AuthorizeUrlRequest, ProviderAuthSession, ProviderError,
            VerifiedWebhookEvent, WorkosIdentityProvider,
        };
        use crate::auth::{AuthConfig, AuthService};
        use async_trait::async_trait;

        struct NoopProvider;
        #[async_trait]
        impl WorkosIdentityProvider for NoopProvider {
            async fn authorization_url(
                &self,
                _req: AuthorizeUrlRequest,
            ) -> Result<String, ProviderError> {
                Ok("https://example.test/authorize".into())
            }
            async fn exchange_code(
                &self,
                _code: &str,
                _code_verifier: &str,
            ) -> Result<ProviderAuthSession, ProviderError> {
                Err(ProviderError::Request("noop".into()))
            }
            async fn refresh_session(
                &self,
                _refresh_token: &str,
            ) -> Result<ProviderAuthSession, ProviderError> {
                Err(ProviderError::Request("noop".into()))
            }
            async fn revoke_session(&self, _workos_session_id: &str) -> Result<(), ProviderError> {
                Ok(())
            }
            async fn delete_user(&self, _workos_user_id: &str) -> Result<(), ProviderError> {
                Ok(())
            }
            fn verify_webhook(
                &self,
                _signature_header: &str,
                _body: &[u8],
            ) -> Result<VerifiedWebhookEvent, ProviderError> {
                Err(ProviderError::Webhook("noop".into()))
            }
        }

        // Minimal auth config without requiring full env for ignored unit test.
        std::env::set_var("WORKOS_API_KEY", "sk_test");
        std::env::set_var("WORKOS_CLIENT_ID", "client_test");
        std::env::set_var("WORKOS_WEBHOOK_SECRET", "whsec_test");
        std::env::set_var(
            "WORKOS_COOKIE_PASSWORD",
            "correct-horse-battery-staple-secret!!",
        );
        std::env::set_var(
            "APP_SESSION_KEYS",
            r#"[{"id":"k1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#,
        );
        std::env::set_var("AUTH_WEB_REDIRECT_URI", "http://localhost:8081/auth/callback");
        std::env::set_var("AUTH_NATIVE_REDIRECT_URI", "ghostpost://auth/callback");
        std::env::set_var(
            "AUTH_WEB_ORIGINS",
            "http://localhost:8081,http://127.0.0.1:8081",
        );
        std::env::set_var(
            "CORS_ALLOWED_ORIGINS",
            "http://localhost:8081,http://127.0.0.1:8081",
        );
        std::env::set_var("AUTH_SECURE_COOKIES", "false");
        let config = AuthConfig::load_from_env(true).expect("auth config");
        Arc::new(AuthService::new(pool, config, Arc::new(NoopProvider)))
    }

    async fn app_pool() -> PgPool {
        test_env();
        let url = std::env::var("DATABASE_URL_APP").expect("DATABASE_URL_APP");
        PgPoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .expect("connect app pool")
    }

    fn worker_config(instance_id: &str) -> Config {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
        let database_url_app =
            std::env::var("DATABASE_URL_APP").unwrap_or_else(|_| database_url.clone());
        Config {
            database_url,
            database_url_app,
            database_app_role: "ghostpost_app".into(),
            db_max_connections: 5,
            migration_max_connections: 2,
            migration_lock_timeout: Duration::from_secs(60),
            bind_addr: "127.0.0.1:0".into(),
            log_format: LogFormat::Pretty,
            instance_id: instance_id.into(),
            worker_lease: Duration::from_secs(60),
            worker_heartbeat: Duration::from_millis(50),
            worker_sweeper: Duration::from_secs(3600),
            shutdown_deadline_secs: 5,
            restore_replay_pending: false,
            is_development: true,
        }
    }

    /// Lost-lease on commit must not abort the claim loop or abandon heartbeat join.
    #[tokio::test]
    #[ignore]
    async fn lost_lease_does_not_stop_claim_loop_or_detach_heartbeat() {
        test_env();
        let config = Config::load_for_migrate().expect("config");
        migrate::run(&config, false).await.expect("migrate");
        let pool = app_pool().await;

        sqlx::query(
            r#"
UPDATE work_items
SET status = 'cancelled', finished_at = now(), updated_at = now()
WHERE status IN ('pending', 'running')
"#,
        )
        .execute(&pool)
        .await
        .expect("drain");

        let tenant = users::create_tenant(&pool).await.expect("tenant");
        let user = users::create_user(
            &pool,
            tenant.id,
            None,
            Some("runner-lost-lease@example.com"),
            Some("R"),
        )
        .await
        .expect("user");

        let gate = test_hang::arm().await;
        let token = CancellationToken::new();
        let worker_pool = pool.clone();
        let worker_token = token.clone();
        let auth = test_auth(pool.clone());
        let worker = tokio::spawn(async move {
            run(
                worker_pool,
                worker_config("worker-lost-lease"),
                auth,
                Arc::new(crate::blob::UnavailableBlobStore),
                worker_token,
            )
            .await
        });

        let hang_item = work_items::enqueue(
            &pool,
            tenant.id,
            Some(user.id),
            "test.hang",
            json!({}),
            None,
            100,
            5,
        )
        .await
        .expect("enqueue hang");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let status: Option<String> = sqlx::query_scalar(
                r#"SELECT status FROM work_items WHERE tenant_id = $1 AND id = $2"#,
            )
            .bind(tenant.id)
            .bind(hang_item.id)
            .fetch_optional(&pool)
            .await
            .expect("status");
            if status.as_deref() == Some("running") {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("timed out waiting for hang item running");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        sqlx::query(
            r#"
UPDATE work_items
SET lease_owner = 'thief'
WHERE tenant_id = $1 AND id = $2
"#,
        )
        .bind(tenant.id)
        .bind(hang_item.id)
        .execute(&pool)
        .await
        .expect("steal");

        let follow_up = work_items::enqueue(
            &pool,
            tenant.id,
            Some(user.id),
            "test.ping",
            json!({}),
            None,
            50,
            5,
        )
        .await
        .expect("enqueue follow-up");

        test_hang::release(&gate);

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let status: Option<String> = sqlx::query_scalar(
                r#"SELECT status FROM work_items WHERE tenant_id = $1 AND id = $2"#,
            )
            .bind(tenant.id)
            .bind(follow_up.id)
            .fetch_optional(&pool)
            .await
            .expect("status");
            if status.as_deref() == Some("succeeded") {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                panic!("follow-up was not processed after lost lease; last={status:?}");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        token.cancel();
        let joined = tokio::time::timeout(Duration::from_secs(3), worker)
            .await
            .expect("worker must join heartbeat/sweeper within deadline after cancel")
            .expect("worker task join");
        assert!(
            joined.is_ok(),
            "lost-lease must not permanently fail the claim loop: {joined:?}"
        );

        test_hang::clear().await;
    }
}
