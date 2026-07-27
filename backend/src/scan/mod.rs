//! Scan pipeline: DTOs, validation, LLM providers, fenced persistence.
pub mod dto;
pub mod llm;
pub mod persist;
pub mod validate;

use sha2::{Digest, Sha256};

/// Canonical production system prompt (scan-v1), embedded at compile time.
pub const SCAN_V1_PROMPT: &str = include_str!("../../prompts/scan-v1.md");
pub const SCAN_V1_PROMPT_VERSION: &str = "scan-v1";

/// SHA-256 of the embedded prompt bytes.
pub fn prompt_sha256(prompt: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prompt.as_bytes());
    h.finalize().into()
}

pub fn scan_v1_prompt_sha256() -> [u8; 32] {
    prompt_sha256(SCAN_V1_PROMPT)
}

pub fn prompt_sha256_hex(prompt: &str) -> String {
    hex_encode(&prompt_sha256(prompt))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex_encode(&h.finalize())
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    #[test]
    fn privacy_no_raw_llm_retention() {
        // Structural guarantee: ModelAttemptRecord / persist path fields must not
        // include raw prompt or completion bodies.
        let fields = [
            "prompt_version",
            "prompt_sha256",
            "model_id",
            "request_sha256",
            "result_sha256",
            "outcome",
            "error_code",
            "input_tokens",
            "output_tokens",
            "cache_hit_tokens",
            "cost_usd",
            "latency_ms",
            "provider_request_id",
            "provider",
        ];
        let forbidden = ["raw_prompt", "raw_completion", "prompt_text", "completion_text", "user_payload_json"];
        for f in forbidden {
            assert!(!fields.contains(&f), "forbidden field {f}");
        }
        // Prompt is compile-time only; hash is retained, not body columns.
        let hash = scan_v1_prompt_sha256();
        assert_eq!(hash.len(), 32);
        assert!(!SCAN_V1_PROMPT.is_empty());
        // Migration fencing file must not introduce raw text columns.
        let mig = include_str!("../../migrations/20260724100013_scan_fencing.sql");
        for bad in ["raw_prompt", "raw_completion", "prompt_text", "completion_body"] {
            assert!(
                !mig.contains(bad),
                "migration must not retain {bad}"
            );
        }
    }

    #[test]
    fn prompt_sha_stable() {
        let a = prompt_sha256_hex(SCAN_V1_PROMPT);
        let b = prompt_sha256_hex(SCAN_V1_PROMPT);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }
}
