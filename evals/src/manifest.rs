//! Manifest listing and hash/schema verification.

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("{0}")]
    Hash(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Deserialize)]
pub struct ModelManifest {
    pub model_id: String,
    pub api_base: String,
    pub pricing_usd_per_million: Pricing,
    pub defaults: ModelDefaults,
    pub prompt_version: String,
    pub prompt_sha256: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Pricing {
    pub input_cache_miss: f64,
    pub input_cache_hit: f64,
    pub output: f64,
}

#[derive(Debug, Deserialize)]
pub struct ModelDefaults {
    pub temperature: f64,
    pub max_tokens: u32,
    pub response_format: String,
    pub thinking: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalManifest {
    pub provider: String,
    pub model_id: String,
    pub prompt_version: String,
    pub prompt_sha256: String,
    pub request_flags: ApprovalFlags,
    pub provider_data_handling: DataHandling,
    pub legal_review: LegalReview,
    pub environments: Environments,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalFlags {
    pub response_format: String,
    pub thinking: String,
    pub temperature: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataHandling {
    pub terms_url: Option<String>,
    pub reviewed_at: Option<String>,
    pub retention_days: Option<u64>,
    pub training_use: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegalReview {
    pub ticket: Option<String>,
    pub reviewer: Option<String>,
    pub approved_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Environments {
    pub local: EnvAllow,
    pub staging: EnvAllow,
    pub production: EnvAllow,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvAllow {
    pub allowed: bool,
    pub requires_api_key: bool,
}

pub fn list_manifests(root: &Path) -> Result<()> {
    let entries = [
        "manifests/models/scan-v1.prompt.md",
        "manifests/models/scan-v1.prompt.sha256",
        "manifests/models/deepseek-v4-flash.json",
        "manifests/schemas/scan_input_v1.schema.json",
        "manifests/schemas/scan_output_v1.schema.json",
        "manifests/gold/scan-v1.core.jsonl",
        "manifests/gold/scan-v1.edge.jsonl",
        "manifests/approval/deepseek-v4-flash.manifest.json",
        "manifests/replay/deepseek-v4-flash/index.json",
    ];
    for e in entries {
        let p = root.join(e);
        let status = if p.exists() { "ok" } else { "MISSING" };
        println!("{status}\t{e}");
    }
    Ok(())
}

pub fn verify_manifests(root: &Path) -> Result<(), VerifyError> {
    let prompt_evals = root.join("manifests/models/scan-v1.prompt.md");
    let prompt_backend = root.join("../backend/prompts/scan-v1.md");
    let hash_path = root.join("manifests/models/scan-v1.prompt.sha256");

    let evals_bytes = fs::read(&prompt_evals)
        .with_context(|| format!("read {}", prompt_evals.display()))
        .map_err(VerifyError::Other)?;
    if prompt_backend.exists() {
        let backend_bytes = fs::read(&prompt_backend)
            .with_context(|| format!("read {}", prompt_backend.display()))
            .map_err(VerifyError::Other)?;
        if evals_bytes != backend_bytes {
            return Err(VerifyError::Hash(
                "backend/prompts/scan-v1.md != evals/manifests/models/scan-v1.prompt.md".into(),
            ));
        }
    }

    let digest = sha256_hex(&evals_bytes);
    let locked = fs::read_to_string(&hash_path)
        .with_context(|| format!("read {}", hash_path.display()))
        .map_err(VerifyError::Other)?;
    let locked = locked.trim();
    if locked.len() != 64 || !locked.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(VerifyError::Hash(
            "scan-v1.prompt.sha256 must be lowercase 64-hex".into(),
        ));
    }
    if locked.to_ascii_lowercase() != locked {
        return Err(VerifyError::Hash(
            "scan-v1.prompt.sha256 must be lowercase".into(),
        ));
    }
    if locked != digest {
        return Err(VerifyError::Hash(format!(
            "prompt sha256 mismatch: lock={locked} actual={digest}"
        )));
    }

    // Schema files exist and parse as JSON objects
    for name in [
        "manifests/schemas/scan_input_v1.schema.json",
        "manifests/schemas/scan_output_v1.schema.json",
    ] {
        let p = root.join(name);
        let raw = fs::read_to_string(&p)
            .with_context(|| format!("read {}", p.display()))
            .map_err(VerifyError::Other)?;
        let v: serde_json::Value = serde_json::from_str(&raw)
            .with_context(|| format!("parse schema {}", p.display()))
            .map_err(VerifyError::Other)?;
        if !v.is_object() {
            return Err(VerifyError::Other(anyhow::anyhow!(
                "schema root must be object: {name}"
            )));
        }
        // Mirror check if backend copies exist
        let backend_name = name.strip_prefix("manifests/schemas/").unwrap();
        let bp = root.join("../backend/src/scan/schema").join(backend_name);
        if bp.exists() {
            let b = fs::read(&bp).map_err(|e| VerifyError::Other(e.into()))?;
            let a = fs::read(&p).map_err(|e| VerifyError::Other(e.into()))?;
            if a != b {
                return Err(VerifyError::Hash(format!(
                    "schema mirror mismatch: {name}"
                )));
            }
        }
    }

    let model_path = root.join("manifests/models/deepseek-v4-flash.json");
    let model: ModelManifest = serde_json::from_str(
        &fs::read_to_string(&model_path)
            .with_context(|| format!("read {}", model_path.display()))
            .map_err(VerifyError::Other)?,
    )
    .context("parse model manifest")
    .map_err(VerifyError::Other)?;

    if model.prompt_sha256 != digest {
        return Err(VerifyError::Hash(format!(
            "model manifest prompt_sha256 mismatch: {} != {digest}",
            model.prompt_sha256
        )));
    }
    if (model.pricing_usd_per_million.input_cache_hit - 0.0028).abs() > 1e-12 {
        return Err(VerifyError::Other(anyhow::anyhow!(
            "input_cache_hit must be 0.0028, got {}",
            model.pricing_usd_per_million.input_cache_hit
        )));
    }
    if (model.pricing_usd_per_million.input_cache_miss - 0.14).abs() > 1e-12 {
        return Err(VerifyError::Other(anyhow::anyhow!(
            "input_cache_miss must be 0.14"
        )));
    }
    if (model.pricing_usd_per_million.output - 0.28).abs() > 1e-12 {
        return Err(VerifyError::Other(anyhow::anyhow!("output must be 0.28")));
    }
    // stale 0.014 guard
    let model_raw = fs::read_to_string(&model_path).map_err(|e| VerifyError::Other(e.into()))?;
    if model_raw.contains("0.014") {
        return Err(VerifyError::Other(anyhow::anyhow!(
            "stale cache-hit 0.014 found in model manifest"
        )));
    }

    let approval_path = root.join("manifests/approval/deepseek-v4-flash.manifest.json");
    let approval: ApprovalManifest = serde_json::from_str(
        &fs::read_to_string(&approval_path)
            .with_context(|| format!("read {}", approval_path.display()))
            .map_err(VerifyError::Other)?,
    )
    .context("parse approval")
    .map_err(VerifyError::Other)?;

    if approval.prompt_sha256 != digest {
        return Err(VerifyError::Hash(
            "approval promptSha256 does not match prompt lock".into(),
        ));
    }
    if approval.environments.local.allowed
        || approval.environments.staging.allowed
        || approval.environments.production.allowed
    {
        // Allowed only when legal fields complete — still ok to verify structure;
        // deny-by-default is the committed state.
        if approval.legal_review.approved_at.is_none() {
            return Err(VerifyError::Other(anyhow::anyhow!(
                "environment allowed without legal approval"
            )));
        }
    }

    // Gold files present and non-empty
    for g in [
        "manifests/gold/scan-v1.core.jsonl",
        "manifests/gold/scan-v1.edge.jsonl",
    ] {
        let p = root.join(g);
        let raw = fs::read_to_string(&p)
            .with_context(|| format!("read gold {g}"))
            .map_err(VerifyError::Other)?;
        let lines: Vec<_> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            return Err(VerifyError::Other(anyhow::anyhow!("empty gold: {g}")));
        }
        if g.contains("core") && lines.len() < 120 {
            return Err(VerifyError::Other(anyhow::anyhow!(
                "core gold needs ≥120 cases, got {}",
                lines.len()
            )));
        }
        if g.contains("edge") && lines.len() < 15 {
            return Err(VerifyError::Other(anyhow::anyhow!(
                "edge gold needs ≥15 cases, got {}",
                lines.len()
            )));
        }
    }

    let replay_index = root.join("manifests/replay/deepseek-v4-flash/index.json");
    if !replay_index.exists() {
        return Err(VerifyError::Other(anyhow::anyhow!(
            "missing replay index"
        )));
    }

    Ok(())
}

pub fn load_model_manifest(root: &Path) -> Result<ModelManifest> {
    let p = root.join("manifests/models/deepseek-v4-flash.json");
    let raw = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn load_approval(root: &Path) -> Result<ApprovalManifest> {
    let p = root.join("manifests/approval/deepseek-v4-flash.manifest.json");
    let raw = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn prompt_sha256(root: &Path) -> Result<String> {
    let p = root.join("manifests/models/scan-v1.prompt.sha256");
    Ok(fs::read_to_string(p)?.trim().to_string())
}

pub fn load_prompt(root: &Path) -> Result<String> {
    let p = root.join("manifests/models/scan-v1.prompt.md");
    Ok(fs::read_to_string(p)?)
}

/// Whether deepseek may be used in the given env (fail closed).
pub fn deepseek_allowed(approval: &ApprovalManifest, env: &str) -> bool {
    let env_ok = match env {
        "local" => approval.environments.local.allowed,
        "staging" => approval.environments.staging.allowed,
        "production" => approval.environments.production.allowed,
        _ => false,
    };
    if !env_ok {
        return false;
    }
    let d = &approval.provider_data_handling;
    let l = &approval.legal_review;
    d.terms_url.is_some()
        && d.reviewed_at.is_some()
        && d.retention_days.is_some()
        && d.training_use.is_some()
        && l.ticket.is_some()
        && l.reviewer.is_some()
        && l.approved_at.is_some()
        && l.expires_at.is_some()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn cost_usd(pricing: &Pricing, input: u32, cache_hit: u32, output: u32) -> f64 {
    let miss = input.saturating_sub(cache_hit) as f64;
    let hit = cache_hit as f64;
    let out = output as f64;
    (miss * pricing.input_cache_miss + hit * pricing.input_cache_hit + out * pricing.output)
        / 1_000_000.0
}

/// Reserved privacy scan hook (no approval short-circuit paths).
pub fn privacy_grep_clean(_root: &Path) -> Result<()> {
    Ok(())
}
