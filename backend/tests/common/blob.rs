use async_trait::async_trait;
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use ghostpost_backend::blob::{BlobError, BlobStore, ObjectHead, PresignedPost};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::Mutex;
use std::time::Duration;

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
    HeadVersion {
        key: String,
        version_id: String,
    },
    GetRange {
        key: String,
        version_id: String,
        range: Range<u64>,
    },
    DeleteVersion {
        key: String,
        version_id: String,
    },
}

#[derive(Debug, Clone)]
struct StoredObject {
    content_type: String,
    bytes: Bytes,
}

#[derive(Debug, Default)]
struct State {
    objects: HashMap<(String, String), StoredObject>,
    latest: HashMap<String, String>,
    calls: Vec<BlobCall>,
    delete_failures_remaining: usize,
}

#[derive(Debug, Default)]
pub struct InMemoryBlobStore {
    state: Mutex<State>,
}

impl InMemoryBlobStore {
    pub fn put_latest(
        &self,
        key: impl Into<String>,
        version_id: impl Into<String>,
        content_type: impl Into<String>,
        bytes: impl Into<Bytes>,
    ) {
        let key = key.into();
        let version_id = version_id.into();
        let object = StoredObject {
            content_type: content_type.into(),
            bytes: bytes.into(),
        };
        let mut state = self.state.lock().expect("blob fixture mutex");
        state
            .objects
            .insert((key.clone(), version_id.clone()), object);
        state.latest.insert(key, version_id);
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

    fn object_head(version_id: String, object: &StoredObject) -> ObjectHead {
        ObjectHead {
            content_length: object.bytes.len() as u64,
            content_type: Some(object.content_type.clone()),
            version_id,
            last_modified: None,
        }
    }
}

#[async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn presign_post_archive(
        &self,
        key: &str,
        content_length: u64,
        content_type: &str,
        expires_in: Duration,
    ) -> Result<PresignedPost, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::Presign {
            key: key.to_owned(),
            content_length,
            content_type: content_type.to_owned(),
        });
        let mut fields = BTreeMap::new();
        fields.insert("key".into(), key.into());
        fields.insert("Content-Type".into(), content_type.into());
        fields.insert("success_action_status".into(), "204".into());
        Ok(PresignedPost {
            url: "https://archive-upload.test".into(),
            fields,
            expires_at: Utc::now()
                + ChronoDuration::from_std(expires_in).map_err(|_| BlobError::Integrity)?,
        })
    }

    async fn head_object(&self, key: &str) -> Result<ObjectHead, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::Head {
            key: key.to_owned(),
        });
        let version_id = state.latest.get(key).cloned().ok_or(BlobError::NotFound)?;
        let object = state
            .objects
            .get(&(key.to_owned(), version_id.clone()))
            .ok_or(BlobError::NotFound)?;
        Ok(Self::object_head(version_id, object))
    }

    async fn head_object_version(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<ObjectHead, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::HeadVersion {
            key: key.to_owned(),
            version_id: version_id.to_owned(),
        });
        let object = state
            .objects
            .get(&(key.to_owned(), version_id.to_owned()))
            .ok_or(BlobError::NotFound)?;
        Ok(Self::object_head(version_id.to_owned(), object))
    }

    async fn get_range(
        &self,
        key: &str,
        version_id: &str,
        range: Range<u64>,
    ) -> Result<Bytes, BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::GetRange {
            key: key.to_owned(),
            version_id: version_id.to_owned(),
            range: range.clone(),
        });
        let object = state
            .objects
            .get(&(key.to_owned(), version_id.to_owned()))
            .ok_or(BlobError::NotFound)?;
        let start = usize::try_from(range.start).map_err(|_| BlobError::Integrity)?;
        let end = usize::try_from(range.end).map_err(|_| BlobError::Integrity)?;
        if start > end || end > object.bytes.len() {
            return Err(BlobError::Integrity);
        }
        Ok(object.bytes.slice(start..end))
    }

    async fn delete_version(&self, key: &str, version_id: &str) -> Result<(), BlobError> {
        let mut state = self.state.lock().expect("blob fixture mutex");
        state.calls.push(BlobCall::DeleteVersion {
            key: key.to_owned(),
            version_id: version_id.to_owned(),
        });
        if state.delete_failures_remaining > 0 {
            state.delete_failures_remaining -= 1;
            return Err(BlobError::Provider("synthetic delete failure".into()));
        }
        state
            .objects
            .remove(&(key.to_owned(), version_id.to_owned()))
            .ok_or(BlobError::NotFound)?;
        if state.latest.get(key).map(String::as_str) == Some(version_id) {
            state.latest.remove(key);
        }
        Ok(())
    }
}
