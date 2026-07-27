//! Eval report serialization.

use crate::metrics::AggregateMetrics;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRecord {
    pub case_id: String,
    pub slice: String,
    pub expected_flag: bool,
    pub predicted_flag: bool,
    pub passed_expect: bool,
    pub invariant_failed: bool,
    pub terminal_failure: bool,
    pub error_code: Option<String>,
    pub result_sha256: Option<String>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_hit_tokens: u32,
    pub cost_usd: f64,
    pub http_calls: u32,
    pub amplified_checked: bool,
    pub amplified_ok: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    pub suite: String,
    pub provider: String,
    pub prompt_version: String,
    pub prompt_sha256: String,
    pub passed: bool,
    pub core_recall: Option<f64>,
    pub core_precision: Option<f64>,
    pub edge_recall: Option<f64>,
    pub invariant_violations: u64,
    pub cost_usd: f64,
    pub privacy_findings: u64,
    pub approval_blocked: bool,
    pub gate_failures: Vec<String>,
    pub metrics: AggregateMetrics,
    pub cases: Vec<CaseRecord>,
    /// Per-case result hashes for equal-hash compare.
    pub case_hashes: BTreeMap<String, String>,
}

impl SuiteReport {
    pub fn finalize_denominators(&mut self) {
        self.core_recall = self.metrics.core_recall();
        self.core_precision = self.metrics.core_precision();
        self.edge_recall = self.metrics.edge_recall();
        self.invariant_violations = self.metrics.invariant_violations;
        self.cost_usd = self.metrics.cost_usd;
        self.case_hashes = self
            .cases
            .iter()
            .filter_map(|c| {
                c.result_sha256
                    .as_ref()
                    .map(|h| (c.case_id.clone(), h.clone()))
            })
            .collect();
    }
}
