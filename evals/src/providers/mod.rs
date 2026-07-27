//! Eval LLM providers: stub, replay, optional deepseek.

mod deepseek;
mod replay;
mod stub;

pub use deepseek::DeepseekProvider;
pub use replay::ReplayFixtureProvider;
pub use stub::StubDeterministicProvider;

use crate::validate::ScanBatchInput;
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub case_id: String,
    pub prompt_version: String,
    pub system_prompt_hash_hex: String,
    pub user_payload_json: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub content_json: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_hit_tokens: u32,
    pub model_id: String,
    pub provider: &'static str,
    pub http_calls: u32,
}

#[async_trait]
pub trait EvalLlmProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse>;
}

pub fn build_provider(
    root: &std::path::Path,
    name: &str,
) -> Result<Box<dyn EvalLlmProvider>> {
    match name {
        "stub-deterministic" => Ok(Box::new(StubDeterministicProvider::new(root)?)),
        "replay-fixture" => Ok(Box::new(ReplayFixtureProvider::new(root)?)),
        "deepseek-v4-flash" => Ok(Box::new(DeepseekProvider::new(root)?)),
        other => anyhow::bail!("unknown provider: {other}"),
    }
}

/// Serialize batch input with canonical key order for the model.
pub fn serialize_input(input: &ScanBatchInput) -> Result<String> {
    // serde already uses camelCase; stable field order via struct definition
    Ok(serde_json::to_string(input)?)
}
