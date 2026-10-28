//! Real `import.normalize` runs against migrated Postgres:
//! - identical re-uploads resolve as duplicates (or wait for the original),
//! - terminal imports release their fingerprint,
//! - peak memory stays bounded on archives near the import size limits.
//!
//! `cargo test --locked --test import_pipeline_e2e -- --ignored --test-threads=1`
//! The memory test writes `target/e2e-artifacts/import-memory.json`.
mod common;

use async_trait::async_trait;
use bytes::Bytes;
use common::blob::InMemoryBlobStore;
use ghostpost_backend::blob::{BlobError, BlobStore, ObjectHead, PresignedPut};
use ghostpost_backend::config::Config;
use ghostpost_backend::db::migrate;
use ghostpost_backend::jobs::archive_import::{normalize, reject_if_deterministic, NormalizeOutcome, DUPLICATE_RECHECK_SECS};
use ghostpost_backend::jobs::queue;
use ghostpost_backend::repository::work_items::WorkItem;
use ghostpost_backend::repository::{archive_imports, users, work_items};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const FINGERPRINT_KEYS: &str = r#"[{"version":7,"secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#;

async fn migrated_pool() -> PgPool {
    for (key, value) in [
        ("DATABASE_URL", "postgres://ghostpost_migrator:ghostpost_migrator@127.0.0.1:5432/ghostpost"),
        ("DATABASE_URL_APP", "postgres://ghostpost_app:ghostpost_app@127.0.0.1:5432/ghostpost"),
        ("DATABASE_APP_ROLE", "ghostpost_app"),
        ("GHOSTPOST_ENV", "development"),
    ] {
        if std::env::var_os(key).is_none() { std::env::set_var(key, value); }
    }
    std::env::set_var("ARCHIVE_FINGERPRINT_KEYS", FINGERPRINT_KEYS);
    let config = Config::load_for_migrate().expect("migration configuration");
    migrate::run(&config, false).await.expect("migrations");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL_APP").unwrap())
        .await
        .expect("connect application pool")
}

fn reddit_zip(rows: &[(&str, &str)]) -> Vec<u8> {
    let mut csv = String::from("id,permalink,date,subreddit,parent,body\n");
    for (id, body) in rows {
        csv.push_str(&format!("{id},/r/s/comments/t3_p/{id},2026-07-23T12:00:00Z,s,t3_p,{body}\n"));
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("comments.csv", SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
    writer.write_all(csv.as_bytes()).unwrap();
    writer.finish().unwrap().into_inner()
}

async fn new_user(pool: &PgPool) -> (Uuid, Uuid) {
    let tenant = users::create_tenant(pool).await.expect("tenant");
    let user = users::create_user(pool, tenant.id, None, None, Some("Synthetic Pipeline User")).await.expect("user");
    (tenant.id, user.id)
}

/// An uploaded, completed (`queued`) import with its running normalize job.
async fn queued_import(pool: &PgPool, tenant_id: Uuid, user_id: Uuid, platform: &str, len: u64, etag: &str, lease_owner: &str) -> (Uuid, String, WorkItem) {
    let import = archive_imports::create_import(pool, tenant_id, user_id, platform).await.expect("import");
    let key = format!("archive-imports/{}/source.zip", import.id);
    sqlx::query("UPDATE archive_imports SET status='queued',raw_storage_key=$3,raw_storage_etag=$4,byte_size=$5,content_type='application/zip',raw_delete_after=now()+interval '24 hours' WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(import.id).bind(&key).bind(etag).bind(len as i64)
        .execute(pool).await.expect("pin import");
    let id: Uuid = sqlx::query_scalar("INSERT INTO work_items(tenant_id,subject_user_id,kind,payload,status,attempt_count,lease_owner,lease_expires_at,heartbeat_at) VALUES($1,$2,'import.normalize',$3,'running',1,$4,now()+interval '30 minutes',now()) RETURNING id")
        .bind(tenant_id).bind(user_id).bind(json!({"import_id": import.id, "key": key, "etag": etag})).bind(lease_owner)
        .fetch_one(pool).await.expect("running job");
    let item = work_items::get(pool, tenant_id, id).await.unwrap().unwrap();
    (import.id, key, item)
}

#[derive(Debug, sqlx::FromRow, PartialEq)]
struct ImportState {
    status: String,
    item_count: Option<i32>,
    duplicate_of_import_id: Option<Uuid>,
    has_fingerprint: bool,
    raw_purged: bool,
    links: i64,
}

async fn state(pool: &PgPool, tenant_id: Uuid, import_id: Uuid) -> ImportState {
    sqlx::query_as("SELECT status,item_count,duplicate_of_import_id,archive_fingerprint IS NOT NULL AS has_fingerprint,raw_deleted_at IS NOT NULL AS raw_purged,(SELECT count(*) FROM archive_import_content_items l WHERE l.tenant_id=a.tenant_id AND l.import_id=a.id) AS links FROM archive_imports a WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(import_id).fetch_one(pool).await.expect("import state")
}

fn fingerprint(tenant_id: Uuid, user_id: Uuid, bytes: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(&[0u8; 32]).unwrap();
    mac.update(tenant_id.as_bytes());
    mac.update(user_id.as_bytes());
    mac.update(&Sha256::digest(bytes));
    mac.finalize().into_bytes().to_vec()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires migrated local Postgres"]
async fn identical_reupload_becomes_a_ready_duplicate_sharing_the_original_content() {
    let pool = migrated_pool().await;
    let (tenant_id, user_id) = new_user(&pool).await;
    let archive = reddit_zip(&[("d1", "first body"), ("d2", "second body")]);
    let blob = Arc::new(InMemoryBlobStore::default());

    let (original, original_key, original_job) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-original\"", "owner").await;
    blob.put(&original_key, "\"etag-original\"", "application/zip", archive.clone());
    assert_eq!(normalize(&pool, blob.clone(), &original_job, "owner").await.expect("original import"), NormalizeOutcome::Completed);

    let (again, again_key, again_job) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-again\"", "owner").await;
    blob.put(&again_key, "\"etag-again\"", "application/zip", archive.clone());
    assert_eq!(
        normalize(&pool, blob.clone(), &again_job, "owner").await.expect("re-upload must not fail"),
        NormalizeOutcome::Completed
    );

    let first = state(&pool, tenant_id, original).await;
    let second = state(&pool, tenant_id, again).await;
    assert_eq!(first, ImportState { status: "ready".into(), item_count: Some(2), duplicate_of_import_id: None, has_fingerprint: true, raw_purged: true, links: 2 });
    assert_eq!(
        second,
        ImportState { status: "ready".into(), item_count: Some(2), duplicate_of_import_id: Some(original), has_fingerprint: false, raw_purged: true, links: 2 },
        "the duplicate is ready with the original's content and its own raw bytes are gone"
    );
    assert!(!blob.contains(&again_key), "duplicate raw bytes are purged");
    let shared: i64 = sqlx::query_scalar("SELECT count(*) FROM archive_import_content_items a JOIN archive_import_content_items b ON a.tenant_id=b.tenant_id AND a.content_item_id=b.content_item_id WHERE a.tenant_id=$1 AND a.import_id=$2 AND b.import_id=$3")
        .bind(tenant_id).bind(original).bind(again).fetch_one(&pool).await.unwrap();
    assert_eq!(shared, 2, "both imports link the same content items (scans can select either)");
    let content_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM content_items WHERE tenant_id=$1 AND user_id=$2")
        .bind(tenant_id).bind(user_id).fetch_one(&pool).await.unwrap();
    assert_eq!(content_rows, 2, "no second copy of the content");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires migrated local Postgres"]
async fn reupload_while_the_original_is_processing_defers_without_spending_attempts() {
    let pool = migrated_pool().await;
    let (tenant_id, user_id) = new_user(&pool).await;
    let archive = reddit_zip(&[("w1", "waiting body")]);
    let blob = Arc::new(InMemoryBlobStore::default());

    // The original holds the fingerprint but is mid-parse.
    let (original, original_key, original_job) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-o\"", "owner").await;
    blob.put(&original_key, "\"etag-o\"", "application/zip", archive.clone());
    sqlx::query("UPDATE archive_imports SET status='parsing',archive_fingerprint=$3,fingerprint_key_version=7 WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(original).bind(fingerprint(tenant_id, user_id, &archive)).execute(&pool).await.unwrap();

    let (again, again_key, again_job) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-a\"", "owner").await;
    blob.put(&again_key, "\"etag-a\"", "application/zip", archive.clone());
    assert_eq!(
        normalize(&pool, blob.clone(), &again_job, "owner").await.expect("in-progress twin is not an error"),
        NormalizeOutcome::Deferred { seconds: DUPLICATE_RECHECK_SECS }
    );
    assert!(queue::defer(&pool, tenant_id, again_job.id, "owner", DUPLICATE_RECHECK_SECS, "waiting").await.unwrap());
    let job: (String, i32, bool) = sqlx::query_as("SELECT status,attempt_count,run_after > now() FROM work_items WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(again_job.id).fetch_one(&pool).await.unwrap();
    assert_eq!(job, ("pending".into(), 0, true), "deferral refunds the attempt and schedules a later re-check");
    assert_eq!(state(&pool, tenant_id, again).await.status, "parsing");

    // The original finishes; the re-check resolves the twin as its duplicate.
    sqlx::query("UPDATE archive_imports SET archive_fingerprint=NULL WHERE tenant_id=$1 AND id=$2").bind(tenant_id).bind(original).execute(&pool).await.unwrap();
    assert_eq!(normalize(&pool, blob.clone(), &original_job, "owner").await.unwrap(), NormalizeOutcome::Completed);
    sqlx::query("UPDATE work_items SET status='running',attempt_count=1,lease_owner='owner',lease_expires_at=now()+interval '5 minutes' WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(again_job.id).execute(&pool).await.unwrap();
    assert_eq!(normalize(&pool, blob.clone(), &again_job, "owner").await.unwrap(), NormalizeOutcome::Completed);
    let resolved = state(&pool, tenant_id, again).await;
    assert_eq!((resolved.status.as_str(), resolved.duplicate_of_import_id, resolved.links), ("ready", Some(original), 1));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires migrated local Postgres"]
async fn rejected_imports_release_their_fingerprint_so_a_reupload_is_processed() {
    let pool = migrated_pool().await;
    let (tenant_id, user_id) = new_user(&pool).await;
    let archive = reddit_zip(&[("r1", "body")]);
    let blob = Arc::new(InMemoryBlobStore::default());

    let (first, first_key, first_job) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-1\"", "owner").await;
    blob.put(&first_key, "\"etag-1\"", "application/zip", archive.clone());
    sqlx::query("UPDATE archive_imports SET archive_fingerprint=$3,fingerprint_key_version=7 WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(first).bind(fingerprint(tenant_id, user_id, &archive)).execute(&pool).await.unwrap();
    assert!(reject_if_deterministic(&pool, blob.clone(), &first_job, "worker error: invalid_record").await.unwrap());
    let rejected = state(&pool, tenant_id, first).await;
    assert_eq!((rejected.status.as_str(), rejected.has_fingerprint, rejected.raw_purged), ("rejected", false, true));

    // A legacy rejected row that still holds the fingerprint must not block either.
    let (legacy, _, _) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-legacy\"", "legacy").await;
    sqlx::query("UPDATE archive_imports SET status='rejected',archive_fingerprint=$3,fingerprint_key_version=7 WHERE tenant_id=$1 AND id=$2")
        .bind(tenant_id).bind(legacy).bind(fingerprint(tenant_id, user_id, &archive)).execute(&pool).await.unwrap();

    let (again, again_key, again_job) = queued_import(&pool, tenant_id, user_id, "reddit", archive.len() as u64, "\"etag-2\"", "owner").await;
    blob.put(&again_key, "\"etag-2\"", "application/zip", archive.clone());
    assert_eq!(normalize(&pool, blob.clone(), &again_job, "owner").await.unwrap(), NormalizeOutcome::Completed);
    let processed = state(&pool, tenant_id, again).await;
    assert_eq!(
        processed,
        ImportState { status: "ready".into(), item_count: Some(1), duplicate_of_import_id: None, has_fingerprint: true, raw_purged: true, links: 1 },
        "the re-upload is parsed as a fresh import, not a duplicate of a rejected one"
    );
    assert!(!state(&pool, tenant_id, legacy).await.has_fingerprint, "the stale holder lost its claim");
}

// ---------------------------------------------------------------------------
// Bounded memory

/// Serves a file on disk with S3 conditional-read semantics, so the archive
/// under test never sits in this process's memory.
struct FileBlobStore {
    path: PathBuf,
    etag: String,
}

#[async_trait]
impl BlobStore for FileBlobStore {
    async fn presign_put_archive(&self, _: &str, _: u64, _: &str, _: Duration) -> Result<PresignedPut, BlobError> { unreachable!() }
    async fn head_object(&self, _: &str) -> Result<ObjectHead, BlobError> { self.head_object_if_match("", &self.etag).await }
    async fn head_object_if_match(&self, _: &str, etag: &str) -> Result<ObjectHead, BlobError> {
        if etag != self.etag { return Err(BlobError::Integrity); }
        let len = std::fs::metadata(&self.path).map_err(|_| BlobError::NotFound)?.len();
        Ok(ObjectHead { content_length: len, content_type: Some("application/zip".into()), etag: self.etag.clone(), last_modified: None })
    }
    async fn get_range(&self, _: &str, etag: &str, range: Range<u64>) -> Result<Bytes, BlobError> {
        if etag != self.etag { return Err(BlobError::Integrity); }
        let mut file = std::fs::File::open(&self.path).map_err(|_| BlobError::NotFound)?;
        file.seek(SeekFrom::Start(range.start)).map_err(|e| BlobError::Provider(e.to_string()))?;
        let mut buffer = vec![0u8; (range.end - range.start) as usize];
        file.read_exact(&mut buffer).map_err(|e| BlobError::Provider(e.to_string()))?;
        Ok(Bytes::from(buffer))
    }
    async fn delete_object(&self, _: &str) -> Result<(), BlobError> { Ok(()) }
}

/// Process-lifetime peak resident set size in bytes.
fn peak_rss() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
    let raw = usage.ru_maxrss as u64;
    if cfg!(target_os = "macos") { raw } else { raw * 1024 }
}

/// Deterministic, moderately compressible prose (deflate ratio ~3x, well
/// under the per-entry and aggregate ratio caps).
struct Prose(u64);
impl Prose {
    const WORDS: [&'static str; 32] = ["archive", "summer", "campus", "party", "review", "maybe", "later", "honestly", "game", "friday", "coffee", "exam", "team", "music", "travel", "weekend", "budget", "project", "message", "photo", "street", "river", "garden", "window", "yellow", "quiet", "random", "signal", "planet", "ticket", "little", "future"];
    fn text(&mut self, bytes: usize) -> String {
        let mut out = String::with_capacity(bytes + 16);
        while out.len() < bytes {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            out.push_str(Self::WORDS[(self.0 >> 59) as usize]);
            out.push_str(if (self.0 >> 40) % 11 == 0 { ". " } else { " " });
        }
        out.trim_end().to_owned()
    }
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).large_file(false)
}

/// Reddit: comments.csv + posts.csv, `per_file` uncompressed bytes each.
fn write_reddit_archive(path: &Path, per_file: usize) -> u64 {
    let mut zip = ZipWriter::new(std::fs::File::create(path).unwrap());
    let mut prose = Prose(7);
    let mut records = 0;
    for (name, header) in [("comments.csv", "id,permalink,date,subreddit,parent,body\n"), ("posts.csv", "id,permalink,date,subreddit,title,url,body\n")] {
        zip.start_file(name, deflated()).unwrap();
        zip.write_all(header.as_bytes()).unwrap();
        let (mut written, mut n) = (header.len(), 0u64);
        while written < per_file {
            let row = if name == "comments.csv" {
                format!("c{n},/r/s/comments/t3_p/c{n},2026-07-23T12:00:00Z,s,t3_p,{}\n", prose.text(3_800))
            } else {
                format!("p{n},/r/s/comments/p{n}/title,2026-07-23T12:00:00Z,s,{},https://example.invalid,{}\n", prose.text(60), prose.text(3_700))
            };
            zip.write_all(row.as_bytes()).unwrap();
            written += row.len();
            n += 1;
        }
        records += n;
    }
    zip.finish().unwrap();
    records
}

/// X: one note file plus two tweet shards of `per_shard` bytes. Every 500th
/// tweet starts a two-revision edit chain and every 1000th is a long-form
/// preview joined to a note, so all cross-record indexes are exercised.
fn write_x_archive(path: &Path, per_shard: usize) -> u64 {
    let mut zip = ZipWriter::new(std::fs::File::create(path).unwrap());
    let mut prose = Prose(11);
    let date = |n: u64| format!("Thu Jul 23 {:02}:{:02}:{:02} +0000 2026", (n / 3600) % 24, (n / 60) % 60, n % 60);
    let iso = |n: u64| format!("2026-07-23T{:02}:{:02}:{:02}Z", (n / 3600) % 24, (n / 60) % 60, n % 60);
    let (mut tweets, mut id) = (0u64, 1_000_000u64);
    let mut notes = Vec::new();
    for shard in 0..2 {
        zip.start_file(format!("data/tweets-part{shard}.js"), deflated()).unwrap();
        let head = format!("window.YTD.tweets.part{shard} = [\n");
        zip.write_all(head.as_bytes()).unwrap();
        let mut written = head.len();
        let mut first = true;
        while written < per_shard {
            id += 1;
            let text = prose.text(650);
            let tweet = if id % 500 == 0 {
                // Revision id-1 superseded by id: chain [id-1, id] (id-1 emitted below).
                format!("{{\"tweet\":{{\"id_str\":\"{id}\",\"full_text\":\"edited {text}\",\"created_at\":\"{}\",\"edit_control\":{{\"edit_tweet_ids\":[\"{}\",\"{id}\"]}}}}}},\n{{\"tweet\":{{\"id_str\":\"{}\",\"full_text\":\"original {text}\",\"created_at\":\"{}\",\"edit_control\":{{\"edit_tweet_ids\":[\"{}\",\"{id}\"]}}}}}}", date(id), id - 1, id - 1, date(id - 1), id - 1)
            } else if id % 1000 == 1 {
                // Long-form preview; the note shares its timestamp (unique seconds).
                notes.push(format!("{{\"noteTweet\":{{\"createdAt\":\"{}\",\"core\":{{\"text\":\"note {id} {text} continued in full\"}}}}}}", iso(id)));
                format!("{{\"tweet\":{{\"id_str\":\"{id}\",\"full_text\":\"note {id} {}…\",\"created_at\":\"{}\"}}}}", &text[..40], date(id))
            } else if id % 500 == 499 {
                continue; // emitted as the chain's first revision
            } else {
                format!("{{\"tweet\":{{\"id_str\":\"{id}\",\"full_text\":\"{text}\",\"created_at\":\"{}\"}}}}", date(id))
            };
            let line = if first { tweet } else { format!(",\n{tweet}") };
            first = false;
            zip.write_all(line.as_bytes()).unwrap();
            written += line.len();
            tweets += 1 + u64::from(id % 500 == 0);
        }
        zip.write_all(b"\n]").unwrap();
    }
    zip.start_file("data/note-tweet.js", deflated()).unwrap();
    zip.write_all(format!("window.YTD.note_tweet.part0 = [{}]", notes.join(",\n")).as_bytes()).unwrap();
    zip.finish().unwrap();
    tweets
}

struct Run {
    platform: &'static str,
    archive_bytes: u64,
    generated_records: u64,
    committed_items: i32,
    peak_rss_before: u64,
    peak_rss_after: u64,
    seconds: f64,
}

async fn run_one(pool: &PgPool, platform: &'static str, path: &Path, generated_records: u64) -> Run {
    let (tenant_id, user_id) = new_user(pool).await;
    let archive_bytes = std::fs::metadata(path).unwrap().len();
    let blob = Arc::new(FileBlobStore { path: path.to_owned(), etag: "\"etag-large\"".into() });
    let (import_id, _, job) = queued_import(pool, tenant_id, user_id, platform, archive_bytes, "\"etag-large\"", "memory-owner").await;
    let peak_rss_before = peak_rss();
    let started = Instant::now();
    let outcome = normalize(pool, blob, &job, "memory-owner").await.expect("large archive normalizes");
    let seconds = started.elapsed().as_secs_f64();
    let peak_rss_after = peak_rss();
    assert_eq!(outcome, NormalizeOutcome::Completed);
    let state = state(pool, tenant_id, import_id).await;
    assert_eq!(state.status, "ready");
    Run { platform, archive_bytes, generated_records, committed_items: state.item_count.unwrap_or(0), peak_rss_before, peak_rss_after, seconds }
}

/// Fixed bound on import working memory: one spool chunk (8 MiB), the ZIP
/// central directory, one inflate stream, one CSV record / JSON element
/// (<= 4 MiB), 256 records in flight, three cross-record indexes capped at
/// 8 MiB each before spilling, plus allocator slack.
const MAX_IMPORT_RSS_GROWTH: u64 = 128 * 1024 * 1024;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires migrated local Postgres; writes ~250 MB of temp archives"]
async fn large_archives_import_with_bounded_peak_memory() {
    let pool = migrated_pool().await;
    let dir = tempfile::tempdir().unwrap();
    let reddit = dir.path().join("reddit.zip");
    let x = dir.path().join("x.zip");
    let reddit_records = write_reddit_archive(&reddit, 110 * 1024 * 1024);
    let x_records = write_x_archive(&x, 90 * 1024 * 1024);

    let mut runs = Vec::new();
    for (platform, path, records) in [("reddit", &reddit, reddit_records), ("x", &x, x_records)] {
        runs.push(run_one(&pool, platform, path, records).await);
    }

    let report: Vec<_> = runs.iter().map(|run| json!({
        "platform": run.platform,
        "archiveBytes": run.archive_bytes,
        "generatedRecords": run.generated_records,
        "committedItems": run.committed_items,
        "peakRssBeforeBytes": run.peak_rss_before,
        "peakRssAfterBytes": run.peak_rss_after,
        "peakRssGrowthBytes": run.peak_rss_after.saturating_sub(run.peak_rss_before),
        "seconds": run.seconds,
    })).collect();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/e2e-artifacts");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("import-memory.json"), serde_json::to_vec_pretty(&json!({
        "boundBytes": MAX_IMPORT_RSS_GROWTH,
        "selectedUncompressedLimitBytes": ghostpost_backend::import::limits::MAX_SELECTED_TOTAL_UNCOMPRESSED_BYTES,
        "runs": report,
    })).unwrap()).unwrap();

    for run in &runs {
        let growth = run.peak_rss_after.saturating_sub(run.peak_rss_before);
        assert!(
            growth <= MAX_IMPORT_RSS_GROWTH,
            "{} import grew peak RSS by {} MiB (> {} MiB) for a {} MiB archive",
            run.platform, growth >> 20, MAX_IMPORT_RSS_GROWTH >> 20, run.archive_bytes >> 20
        );
        assert!(run.committed_items > 0);
    }
}
