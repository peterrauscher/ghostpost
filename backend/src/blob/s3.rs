use super::{signed_post::{sign_post,PostPolicyConfig,SigningCredentials},BlobError,BlobStore,ObjectHead,PresignedPost};
use async_trait::async_trait;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::Client;
use bytes::Bytes;
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
}
