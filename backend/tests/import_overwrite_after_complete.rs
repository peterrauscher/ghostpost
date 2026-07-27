use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::primitives::ByteStream;
use ghostpost_backend::blob::s3::S3BlobStore;
use ghostpost_backend::blob::BlobStore;
use uuid::Uuid;

struct VersionCleanup<'a> {
    store: &'a S3BlobStore,
    key: String,
    versions: Vec<String>,
}

impl<'a> VersionCleanup<'a> {
    async fn run(self) {
        for version in self.versions {
            let _ = self.store.delete_version(&self.key, &version).await;
        }
    }
}

#[tokio::test]
#[ignore = "requires PLAN004_S3_INTEGRATION=1 and a private versioned S3-compatible bucket"]
async fn import_overwrite_after_complete_reads_and_deletes_only_the_pinned_version() {
    assert_eq!(
        std::env::var("PLAN004_S3_INTEGRATION").as_deref(),
        Ok("1"),
        "explicitly opt in to the destructive versioned-bucket integration gate"
    );
    let bucket = std::env::var("ARCHIVE_BUCKET").expect("ARCHIVE_BUCKET");
    let region = std::env::var("ARCHIVE_S3_REGION").expect("ARCHIVE_S3_REGION");
    let endpoint = std::env::var("ARCHIVE_S3_ENDPOINT").ok();
    let force_path_style = std::env::var("ARCHIVE_S3_FORCE_PATH_STYLE")
        .map(|value| value == "true")
        .unwrap_or(false);

    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(Region::new(region.clone()));
    if let Some(endpoint) = endpoint.as_deref() {
        loader = loader.endpoint_url(endpoint);
    }
    let shared = loader.load().await;
    let s3_config = aws_sdk_s3::config::Builder::from(&shared)
        .force_path_style(force_path_style)
        .build();
    let client = aws_sdk_s3::Client::from_conf(s3_config);
    let store = S3BlobStore::new(
        client.clone(),
        bucket.clone(),
        region,
        endpoint.unwrap_or_else(|| format!("https://{bucket}.s3.amazonaws.com")),
        false,
        None,
    );

    let key = format!("plan004-integration/{}/archive.zip", Uuid::new_v4());
    let version_a_bytes = b"PK\x03\x04synthetic-pinned-version-a";
    let version_b_bytes = b"PK\x03\x04synthetic-overwrite-version-b";

    let put_a = client
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .content_type("application/zip")
        .body(ByteStream::from_static(version_a_bytes))
        .send()
        .await
        .expect("upload version A");
    let version_a = put_a
        .version_id()
        .filter(|value| !value.is_empty() && *value != "null")
        .expect("bucket versioning must return immutable version A")
        .to_owned();

    let completed_head = store.head_object(&key).await.expect("HEAD completed upload A");
    assert_eq!(completed_head.version_id, version_a);
    assert_eq!(completed_head.content_length, version_a_bytes.len() as u64);
    assert_eq!(completed_head.content_type.as_deref(), Some("application/zip"));

    let put_b = client
        .put_object()
        .bucket(&bucket)
        .key(&key)
        .content_type("application/zip")
        .body(ByteStream::from_static(version_b_bytes))
        .send()
        .await
        .expect("reuse still-valid key for version B");
    let version_b = put_b
        .version_id()
        .filter(|value| !value.is_empty() && *value != "null")
        .expect("bucket versioning must return immutable version B")
        .to_owned();
    let cleanup = VersionCleanup {
        store: &store,
        key: key.clone(),
        versions: vec![version_a.clone(), version_b.clone()],
    };

    assert_ne!(version_a, version_b, "overwrite must create a distinct object version");
    let latest = store.head_object(&key).await.expect("HEAD latest version B");
    assert_eq!(latest.version_id, version_b);
    let pinned = store
        .head_object_version(&key, &version_a)
        .await
        .expect("HEAD pinned version A after overwrite");
    assert_eq!(pinned.version_id, version_a);
    assert_eq!(pinned.content_length, version_a_bytes.len() as u64);
    assert_eq!(
        store
            .get_range(&key, &version_a, 0..version_a_bytes.len() as u64)
            .await
            .expect("worker range-reads pinned A")
            .as_ref(),
        version_a_bytes
    );
    assert_eq!(
        store
            .get_range(&key, &version_b, 0..version_b_bytes.len() as u64)
            .await
            .expect("version B remains independently addressable")
            .as_ref(),
        version_b_bytes
    );

    store
        .delete_version(&key, &version_a)
        .await
        .expect("purge exact pinned A");
    assert!(
        store.head_object_version(&key, &version_a).await.is_err(),
        "pinned A is physically gone"
    );
    assert_eq!(
        store
            .get_range(&key, &version_b, 0..version_b_bytes.len() as u64)
            .await
            .expect("deleting A must not delete B")
            .as_ref(),
        version_b_bytes
    );

    cleanup.run().await;
}
