//! Hash-based deterministic stub that honors gold expect when case_id known.

use super::{CompletionRequest, CompletionResponse, EvalLlmProvider};
use crate::runner::GoldCase;
use crate::validate::{Evidence, Reason, ScanBatchInput, ScanBatchOutput, ScanResult};
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub struct StubDeterministicProvider {
    gold: HashMap<String, GoldCase>,
}

impl StubDeterministicProvider {
    pub fn new(root: &Path) -> Result<Self> {
        let mut gold = HashMap::new();
        for name in ["scan-v1.core.jsonl", "scan-v1.edge.jsonl"] {
            let p = root.join("manifests/gold").join(name);
            if !p.exists() {
                continue;
            }
            let raw = fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                let c: GoldCase = serde_json::from_str(line)?;
                gold.insert(c.case_id.clone(), c);
            }
        }
        Ok(Self { gold })
    }
}

#[async_trait]
impl EvalLlmProvider for StubDeterministicProvider {
    fn provider_id(&self) -> &'static str {
        "stub-deterministic"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let input: ScanBatchInput = serde_json::from_str(&req.user_payload_json)?;
        let out = if let Some(case) = self.gold.get(&req.case_id) {
            synthesize_from_expect(case, &input)?
        } else {
            // default all no_flag
            all_no_flag(&input)
        };
        let content_json = serde_json::to_string(&out)?;
        let tokens = (req.user_payload_json.len() as u32 / 4).max(1);
        let out_tokens = (content_json.len() as u32 / 4).max(1);
        Ok(CompletionResponse {
            content_json,
            input_tokens: tokens + 200,
            output_tokens: out_tokens,
            cache_hit_tokens: 0,
            model_id: "stub-deterministic".into(),
            provider: "stub-deterministic",
            http_calls: 1,
        })
    }
}

pub fn synthesize_from_expect(case: &GoldCase, input: &ScanBatchInput) -> Result<ScanBatchOutput> {
    let mut results = Vec::new();
    for item in &input.items {
        let idx = item.item_index.to_string();
        let decision = case
            .expect
            .decisions
            .get(&idx)
            .cloned()
            .unwrap_or_else(|| "no_flag".into());
        if decision == "flag" {
            let category = case
                .expect
                .category
                .get(&idx)
                .cloned()
                .unwrap_or_else(|| {
                    input
                        .policy
                        .concerns
                        .iter()
                        .find(|c| *c != "other")
                        .cloned()
                        .unwrap_or_else(|| "negativity".into())
                });
            let risk = case
                .expect
                .risk_at_least
                .get(&idx)
                .cloned()
                .unwrap_or_else(|| "low".into());
            let code = category_to_code(&category);
            // evidence: short contiguous substring from text
            let ev = pick_evidence(&item.text);
            let summary = if item.authorship == "amplified" {
                format!("you shared content matching {category}")
            } else {
                format!("text matches {category}")
            };
            results.push(ScanResult {
                item_index: item.item_index,
                decision: "flag".into(),
                risk,
                category: Some(category),
                confidence: 0.91,
                reasons: vec![Reason {
                    code: code.clone(),
                    summary,
                }],
                evidence: vec![Evidence {
                    text: ev,
                    supports_reason_code: code,
                }],
            });
        } else {
            results.push(ScanResult {
                item_index: item.item_index,
                decision: "no_flag".into(),
                risk: "none".into(),
                category: None,
                confidence: 0.92,
                reasons: vec![],
                evidence: vec![],
            });
        }
    }
    Ok(ScanBatchOutput {
        schema_version: "scan-output.v1".into(),
        results,
    })
}

fn all_no_flag(input: &ScanBatchInput) -> ScanBatchOutput {
    ScanBatchOutput {
        schema_version: "scan-output.v1".into(),
        results: input
            .items
            .iter()
            .map(|i| ScanResult {
                item_index: i.item_index,
                decision: "no_flag".into(),
                risk: "none".into(),
                category: None,
                confidence: 0.9,
                reasons: vec![],
                evidence: vec![],
            })
            .collect(),
    }
}

pub fn category_to_code(cat: &str) -> String {
    match cat {
        "inappropriate_language" => "targeted_insult",
        "drinking_drugs" => "risky_alcohol_or_drug_content",
        "political_takes" => "political_advocacy",
        "controversial_topics" => "polarizing_advocacy",
        "negativity" => "sustained_hostility",
        "public_image" => "admitted_misconduct",
        _ => "sustained_hostility",
    }
    .into()
}

pub fn pick_evidence(text: &str) -> String {
    let t: String = text.chars().take(40).collect();
    if t.is_empty() {
        "x".into()
    } else {
        t
    }
}

#[allow(dead_code)]
fn _path_type(_: PathBuf) {}
