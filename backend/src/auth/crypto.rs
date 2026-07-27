//! Sealing, hashing, and constant-time compare helpers for auth secrets.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use base64::Engine;
use zeroize::Zeroize;

const NONCE_LEN: usize = 12;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("seal failed")]
    Seal,
    #[error("open failed")]
    Open,
    #[error("invalid key material")]
    InvalidKey,
}

/// Derive a 32-byte AES key from an arbitrary password/secret via SHA-256.
pub fn derive_key32(secret: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let dig = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&dig);
    out
}

pub fn hash_token(raw: impl AsRef<[u8]>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_ref());
    let dig = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&dig);
    out
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    bool::from(a.ct_eq(b))
}

/// AES-256-GCM seal: nonce || ciphertext+tag.
pub fn seal_bytes(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| CryptoError::Seal)?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open_bytes(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if sealed.len() <= NONCE_LEN {
        return Err(CryptoError::Open);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::InvalidKey)?;
    let (nonce_bytes, ct) = sealed.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ct).map_err(|_| CryptoError::Open)
}


/// Open an online sealed value with the current seal key, then
/// `WORKOS_COOKIE_PASSWORD_PREVIOUS` when set (password rotation window).
/// New seals must always use the current key only.
pub fn open_online_sealed(current_key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
    match open_bytes(current_key, sealed) {
        Ok(plain) => Ok(plain),
        Err(first) => {
            let Ok(prev) = std::env::var("WORKOS_COOKIE_PASSWORD_PREVIOUS") else {
                return Err(first);
            };
            if prev.is_empty() {
                return Err(first);
            }
            let prev_key = derive_key32(&prev);
            open_bytes(&prev_key, sealed).map_err(|_| first)
        }
    }
}

/// Generate `nbytes` of CSPRNG data encoded as URL-safe base64 (no pad).
pub fn random_token_b64(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    rand::rng().fill_bytes(&mut buf);
    let out = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&buf);
    buf.zeroize();
    out
}

#[derive(Debug, Clone)]
pub struct PkcePair {
    pub code_verifier: String,
    pub code_challenge: String,
}

/// S256 PKCE pair; verifier is 32 random bytes, base64url-encoded.
pub fn generate_pkce_pair() -> PkcePair {
    let code_verifier = random_token_b64(32);
    let challenge = hash_token(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge);
    PkcePair {
        code_verifier,
        code_challenge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let key = derive_key32("test-password-at-least-32-characters!!");
        let sealed = seal_bytes(&key, b"hello-workos-session").unwrap();
        let opened = open_bytes(&key, &sealed).unwrap();
        assert_eq!(opened, b"hello-workos-session");
    }

    #[test]
    fn open_fails_wrong_password() {
        let key = derive_key32("correct-horse-battery-staple-xxxxxxxx");
        let sealed = seal_bytes(&key, b"payload").unwrap();
        let bad = derive_key32("wrong-password-xxxxxxxxxxxxxxxxxxxxxxx");
        assert!(open_bytes(&bad, &sealed).is_err());
    }

    #[test]
    fn hash_and_constant_time() {
        let a = hash_token("abc");
        let b = hash_token("abc");
        let c = hash_token("abd");
        assert!(constant_time_eq(&a, &b));
        assert!(!constant_time_eq(&a, &c));
        assert!(!constant_time_eq(&a[..16], &a));
    }

    #[test]
    fn pkce_challenge_is_s256() {
        let pair = generate_pkce_pair();
        let expected = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(hash_token(pair.code_verifier.as_bytes()));
        assert_eq!(pair.code_challenge, expected);
    }
}
