//! Semantic scan-output.v1 validator (mirrors backend/src/scan/validate.rs).
//! Stable error codes; never includes provider content in errors.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const ERR_JSON: &str = "SCAN_OUTPUT_JSON";
pub const ERR_SCHEMA: &str = "SCAN_OUTPUT_SCHEMA";
pub const ERR_INDEX: &str = "SCAN_RESULT_INDEX_SET";
pub const ERR_DECISION: &str = "SCAN_DECISION_FIELDS";
pub const ERR_CATEGORY: &str = "SCAN_CATEGORY_NOT_SELECTED";
pub const ERR_REASON: &str = "SCAN_REASON_CATEGORY";
pub const ERR_EVIDENCE: &str = "SCAN_EVIDENCE_NOT_SUBSTRING";
pub const ERR_AMPLIFIED: &str = "SCAN_AMPLIFIED_ATTRIBUTION";
pub const ERR_LIMIT: &str = "SCAN_OUTPUT_TEXT_LIMIT";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanBatchInput {
    pub schema_version: String,
    pub policy: Policy,
    pub items: Vec<ScanItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Policy {
    pub coming_up: Vec<String>,
    pub concerns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanItem {
    pub item_index: u32,
    pub platform: String,
    pub kind: String,
    pub authorship: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanBatchOutput {
    pub schema_version: String,
    pub results: Vec<ScanResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanResult {
    pub item_index: u32,
    pub decision: String,
    pub risk: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub reasons: Vec<Reason>,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reason {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub text: String,
    pub supports_reason_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateError {
    pub code: &'static str,
}

impl std::fmt::Display for ValidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code)
    }
}

impl std::error::Error for ValidateError {}

fn reason_codes_for_category(cat: &str) -> &'static [&'static str] {
    match cat {
        "inappropriate_language" => &[
            "targeted_insult",
            "slur_or_dehumanization",
            "threat_or_incitement",
            "explicit_sexual_language",
        ],
        "drinking_drugs" => &["risky_alcohol_or_drug_content"],
        "political_takes" => &["political_advocacy"],
        "controversial_topics" => &["polarizing_advocacy"],
        "negativity" => &["sustained_hostility"],
        "public_image" => &["admitted_misconduct", "serious_unprofessional_conduct"],
        _ => &[],
    }
}

/// Parse raw model text (trim whitespace; reject fences / trailing prose).
pub fn parse_output_json(raw: &str) -> Result<ScanBatchOutput, ValidateError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ValidateError { code: ERR_JSON });
    }
    if trimmed.contains("```") {
        return Err(ValidateError { code: ERR_JSON });
    }
    if trimmed.chars().any(|c| c == '\0') {
        return Err(ValidateError { code: ERR_JSON });
    }
    // Reject trailing non-whitespace after one JSON value.
    let end = match first_json_end(trimmed) {
        Some(e) => e,
        None => return Err(ValidateError { code: ERR_JSON }),
    };
    let remaining = &trimmed[end..];
    if remaining.chars().any(|c| !c.is_whitespace()) {
        return Err(ValidateError { code: ERR_JSON });
    }
    let slice = &trimmed[..end];
    serde_json::from_str(slice).map_err(|_| ValidateError { code: ERR_SCHEMA })
}

/// Byte end offset of the first top-level JSON object/array value.
fn first_json_end(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= b.len() || (b[i] != b'{' && b[i] != b'[') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' => in_str = true,
            b'{' | b'[' => depth += 1,
            b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
/// Full semantic validation against the batch input.
pub fn validate_output(
    input: &ScanBatchInput,
    output: &ScanBatchOutput,
) -> Result<(), ValidateError> {
    if output.schema_version != "scan-output.v1" {
        return Err(ValidateError { code: ERR_SCHEMA });
    }
    let n = input.items.len();
    if output.results.len() != n {
        return Err(ValidateError { code: ERR_INDEX });
    }
    let mut seen = BTreeSet::new();
    for r in &output.results {
        if r.item_index as usize >= n || !seen.insert(r.item_index) {
            return Err(ValidateError { code: ERR_INDEX });
        }
    }
    if seen.len() != n {
        return Err(ValidateError { code: ERR_INDEX });
    }
    let concerns: BTreeSet<&str> = input.policy.concerns.iter().map(|s| s.as_str()).collect();
    let items_by_idx: BTreeMap<u32, &ScanItem> =
        input.items.iter().map(|i| (i.item_index, i)).collect();

    for r in &output.results {
        if !r.confidence.is_finite() || !(0.0..=1.0).contains(&r.confidence) {
            return Err(ValidateError { code: ERR_SCHEMA });
        }
        match r.decision.as_str() {
            "no_flag" => {
                if r.risk != "none"
                    || r.category.is_some()
                    || !r.reasons.is_empty()
                    || !r.evidence.is_empty()
                {
                    return Err(ValidateError { code: ERR_DECISION });
                }
            }
            "flag" => {
                if !matches!(r.risk.as_str(), "low" | "medium" | "high") {
                    return Err(ValidateError { code: ERR_DECISION });
                }
                let cat = r
                    .category
                    .as_deref()
                    .ok_or(ValidateError { code: ERR_DECISION })?;
                if cat == "other" || !concerns.contains(cat) {
                    return Err(ValidateError { code: ERR_CATEGORY });
                }
                if r.reasons.is_empty() || r.reasons.len() > 3 {
                    return Err(ValidateError { code: ERR_DECISION });
                }
                if r.evidence.is_empty() || r.evidence.len() > 3 {
                    return Err(ValidateError { code: ERR_DECISION });
                }
                let allowed = reason_codes_for_category(cat);
                for reason in &r.reasons {
                    if !allowed.contains(&reason.code.as_str()) {
                        return Err(ValidateError { code: ERR_REASON });
                    }
                    let sc = reason.summary.chars().count();
                    if sc == 0 || sc > 240 {
                        return Err(ValidateError { code: ERR_LIMIT });
                    }
                }
                let reason_codes: BTreeSet<&str> =
                    r.reasons.iter().map(|x| x.code.as_str()).collect();
                let item = items_by_idx
                    .get(&r.item_index)
                    .ok_or(ValidateError { code: ERR_INDEX })?;
                for ev in &r.evidence {
                    if !reason_codes.contains(ev.supports_reason_code.as_str()) {
                        return Err(ValidateError { code: ERR_EVIDENCE });
                    }
                    let ec = ev.text.chars().count();
                    if ec == 0 || ec > 160 {
                        return Err(ValidateError { code: ERR_LIMIT });
                    }
                    if !item.text.contains(&ev.text) {
                        return Err(ValidateError { code: ERR_EVIDENCE });
                    }
                }
                if item.authorship == "amplified" {
                    for reason in &r.reasons {
                        let folded = reason.summary.to_lowercase();
                        if folded.contains("you wrote")
                            || folded.contains("you said")
                            || folded.contains("you posted")
                        {
                            return Err(ValidateError { code: ERR_AMPLIFIED });
                        }
                    }
                }
            }
            _ => return Err(ValidateError { code: ERR_DECISION }),
        }
    }
    Ok(())
}

/// Canonical JSON of results[] with sorted keys for hashing.
pub fn canonical_results_json(results: &[ScanResult]) -> Result<String, serde_json::Error> {
    let mut ordered = results.to_vec();
    ordered.sort_by_key(|r| r.item_index);
    let v = serde_json::to_value(&ordered)?;
    Ok(canonical_value(&v))
}

fn canonical_value(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let mut parts = Vec::new();
            for k in keys {
                parts.push(format!(
                    "{}:{}",
                    serde_json::to_string(&k).unwrap(),
                    canonical_value(&map[&k])
                ));
            }
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(arr) => {
            let parts: Vec<_> = arr.iter().map(canonical_value).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input(text: &str, authorship: &str, concerns: &[&str]) -> ScanBatchInput {
        ScanBatchInput {
            schema_version: "scan-input.v1".into(),
            policy: Policy {
                coming_up: vec!["college_apps".into()],
                concerns: concerns.iter().map(|s| (*s).to_string()).collect(),
            },
            items: vec![ScanItem {
                item_index: 0,
                platform: "x".into(),
                kind: "tweet".into(),
                authorship: authorship.into(),
                text: text.into(),
            }],
        }
    }

    #[test]
    fn no_flag_ok() {
        let input = sample_input("nice day", "authored", &["negativity"]);
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResult {
                item_index: 0,
                decision: "no_flag".into(),
                risk: "none".into(),
                category: None,
                confidence: 0.9,
                reasons: vec![],
                evidence: vec![],
            }],
        };
        assert!(validate_output(&input, &out).is_ok());
    }

    #[test]
    fn amplified_authorship_phrase_fails() {
        let text = "these people are garbage always";
        let input = sample_input(text, "amplified", &["negativity"]);
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResult {
                item_index: 0,
                decision: "flag".into(),
                risk: "medium".into(),
                category: Some("negativity".into()),
                confidence: 0.8,
                reasons: vec![Reason {
                    code: "sustained_hostility".into(),
                    summary: "you wrote hostile content".into(),
                }],
                evidence: vec![Evidence {
                    text: "garbage".into(),
                    supports_reason_code: "sustained_hostility".into(),
                }],
            }],
        };
        let err = validate_output(&input, &out).unwrap_err();
        assert_eq!(err.code, ERR_AMPLIFIED);
    }
}
