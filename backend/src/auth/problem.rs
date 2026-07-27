use crate::api::problem::Problem;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, Clone)]
pub struct AuthProblem {
    pub status: StatusCode,
    pub code: &'static str,
    pub title: &'static str,
    pub detail: String,
    pub instance: Option<String>,
}

impl AuthProblem {
    pub fn new(
        status: StatusCode,
        code: &'static str,
        title: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            status,
            code,
            title,
            detail: detail.into(),
            instance: None,
        }
    }

    pub fn with_instance(mut self, instance: impl Into<String>) -> Self {
        self.instance = Some(instance.into());
        self
    }

    pub fn unauthorized(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Unauthorized", detail)
    }

    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", "Forbidden", detail)
    }

    pub fn bad_request(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, "Bad Request", detail)
    }

    pub fn gone(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::GONE, code, "Gone", detail)
    }

    pub fn conflict(code: &'static str, title: &'static str, detail: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, title, detail)
    }

    pub fn unprocessable(code: &'static str, detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            code,
            "Unprocessable Entity",
            detail,
        )
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "INTERNAL",
            "Internal Server Error",
            detail,
        )
    }

    /// Log the full failure server-side; return a stable sanitized INTERNAL problem.
    pub fn internal_sanitized(err: impl std::fmt::Display, what: &'static str) -> Self {
        tracing::error!(error = %err, "{what}");
        Self::internal("internal error")
    }

    /// Provider outage/failure: log full error; never echo provider/secret strings.
    pub fn provider_error(err: impl std::fmt::Display) -> Self {
        tracing::error!(error = %err, "authentication provider error");
        Self::internal("authentication provider error")
    }

    /// Provider refresh failure surfaced as unauthorized (session cannot be renewed).
    pub fn provider_unauthorized(err: impl std::fmt::Display) -> Self {
        tracing::error!(error = %err, "authentication provider error");
        Self::unauthorized("authentication provider error")
    }

    /// Webhook signature/verification failure: log full error; stable FORBIDDEN detail.
    pub fn webhook_verification_failed(err: impl std::fmt::Display) -> Self {
        tracing::error!(error = %err, "webhook verification failed");
        Self::forbidden("webhook verification failed")
    }
}

impl IntoResponse for AuthProblem {
    fn into_response(self) -> Response {
        Problem {
            type_uri: format!("https://ghostpost.app/problems/{}", self.code.to_ascii_lowercase().replace('_', "-")),
            title: self.title.into(),
            status: self.status.as_u16(),
            detail: self.detail,
            instance: self.instance,
            code: Some(self.code.into()),
        }
        .into_response()
    }
}
