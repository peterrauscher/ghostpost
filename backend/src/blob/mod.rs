use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::Range, time::Duration};
use uuid::Uuid;

pub mod s3;
pub mod signed_post;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresignedPost { pub url: String, pub fields: BTreeMap<String, String>, pub expires_at: DateTime<Utc> }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectHead { pub content_length: u64, pub content_type: Option<String>, pub version_id: String, pub last_modified: Option<DateTime<Utc>> }
#[derive(Debug, thiserror::Error)]
pub enum BlobError { #[error("object not found")] NotFound, #[error("invalid storage configuration: {0}")] Config(String), #[error("storage integrity failure")] Integrity, #[error("storage operation failed: {0}")] Provider(String) }

#[async_trait]
pub trait BlobStore: Send + Sync {
 async fn presign_post_archive(&self,key:&str,content_length:u64,content_type:&str,expires_in:Duration)->Result<PresignedPost,BlobError>;
 async fn head_object(&self,key:&str)->Result<ObjectHead,BlobError>;
 async fn head_object_version(&self,key:&str,version_id:&str)->Result<ObjectHead,BlobError>;
 async fn get_range(&self,key:&str,version_id:&str,range:Range<u64>)->Result<Bytes,BlobError>;
 async fn delete_version(&self,key:&str,version_id:&str)->Result<(),BlobError>;
 async fn record_deletion_marker(&self,_:&AccountDeletionMarker)->Result<DeletionLedgerReceipt,BlobError>{Err(BlobError::Config("deletion ledger unavailable".into()))}
 async fn record_provider_complete(&self,_:&AccountDeletionMarker,_:&ProviderDeletionReceipt)->Result<(),BlobError>{Err(BlobError::Config("deletion ledger unavailable".into()))}
 async fn list_deletion_markers(&self,_:DateTime<Utc>)->Result<Vec<AccountDeletionMarker>,BlobError>{Err(BlobError::Config("deletion ledger unavailable".into()))}
 async fn provider_complete(&self,_:&AccountDeletionMarker)->Result<bool,BlobError>{Err(BlobError::Config("deletion ledger unavailable".into()))}
}

/// Fail-closed default used by routers that do not inject object storage.
/// Production must call `build_with_blob_store`; tests may inject a deterministic seam.
#[derive(Debug, Default)]
pub struct UnavailableBlobStore;

#[async_trait]
impl BlobStore for UnavailableBlobStore {
 async fn presign_post_archive(&self,_:&str,_:u64,_:&str,_:Duration)->Result<PresignedPost,BlobError>{Err(BlobError::Config("archive blob store not configured".into()))}
 async fn head_object(&self,_:&str)->Result<ObjectHead,BlobError>{Err(BlobError::NotFound)}
 async fn head_object_version(&self,_:&str,_:&str)->Result<ObjectHead,BlobError>{Err(BlobError::NotFound)}
 async fn get_range(&self,_:&str,_:&str,_:Range<u64>)->Result<Bytes,BlobError>{Err(BlobError::NotFound)}
 async fn delete_version(&self,_:&str,_:&str)->Result<(),BlobError>{Err(BlobError::NotFound)}
}

#[derive(Debug, Default)]
pub struct DeterministicBlobStore;

#[async_trait]
impl BlobStore for DeterministicBlobStore {
 async fn presign_post_archive(&self,key:&str,length:u64,content_type:&str,ttl:Duration)->Result<PresignedPost,BlobError>{
  signed_post::sign_post(
   &signed_post::PostPolicyConfig{bucket:"ghostpost-archives".into(),region:"us-east-1".into(),endpoint:"https://ghostpost-archives.s3.us-east-1.amazonaws.com".into(),require_kms:false,kms_key_id:None},
   &signed_post::SigningCredentials{access_key_id:"TESTACCESS".into(),secret_access_key:"test-secret-not-for-production".into(),session_token:None},
   key,length,content_type,ttl,Utc::now())
 }
 async fn head_object(&self,_:&str)->Result<ObjectHead,BlobError>{Err(BlobError::NotFound)}
 async fn head_object_version(&self,_:&str,_:&str)->Result<ObjectHead,BlobError>{Err(BlobError::NotFound)}
 async fn get_range(&self,_:&str,_:&str,_:Range<u64>)->Result<Bytes,BlobError>{Err(BlobError::NotFound)}
 async fn delete_version(&self,_:&str,_:&str)->Result<(),BlobError>{Err(BlobError::NotFound)}
 async fn record_deletion_marker(&self,marker:&AccountDeletionMarker)->Result<DeletionLedgerReceipt,BlobError>{Ok(DeletionLedgerReceipt{key:deletion_marker_key(marker)})}
 async fn record_provider_complete(&self,_:&AccountDeletionMarker,_:&ProviderDeletionReceipt)->Result<(),BlobError>{Ok(())}
 async fn list_deletion_markers(&self,_:DateTime<Utc>)->Result<Vec<AccountDeletionMarker>,BlobError>{Ok(Vec::new())}
 async fn provider_complete(&self,_:&AccountDeletionMarker)->Result<bool,BlobError>{Ok(false)}
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] #[serde(rename_all="camelCase")] pub struct AccountDeletionMarker { pub schema_version:u16,pub tenant_id:Uuid,pub user_id:Uuid,pub deleted_at:DateTime<Utc> }
#[derive(Debug,Clone,PartialEq,Eq)] pub struct DeletionLedgerReceipt { pub key:String }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] #[serde(rename_all="camelCase")] pub struct ProviderDeletionReceipt { pub schema_version:u16,pub tenant_id:Uuid,pub user_id:Uuid,pub completed_at:DateTime<Utc> }

pub fn deletion_marker_key(marker:&AccountDeletionMarker)->String { format!("deletion-ledger/v1/{}/{:02}/{:02}/{}-{}-{}.json",marker.deleted_at.format("%Y"),marker.deleted_at.format("%m"),marker.deleted_at.format("%d"),marker.deleted_at.timestamp_millis(),marker.tenant_id,marker.user_id) }
pub fn provider_receipt_key(marker:&AccountDeletionMarker)->String { let base=deletion_marker_key(marker); let dir=base.rsplit_once('/').map(|(d,_)|d).unwrap_or("deletion-ledger/v1"); format!("{dir}/{}.provider-complete.json",marker.user_id) }

#[cfg(test)]
mod deletion_ledger_tests {
 use super::*;
 use chrono::TimeZone;
 #[test]
 fn marker_schema_and_keys_are_stable() {
  let marker=AccountDeletionMarker{schema_version:1,tenant_id:Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap(),user_id:Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap(),deleted_at:Utc.with_ymd_and_hms(2026,7,27,12,34,56).unwrap()};
  assert_eq!(serde_json::to_value(&marker).unwrap(),serde_json::json!({"schemaVersion":1,"tenantId":"11111111-1111-1111-1111-111111111111","userId":"22222222-2222-2222-2222-222222222222","deletedAt":"2026-07-27T12:34:56Z"}));
  assert_eq!(deletion_marker_key(&marker),"deletion-ledger/v1/2026/07/27/1785155696000-11111111-1111-1111-1111-111111111111-22222222-2222-2222-2222-222222222222.json");
  assert_eq!(provider_receipt_key(&marker),"deletion-ledger/v1/2026/07/27/22222222-2222-2222-2222-222222222222.provider-complete.json");
 }
}
