//! DeepSeek V4 Flash adapter — thinking disabled, JSON object mode.
use super::{
    cost_usd, ScanCompletionRequest, ScanCompletionResponse, ScanLlmError, ScanLlmProvider,
    REPAIR_USER_PREFIX,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

pub const PROVIDER_ID: &str = "deepseek-v4-flash";
pub const MODEL_ID: &str = "deepseek-v4-flash";
pub const API_BASE: &str = "https://api.deepseek.com/v1";
pub const DEFAULT_MAX_TOKENS: u32 = 4096;
pub const DEFAULT_TEMPERATURE: f32 = 0.0;

pub struct DeepseekV4FlashProvider {
    api_key: String,
    api_base: String,
    client: reqwest::Client,
    inflight: Arc<Semaphore>,
    max_inflight: usize,
    active: AtomicUsize,
}

impl DeepseekV4FlashProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        let max_inflight = std::env::var("SCAN_LLM_MAX_INFLIGHT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4usize)
            .max(1);
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(120))
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self {
            api_key: api_key.into(),
            api_base: std::env::var("DEEPSEEK_API_BASE").unwrap_or_else(|_| API_BASE.into()),
            client,
            inflight: Arc::new(Semaphore::new(max_inflight)),
            max_inflight,
            active: AtomicUsize::new(0),
        }
    }

    pub fn max_inflight(&self) -> usize {
        self.max_inflight
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    id: Option<String>,
    choices: Option<Vec<Choice>>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
    // DeepSeek may report cache hits under prompt_cache_hit_tokens
    prompt_cache_hit_tokens: Option<u32>,
}

#[async_trait]
impl ScanLlmProvider for DeepseekV4FlashProvider {
    fn provider_id(&self) -> &'static str {
        PROVIDER_ID
    }

    async fn complete(
        &self,
        req: ScanCompletionRequest,
    ) -> Result<ScanCompletionResponse, ScanLlmError> {
        let _permit = self
            .inflight
            .acquire()
            .await
            .map_err(|_| ScanLlmError::Transport)?;
        self.active.fetch_add(1, Ordering::Relaxed);
        let start = std::time::Instant::now();

        let system = crate::scan::SCAN_V1_PROMPT;
        let user_content = if req.repair {
            format!("{REPAIR_USER_PREFIX}{}", req.user_payload_json)
        } else {
            req.user_payload_json.clone()
        };

        // Do NOT log message bodies.
        let body = serde_json::json!({
            "model": MODEL_ID,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user_content}
            ],
            "temperature": req.temperature,
            "max_tokens": req.max_output_tokens,
            "stream": false,
            "response_format": {"type": "json_object"},
            "thinking": {"type": "disabled"}
        });

        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let result = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        self.active.fetch_sub(1, Ordering::Relaxed);

        let resp = match result {
            Ok(r) => r,
            Err(e) => {
                if e.is_timeout() {
                    return Err(ScanLlmError::Timeout);
                }
                return Err(ScanLlmError::Transport);
            }
        };

        let status = resp.status();
        if status.as_u16() == 429 {
            let retry_after_ms = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(|secs| secs.saturating_mul(1000));
            return Err(ScanLlmError::RateLimited { retry_after_ms });
        }
        if status.as_u16() == 503 {
            return Err(ScanLlmError::Unavailable);
        }
        if status.is_client_error() {
            return Err(ScanLlmError::Rejected);
        }
        if !status.is_success() {
            return Err(ScanLlmError::Transport);
        }

        let parsed: ChatCompletionResponse = resp
            .json()
            .await
            .map_err(|_| ScanLlmError::Transport)?;
        let content = parsed
            .choices
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.message.as_ref())
            .and_then(|m| m.content.clone())
            .ok_or(ScanLlmError::Rejected)?;

        let input_tokens = parsed.usage.as_ref().and_then(|u| u.prompt_tokens).unwrap_or(0);
        let output_tokens = parsed
            .usage
            .as_ref()
            .and_then(|u| u.completion_tokens)
            .unwrap_or(0);
        let cache_hit_tokens = parsed
            .usage
            .as_ref()
            .and_then(|u| u.prompt_cache_hit_tokens)
            .unwrap_or(0);

        // Touch pricing helper so cost path is covered when callers ask.
        let _ = cost_usd(input_tokens, cache_hit_tokens, output_tokens);

        tracing::info!(
            request_id = %req.request_id,
            model_id = MODEL_ID,
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            latency_ms = start.elapsed().as_millis() as u32,
            outcome = "http_ok",
            "scan llm completion"
        );

        Ok(ScanCompletionResponse {
            content_json: content,
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            model_id: MODEL_ID.into(),
            provider: PROVIDER_ID,
            provider_request_id: parsed.id,
            latency_ms: start.elapsed().as_millis() as u32,
        })
    }
}
