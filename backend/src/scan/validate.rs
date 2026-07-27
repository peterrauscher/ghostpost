//! Semantic scan-output validator with stable error codes.
use crate::scan::dto::{ScanBatchInput, ScanBatchOutput, ScanResultItem};

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

const REASON_BY_CATEGORY: &[(&str, &[&str])] = &[
    (
        "inappropriate_language",
        &[
            "targeted_insult",
            "slur_or_dehumanization",
            "threat_or_incitement",
            "explicit_sexual_language",
        ],
    ),
    ("drinking_drugs", &["risky_alcohol_or_drug_content"]),
    ("political_takes", &["political_advocacy"]),
    ("controversial_topics", &["polarizing_advocacy"]),
    ("negativity", &["sustained_hostility"]),
    (
        "public_image",
        &["admitted_misconduct", "serious_unprofessional_conduct"],
    ),
];

const FORBIDDEN_AMPLIFIED: &[&str] = &["you wrote", "you said", "you posted"];

/// Parse model text: trim, reject fences/trailing prose, deserialize.
pub fn parse_output_json(raw: &str) -> Result<ScanBatchOutput, ValidateError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ValidateError {
            code: "SCAN_OUTPUT_JSON",
        });
    }
    if trimmed.contains("```") {
        return Err(ValidateError {
            code: "SCAN_OUTPUT_JSON",
        });
    }
    // Reject trailing non-whitespace after a top-level object by ensuring the
    // whole trimmed buffer is one JSON value.
    let value: serde_json::Value = serde_json::from_str(trimmed).map_err(|_| ValidateError {
        code: "SCAN_OUTPUT_JSON",
    })?;
    // Re-serialize check: must not leave trailing tokens — from_str already fails on trailing.
    let out: ScanBatchOutput = serde_json::from_value(value).map_err(|_| ValidateError {
        code: "SCAN_OUTPUT_SCHEMA",
    })?;
    Ok(out)
}

pub fn validate_scan_output(
    input: &ScanBatchInput,
    output: &ScanBatchOutput,
) -> Result<(), ValidateError> {
    // 1. schemaVersion
    if output.schema_version != "scan-output.v1" {
        return Err(ValidateError {
            code: "SCAN_OUTPUT_SCHEMA",
        });
    }

    // 2. index set
    let n = input.items.len();
    if output.results.len() != n {
        return Err(ValidateError {
            code: "SCAN_RESULT_INDEX_SET",
        });
    }
    let mut seen = vec![false; n];
    for r in &output.results {
        let idx = r.item_index as usize;
        if idx >= n || seen[idx] {
            return Err(ValidateError {
                code: "SCAN_RESULT_INDEX_SET",
            });
        }
        seen[idx] = true;
    }
    if seen.iter().any(|s| !*s) {
        return Err(ValidateError {
            code: "SCAN_RESULT_INDEX_SET",
        });
    }

    // Build item lookup
    let mut by_index: Vec<Option<&crate::scan::dto::ScanInputItem>> = vec![None; n];
    for it in &input.items {
        let idx = it.item_index as usize;
        if idx < n {
            by_index[idx] = Some(it);
        }
    }

    for r in &output.results {
        let item = by_index[r.item_index as usize].ok_or(ValidateError {
            code: "SCAN_RESULT_INDEX_SET",
        })?;
        validate_one(input, item, r)?;
    }
    Ok(())
}

fn validate_one(
    input: &ScanBatchInput,
    item: &crate::scan::dto::ScanInputItem,
    r: &ScanResultItem,
) -> Result<(), ValidateError> {
    match r.decision.as_str() {
        "no_flag" => {
            // 3. no_flag fields
            if r.risk != "none"
                || r.category.is_some()
                || !r.reasons.is_empty()
                || !r.evidence.is_empty()
            {
                return Err(ValidateError {
                    code: "SCAN_DECISION_FIELDS",
                });
            }
        }
        "flag" => {
            // 4. flag fields
            if !matches!(r.risk.as_str(), "low" | "medium" | "high") {
                return Err(ValidateError {
                    code: "SCAN_DECISION_FIELDS",
                });
            }
            let cat = match &r.category {
                Some(c) => c.as_str(),
                None => {
                    return Err(ValidateError {
                        code: "SCAN_DECISION_FIELDS",
                    })
                }
            };
            if !input.policy.concerns.iter().any(|c| c == cat) {
                return Err(ValidateError {
                    code: "SCAN_CATEGORY_NOT_SELECTED",
                });
            }
            if cat == "other" {
                return Err(ValidateError {
                    code: "SCAN_CATEGORY_NOT_SELECTED",
                });
            }
            if r.reasons.is_empty() || r.reasons.len() > 3 {
                return Err(ValidateError {
                    code: "SCAN_DECISION_FIELDS",
                });
            }
            if r.evidence.is_empty() || r.evidence.len() > 3 {
                return Err(ValidateError {
                    code: "SCAN_DECISION_FIELDS",
                });
            }

            // 5. reason codes belong to category
            let allowed = REASON_BY_CATEGORY
                .iter()
                .find(|(c, _)| *c == cat)
                .map(|(_, codes)| *codes)
                .unwrap_or(&[]);
            for reason in &r.reasons {
                if !allowed.contains(&reason.code.as_str()) {
                    return Err(ValidateError {
                        code: "SCAN_REASON_CATEGORY",
                    });
                }
                let summary_scalars = reason.summary.chars().count();
                if reason.summary.is_empty() || summary_scalars > 240 || reason.summary.contains('\0')
                {
                    return Err(ValidateError {
                        code: "SCAN_OUTPUT_TEXT_LIMIT",
                    });
                }
            }

            // 6. evidence
            let reason_codes: Vec<&str> = r.reasons.iter().map(|x| x.code.as_str()).collect();
            for ev in &r.evidence {
                if !reason_codes.contains(&ev.supports_reason_code.as_str()) {
                    return Err(ValidateError {
                        code: "SCAN_EVIDENCE_NOT_SUBSTRING",
                    });
                }
                let scalars = ev.text.chars().count();
                if ev.text.is_empty() || scalars > 160 || ev.text.contains('\0') {
                    return Err(ValidateError {
                        code: "SCAN_OUTPUT_TEXT_LIMIT",
                    });
                }
                if !item.text.contains(&ev.text) {
                    return Err(ValidateError {
                        code: "SCAN_EVIDENCE_NOT_SUBSTRING",
                    });
                }
            }

            // 8. amplified attribution (after confidence check we'll do it)
            if item.authorship == "amplified" {
                for reason in &r.reasons {
                    let folded = case_fold(&reason.summary);
                    for bad in FORBIDDEN_AMPLIFIED {
                        if folded.contains(bad) {
                            return Err(ValidateError {
                                code: "SCAN_AMPLIFIED_ATTRIBUTION",
                            });
                        }
                    }
                }
            }
        }
        _ => {
            return Err(ValidateError {
                code: "SCAN_DECISION_FIELDS",
            })
        }
    }

    // 7. confidence
    if !r.confidence.is_finite() || r.confidence < 0.0 || r.confidence > 1.0 {
        return Err(ValidateError {
            code: "SCAN_OUTPUT_SCHEMA",
        });
    }
    Ok(())
}

fn case_fold(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}

/// Full pipeline: parse + semantic validate.
pub fn parse_and_validate(
    input: &ScanBatchInput,
    raw: &str,
) -> Result<ScanBatchOutput, ValidateError> {
    let out = parse_output_json(raw)?;
    validate_scan_output(input, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::dto::*;

    fn sample_input(text: &str, authorship: &str) -> ScanBatchInput {
        ScanBatchInput {
            schema_version: "scan-input.v1".into(),
            policy: ScanPolicy {
                coming_up: vec!["college_apps".into()],
                concerns: vec!["negativity".into(), "inappropriate_language".into()],
            },
            items: vec![ScanInputItem {
                item_index: 0,
                platform: "x".into(),
                kind: "tweet".into(),
                authorship: authorship.into(),
                text: text.into(),
            }],
        }
    }

    fn no_flag() -> ScanBatchOutput {
        ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "no_flag".into(),
                risk: "none".into(),
                category: None,
                confidence: 0.9,
                reasons: vec![],
                evidence: vec![],
            }],
        }
    }

    #[test]
    fn accepts_no_flag() {
        let input = sample_input("hello world", "authored");
        assert!(validate_scan_output(&input, &no_flag()).is_ok());
    }

    #[test]
    fn rejects_bad_schema_version() {
        let input = sample_input("hello", "authored");
        let mut out = no_flag();
        out.schema_version = "nope".into();
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_OUTPUT_SCHEMA"
        );
    }

    #[test]
    fn rejects_index_mismatch() {
        let input = sample_input("hello", "authored");
        let mut out = no_flag();
        out.results[0].item_index = 3;
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_RESULT_INDEX_SET"
        );
    }

    #[test]
    fn rejects_no_flag_with_reasons() {
        let input = sample_input("hello", "authored");
        let mut out = no_flag();
        out.results[0].reasons = vec![ScanReason {
            code: "sustained_hostility".into(),
            summary: "x".into(),
        }];
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_DECISION_FIELDS"
        );
    }

    #[test]
    fn accepts_valid_flag() {
        let text = "you people are worthless garbage";
        let input = sample_input(text, "authored");
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "flag".into(),
                risk: "medium".into(),
                category: Some("negativity".into()),
                confidence: 0.8,
                reasons: vec![ScanReason {
                    code: "sustained_hostility".into(),
                    summary: "targeted hostility".into(),
                }],
                evidence: vec![ScanEvidence {
                    text: "worthless garbage".into(),
                    supports_reason_code: "sustained_hostility".into(),
                }],
            }],
        };
        assert!(validate_scan_output(&input, &out).is_ok());
    }

    #[test]
    fn rejects_unselected_category() {
        let text = "vote for party x now";
        let input = sample_input(text, "authored");
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "flag".into(),
                risk: "low".into(),
                category: Some("political_takes".into()),
                confidence: 0.7,
                reasons: vec![ScanReason {
                    code: "political_advocacy".into(),
                    summary: "advocacy".into(),
                }],
                evidence: vec![ScanEvidence {
                    text: "vote for party".into(),
                    supports_reason_code: "political_advocacy".into(),
                }],
            }],
        };
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_CATEGORY_NOT_SELECTED"
        );
    }

    #[test]
    fn rejects_reason_category_mismatch() {
        let text = "you people are worthless garbage";
        let input = sample_input(text, "authored");
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "flag".into(),
                risk: "medium".into(),
                category: Some("negativity".into()),
                confidence: 0.8,
                reasons: vec![ScanReason {
                    code: "targeted_insult".into(),
                    summary: "insult".into(),
                }],
                evidence: vec![ScanEvidence {
                    text: "worthless".into(),
                    supports_reason_code: "targeted_insult".into(),
                }],
            }],
        };
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_REASON_CATEGORY"
        );
    }

    #[test]
    fn rejects_evidence_not_substring() {
        let text = "hello world";
        let input = sample_input(text, "authored");
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "flag".into(),
                risk: "low".into(),
                category: Some("negativity".into()),
                confidence: 0.8,
                reasons: vec![ScanReason {
                    code: "sustained_hostility".into(),
                    summary: "hostile".into(),
                }],
                evidence: vec![ScanEvidence {
                    text: "not in source".into(),
                    supports_reason_code: "sustained_hostility".into(),
                }],
            }],
        };
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_EVIDENCE_NOT_SUBSTRING"
        );
    }

    #[test]
    fn rejects_amplified_authorship_phrasing() {
        let text = "shared garbage content here";
        let input = sample_input(text, "amplified");
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "flag".into(),
                risk: "low".into(),
                category: Some("negativity".into()),
                confidence: 0.8,
                reasons: vec![ScanReason {
                    code: "sustained_hostility".into(),
                    summary: "You wrote hostile content".into(),
                }],
                evidence: vec![ScanEvidence {
                    text: "garbage content".into(),
                    supports_reason_code: "sustained_hostility".into(),
                }],
            }],
        };
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_AMPLIFIED_ATTRIBUTION"
        );
    }

    #[test]
    fn rejects_fenced_json() {
        let input = sample_input("hello", "authored");
        let raw = "```json\n{\"schemaVersion\":\"scan-output.v1\",\"results\":[]}\n```";
        assert_eq!(parse_and_validate(&input, raw).unwrap_err().code, "SCAN_OUTPUT_JSON");
    }

    #[test]
    fn rejects_evidence_over_160_scalars() {
        let long_ev: String = "a".repeat(161);
        let text = format!("prefix {long_ev} suffix");
        let input = sample_input(&text, "authored");
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![ScanResultItem {
                item_index: 0,
                decision: "flag".into(),
                risk: "low".into(),
                category: Some("negativity".into()),
                confidence: 0.5,
                reasons: vec![ScanReason {
                    code: "sustained_hostility".into(),
                    summary: "x".into(),
                }],
                evidence: vec![ScanEvidence {
                    text: long_ev,
                    supports_reason_code: "sustained_hostility".into(),
                }],
            }],
        };
        assert_eq!(
            validate_scan_output(&input, &out).unwrap_err().code,
            "SCAN_OUTPUT_TEXT_LIMIT"
        );
    }
}
