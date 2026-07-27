use crate::api::problem::Problem;
use crate::api::router::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use tracing::warn;

#[derive(Serialize)]
struct LiveBody {
    status: &'static str,
}

#[derive(Serialize)]
struct ReadyBody {
    status: &'static str,
}

pub async fn live() -> impl IntoResponse {
    (StatusCode::OK, Json(LiveBody { status: "live" }))
}

pub async fn ready(State(state): State<AppState>) -> Response {
    if state.restore_replay_pending {
        return Problem::service_unavailable("Restore replay pending; service not ready")
            .into_response();
    }

    // For --role all the worker is initialized in-process; api-only only needs DB.
    let _ = state.worker_ready;

    match sqlx::query_scalar!("SELECT 1")
        .fetch_one(&state.pool)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(ReadyBody { status: "ready" })).into_response(),
        Err(err) => {
            warn!(error = %err, "readiness probe failed");
            let sanitize = std::env::var("LOG_FORMAT")
                .map(|v| v.eq_ignore_ascii_case("json"))
                .unwrap_or(false);
            Problem::service_unavailable(if sanitize {
                "Database readiness check failed".into()
            } else {
                format!("Database readiness check failed: {err}")
            })
            .into_response()
        }
    }
}
