use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::r#import::commit::commit_staged;
use ghostpost_backend::r#import::staging::{stage, BoundedIndex};
use ghostpost_backend::r#import::{
    ArchivePlatform, Authorship, ContentState, FormatConfidence, FormatFamily,
    NormalizedArchiveRecord, RecordProvenance, RecordType, RelationConfidence, TextFormat,
};
use ghostpost_backend::repository::{archive_imports, users};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

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
        .connect(
            &std::env::var("DATABASE_URL_APP").expect("DATABASE_URL_APP for integration test"),
        )
        .await
        .expect("connect application pool")
}

async fn tenant_user_import(pool: &PgPool) -> (Uuid, Uuid, Uuid) {
    let tenant = users::create_tenant(pool).await.expect("create test tenant");
    let user = users::create_user(pool, tenant.id, None, None, Some("Synthetic Archive User"))
        .await
        .expect("create test user");
    let import = archive_imports::create_import(pool, tenant.id, user.id, "reddit")
        .await
        .expect("create test import");
    (tenant.id, user.id, import.id)
}

fn comment(import_id: Uuid, revision: &str, body: &str) -> NormalizedArchiveRecord {
    NormalizedArchiveRecord {
        schema_version: 1,
        import_id,
        platform: ArchivePlatform::Reddit,
        source_logical_id: revision.into(),
        source_revision_id: revision.into(),
        record_type: RecordType::Comment,
        text_format: TextFormat::Standard,
        authorship: Authorship::OwnerAuthored,
        state: ContentState::Active,
        created_at: Some(chrono::DateTime::parse_from_rfc3339("2026-07-23T12:00:00Z").unwrap().into()),
        title: None,
        body: Some(body.into()),
        parent_source_id: Some("parent".into()),
        quoted_source_id: None,
        relation_confidence: RelationConfidence::Explicit,
        text_truncated: false,
        provenance: RecordProvenance {
            source_file: "comments.csv".into(),
            source_ordinal: 1,
            format_family: FormatFamily::RedditGdprCsv,
            format_confidence: FormatConfidence::Confirmed,
        },
    }
}

async fn running_import_work_item(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    import_id: Uuid,
    lease_owner: &str,
) -> Uuid {
    sqlx::query_scalar(
        r#"
INSERT INTO work_items(
    tenant_id, subject_user_id, kind, payload, status,
    attempt_count, lease_owner, lease_expires_at, heartbeat_at
)
VALUES($1, $2, 'import.normalize', $3, 'running', 1, $4, now() + interval '5 minutes', now())
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(json!({"import_id": import_id}))
    .bind(lease_owner)
    .fetch_one(pool)
    .await
    .expect("running import work item")
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn staging_spills_only_after_the_memory_budget_and_enforces_import_scope() {
    let pool = migrated_pool().await;
    let (tenant_id, _user_id, import_id) = tenant_user_import(&pool).await;
    let mut index = BoundedIndex::default();

    let memory_payload = vec![0x41; 32_777_216 - "memory".len()];
    index
        .insert(
            &pool,
            tenant_id,
            import_id,
            "x_note_ts",
            "memory".into(),
            memory_payload,
        )
        .await
        .expect("entry exactly at in-memory limit");
    let rows_before: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rows_before, 0, "the exact in-memory boundary must not spill early");

    index
        .insert(
            &pool,
            tenant_id,
            import_id,
            "x_note_ts",
            "spill".into(),
            b"bounded spill payload".to_vec(),
        )
        .await
        .expect("first over-budget entry spills");
    let spilled: (String, Vec<u8>) = sqlx::query_as(
        "SELECT entry_key,entry_payload FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2 AND index_kind='x_note_ts'",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(spilled.0, "spill");
    assert_eq!(spilled.1, b"bounded spill payload");

    let other_tenant = users::create_tenant(&pool).await.unwrap();
    let cross_tenant_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(other_tenant.id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cross_tenant_rows, 0, "spill rows are tenant/import scoped");
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn stale_fence_cannot_commit_then_valid_commit_is_atomic_deduped_and_scan_free() {
    let pool = migrated_pool().await;
    let (tenant_id, user_id, import_id) = tenant_user_import(&pool).await;
    let first = comment(import_id, "c1", "first");
    let second = comment(import_id, "c2", "second");
    stage(&pool, tenant_id, import_id, &first, &[1; 32])
        .await
        .expect("stage first record");
    stage(&pool, tenant_id, import_id, &first, &[1; 32])
        .await
        .expect("exact duplicate coalesces");
    stage(&pool, tenant_id, import_id, &second, &[2; 32])
        .await
        .expect("stage second record");
    sqlx::query(
        "INSERT INTO import_index_entries(tenant_id,import_id,index_kind,entry_key,entry_payload) VALUES($1,$2,'x_edit_chain','synthetic',$3)",
    )
    .bind(tenant_id)
    .bind(import_id)
    .bind(b"spill".as_slice())
    .execute(&pool)
    .await
    .expect("seed spill row");
    running_import_work_item(&pool, tenant_id, user_id, import_id, "current-owner").await;

    assert!(
        !commit_staged(&pool, tenant_id, import_id, "stale-owner")
            .await
            .expect("stale fence is a normal rejection"),
        "a stale worker cannot commit"
    );
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM content_items WHERE tenant_id=$1 AND user_id=$2",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 0);
    let staged_before: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staged_before, 2, "lost fence leaves retryable staging intact");

    assert!(
        commit_staged(&pool, tenant_id, import_id, "current-owner")
            .await
            .expect("valid fenced commit"),
        "current fence commits exactly once"
    );
    let committed: Vec<(String, String)> = sqlx::query_as(
        "SELECT source_revision_id,body FROM content_items WHERE tenant_id=$1 AND user_id=$2 ORDER BY source_revision_id",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        committed,
        vec![("c1".into(), "first".into()), ("c2".into(), "second".into())],
        "exact duplicates coalesce and stable revision order is committed"
    );
    let import_state: (String, Option<i32>, bool) = sqlx::query_as(
        "SELECT status,item_count,finished_at IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(import_state, ("ready".into(), Some(2), true));
    let staging_left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2) + (SELECT count(*) FROM import_index_entries WHERE tenant_id=$1 AND import_id=$2)",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staging_left, 0, "commit removes both staging stores");
    let scan_jobs: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM work_items WHERE tenant_id=$1 AND kind IN ('scan_posts','scan.batch')",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(scan_jobs, 0, "archive commit never creates scan work");
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn commit_failure_rolls_back_all_final_rows_and_ready_transition() {
    let pool = migrated_pool().await;
    let (tenant_id, user_id, import_id) = tenant_user_import(&pool).await;
    let valid = comment(import_id, "c-valid", "must roll back");
    stage(&pool, tenant_id, import_id, &valid, &[3; 32])
        .await
        .expect("stage valid record");
    let mut invalid = comment(import_id, "c-invalid", "invalid platform record");
    let mut invalid_json = serde_json::to_value(&mut invalid).unwrap();
    invalid_json["platform"] = json!("not-a-platform");
    sqlx::query(
        "INSERT INTO import_staging_records(tenant_id,import_id,source_revision_id,record_json,content_hmac) VALUES($1,$2,'c-invalid',$3,$4)",
    )
    .bind(tenant_id)
    .bind(import_id)
    .bind(invalid_json)
    .bind(vec![4_u8; 32])
    .execute(&pool)
    .await
    .expect("seed invalid staged row");
    running_import_work_item(&pool, tenant_id, user_id, import_id, "atomic-owner").await;

    commit_staged(&pool, tenant_id, import_id, "atomic-owner")
        .await
        .expect_err("content constraint must abort the transaction");

    let final_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM content_items WHERE tenant_id=$1 AND user_id=$2",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(final_rows, 0, "earlier inserts roll back with the failing row");
    let state: (String, Option<i32>, bool) = sqlx::query_as(
        "SELECT status,item_count,finished_at IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("awaiting_upload".into(), None, false));
    let staged: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staged, 2, "failed commit leaves complete retry/debug staging");
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn conflicting_duplicate_revision_never_silently_overwrites_staging() {
    let pool = migrated_pool().await;
    let (tenant_id, _user_id, import_id) = tenant_user_import(&pool).await;
    let original = comment(import_id, "c-conflict", "original body");
    let conflicting = comment(import_id, "c-conflict", "different body");
    stage(&pool, tenant_id, import_id, &original, &[0x11; 32])
        .await
        .expect("stage original revision");
    stage(&pool, tenant_id, import_id, &conflicting, &[0x22; 32])
        .await
        .expect_err("same revision with different canonical HMAC is a conflict");

    let rows: Vec<(serde_json::Value, Vec<u8>)> = sqlx::query_as(
        "SELECT record_json,content_hmac FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2 AND source_revision_id='c-conflict'",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0["body"], "original body");
    assert_eq!(rows[0].1, vec![0x11; 32]);
    let final_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM content_items WHERE tenant_id=$1 AND source_revision_id='c-conflict'",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(final_rows, 0, "conflict is rejected before final commit");
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn archive_fingerprint_is_exactly_32_bytes() {
    let pool = migrated_pool().await;
    let (tenant_id, _user_id, import_id) = tenant_user_import(&pool).await;
    sqlx::query(
        "UPDATE archive_imports SET archive_fingerprint=$3,fingerprint_key_version=1 WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .bind(vec![0x55_u8; 31])
    .execute(&pool)
    .await
    .expect_err("tenant HMAC fingerprints are exactly SHA-256 width");
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn archive_metadata_has_no_durable_raw_digest_or_etag_column() {
    let pool = migrated_pool().await;
    let forbidden_columns: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='archive_imports' AND column_name IN ('content_sha256','archive_sha256','etag','raw_etag')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        forbidden_columns, 0,
        "raw digests and provider ETags are never durable archive metadata"
    );
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn archive_fingerprint_dedupes_within_tenant_platform_but_not_across_tenants() {
    let pool = migrated_pool().await;
    let (tenant_a, user_a, import_a) = tenant_user_import(&pool).await;
    let second_a = archive_imports::create_import(&pool, tenant_a, user_a, "reddit")
        .await
        .unwrap();
    let (tenant_b, _user_b, import_b) = tenant_user_import(&pool).await;
    let fingerprint = vec![0x66_u8; 32];

    for (tenant, import) in [(tenant_a, import_a), (tenant_b, import_b)] {
        sqlx::query(
            "UPDATE archive_imports SET archive_fingerprint=$3,fingerprint_key_version=1 WHERE tenant_id=$1 AND id=$2",
        )
        .bind(tenant)
        .bind(import)
        .bind(&fingerprint)
        .execute(&pool)
        .await
        .expect("the same archive bytes in another tenant do not collide");
    }
    sqlx::query(
        "UPDATE archive_imports SET archive_fingerprint=$3,fingerprint_key_version=1 WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_a)
    .bind(second_a.id)
    .bind(&fingerprint)
    .execute(&pool)
    .await
    .expect_err("same tenant/platform fingerprint resolves as a duplicate import");

    let tenant_b_rows: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM archive_imports WHERE tenant_id=$1 AND archive_fingerprint=$2",
    )
    .bind(tenant_b)
    .bind(&fingerprint)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tenant_b_rows, 1);
}
