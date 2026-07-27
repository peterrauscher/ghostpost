//! Offline replay provider reading manifests/replay/deepseek-v4-flash/.

use super::{CompletionRequest, CompletionResponse, EvalLlmProvider};
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct ReplayIndex {
    model_id: String,
    #[allow(dead_code)]
    prompt_sha256: String,
    cases: HashMap<String, ReplayCaseMeta>,
}

#[derive(Debug, Deserialize)]
struct ReplayCaseMeta {
    response_sha256: String,
    response_path: String,
    input_tokens: u32,
    output_tokens: u32,
    cache_hit_tokens: u32,
}

pub struct ReplayFixtureProvider {
    root: PathBuf,
    index: ReplayIndex,
}

impl ReplayFixtureProvider {
    pub fn new(root: &Path) -> Result<Self> {
        let dir = root.join("manifests/replay/deepseek-v4-flash");
        let index_path = dir.join("index.json");
        let raw = fs::read_to_string(&index_path)
            .with_context(|| format!("read {}", index_path.display()))?;
        let index: ReplayIndex = serde_json::from_str(&raw)?;
        Ok(Self {
            root: dir,
            index,
        })
    }
}

#[async_trait]
impl EvalLlmProvider for ReplayFixtureProvider {
    fn provider_id(&self) -> &'static str {
        "replay-fixture"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let meta = self
            .index
            .cases
            .get(&req.case_id)
            .with_context(|| format!("no replay fixture for case {}", req.case_id))?;
        let path = self.root.join(&meta.response_path);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read replay {}", path.display()))?;
        let digest = crate::manifest::sha256_hex(content.as_bytes());
        if digest != meta.response_sha256 {
            bail!(
                "replay response hash mismatch for {}: lock={} actual={}",
                req.case_id,
                meta.response_sha256,
                digest
            );
        }
        Ok(CompletionResponse {
            content_json: content,
            input_tokens: meta.input_tokens,
            output_tokens: meta.output_tokens,
            cache_hit_tokens: meta.cache_hit_tokens,
            model_id: self.index.model_id.clone(),
            provider: "replay-fixture",
            http_calls: 1,
        })
    }
}
