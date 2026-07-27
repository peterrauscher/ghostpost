//! Replay fixture provider (eval offline mode).
use super::{ScanCompletionRequest, ScanCompletionResponse, ScanLlmError, ScanLlmProvider};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ReplayFixtureProvider {
    /// Map from request_sha256 hex or case id -> response JSON body
    fixtures: HashMap<String, String>,
    root: PathBuf,
}

impl ReplayFixtureProvider {
    pub fn from_dir(dir: impl Into<PathBuf>) -> Result<Self, ScanLlmError> {
        let root = dir.into();
        let mut fixtures = HashMap::new();
        let index_path = root.join("index.json");
        if index_path.exists() {
            let raw = std::fs::read_to_string(&index_path).map_err(|_| ScanLlmError::FixtureMissing)?;
            let v: serde_json::Value =
                serde_json::from_str(&raw).map_err(|_| ScanLlmError::FixtureMissing)?;
            if let Some(cases) = v.get("cases").and_then(|c| c.as_object()) {
                for (case_id, meta) in cases {
                    if let Some(path) = meta.get("response_path").and_then(|p| p.as_str()) {
                        let full = root.join(path);
                        if let Ok(body) = std::fs::read_to_string(&full) {
                            fixtures.insert(case_id.clone(), body);
                            if let Some(sha) = meta.get("response_sha256").and_then(|s| s.as_str()) {
                                fixtures.insert(sha.to_string(), fixtures.get(case_id).cloned().unwrap_or_default());
                            }
                        }
                    }
                }
            }
        }
        // Also load any *.response.json directly
        if let Ok(rd) = std::fs::read_dir(&root) {
            for ent in rd.flatten() {
                let path = ent.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        if name.ends_with(".response") || name.contains("response") {
                            if let Ok(body) = std::fs::read_to_string(&path) {
                                fixtures.insert(name.to_string(), body);
                            }
                        }
                    }
                }
            }
        }
        Ok(Self { fixtures, root })
    }

    pub fn insert_fixture(&mut self, key: impl Into<String>, body: impl Into<String>) {
        self.fixtures.insert(key.into(), body.into());
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[async_trait]
impl ScanLlmProvider for ReplayFixtureProvider {
    fn provider_id(&self) -> &'static str {
        "replay-fixture"
    }

    async fn complete(
        &self,
        req: ScanCompletionRequest,
    ) -> Result<ScanCompletionResponse, ScanLlmError> {
        let start = std::time::Instant::now();
        let req_hash = crate::scan::sha256_hex(req.user_payload_json.as_bytes());
        let body = self
            .fixtures
            .get(&req_hash)
            .or_else(|| self.fixtures.get(&req.request_id.to_string()))
            .cloned()
            .ok_or(ScanLlmError::FixtureMissing)?;
        Ok(ScanCompletionResponse {
            content_json: body,
            input_tokens: 0,
            output_tokens: 0,
            cache_hit_tokens: 0,
            model_id: "replay-fixture".into(),
            provider: "replay-fixture",
            provider_request_id: Some(req.request_id.to_string()),
            latency_ms: start.elapsed().as_millis() as u32,
        })
    }
}
