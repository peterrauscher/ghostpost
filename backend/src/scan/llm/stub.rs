//! Deterministic hash-based canned JSON provider for local/dev/tests.
use super::{ScanCompletionRequest, ScanCompletionResponse, ScanLlmError, ScanLlmProvider};
use async_trait::async_trait;
use sha2::{Digest, Sha256};

#[derive(Debug, Default)]
pub struct StubDeterministicProvider;

fn hash_byte(payload: &str) -> u8 {
    let mut h = Sha256::new();
    h.update(payload.as_bytes());
    h.finalize()[0]
}

#[async_trait]
impl ScanLlmProvider for StubDeterministicProvider {
    fn provider_id(&self) -> &'static str {
        "stub-deterministic"
    }

    async fn complete(
        &self,
        req: ScanCompletionRequest,
    ) -> Result<ScanCompletionResponse, ScanLlmError> {
        let start = std::time::Instant::now();
        // Parse input items length from payload without retaining long-term.
        let v: serde_json::Value =
            serde_json::from_str(&req.user_payload_json).map_err(|_| ScanLlmError::Rejected)?;
        let items = v
            .get("items")
            .and_then(|i| i.as_array())
            .ok_or(ScanLlmError::Rejected)?;

        let mut results = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let text = item.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let idx = item
                .get("itemIndex")
                .and_then(|x| x.as_u64())
                .unwrap_or(i as u64) as u32;
            let b = hash_byte(&format!("{idx}:{text}"));
            // Deterministic: high bit clear => no_flag; set => flag when possible
            let flag = b & 1 == 1 && text.len() >= 8;
            if flag {
                // Use a short evidence substring
                let ev = if text.len() >= 8 {
                    text.chars().take(8).collect::<String>()
                } else {
                    text.to_string()
                };
                // Prefer negativity if present in concerns else first non-other
                let concerns = v
                    .pointer("/policy/concerns")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                let cat = concerns
                    .iter()
                    .filter_map(|c| c.as_str())
                    .find(|c| *c != "other")
                    .unwrap_or("negativity");
                let (code, category) = match cat {
                    "inappropriate_language" => ("targeted_insult", "inappropriate_language"),
                    "drinking_drugs" => ("risky_alcohol_or_drug_content", "drinking_drugs"),
                    "political_takes" => ("political_advocacy", "political_takes"),
                    "controversial_topics" => ("polarizing_advocacy", "controversial_topics"),
                    "public_image" => ("admitted_misconduct", "public_image"),
                    _ => ("sustained_hostility", "negativity"),
                };
                // Only flag if category is selected
                let selected = concerns.iter().any(|c| c.as_str() == Some(category));
                if selected && text.contains(&ev) {
                    results.push(serde_json::json!({
                        "itemIndex": idx,
                        "decision": "flag",
                        "risk": "low",
                        "category": category,
                        "confidence": 0.75,
                        "reasons": [{"code": code, "summary": "stub flag"}],
                        "evidence": [{"text": ev, "supportsReasonCode": code}]
                    }));
                    continue;
                }
            }
            results.push(serde_json::json!({
                "itemIndex": idx,
                "decision": "no_flag",
                "risk": "none",
                "category": null,
                "confidence": 0.91,
                "reasons": [],
                "evidence": []
            }));
        }
        let body = serde_json::json!({
            "schemaVersion": "scan-output.v1",
            "results": results
        });
        let content_json = serde_json::to_string(&body).map_err(|_| ScanLlmError::Rejected)?;
        let latency_ms = start.elapsed().as_millis() as u32;
        let input_tokens = (req.user_payload_json.len() as u32 / 4).max(1);
        let output_tokens = (content_json.len() as u32 / 4).max(1);
        Ok(ScanCompletionResponse {
            content_json,
            input_tokens,
            output_tokens,
            cache_hit_tokens: 0,
            model_id: "stub-deterministic".into(),
            provider: "stub-deterministic",
            provider_request_id: Some(req.request_id.to_string()),
            latency_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn stub_returns_valid_shape() {
        let p = StubDeterministicProvider;
        let payload = r#"{"schemaVersion":"scan-input.v1","policy":{"comingUp":[],"concerns":["negativity"]},"items":[{"itemIndex":0,"platform":"x","kind":"tweet","authorship":"authored","text":"hello world friends"}]}"#;
        let resp = p
            .complete(ScanCompletionRequest {
                prompt_version: "scan-v1",
                system_prompt_hash: [0u8; 32],
                user_payload_json: payload.into(),
                temperature: 0.0,
                max_output_tokens: 4096,
                request_id: Uuid::nil(),
                repair: false,
            })
            .await
            .unwrap();
        assert!(resp.content_json.contains("scan-output.v1"));
        assert_eq!(resp.provider, "stub-deterministic");
    }
}
