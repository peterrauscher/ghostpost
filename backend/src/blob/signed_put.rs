//! SigV4 query-string presigning for create-once archive uploads.
//!
//! The URL signs `content-length`, `content-type`, `host`, and
//! `if-none-match: *`, so the holder can upload exactly the reserved size and
//! type once. A replayed request is rejected by the store with 412.
use super::{BlobError, PresignedPut};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use url::Url;

type HmacSha256 = Hmac<Sha256>;

/// S3 SigV4 rejects presigned URLs valid for longer than seven days.
const MAX_EXPIRES_SECS: u64 = 604_800;

#[derive(Debug, Clone)]
pub struct SigningCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PutTarget {
    /// Base URL the object key is appended to: `https://bucket.host` for
    /// virtual-hosted style or `https://host/bucket` for path style.
    pub endpoint: String,
    pub region: String,
}

fn mac(key: &[u8], data: &str) -> Vec<u8> {
    let mut h = HmacSha256::new_from_slice(key).expect("HMAC accepts any key");
    h.update(data.as_bytes());
    h.finalize().into_bytes().to_vec()
}

/// RFC 3986 unreserved characters pass through; everything else is
/// percent-encoded. `/` survives only when encoding a path.
fn uri_encode(value: &str, keep_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b'/' if keep_slash => out.push('/'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn sign_put(
    target: &PutTarget,
    creds: &SigningCredentials,
    key: &str,
    content_length: u64,
    content_type: &str,
    ttl: Duration,
    now: DateTime<Utc>,
) -> Result<PresignedPut, BlobError> {
    if key.is_empty() || content_length == 0 || content_type.is_empty() {
        return Err(BlobError::Config("key, length, and content type are required".into()));
    }
    let expires_secs = ttl.as_secs();
    if expires_secs == 0 || expires_secs > MAX_EXPIRES_SECS {
        return Err(BlobError::Config("presign TTL must be between 1 second and 7 days".into()));
    }
    let base = Url::parse(&target.endpoint).map_err(|e| BlobError::Config(format!("invalid storage endpoint: {e}")))?;
    let host = match (base.host_str(), base.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        (None, _) => return Err(BlobError::Config("storage endpoint has no host".into())),
    };
    let canonical_uri = format!("{}/{}", base.path().trim_end_matches('/'), uri_encode(key, true));

    let date = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let scope = format!("{date}/{}/s3/aws4_request", target.region);
    let signed_headers = "content-length;content-type;host;if-none-match";

    let mut query = BTreeMap::from([
        ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential".to_owned(), format!("{}/{scope}", creds.access_key_id)),
        ("X-Amz-Date".to_owned(), amz_date.clone()),
        ("X-Amz-Expires".to_owned(), expires_secs.to_string()),
        ("X-Amz-SignedHeaders".to_owned(), signed_headers.to_owned()),
    ]);
    if let Some(token) = &creds.session_token {
        query.insert("X-Amz-Security-Token".to_owned(), token.clone());
    }
    let canonical_query = query
        .iter()
        .map(|(k, v)| format!("{}={}", uri_encode(k, false), uri_encode(v, false)))
        .collect::<Vec<_>>()
        .join("&");
    let canonical_headers = format!(
        "content-length:{content_length}\ncontent-type:{}\nhost:{host}\nif-none-match:*\n",
        content_type.trim()
    );
    let canonical_request =
        format!("PUT\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\nUNSIGNED-PAYLOAD");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let k_date = mac(format!("AWS4{}", creds.secret_access_key).as_bytes(), &date);
    let k_region = mac(&k_date, &target.region);
    let k_service = mac(&k_region, "s3");
    let k_signing = mac(&k_service, "aws4_request");
    let signature = hex::encode(mac(&k_signing, &string_to_sign));

    let mut url = base;
    url.set_path(&canonical_uri);
    url.set_query(Some(&format!("{canonical_query}&X-Amz-Signature={signature}")));
    let expires_at = now + chrono::Duration::seconds(expires_secs as i64);
    Ok(PresignedPut {
        url: url.into(),
        headers: BTreeMap::from([
            ("Content-Type".to_owned(), content_type.to_owned()),
            ("If-None-Match".to_owned(), "*".to_owned()),
        ]),
        expires_at,
    })
}
