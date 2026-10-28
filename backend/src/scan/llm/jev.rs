//! Jev (TypeSafe System One) scan classifier: `scan-jev` question set over a
//! reqwest transport.
use super::{ScanCompletionRequest, ScanCompletionResponse, ScanLlmError, ScanLlmProvider};
use crate::scan::dto::ScanBatchInput;
use async_trait::async_trait;
use scan_jev::{Call, Item, JevError, JevLimits, SystemOne, SystemOneResponse};
use std::time::Duration;

pub const PROVIDER_ID: &str = "typesafe-jev";
pub const API_BASE: &str = "https://api.typesafe.ai";

pub struct JevHttp {
    api_key: String,
    api_base: String,
    client: reqwest::Client,
}

impl JevHttp {
    pub fn new(api_key: impl Into<String>, api_base: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self { api_key: api_key.into(), api_base: api_base.into(), client }
    }
}

#[async_trait]
impl SystemOne for JevHttp {
    async fn evaluate(&self, request_json: &str) -> Result<Call, JevError> {
        let start = std::time::Instant::now();
        let url = format!("{}/v1/systemone", self.api_base.trim_end_matches('/'));
        // Do NOT log request or response bodies: they carry user post text.
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.api_key)
            .header("Content-Type", "application/json")
            .body(request_json.to_owned())
            .send()
            .await
            .map_err(|e| if e.is_timeout() { JevError::Timeout } else { JevError::Transport })?;
        match resp.status().as_u16() {
            200..=299 => {}
            429 => {
                let retry_after_ms = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|secs| secs.saturating_mul(1000));
                return Err(JevError::RateLimited { retry_after_ms });
            }
            500..=599 => return Err(JevError::Unavailable), // includes 529 Overloaded
            400..=499 => return Err(JevError::Rejected),
            _ => return Err(JevError::Transport),
        }
        let bytes = resp.bytes().await.map_err(|_| JevError::Transport)?;
        let response: SystemOneResponse = serde_json::from_slice(&bytes).map_err(|_| JevError::Unavailable)?;
        Ok(Call {
            response,
            response_sha256: crate::scan::sha256_hex(&bytes),
            latency_ms: start.elapsed().as_millis() as u32,
        })
    }
}

fn to_scan_error(e: JevError) -> ScanLlmError {
    match e {
        JevError::RateLimited { retry_after_ms } => ScanLlmError::RateLimited { retry_after_ms },
        JevError::Unavailable => ScanLlmError::Unavailable,
        JevError::Timeout => ScanLlmError::Timeout,
        JevError::Transport => ScanLlmError::Transport,
        JevError::Rejected => ScanLlmError::Rejected,
        // pack_batches segments posts (SCAN_BATCH_MAX_SCALARS) well below the limit.
        JevError::PostTooLong => {
            ScanLlmError::Misconfigured("segment exceeds SCAN_JEV_MAX_POST_TOKENS; lower SCAN_BATCH_MAX_SCALARS".into())
        }
    }
}

pub struct JevScanProvider {
    http: JevHttp,
    limits: JevLimits,
}

impl JevScanProvider {
    pub fn new(http: JevHttp, limits: JevLimits) -> Self {
        Self { http, limits }
    }

    /// `TYPESAFE_API_KEY` and optional `SCAN_JEV_*` limits.
    pub fn from_env() -> Result<Self, ScanLlmError> {
        let api_key = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            return Err(ScanLlmError::Misconfigured("TYPESAFE_API_KEY required".into()));
        }
        let api_base = std::env::var("TYPESAFE_API_BASE").unwrap_or_else(|_| API_BASE.into());
        Ok(Self::new(JevHttp::new(api_key, api_base), limits_from_env()?))
    }
}

pub fn limits_from_env() -> Result<JevLimits, ScanLlmError> {
    fn var<T: std::str::FromStr>(name: &str, default: T) -> Result<T, ScanLlmError> {
        match std::env::var(name) {
            Ok(v) => v.parse().map_err(|_| ScanLlmError::Misconfigured(format!("invalid {name}={v}"))),
            Err(_) => Ok(default),
        }
    }
    let d = JevLimits::default();
    let limits = JevLimits {
        max_posts: var("SCAN_JEV_MAX_POSTS", d.max_posts)?,
        max_state_tokens: var("SCAN_JEV_MAX_STATE_TOKENS", d.max_state_tokens)?,
        max_request_tokens: var("SCAN_JEV_MAX_REQUEST_TOKENS", d.max_request_tokens)?,
        max_post_tokens: var("SCAN_JEV_MAX_POST_TOKENS", d.max_post_tokens)?,
        escalate_at: var("SCAN_JEV_ESCALATE_AT", d.escalate_at)?,
        flag_at: var("SCAN_JEV_FLAG_AT", d.flag_at)?,
    };
    let unit = |v: f64| v > 0.0 && v <= 1.0;
    if limits.max_posts == 0 || !unit(limits.escalate_at) || !unit(limits.flag_at) {
        return Err(ScanLlmError::Misconfigured(
            "SCAN_JEV_MAX_POSTS must be > 0; SCAN_JEV_ESCALATE_AT and SCAN_JEV_FLAG_AT in (0, 1]".into(),
        ));
    }
    Ok(limits)
}

#[async_trait]
impl ScanLlmProvider for JevScanProvider {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    fn prompt_version(&self) -> &'static str {
        scan_jev::QUESTIONS_VERSION
    }

    fn prompt_sha256(&self) -> [u8; 32] {
        scan_jev::questions_sha256()
    }

    async fn complete(&self, req: ScanCompletionRequest) -> Result<ScanCompletionResponse, ScanLlmError> {
        let input: ScanBatchInput = serde_json::from_str(&req.user_payload_json).map_err(|_| ScanLlmError::Rejected)?;
        let items: Vec<Item> = input.items.iter().map(|i| Item { item_index: i.item_index, text: &i.text }).collect();
        let out = scan_jev::classify(&self.http, &input.policy.concerns, &input.policy.coming_up, &items, &self.limits)
            .await
            .map_err(to_scan_error)?;
        let usage = out.usage();
        tracing::info!(
            request_id = %req.request_id,
            posts = items.len(),
            jev_calls = out.calls.len(),
            input_tokens = usage.input_tokens,
            "scan jev classification"
        );
        Ok(ScanCompletionResponse {
            content_json: scan_jev::output_json(&out.results),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_hit_tokens: 0,
            // Versioned id reported by the API, not the requested alias.
            model_id: out.calls.first().map(|c| c.model.clone()).unwrap_or_else(|| scan_jev::MODEL_ID.into()),
            provider: PROVIDER_ID,
            provider_request_id: None,
            latency_ms: out.calls.iter().map(|c| c.latency_ms).sum(),
            cost_usd: scan_jev::cost_usd(&usage),
        })
    }
}

