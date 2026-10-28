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
