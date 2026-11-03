use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const HEADER_TS: &str = "x-control-plane-ts";
pub const HEADER_NONCE: &str = "x-control-plane-nonce";
pub const HEADER_SIG: &str = "x-control-plane-signature";
pub const HEADER_KEY_ID: &str = "x-control-plane-key-id";
pub const HEADER_CLIENT: &str = "x-control-plane-client";

#[derive(Debug, Clone)]
pub struct ControlPlaneRequestAuth {
    pub key_id: String,
    pub client_id: String,
    pub timestamp: i64,
    pub nonce: String,
    pub signature: String,
}

pub fn unix_now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs() as i64)
        .unwrap_or(0)
}

pub fn random_nonce_hex() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn sha256_hex(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    hex::encode(digest)
}

pub fn canonical_message(
    method: &str,
    path: &str,
    body_sha256_hex: &str,
    timestamp: i64,
    nonce: &str,
    client_id: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method.trim().to_uppercase(),
        path.trim(),
        body_sha256_hex.trim().to_lowercase(),
        timestamp,
        nonce.trim(),
        client_id.trim(),
    )
}

pub fn sign_message_hex(secret_hex: &str, message: &str) -> Result<String, String> {
    let key = hex::decode(secret_hex).map_err(|e| format!("secret hex: {e}"))?;
    let mut mac = HmacSha256::new_from_slice(&key).map_err(|e| e.to_string())?;
    mac.update(message.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

pub fn verify_message_hex(
    secret_hex: &str,
    message: &str,
    signature_hex: &str,
) -> Result<(), String> {
    let expected = sign_message_hex(secret_hex, message)?;
    if constant_time_eq(expected.as_bytes(), signature_hex.trim().as_bytes()) {
        Ok(())
    } else {
        Err("control-plane signature mismatch".to_string())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_message_is_stable() {
        let msg = canonical_message(
            "post",
            "/sign",
            "ab".repeat(32).as_str(),
            123,
            "nonce-1",
            "backend",
        );
        assert_eq!(
            msg,
            format!("POST\n/sign\n{}\n123\nnonce-1\nbackend", "ab".repeat(32))
        );
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let secret = "11".repeat(32);
        let msg = canonical_message(
            "POST",
            crate::sealed_protocol::sealed_str!("/api/licenses/runtime-verify"),
            &"22".repeat(32),
            456,
            "abc",
            "backend",
        );
        let sig = sign_message_hex(&secret, &msg).unwrap();
        verify_message_hex(&secret, &msg, &sig).unwrap();
    }
}
