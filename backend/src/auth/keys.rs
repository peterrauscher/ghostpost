//! APP_SESSION_KEYS parsing and selection.

use crate::error::{AppError, AppResult};
use base64::Engine;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SessionKey {
    pub id: String,
    pub secret: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct SessionKeyring {
    /// Newest-first.
    keys: Vec<SessionKey>,
}

#[derive(Debug, Deserialize)]
struct RawKey {
    id: String,
    secret: String,
}



fn strip_wrapping_quotes(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return &raw[1..raw.len() - 1];
        }
    }
    raw
}

impl SessionKeyring {
    pub fn parse(raw_json: &str) -> AppResult<Self> {
        let trimmed = strip_wrapping_quotes(raw_json.trim());
        let raw: Vec<RawKey> = serde_json::from_str(trimmed)
            .map_err(|err| AppError::Config(format!("APP_SESSION_KEYS invalid JSON: {err}")))?;
        if raw.is_empty() {
            return Err(AppError::Config(
                "APP_SESSION_KEYS must contain at least one key".into(),
            ));
        }
        let mut keys = Vec::with_capacity(raw.len());
        for item in raw {
            if !item.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                || item.id.is_empty()
            {
                return Err(AppError::Config(format!(
                    "APP_SESSION_KEYS id {:?} must match ^[a-z0-9_]+$",
                    item.id
                )));
            }
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(item.secret.trim())
                .or_else(|_| {
                    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(item.secret.trim())
                })
                .map_err(|err| {
                    AppError::Config(format!("APP_SESSION_KEYS secret for {} invalid base64: {err}", item.id))
                })?;
            if decoded.len() != 32 {
                return Err(AppError::Config(format!(
                    "APP_SESSION_KEYS secret for {} must decode to 32 bytes",
                    item.id
                )));
            }
            let mut secret = [0u8; 32];
            secret.copy_from_slice(&decoded);
            keys.push(SessionKey {
                id: item.id,
                secret,
            });
        }
        Ok(Self { keys })
    }

    pub fn current(&self) -> &SessionKey {
        &self.keys[0]
    }

    pub fn contains_id(&self, id: &str) -> bool {
        self.keys.iter().any(|k| k.id == id)
    }

    pub fn keys(&self) -> &[SessionKey] {
        &self.keys
    }
}
