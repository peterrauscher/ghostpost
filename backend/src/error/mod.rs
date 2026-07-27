use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(String),
    #[error("worker error: {0}")]
    Worker(String),
    #[error("shutdown error: {0}")]
    Shutdown(String),
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl AppError {
    pub fn status_code(&self) -> u16 {
        match self {
            Self::Config(_) | Self::Migration(_) | Self::Worker(_) | Self::Shutdown(_) => 500,
            Self::Database(_) => 503,
            Self::NotFound => 404,
            Self::Conflict(_) => 409,
            Self::InvalidInput(_) => 400,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Config(_) => "Configuration error",
            Self::Database(_) => "Database unavailable",
            Self::Migration(_) => "Migration error",
            Self::Worker(_) => "Worker error",
            Self::Shutdown(_) => "Shutdown error",
            Self::NotFound => "Not found",
            Self::Conflict(_) => "Conflict",
            Self::InvalidInput(_) => "Invalid input",
        }
    }

    pub fn type_uri(&self) -> &'static str {
        match self {
            Self::Config(_) => "https://ghostpost.app/problems/config",
            Self::Database(_) => "https://ghostpost.app/problems/database",
            Self::Migration(_) => "https://ghostpost.app/problems/migration",
            Self::Worker(_) => "https://ghostpost.app/problems/worker",
            Self::Shutdown(_) => "https://ghostpost.app/problems/shutdown",
            Self::NotFound => "https://ghostpost.app/problems/not-found",
            Self::Conflict(_) => "https://ghostpost.app/problems/conflict",
            Self::InvalidInput(_) => "https://ghostpost.app/problems/invalid-input",
        }
    }

    /// Public detail for Problem+JSON. Never include internal DB errors when
    /// producing JSON production responses.
    pub fn public_detail(&self, sanitize: bool) -> String {
        match self {
            Self::Database(_) if sanitize => "Database readiness check failed".into(),
            Self::Database(err) => err.to_string(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_taxonomy_status_codes() {
        assert_eq!(AppError::NotFound.status_code(), 404);
        assert_eq!(AppError::Conflict("x".into()).status_code(), 409);
        assert_eq!(AppError::InvalidInput("x".into()).status_code(), 400);
        assert_eq!(
            AppError::Database(sqlx::Error::PoolTimedOut).status_code(),
            503
        );
    }
}
