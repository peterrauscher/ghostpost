//! Scan batch input/output DTOs (camelCase, deny_unknown_fields).
use serde::{Deserialize, Serialize};

pub const COMING_UP_ORDER: &[&str] = &[
    "rush",
    "college_apps",
    "job_interviews",
    "friends_family",
    "just_concerned",
    "something_else",
];

pub const CONCERNS_ORDER: &[&str] = &[
    "inappropriate_language",
    "drinking_drugs",
    "political_takes",
    "controversial_topics",
    "negativity",
    "public_image",
    "other",
];

pub const CATEGORY_ORDER: &[&str] = &[
    "inappropriate_language",
    "drinking_drugs",
    "political_takes",
    "controversial_topics",
    "negativity",
    "public_image",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanBatchInput {
    pub schema_version: String,
    pub policy: ScanPolicy,
    pub items: Vec<ScanInputItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanPolicy {
    pub coming_up: Vec<String>,
    pub concerns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanInputItem {
    pub item_index: u32,
    pub platform: String,
    pub kind: String,
    pub authorship: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanBatchOutput {
    pub schema_version: String,
    pub results: Vec<ScanResultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanResultItem {
    pub item_index: u32,
    pub decision: String,
    pub risk: String,
    pub category: Option<String>,
    pub confidence: f64,
    pub reasons: Vec<ScanReason>,
    pub evidence: Vec<ScanEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanReason {
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanEvidence {
    pub text: String,
    pub supports_reason_code: String,
}

/// Sort policy arrays into canonical enum order; drop unknowns.
pub fn sort_coming_up(values: &[String]) -> Vec<String> {
    sort_by_order(values, COMING_UP_ORDER)
}

pub fn sort_concerns(values: &[String]) -> Vec<String> {
    sort_by_order(values, CONCERNS_ORDER)
}

fn sort_by_order(values: &[String], order: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for &o in order {
        if values.iter().any(|v| v == o) && !out.iter().any(|v: &String| v == o) {
            out.push(o.to_string());
        }
    }
    out
}

impl ScanBatchInput {
    pub fn canonical_json(&self) -> Result<String, serde_json::Error> {
        // serde_json preserves field order from struct definition with default serializer
        serde_json::to_string(self)
    }
}
