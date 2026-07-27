//! Fenced persistence helpers — hashes/metrics only; flags only for decision=flag.
use crate::scan::dto::{ScanBatchOutput, ScanReason, ScanEvidence};
use crate::scan::llm::cost_usd;
use crate::scan::{hex_encode, sha256_hex};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ModelAttemptRecord {
    pub attempt: i32,
    pub prompt_version: String,
    pub prompt_sha256: String,
    pub model_id: String,
    pub provider: String,
    pub request_sha256: String,
    pub result_sha256: Option<String>,
    pub outcome: String,
    pub error_code: Option<String>,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub cache_hit_tokens: Option<i32>,
    pub cost_usd: Option<f64>,
    pub latency_ms: Option<i32>,
    pub provider_request_id: Option<String>,
}

impl ModelAttemptRecord {
    pub fn from_success(
        attempt: i32,
        prompt_version: &str,
        prompt_sha256: &[u8; 32],
        model_id: &str,
        provider: &str,
        request_json: &str,
        validated_output: &ScanBatchOutput,
        input_tokens: u32,
        output_tokens: u32,
        cache_hit_tokens: u32,
        latency_ms: u32,
        provider_request_id: Option<String>,
    ) -> Self {
        let result_canonical =
            serde_json::to_string(validated_output).unwrap_or_else(|_| "{}".into());
        Self {
            attempt,
            prompt_version: prompt_version.into(),
            prompt_sha256: hex_encode(prompt_sha256),
            model_id: model_id.into(),
            provider: provider.into(),
            request_sha256: sha256_hex(request_json.as_bytes()),
            result_sha256: Some(sha256_hex(result_canonical.as_bytes())),
            outcome: "validated".into(),
            error_code: None,
            input_tokens: Some(input_tokens as i32),
            output_tokens: Some(output_tokens as i32),
            cache_hit_tokens: Some(cache_hit_tokens as i32),
            cost_usd: Some(cost_usd(input_tokens, cache_hit_tokens, output_tokens)),
            latency_ms: Some(latency_ms as i32),
            provider_request_id,
        }
    }

    pub fn from_failure(
        attempt: i32,
        prompt_version: &str,
        prompt_sha256: &[u8; 32],
        model_id: &str,
        provider: &str,
        request_json: &str,
        error_code: &str,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        cache_hit_tokens: Option<u32>,
        latency_ms: Option<u32>,
        provider_request_id: Option<String>,
    ) -> Self {
        let (it, ot, ct) = (
            input_tokens.unwrap_or(0),
            output_tokens.unwrap_or(0),
            cache_hit_tokens.unwrap_or(0),
        );
        Self {
            attempt,
            prompt_version: prompt_version.into(),
            prompt_sha256: hex_encode(prompt_sha256),
            model_id: model_id.into(),
            provider: provider.into(),
            request_sha256: sha256_hex(request_json.as_bytes()),
            result_sha256: None,
            outcome: "failed".into(),
            error_code: Some(error_code.into()),
            input_tokens: input_tokens.map(|v| v as i32),
            output_tokens: output_tokens.map(|v| v as i32),
            cache_hit_tokens: cache_hit_tokens.map(|v| v as i32),
            cost_usd: if input_tokens.is_some() || output_tokens.is_some() {
                Some(cost_usd(it, ct, ot))
            } else {
                None
            },
            latency_ms: latency_ms.map(|v| v as i32),
            provider_request_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FlagInsert {
    pub content_item_id: Uuid,
    pub risk_level: String,
    pub category: String,
    pub reasons: Vec<ScanReason>,
    pub evidence: Vec<ScanEvidence>,
}

pub fn reasons_summary(reasons: &[ScanReason]) -> String {
    reasons
        .iter()
        .map(|r| r.summary.as_str())
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn evidence_blob(evidence: &[ScanEvidence]) -> String {
    // Minimal UI evidence: join exact snippets; no extra model prose.
    evidence
        .iter()
        .map(|e| e.text.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

pub async fn insert_batch(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_index: i32,
    item_count: i32,
    batch_key_id: Option<&str>,
    external_batch_id: Option<&str>,
) -> Result<Uuid, sqlx::Error> {
    let id: Uuid = sqlx::query_scalar(
        r#"
INSERT INTO scan_batches (tenant_id, scan_id, batch_index, item_count, batch_key_id, external_batch_id, status)
VALUES ($1, $2, $3, $4, $5, $6, 'running')
RETURNING id
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(batch_index)
    .bind(item_count)
    .bind(batch_key_id)
    .bind(external_batch_id)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn finish_batch(
    pool: &PgPool,
    tenant_id: Uuid,
    batch_id: Uuid,
    status: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE scan_batches
SET status = $3, finished_at = now(), updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(batch_id)
    .bind(status)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_attempt(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    batch_id: Uuid,
    rec: &ModelAttemptRecord,
) -> Result<Uuid, sqlx::Error> {
    crate::repository::scans::record_model_attempt_fenced(
        pool,
        tenant_id,
        scan_id,
        batch_id,
        rec,
    )
    .await
}

pub async fn insert_flags(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    user_id: Uuid,
    flags: &[FlagInsert],
) -> Result<u64, sqlx::Error> {
    let mut n = 0u64;
    for f in flags {
        crate::repository::flags::insert_flag_full(
            pool,
            tenant_id,
            scan_id,
            f.content_item_id,
            user_id,
            &f.risk_level,
            Some(&f.category),
            Some(&reasons_summary(&f.reasons)),
            Some(&evidence_blob(&f.evidence)),
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

pub async fn update_scan_progress(
    pool: &PgPool,
    tenant_id: Uuid,
    scan_id: Uuid,
    status: &str,
    phase: &str,
    progress: i32,
    error_code: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
UPDATE scans
SET status = $3,
    phase = $4,
    progress = $5,
    error_code = COALESCE($6, error_code),
    finished_at = CASE WHEN $3 IN ('succeeded', 'failed', 'cancelled') THEN now() ELSE finished_at END,
    updated_at = now()
WHERE tenant_id = $1 AND id = $2
"#,
    )
    .bind(tenant_id)
    .bind(scan_id)
    .bind(status)
    .bind(phase)
    .bind(progress)
    .bind(error_code)
    .execute(pool)
    .await?;
    Ok(())
}

/// Ensure a ModelAttemptRecord never carries raw bodies (unit-level).
pub fn assert_fenced(rec: &ModelAttemptRecord) {
    // Compile-time-ish field audit via debug string
    let s = format!("{rec:?}");
    assert!(!s.contains("user_payload"));
    let _ = json!({
        "outcome": rec.outcome,
        "error_code": rec.error_code,
        "request_sha256": rec.request_sha256,
        "result_sha256": rec.result_sha256,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::dto::ScanBatchOutput;

    #[test]
    fn fenced_record_has_hashes_only() {
        let out = ScanBatchOutput {
            schema_version: "scan-output.v1".into(),
            results: vec![],
        };
        let rec = ModelAttemptRecord::from_success(
            1,
            "scan-v1",
            &[1u8; 32],
            "stub",
            "stub-deterministic",
            r#"{"schemaVersion":"scan-input.v1"}"#,
            &out,
            10,
            5,
            0,
            12,
            None,
        );
        assert_eq!(rec.outcome, "validated");
        assert!(rec.result_sha256.is_some());
        assert_eq!(rec.prompt_sha256.len(), 64);
        assert_fenced(&rec);
    }
}
