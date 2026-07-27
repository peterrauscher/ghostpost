//! Durable `scan_posts` worker: pack → LLM → validate → fenced persist.
use crate::error::{AppError, AppResult};
use crate::repository::work_items::WorkItem;
use crate::scan::dto::{
    sort_coming_up, sort_concerns, ScanBatchInput, ScanInputItem, ScanPolicy,
};
use crate::scan::llm::{self, ScanCompletionRequest, ScanLlmError, ScanLlmProvider};
use crate::scan::persist::{
    self, FlagInsert, ModelAttemptRecord,
};
use crate::scan::validate::parse_and_validate;
use crate::scan::{scan_v1_prompt_sha256, SCAN_V1_PROMPT_VERSION};
use sha2::Digest;
use scan_text::{
    aggregate_source_results, pack_batches, Authorship, BatchHmacKey, BatchLimits, NormalizedRow,
    SegmentResult, AggregateEvidence, AggregateReason,
};
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use uuid::Uuid;

const MAX_HTTP_CALLS: u32 = 5;
const MAX_VALID_RESPONSES: u32 = 2;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanPostsPayload {
    pub scan_id: Uuid,
    #[serde(default)]
    pub archive_import_ids: Vec<Uuid>,
    #[serde(default)]
    pub import_ids: Vec<Uuid>,
}

fn worker_err(msg: impl Into<String>) -> AppError {
    AppError::Worker(msg.into())
}

fn map_authorship(raw: &str) -> Option<Authorship> {
    Authorship::from_storage(raw)
}

fn load_batch_keys() -> AppResult<Vec<BatchHmacKey>> {
    let raw = std::env::var("SCAN_BATCH_HMAC_KEYS")
        .map_err(|_| worker_err("SCAN_BATCH_HMAC_KEYS is required"))?;
    parse_batch_keys(&raw).map_err(worker_err)
}

pub fn parse_batch_keys(raw: &str) -> Result<Vec<BatchHmacKey>, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| "invalid SCAN_BATCH_HMAC_KEYS JSON".to_string())?;
    let arr = v
        .as_array()
        .ok_or_else(|| "SCAN_BATCH_HMAC_KEYS must be array".to_string())?;
    if arr.is_empty() {
        return Err("SCAN_BATCH_HMAC_KEYS empty".into());
    }
    let mut keys = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in arr {
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| "key missing id".to_string())?;
        if !seen.insert(id.to_string()) {
            return Err(format!("duplicate batch key id {id}"));
        }
        let secret_b64 = item
            .get("secret")
            .and_then(|x| x.as_str())
            .ok_or_else(|| "key missing secret".to_string())?;
        use base64::Engine;
        let secret = base64::engine::general_purpose::STANDARD
            .decode(secret_b64)
            .map_err(|_| "invalid batch key secret b64".to_string())?;
        if secret.len() != 32 {
            return Err("batch key secret must be 32 bytes".into());
        }
        keys.push(BatchHmacKey {
            id: id.to_string(),
            secret,
        });
    }
    Ok(keys)
}

fn batch_limits_from_env() -> BatchLimits {
    let mut limits = BatchLimits::default();
    if let Ok(v) = std::env::var("SCAN_BATCH_MAX_ITEMS") {
        if let Ok(n) = v.parse() {
            limits.max_items = n;
        }
    }
    if let Ok(v) = std::env::var("SCAN_BATCH_MAX_TOKENS") {
        if let Ok(n) = v.parse() {
            limits.max_tokens = n;
        }
    }
    if let Ok(v) = std::env::var("SCAN_BATCH_MAX_SCALARS") {
        if let Ok(n) = v.parse() {
            limits.max_scalars = n;
        }
    }
    limits
}

#[derive(Debug, sqlx::FromRow)]
#[allow(dead_code)]
struct ContentRow {
    id: Uuid,
    platform: String,
    kind: String,
    authorship: String,
    body: Option<String>,
    content_hmac: Option<Vec<u8>>,
    source_logical_id: Option<String>,
    source_revision_id: Option<String>, // loaded for stable order / future use
}

pub async fn mark_terminal_failure(pool: &PgPool, item: &WorkItem) -> AppResult<()> {
    let payload: ScanPostsPayload = serde_json::from_value(item.payload.clone())
        .map_err(|e| worker_err(format!("invalid scan_posts payload: {e}")))?;
    persist::update_scan_progress(
        pool,
        item.tenant_id,
        payload.scan_id,
        "failed",
        "complete",
        5,
        Some("worker_failed"),
    )
    .await?;
    Ok(())
}

pub async fn handle(
    pool: &PgPool,
    item: &WorkItem,
    provider: Arc<dyn ScanLlmProvider>,
) -> AppResult<()> {
    let payload: ScanPostsPayload = serde_json::from_value(item.payload.clone())
        .map_err(|e| worker_err(format!("invalid scan_posts payload: {e}")))?;
    let scan_id = payload.scan_id;
    let tenant_id = item.tenant_id;

    let scan = sqlx::query_as::<_, crate::repository::scans::Scan>(
        r#"
SELECT tenant_id, id, user_id, status, phase, progress, cancel_requested_at,
       lease_owner, lease_expires_at, error_code, created_at, updated_at, finished_at
FROM scans
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| worker_err("scan not found"))?;

    if scan.cancel_requested_at.is_some() {
        persist::update_scan_progress(pool, tenant_id, scan_id, "cancelled", "complete", scan.progress, None).await?;
        return Ok(());
    }

    let user_id = scan.user_id;
    persist::update_scan_progress(pool, tenant_id, scan_id, "running", "scanning", 5, None).await?;

    let mut import_ids = payload.archive_import_ids.clone();
    import_ids.extend(payload.import_ids.iter().copied());
    import_ids.sort();
    import_ids.dedup();

    let rows: Vec<ContentRow> = if import_ids.is_empty() {
        // All ready imports for user
        sqlx::query_as(
            r#"
SELECT DISTINCT c.id, c.platform, c.kind, c.authorship, c.body, c.content_hmac,
       c.source_logical_id, c.source_revision_id
FROM content_items c
JOIN archive_import_content_items l
  ON l.tenant_id = c.tenant_id AND l.content_item_id = c.id
JOIN archive_imports a
  ON a.tenant_id = l.tenant_id AND a.id = l.import_id
WHERE c.tenant_id = $1 AND c.user_id = $2 AND a.status = 'ready'
  AND c.platform IN ('reddit', 'x')
ORDER BY c.source_logical_id NULLS LAST, c.id
"#,
        )
        .bind(tenant_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as(
            r#"
SELECT DISTINCT c.id, c.platform, c.kind, c.authorship, c.body, c.content_hmac,
       c.source_logical_id, c.source_revision_id
FROM content_items c
JOIN archive_import_content_items l
  ON l.tenant_id = c.tenant_id AND l.content_item_id = c.id
WHERE c.tenant_id = $1 AND c.user_id = $2 AND l.import_id = ANY($3)
  AND c.platform IN ('reddit', 'x')
ORDER BY c.source_logical_id NULLS LAST, c.id
"#,
        )
        .bind(tenant_id)
        .bind(user_id)
        .bind(&import_ids)
        .fetch_all(pool)
        .await?
    };

    let profile = crate::repository::onboarding::get(pool, tenant_id, user_id)
        .await?
        .ok_or_else(|| worker_err("onboarding profile required for scan"))?;

    let coming_up = sort_coming_up(&profile.coming_up);
    let concerns = sort_concerns(&profile.concerns);
    if concerns.is_empty() {
        return Err(worker_err("onboarding concerns empty"));
    }

    let mut normalized = Vec::new();
    for (order, r) in rows.into_iter().enumerate() {
        let Some(auth) = map_authorship(&r.authorship) else {
            continue;
        };
        let text = r.body.unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let hmac = r.content_hmac.unwrap_or_else(|| {
            let h = sha2::Sha256::digest(text.as_bytes()).to_vec();
            h
        });
        normalized.push(NormalizedRow {
            content_item_id: r.id,
            platform: r.platform,
            kind: if r.kind.is_empty() { "post".into() } else { r.kind },
            authorship: auth,
            text,
            content_hmac: hmac,
            source_logical_id: r
                .source_logical_id
                .unwrap_or_else(|| r.id.to_string()),
            source_order: order as u64,
        });
    }

    let keys = load_batch_keys()?;
    let key = keys.last().expect("non-empty keys");
    let prompt_hash = scan_v1_prompt_sha256();
    let limits = batch_limits_from_env();
    let batches = pack_batches(&normalized, scan_id, &prompt_hash, key, &limits);

    if batches.is_empty() {
        persist::update_scan_progress(pool, tenant_id, scan_id, "succeeded", "complete", 100, None)
            .await?;
        return Ok(());
    }

    let mut all_segments: Vec<SegmentResult> = Vec::new();
    let total = batches.len().max(1);

    for (bi, batch) in batches.iter().enumerate() {
        // cancel check
        let cancel: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            r#"SELECT cancel_requested_at FROM scans WHERE tenant_id = $1 AND id = $2"#,
        )
        .bind(tenant_id)
        .bind(scan_id)
        .fetch_one(pool)
        .await?;
        if cancel.is_some() {
            persist::update_scan_progress(
                pool, tenant_id, scan_id, "cancelled", "complete",
                ((bi * 100) / total) as i32, None,
            )
            .await?;
            return Ok(());
        }

        let input = ScanBatchInput {
            schema_version: "scan-input.v1".into(),
            policy: ScanPolicy {
                coming_up: coming_up.clone(),
                concerns: concerns.clone(),
            },
            items: batch
                .items
                .iter()
                .map(|it| ScanInputItem {
                    item_index: it.item_index,
                    platform: it.platform.clone(),
                    kind: it.kind.clone(),
                    authorship: it.authorship.as_str().to_string(),
                    text: it.text.clone(),
                })
                .collect(),
        };
        let payload_json = input
            .canonical_json()
            .map_err(|e| worker_err(format!("serialize input: {e}")))?;

        let batch_db_id = persist::insert_batch(
            pool,
            tenant_id,
            scan_id,
            batch.batch_ordinal as i32,
            batch.items.len() as i32,
            Some(&batch.batch_key_id),
            Some(&batch.batch_id),
        )
        .await?;

        match run_batch_with_budget(
            pool,
            tenant_id,
            scan_id,
            batch_db_id,
            provider.as_ref(),
            &input,
            &payload_json,
            &prompt_hash,
        )
        .await
        {
            Ok(output) => {
                persist::finish_batch(pool, tenant_id, batch_db_id, "succeeded").await?;
                // Map results to segments
                let by_idx: HashMap<u32, _> =
                    batch.items.iter().map(|i| (i.item_index, i)).collect();
                for r in output.results {
                    let packed = by_idx.get(&r.item_index).ok_or_else(|| {
                        worker_err("internal itemIndex map miss")
                    })?;
                    all_segments.push(SegmentResult {
                        content_item_id: packed.content_item_id,
                        segment_ordinal: packed.segment_ordinal,
                        decision: r.decision.clone(),
                        risk: r.risk.clone(),
                        category: r.category.clone(),
                        confidence: r.confidence,
                        reasons: r
                            .reasons
                            .iter()
                            .map(|x| AggregateReason {
                                code: x.code.clone(),
                                summary: x.summary.clone(),
                            })
                            .collect(),
                        evidence: r
                            .evidence
                            .iter()
                            .map(|x| AggregateEvidence {
                                text: x.text.clone(),
                                supports_reason_code: x.supports_reason_code.clone(),
                            })
                            .collect(),
                    });
                }
            }
            Err(code) => {
                persist::finish_batch(pool, tenant_id, batch_db_id, "failed").await?;
                persist::update_scan_progress(
                    pool,
                    tenant_id,
                    scan_id,
                    "failed",
                    "complete",
                    ((bi * 100) / total) as i32,
                    Some(&code),
                )
                .await?;
                return Err(worker_err(code));
            }
        }

        let progress = (((bi + 1) * 90) / total) as i32 + 5;
        persist::update_scan_progress(
            pool,
            tenant_id,
            scan_id,
            "running",
            "scanning",
            progress.min(95),
            None,
        )
        .await?;
    }

    persist::update_scan_progress(pool, tenant_id, scan_id, "running", "flagging", 96, None)
        .await?;

    let aggregates = aggregate_source_results(&all_segments);
    let mut flags = Vec::new();
    for a in aggregates {
        if a.decision != "flag" {
            continue;
        }
        let Some(cat) = a.category.clone() else {
            continue;
        };
        let risk = match a.risk.as_str() {
            "high" | "medium" | "low" => a.risk.clone(),
            _ => continue,
        };
        flags.push(FlagInsert {
            content_item_id: a.content_item_id,
            risk_level: risk,
            category: cat,
            reasons: a
                .reasons
                .into_iter()
                .map(|r| crate::scan::dto::ScanReason {
                    code: r.code,
                    summary: r.summary,
                })
                .collect(),
            evidence: a
                .evidence
                .into_iter()
                .map(|e| crate::scan::dto::ScanEvidence {
                    text: e.text,
                    supports_reason_code: e.supports_reason_code,
                })
                .collect(),
        });
    }

    persist::insert_flags(pool, tenant_id, scan_id, user_id, &flags).await?;
    persist::update_scan_progress(pool, tenant_id, scan_id, "succeeded", "complete", 100, None)
        .await?;

    info!(
        %scan_id,
        flags = flags.len(),
        provider = provider.provider_id(),
        "scan_posts completed"
    );
    Ok(())
}

async fn run_batch_with_budget(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_id: Uuid,
    provider: &dyn ScanLlmProvider,
    input: &ScanBatchInput,
    payload_json: &str,
    prompt_hash: &[u8; 32],
) -> Result<crate::scan::dto::ScanBatchOutput, String> {
    let mut http_calls = 0u32;
    let mut valid_responses = 0u32;
    let mut attempt_no = 0i32;
    let mut repair = false;
    let mut backoff_idx = 0usize;
    const BACKOFF_MS: &[u64] = &[1000, 2000, 4000];

    loop {
        if http_calls >= MAX_HTTP_CALLS {
            return Err("SCAN_RETRY_EXHAUSTED".into());
        }
        if valid_responses >= MAX_VALID_RESPONSES {
            return Err("SCAN_RETRY_EXHAUSTED".into());
        }

        attempt_no += 1;
        http_calls += 1;
        let request_id = Uuid::new_v4();
        let req = ScanCompletionRequest {
            prompt_version: SCAN_V1_PROMPT_VERSION,
            system_prompt_hash: *prompt_hash,
            user_payload_json: payload_json.to_string(),
            temperature: 0.0,
            max_output_tokens: 4096,
            request_id,
            repair,
        };

        match provider.complete(req).await {
            Ok(resp) => {
                valid_responses += 1;
                match parse_and_validate(input, &resp.content_json) {
                    Ok(out) => {
                        let rec = ModelAttemptRecord::from_success(
                            attempt_no,
                            SCAN_V1_PROMPT_VERSION,
                            prompt_hash,
                            &resp.model_id,
                            resp.provider,
                            payload_json,
                            &out,
                            resp.input_tokens,
                            resp.output_tokens,
                            resp.cache_hit_tokens,
                            resp.latency_ms,
                            resp.provider_request_id,
                        );
                        let _ = persist::record_attempt(pool, tenant_id, scan_id, batch_id, &rec)
                            .await
                            .map_err(|e| e.to_string())?;
                        // Drop resp.content_json by moving out
                        return Ok(out);
                    }
                    Err(ve) => {
                        let rec = ModelAttemptRecord::from_failure(
                            attempt_no,
                            SCAN_V1_PROMPT_VERSION,
                            prompt_hash,
                            &resp.model_id,
                            resp.provider,
                            payload_json,
                            ve.code,
                            Some(resp.input_tokens),
                            Some(resp.output_tokens),
                            Some(resp.cache_hit_tokens),
                            Some(resp.latency_ms),
                            resp.provider_request_id,
                        );
                        let _ = persist::record_attempt(pool, tenant_id, scan_id, batch_id, &rec)
                            .await
                            .map_err(|e| e.to_string())?;
                        // one repair allowed if we still have budget for another valid response
                        if valid_responses < MAX_VALID_RESPONSES && http_calls < MAX_HTTP_CALLS {
                            repair = true;
                            continue;
                        }
                        return Err(ve.code.to_string());
                    }
                }
            }
            Err(err) => {
                if matches!(err, ScanLlmError::PrivacyGuard) {
                    let rec = ModelAttemptRecord::from_failure(
                        attempt_no,
                        SCAN_V1_PROMPT_VERSION,
                        prompt_hash,
                        provider.provider_id(),
                        provider.provider_id(),
                        payload_json,
                        err.code(),
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    let _ = persist::record_attempt(pool, tenant_id, scan_id, batch_id, &rec).await;
                    return Err(err.code().into());
                }
                if matches!(err, ScanLlmError::NotApproved(_)) {
                    let rec = ModelAttemptRecord::from_failure(
                        attempt_no,
                        SCAN_V1_PROMPT_VERSION,
                        prompt_hash,
                        provider.provider_id(),
                        provider.provider_id(),
                        payload_json,
                        err.code(),
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    let _ = persist::record_attempt(pool, tenant_id, scan_id, batch_id, &rec).await;
                    return Err(err.code().into());
                }
                if err.is_retryable_transport() && http_calls < MAX_HTTP_CALLS {
                    let mut delay = BACKOFF_MS.get(backoff_idx).copied().unwrap_or(4000);
                    if let ScanLlmError::RateLimited {
                        retry_after_ms: Some(ms),
                    } = &err
                    {
                        delay = (*ms).min(delay.saturating_mul(2)).max(1);
                    }
                    // jitter up to 250ms
                    delay += (request_id.as_u128() % 250) as u64;
                    backoff_idx = (backoff_idx + 1).min(BACKOFF_MS.len().saturating_sub(1));
                    let rec = ModelAttemptRecord::from_failure(
                        attempt_no,
                        SCAN_V1_PROMPT_VERSION,
                        prompt_hash,
                        provider.provider_id(),
                        provider.provider_id(),
                        payload_json,
                        err.code(),
                        None,
                        None,
                        None,
                        None,
                        None,
                    );
                    let _ = persist::record_attempt(pool, tenant_id, scan_id, batch_id, &rec).await;
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                // terminal 4xx or exhausted
                let rec = ModelAttemptRecord::from_failure(
                    attempt_no,
                    SCAN_V1_PROMPT_VERSION,
                    prompt_hash,
                    provider.provider_id(),
                    provider.provider_id(),
                    payload_json,
                    err.code(),
                    None,
                    None,
                    None,
                    None,
                    None,
                );
                let _ = persist::record_attempt(pool, tenant_id, scan_id, batch_id, &rec).await;
                return Err(err.code().into());
            }
        }
    }
}

/// Build provider for worker; fails closed for deepseek without approval.
pub fn build_scan_provider() -> AppResult<Arc<dyn ScanLlmProvider>> {
    llm::build_provider_from_env().map_err(|e| worker_err(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keys_ok() {
        let raw = r#"[{"id":"local_v1","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#;
        let keys = parse_batch_keys(raw).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].secret.len(), 32);
    }

    #[test]
    fn parse_keys_reject_dup() {
        let raw = r#"[{"id":"a","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},{"id":"a","secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="}]"#;
        assert!(parse_batch_keys(raw).is_err());
    }
}
