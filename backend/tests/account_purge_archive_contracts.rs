use async_trait::async_trait;
use ghostpost_backend::auth::purge::handle_account_purge;
use ghostpost_backend::auth::{
    AuthorizeUrlRequest, ProviderAuthSession, ProviderError, VerifiedWebhookEvent,
    WorkosIdentityProvider,
};
use ghostpost_backend::config::Config;
use ghostpost_backend::blob::DeterministicBlobStore;
use ghostpost_backend::db::migrate;
use ghostpost_backend::repository::{archive_imports, users};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default)]
struct SyntheticProvider {
    delete_failures_remaining: AtomicUsize,
    delete_calls: AtomicUsize,
}

impl SyntheticProvider {
    fn failing_once() -> Self {
        Self {
            delete_failures_remaining: AtomicUsize::new(1),
            delete_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl WorkosIdentityProvider for SyntheticProvider {
    async fn authorization_url(&self, _request: AuthorizeUrlRequest) -> Result<String, ProviderError> {
        unreachable!("account purge does not authorize")
    }

    async fn exchange_code(
        &self,
        _code: &str,
        _code_verifier: &str,
    ) -> Result<ProviderAuthSession, ProviderError> {
        unreachable!("account purge does not exchange")
    }

    async fn refresh_session(
        &self,
        _refresh_token: &str,
    ) -> Result<ProviderAuthSession, ProviderError> {
        unreachable!("account purge does not refresh")
    }

    async fn revoke_session(&self, _workos_session_id: &str) -> Result<(), ProviderError> {
        unreachable!("account purge does not revoke an individual session")
    }

    async fn delete_user(&self, _workos_user_id: &str) -> Result<(), ProviderError> {
        self.delete_calls.fetch_add(1, Ordering::SeqCst);
        if self
            .delete_failures_remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(ProviderError::Request(
                "synthetic provider deletion failure".into(),
            ));
        }
        Ok(())
    }

    fn verify_webhook(
        &self,
        _signature_header: &str,
        _body: &[u8],
    ) -> Result<VerifiedWebhookEvent, ProviderError> {
        unreachable!("account purge does not verify webhooks")
    }
}

fn database_env() {
    if std::env::var_os("DATABASE_URL").is_none() {
        std::env::set_var(
            "DATABASE_URL",
            "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost",
        );
    }
    if std::env::var_os("DATABASE_URL_APP").is_none() {
        std::env::set_var(
            "DATABASE_URL_APP",
            "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost",
        );
    }
    if std::env::var_os("DATABASE_APP_ROLE").is_none() {
        std::env::set_var("DATABASE_APP_ROLE", "ghostpost_app");
    }
    if std::env::var_os("GHOSTPOST_ENV").is_none() {
        std::env::set_var("GHOSTPOST_ENV", "development");
    }
}

async fn migrated_pool() -> PgPool {
    database_env();
    let config = Config::load_for_migrate().expect("migration configuration");
    migrate::run(&config, false).await.expect("Plan 004 migrations");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL_APP").expect("DATABASE_URL_APP"))
        .await
        .expect("connect application pool")
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn account_purge_removes_every_archive_row_and_personal_field() {
    let pool = migrated_pool().await;
    let tenant = users::create_tenant(&pool).await.expect("tenant");
    let workos_id = format!("workos_synthetic_archive_purge_{}", Uuid::new_v4());
    let email = format!("{workos_id}@example.invalid");
    let user = users::create_user(
        &pool,
        tenant.id,
        Some(&workos_id),
        Some(&email),
        Some("Synthetic Archive User"),
    )
    .await
    .expect("user");
    let import = archive_imports::create_import(&pool, tenant.id, user.id, "reddit")
        .await
        .expect("import");
    sqlx::query(
        "UPDATE archive_imports SET raw_storage_key=$3,raw_storage_version_id=$4,status='ready',item_count=1 WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(import.id)
    .bind(format!("archive-imports/{}/source.zip", import.id))
    .bind("synthetic-version-a")
    .execute(&pool)
    .await
    .expect("pin raw version");
    let item = archive_imports::insert_content_item(
        &pool,
        tenant.id,
        user.id,
        "reddit",
        "comment",
        "owner_authored",
        Some("synthetic personal archive body"),
    )
    .await
    .expect("content item");
    sqlx::query(
        "INSERT INTO archive_import_content_items(tenant_id,import_id,content_item_id) VALUES($1,$2,$3)",
    )
    .bind(tenant.id)
    .bind(import.id)
    .bind(item.id)
    .execute(&pool)
    .await
    .expect("import/content link");
    sqlx::query(
        "INSERT INTO import_staging_records(tenant_id,import_id,source_revision_id,record_json,content_hmac) VALUES($1,$2,'staged', $3, $4)",
    )
    .bind(tenant.id)
    .bind(import.id)
    .bind(json!({"body":"synthetic staged personal text"}))
    .bind(vec![9_u8; 32])
    .execute(&pool)
    .await
    .expect("staging row");
    let deleted_at = chrono::Utc::now();
    let work_item_id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_items(tenant_id,subject_user_id,kind,payload,status) VALUES($1,$2,'account.purge',$3,'running') RETURNING id",
    )
    .bind(tenant.id)
    .bind(user.id)
    .bind(json!({"user_id":user.id,"deleted_at":deleted_at}))
    .fetch_one(&pool)
    .await
    .expect("account purge item");

    handle_account_purge(
        &pool,
        Arc::new(SyntheticProvider::default()),
        Arc::new(DeterministicBlobStore),
        tenant.id,
        work_item_id,
        json!({"user_id":user.id,"deleted_at":deleted_at}),
    )
    .await
    .expect("account purge workflow");

    let archive_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM archive_imports WHERE tenant_id=$1 AND user_id=$2",
    )
    .bind(tenant.id)
    .bind(user.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(archive_rows, 0, "pinned archive metadata is purged");
    let content_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM content_items WHERE tenant_id=$1 AND user_id=$2",
    )
    .bind(tenant.id)
    .bind(user.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(content_rows, 0, "normalized archive text is purged");
    let staging_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant.id)
    .bind(import.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staging_rows, 0, "staged archive text is purged");

    let survivor: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT workos_user_id,email,display_name FROM users WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(user.id)
    .fetch_one(&pool)
    .await
    .expect("minimal provider tombstone survives");
    assert_eq!(
        survivor,
        (Some(workos_id), None, None),
        "only the minimal provider retry identifier survives"
    );
    let checkpoint: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM work_items WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(work_item_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(checkpoint["scrubbed"], true);
    assert!(checkpoint["local_purged_at"].is_string());
    assert!(checkpoint["provider_purged_at"].is_string());
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn account_purge_checkpoints_local_deletion_before_provider_failure_and_resumes_without_recreating_personal_data() {
    let pool = migrated_pool().await;
    let tenant = users::create_tenant(&pool).await.expect("tenant");
    let workos_id = format!("workos_synthetic_archive_retry_{}", Uuid::new_v4());
    let user = users::create_user(
        &pool,
        tenant.id,
        Some(&workos_id),
        Some(&format!("{workos_id}@example.invalid")),
        Some("Synthetic Retry User"),
    )
    .await
    .expect("user");
    let import = archive_imports::create_import(&pool, tenant.id, user.id, "x")
        .await
        .expect("import");
    sqlx::query(
        "UPDATE archive_imports SET raw_storage_key=$3,raw_storage_version_id='retry-version',status='ready' WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(import.id)
    .bind(format!("archive-imports/{}/source.zip", import.id))
    .execute(&pool)
    .await
    .expect("pin raw archive");
    let item = archive_imports::insert_content_item(
        &pool,
        tenant.id,
        user.id,
        "x",
        "post",
        "owner_authored",
        Some("synthetic retry body"),
    )
    .await
    .expect("content item");
    sqlx::query(
        "INSERT INTO archive_import_content_items(tenant_id,import_id,content_item_id) VALUES($1,$2,$3)",
    )
    .bind(tenant.id)
    .bind(import.id)
    .bind(item.id)
    .execute(&pool)
    .await
    .expect("archive content link");
    let deleted_at = chrono::Utc::now();
    let initial_payload = json!({"user_id":user.id,"deleted_at":deleted_at});
    let work_item_id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_items(tenant_id,subject_user_id,kind,payload,status) VALUES($1,$2,'account.purge',$3,'running') RETURNING id",
    )
    .bind(tenant.id)
    .bind(user.id)
    .bind(&initial_payload)
    .fetch_one(&pool)
    .await
    .expect("account purge work item");
    let provider = Arc::new(SyntheticProvider::failing_once());

    handle_account_purge(
        &pool,
        provider.clone(),
        Arc::new(DeterministicBlobStore),
        tenant.id,
        work_item_id,
        initial_payload,
    )
    .await
    .expect_err("provider failure remains a durable retry boundary");
    let checkpoint_after_failure: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM work_items WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(work_item_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let local_checkpoint = checkpoint_after_failure["local_purged_at"]
        .as_str()
        .expect("local purge checkpoint")
        .to_owned();
    assert_eq!(checkpoint_after_failure["provider_purged_at"], serde_json::Value::Null);
    assert_eq!(provider.delete_calls.load(Ordering::SeqCst), 1);
    let after_failure: (i64, i64, Option<String>, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM archive_imports WHERE tenant_id=$1 AND user_id=$2), (SELECT count(*) FROM content_items WHERE tenant_id=$1 AND user_id=$2), workos_user_id,email,display_name FROM users WHERE tenant_id=$1 AND id=$2",
        )
        .bind(tenant.id)
        .bind(user.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        after_failure,
        (0, 0, Some(workos_id.clone()), None, None),
        "local personal data is durably gone while the minimal provider retry identifier survives"
    );

    handle_account_purge(
        &pool,
        provider.clone(),
        Arc::new(DeterministicBlobStore),
        tenant.id,
        work_item_id,
        checkpoint_after_failure,
    )
    .await
    .expect("retry resumes at provider deletion");
    let final_checkpoint: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM work_items WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(work_item_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        final_checkpoint["local_purged_at"].as_str(),
        Some(local_checkpoint.as_str()),
        "provider retry preserves rather than redoing the local purge checkpoint"
    );
    assert!(final_checkpoint["provider_purged_at"].is_string());
    assert_eq!(final_checkpoint["scrubbed"], true);
    assert_eq!(
        provider.delete_calls.load(Ordering::SeqCst),
        2,
        "the durable retry repeats only the failed provider boundary"
    );
}
