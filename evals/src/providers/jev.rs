//! Live Jev (TypeSafe) classifier via `scan-jev`; requires `TYPESAFE_API_KEY`.
//!
//! Gold cases hold one or two posts, but production batches up to 8 per request
//! and Jev's accuracy shifts with batch content. Each case is therefore padded
//! with deterministic neighbor posts from the rest of the gold suite up to
//! `SCAN_JEV_MAX_POSTS` (default 8); only the case's own items are returned.
use super::{CompletionRequest, CompletionResponse, EvalLlmProvider};
use crate::runner::GoldCase;
use crate::validate::ScanBatchInput;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use scan_jev::{Call, Item, JevError, JevLimits, SystemOne, SystemOneResponse};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

const API_BASE: &str = "https://api.typesafe.ai";

struct Curl {
    api_key: String,
    api_base: String,
}

#[async_trait]
impl SystemOne for Curl {
    async fn evaluate(&self, request_json: &str) -> Result<Call, JevError> {
        let url = format!("{}/v1/systemone", self.api_base.trim_end_matches('/'));
        let auth = format!("Authorization: Bearer {}", self.api_key);
        let body = request_json.to_owned();
        let started = Instant::now();
        let out = tokio::task::spawn_blocking(move || -> std::io::Result<std::process::Output> {
            let mut child = Command::new("curl")
                .args(["-sS", "-X", "POST", &url, "-H", &auth, "-H", "Content-Type: application/json",
                       "--data-binary", "@-", "--max-time", "60", "-w", "\n%{http_code}"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;
            child.stdin.take().expect("piped").write_all(body.as_bytes())?;
            child.wait_with_output()
        })
        .await
        .map_err(|_| JevError::Transport)?
        .map_err(|_| JevError::Transport)?;
        if !out.status.success() {
            return Err(if out.status.code() == Some(28) { JevError::Timeout } else { JevError::Transport });
        }
        let raw = String::from_utf8_lossy(&out.stdout);
        let (payload, status) = raw.rsplit_once('\n').ok_or(JevError::Transport)?;
        match status.trim().parse::<u16>().map_err(|_| JevError::Transport)? {
            200..=299 => {}
            429 => return Err(JevError::RateLimited { retry_after_ms: None }),
            500..=599 => return Err(JevError::Unavailable),
            400..=499 => return Err(JevError::Rejected),
            _ => return Err(JevError::Transport),
        }
        let response: SystemOneResponse = serde_json::from_str(payload).map_err(|_| JevError::Unavailable)?;
        Ok(Call {
            response,
            response_sha256: hex::encode(Sha256::digest(payload.as_bytes())),
            latency_ms: started.elapsed().as_millis() as u32,
        })
    }
}

pub struct JevProvider {
    client: Curl,
    limits: JevLimits,
    /// Neighbor pool: every gold post text, in file order.
    pool: Vec<String>,
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> Result<T> {
    match std::env::var(name) {
        Ok(v) => v.parse().map_err(|_| anyhow::anyhow!("invalid {name}={v}")),
        Err(_) => Ok(default),
    }
}

impl JevProvider {
    pub fn new(root: &Path) -> Result<Self> {
        let api_key = std::env::var("TYPESAFE_API_KEY").unwrap_or_default();
        if api_key.trim().is_empty() {
            bail!("TYPESAFE_API_KEY required for jev");
        }
        let d = JevLimits::default();
        let limits = JevLimits {
            max_posts: env_or("SCAN_JEV_MAX_POSTS", d.max_posts)?,
            escalate_at: env_or("SCAN_JEV_ESCALATE_AT", d.escalate_at)?,
            flag_at: env_or("SCAN_JEV_FLAG_AT", d.flag_at)?,
            ..d
        };
        let mut pool = Vec::new();
        for name in ["scan-v1.core.jsonl", "scan-v1.edge.jsonl"] {
            let p = root.join("manifests/gold").join(name);
            let raw = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                let c: GoldCase = serde_json::from_str(line)?;
                pool.extend(c.input.items.into_iter().map(|i| scan_text::minimize_text(&i.text)));
            }
        }
        Ok(Self {
            client: Curl {
                api_key,
                api_base: std::env::var("TYPESAFE_API_BASE").unwrap_or_else(|_| API_BASE.into()),
            },
            limits,
            pool,
        })
    }

    /// Deterministic neighbors: walk the pool from an offset derived from the case id.
    fn neighbors(&self, case_id: &str, own: &[String], n: usize) -> Vec<String> {
        if self.pool.is_empty() || n == 0 {
            return vec![];
        }
        let start = Sha256::digest(case_id.as_bytes())[..8].iter().fold(0usize, |a, b| (a << 8) | *b as usize) % self.pool.len();
        (0..self.pool.len())
            .map(|k| &self.pool[(start + k) % self.pool.len()])
            .filter(|t| !t.is_empty() && !own.contains(t))
            .take(n)
            .cloned()
            .collect()
    }
}

#[async_trait]
impl EvalLlmProvider for JevProvider {
    fn provider_id(&self) -> &'static str {
        "jev"
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse> {
        let input: ScanBatchInput = serde_json::from_str(&req.user_payload_json)?;
        let own: Vec<String> = input.items.iter().map(|i| i.text.clone()).collect();
        let pad = self.limits.max_posts.saturating_sub(own.len());
        let neighbors = self.neighbors(&req.case_id, &own, pad);
        let mut items: Vec<Item> = input.items.iter().map(|i| Item { item_index: i.item_index, text: &i.text }).collect();
        items.extend(neighbors.iter().enumerate().map(|(j, t)| Item { item_index: 10_000 + j as u32, text: t }));

        let mut attempt = 0;
        let classified = loop {
            attempt += 1;
            match scan_jev::classify(&self.client, &input.policy.concerns, &input.policy.coming_up, &items, &self.limits).await {
                Ok(c) => break c,
                Err(JevError::RateLimited { .. } | JevError::Unavailable | JevError::Timeout | JevError::Transport) if attempt < 3 => {
                    tokio::time::sleep(std::time::Duration::from_millis(1000 * attempt)).await;
                }
                Err(e) => bail!("jev classify failed: {e:?}"),
            }
        };
        let usage = classified.usage();
        let own_results: Vec<_> = classified.results.into_iter().filter(|r| r.item_index < 10_000).collect();
        Ok(CompletionResponse {
            content_json: scan_jev::output_json(&own_results),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_hit_tokens: 0,
            model_id: classified.calls.first().map(|c| c.model.clone()).unwrap_or_else(|| scan_jev::MODEL_ID.into()),
            provider: "jev",
            http_calls: classified.calls.len() as u32,
            cost_usd: Some(scan_jev::cost_usd(&usage)),
        })
    }
}
