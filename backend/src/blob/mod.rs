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
}

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct AccountDeletionMarker { pub schema_version:u16,pub tenant_id:Uuid,pub user_id:Uuid,pub deleted_at:DateTime<Utc> }
#[derive(Debug,Clone,PartialEq,Eq)] pub struct DeletionLedgerReceipt { pub key:String }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)] pub struct ProviderDeletionReceipt { pub marker:AccountDeletionMarker,pub completed_at:DateTime<Utc> }
#[async_trait] pub trait DeletionLedgerStore:Send+Sync { async fn record_if_absent(&self,marker:&AccountDeletionMarker)->Result<DeletionLedgerReceipt,BlobError>; async fn record_provider_complete(&self,receipt:&ProviderDeletionReceipt)->Result<(),BlobError>; }
#[async_trait] pub trait DeletionLedgerReader:Send+Sync { async fn list_since(&self,restore_point:DateTime<Utc>)->Result<Vec<AccountDeletionMarker>,BlobError>; async fn provider_complete(&self,marker:&AccountDeletionMarker)->Result<bool,BlobError>; }
