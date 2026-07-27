use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{TimeZone, Utc};
use ghostpost_backend::blob::signed_post::{
    sign_post, PostPolicyConfig, SigningCredentials,
};
use aws_config::BehaviorVersion;
use aws_credential_types::provider::SharedCredentialsProvider;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::Region;
use ghostpost_backend::blob::s3::S3BlobStore;
use ghostpost_backend::blob::BlobStore;
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::{collections::BTreeMap, time::Duration};

type HmacSha256 = Hmac<Sha256>;

const ACCESS_KEY: &str = "AKIDEXAMPLE";
const SECRET_KEY: &str = "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY";
const SESSION_TOKEN: &str = "synthetic-session-token";
const KEY: &str = "archive-imports/00000000-0000-0000-0000-000000000001/source.zip";
const CONTENT_TYPE: &str = "application/zip";
const RESERVED_BYTES: u64 = 12_345_678;

fn hmac(key: &[u8], value: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts the test key");
    mac.update(value.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn signature(secret: &str, date: &str, region: &str, policy: &str) -> String {
    let date_key = hmac(format!("AWS4{secret}").as_bytes(), date);
    let region_key = hmac(&date_key, region);
    let service_key = hmac(&region_key, "s3");
    let signing_key = hmac(&service_key, "aws4_request");
    hmac(&signing_key, policy)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn field<'a>(fields: &'a BTreeMap<String, String>, name: &str) -> &'a str {
    fields.get(name).unwrap_or_else(|| panic!("missing signed POST field {name}"))
}

fn condition_matches(fields: &BTreeMap<String, String>, condition: &Value) -> bool {
    if let Some(map) = condition.as_object() {
        return map.iter().all(|(name, expected)| {
            if name == "bucket" {
                return expected.as_str() == Some("ghostpost-archives");
            }
            fields.get(name).map(String::as_str) == expected.as_str()
        });
    }
    true
}

fn independently_accepts(
    fields: &BTreeMap<String, String>,
    submitted_file_bytes: u64,
    secret: &str,
    region: &str,
) -> bool {
    let Some(encoded_policy) = fields.get("policy") else {
        return false;
    };
    let Some(actual_signature) = fields.get("x-amz-signature") else {
        return false;
    };
    let Ok(policy_bytes) = STANDARD.decode(encoded_policy) else {
        return false;
    };
    let Ok(policy) = serde_json::from_slice::<Value>(&policy_bytes) else {
        return false;
    };
    let Some(conditions) = policy.get("conditions").and_then(Value::as_array) else {
        return false;
    };
    if !conditions.iter().all(|condition| condition_matches(fields, condition)) {
        return false;
    }
    let Some(length_condition) = conditions.iter().find_map(|condition| {
        let values = condition.as_array()?;
        (values.first()?.as_str()? == "content-length-range").then_some(values)
    }) else {
        return false;
    };
    let Some(minimum) = length_condition.get(1).and_then(Value::as_u64) else {
        return false;
    };
    let Some(maximum) = length_condition.get(2).and_then(Value::as_u64) else {
        return false;
    };
    if !(minimum..=maximum).contains(&submitted_file_bytes) {
        return false;
    }
    let Some(date) = fields.get("x-amz-date").and_then(|date| date.get(..8)) else {
        return false;
    };
    signature(secret, date, region, encoded_policy) == *actual_signature
}

fn signed_post() -> ghostpost_backend::blob::PresignedPost {
    sign_post(
        &PostPolicyConfig {
            bucket: "ghostpost-archives".into(),
            region: "us-east-1".into(),
            endpoint: "https://ghostpost-archives.s3.us-east-1.amazonaws.com".into(),
            require_kms: true,
            kms_key_id: Some("arn:aws:kms:us-east-1:111122223333:key/synthetic-test-key".into()),
        },
        &SigningCredentials {
            access_key_id: ACCESS_KEY.into(),
            secret_access_key: SECRET_KEY.into(),
            session_token: Some(SESSION_TOKEN.into()),
        },
        KEY,
        RESERVED_BYTES,
        CONTENT_TYPE,
        Duration::from_secs(900),
        Utc.with_ymd_and_hms(2026, 7, 23, 12, 0, 0).unwrap(),
    )
    .expect("fixed signed POST vector")
}

#[test]
fn signed_post_policy_matches_fixed_sigv4_golden_vector() {
    let post = signed_post();

    assert_eq!(
        post.url,
        "https://ghostpost-archives.s3.us-east-1.amazonaws.com"
    );
    assert_eq!(
        post.expires_at,
        Utc.with_ymd_and_hms(2026, 7, 23, 12, 15, 0).unwrap()
    );
    assert_eq!(post.fields.len(), 11, "the multipart form has no unsigned extras");
    assert_eq!(field(&post.fields, "key"), KEY);
    assert_eq!(field(&post.fields, "Content-Type"), CONTENT_TYPE);
    assert_eq!(field(&post.fields, "success_action_status"), "204");
    assert_eq!(field(&post.fields, "x-amz-algorithm"), "AWS4-HMAC-SHA256");
    assert_eq!(field(&post.fields, "x-amz-date"), "20260723T120000Z");
    assert_eq!(
        field(&post.fields, "x-amz-credential"),
        "AKIDEXAMPLE/20260723/us-east-1/s3/aws4_request"
    );
    assert_eq!(field(&post.fields, "x-amz-security-token"), SESSION_TOKEN);
    assert_eq!(field(&post.fields, "x-amz-server-side-encryption"), "aws:kms");
    assert_eq!(
        field(
            &post.fields,
            "x-amz-server-side-encryption-aws-kms-key-id"
        ),
        "arn:aws:kms:us-east-1:111122223333:key/synthetic-test-key"
    );

    let decoded: Value = serde_json::from_slice(
        &STANDARD
            .decode(field(&post.fields, "policy"))
            .expect("base64 policy"),
    )
    .expect("JSON policy");
    assert_eq!(
        decoded,
        serde_json::json!({
            "expiration": "2026-07-23T12:15:00Z",
            "conditions": [
                {"bucket": "ghostpost-archives"},
                {"key": KEY},
                {"Content-Type": CONTENT_TYPE},
                {"success_action_status": "204"},
                {"x-amz-algorithm": "AWS4-HMAC-SHA256"},
                {"x-amz-credential": "AKIDEXAMPLE/20260723/us-east-1/s3/aws4_request"},
                {"x-amz-date": "20260723T120000Z"},
                ["content-length-range", 1, RESERVED_BYTES],
                {"x-amz-security-token": SESSION_TOKEN},
                {"x-amz-server-side-encryption": "aws:kms"},
                {"x-amz-server-side-encryption-aws-kms-key-id": "arn:aws:kms:us-east-1:111122223333:key/synthetic-test-key"}
            ]
        })
    );

    let expected_signature = signature(
        SECRET_KEY,
        "20260723",
        "us-east-1",
        field(&post.fields, "policy"),
    );
    assert_eq!(field(&post.fields, "x-amz-signature"), expected_signature);
    assert_eq!(
        field(&post.fields, "x-amz-signature"),
        "ae212043c5f2704cdb6d0dc67dcacff27d9b4ec2fe390b163324a38bfd3f9370"
    );
}

#[test]
fn signed_post_policy_rejects_field_tampering_and_length_boundaries() {
    let post = signed_post();
    assert!(independently_accepts(
        &post.fields,
        RESERVED_BYTES,
        SECRET_KEY,
        "us-east-1"
    ));
    assert!(independently_accepts(
        &post.fields,
        1,
        SECRET_KEY,
        "us-east-1"
    ));
    assert!(!independently_accepts(
        &post.fields,
        0,
        SECRET_KEY,
        "us-east-1"
    ));
    assert!(!independently_accepts(
        &post.fields,
        RESERVED_BYTES + 1,
        SECRET_KEY,
        "us-east-1"
    ));

    for (field_name, tampered_value) in [
        ("key", "archive-imports/attacker/source.zip"),
        ("Content-Type", "text/plain"),
        ("success_action_status", "201"),
        ("x-amz-server-side-encryption", "AES256"),
        (
            "x-amz-server-side-encryption-aws-kms-key-id",
            "arn:aws:kms:us-east-1:111122223333:key/other",
        ),
        ("x-amz-security-token", "other-session"),
    ] {
        let mut fields = post.fields.clone();
        fields.insert(field_name.into(), tampered_value.into());
        assert!(
            !independently_accepts(&fields, RESERVED_BYTES, SECRET_KEY, "us-east-1"),
            "tampering {field_name} must invalidate the multipart submission"
        );
    }

    let mut altered_policy = post.fields.clone();
    let mut policy: Value = serde_json::from_slice(
        &STANDARD
            .decode(field(&altered_policy, "policy"))
            .expect("base64 policy"),
    )
    .expect("JSON policy");
    policy["expiration"] = Value::String("2026-07-23T12:16:00Z".into());
    altered_policy.insert(
        "policy".into(),
        STANDARD.encode(serde_json::to_vec(&policy).unwrap()),
    );
    assert!(
        !independently_accepts(
            &altered_policy,
            RESERVED_BYTES,
            SECRET_KEY,
            "us-east-1"
        ),
        "the fixed signature must not authenticate a modified policy"
    );
}

#[tokio::test]
async fn s3_adapter_presign_uses_the_injected_shared_credentials_provider_without_network() {
    let credentials = Credentials::new(
        ACCESS_KEY,
        SECRET_KEY,
        Some(SESSION_TOKEN.into()),
        None,
        "synthetic-test-provider",
    );
    let credentials_provider = SharedCredentialsProvider::new(credentials);
    let config = aws_sdk_s3::config::Builder::new()
        .behavior_version(BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials_provider.clone())
        .build();
    let store = S3BlobStore::new_with_credentials_provider(
        aws_sdk_s3::Client::from_conf(config),
        credentials_provider,
        "ghostpost-archives".into(),
        "us-east-1".into(),
        "https://ghostpost-archives.s3.us-east-1.amazonaws.com".into(),
        true,
        Some("arn:aws:kms:us-east-1:111122223333:key/synthetic-test-key".into()),
    );

    let post = store
        .presign_post_archive(
            KEY,
            RESERVED_BYTES,
            CONTENT_TYPE,
            Duration::from_secs(900),
        )
        .await
        .expect("the S3 adapter signs with the provider injected alongside its SDK client");
    assert_eq!(
        field(&post.fields, "x-amz-credential").split('/').next(),
        Some(ACCESS_KEY)
    );
    assert_eq!(field(&post.fields, "x-amz-security-token"), SESSION_TOKEN);
    assert!(independently_accepts(
        &post.fields,
        RESERVED_BYTES,
        SECRET_KEY,
        "us-east-1"
    ));
}
