use async_trait::async_trait;
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use ghostpost_backend::blob::{BlobError, BlobStore, ObjectHead, PresignedPut};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Mutex;
use std::time::Duration;

/// Base URL of fake presigned PUTs; the object key is the remaining path.
pub const UPLOAD_BASE: &str = "https://archive-upload.test/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobCall {
    Presign {
        key: String,
        content_length: u64,
        content_type: String,
    },
    Head {
        key: String,
    },
    HeadIfMatch {
        key: String,
        etag: String,
    },
    GetRange {
        key: String,
        etag: String,
        range: Range<u64>,
    },
    Delete {
        key: String,
    },
}

#[derive(Debug, Clone)]
struct StoredObject {
    etag: String,
    content_type: String,
    bytes: Bytes,
}

#[derive(Debug, Default)]
struct State {
    objects: HashMap<String, StoredObject>,
    calls: Vec<BlobCall>,
    delete_failures_remaining: usize,
}

/// Unversioned store with S3 conditional-read semantics: a pinned read whose
/// ETag no longer matches fails with `BlobError::Integrity` (HTTP 412).
#[derive(Debug, Default)]
pub struct InMemoryBlobStore {
    state: Mutex<State>,
}

/// Object key of a reservation minted by `InMemoryBlobStore`.
pub fn reserved_key(reservation: &serde_json::Value) -> &str {
    reservation["upload"]["url"]
        .as_str()
        .and_then(|url| url.strip_prefix(UPLOAD_BASE))
        .expect("fake presigned PUT URL")
}

impl InMemoryBlobStore {
    /// Stores (or overwrites) `key` with a new ETag, like an out-of-band PUT.
    pub fn put(
        &self,
        key: impl Into<String>,
        etag: impl Into<String>,
        content_type: impl Into<String>,
        bytes: impl Into<Bytes>,
    ) {
        let object = StoredObject {
            etag: etag.into(),
            content_type: content_type.into(),
            bytes: bytes.into(),
        };
        self.state
            .lock()
            .expect("blob fixture mutex")
            .objects
            .insert(key.into(), object);
    }

    pub fn contains(&self, key: &str) -> bool {
        self.state.lock().expect("blob fixture mutex").objects.contains_key(key)
    }

    pub fn set_delete_failures(&self, count: usize) {
        self.state
            .lock()
            .expect("blob fixture mutex")
            .delete_failures_remaining = count;
    }

    pub fn calls(&self) -> Vec<BlobCall> {
        self.state
            .lock()
            .expect("blob fixture mutex")
            .calls
            .clone()
    }

    pub fn clear_calls(&self) {
        self.state.lock().expect("blob fixture mutex").calls.clear();
    }

    fn object_head(object: &StoredObject) -> ObjectHead {
        ObjectHead {
            content_length: object.bytes.len() as u64,
            content_type: Some(object.content_type.clone()),
            etag: object.etag.clone(),
            last_modified: None,
        }
    }

    fn matching<'a>(state: &'a State, key: &str, etag: &str) -> Result<&'a StoredObject, BlobError> {
        let object = state.objects.get(key).ok_or(BlobError::NotFound)?;
        if object.etag != etag {
            return Err(BlobError::Integrity);
        }
        Ok(object)
    }
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn presign_put_archive(
        &self,
        key: &str,
        content_length: u64,
        content_type: &str,
        expires_in: Duration,
    ) -> Result<PresignedPut, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::Presign {
            key: key.to_owned(),
            content_length,
            content_type: content_type.to_owned(),
        });
        Ok(PresignedPut {
            url: format!("{UPLOAD_BASE}{key}"),
            headers: BTreeMap::from([
                ("Content-Type".into(), content_type.into()),
                ("If-None-Match".into(), "*".into()),
            ]),
            expires_at: Utc::now()
                + ChronoDuration::from_std(expires_in).map_err(|_| BlobError::Integrity)?,
        })
    }

    async fn head_object(&self, key: &str) -> Result<ObjectHead, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::Head { key: key.to_owned() });
        state.objects.get(key).map(Self::object_head).ok_or(BlobError::NotFound)
    }

    async fn head_object_if_match(&self, key: &str, etag: &str) -> Result<ObjectHead, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::HeadIfMatch {
            key: key.to_owned(),
            etag: etag.to_owned(),
        });
        Self::matching(&state, key, etag).map(Self::object_head)
    }

    async fn get_range(&self, key: &str, etag: &str, range: Range<u64>) -> Result<Bytes, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::GetRange {
            key: key.to_owned(),
            etag: etag.to_owned(),
            range: range.clone(),
        });
        let object = Self::matching(&state, key, etag)?;
        let start = usize::try_from(range.start).map_err(|_| BlobError::Integrity)?;
        let end = usize::try_from(range.end).map_err(|_| BlobError::Integrity)?;
        if start > end || end > object.bytes.len() {
            return Err(BlobError::Integrity);
        }
        Ok(object.bytes.slice(start..end))
    }

    async fn delete_object(&self, key: &str) -> Result<(), BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::Delete { key: key.to_owned() });
        if state.delete_failures_remaining > 0 {
            state.delete_failures_remaining -= 1;
            return Err(BlobError::Provider("synthetic delete failure".into()));
        }
        state.objects.remove(key).map(|_| ()).ok_or(BlobError::NotFound)
    }
}
