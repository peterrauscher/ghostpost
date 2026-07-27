use crate::error::AppError;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    #[serde(rename = "type")]
    pub type_uri: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl Problem {
    pub fn from_app_error(err: &AppError, sanitize: bool) -> Self {
        Self {
            type_uri: err.type_uri().into(),
            title: err.title().into(),
            status: err.status_code(),
            detail: err.public_detail(sanitize),
            instance: None,
            code: None,
        }
    }

    pub fn service_unavailable(detail: impl Into<String>) -> Self {
        Self {
            type_uri: "https://ghostpost.app/problems/not-ready".into(),
            title: "Service Unavailable".into(),
            status: 503,
            detail: detail.into(),
            instance: Some("/health/ready".into()),
            code: Some("NOT_READY".into()),
        }
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (
            status,
            [(header::CONTENT_TYPE, "application/problem+json")],
            Json(self),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn problem_json_shape() {
        let problem = Problem::from_app_error(&AppError::NotFound, true);
        let value = serde_json::to_value(&problem).unwrap();
        assert_eq!(value["status"], 404);
        assert_eq!(value["title"], "Not found");
        assert!(value.get("type").is_some());
        assert!(value.get("detail").is_some());
    }

    #[test]
    fn database_detail_sanitized_for_json() {
        let err = AppError::Database(sqlx::Error::PoolTimedOut);
        let problem = Problem::from_app_error(&err, true);
        assert_eq!(problem.detail, "Database readiness check failed");
        assert!(!problem.detail.to_lowercase().contains("pool"));
    }
}
