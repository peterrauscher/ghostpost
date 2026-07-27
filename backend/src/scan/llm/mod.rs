//! Provider-neutral scan LLM port.
pub mod approval;
pub mod deepseek;
pub mod replay;
pub mod stub;

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ScanCompletionRequest {
    pub prompt_version: &'static str,
    pub system_prompt_hash: [u8; 32],
    pub user_payload_json: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
    pub request_id: Uuid,
    /// When true, provider should use the repair system nudge (same user payload).
    pub repair: bool,
}

#[derive(Debug, Clone)]
pub struct ScanCompletionResponse {
    pub content_json: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_hit_tokens: u32,
    pub model_id: String,
    pub provider: &'static str,
    pub provider_request_id: Option<String>,
    pub latency_ms: u32,
}

#[derive(Debug, Error)]
pub enum ScanLlmError {
    #[error("provider not approved: {0}")]
    NotApproved(String),
    #[error("provider transport error")]
    Transport,
    #[error("provider rejected request")]
    Rejected,
    #[error("provider rate limited")]
    RateLimited { retry_after_ms: Option<u64> },
    #[error("provider unavailable")]
    Unavailable,
    #[error("provider timeout")]
    Timeout,
    #[error("provider misconfigured: {0}")]
    Misconfigured(String),
    #[error("fixture missing")]
    FixtureMissing,
    #[error("privacy guard")]
    PrivacyGuard,
}

impl ScanLlmError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotApproved(_) => "SCAN_PROVIDER_NOT_APPROVED",
            Self::Transport => "SCAN_PROVIDER_TRANSPORT",
            Self::Rejected => "SCAN_PROVIDER_REJECTED",
            Self::RateLimited { .. } => "SCAN_PROVIDER_RATE_LIMITED",
            Self::Unavailable => "SCAN_PROVIDER_UNAVAILABLE",
            Self::Timeout => "SCAN_PROVIDER_TIMEOUT",
            Self::Misconfigured(_) => "SCAN_PROVIDER_MISCONFIGURED",
            Self::FixtureMissing => "SCAN_PROVIDER_FIXTURE_MISSING",
            Self::PrivacyGuard => "SCAN_PRIVACY_GUARD",
        }
    }

    pub fn is_retryable_transport(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Unavailable | Self::Timeout | Self::Transport
        )
    }
}

#[async_trait]
pub trait ScanLlmProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn complete(
        &self,
        req: ScanCompletionRequest,
    ) -> Result<ScanCompletionResponse, ScanLlmError>;
}

/// DeepSeek V4 Flash pricing (USD per 1M tokens).
pub const PRICE_INPUT_MISS: f64 = 0.14;
pub const PRICE_INPUT_HIT: f64 = 0.0028;
pub const PRICE_OUTPUT: f64 = 0.28;

pub fn cost_usd(input_tokens: u32, cache_hit_tokens: u32, output_tokens: u32) -> f64 {
    let hit = cache_hit_tokens.min(input_tokens) as f64;
    let miss = (input_tokens as f64) - hit;
    miss * PRICE_INPUT_MISS / 1_000_000.0
        + hit * PRICE_INPUT_HIT / 1_000_000.0
        + (output_tokens as f64) * PRICE_OUTPUT / 1_000_000.0
}

pub const REPAIR_USER_PREFIX: &str =
    "Return only scan-output.v1 JSON matching the supplied schema. Do not add or omit items.\n";

/// Build provider from env. DeepSeek requires approval gate.
pub fn build_provider_from_env() -> Result<std::sync::Arc<dyn ScanLlmProvider>, ScanLlmError> {
    let name = std::env::var("SCAN_LLM_PROVIDER").unwrap_or_else(|_| "stub-deterministic".into());
    match name.as_str() {
        "stub-deterministic" => Ok(std::sync::Arc::new(stub::StubDeterministicProvider::default())),
        "replay-fixture" => {
            let dir = std::env::var("SCAN_LLM_REPLAY_DIR").unwrap_or_else(|_| "evals/manifests/replay/deepseek-v4-flash".into());
            Ok(std::sync::Arc::new(replay::ReplayFixtureProvider::from_dir(dir)?))
        }
        "deepseek-v4-flash" => {
            let env_raw = std::env::var("GHOSTPOST_ENV").unwrap_or_else(|_| "local".into());
            let env_lower = env_raw.to_ascii_lowercase();
            let env_name = match env_lower.as_str() {
                "development" | "dev" | "local" => "local",
                "staging" => "staging",
                "production" | "prod" => "production",
                other => other,
            };
            let manifest = approval::load_manifest_from_env()?;
            approval::assert_approved(&manifest, env_name, &crate::scan::hex_encode(&crate::scan::scan_v1_prompt_sha256()))?;
            let api_key = std::env::var("DEEPSEEK_API_KEY")
                .map_err(|_| ScanLlmError::Misconfigured("DEEPSEEK_API_KEY required".into()))?;
            if api_key.trim().is_empty() {
                return Err(ScanLlmError::Misconfigured("DEEPSEEK_API_KEY empty".into()));
            }
            Ok(std::sync::Arc::new(deepseek::DeepseekV4FlashProvider::new(api_key)))
        }
        other => Err(ScanLlmError::Misconfigured(format!("unknown SCAN_LLM_PROVIDER={other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pricing_uses_0028_hit() {
        let c = cost_usd(1_000_000, 500_000, 0);
        // 0.5M * 0.14 + 0.5M * 0.0028 = 0.07 + 0.0014 = 0.0714
        assert!((c - 0.0714).abs() < 1e-9);
        assert!((PRICE_INPUT_HIT - 0.0028).abs() < 1e-12);
    }
}
