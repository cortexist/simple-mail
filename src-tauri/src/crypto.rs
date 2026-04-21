//! crypto.rs — AES-256-GCM encryption for stored passwords.
//!
//! The master key is stored in the OS credential store (Windows Credential Manager,
//! macOS Keychain, Linux Secret Service) via the `keyring` crate.
//! Each encrypted value is stored as base64( nonce[12] || ciphertext || tag[16] ).

use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use aes_gcm::aead::Aead;
use base64::Engine;
use rand::RngExt;
use std::sync::Mutex;

const SERVICE_NAME: &str = "cortexist-mail";
const KEY_ENTRY: &str = "master-encryption-key";

/// Cached master key (loaded once per process).
static MASTER_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

/// Get or create the 256-bit master key from the OS keyring.
fn get_master_key() -> Result<[u8; 32], String> {
    let mut guard = MASTER_KEY.lock().map_err(|e| format!("lock error: {e}"))?;
    if let Some(key) = *guard {
        return Ok(key);
    }

    let entry = keyring::Entry::new(SERVICE_NAME, KEY_ENTRY)
        .map_err(|e| format!("keyring entry error: {e}"))?;

    let key = match entry.get_password() {
        Ok(b64_key) => {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&b64_key)
                .map_err(|e| format!("failed to decode master key: {e}"))?;
            if bytes.len() != 32 {
                return Err(format!("stored master key has wrong length: {}", bytes.len()));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            key
        }
        Err(keyring::Error::NoEntry) => {
            // First run: generate a new key
            let mut key = [0u8; 32];
            rand::rng().fill(&mut key);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&key);
            entry.set_password(&b64)
                .map_err(|e| format!("failed to store master key in OS keyring: {e}"))?;
            key
        }
        Err(e) => return Err(format!("keyring error: {e}")),
    };

    *guard = Some(key);
    Ok(key)
}

/// Encrypt a plaintext password. Returns base64( nonce || ciphertext+tag ).
pub fn encrypt_password(plaintext: &str) -> Result<String, String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let key = get_master_key()?;
    let cipher = Aes256Gcm::new(&key.into());

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| format!("encryption failed: {e}"))?;

    // nonce (12) + ciphertext + tag (16)
    let mut blob = Vec::with_capacity(12 + ciphertext.len());
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);

    Ok(base64::engine::general_purpose::STANDARD.encode(&blob))
}

/// Decrypt a base64-encoded blob back to plaintext.
pub fn decrypt_password(encoded: &str) -> Result<String, String> {
    if encoded.is_empty() {
        return Ok(String::new());
    }
    let key = get_master_key()?;
    let cipher = Aes256Gcm::new(&key.into());

    let blob = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("base64 decode failed: {e}"))?;

    if blob.len() < 12 + 16 {
        return Err("encrypted blob too short".into());
    }

    let nonce = Nonce::from_slice(&blob[..12]);
    let ciphertext = &blob[12..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "decryption failed — wrong key or corrupted data".to_string())?;

    String::from_utf8(plaintext).map_err(|e| format!("decrypted data is not valid UTF-8: {e}"))
}

/// Check if a string looks like it's already encrypted (valid base64 of sufficient length).
/// Plaintext passwords typically don't produce valid base64 of 40+ chars with correct structure.
pub fn is_encrypted(value: &str) -> bool {
    if value.is_empty() {
        return true; // empty is "encrypted" (no-op)
    }
    // Minimum: 12 (nonce) + 1 (data) + 16 (tag) = 29 bytes → ~40 base64 chars
    if value.len() < 40 {
        return false;
    }
    // Must be valid base64 that decodes to at least 28 bytes
    match base64::engine::general_purpose::STANDARD.decode(value) {
        Ok(blob) => blob.len() >= 28,
        Err(_) => false,
    }
}

/// Encrypt a password only if it isn't already encrypted.
/// Used during migration to avoid double-encrypting.
pub fn encrypt_if_plaintext(value: &str) -> Result<String, String> {
    if value.is_empty() || is_encrypted(value) {
        return Ok(value.to_string());
    }
    encrypt_password(value)
}
