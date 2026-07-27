mod common;

use common::blob::{BlobCall, InMemoryBlobStore};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::jobs::archive_import::{normalize, purge_one, sweep_due};
use ghostpost_backend::repository::{archive_imports, users, work_items};
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::ffi::OsString;
use std::io::{Cursor, Write};
use std::sync::Arc;
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

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

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
            )
            .expect("start synthetic ZIP entry");
        writer
            .write_all(bytes)
            .expect("write synthetic ZIP entry");
    }
    writer.finish().expect("finish synthetic ZIP").into_inner()
}

struct EnvRestore {
    name: &'static str,
    previous: Option<OsString>,
}

impl EnvRestore {
    fn set(name: &'static str, value: &str) -> Self {
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self { name, previous }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

async fn pinned_import(
    pool: &PgPool,
    platform: &str,
    bytes: &[u8],
    key: &str,
    version_id: &str,
    status: &str,
) -> (Uuid, Uuid, Uuid) {
    let tenant = users::create_tenant(pool).await.expect("tenant");
    let user = users::create_user(
        pool,
        tenant.id,
        None,
        None,
        Some("Synthetic Archive Worker User"),
    )
    .await
    .expect("user");
    let import = archive_imports::create_import(pool, tenant.id, user.id, platform)
        .await
        .expect("archive import");
    sqlx::query(
        "UPDATE archive_imports SET status=$3,raw_storage_key=$4,raw_storage_version_id=$5,byte_size=$6,content_type='application/zip',raw_delete_after=now()+interval '24 hours' WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.id)
    .bind(import.id)
    .bind(status)
    .bind(key)
    .bind(version_id)
    .bind(i64::try_from(bytes.len()).unwrap())
    .execute(pool)
    .await
    .expect("pin archive import");
    (tenant.id, user.id, import.id)
}

async fn running_normalize_item(
    pool: &PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    import_id: Uuid,
    key: &str,
    version_id: &str,
    lease_owner: &str,
) -> ghostpost_backend::repository::work_items::WorkItem {
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO work_items(tenant_id,subject_user_id,kind,payload,status,attempt_count,lease_owner,lease_expires_at,heartbeat_at) VALUES($1,$2,'import.normalize',$3,'running',1,$4,now()+interval '5 minutes',now()) RETURNING id",
    )
    .bind(tenant_id)
    .bind(user_id)
    .bind(json!({"import_id":import_id,"key":key,"version_id":version_id}))
    .bind(lease_owner)
    .fetch_one(pool)
    .await
    .expect("running normalization item");
    work_items::get(pool, tenant_id, id)
        .await
        .expect("load work item")
        .expect("work item exists")
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires migrated local Postgres"]
async fn worker_reads_only_the_pinned_version_then_atomically_commits_multifile_records_without_scan_or_model_work() {
    let _fingerprint_key = EnvRestore::set(
        "ARCHIVE_FINGERPRINT_KEYS",
        r#"[{"version":7,"secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#,
    );
    let pool = migrated_pool().await;
    let archive = zip(&[
        (
            "comments.csv",
            &b"id,permalink,date,subreddit,parent,body\nc1,/r/s/comments/t3_p1/c1,2026-07-23T12:01:00Z,s,t3_p1,comment body\n"[..],
        ),
        (
            "posts.csv",
            &b"id,permalink,date,subreddit,title,url,body\np1,/r/s/comments/p1/title,2026-07-23T12:00:00Z,s,Active title,https://example.invalid,post body\np2,/r/s/comments/p2/title,2026-07-23T12:02:00Z,s,[deleted by user],https://example.invalid,discarded body\n"[..],
        ),
    ]);
    let key = format!("archive-imports/{}/source.zip", Uuid::new_v4());
    let pinned_version = "immutable-version-a";
    let (tenant_id, user_id, import_id) = pinned_import(
        &pool,
        "reddit",
        &archive,
        &key,
        pinned_version,
        "queued",
    )
    .await;
    let item = running_normalize_item(
        &pool,
        tenant_id,
        user_id,
        import_id,
        &key,
        pinned_version,
        "current-owner",
    )
    .await;
    let blob = Arc::new(InMemoryBlobStore::default());
    blob.put_latest(
        &key,
        pinned_version,
        "application/zip",
        archive.clone(),
    );
    blob.put_latest(
        &key,
        "newer-overwrite",
        "application/zip",
        &b"not the pinned archive"[..],
    );

    let stale = normalize(&pool, blob.clone(), &item, "stale-owner")
        .await
        .expect_err("a stale lease cannot publish normalized content");
    assert!(stale.to_string().contains("lost import fence"));
    assert_eq!(
        blob.calls(),
        vec![
            BlobCall::HeadVersion {
                key: key.clone(),
                version_id: pinned_version.into(),
            },
            BlobCall::GetRange {
                key: key.clone(),
                version_id: pinned_version.into(),
                range: 0..archive.len() as u64,
            },
        ],
        "the stale attempt reads only the immutable pin and cannot purge it"
    );
    let staged_after_stale: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staged_after_stale, 3, "lost fencing retains the retryable staged set");
    let published_after_stale: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM archive_import_content_items WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(published_after_stale, 0, "lost fencing publishes nothing");

    blob.clear_calls();
    normalize(&pool, blob.clone(), &item, "current-owner")
        .await
        .expect("the current lease deterministically retries and commits");
    assert_eq!(
        blob.calls(),
        vec![
            BlobCall::HeadVersion {
                key: key.clone(),
                version_id: pinned_version.into(),
            },
            BlobCall::GetRange {
                key: key.clone(),
                version_id: pinned_version.into(),
                range: 0..archive.len() as u64,
            },
            BlobCall::DeleteVersion {
                key: key.clone(),
                version_id: pinned_version.into(),
            },
        ],
        "the winning attempt reads and immediately purges that same exact version"
    );

    let committed: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT source_revision_id,content_state,title,body FROM content_items WHERE tenant_id=$1 AND user_id=$2 ORDER BY source_revision_id",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        committed,
        vec![
            ("c1".into(), "active".into(), None, Some("comment body".into())),
            (
                "p1".into(),
                "active".into(),
                Some("Active title".into()),
                Some("post body".into()),
            ),
            ("p2".into(), "deleted".into(), None, None),
        ],
        "archive-level normalization commits both selected files and preserves explicit tombstones"
    );
    let import_state: (String, Option<i32>, bool, i16, String, String) = sqlx::query_as(
        "SELECT status,item_count,raw_deleted_at IS NOT NULL,fingerprint_key_version,format_family,format_confidence FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        import_state,
        (
            "ready".into(),
            Some(3),
            true,
            7,
            "RedditGdprCsv".into(),
            "provisional".into(),
        )
    );
    let staging_left: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staging_left, 0, "successful commit removes retry staging");
    let forbidden_work: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM work_items WHERE tenant_id=$1 AND kind LIKE 'scan%' OR tenant_id=$1 AND kind LIKE 'model%'",
    )
    .bind(tenant_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let scans: i64 = sqlx::query_scalar("SELECT count(*)::bigint FROM scans WHERE tenant_id=$1")
        .bind(tenant_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let model_attempts: i64 =
        sqlx::query_scalar("SELECT count(*)::bigint FROM model_attempts WHERE tenant_id=$1")
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (forbidden_work, scans, model_attempts),
        (0, 0, 0),
        "archive normalization never schedules or invokes scanning/model work"
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires migrated local Postgres"]
async fn x_worker_orders_shards_joins_notes_preserves_tombstones_and_coalesces_cross_shard_edits() {
    let _fingerprint_key = EnvRestore::set(
        "ARCHIVE_FINGERPRINT_KEYS",
        r#"[{"version":7,"secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#,
    );
    let pool = migrated_pool().await;
    let archive = zip(&[
        (
            "data/tweets-part2.js",
            r#"window.YTD.tweets.part2 = [
              {"tweet":{"id_str":"12","full_text":"final edited body","created_at":"Thu Jul 23 12:02:00 +0000 2026","edit_control":{"edit_tweet_ids":["10","11","12"]}}}
            ]"#
                .as_bytes(),
        ),
        (
            "data/deleted-tweets.js",
            r#"window.YTD.deleted_tweets.part0 = [
              {"tweet":{"id_str":"30","created_at":"Thu Jul 23 12:06:00 +0000 2026"}}
            ]"#
                .as_bytes(),
        ),
        (
            "data/note-tweet.js",
            r#"window.YTD.note_tweet.part0 = [
              {"noteTweet":{"createdAt":"2026-07-23T12:05:00Z","core":{"text":"Long-form orchestration preview extended through the real archive worker."}}}
            ]"#
                .as_bytes(),
        ),
        (
            "data/tweets-part0.js",
            r#"window.YTD.tweets.part0 = [
              {"tweet":{"id_str":"10","full_text":"first edited body","created_at":"Thu Jul 23 12:00:00 +0000 2026","edit_control":{"edit_tweet_ids":["10","11","12"]}}},
              {"tweet":{"id_str":"20","full_text":"Long-form orchestration preview…","created_at":"Thu Jul 23 12:05:00 +0000 2026"}}
            ]"#
                .as_bytes(),
        ),
        (
            "data/tweets-part1.js",
            r#"window.YTD.tweets.part1 = [
              {"tweet":{"id_str":"11","full_text":"middle edited body","created_at":"Thu Jul 23 12:01:00 +0000 2026","edit_control":{"edit_tweet_ids":["10","11","12"]}}}
            ]"#
                .as_bytes(),
        ),
    ]);
    let key = format!("archive-imports/{}/x-source.zip", Uuid::new_v4());
    let pinned_version = "x-orchestration-version";
    let (tenant_id, user_id, import_id) =
        pinned_import(&pool, "x", &archive, &key, pinned_version, "queued").await;
    let item = running_normalize_item(
        &pool,
        tenant_id,
        user_id,
        import_id,
        &key,
        pinned_version,
        "x-current-owner",
    )
    .await;
    let blob = Arc::new(InMemoryBlobStore::default());
    blob.put_latest(
        &key,
        pinned_version,
        "application/zip",
        archive.clone(),
    );

    let stale = normalize(&pool, blob.clone(), &item, "x-stale-owner")
        .await
        .expect_err("the stale lease leaves the worker's normalized X records inspectable");
    assert!(stale.to_string().contains("lost import fence"));

    let staged: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT record_json FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2 ORDER BY source_revision_id",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_all(&pool)
    .await
    .expect("load normalized X staging records");
    assert_eq!(
        staged.len(),
        3,
        "three edit revisions across numerically ordered shards coalesce to the greatest revision"
    );

    let edited = staged
        .iter()
        .find(|record| record["source_revision_id"].as_str() == Some("12"))
        .expect("greatest edit revision is staged");
    assert_eq!(edited["source_logical_id"], "10");
    assert_eq!(edited["body"], "final edited body");
    assert_eq!(
        edited["provenance"]["source_file"],
        "data/tweets-part2.js"
    );
    assert_eq!(edited["provenance"]["source_ordinal"], 1);

    let note = staged
        .iter()
        .find(|record| record["source_revision_id"].as_str() == Some("20"))
        .expect("preview tweet is staged");
    assert_eq!(
        note["body"],
        "Long-form orchestration preview extended through the real archive worker."
    );
    assert_eq!(note["text_format"], "long_form_note");
    assert_eq!(
        note["provenance"]["source_file"],
        "data/tweets-part0.js"
    );
    assert_eq!(note["provenance"]["source_ordinal"], 2);

    let tombstone = staged
        .iter()
        .find(|record| record["source_revision_id"].as_str() == Some("30"))
        .expect("explicit deleted-tweets row is staged");
    assert_eq!(tombstone["state"], "deleted");
    assert_eq!(tombstone["title"], serde_json::Value::Null);
    assert_eq!(tombstone["body"], serde_json::Value::Null);
    assert_eq!(
        tombstone["provenance"]["source_file"],
        "data/deleted-tweets.js"
    );

    normalize(&pool, blob, &item, "x-current-owner")
        .await
        .expect("the current lease commits the same archive-level X normalization");

    let committed: Vec<(String, String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT source_logical_id,source_revision_id,content_state,text_format,body FROM content_items WHERE tenant_id=$1 AND user_id=$2 ORDER BY source_logical_id",
    )
    .bind(tenant_id)
    .bind(user_id)
    .fetch_all(&pool)
    .await
    .expect("load committed X content");
    assert_eq!(
        committed,
        vec![
            (
                "10".into(),
                "12".into(),
                "active".into(),
                "standard".into(),
                Some("final edited body".into()),
            ),
            (
                "20".into(),
                "20".into(),
                "active".into(),
                "long_form_note".into(),
                Some(
                    "Long-form orchestration preview extended through the real archive worker."
                        .into(),
                ),
            ),
            (
                "30".into(),
                "30".into(),
                "deleted".into(),
                "standard".into(),
                None,
            ),
        ]
    );
    let import_state: (String, Option<i32>, String, String, bool) = sqlx::query_as(
        "SELECT status,item_count,format_family,format_confidence,raw_deleted_at IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .expect("load completed X import");
    assert_eq!(
        import_state,
        (
            "ready".into(),
            Some(3),
            "XGdpr".into(),
            "compatible".into(),
            true,
        )
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires migrated local Postgres"]
async fn x_worker_rejects_an_incomplete_cross_shard_edit_chain_without_partial_publication() {
    let _fingerprint_key = EnvRestore::set(
        "ARCHIVE_FINGERPRINT_KEYS",
        r#"[{"version":7,"secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#,
    );
    let pool = migrated_pool().await;
    let archive = zip(&[
        (
            "data/tweets-part1.js",
            r#"window.YTD.tweets.part1 = [
              {"tweet":{"id_str":"92","full_text":"latest revision","created_at":"Thu Jul 23 12:02:00 +0000 2026","edit_control":{"edit_tweet_ids":["90","91","92"]}}}
            ]"#
                .as_bytes(),
        ),
        (
            "data/tweets-part0.js",
            r#"window.YTD.tweets.part0 = [
              {"tweet":{"id_str":"90","full_text":"first revision","created_at":"Thu Jul 23 12:00:00 +0000 2026","edit_control":{"edit_tweet_ids":["90","91","92"]}}}
            ]"#
                .as_bytes(),
        ),
    ]);
    let key = format!("archive-imports/{}/invalid-x-source.zip", Uuid::new_v4());
    let pinned_version = "x-incomplete-chain-version";
    let (tenant_id, user_id, import_id) =
        pinned_import(&pool, "x", &archive, &key, pinned_version, "queued").await;
    let item = running_normalize_item(
        &pool,
        tenant_id,
        user_id,
        import_id,
        &key,
        pinned_version,
        "x-invalid-owner",
    )
    .await;
    let blob = Arc::new(InMemoryBlobStore::default());
    blob.put_latest(&key, pinned_version, "application/zip", archive);

    let error = normalize(&pool, blob, &item, "x-invalid-owner")
        .await
        .expect_err("a missing middle revision makes the archive-level chain invalid");
    assert!(
        error.to_string().contains("invalid_edit_chain"),
        "the worker surfaces the stable deterministic parser code: {error}"
    );
    let staged: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM import_staging_records WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .expect("count staged records");
    let committed: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM archive_import_content_items WHERE tenant_id=$1 AND import_id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .expect("count published records");
    assert_eq!(
        (staged, committed),
        (0, 0),
        "chain validation fails before staging or publication"
    );
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn immediate_raw_purge_retries_provider_failure_without_marking_or_changing_the_exact_version() {
    let pool = migrated_pool().await;
    let archive = b"synthetic raw archive";
    let key = format!("archive-imports/{}/source.zip", Uuid::new_v4());
    let version = "purge-version-a";
    let (tenant_id, _user_id, import_id) =
        pinned_import(&pool, "reddit", archive, &key, version, "deleting").await;
    let blob = Arc::new(InMemoryBlobStore::default());
    blob.put_latest(&key, version, "application/zip", &archive[..]);
    blob.put_latest(
        &key,
        "surviving-newer-version",
        "application/zip",
        &b"newer raw archive"[..],
    );
    blob.set_delete_failures(1);

    purge_one(&pool, blob.clone(), tenant_id, import_id)
        .await
        .expect_err("provider failure must remain retryable");
    let after_failure: (String, bool, Option<String>) = sqlx::query_as(
        "SELECT status,raw_deleted_at IS NOT NULL,raw_storage_version_id FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        after_failure,
        ("deleting".into(), false, Some(version.into())),
        "failed exact-version deletion cannot advance the durable checkpoint"
    );

    purge_one(&pool, blob.clone(), tenant_id, import_id)
        .await
        .expect("durable retry deletes the exact pin");
    let after_retry: (String, bool) = sqlx::query_as(
        "SELECT status,raw_deleted_at IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant_id)
    .bind(import_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after_retry, ("deleted".into(), true));
    assert_eq!(
        blob.calls(),
        vec![
            BlobCall::DeleteVersion {
                key: key.clone(),
                version_id: version.into(),
            },
            BlobCall::DeleteVersion {
                key,
                version_id: version.into(),
            },
        ],
        "retries never broaden deletion from the immutable pin to the latest key"
    );
}

#[tokio::test]
#[ignore = "requires migrated local Postgres"]
async fn scheduled_raw_purge_deletes_only_due_exact_versions() {
    let pool = migrated_pool().await;
    let due_bytes = b"due archive";
    let future_bytes = b"future archive";
    let due_key = format!("archive-imports/{}/source.zip", Uuid::new_v4());
    let future_key = format!("archive-imports/{}/source.zip", Uuid::new_v4());
    let due_version = "due-version";
    let future_version = "future-version";
    let (due_tenant, _due_user, due_import) = pinned_import(
        &pool,
        "reddit",
        due_bytes,
        &due_key,
        due_version,
        "ready",
    )
    .await;
    let (future_tenant, _future_user, future_import) = pinned_import(
        &pool,
        "reddit",
        future_bytes,
        &future_key,
        future_version,
        "ready",
    )
    .await;
    sqlx::query(
        "UPDATE archive_imports SET raw_delete_after=now()-interval '1 second' WHERE tenant_id=$1 AND id=$2",
    )
    .bind(due_tenant)
    .bind(due_import)
    .execute(&pool)
    .await
    .unwrap();
    let due_before: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM archive_imports WHERE raw_deleted_at IS NULL AND raw_storage_key IS NOT NULL AND raw_storage_version_id IS NOT NULL AND raw_delete_after<=now()",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let blob = Arc::new(InMemoryBlobStore::default());
    blob.put_latest(&due_key, due_version, "application/zip", &due_bytes[..]);
    blob.put_latest(
        &future_key,
        future_version,
        "application/zip",
        &future_bytes[..],
    );

    let swept = sweep_due(&pool, blob.clone())
        .await
        .expect("scheduled purge sweep");
    assert_eq!(swept as i64, due_before.min(100));
    let due_deleted: bool = sqlx::query_scalar(
        "SELECT raw_deleted_at IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(due_tenant)
    .bind(due_import)
    .fetch_one(&pool)
    .await
    .unwrap();
    let future_deleted: bool = sqlx::query_scalar(
        "SELECT raw_deleted_at IS NOT NULL FROM archive_imports WHERE tenant_id=$1 AND id=$2",
    )
    .bind(future_tenant)
    .bind(future_import)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(due_deleted, "the due immutable version is purged");
    assert!(!future_deleted, "a future deadline is not purged early");
    assert!(
        blob.calls().contains(&BlobCall::DeleteVersion {
            key: due_key,
            version_id: due_version.into(),
        }),
        "the sweeper deletes the due exact version"
    );
    assert!(
        !blob.calls().contains(&BlobCall::DeleteVersion {
            key: future_key,
            version_id: future_version.into(),
        }),
        "the sweeper does not touch a future exact version"
    );
}
