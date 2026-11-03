//! Desktop → phone account bundle transfer via encrypted QR code.
//!
//! The desktop app exports a small JSON blob (e.g. the active xianyu cookies
//! plus metadata) and the phone scans a QR to import it. Because the QR may
//! be photographed by a third party, we encrypt the payload with a fresh
//! AES-256-GCM key. The key travels **inside the URL fragment** alongside
//! the ciphertext — the QR itself is the secret. There is no server trust
//! involved: whoever holds the QR can decrypt, nobody else can.
//!
//! URL shape (single token, fits inside one QR):
//!
//!   xianyu://import/v1?k=<b64url key>&n=<b64url nonce>&c=<b64url ciphertext>
//!
//! Capacity: at QR error-correction level M a version-40 code holds ~2.3 KB
//! of alphanumeric/binary data. base64url'd the overhead is ~33% so our
//! realistic plaintext budget is ~1.5 KB — comfortable for a handful of
//! account cookies and labels, which is all this flow is designed for.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use fast_qr::{
    convert::{svg::SvgBuilder, Builder, Shape},
    QRBuilder,
};
use rand::RngCore;
use serde::Serialize;
use thiserror::Error;

const URL_SCHEME: &str = "xianyu";
const URL_HOST: &str = "import";
const URL_VERSION_PATH: &str = "v1";

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("plaintext too large ({0} bytes) — QR capacity is ~1500 bytes")]
    PayloadTooLarge(usize),
    #[error("invalid bundle URL: {0}")]
    InvalidUrl(String),
    #[error("decrypt failed: {0}")]
    DecryptFailed(String),
    #[error("QR encoding failed: {0}")]
    QrFailed(String),
    #[error("base64 decode failed: {0}")]
    Base64(#[from] base64::DecodeError),
}

#[derive(Debug, Serialize)]
pub struct BundleEnvelope {
    /// The full `xianyu://import/v1?...` URL to encode.
    pub url: String,
    /// SVG string of the QR code rendering `url`.
    pub qr_svg: String,
    /// The raw key, base64url — exposed so a UI can show
    /// "rotate key" / "copy link" separately if desired.
    pub key_b64: String,
}

/// Encrypt `plaintext`, wrap into a `xianyu://import/v1?...` URL and render
/// a QR SVG. Key + nonce are freshly generated every call.
pub fn encrypt_bundle(plaintext: &[u8]) -> Result<BundleEnvelope, BundleError> {
    // Soft-limit: the QR we render targets ~1.5KB plaintext. Anything larger
    // will probably still fit but we'd rather surface a clean error than
    // hand back a QR that can't be scanned reliably.
    if plaintext.len() > 1_500 {
        return Err(BundleError::PayloadTooLarge(plaintext.len()));
    }

    let mut key_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut key_bytes);
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new((&key_bytes).into());
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|e| BundleError::DecryptFailed(format!("encrypt: {e}")))?;

    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let nonce_b64 = URL_SAFE_NO_PAD.encode(nonce_bytes);
    let ct_b64 = URL_SAFE_NO_PAD.encode(&ciphertext);

    let url = format!(
        "{scheme}://{host}/{version}?k={k}&n={n}&c={c}",
        scheme = URL_SCHEME,
        host = URL_HOST,
        version = URL_VERSION_PATH,
        k = key_b64,
        n = nonce_b64,
        c = ct_b64,
    );

    let qr = QRBuilder::new(url.clone())
        .ecl(fast_qr::ECL::M)
        .build()
        .map_err(|e| BundleError::QrFailed(format!("{e:?}")))?;

    let qr_svg = SvgBuilder::default()
        .shape(Shape::Square)
        .margin(4)
        .to_str(&qr);

    Ok(BundleEnvelope {
        url,
        qr_svg,
        key_b64,
    })
}

/// Parse a `xianyu://import/v1?...` URL and return the decrypted plaintext.
pub fn decrypt_bundle(raw: &str) -> Result<Vec<u8>, BundleError> {
    let parsed = url::Url::parse(raw).map_err(|e| BundleError::InvalidUrl(e.to_string()))?;

    if parsed.scheme() != URL_SCHEME {
        return Err(BundleError::InvalidUrl(format!(
            "unexpected scheme {}",
            parsed.scheme()
        )));
    }
    if parsed.host_str() != Some(URL_HOST) {
        return Err(BundleError::InvalidUrl(format!(
            "unexpected host {:?}",
            parsed.host_str()
        )));
    }
    if parsed.path().trim_matches('/') != URL_VERSION_PATH {
        return Err(BundleError::InvalidUrl(format!(
            "unexpected path {}",
            parsed.path()
        )));
    }

    let mut k = None;
    let mut n = None;
    let mut c = None;
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "k" => k = Some(value.into_owned()),
            "n" => n = Some(value.into_owned()),
            "c" => c = Some(value.into_owned()),
            _ => {}
        }
    }

    let key_b64 = k.ok_or_else(|| BundleError::InvalidUrl("missing k".into()))?;
    let nonce_b64 = n.ok_or_else(|| BundleError::InvalidUrl("missing n".into()))?;
    let ct_b64 = c.ok_or_else(|| BundleError::InvalidUrl("missing c".into()))?;

    let key_bytes = URL_SAFE_NO_PAD.decode(key_b64.as_bytes())?;
    let nonce_bytes = URL_SAFE_NO_PAD.decode(nonce_b64.as_bytes())?;
    let ct_bytes = URL_SAFE_NO_PAD.decode(ct_b64.as_bytes())?;

    if key_bytes.len() != 32 {
        return Err(BundleError::InvalidUrl(format!(
            "key must be 32 bytes, got {}",
            key_bytes.len()
        )));
    }
    if nonce_bytes.len() != 12 {
        return Err(BundleError::InvalidUrl(format!(
            "nonce must be 12 bytes, got {}",
            nonce_bytes.len()
        )));
    }

    let cipher = Aes256Gcm::new(key_bytes.as_slice().into());
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ct_bytes.as_ref())
        .map_err(|e| BundleError::DecryptFailed(e.to_string()))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let plaintext = br#"{"cookies":"cookie2=abc;unb=123","label":"my-acct"}"#;
        let env = encrypt_bundle(plaintext).unwrap();
        assert!(env.url.starts_with("xianyu://import/v1?"));
        assert!(env.qr_svg.contains("<svg"));
        let out = decrypt_bundle(&env.url).unwrap();
        assert_eq!(out, plaintext);
    }

    #[test]
    fn tampered_url_fails() {
        let env = encrypt_bundle(b"hello").unwrap();
        // Flip the last ciphertext char.
        let mut bad = env.url.clone();
        let last = bad.pop().unwrap();
        bad.push(if last == 'A' { 'B' } else { 'A' });
        assert!(decrypt_bundle(&bad).is_err());
    }

    #[test]
    fn rejects_bad_scheme() {
        assert!(decrypt_bundle("https://import/v1?k=a&n=b&c=c").is_err());
    }

    #[test]
    fn size_limit_enforced() {
        let big = vec![0u8; 2_000];
        assert!(matches!(
            encrypt_bundle(&big),
            Err(BundleError::PayloadTooLarge(_))
        ));
    }
}
