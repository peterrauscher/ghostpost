use super::{deletion_marker_key,provider_receipt_key,signed_put::{sign_put,PutTarget,SigningCredentials},AccountDeletionMarker,BlobError,BlobStore,DeletionLedgerReceipt,ObjectHead,PresignedPut,ProviderDeletionReceipt};
use async_trait::async_trait;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use aws_smithy_runtime_api::client::orchestrator::HttpResponse;
use bytes::Bytes;
use chrono::{TimeZone,Utc};
use std::{ops::Range,time::Duration};

/// S3-compatible archive store (Cloudflare R2 in production, MinIO locally).
/// Relies only on create-once conditional PUT, `If-Match` reads, and plain
/// key deletes, which R2, MinIO, and AWS S3 all support. No bucket versioning.
#[derive(Clone)]
pub struct S3BlobStore {
    client: Client,
    credentials: SharedCredentialsProvider,
    bucket: String,
    region: String,
    endpoint: String,
}

fn status<E>(err:&SdkError<E,HttpResponse>)->Option<u16>{ err.raw_response().map(|r|r.status().as_u16()) }
fn classify<E:std::error::Error+'static>(err:SdkError<E,HttpResponse>)->BlobError{
    match status(&err) {
        Some(404) => BlobError::NotFound,
        Some(412) => BlobError::Integrity,
        _ => BlobError::Provider(aws_sdk_s3::error::DisplayErrorContext(&err).to_string()),
    }
}
fn provider(e:impl std::fmt::Display)->BlobError{BlobError::Provider(e.to_string())}

fn is_object_locked<E:aws_sdk_s3::error::ProvideErrorMetadata>(err:&SdkError<E,HttpResponse>)->bool{
    status(err)==Some(409) && err.as_service_error().and_then(|e|e.code())==Some("ObjectLockedByBucketPolicy")
}

impl S3BlobStore {
    /// `credentials` must be the provider the SDK client was built with: the
    /// generated `Config::credentials_provider()` accessor does not return it,
    /// and presigning needs the raw keys.
    pub fn new(client:Client,credentials:SharedCredentialsProvider,bucket:String,region:String,endpoint:String)->Self {
        Self { client, credentials, bucket, region, endpoint }
    }

    /// Builds the store from `ARCHIVE_BUCKET`, `ARCHIVE_S3_REGION`, optional
    /// `ARCHIVE_S3_ENDPOINT` (server-side), optional `ARCHIVE_S3_PUBLIC_ENDPOINT`
    /// (host browsers upload to), `ARCHIVE_S3_FORCE_PATH_STYLE`, and the
    /// standard AWS credential chain.
    pub async fn from_env()->Result<Self,BlobError>{
        let var=|name:&str|std::env::var(name).map_err(|_|BlobError::Config(format!("missing required env {name}")));
        let bucket=var("ARCHIVE_BUCKET")?;
        let region=var("ARCHIVE_S3_REGION")?;
        let force_path_style=std::env::var("ARCHIVE_S3_FORCE_PATH_STYLE").map(|v|matches!(v.as_str(),"1"|"true"|"yes")).unwrap_or(false);
        let endpoint_override=std::env::var("ARCHIVE_S3_ENDPOINT").ok();
        let sdk=aws_config::defaults(aws_config::BehaviorVersion::latest()).region(aws_sdk_s3::config::Region::new(region.clone())).load().await;
        let credentials=sdk.credentials_provider().ok_or_else(||BlobError::Config("AWS credentials provider missing from resolved config".into()))?;
        let mut builder=aws_sdk_s3::config::Builder::from(&sdk).force_path_style(force_path_style);
        if let Some(endpoint)=endpoint_override.as_deref() { builder=builder.endpoint_url(endpoint); }
        let public=std::env::var("ARCHIVE_S3_PUBLIC_ENDPOINT").ok().or(endpoint_override);
        let upload_base=match public {
            Some(base) if force_path_style => format!("{}/{}",base.trim_end_matches('/'),bucket),
            Some(base) => {
                let mut url=url::Url::parse(&base).map_err(|e|BlobError::Config(format!("invalid ARCHIVE_S3 endpoint: {e}")))?;
                let host=url.host_str().ok_or_else(||BlobError::Config("ARCHIVE_S3 endpoint has no host".into()))?.to_owned();
                url.set_host(Some(&format!("{bucket}.{host}"))).map_err(|e|BlobError::Config(e.to_string()))?;
                url.to_string().trim_end_matches('/').to_owned()
            }
            None => format!("https://{bucket}.s3.{region}.amazonaws.com"),
        };
        Ok(Self::new(Client::from_conf(builder.build()),credentials,bucket,region,upload_base))
    }

    fn head(len:Option<i64>,ct:Option<&str>,etag:Option<&str>,secs:Option<i64>)->Result<ObjectHead,BlobError>{
        let etag=etag.filter(|v|!v.is_empty()).ok_or(BlobError::Integrity)?.to_owned();
        Ok(ObjectHead{content_length:u64::try_from(len.ok_or(BlobError::Integrity)?).map_err(|_|BlobError::Integrity)?,content_type:ct.map(str::to_owned),etag,last_modified:secs.and_then(|s|Utc.timestamp_opt(s,0).single())})
    }

    /// Writes `body` only if `key` is absent. If it already exists, succeeds
    /// only when the stored bytes are identical (idempotent retry).
    ///
    /// R2 evaluates bucket-lock rules before preconditions, so overwriting an
    /// existing key under the locked `deletion-ledger/` prefix returns
    /// 409 `ObjectLockedByBucketPolicy` instead of 412; both mean "exists".
    async fn put_create_once(&self,key:&str,body:Vec<u8>)->Result<(),BlobError>{
        let put=self.client.put_object().bucket(&self.bucket).key(key).content_type("application/json").body(ByteStream::from(body.clone()))
            .customize()
            .mutate_request(|req| { req.headers_mut().insert("if-none-match","*"); })
            .send().await;
        match put {
            Ok(_) => Ok(()),
            Err(err) if status(&err)==Some(412) || is_object_locked(&err) => {
                let existing=self.client.get_object().bucket(&self.bucket).key(key).send().await.map_err(classify)?;
                let bytes=existing.body.collect().await.map_err(provider)?.into_bytes();
                if bytes.as_ref()==body.as_slice() { Ok(()) } else { Err(BlobError::Integrity) }
            }
            Err(err) => Err(classify(err)),
        }
    }
}

#[async_trait]
impl BlobStore for S3BlobStore {
 async fn presign_put_archive(&self,key:&str,content_length:u64,content_type:&str,expires_in:Duration)->Result<PresignedPut,BlobError>{
  let c=self.credentials.provide_credentials().await.map_err(provider)?;
  sign_put(&PutTarget{endpoint:self.endpoint.clone(),region:self.region.clone()},&SigningCredentials{access_key_id:c.access_key_id().into(),secret_access_key:c.secret_access_key().into(),session_token:c.session_token().map(str::to_owned)},key,content_length,content_type,expires_in,Utc::now())
 }
 async fn head_object(&self,key:&str)->Result<ObjectHead,BlobError>{let o=self.client.head_object().bucket(&self.bucket).key(key).send().await.map_err(classify)?;Self::head(o.content_length(),o.content_type(),o.e_tag(),o.last_modified().map(|d|d.secs()))}
 async fn head_object_if_match(&self,key:&str,etag:&str)->Result<ObjectHead,BlobError>{let o=self.client.head_object().bucket(&self.bucket).key(key).if_match(etag).send().await.map_err(classify)?;Self::head(o.content_length(),o.content_type(),o.e_tag(),o.last_modified().map(|d|d.secs()))}
 async fn get_range(&self,key:&str,etag:&str,range:Range<u64>)->Result<Bytes,BlobError>{if range.start>=range.end{return Ok(Bytes::new())}let o=self.client.get_object().bucket(&self.bucket).key(key).if_match(etag).range(format!("bytes={}-{}",range.start,range.end-1)).send().await.map_err(classify)?;Ok(o.body.collect().await.map_err(provider)?.into_bytes())}
 async fn delete_object(&self,key:&str)->Result<(),BlobError>{self.client.delete_object().bucket(&self.bucket).key(key).send().await.map_err(classify)?;Ok(())}
 async fn record_deletion_marker(&self,marker:&AccountDeletionMarker)->Result<DeletionLedgerReceipt,BlobError>{
  let key=deletion_marker_key(marker); self.put_create_once(&key,serde_json::to_vec(marker).map_err(provider)?).await?; Ok(DeletionLedgerReceipt{key})
 }
 async fn record_provider_complete(&self,marker:&AccountDeletionMarker,receipt:&ProviderDeletionReceipt)->Result<(),BlobError>{
  self.put_create_once(&provider_receipt_key(marker),serde_json::to_vec(receipt).map_err(provider)?).await
 }
 async fn list_deletion_markers(&self,restore_point:chrono::DateTime<Utc>)->Result<Vec<AccountDeletionMarker>,BlobError>{
  let mut token=None; let mut markers=Vec::new();
  loop { let mut req=self.client.list_objects_v2().bucket(&self.bucket).prefix("deletion-ledger/v1/"); if let Some(t)=token { req=req.continuation_token(t); } let page=req.send().await.map_err(classify)?;
   for object in page.contents() { let Some(key)=object.key() else {continue}; if key.ends_with(".provider-complete.json") {continue} let out=self.client.get_object().bucket(&self.bucket).key(key).send().await.map_err(classify)?; let bytes=out.body.collect().await.map_err(provider)?.into_bytes(); let marker:AccountDeletionMarker=serde_json::from_slice(&bytes).map_err(provider)?; if deletion_marker_key(&marker)!=key {return Err(BlobError::Integrity)} if marker.deleted_at>restore_point {markers.push(marker)} }
   if page.is_truncated()!=Some(true) {break} token=page.next_continuation_token().map(str::to_owned); if token.is_none(){return Err(BlobError::Integrity)}
  }
  markers.sort_by_key(|m|m.deleted_at); Ok(markers)
 }
 async fn provider_complete(&self,marker:&AccountDeletionMarker)->Result<bool,BlobError>{ match self.client.head_object().bucket(&self.bucket).key(provider_receipt_key(marker)).send().await {Ok(_)=>Ok(true),Err(e)=>match classify(e){BlobError::NotFound=>Ok(false),other=>Err(other)}} }
}
