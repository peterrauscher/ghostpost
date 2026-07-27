//! Launch-v1 metrics and threshold gates.

use crate::report::{CaseRecord, SuiteReport};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub core_recall_min: f64,
    pub core_precision_min: f64,
    pub edge_recall_min: f64,
    pub amplified_wording_min: f64,
    pub invariant_failures_max: u64,
    pub terminal_failure_max: f64,
    pub http_calls_per_batch_max: u32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            core_recall_min: 0.92,
            core_precision_min: 0.88,
            edge_recall_min: 0.85,
            amplified_wording_min: 0.95,
            invariant_failures_max: 0,
            terminal_failure_max: 0.0,
            http_calls_per_batch_max: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AggregateMetrics {
    pub core_tp: u64,
    pub core_fp: u64,
    pub core_fn: u64,
    pub core_tn: u64,
    pub edge_tp: u64,
    pub edge_fn: u64,
    pub edge_labeled_flag: u64,
    pub invariant_violations: u64,
    pub terminal_failures: u64,
    pub total_cases: u64,
    pub amplified_checked: u64,
    pub amplified_ok: u64,
    pub cost_usd: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_hit_tokens: u64,
    pub max_http_calls: u32,
}

impl AggregateMetrics {
    pub fn core_recall(&self) -> Option<f64> {
        let den = self.core_tp + self.core_fn;
        if den == 0 {
            None
        } else {
            Some(self.core_tp as f64 / den as f64)
        }
    }

    pub fn core_precision(&self) -> Option<f64> {
        let den = self.core_tp + self.core_fp;
        if den == 0 {
            None
        } else {
            Some(self.core_tp as f64 / den as f64)
        }
    }

    pub fn edge_recall(&self) -> Option<f64> {
        if self.edge_labeled_flag == 0 {
            None
        } else {
            Some(self.edge_tp as f64 / self.edge_labeled_flag as f64)
        }
    }

    pub fn amplified_accuracy(&self) -> Option<f64> {
        if self.amplified_checked == 0 {
            None
        } else {
            Some(self.amplified_ok as f64 / self.amplified_checked as f64)
        }
    }

    pub fn terminal_failure_rate(&self) -> Option<f64> {
        if self.total_cases == 0 {
            None
        } else {
            Some(self.terminal_failures as f64 / self.total_cases as f64)
        }
    }
}

pub fn accumulate(metrics: &mut AggregateMetrics, rec: &CaseRecord) {
    metrics.total_cases += 1;
    metrics.cost_usd += rec.cost_usd;
    metrics.input_tokens += rec.input_tokens as u64;
    metrics.output_tokens += rec.output_tokens as u64;
    metrics.cache_hit_tokens += rec.cache_hit_tokens as u64;
    metrics.max_http_calls = metrics.max_http_calls.max(rec.http_calls);
    if rec.invariant_failed {
        metrics.invariant_violations += 1;
    }
    if rec.terminal_failure {
        metrics.terminal_failures += 1;
    }
    if rec.amplified_checked {
        metrics.amplified_checked += 1;
        if rec.amplified_ok {
            metrics.amplified_ok += 1;
        }
    }
    let is_edge = rec.slice == "edge";
    match (rec.expected_flag, rec.predicted_flag) {
        (true, true) => {
            if is_edge {
                metrics.edge_tp += 1;
                metrics.edge_labeled_flag += 1;
            } else {
                metrics.core_tp += 1;
            }
        }
        (true, false) => {
            if is_edge {
                metrics.edge_fn += 1;
                metrics.edge_labeled_flag += 1;
            } else {
                metrics.core_fn += 1;
            }
        }
        (false, true) => {
            if !is_edge {
                metrics.core_fp += 1;
            }
        }
        (false, false) => {
            if !is_edge {
                metrics.core_tn += 1;
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateResult {
    pub passed: bool,
    pub failures: Vec<String>,
}

pub fn evaluate_gates(report: &SuiteReport, thr: &Thresholds) -> GateResult {
    let m = &report.metrics;
    let mut failures = Vec::new();

    match m.core_recall() {
        Some(v) if v + 1e-12 >= thr.core_recall_min => {}
        Some(v) => failures.push(format!(
            "core_recall {v:.4} < {}",
            thr.core_recall_min
        )),
        None => failures.push("core_recall denominator 0".into()),
    }
    match m.core_precision() {
        Some(v) if v + 1e-12 >= thr.core_precision_min => {}
        Some(v) => failures.push(format!(
            "core_precision {v:.4} < {}",
            thr.core_precision_min
        )),
        None => failures.push("core_precision denominator 0".into()),
    }
    match m.edge_recall() {
        Some(v) if v + 1e-12 >= thr.edge_recall_min => {}
        Some(v) => failures.push(format!("edge_recall {v:.4} < {}", thr.edge_recall_min)),
        None => {
            // edge may have no must-flag labels in partial suites
            if report.suite == "all" || report.suite == "edge" {
                failures.push("edge_recall denominator 0".into());
            }
        }
    }
    if m.invariant_violations > thr.invariant_failures_max {
        failures.push(format!(
            "invariant_violations {} > {}",
            m.invariant_violations, thr.invariant_failures_max
        ));
    }
    match m.terminal_failure_rate() {
        Some(v) if v <= thr.terminal_failure_max + 1e-12 => {}
        Some(v) => failures.push(format!("terminal_failure_rate {v:.4} > 0")),
        None => {}
    }
    if let Some(a) = m.amplified_accuracy() {
        if a + 1e-12 < thr.amplified_wording_min {
            failures.push(format!(
                "amplified_wording {a:.4} < {}",
                thr.amplified_wording_min
            ));
        }
    }
    if m.max_http_calls > thr.http_calls_per_batch_max {
        failures.push(format!(
            "http_calls_per_batch {} > {}",
            m.max_http_calls, thr.http_calls_per_batch_max
        ));
    }
    if report.privacy_findings > 0 {
        failures.push(format!(
            "privacy_findings {}",
            report.privacy_findings
        ));
    }

    GateResult {
        passed: failures.is_empty(),
        failures,
    }
}
