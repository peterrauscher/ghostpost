use super::{deletion_marker_key,provider_receipt_key,signed_post::{sign_post,PostPolicyConfig,SigningCredentials},AccountDeletionMarker,BlobError,BlobStore,DeletionLedgerReceipt,ObjectHead,PresignedPost,ProviderDeletionReceipt};
use async_trait::async_trait;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::Client;
use bytes::Bytes;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ServerSideEncryption;
use chrono::{TimeZone,Utc};
use std::{ops::Range,time::Duration};

#[derive(Clone)]
pub struct S3BlobStore {
    client: Client,
    credentials: Option<SharedCredentialsProvider>,
    bucket: String,
    region: String,
    endpoint: String,
    require_kms: bool,
    kms_key_id: Option<String>,
}

impl S3BlobStore {
    /// Compatibility constructor for non-presigning uses. Call
    /// `new_with_credentials_provider` whenever signed POSTs are enabled.
    pub fn new(client:Client,bucket:String,region:String,endpoint:String,require_kms:bool,kms_key_id:Option<String>)->Self {
        Self { client, credentials: None, bucket, region, endpoint, require_kms, kms_key_id }
    }

    /// Constructs the adapter with the exact provider used to build the SDK
    /// config. The generated S3 `Config::credentials_provider()` accessor is
    /// permanently nonfunctional, so credentials must be retained explicitly.
    pub fn new_with_credentials_provider(
        client:Client,
        credentials:SharedCredentialsProvider,
        bucket:String,
        region:String,
        endpoint:String,
        require_kms:bool,
        kms_key_id:Option<String>,
    )->Self {
        Self { client, credentials: Some(credentials), bucket, region, endpoint, require_kms, kms_key_id }
    }

    fn head(&self,len:Option<i64>,ct:Option<&str>,version:Option<&str>,secs:Option<i64>)->Result<ObjectHead,BlobError>{
        let version_id=version.filter(|v|!v.is_empty()).ok_or(BlobError::Integrity)?.to_owned();
        Ok(ObjectHead{content_length:u64::try_from(len.ok_or(BlobError::Integrity)?).map_err(|_|BlobError::Integrity)?,content_type:ct.map(str::to_owned),version_id,last_modified:secs.and_then(|s|Utc.timestamp_opt(s,0).single())})
    }
}
fn map_err(e:impl std::fmt::Display)->BlobError{let s=e.to_string();if s.contains("NoSuchKey")||s.contains("NotFound"){BlobError::NotFound}else{BlobError::Provider(s)}}

#[async_trait]
impl BlobStore for S3BlobStore {
 async fn presign_post_archive(&self,key:&str,content_length:u64,content_type:&str,expires_in:Duration)->Result<PresignedPost,BlobError>{
  let provider=self.credentials.as_ref().ok_or_else(||BlobError::Config("AWS credentials provider was not injected alongside the S3 client".into()))?;
  let c=provider.provide_credentials().await.map_err(map_err)?;
  sign_post(&PostPolicyConfig{bucket:self.bucket.clone(),region:self.region.clone(),endpoint:self.endpoint.clone(),require_kms:self.require_kms,kms_key_id:self.kms_key_id.clone()},&SigningCredentials{access_key_id:c.access_key_id().into(),secret_access_key:c.secret_access_key().into(),session_token:c.session_token().map(str::to_owned)},key,content_length,content_type,expires_in,Utc::now())
 }
 async fn head_object(&self,key:&str)->Result<ObjectHead,BlobError>{let o=self.client.head_object().bucket(&self.bucket).key(key).send().await.map_err(map_err)?;self.head(o.content_length(),o.content_type(),o.version_id(),o.last_modified().map(|d|d.secs()))}
 async fn head_object_version(&self,key:&str,version_id:&str)->Result<ObjectHead,BlobError>{let o=self.client.head_object().bucket(&self.bucket).key(key).version_id(version_id).send().await.map_err(map_err)?;self.head(o.content_length(),o.content_type(),o.version_id(),o.last_modified().map(|d|d.secs()))}
 async fn get_range(&self,key:&str,version_id:&str,range:Range<u64>)->Result<Bytes,BlobError>{if range.start>=range.end{return Ok(Bytes::new())}let o=self.client.get_object().bucket(&self.bucket).key(key).version_id(version_id).range(format!("bytes={}-{}",range.start,range.end-1)).send().await.map_err(map_err)?;Ok(o.body.collect().await.map_err(map_err)?.into_bytes())}
 async fn delete_version(&self,key:&str,version_id:&str)->Result<(),BlobError>{self.client.delete_object().bucket(&self.bucket).key(key).version_id(version_id).send().await.map_err(map_err)?;Ok(())}
 async fn record_deletion_marker(&self,marker:&AccountDeletionMarker)->Result<DeletionLedgerReceipt,BlobError>{
  let key=deletion_marker_key(marker); let body=serde_json::to_vec(marker).map_err(map_err)?;
  match self.client.get_object().bucket(&self.bucket).key(&key).send().await {
   Ok(existing) => { let bytes=existing.body.collect().await.map_err(map_err)?.into_bytes(); if bytes.as_ref()!=body.as_slice(){return Err(BlobError::Integrity)} return Ok(DeletionLedgerReceipt{key}); }
   Err(error) => { let text=error.to_string(); if !(text.contains("NoSuchKey")||text.contains("NotFound")||text.contains("404")){return Err(map_err(error))} }
  }
  let mut put=self.client.put_object().bucket(&self.bucket).key(&key).content_type("application/json").body(ByteStream::from(body));
  if self.require_kms { put=put.server_side_encryption(ServerSideEncryption::AwsKms); if let Some(id)=&self.kms_key_id { put=put.ssekms_key_id(id); } }
  put.send().await.map_err(map_err)?; Ok(DeletionLedgerReceipt{key})
 }
 async fn record_provider_complete(&self,marker:&AccountDeletionMarker,receipt:&ProviderDeletionReceipt)->Result<(),BlobError>{
  let key=provider_receipt_key(marker); let body=serde_json::to_vec(receipt).map_err(map_err)?;
  match self.client.get_object().bucket(&self.bucket).key(&key).send().await {
   Ok(existing) => { let bytes=existing.body.collect().await.map_err(map_err)?.into_bytes(); if bytes.as_ref()!=body.as_slice(){return Err(BlobError::Integrity)} return Ok(()); }
   Err(error) => { let text=error.to_string(); if !(text.contains("NoSuchKey")||text.contains("NotFound")||text.contains("404")){return Err(map_err(error))} }
  }
  let mut put=self.client.put_object().bucket(&self.bucket).key(key).content_type("application/json").body(ByteStream::from(body));
  if self.require_kms { put=put.server_side_encryption(ServerSideEncryption::AwsKms); if let Some(id)=&self.kms_key_id { put=put.ssekms_key_id(id); } }
  put.send().await.map_err(map_err)?; Ok(())
 }
 async fn list_deletion_markers(&self,restore_point:chrono::DateTime<Utc>)->Result<Vec<AccountDeletionMarker>,BlobError>{
  let mut token=None; let mut markers=Vec::new();
  loop { let mut req=self.client.list_objects_v2().bucket(&self.bucket).prefix("deletion-ledger/v1/"); if let Some(t)=token { req=req.continuation_token(t); } let page=req.send().await.map_err(map_err)?;
   for object in page.contents() { let Some(key)=object.key() else {continue}; if key.ends_with(".provider-complete.json") {continue} let out=self.client.get_object().bucket(&self.bucket).key(key).send().await.map_err(map_err)?; let bytes=out.body.collect().await.map_err(map_err)?.into_bytes(); let marker:AccountDeletionMarker=serde_json::from_slice(&bytes).map_err(map_err)?; if deletion_marker_key(&marker)!=key {return Err(BlobError::Integrity)} if marker.deleted_at>restore_point {markers.push(marker)} }
   if page.is_truncated()!=Some(true) {break} token=page.next_continuation_token().map(str::to_owned); if token.is_none(){return Err(BlobError::Integrity)}
  }
  markers.sort_by_key(|m|m.deleted_at); Ok(markers)
 }
 async fn provider_complete(&self,marker:&AccountDeletionMarker)->Result<bool,BlobError>{ let key=provider_receipt_key(marker); match self.client.head_object().bucket(&self.bucket).key(key).send().await {Ok(_)=>Ok(true),Err(e)=>{let text=e.to_string();if text.contains("NoSuchKey")||text.contains("NotFound")||text.contains("404"){Ok(false)}else{Err(map_err(e))}}} }
}
