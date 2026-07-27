//! Flag list/detail/review-actions (Plan 006). No social HTTP.
use crate::api::idempotency::{self, IdemClaim};
use crate::api::problem::Problem;
use crate::api::product_dto::{
    flag_row_to_response, FlaggedPostResponse, ReviewActionRequest, ReviewFilters,
    ReviewListResponse,
};
use crate::api::router::AppState;
use crate::api::scans::Mutation;
use crate::auth::extract::AuthSession;
use crate::domain::entitlements::EntitlementService;
use crate::repository::flags;
use crate::repository::scans;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
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
    tracing::error!(error = %err, "product flags db error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Flag operation failed",
        None,
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagListQuery {
    pub scan_id: Option<Uuid>,
    pub risk: Option<String>,
    pub status: Option<String>,
    pub cursor: Option<Uuid>,
    pub limit: Option<i64>,
}

fn map_action(action: &str) -> Result<(&'static str, &'static str), Problem> {
    match action {
        "resolve" => Ok(("resolve", "resolved")),
        "delete_local" => Ok(("delete_local", "deleted")),
        "archive" => Ok(("archive", "archived")),
        "keep" => Ok(("keep", "kept")),
        "delete" => Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unknown action; use delete_local",
            Some("UNKNOWN_ACTION"),
        )),
        _ => Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unknown action",
            Some("UNKNOWN_ACTION"),
        )),
    }
}

async fn resolve_scan_id(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    user_id: Uuid,
    scan_id: Option<Uuid>,
) -> ApiResult<Uuid> {
    if let Some(id) = scan_id {
        let scan = scans::get_scan(pool, tenant_id, id)
            .await
            .map_err(db_problem)?
            .ok_or_else(|| {
                problem(
                    StatusCode::NOT_FOUND,
                    "Not found",
                    Some("RESOURCE_NOT_FOUND"),
                )
            })?;
        if scan.user_id != user_id {
            return Err(problem(
                StatusCode::NOT_FOUND,
                "Not found",
                Some("RESOURCE_NOT_FOUND"),
            ));
        }
        return Ok(scan.id);
    }
    scans::latest_succeeded_scan(pool, tenant_id, user_id)
        .await
        .map_err(db_problem)?
        .map(|s| s.id)
        .ok_or_else(|| {
            problem(
                StatusCode::NOT_FOUND,
                "No succeeded scan",
                Some("RESOURCE_NOT_FOUND"),
            )
        })
}

pub async fn list_flags(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
    Query(q): Query<FlagListQuery>,
) -> ApiResult<Json<ReviewListResponse>> {
    EntitlementService::require_review_access(&state.pool, session.tenant_id, session.user_id)
        .await
        .map_err(|e| match e {
            crate::domain::entitlements::EntitlementDeny::Required => problem(
                StatusCode::FORBIDDEN,
                "Review access required",
                Some("ENTITLEMENT_REQUIRED"),
            ),
            crate::domain::entitlements::EntitlementDeny::Db(err) => db_problem(err),
        })?;

    let risk = match q.risk.as_deref() {
        None | Some("all") => None,
        Some("high") | Some("medium") | Some("low") => q.risk.as_deref(),
        Some(_) => {
            return Err(problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Invalid risk filter",
                Some("INVALID_RISK"),
            ));
        }
    };
    let status = q.status.as_deref().unwrap_or("open");
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let scan_id =
        resolve_scan_id(&state.pool, session.tenant_id, session.user_id, q.scan_id).await?;

    let mut rows = flags::list_flags(
        &state.pool,
        session.tenant_id,
        scan_id,
        risk,
        status,
        q.cursor,
        limit + 1,
    )
    .await
    .map_err(db_problem)?;

    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.pop();
    }
    let next_cursor = if has_more {
        rows.last().map(|r| r.id)
    } else {
        None
    };

    let (all, high, medium, low) =
        flags::count_flags_by_risk(&state.pool, session.tenant_id, scan_id, status)
            .await
            .map_err(db_problem)?;

    Ok(Json(ReviewListResponse {
        posts: rows.iter().map(flag_row_to_response).collect(),
        filters: ReviewFilters {
            all,
            high,
            medium,
            low,
        },
        next_cursor,
        has_more,
    }))
}

pub async fn get_flag(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<FlaggedPostResponse>> {
    EntitlementService::require_review_access(&state.pool, session.tenant_id, session.user_id)
        .await
        .map_err(|e| match e {
            crate::domain::entitlements::EntitlementDeny::Required => problem(
                StatusCode::FORBIDDEN,
                "Review access required",
                Some("ENTITLEMENT_REQUIRED"),
            ),
            crate::domain::entitlements::EntitlementDeny::Db(err) => db_problem(err),
        })?;

    let row = flags::get_flag_detail(&state.pool, session.tenant_id, id)
        .await
        .map_err(db_problem)?
        .ok_or_else(|| {
            problem(
                StatusCode::NOT_FOUND,
                "Not found",
                Some("RESOURCE_NOT_FOUND"),
            )
        })?;
    if row.user_id != session.user_id {
        return Err(problem(
            StatusCode::NOT_FOUND,
            "Not found",
            Some("RESOURCE_NOT_FOUND"),
        ));
    }
    Ok(Json(flag_row_to_response(&row)))
}

pub async fn review_action(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Mutation { session, key }: Mutation,
    Json(req): Json<ReviewActionRequest>,
) -> ApiResult<Response> {
    let (action, new_status) = map_action(&req.action)?;

    EntitlementService::require_review_access(&state.pool, session.tenant_id, session.user_id)
        .await
        .map_err(|e| match e {
            crate::domain::entitlements::EntitlementDeny::Required => problem(
                StatusCode::FORBIDDEN,
                "Review access required",
                Some("ENTITLEMENT_REQUIRED"),
            ),
            crate::domain::entitlements::EntitlementDeny::Db(err) => db_problem(err),
        })?;

    let hash_input = json!({
        "action": action,
        "expectedStatus": req.expected_status,
        "flagId": id,
    });
    let hash = Sha256::digest(
        serde_json::to_vec(&hash_input)
            .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "encode failed", None))?,
    )
    .to_vec();

    let mut tx = state.pool.begin().await.map_err(db_problem)?;

    match idempotency::claim(
        &mut tx,
        session.tenant_id,
        "flag_review_action",
        &key,
        &hash,
        60,
    )
    .await
    .map_err(db_problem)?
    {
        IdemClaim::Fresh => {}
        other => {
            tx.commit().await.map_err(db_problem)?;
            let (status, body) = idempotency::claim_problem(other)?;
            return Ok((
                StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                Json(body),
            )
                .into_response());
        }
    }

    let flag = flags::lock_flag_tx(&mut tx, session.tenant_id, id)
        .await
        .map_err(db_problem)?
        .ok_or_else(|| {
            problem(
                StatusCode::NOT_FOUND,
                "Not found",
                Some("RESOURCE_NOT_FOUND"),
            )
        })?;

    if flag.user_id != session.user_id {
        return Err(problem(
            StatusCode::NOT_FOUND,
            "Not found",
            Some("RESOURCE_NOT_FOUND"),
        ));
    }

    if flag.review_status != "open" {
        return Err(problem(
            StatusCode::CONFLICT,
            "Flag already reviewed",
            Some("FLAG_ALREADY_REVIEWED"),
        ));
    }

    if let Some(expected) = &req.expected_status {
        if expected != "open" && expected != &flag.review_status {
            return Err(problem(
                StatusCode::CONFLICT,
                "expectedStatus mismatch",
                Some("EXPECTED_STATUS_MISMATCH"),
            ));
        }
    }

    let _updated = flags::apply_review_tx(
        &mut tx,
        session.tenant_id,
        id,
        session.user_id,
        action,
        new_status,
    )
    .await
    .map_err(db_problem)?;

    if action == "delete_local" {
        // Local-only: mark content pending + hide flags. No HTTP clients.
        flags::mark_delete_local_tx(&mut tx, session.tenant_id, flag.content_item_id)
            .await
            .map_err(db_problem)?;

        sqlx::query(
            r#"
INSERT INTO work_items (
  tenant_id, subject_user_id, kind, payload, dedupe_key, priority, max_attempts
)
VALUES (
  $1, $2, 'purge_content',
  jsonb_build_object('contentItemId', $3::text),
  $3::text,
  0, 5
)
ON CONFLICT (tenant_id, kind, dedupe_key) WHERE dedupe_key IS NOT NULL DO NOTHING
"#,
        )
        .bind(session.tenant_id)
        .bind(session.user_id)
        .bind(flag.content_item_id)
        .execute(&mut *tx)
        .await
        .map_err(db_problem)?;
    }

    // Build response snapshot (may be hidden after delete_local — still return disposition).
    let detail = sqlx::query_as::<_, flags::FlagListRow>(
        r#"
SELECT f.tenant_id, f.id, f.scan_id, f.content_item_id, f.user_id, f.risk_level, f.category,
       f.reason_summary, f.evidence, f.review_status, f.closed_at, f.created_at, f.updated_at,
       f.hidden_at, c.platform, c.body, c.created_at AS content_created_at
FROM flagged_posts f
JOIN content_items c
  ON c.tenant_id = f.tenant_id AND c.id = f.content_item_id
WHERE f.tenant_id = $1 AND f.id = $2
"#,
    )
    .bind(session.tenant_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_problem)?;

    let response = flag_row_to_response(&detail);
    let body = serde_json::to_value(&response)
        .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "encode failed", None))?;

    idempotency::complete(
        &mut tx,
        session.tenant_id,
        "flag_review_action",
        &key,
        200,
        &body,
        Some(id),
    )
    .await
    .map_err(db_problem)?;

    tx.commit().await.map_err(db_problem)?;
    Ok((StatusCode::OK, Json(body)).into_response())
}
