//! Scan worker e2e with stub provider (requires DATABASE_URL).
use ghostpost_backend::jobs::scan_posts::{self, parse_batch_keys};
use ghostpost_backend::repository::{onboarding, scans, users, work_items};
use ghostpost_backend::scan::llm::stub::StubDeterministicProvider;
use ghostpost_backend::scan::llm::ScanLlmProvider;
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

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
    if std::env::var("SCAN_BATCH_HMAC_KEYS").is_err() {
        std::env::set_var(
            "SCAN_BATCH_HMAC_KEYS",
            r#"[{"id":"local_v1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#,
        );
    }
    std::env::set_var("SCAN_LLM_PROVIDER", "stub-deterministic");
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

async fn ensure_migrated() {
    test_env();
    let config = Config::load_for_migrate().expect("config");
    migrate::run(&config, false).await.expect("migrate");
}

#[tokio::test]
#[ignore]
async fn scan_worker_stub_e2e() {
    ensure_migrated().await;
    let pool = app_pool().await;

    let tenant = users::create_tenant(&pool).await.expect("tenant");
    let user = users::create_user(&pool, tenant.id, None, Some("scan@example.com"), Some("S"))
        .await
        .expect("user");

    onboarding::upsert(
        &pool,
        tenant.id,
        user.id,
        &["college_apps".into()],
        &["negativity".into(), "inappropriate_language".into()],
        &["x".into()],
    )
    .await
    .expect("onboarding");

    let import_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO archive_imports (tenant_id, user_id, platform, status)
VALUES ($1, $2, 'x', 'ready')
RETURNING id
"#,
    )
    .bind(tenant.id)
    .bind(user.id)
    .fetch_one(&pool)
    .await
    .expect("import");

    let content_id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO content_items (
  tenant_id, user_id, platform, kind, authorship, body,
  source_logical_id, source_revision_id, content_hmac
)
VALUES ($1, $2, 'x', 'tweet', 'owner_authored', $3, $4, $4, $5)
RETURNING id
"#,
    )
    .bind(tenant.id)
    .bind(user.id)
    .bind("these people are so dumb and worthless honestly")
    .bind(format!("logical-{}", Uuid::new_v4()))
    .bind(vec![7u8; 32])
    .fetch_one(&pool)
    .await
    .expect("content");

    sqlx::query(
        "INSERT INTO archive_import_content_items(tenant_id,import_id,content_item_id) VALUES($1,$2,$3)",
    )
    .bind(tenant.id)
    .bind(import_id)
    .bind(content_id)
    .execute(&pool)
    .await
    .expect("link");

    let scan = scans::create_scan(&pool, tenant.id, user.id)
        .await
        .expect("scan");

    let work = work_items::enqueue(
        &pool,
        tenant.id,
        Some(user.id),
        "scan_posts",
        json!({"scanId": scan.id, "archiveImportIds": [import_id]}),
        None,
        0,
        3,
    )
    .await
    .expect("enqueue");

    let provider: Arc<dyn ScanLlmProvider> = Arc::new(StubDeterministicProvider);
    scan_posts::handle(&pool, &work, provider)
        .await
        .expect("handle");

    let status: String = sqlx::query_scalar(
        "SELECT status FROM scans WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(scan.id)
    .fetch_one(&pool)
    .await
    .expect("status");
    assert_eq!(status, "succeeded");

    // model_attempts fenced only
    let attempts: Vec<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        r#"
SELECT outcome, request_sha256, result_sha256
FROM model_attempts
WHERE tenant_id=$1 AND scan_id=$2
"#,
    )
    .bind(tenant.id)
    .bind(scan.id)
    .fetch_all(&pool)
    .await
    .expect("attempts");
    assert!(!attempts.is_empty());
    for (outcome, req, res) in &attempts {
        assert!(outcome.is_some());
        assert!(req.as_ref().map(|s| s.len() == 64).unwrap_or(false));
        let _ = res;
    }

    // Ensure no raw prompt columns accidentally populated — just check table columns via attempt row shape
    let cols: Vec<String> = sqlx::query_scalar(
        r#"
SELECT column_name::text FROM information_schema.columns
WHERE table_name='model_attempts'
"#,
    )
    .fetch_all(&pool)
    .await
    .expect("cols");
    for bad in ["raw_prompt", "raw_completion", "prompt_text", "completion_text"] {
        assert!(!cols.iter().any(|c| c == bad), "found forbidden col {bad}");
    }
}

#[test]
fn batch_keys_parse_unit() {
    let keys = parse_batch_keys(
        r#"[{"id":"local_v1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#,
    )
    .unwrap();
    assert_eq!(keys[0].id, "local_v1");
}
