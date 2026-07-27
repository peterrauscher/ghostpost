//! Live DeepSeek V4 Flash provider (requires approval + API key).

use super::{CompletionRequest, CompletionResponse, EvalLlmProvider};
use crate::manifest::{self, deepseek_allowed};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub struct DeepseekProvider {
    #[allow(dead_code)]
    root: PathBuf,
    api_key: String,
    api_base: String,
    system_prompt: String,
    approved: bool,
}

impl DeepseekProvider {
    pub fn new(root: &Path) -> Result<Self> {
        let approval = manifest::load_approval(root)?;
        let model = manifest::load_model_manifest(root)?;
        let env = std::env::var("GHOSTPOST_ENV").unwrap_or_else(|_| "local".into());
        let approved = deepseek_allowed(&approval, &env);
        let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
        let system_prompt = manifest::load_prompt(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            api_key,
            api_base: model.api_base,
            system_prompt,
            approved,
        })
    }

    pub fn is_approved(&self) -> bool {
        self.approved && !self.api_key.is_empty()
    }
}

#[async_trait]
impl EvalLlmProvider for DeepseekProvider {
    fn provider_id(&self) -> &'static str {
        "deepseek-v4-flash"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        if !self.approved {
            bail!("approval manifest blocked deepseek-v4-flash");
        }
        if self.api_key.is_empty() {
            bail!("DEEPSEEK_API_KEY required for deepseek-v4-flash");
        }
        // Minimal HTTP via std — avoid pulling reqwest if not needed for offline CI.
        // Use ureq-less approach: return clear error that live calls need network feature.
        // For plan compliance we implement via `std::process` curl fallback OR tokio + hyper.
        // Use blocking-free manual with tokio::task and std::net is heavy; use `std::process::Command` curl.
        let body = serde_json::json!({
            "model": "deepseek-v4-flash",
            "messages": [
                {"role": "system", "content": self.system_prompt},
                {"role": "user", "content": req.user_payload_json},
            ],
            "temperature": req.temperature,
            "max_tokens": req.max_output_tokens,
            "stream": false,
            "response_format": {"type": "json_object"},
            "thinking": {"type": "disabled"},
        });
        let url = format!("{}/chat/completions", self.api_base.trim_end_matches('/'));
        let body_s = body.to_string();
        let key = self.api_key.clone();
        let resp_text = tokio::task::spawn_blocking(move || {
            // Prefer curl for zero extra deps
            let out = std::process::Command::new("curl")
                .args([
                    "-sS",
                    "-X",
                    "POST",
                    &url,
                    "-H",
                    "Content-Type: application/json",
                    "-H",
                    &format!("Authorization: Bearer {key}"),
                    "-d",
                    &body_s,
                    "--max-time",
                    "120",
                ])
                .output()
                .context("spawn curl")?;
            if !out.status.success() {
                bail!(
                    "curl failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Ok::<String, anyhow::Error>(String::from_utf8_lossy(&out.stdout).into_owned())
        })
        .await??;

        let v: serde_json::Value =
            serde_json::from_str(&resp_text).context("parse deepseek response")?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let usage = &v["usage"];
        let input_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0) as u32;
        let output_tokens = usage["completion_tokens"].as_u64().unwrap_or(0) as u32;
        let cache_hit = usage["prompt_cache_hit_tokens"]
            .as_u64()
            .or_else(|| usage["cache_hit_tokens"].as_u64())
            .unwrap_or(0) as u32;
        Ok(CompletionResponse {
            content_json: content,
            input_tokens,
            output_tokens,
            cache_hit_tokens: cache_hit,
            model_id: "deepseek-v4-flash".into(),
            provider: "deepseek-v4-flash",
            http_calls: 1,
        })
    }
}
