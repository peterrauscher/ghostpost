//! Upload/pin/purge contract against a real S3-compatible store (MinIO in
//! Compose, R2 in production). Run with the archive env from `.env.example`:
//!
//! `ARCHIVE_S3_INTEGRATION=1 cargo test --locked --test archive_store_s3_contract -- --ignored`
//!
//! Writes `target/e2e-artifacts/archive-store-s3-contract.json` with every
//! observed status code so the run can be compared across stores.
use aws_sdk_s3::primitives::ByteStream;
use chrono::{TimeZone, Utc};
use ghostpost_backend::blob::s3::S3BlobStore;
use ghostpost_backend::blob::{AccountDeletionMarker, BlobError, BlobStore, PresignedPut, ProviderDeletionReceipt};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

const CONTENT_TYPE: &str = "application/zip";

async fn put(http: &reqwest::Client, upload: &PresignedPut, content_type: &str, body: &'static [u8]) -> u16 {
    let mut request = http.put(&upload.url).body(body);
    for (name, value) in &upload.headers {
        let value = if name.eq_ignore_ascii_case("content-type") { content_type } else { value };
        request = request.header(name, value);
    }
    request.send().await.expect("presigned PUT reaches the store").status().as_u16()
}

#[tokio::test]
#[ignore = "requires ARCHIVE_S3_INTEGRATION=1 and an S3-compatible bucket (Compose MinIO or R2)"]
async fn presigned_put_is_create_once_size_and_type_bound_and_pins_by_etag() {
    assert_eq!(
        std::env::var("ARCHIVE_S3_INTEGRATION").as_deref(),
        Ok("1"),
        "explicitly opt in to the destructive object-store integration gate"
    );
    let store = S3BlobStore::from_env().await.expect("archive store from env");
    let admin = {
        let region = std::env::var("ARCHIVE_S3_REGION").unwrap();
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new(region));
        if let Ok(endpoint) = std::env::var("ARCHIVE_S3_ENDPOINT") {
            loader = loader.endpoint_url(endpoint);
        }
        let shared = loader.load().await;
        aws_sdk_s3::Client::from_conf(
            aws_sdk_s3::config::Builder::from(&shared)
                .force_path_style(std::env::var("ARCHIVE_S3_FORCE_PATH_STYLE").as_deref() == Ok("true"))
                .build(),
        )
    };
    let bucket = std::env::var("ARCHIVE_BUCKET").unwrap();
    let http = reqwest::Client::new();
    let mut report = serde_json::Map::new();

    let key = format!("archive-imports/{}/source.zip", Uuid::new_v4());
    let original: &'static [u8] = b"PK\x03\x04synthetic-archive-original";
    let upload = store
        .presign_put_archive(&key, original.len() as u64, CONTENT_TYPE, Duration::from_secs(900))
        .await
        .expect("presign");

    let short = put(&http, &upload, CONTENT_TYPE, &original[..original.len() - 1]).await;
    let wrong_type = put(&http, &upload, "application/octet-stream", original).await;
    let accepted = put(&http, &upload, CONTENT_TYPE, original).await;
    let replay = put(&http, &upload, CONTENT_TYPE, b"PK\x03\x04synthetic-archive-replaced").await;
    report.insert("putShortBody".into(), json!(short));
    report.insert("putWrongContentType".into(), json!(wrong_type));
    report.insert("putExact".into(), json!(accepted));
    report.insert("putReplay".into(), json!(replay));
    assert_eq!(short, 403, "a body shorter than the reserved length fails the signature");
    assert_eq!(wrong_type, 403, "a different content type fails the signature");
    assert_eq!(accepted, 200, "the exact reserved upload succeeds");
    assert_eq!(replay, 412, "the presigned URL cannot overwrite the uploaded object");

    let head = store.head_object(&key).await.expect("completion HEAD");
    assert_eq!(head.content_length, original.len() as u64);
    assert_eq!(head.content_type.as_deref(), Some(CONTENT_TYPE));
    assert!(!head.etag.is_empty());
    let pinned = store.head_object_if_match(&key, &head.etag).await.expect("pinned HEAD");
    assert_eq!(pinned.etag, head.etag);
    let bytes = store.get_range(&key, &head.etag, 0..original.len() as u64).await.expect("pinned read");
    assert_eq!(bytes.as_ref(), original);

    admin
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .content_type(CONTENT_TYPE)
        .body(ByteStream::from_static(b"PK\x03\x04out-of-band-overwrite"))
        .send()
        .await
        .expect("out-of-band overwrite with admin credentials");
    let stale_head = store.head_object_if_match(&key, &head.etag).await;
    let stale_read = store.get_range(&key, &head.etag, 0..4).await;
    report.insert("pinnedHeadAfterOverwrite".into(), json!(format!("{stale_head:?}")));
    report.insert("pinnedReadAfterOverwrite".into(), json!(format!("{stale_read:?}")));
    assert!(matches!(stale_head, Err(BlobError::Integrity)), "pinned HEAD after overwrite: {stale_head:?}");
    assert!(matches!(stale_read, Err(BlobError::Integrity)), "pinned read after overwrite: {stale_read:?}");

    store.delete_object(&key).await.expect("purge by key");
    let after_delete = store.head_object(&key).await;
    report.insert("headAfterDelete".into(), json!(format!("{after_delete:?}")));
    assert!(matches!(after_delete, Err(BlobError::NotFound)), "{after_delete:?}");

    let marker = AccountDeletionMarker {
        schema_version: 1,
        tenant_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        deleted_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap(),
    };
    let first = store.record_deletion_marker(&marker).await.expect("ledger marker");
    let retry = store.record_deletion_marker(&marker).await.expect("identical retry is idempotent");
    assert_eq!(first, retry);
    let receipt = |minute| ProviderDeletionReceipt {
        schema_version: 1,
        tenant_id: marker.tenant_id,
        user_id: marker.user_id,
        completed_at: Utc.with_ymd_and_hms(2026, 10, 20, 12, minute, 0).unwrap(),
    };
    store.record_provider_complete(&marker, &receipt(1)).await.expect("provider receipt");
    let conflicting = store.record_provider_complete(&marker, &receipt(2)).await;
    report.insert("conflictingReceipt".into(), json!(format!("{conflicting:?}")));
    assert!(matches!(conflicting, Err(BlobError::Integrity)), "ledger entries are create-once: {conflicting:?}");
    assert!(store.provider_complete(&marker).await.expect("receipt lookup"));
    // A ledger delete succeeds on MinIO; on R2 the bucket lock must refuse it.
    // A refused synthetic marker is harmless: it has a provider receipt and no
    // user row, so deletion replay treats it as already complete.
    let mut ledger_deletes = Vec::new();
    for ledger_key in [first.key, ghostpost_backend::blob::provider_receipt_key(&marker)] {
        let deleted = admin.delete_object().bucket(&bucket).key(ledger_key).send().await;
        ledger_deletes.push(match deleted {
            Ok(_) => "deleted".to_owned(),
            Err(err) => aws_sdk_s3::error::DisplayErrorContext(&err).to_string(),
        });
    }
    if std::env::var("ARCHIVE_EXPECT_LEDGER_LOCK").as_deref() == Ok("1") {
        assert!(
            ledger_deletes.iter().all(|r| r.contains("ObjectLockedByBucketPolicy")),
            "the ledger prefix must be locked: {ledger_deletes:?}"
        );
    }
    report.insert("ledgerDeletes".into(), json!(ledger_deletes));

    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/e2e-artifacts");
    std::fs::create_dir_all(&dir).unwrap();
    report.insert("uploadUrlHost".into(), json!(reqwest::Url::parse(&upload.url).unwrap().host_str()));
    report.insert("signedHeaders".into(), json!(upload.headers));
    std::fs::write(
        dir.join("archive-store-s3-contract.json"),
        serde_json::to_vec_pretty(&serde_json::Value::Object(report)).unwrap(),
    )
    .unwrap();
}
