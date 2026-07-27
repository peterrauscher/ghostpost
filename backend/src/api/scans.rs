//! Product scan routes: create, current, detail poll.
use crate::api::idempotency::{self, IdemClaim};
use crate::api::problem::Problem;
use crate::api::product_dto::{scan_to_response, CreateScanRequest, ScanStatusResponse};
use crate::api::router::AppState;
use crate::auth::extract::{enforce_web_mutation_guards, AuthSession};
use crate::auth::types::AppSession;
use crate::domain::entitlements::{EntitlementDeny, EntitlementService};
use crate::repository::scans;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
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
    tracing::error!(error = %err, "product scan db error");
    problem(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Scan operation failed",
        None,
    )
}

pub struct Mutation {
    pub session: AppSession,
    pub key: String,
}

impl FromRequestParts<AppState> for Mutation {
    type Rejection = crate::auth::problem::AuthProblem;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthSession(session) = AuthSession::from_request_parts(parts, state).await?;
        enforce_web_mutation_guards(parts, state, &session).await?;
        let key = parts
            .headers
            .get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|v| {
                !v.is_empty()
                    && v.len() <= 128
                    && v.bytes().all(|b| (0x20..=0x7E).contains(&b))
            })
            .ok_or_else(|| {
                crate::auth::problem::AuthProblem::bad_request(
                    "IDEMPOTENCY_KEY_REQUIRED",
                    "missing or invalid Idempotency-Key",
                )
            })?
            .to_owned();
        Ok(Self { session, key })
    }
}

pub async fn create_scan(
    State(state): State<AppState>,
    Mutation { session, key }: Mutation,
    Json(req): Json<CreateScanRequest>,
) -> ApiResult<Response> {
    let mut ids = req.archive_import_ids;
    if ids.is_empty() {
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "archiveImportIds is required and must be non-empty",
            Some("INVALID_ARCHIVE_IMPORT_IDS"),
        ));
    }
    let unique: BTreeSet<Uuid> = ids.iter().copied().collect();
    if unique.len() != ids.len() {
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "archiveImportIds must be unique",
            Some("INVALID_ARCHIVE_IMPORT_IDS"),
        ));
    }
    ids = unique.into_iter().collect();

    let hash = {
        let canonical = serde_json::to_vec(&json!({ "archiveImportIds": ids }))
            .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "encode failed", None))?;
        Sha256::digest(&canonical).to_vec()
    };

    let mut tx = state.pool.begin().await.map_err(db_problem)?;

    match idempotency::claim(&mut tx, session.tenant_id, "scan_create", &key, &hash, 60)
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

    let entitlement = match EntitlementService::resolve_tx(&mut tx, session.tenant_id, session.user_id)
        .await
        .map_err(db_problem)?
    {
        Some(e) => e,
        None => {
            return Err(problem(
                StatusCode::FORBIDDEN,
                "Active entitlement required",
                Some("ENTITLEMENT_REQUIRED"),
            ));
        }
    };
    if let Some(0) = entitlement.rescans_remaining {
        return Err(problem(
            StatusCode::FORBIDDEN,
            "Rescan quota exhausted",
            Some("ENTITLEMENT_REQUIRED"),
        ));
    }

    // Lock and validate exact ready imports owned by tenant/user.
    let rows = sqlx::query_as::<_, (Uuid, String, String)>(
        r#"
SELECT id, platform, status
FROM archive_imports
WHERE tenant_id = $1 AND user_id = $2 AND id = ANY($3)
FOR UPDATE
"#,
    )
    .bind(session.tenant_id)
    .bind(session.user_id)
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(db_problem)?;

    if rows.len() != ids.len() {
        return Err(problem(
            StatusCode::NOT_FOUND,
            "One or more archive imports were not found",
            Some("RESOURCE_NOT_FOUND"),
        ));
    }
    for (_, _, status) in &rows {
        if status != "ready" {
            return Err(problem(
                StatusCode::UNPROCESSABLE_ENTITY,
                "All archive imports must be ready",
                Some("IMPORTS_NOT_READY"),
            ));
        }
    }
    let platforms: BTreeSet<&str> = rows.iter().map(|(_, p, _)| p.as_str()).collect();
    if platforms.len() as i32 > entitlement.platform_limit {
        return Err(problem(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Platform limit exceeded for entitlement",
            Some("PLATFORM_LIMIT_EXCEEDED"),
        ));
    }

    if scans::has_active_scan_tx(&mut tx, session.tenant_id, session.user_id)
        .await
        .map_err(db_problem)?
    {
        return Err(problem(
            StatusCode::CONFLICT,
            "A scan is already running",
            Some("SCAN_ALREADY_RUNNING"),
        ));
    }

    let scan = scans::create_scan_tx(&mut *tx, session.tenant_id, session.user_id)
        .await
        .map_err(|err| {
            // unique active index race
            if let sqlx::Error::Database(db) = &err {
                if db.constraint() == Some("scans_one_active_per_user_idx") {
                    return problem(
                        StatusCode::CONFLICT,
                        "A scan is already running",
                        Some("SCAN_ALREADY_RUNNING"),
                    );
                }
            }
            db_problem(err)
        })?;

    scans::link_scan_imports_tx(&mut tx, session.tenant_id, scan.id, &ids)
        .await
        .map_err(db_problem)?;

    if let Err(e) = EntitlementService::consume_rescan(
        &mut tx,
        &entitlement,
        session.tenant_id,
        session.user_id,
        scan.id,
    )
    .await
    {
        match e {
            EntitlementDeny::Required => {
                return Err(problem(
                    StatusCode::FORBIDDEN,
                    "Rescan quota exhausted",
                    Some("ENTITLEMENT_REQUIRED"),
                ));
            }
            EntitlementDeny::Db(err) => return Err(db_problem(err)),
        }
    }

    let payload = json!({
        "scanId": scan.id,
        "archiveImportIds": ids,
    });
    // Enqueue inside same transaction via raw SQL so scan create is atomic.
    sqlx::query(
        r#"
INSERT INTO work_items (
  tenant_id, subject_user_id, kind, payload, dedupe_key, priority, max_attempts
)
VALUES ($1, $2, 'scan_posts', $3, $4, 0, 5)
"#,
    )
    .bind(session.tenant_id)
    .bind(session.user_id)
    .bind(payload)
    .bind(format!("scan_posts:{}", scan.id))
    .execute(&mut *tx)
    .await
    .map_err(db_problem)?;

    let response = scan_to_response(&scan);
    let body = serde_json::to_value(&response)
        .map_err(|_| problem(StatusCode::INTERNAL_SERVER_ERROR, "encode failed", None))?;
    idempotency::complete(
        &mut tx,
        session.tenant_id,
        "scan_create",
        &key,
        202,
        &body,
        Some(scan.id),
    )
    .await
    .map_err(db_problem)?;

    tx.commit().await.map_err(db_problem)?;

    Ok((StatusCode::ACCEPTED, Json(body)).into_response())
}

pub async fn get_current(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
) -> ApiResult<Json<ScanStatusResponse>> {
    let scan = scans::get_current_scan(&state.pool, session.tenant_id, session.user_id)
        .await
        .map_err(db_problem)?
        .ok_or_else(|| {
            problem(
                StatusCode::NOT_FOUND,
                "No scans found",
                Some("RESOURCE_NOT_FOUND"),
            )
        })?;
    Ok(Json(scan_to_response(&scan)))
}

pub async fn get_one(
    State(state): State<AppState>,
    AuthSession(session): AuthSession,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ScanStatusResponse>> {
    let scan = scans::get_scan(&state.pool, session.tenant_id, id)
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
        // tenant-scoped already; still hide other users in same tenant if any
        return Err(problem(
            StatusCode::NOT_FOUND,
            "Not found",
            Some("RESOURCE_NOT_FOUND"),
        ));
    }
    Ok(Json(scan_to_response(&scan)))
}

