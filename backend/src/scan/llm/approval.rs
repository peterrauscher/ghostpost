//! Deny-by-default approval manifest gate. NO bypass.
use super::ScanLlmError;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalManifest {
    pub provider: String,
    pub model_id: String,
    pub prompt_version: String,
    pub prompt_sha256: String,
    #[serde(default)]
    pub model_manifest_sha256: Option<String>,
    pub request_flags: RequestFlags,
    #[serde(default)]
    pub provider_data_handling: ProviderDataHandling,
    #[serde(default)]
    pub legal_review: LegalReview,
    pub environments: Environments,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFlags {
    pub response_format: String,
    pub thinking: String,
    pub temperature: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDataHandling {
    pub terms_url: Option<String>,
    pub reviewed_at: Option<String>,
    pub retention_days: Option<i64>,
    pub training_use: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LegalReview {
    pub ticket: Option<String>,
    pub reviewer: Option<String>,
    pub approved_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Environments {
    pub local: EnvAllow,
    pub staging: EnvAllow,
    pub production: EnvAllow,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvAllow {
    pub allowed: bool,
    #[serde(default)]
    pub requires_api_key: bool,
}

pub fn load_manifest_from_env() -> Result<ApprovalManifest, ScanLlmError> {
    let json = std::env::var("SCAN_LLM_APPROVAL_MANIFEST_JSON").ok();
    let path = std::env::var("SCAN_LLM_APPROVAL_MANIFEST_PATH").ok();
    match (json, path) {
        (Some(j), None) => parse_manifest(&j),
        (None, Some(p)) => load_manifest_path(Path::new(&p)),
        (Some(_), Some(_)) => Err(ScanLlmError::Misconfigured(
            "provide exactly one of SCAN_LLM_APPROVAL_MANIFEST_JSON or SCAN_LLM_APPROVAL_MANIFEST_PATH"
                .into(),
        )),
        (None, None) => Err(ScanLlmError::Misconfigured(
            "approval manifest required for deepseek-v4-flash".into(),
        )),
    }
}

pub fn load_manifest_path(path: &Path) -> Result<ApprovalManifest, ScanLlmError> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        ScanLlmError::Misconfigured(format!("cannot read approval manifest: {e}"))
    })?;
    parse_manifest(&raw)
}

pub fn parse_manifest(raw: &str) -> Result<ApprovalManifest, ScanLlmError> {
    serde_json::from_str(raw).map_err(|e| {
        ScanLlmError::Misconfigured(format!("invalid approval manifest: {e}"))
    })
}

/// Fail-closed gate: environment allowed, legal non-null unexpired, data handling complete,
/// prompt hash match, request flags match.
pub fn assert_approved(
    manifest: &ApprovalManifest,
    environment: &str,
    expected_prompt_sha256_hex: &str,
) -> Result<(), ScanLlmError> {
    if manifest.model_id != "deepseek-v4-flash" || manifest.provider != "deepseek" {
        return Err(ScanLlmError::NotApproved(
            "manifest model/provider mismatch".into(),
        ));
    }
    if manifest.prompt_version != "scan-v1" {
        return Err(ScanLlmError::NotApproved("prompt version mismatch".into()));
    }
    if manifest.prompt_sha256.to_ascii_lowercase() != expected_prompt_sha256_hex.to_ascii_lowercase()
    {
        return Err(ScanLlmError::NotApproved("prompt sha256 mismatch".into()));
    }
    if manifest.request_flags.response_format != "json_object"
        || manifest.request_flags.thinking != "disabled"
        || (manifest.request_flags.temperature - 0.0).abs() > f64::EPSILON
    {
        return Err(ScanLlmError::NotApproved(
            "request flags mismatch".into(),
        ));
    }

    let env = match environment {
        "local" | "development" | "dev" => &manifest.environments.local,
        "staging" => &manifest.environments.staging,
        "production" | "prod" => &manifest.environments.production,
        other => {
            return Err(ScanLlmError::NotApproved(format!(
                "unknown environment {other}"
            )))
        }
    };
    if !env.allowed {
        return Err(ScanLlmError::NotApproved(format!(
            "environment {environment} not allowed"
        )));
    }

    // provider data handling must be complete
    let pdh = &manifest.provider_data_handling;
    if pdh.terms_url.is_none()
        || pdh.reviewed_at.is_none()
        || pdh.retention_days.is_none()
        || pdh.training_use.is_none()
    {
        return Err(ScanLlmError::NotApproved(
            "provider data handling incomplete".into(),
        ));
    }

    let legal = &manifest.legal_review;
    if legal.ticket.is_none()
        || legal.reviewer.is_none()
        || legal.approved_at.is_none()
        || legal.expires_at.is_none()
    {
        return Err(ScanLlmError::NotApproved("legal review incomplete".into()));
    }

    // expiry check (RFC3339 if parseable)
    if let Some(exp) = &legal.expires_at {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(exp) {
            if dt < chrono::Utc::now() {
                return Err(ScanLlmError::NotApproved("legal approval expired".into()));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{prompt_sha256_hex, SCAN_V1_PROMPT};

    fn deny_manifest() -> ApprovalManifest {
        let sha = prompt_sha256_hex(SCAN_V1_PROMPT);
        parse_manifest(&format!(
            r#"{{
            "provider":"deepseek",
            "modelId":"deepseek-v4-flash",
            "promptVersion":"scan-v1",
            "promptSha256":"{sha}",
            "requestFlags":{{"responseFormat":"json_object","thinking":"disabled","temperature":0.0}},
            "providerDataHandling":{{"termsUrl":null,"reviewedAt":null,"retentionDays":null,"trainingUse":null}},
            "legalReview":{{"ticket":null,"reviewer":null,"approvedAt":null,"expiresAt":null}},
            "environments":{{
                "local":{{"allowed":false,"requiresApiKey":true}},
                "staging":{{"allowed":false,"requiresApiKey":true}},
                "production":{{"allowed":false,"requiresApiKey":true}}
            }}
        }}"#
        ))
        .unwrap()
    }

    #[test]
    fn approval_gate_denies_unapproved() {
        let m = deny_manifest();
        let sha = prompt_sha256_hex(SCAN_V1_PROMPT);
        let err = assert_approved(&m, "local", &sha).unwrap_err();
        match err {
            ScanLlmError::NotApproved(_) => {}
            other => panic!("expected NotApproved, got {other}"),
        }
    }

    #[test]
    fn approval_gate_denies_missing_legal_even_if_allowed() {
        let sha = prompt_sha256_hex(SCAN_V1_PROMPT);
        let m = parse_manifest(&format!(
            r#"{{
            "provider":"deepseek",
            "modelId":"deepseek-v4-flash",
            "promptVersion":"scan-v1",
            "promptSha256":"{sha}",
            "requestFlags":{{"responseFormat":"json_object","thinking":"disabled","temperature":0.0}},
            "providerDataHandling":{{"termsUrl":"https://x","reviewedAt":"2026-01-01","retentionDays":30,"trainingUse":"none"}},
            "legalReview":{{"ticket":null,"reviewer":null,"approvedAt":null,"expiresAt":null}},
            "environments":{{
                "local":{{"allowed":true,"requiresApiKey":true}},
                "staging":{{"allowed":false,"requiresApiKey":true}},
                "production":{{"allowed":false,"requiresApiKey":true}}
            }}
        }}"#
        ))
        .unwrap();
        assert!(matches!(
            assert_approved(&m, "local", &sha),
            Err(ScanLlmError::NotApproved(_))
        ));
    }
}
