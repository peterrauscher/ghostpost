use crate::config::LogFormat;
use crate::error::{AppError, AppResult};
use std::env;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_from_env() -> AppResult<()> {
    let format = LogFormat::parse(&env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".into()))?;
    init(format)
}

pub fn init(format: LogFormat) -> AppResult<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,ghostpost_backend=debug,sqlx=warn"));

    let result = match format {
        LogFormat::Pretty => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().with_target(true).with_ansi(true))
            .try_init(),
        LogFormat::Json => tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json().with_current_span(true).with_span_list(false))
            .try_init(),
    };

    // Allow re-init in tests.
    if let Err(err) = result {
        tracing::debug!(error = %err, "telemetry already initialized");
    }
    Ok(())
}

pub fn init_or_config_err(format: LogFormat) -> AppResult<()> {
    init(format).map_err(|err| AppError::Config(err.to_string()))
}
