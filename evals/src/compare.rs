//! Report comparison with equal-hash winner rule.

use crate::report::SuiteReport;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub winner: String,
    pub reason: String,
    pub prompt_sha256: Option<String>,
    pub baseline_core_recall: Option<f64>,
    pub candidate_core_recall: Option<f64>,
    pub baseline_cost_usd: f64,
    pub candidate_cost_usd: f64,
    pub equal_hash: bool,
    pub message: String,
}

pub fn compare_reports(baseline: &Path, candidate: &Path) -> Result<CompareResult> {
    let b: SuiteReport = serde_json::from_str(
        &fs::read_to_string(baseline).with_context(|| format!("read {}", baseline.display()))?,
    )?;
    let c: SuiteReport = serde_json::from_str(
        &fs::read_to_string(candidate).with_context(|| format!("read {}", candidate.display()))?,
    )?;

    let equal_hash = !b.case_hashes.is_empty()
        && b.case_hashes.len() == c.case_hashes.len()
        && b.case_hashes
            .iter()
            .all(|(k, v)| c.case_hashes.get(k) == Some(v));

    if equal_hash {
        let (winner, sha) = if b.prompt_sha256 <= c.prompt_sha256 {
            ("baseline", b.prompt_sha256.clone())
        } else {
            ("candidate", c.prompt_sha256.clone())
        };
        return Ok(CompareResult {
            winner: winner.into(),
            reason: "equal-hash-prompt-sha256".into(),
            prompt_sha256: Some(sha),
            baseline_core_recall: b.core_recall,
            candidate_core_recall: c.core_recall,
            baseline_cost_usd: b.cost_usd,
            candidate_cost_usd: c.cost_usd,
            equal_hash: true,
            message: "TIE (equal-hash)".into(),
        });
    }

    let br = b.core_recall.unwrap_or(0.0);
    let cr = c.core_recall.unwrap_or(0.0);
    if (br - cr).abs() > 1e-12 {
        let winner = if cr > br { "candidate" } else { "baseline" };
        return Ok(CompareResult {
            winner: winner.into(),
            reason: "higher-core-recall".into(),
            prompt_sha256: None,
            baseline_core_recall: b.core_recall,
            candidate_core_recall: c.core_recall,
            baseline_cost_usd: b.cost_usd,
            candidate_cost_usd: c.cost_usd,
            equal_hash: false,
            message: format!("winner={winner} by core recall"),
        });
    }
    if (b.cost_usd - c.cost_usd).abs() > 1e-12 {
        let winner = if c.cost_usd < b.cost_usd {
            "candidate"
        } else {
            "baseline"
        };
        return Ok(CompareResult {
            winner: winner.into(),
            reason: "lower-cost-usd".into(),
            prompt_sha256: None,
            baseline_core_recall: b.core_recall,
            candidate_core_recall: c.core_recall,
            baseline_cost_usd: b.cost_usd,
            candidate_cost_usd: c.cost_usd,
            equal_hash: false,
            message: format!("winner={winner} by cost"),
        });
    }
    let (winner, sha) = if b.prompt_sha256 <= c.prompt_sha256 {
        ("baseline", b.prompt_sha256.clone())
    } else {
        ("candidate", c.prompt_sha256.clone())
    };
    Ok(CompareResult {
        winner: winner.into(),
        reason: "lower-prompt-sha256".into(),
        prompt_sha256: Some(sha),
        baseline_core_recall: b.core_recall,
        candidate_core_recall: c.core_recall,
        baseline_cost_usd: b.cost_usd,
        candidate_cost_usd: c.cost_usd,
        equal_hash: false,
        message: format!("winner={winner} by prompt sha"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::AggregateMetrics;
    use std::collections::BTreeMap;
    use std::io::Write;

    fn sample_report(sha: &str, hash_a: &str) -> SuiteReport {
        let mut case_hashes = BTreeMap::new();
        case_hashes.insert("c1".into(), hash_a.into());
        SuiteReport {
            suite: "all".into(),
            provider: "replay-fixture".into(),
            prompt_version: "scan-v1".into(),
            prompt_sha256: sha.into(),
            passed: true,
            core_recall: Some(0.95),
            core_precision: Some(0.9),
            edge_recall: Some(0.9),
            invariant_violations: 0,
            cost_usd: 0.0,
            privacy_findings: 0,
            approval_blocked: false,
            gate_failures: vec![],
            metrics: AggregateMetrics::default(),
            cases: vec![],
            case_hashes,
        }
    }

    #[test]
    fn equal_hash_picks_lexicographically_smaller_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let bpath = dir.path().join("b.json");
        let cpath = dir.path().join("c.json");
        let mut bf = fs::File::create(&bpath).unwrap();
        let mut cf = fs::File::create(&cpath).unwrap();
        writeln!(
            bf,
            "{}",
            serde_json::to_string(&sample_report("ffffff", "abc")).unwrap()
        )
        .unwrap();
        writeln!(
            cf,
            "{}",
            serde_json::to_string(&sample_report("000001", "abc")).unwrap()
        )
        .unwrap();
        let r = compare_reports(&bpath, &cpath).unwrap();
        assert!(r.equal_hash);
        assert_eq!(r.reason, "equal-hash-prompt-sha256");
        assert_eq!(r.winner, "candidate");
        assert_eq!(r.message, "TIE (equal-hash)");
    }
}
