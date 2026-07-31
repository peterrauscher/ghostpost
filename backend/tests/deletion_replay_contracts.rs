use async_trait::async_trait;
use bytes::Bytes;
use chrono::{Duration, Utc};
use ghostpost_backend::auth::{
    AuthorizeUrlRequest, ProviderAuthSession, ProviderError, VerifiedWebhookEvent,
    WorkosIdentityProvider,
};
use ghostpost_backend::blob::{
    AccountDeletionMarker, BlobError, BlobStore, DeletionLedgerReceipt, ObjectHead, PresignedPost,
    ProviderDeletionReceipt,
};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::deletion::replay;
use ghostpost_backend::repository::users;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use std::{
    ops::Range,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration as StdDuration,
};

#[derive(Default)]
struct Ledger {
    markers: Mutex<Vec<AccountDeletionMarker>>,
    complete: Mutex<Vec<(uuid::Uuid, uuid::Uuid)>>,
}
#[async_trait]
impl BlobStore for Ledger {
    async fn presign_post_archive(
        &self,
        _: &str,
        _: u64,
        _: &str,
        _: StdDuration,
    ) -> Result<PresignedPost, BlobError> {
        unreachable!()
    }
    async fn head_object(&self, _: &str) -> Result<ObjectHead, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn head_object_version(&self, _: &str, _: &str) -> Result<ObjectHead, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn get_range(&self, _: &str, _: &str, _: Range<u64>) -> Result<Bytes, BlobError> {
        Err(BlobError::NotFound)
    }
    async fn delete_version(&self, _: &str, _: &str) -> Result<(), BlobError> {
        Ok(())
    }
    async fn record_deletion_marker(
        &self,
        marker: &AccountDeletionMarker,
    ) -> Result<DeletionLedgerReceipt, BlobError> {
        self.markers.lock().push(marker.clone());
        Ok(DeletionLedgerReceipt {
            key: "marker".into(),
        })
    }
    async fn record_provider_complete(
        &self,
        marker: &AccountDeletionMarker,
        _: &ProviderDeletionReceipt,
    ) -> Result<(), BlobError> {
        self.complete
            .lock()
            .push((marker.tenant_id, marker.user_id));
        Ok(())
    }
    async fn list_deletion_markers(
        &self,
        point: chrono::DateTime<Utc>,
    ) -> Result<Vec<AccountDeletionMarker>, BlobError> {
        Ok(self
            .markers
            .lock()
            .iter()
            .filter(|m| m.deleted_at > point)
            .cloned()
            .collect())
    }
    async fn provider_complete(&self, marker: &AccountDeletionMarker) -> Result<bool, BlobError> {
        Ok(self
            .complete
            .lock()
            .contains(&(marker.tenant_id, marker.user_id)))
    }
}
#[derive(Default)]
struct Provider {
    deletes: AtomicUsize,
}
#[async_trait]
impl WorkosIdentityProvider for Provider {
    async fn authorization_url(&self, _: AuthorizeUrlRequest) -> Result<String, ProviderError> {
        unreachable!()
    }
    async fn exchange_code(&self, _: &str, _: &str) -> Result<ProviderAuthSession, ProviderError> {
        unreachable!()
    }
    async fn refresh_session(&self, _: &str) -> Result<ProviderAuthSession, ProviderError> {
        unreachable!()
    }
    async fn revoke_session(&self, _: &str) -> Result<(), ProviderError> {
        unreachable!()
    }
    async fn delete_user(&self, _: &str) -> Result<(), ProviderError> {
        self.deletes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn verify_webhook(&self, _: &str, _: &[u8]) -> Result<VerifiedWebhookEvent, ProviderError> {
        unreachable!()
    }
}
fn database_env() {
    std::env::set_var(
        "DATABASE_URL",
        "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost",
    );
    std::env::set_var(
        "DATABASE_URL_APP",
        "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost",
    );
    std::env::set_var("DATABASE_APP_ROLE", "ghostpost_app");
    std::env::set_var("GHOSTPOST_ENV", "development");
}
#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn guarded_replay_is_idempotent_and_removes_restored_user() {
    database_env();
    let config = Config::load_for_migrate().unwrap();
    migrate::run(&config, false).await.unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = PgPoolOptions::new()
        .max_connections(2)
        .connect(&std::env::var("DATABASE_URL_APP").unwrap())
        .await
        .unwrap();
    let tenant = users::create_tenant(&app).await.unwrap();
    let workos_id = format!("workos_restore_{}", uuid::Uuid::new_v4());
    let user = users::create_user(&app, tenant.id, Some(&workos_id), None, None)
        .await
        .unwrap();
    let token = "restore-contract-token";
    let hash = Sha256::digest(token.as_bytes());
    sqlx::query("INSERT INTO ghostpost_restore_guard(singleton,token_hash) VALUES(true,$1) ON CONFLICT(singleton) DO UPDATE SET token_hash=excluded.token_hash").bind(hash.as_slice()).execute(&admin).await.unwrap();
    let marker = AccountDeletionMarker {
        schema_version: 1,
        tenant_id: tenant.id,
        user_id: user.id,
        deleted_at: Utc::now(),
    };
    let ledger = Arc::new(Ledger::default());
    ledger.markers.lock().push(marker.clone());
    let provider = Arc::new(Provider::default());
    let point = marker.deleted_at - Duration::seconds(1);
    assert_eq!(
        replay(&admin, ledger.clone(), provider.clone(), point, token)
            .await
            .unwrap(),
        1
    );
    assert_eq!(provider.deletes.load(Ordering::SeqCst), 1);
    assert_eq!(
        replay(&admin, ledger.clone(), provider.clone(), point, token)
            .await
            .unwrap(),
        1
    );
    assert_eq!(provider.deletes.load(Ordering::SeqCst), 1);
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE tenant_id=$1 AND id=$2")
            .bind(tenant.id)
            .bind(user.id)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    admin.close().await;
    app.close().await;
}
