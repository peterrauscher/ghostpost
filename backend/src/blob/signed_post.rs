use super::{BlobError,PresignedPost};
use base64::{engine::general_purpose::STANDARD,Engine};
use chrono::{DateTime,SecondsFormat,Utc};
use hmac::{Hmac,Mac};
use serde_json::json;
use sha2::Sha256;
use std::{collections::BTreeMap,time::Duration};

type HmacSha256=Hmac<Sha256>;
#[derive(Debug,Clone)] pub struct SigningCredentials { pub access_key_id:String,pub secret_access_key:String,pub session_token:Option<String> }
#[derive(Debug,Clone)] pub struct PostPolicyConfig { pub bucket:String,pub region:String,pub endpoint:String,pub require_kms:bool,pub kms_key_id:Option<String> }
fn mac(key:&[u8],data:&str)->Vec<u8>{let mut h=HmacSha256::new_from_slice(key).expect("HMAC accepts any key");h.update(data.as_bytes());h.finalize().into_bytes().to_vec()}
fn hex(bytes:&[u8])->String{bytes.iter().map(|b|format!("{b:02x}")).collect()}
pub fn sign_post(cfg:&PostPolicyConfig,creds:&SigningCredentials,key:&str,length:u64,content_type:&str,ttl:Duration,now:DateTime<Utc>)->Result<PresignedPost,BlobError>{
 if key.is_empty()||length==0{return Err(BlobError::Config("key and length are required".into()))}
 let expires_at=now+chrono::Duration::from_std(ttl).map_err(|_|BlobError::Config("invalid TTL".into()))?;
 let date=now.format("%Y%m%d").to_string();let amz_date=now.format("%Y%m%dT%H%M%SZ").to_string();let credential=format!("{}/{}/{}/s3/aws4_request",creds.access_key_id,date,cfg.region);
 let mut fields=BTreeMap::from([("key".into(),key.into()),("Content-Type".into(),content_type.into()),("success_action_status".into(),"204".into()),("x-amz-algorithm".into(),"AWS4-HMAC-SHA256".into()),("x-amz-credential".into(),credential.clone()),("x-amz-date".into(),amz_date.clone())]);
 let mut conditions=vec![json!({"bucket":cfg.bucket}),json!({"key":key}),json!({"Content-Type":content_type}),json!({"success_action_status":"204"}),json!({"x-amz-algorithm":"AWS4-HMAC-SHA256"}),json!({"x-amz-credential":credential}),json!({"x-amz-date":amz_date}),json!(["content-length-range",1,length])];
 if let Some(token)=&creds.session_token{fields.insert("x-amz-security-token".into(),token.clone());conditions.push(json!({"x-amz-security-token":token}));}
 if cfg.require_kms { fields.insert("x-amz-server-side-encryption".into(),"aws:kms".into());conditions.push(json!({"x-amz-server-side-encryption":"aws:kms"}));if let Some(id)=&cfg.kms_key_id{fields.insert("x-amz-server-side-encryption-aws-kms-key-id".into(),id.clone());conditions.push(json!({"x-amz-server-side-encryption-aws-kms-key-id":id}));}}
 let policy=STANDARD.encode(serde_json::to_vec(&json!({"expiration":expires_at.to_rfc3339_opts(SecondsFormat::Secs,true),"conditions":conditions})).map_err(|e|BlobError::Provider(e.to_string()))?);
 let k_date=mac(format!("AWS4{}",creds.secret_access_key).as_bytes(),&date);let k_region=mac(&k_date,&cfg.region);let k_service=mac(&k_region,"s3");let k_signing=mac(&k_service,"aws4_request");let signature=hex(&mac(&k_signing,&policy));fields.insert("policy".into(),policy);fields.insert("x-amz-signature".into(),signature);
 Ok(PresignedPost{url:cfg.endpoint.clone(),fields,expires_at})
}
