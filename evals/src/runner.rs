//! Gold suite runner.

use crate::manifest::{self, cost_usd};
use crate::metrics::{self, AggregateMetrics, Thresholds};
use crate::providers::{self, CompletionRequest};
use crate::report::{CaseRecord, SuiteReport};
use crate::validate::{self, ScanBatchInput, ScanBatchOutput};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Deserialize)]
pub struct GoldCase {
    pub case_id: String,
    pub slice: String,
    pub input: ScanBatchInput,
    pub expect: Expect,
    /// When true, runner expects validation failure (edge adversarial).
    #[serde(default)]
    pub expect_invariant_failure: bool,
    /// Skip decision scoring (schema/transport adversarial only).
    #[serde(default)]
    pub score_decisions: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Expect {
    #[serde(default)]
    pub decisions: BTreeMap<String, String>,
    #[serde(default)]
    pub risk_at_least: BTreeMap<String, String>,
    #[serde(default)]
    pub category: BTreeMap<String, String>,
    #[serde(default)]
    pub authorship: BTreeMap<String, String>,
}

pub struct RunOutcome {
    pub passed: bool,
    pub privacy_findings: u64,
    pub approval_blocked: bool,
    pub report_path: std::path::PathBuf,
}

pub async fn run_suite(
    root: &Path,
    suite: &str,
    provider_name: &str,
    report_path: &Path,
) -> Result<RunOutcome> {
    let mut approval_blocked = false; // set on late provider errors
    if provider_name == "deepseek-v4-flash" {
        let approval = manifest::load_approval(root)?;
        let env = std::env::var("GHOSTPOST_ENV").unwrap_or_else(|_| "local".into());
        if !manifest::deepseek_allowed(&approval, &env) {
            let report = blocked_report(suite, provider_name, root)?;
            write_report(report_path, &report)?;
            return Ok(RunOutcome {
                passed: false,
                privacy_findings: 0,
                approval_blocked: true,
                report_path: report_path.to_path_buf(),
            });
        }
    }
    let cases = load_cases(root, suite)?;
    let provider = providers::build_provider(root, provider_name)?;
    let model = manifest::load_model_manifest(root)?;
    let prompt_sha = manifest::prompt_sha256(root)?;
    let pricing = model.pricing_usd_per_million.clone();

    let mut metrics = AggregateMetrics::default();
    let mut records = Vec::new();
    let mut privacy_findings = 0u64;

    for case in &cases {
        let started = Instant::now();
        let payload = providers::serialize_input(&case.input)?;
        if payload.contains("source_id") || payload.contains("sourceId") {
            privacy_findings += 1;
        }
        let req = CompletionRequest {
            case_id: case.case_id.clone(),
            prompt_version: "scan-v1".into(),
            system_prompt_hash_hex: prompt_sha.clone(),
            user_payload_json: payload,
            temperature: 0.0,
            max_output_tokens: 4096,
        };

        let mut rec = CaseRecord {
            case_id: case.case_id.clone(),
            slice: case.slice.clone(),
            expected_flag: case_expects_any_flag(case),
            predicted_flag: false,
            passed_expect: false,
            invariant_failed: false,
            terminal_failure: false,
            error_code: None,
            result_sha256: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_hit_tokens: 0,
            cost_usd: 0.0,
            http_calls: 0,
            amplified_checked: false,
            amplified_ok: true,
            latency_ms: 0,
        };

        match provider.complete(req).await {
            Ok(resp) => {
                rec.input_tokens = resp.input_tokens;
                rec.output_tokens = resp.output_tokens;
                rec.cache_hit_tokens = resp.cache_hit_tokens;
                rec.http_calls = resp.http_calls;
                rec.cost_usd = cost_usd(
                    &pricing,
                    resp.input_tokens,
                    resp.cache_hit_tokens,
                    resp.output_tokens,
                );
                if resp.content_json.contains("DEEPSEEK_API_KEY") {
                    privacy_findings += 1;
                }
                match validate::parse_output_json(&resp.content_json) {
                    Ok(out) => match validate::validate_output(&case.input, &out) {
                        Ok(()) => {
                            if case.expect_invariant_failure {
                                // Adversarial case should have failed validation.
                                rec.invariant_failed = true;
                                rec.terminal_failure = true;
                                rec.error_code = Some("EXPECTED_INVARIANT_MISS".into());
                            } else {
                                score_case(case, &out, &mut rec);
                                if let Ok(canon) = validate::canonical_results_json(&out.results) {
                                    rec.result_sha256 =
                                        Some(manifest::sha256_hex(canon.as_bytes()));
                                }
                            }
                        }
                        Err(e) => {
                            rec.error_code = Some(e.code.to_string());
                            if case.expect_invariant_failure {
                                // Expected failure: count as pass, not a gate violation.
                                rec.passed_expect = true;
                                rec.expected_flag = false;
                                rec.predicted_flag = false;
                                rec.invariant_failed = false;
                                rec.terminal_failure = false;
                            } else {
                                rec.invariant_failed = true;
                                rec.terminal_failure = true;
                            }
                        }
                    },
                    Err(e) => {
                        rec.error_code = Some(e.code.to_string());
                        if case.expect_invariant_failure {
                            rec.passed_expect = true;
                            rec.expected_flag = false;
                            rec.predicted_flag = false;
                            rec.invariant_failed = false;
                            rec.terminal_failure = false;
                        } else {
                            rec.invariant_failed = true;
                            rec.terminal_failure = true;
                        }
                    }
                }
            }
            Err(e) => {
                let msg = format!("{e:#}");
                if msg.contains("approval manifest blocked") {
                    approval_blocked = true;
                }
                rec.error_code = Some("PROVIDER".into());
                if case.expect_invariant_failure {
                    rec.passed_expect = true;
                    rec.expected_flag = false;
                    rec.predicted_flag = false;
                    rec.invariant_failed = false;
                    rec.terminal_failure = false;
                } else {
                    rec.terminal_failure = true;
                }
            }
        }
        rec.latency_ms = started.elapsed().as_millis() as u64;
        metrics::accumulate(&mut metrics, &rec);
        records.push(rec);
    }

    let mut report = SuiteReport {
        suite: suite.into(),
        provider: provider_name.into(),
        prompt_version: "scan-v1".into(),
        prompt_sha256: prompt_sha,
        passed: false,
        core_recall: None,
        core_precision: None,
        edge_recall: None,
        invariant_violations: 0,
        cost_usd: 0.0,
        privacy_findings,
        approval_blocked,
        gate_failures: vec![],
        metrics,
        cases: records,
        case_hashes: BTreeMap::new(),
    };
    report.finalize_denominators();
    let thr = Thresholds::default();
    let gate = metrics::evaluate_gates(&report, &thr);
    report.passed = gate.passed && privacy_findings == 0 && !approval_blocked;
    report.gate_failures = gate.failures;
    write_report(report_path, &report)?;

    Ok(RunOutcome {
        passed: report.passed,
        privacy_findings,
        approval_blocked,
        report_path: report_path.to_path_buf(),
    })
}

fn blocked_report(suite: &str, provider: &str, root: &Path) -> Result<SuiteReport> {
    Ok(SuiteReport {
        suite: suite.into(),
        provider: provider.into(),
        prompt_version: "scan-v1".into(),
        prompt_sha256: manifest::prompt_sha256(root).unwrap_or_default(),
        passed: false,
        core_recall: None,
        core_precision: None,
        edge_recall: None,
        invariant_violations: 0,
        cost_usd: 0.0,
        privacy_findings: 0,
        approval_blocked: true,
        gate_failures: vec!["approval_blocked".into()],
        metrics: AggregateMetrics::default(),
        cases: vec![],
        case_hashes: BTreeMap::new(),
    })
}

fn write_report(path: &Path, report: &SuiteReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(report)?;
    fs::write(path, s)?;
    Ok(())
}

fn load_cases(root: &Path, suite: &str) -> Result<Vec<GoldCase>> {
    let files: Vec<&str> = match suite {
        "core" | "gold" => vec!["scan-v1.core.jsonl"],
        "edge" => vec!["scan-v1.edge.jsonl"],
        "all" => vec!["scan-v1.core.jsonl", "scan-v1.edge.jsonl"],
        other => bail!("unknown suite: {other}"),
    };
    let mut out = Vec::new();
    for f in files {
        let p = root.join("manifests/gold").join(f);
        let raw = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
        for (i, line) in raw.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let c: GoldCase = serde_json::from_str(line)
                .with_context(|| format!("parse {f} line {}", i + 1))?;
            out.push(c);
        }
    }
    Ok(out)
}

fn case_expects_any_flag(case: &GoldCase) -> bool {
    case.expect.decisions.values().any(|d| d == "flag")
}

fn risk_rank(r: &str) -> u8 {
    match r {
        "high" => 4,
        "medium" => 3,
        "low" => 2,
        "none" => 1,
        _ => 0,
    }
}

fn score_case(case: &GoldCase, out: &ScanBatchOutput, rec: &mut CaseRecord) {
    if case.score_decisions == Some(false) {
        rec.passed_expect = true;
        rec.expected_flag = false;
        rec.predicted_flag = false;
        return;
    }
    let by_idx: BTreeMap<u32, _> = out.results.iter().map(|r| (r.item_index, r)).collect();
    let mut ok = true;
    let mut any_pred_flag = false;
    for item in &case.input.items {
        let idx_s = item.item_index.to_string();
        let r = match by_idx.get(&item.item_index) {
            Some(r) => *r,
            None => {
                ok = false;
                continue;
            }
        };
        if r.decision == "flag" {
            any_pred_flag = true;
        }
        if let Some(exp) = case.expect.decisions.get(&idx_s) {
            if &r.decision != exp {
                ok = false;
            }
        }
        if let Some(min_risk) = case.expect.risk_at_least.get(&idx_s) {
            if r.decision == "flag" && risk_rank(&r.risk) < risk_rank(min_risk) {
                ok = false;
            }
        }
        if let Some(cat) = case.expect.category.get(&idx_s) {
            if r.category.as_deref() != Some(cat.as_str()) {
                ok = false;
            }
        }
        if item.authorship == "amplified" && r.decision == "flag" {
            rec.amplified_checked = true;
            let bad = r.reasons.iter().any(|reason| {
                let f = reason.summary.to_lowercase();
                f.contains("you wrote") || f.contains("you said") || f.contains("you posted")
            });
            if bad {
                rec.amplified_ok = false;
                ok = false;
            }
        }
    }
    rec.predicted_flag = any_pred_flag;
    rec.passed_expect = ok;
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    use scan_text::{pack_batches, Authorship, BatchHmacKey, BatchLimits, NormalizedRow};
    use uuid::Uuid;

    #[test]
    fn batching_parity() {
        let id = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let rows = vec![NormalizedRow {
            content_item_id: id,
            platform: "x".into(),
            kind: "tweet".into(),
            authorship: Authorship::Authored,
            text: "parity check hello".into(),
            content_hmac: vec![1, 2, 3, 4],
            source_logical_id: "L1".into(),
            source_order: 1,
        }];
        let key = BatchHmacKey {
            id: "local_v1".into(),
            secret: vec![9u8; 32],
        };
        let mut prompt = [0u8; 32];
        prompt[0] = 0xaa;
        let limits = BatchLimits::default();
        let b1 = pack_batches(&rows, id, &prompt, &key, &limits);
        let b2 = pack_batches(&rows, id, &prompt, &key, &limits);
        assert_eq!(b1.len(), 1);
        assert_eq!(b1[0].batch_id, b2[0].batch_id);
        assert!(b1[0].batch_id.starts_with("local_v1."));
    }
}
