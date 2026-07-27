//! Dashboard + read-only entitlement routes (Plan 006).
use crate::api::problem::Problem;
use crate::api::product_dto::{
    flag_row_to_response, focus_label, focus_symbol, gauge_for_level, DashboardResponse, FocusArea,
    RiskSummary, UserBrief, EntitlementCaps, EntitlementResponse,
};
use crate::api::router::AppState;
use crate::auth::extract::AuthSession;
use crate::domain::entitlements::EntitlementService;
use crate::repository::flags;
use crate::repository::scans;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;

type ApiResult<T> = Result<T, Problem>;

fn problem(status: StatusCode, detail: &str, code: Option<&str>) -> Problem {
    Problem {
        type_uri: "https://ghostpost.app/problems/product".into(),
        title: status
            .canonical_reason()
            .unwrap_or("Request failed")
            .into(),
        status: status.as_u16(),
        detail: detail.into(),
        instance: None,
        code: code.map(str::to_owned),
    }
}

fn db_problem(err: sqlx::Error) -> Problem {
    tracing::error!(error = %err, "product profile db error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Operation failed",
        None,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardQuery {
    pub scan_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntitlementQuery {
    pub scan_id: Option<Uuid>,
}

pub async fn dashboard(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
    Query(q): Query<DashboardQuery>,
) -> ApiResult<Json<DashboardResponse>> {
    let user = crate::auth::store::get_user(&state.pool, session.tenant_id, session.user_id)
        .await
        .map_err(db_problem)?
        .ok_or_else(|| {
            problem(
                StatusCode::UNAUTHORIZED,
                "Session user missing",
                Some("UNAUTHORIZED"),
            )
        })?;

    let scan = if let Some(scan_id) = q.scan_id {
        let s = scans::get_scan(&state.pool, session.tenant_id, scan_id)
            .await
            .map_err(db_problem)?
            .ok_or_else(|| {
                problem(
                    StatusCode::NOT_FOUND,
                    "Not found",
                    Some("RESOURCE_NOT_FOUND"),
                )
            })?;
        if s.user_id != session.user_id {
            return Err(problem(
                StatusCode::NOT_FOUND,
                "Not found",
                Some("RESOURCE_NOT_FOUND"),
            ));
        }
        Some(s)
    } else {
        scans::latest_succeeded_scan(&state.pool, session.tenant_id, session.user_id)
            .await
            .map_err(db_problem)?
    };

    let (flagged_count, top_risk, preview) = if let Some(scan) = &scan {
        let (count, top) = flags::highest_open_risk(&state.pool, session.tenant_id, scan.id)
            .await
            .map_err(db_problem)?;
        let preview = flags::list_open_preview(&state.pool, session.tenant_id, scan.id, 3)
            .await
            .map_err(db_problem)?;
        (count, top, preview)
    } else {
        (0, None, vec![])
    };

    let level = if flagged_count == 0 {
        "none".to_string()
    } else {
        top_risk.unwrap_or_else(|| "low".into())
    };

    let audit_headline = if flagged_count == 0 {
        "no flagged posts found".into()
    } else if flagged_count == 1 {
        "we found 1 post that could raise red flags".into()
    } else {
        format!("we found {flagged_count} posts that could raise red flags")
    };

    let onboarding =
        crate::auth::store::get_onboarding(&state.pool, session.tenant_id, session.user_id)
            .await
            .map_err(db_problem)?;
    let focus_areas: Vec<FocusArea> = onboarding
        .map(|o| {
            o.coming_up
                .into_iter()
                .map(|id| FocusArea {
                    label: focus_label(&id),
                    symbol: focus_symbol(&id).into(),
                    id,
                })
                .collect()
        })
        .unwrap_or_default();

    let name = user
        .display_name
        .clone()
        .unwrap_or_else(|| "User".into());
    let greeting_name = user
        .greeting_name
        .clone()
        .unwrap_or_else(|| name.to_ascii_lowercase());

    Ok(Json(DashboardResponse {
        user: UserBrief {
            id: user.id,
            name,
            greeting_name,
            avatar_url: user.avatar_url,
        },
        audit_headline,
        focus_areas,
        flagged_preview: preview.iter().map(flag_row_to_response).collect(),
        risk: RiskSummary {
            level: level.clone(),
            flagged_count,
            gauge_sweep: gauge_for_level(&level),
        },
    }))
}

pub async fn entitlement(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
    Query(q): Query<EntitlementQuery>,
) -> ApiResult<Json<EntitlementResponse>> {
    if let Some(scan_id) = q.scan_id {
        let scan = scans::get_scan(&state.pool, session.tenant_id, scan_id)
            .await
            .map_err(db_problem)?
            .ok_or_else(|| {
                problem(
                    StatusCode::NOT_FOUND,
                    "Not found",
                    Some("RESOURCE_NOT_FOUND"),
                )
            })?;
        if scan.user_id != session.user_id {
            return Err(problem(
                StatusCode::NOT_FOUND,
                "Not found",
                Some("RESOURCE_NOT_FOUND"),
            ));
        }
    }

    let effective = EntitlementService::resolve(&state.pool, session.tenant_id, session.user_id)
        .await
        .map_err(db_problem)?
        .ok_or_else(|| {
            // Launch cohort should always have free_beta after signup; still return a stable inactive shape.
            problem(
                StatusCode::NOT_FOUND,
                "No entitlement grant",
                Some("RESOURCE_NOT_FOUND"),
            )
        })?;

    Ok(Json(EntitlementResponse {
        status: effective.status,
        product_id: effective.product_id,
        valid_from: effective.valid_from,
        expires_at: effective.expires_at,
        scan_id: q.scan_id,
        capabilities: EntitlementCaps {
            review_access: effective.review_access,
            rescans_remaining: effective.rescans_remaining,
            platform_limit: effective.platform_limit,
        },
    }))
}
