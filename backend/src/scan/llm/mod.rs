//! Provider-neutral scan LLM port.
pub mod deepseek;
pub mod jev;
pub mod replay;
pub mod stub;

use chrono::{DateTime, Datelike, Timelike, Utc, Weekday};
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
    /// USD, computed by the provider from its own pricing.
    pub cost_usd: f64,
}

#[derive(Debug, Error)]
pub enum ScanLlmError {
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
    /// Version of the prompt or question set recorded on each attempt.
    fn prompt_version(&self) -> &'static str {
        crate::scan::SCAN_V1_PROMPT_VERSION
    }
    /// SHA-256 of that prompt or question set; also keys batch ids.
    fn prompt_sha256(&self) -> [u8; 32] {
        crate::scan::scan_v1_prompt_sha256()
    }
    async fn complete(
        &self,
        req: ScanCompletionRequest,
    ) -> Result<ScanCompletionResponse, ScanLlmError>;
}

/// DeepSeek V4.1 Flash peak pricing (USD per 1M tokens). Off-peak is exactly half.
pub const PEAK_PRICE_INPUT_MISS: f64 = 0.30;
pub const PEAK_PRICE_INPUT_HIT: f64 = 0.006;
pub const PEAK_PRICE_OUTPUT: f64 = 1.20;

/// DeepSeek peak windows: 01:00–04:00 and 06:00–10:00 UTC, Monday–Friday.
/// Chinese public holidays are off-peak upstream but not modeled here, so
/// recorded cost over-estimates on those days.
pub fn is_deepseek_peak(at: DateTime<Utc>) -> bool {
    if matches!(at.weekday(), Weekday::Sat | Weekday::Sun) {
        return false;
    }
    matches!(at.hour(), 1..=3 | 6..=9)
}

pub fn cost_usd(
    input_tokens: u32,
    cache_hit_tokens: u32,
    output_tokens: u32,
    at: DateTime<Utc>,
) -> f64 {
    let scale = if is_deepseek_peak(at) { 1.0 } else { 0.5 };
    let hit = cache_hit_tokens.min(input_tokens) as f64;
    let miss = (input_tokens as f64) - hit;
    scale
        * (miss * PEAK_PRICE_INPUT_MISS + hit * PEAK_PRICE_INPUT_HIT
            + (output_tokens as f64) * PEAK_PRICE_OUTPUT)
        / 1_000_000.0
}

pub const REPAIR_USER_PREFIX: &str =
    "Return only scan-output.v1 JSON matching the supplied schema. Do not add or omit items.\n";

/// Build provider from env. Live providers require their API key.
pub fn build_provider_from_env() -> Result<std::sync::Arc<dyn ScanLlmProvider>, ScanLlmError> {
    let name = std::env::var("SCAN_LLM_PROVIDER").unwrap_or_else(|_| "stub-deterministic".into());
    match name.as_str() {
        "stub-deterministic" => Ok(std::sync::Arc::new(stub::StubDeterministicProvider::default())),
        "replay-fixture" => {
            let dir = std::env::var("SCAN_LLM_REPLAY_DIR").unwrap_or_else(|_| "evals/manifests/replay/deepseek-flash".into());
            Ok(std::sync::Arc::new(replay::ReplayFixtureProvider::from_dir(dir)?))
        }
        "deepseek-flash" => {
            let api_key = std::env::var("DEEPSEEK_API_KEY")
                .map_err(|_| ScanLlmError::Misconfigured("DEEPSEEK_API_KEY required".into()))?;
            if api_key.trim().is_empty() {
                return Err(ScanLlmError::Misconfigured("DEEPSEEK_API_KEY empty".into()));
            }
            Ok(std::sync::Arc::new(deepseek::DeepseekFlashProvider::new(api_key)))
        }
        "jev" => Ok(std::sync::Arc::new(jev::JevScanProvider::from_env()?)),
        other => Err(ScanLlmError::Misconfigured(format!("unknown SCAN_LLM_PROVIDER={other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn peak_windows_follow_deepseek_schedule() {
        // 2026-10-12 is a Monday.
        assert!(!is_deepseek_peak(at("2026-10-12T00:59:59Z")));
        assert!(is_deepseek_peak(at("2026-10-12T01:00:00Z")));
        assert!(is_deepseek_peak(at("2026-10-12T03:59:59Z")));
        assert!(!is_deepseek_peak(at("2026-10-12T04:00:00Z")));
        assert!(!is_deepseek_peak(at("2026-10-12T05:59:59Z")));
        assert!(is_deepseek_peak(at("2026-10-12T06:00:00Z")));
        assert!(is_deepseek_peak(at("2026-10-16T09:59:59Z")));
        assert!(!is_deepseek_peak(at("2026-10-16T10:00:00Z")));
        assert!(!is_deepseek_peak(at("2026-10-17T02:00:00Z"))); // Saturday
        assert!(!is_deepseek_peak(at("2026-10-18T07:00:00Z"))); // Sunday
    }

    #[test]
    fn off_peak_costs_half_of_peak() {
        // 0.5M miss * 0.30 + 0.5M hit * 0.006 + 0.1M out * 1.20 = 0.15 + 0.003 + 0.12
        let peak = cost_usd(1_000_000, 500_000, 100_000, at("2026-10-12T02:00:00Z"));
        let off = cost_usd(1_000_000, 500_000, 100_000, at("2026-10-12T12:00:00Z"));
        assert!((peak - 0.273).abs() < 1e-9);
        assert!((off - 0.1365).abs() < 1e-9);
    }

    #[test]
    fn cache_hits_cannot_exceed_input() {
        let c = cost_usd(100, 1_000, 0, at("2026-10-12T02:00:00Z"));
        assert!((c - 100.0 * PEAK_PRICE_INPUT_HIT / 1_000_000.0).abs() < 1e-15);
    }
}
