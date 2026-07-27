use crate::error::{AppError, AppResult};
use std::env;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub database_url_app: String,
    pub database_app_role: String,
    pub db_max_connections: u32,
    pub migration_max_connections: u32,
    pub migration_lock_timeout: Duration,
    pub bind_addr: String,
    pub log_format: LogFormat,
    pub instance_id: String,
    pub worker_lease: Duration,
    pub worker_heartbeat: Duration,
    pub worker_sweeper: Duration,
    pub shutdown_deadline_secs: u64,
    pub restore_replay_pending: bool,
    pub is_development: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
}

impl LogFormat {
    pub fn parse(raw: &str) -> AppResult<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            other => Err(AppError::Config(format!(
                "LOG_FORMAT must be pretty|json, got {other}"
            ))),
        }
    }
}

fn load_dotenv_if_dev() {
    let ghostpost_env = env::var("GHOSTPOST_ENV").unwrap_or_default();
    let is_dev = cfg!(debug_assertions) || ghostpost_env.eq_ignore_ascii_case("development");
    if is_dev {
        let _ = dotenvy::dotenv();
    }
}

fn require_env(key: &str) -> AppResult<String> {
    env::var(key).map_err(|_| AppError::Config(format!("missing required env {key}")))
}

fn parse_u32(key: &str, default: u32) -> AppResult<u32> {
    match env::var(key) {
        Ok(raw) => raw
            .parse::<u32>()
            .map_err(|err| AppError::Config(format!("{key} invalid: {err}"))),
        Err(_) => Ok(default),
    }
}

fn parse_u64(key: &str, default: u64) -> AppResult<u64> {
    match env::var(key) {
        Ok(raw) => raw
            .parse::<u64>()
            .map_err(|err| AppError::Config(format!("{key} invalid: {err}"))),
        Err(_) => Ok(default),
    }
}

fn parse_bool(key: &str, default: bool) -> AppResult<bool> {
    match env::var(key) {
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(AppError::Config(format!(
                "{key} must be bool, got {other}"
            ))),
        },
        Err(_) => Ok(default),
    }
}

/// Validate an unquoted Postgres identifier before safely quoting it.
pub fn validate_pg_ident(raw: &str) -> AppResult<&str> {
    let bytes = raw.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return Err(AppError::Config(
            "DATABASE_APP_ROLE must be a Postgres identifier (1..=63 chars)".into(),
        ));
    }
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first == b'_') {
        return Err(AppError::Config(
            "DATABASE_APP_ROLE must start with [a-z_]".into(),
        ));
    }
    if !bytes[1..]
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
    {
        return Err(AppError::Config(
            "DATABASE_APP_ROLE must match [a-z_][a-z0-9_]*".into(),
        ));
    }
    Ok(raw)
}

pub fn quote_ident(raw: &str) -> AppResult<String> {
    let ident = validate_pg_ident(raw)?;
    Ok(format!("\"{ident}\""))
}

fn default_instance_id() -> String {
    let host = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".into());
    format!("{host}-{}", std::process::id())
}

impl Config {
    pub fn load_for_migrate() -> AppResult<Self> {
        load_dotenv_if_dev();
        let database_url = require_env("DATABASE_URL")?;
        let database_app_role =
            env::var("DATABASE_APP_ROLE").unwrap_or_else(|_| "ghostpost_app".into());
        validate_pg_ident(&database_app_role)?;
        let migration_max_connections = parse_u32("MIGRATION_MAX_CONNECTIONS", 2)?;
        if migration_max_connections != 2 {
            return Err(AppError::Config(
                "MIGRATION_MAX_CONNECTIONS must be exactly 2 in v1".into(),
            ));
        }
        let migration_lock_timeout_secs = parse_u64("MIGRATION_LOCK_TIMEOUT_SECS", 60)?;
        let log_format = LogFormat::parse(
            &env::var("LOG_FORMAT").unwrap_or_else(|_| "pretty".into()),
        )?;
        let is_development = cfg!(debug_assertions)
            || env::var("GHOSTPOST_ENV")
                .map(|v| v.eq_ignore_ascii_case("development"))
                .unwrap_or(false);

        Ok(Self {
            database_url: database_url.clone(),
            database_url_app: env::var("DATABASE_URL_APP").unwrap_or(database_url),
            database_app_role,
            db_max_connections: parse_u32("DB_MAX_CONNECTIONS", 10)?,
            migration_max_connections,
            migration_lock_timeout: Duration::from_secs(migration_lock_timeout_secs),
            bind_addr: env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            log_format,
            instance_id: env::var("INSTANCE_ID").unwrap_or_else(|_| default_instance_id()),
            worker_lease: Duration::from_secs(parse_u64("WORKER_LEASE_SECS", 60)?),
            worker_heartbeat: Duration::from_secs(parse_u64("WORKER_HEARTBEAT_SECS", 20)?),
            worker_sweeper: Duration::from_secs(parse_u64("WORKER_SWEEPER_SECS", 30)?),
            shutdown_deadline_secs: parse_u64("SHUTDOWN_DEADLINE_SECS", 30)?,
            restore_replay_pending: parse_bool("RESTORE_REPLAY_PENDING", false)?,
            is_development,
        })
    }

    pub fn load_for_serve() -> AppResult<Self> {
        let mut cfg = Self::load_for_migrate()?;
        let db_max = parse_u32("DB_MAX_CONNECTIONS", 10)?;
        if db_max < 1 {
            return Err(AppError::Config(
                "DB_MAX_CONNECTIONS must be >= 1".into(),
            ));
        }
        cfg.db_max_connections = db_max;

        // Prefer app URL; allow DATABASE_URL fallback in local development only.
        match env::var("DATABASE_URL_APP") {
            Ok(url) => cfg.database_url_app = url,
            Err(_) if cfg.is_development => {
                cfg.database_url_app = cfg.database_url.clone();
            }
            Err(_) => {
                return Err(AppError::Config(
                    "DATABASE_URL_APP required for serve outside development".into(),
                ));
            }
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_bad_role_ident() {
        assert!(validate_pg_ident("ghostpost_app").is_ok());
        assert!(validate_pg_ident("Bad-Role").is_err());
        assert!(validate_pg_ident("';DROP TABLE").is_err());
        assert_eq!(quote_ident("ghostpost_app").unwrap(), format!("\"{}\"", "ghostpost_app"));
    }

    #[test]
    fn log_format_parses() {
        assert_eq!(LogFormat::parse("json").unwrap(), LogFormat::Json);
        assert_eq!(LogFormat::parse("pretty").unwrap(), LogFormat::Pretty);
        assert!(LogFormat::parse("xml").is_err());
    }
}
