//! Shared camelCase product API DTOs (Plan 006).
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::repository::flags::FlagListRow;
use crate::repository::scans::Scan;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatusResponse {
    pub id: Uuid,
    pub status: String,
    pub phase: String,
    pub progress: f64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
}

pub fn scan_message(status: &str, phase: &str, progress01: f64) -> String {
    if status == "failed" {
        return "scan failed".into();
    }
    if status == "cancelled" {
        return "scan cancelled".into();
    }
    if status == "succeeded" || phase == "complete" {
        return "scan complete".into();
    }
    if phase == "flagging" || progress01 >= 0.67 {
        return "flagging...".into();
    }
    if phase == "scanning" || progress01 >= 0.34 {
        return "scanning...".into();
    }
    "connecting...".into()
}

pub fn scan_to_response(scan: &Scan) -> ScanStatusResponse {
    let mut progress = (scan.progress as f64 / 100.0).clamp(0.0, 1.0);
    let mut phase = scan.phase.clone();
    if scan.status == "succeeded" {
        phase = "complete".into();
        progress = 1.0;
    } else if phase == "complete" && scan.status != "succeeded" {
        phase = match scan.status.as_str() {
            "queued" => "connecting",
            "running" => "scanning",
            _ => "scanning",
        }
        .into();
    }
    let error_code = if scan.status == "failed" {
        scan.error_code.clone()
    } else {
        None
    };
    let finished_at = if matches!(
        scan.status.as_str(),
        "succeeded" | "failed" | "cancelled"
    ) {
        scan.finished_at
    } else {
        None
    };
    ScanStatusResponse {
        id: scan.id,
        status: scan.status.clone(),
        phase: phase.clone(),
        progress,
        message: scan_message(&scan.status, &phase, progress),
        error_code,
        created_at: scan.created_at,
        finished_at,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScanRequest {
    pub archive_import_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserBrief {
    pub id: Uuid,
    pub name: String,
    pub greeting_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusArea {
    pub id: String,
    pub label: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskSummary {
    pub level: String,
    pub flagged_count: i64,
    pub gauge_sweep: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardResponse {
    pub user: UserBrief,
    pub audit_headline: String,
    pub focus_areas: Vec<FocusArea>,
    pub flagged_preview: Vec<FlaggedPostResponse>,
    pub risk: RiskSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlaggedPostResponse {
    pub id: Uuid,
    pub platform: String,
    pub platform_label: String,
    pub date: String,
    pub quote: String,
    pub risk: String,
    pub category: String,
    pub tags: Vec<String>,
    pub explanation: String,
    pub why_flagged: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFilters {
    pub all: i64,
    pub high: i64,
    pub medium: i64,
    pub low: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewListResponse {
    pub posts: Vec<FlaggedPostResponse>,
    pub filters: ReviewFilters,
    pub next_cursor: Option<Uuid>,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewActionRequest {
    pub action: String,
    #[serde(default)]
    pub expected_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementResponse {
    pub status: String,
    pub product_id: String,
    pub valid_from: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scan_id: Option<Uuid>,
    pub capabilities: EntitlementCaps,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementCaps {
    pub review_access: bool,
    pub rescans_remaining: Option<i32>,
    pub platform_limit: i32,
}

pub fn platform_label(platform: &str) -> &'static str {
    match platform {
        "x" => "X",
        "reddit" => "Reddit",
        "facebook" => "Facebook",
        "instagram" => "Instagram",
        "tiktok" => "TikTok",
        _ => "Unknown",
    }
}

pub fn format_flag_date(dt: DateTime<Utc>) -> String {
    // %-d is Linux-only; use day without zero-pad manually for portability.
    let day = dt.format("%d").to_string();
    let day = day.trim_start_matches('0');
    let day = if day.is_empty() { "0" } else { day };
    format!("{} {}, {}", dt.format("%b"), day, dt.format("%Y"))
}

pub fn api_review_status(db_status: &str) -> &str {
    match db_status {
        "closed" => "resolved",
        other => other,
    }
}

pub fn flag_row_to_response(row: &FlagListRow) -> FlaggedPostResponse {
    let quote = row
        .body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
    let explanation = row
        .reason_summary
        .clone()
        .unwrap_or_else(|| "Flagged for review.".into());
    let category = row.category.clone().unwrap_or_else(|| "other".into());
    let why = format!(
        "Matches your {} review concern.",
        category.replace('_', "-")
    );
    let tags: Vec<String> = row
        .evidence
        .as_deref()
        .map(|e| {
            e.split('|')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .take(3)
                .map(|s| s.chars().take(40).collect::<String>())
                .collect()
        })
        .unwrap_or_default();
    FlaggedPostResponse {
        id: row.id,
        platform: row.platform.clone(),
        platform_label: platform_label(&row.platform).into(),
        date: format_flag_date(row.content_created_at),
        quote,
        risk: row.risk_level.clone(),
        category,
        tags,
        explanation,
        why_flagged: why,
        status: api_review_status(&row.review_status).into(),
    }
}

pub fn gauge_for_level(level: &str) -> i32 {
    match level {
        "none" => 0,
        "high" => 320,
        "medium" => 245,
        "low" => 180,
        _ => 0,
    }
}

pub fn focus_symbol(id: &str) -> &'static str {
    match id {
        "rush" => "⌂",
        "college_apps" => "🎓",
        "job_interviews" => "💼",
        "friends_family" => "👥",
        "just_concerned" => "🔍",
        "something_else" => "•",
        _ => "•",
    }
}

pub fn focus_label(id: &str) -> String {
    crate::auth::catalog::platforms_catalog()
        .coming_up_options
        .iter()
        .find(|o| o.id == id)
        .map(|o| o.label.to_ascii_lowercase())
        .unwrap_or_else(|| id.replace('_', " "))
}
