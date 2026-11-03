//! OpenClaw / Tencent iLink CDN upload helper for image messages.
//!
//! Wire protocol mirrors the openclaw-weixin plugin
//! (`src/cdn/{aes-ecb,cdn-upload,cdn-url,upload}.ts`):
//!
//!   1. POST `<base_url>/ilink/bot/getuploadurl` with the plaintext's
//!      `rawsize` / `rawfilemd5` plus the ciphertext `filesize`, a random
//!      32-hex `filekey`, a random 16-byte `aeskey` (hex-encoded), and
//!      `media_type: 1` (IMAGE) → receives `upload_full_url` (preferred)
//!      or `upload_param` to compose `<cdn_base_url>/upload?...`.
//!   2. AES-128-ECB encrypt the plaintext with PKCS7 padding (key = the 16
//!      raw aeskey bytes), POST the ciphertext as
//!      `Content-Type: application/octet-stream` to the upload URL.
//!   3. The CDN response's `x-encrypted-param` header is the
//!      `encrypt_query_param` we embed in the outbound `ImageItem.media`.
//!
//! Both POSTs go through the `curl` subprocess. On macOS + ClashX TUN
//! reqwest is hijacked by SCDynamicStore and tunnels through clash's HTTP
//! port (which clash rejects on fake-ip CONNECT). `curl --noproxy '*'` is
//! the only path that reliably bypasses both env- and system-level proxy
//! settings. Same approach as `openclaw_weixin::curl_post_authed`.

use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
use aes::Aes128;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::RngCore;
use serde_json::{json, Value};

use super::openclaw_weixin::{curl_post_authed, CHANNEL_VERSION};

/// Default CDN host when the upload-URL response only gives us
/// `upload_param`. Matches openclaw's `ENABLE_CDN_URL_FALLBACK` default
/// (`src/cdn/cdn-url.ts`).
const DEFAULT_CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
/// `UploadMediaType.IMAGE` from `src/api/types.ts`.
const MEDIA_TYPE_IMAGE: i64 = 1;

/// Everything `send_image` needs after a successful upload. `filekey` and
/// the plain/cipher sizes are kept around for logging — the wire-format
/// `ImageItem` only references `encrypt_query_param`, `aes_key_b64` and
/// `filesize` (as `mid_size`).
#[derive(Debug, Clone)]
pub struct UploadedImage {
    pub filekey: String,
    pub encrypt_query_param: String,
    /// Base64 of the AES key's ASCII hex form — matches the openclaw plugin
    /// (`Buffer.from(uploaded.aeskey).toString("base64")` where `aeskey` is
    /// the *hex string*, not raw bytes).
    pub aes_key_b64: String,
    pub rawsize: usize,
    pub filesize: usize,
}

/// Upload one PNG buffer to the WeChat CDN.
///
/// `base_url` is the per-account ilink host from the QR-confirm response
/// (same one used by `IlinkBotClient`). `bot_agent` is the channel's
/// configured UA token (e.g. `xianyu-proxy/1.0`).
pub async fn upload_image_via_cdn(
    base_url: &str,
    token: &str,
    bot_agent: &str,
    to_user_id: &str,
    png_bytes: &[u8],
) -> Result<UploadedImage, String> {
    let rawsize = png_bytes.len();
    let rawfilemd5 = format!("{:x}", md5::compute(png_bytes));
    let filekey = random_hex(16);
    let aeskey_bytes = random_bytes_16();
    let aeskey_hex: String = aeskey_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    let ciphertext = aes128_ecb_encrypt_pkcs7(png_bytes, &aeskey_bytes);
    let filesize = ciphertext.len();

    // Step 1 — getUploadUrl (POST /ilink/bot/getuploadurl).
    let upload_url = fetch_upload_url(
        base_url,
        token,
        bot_agent,
        to_user_id,
        &filekey,
        &aeskey_hex,
        rawsize,
        &rawfilemd5,
        filesize,
    )
    .await?;

    // Step 2 — POST ciphertext to the CDN URL.
    let encrypt_query_param = curl_post_octet_stream(&upload_url, &ciphertext, 30).await?;

    // CDNMedia.aes_key carries the base64-of-hex form (see openclaw
    // `src/messaging/send.ts::sendImageMessageWeixin`).
    let aes_key_b64 = BASE64_STANDARD.encode(aeskey_hex.as_bytes());

    Ok(UploadedImage {
        filekey,
        encrypt_query_param,
        aes_key_b64,
        rawsize,
        filesize,
    })
}

/// POST `/ilink/bot/getuploadurl` and pick the resulting upload URL.
///
/// Prefers `upload_full_url` when present (openclaw `cdn-upload.ts:
/// trimmedFull`); falls back to `cdnBaseUrl/upload?encrypted_query_param=...&filekey=...`
/// (openclaw `cdn-url.ts::buildCdnUploadUrl`).
#[allow(clippy::too_many_arguments)]
async fn fetch_upload_url(
    base_url: &str,
    token: &str,
    bot_agent: &str,
    to_user_id: &str,
    filekey: &str,
    aeskey_hex: &str,
    rawsize: usize,
    rawfilemd5: &str,
    filesize: usize,
) -> Result<String, String> {
    let endpoint = format!("{}/ilink/bot/getuploadurl", base_url.trim_end_matches('/'),);
    let body = json!({
        "filekey": filekey,
        "media_type": MEDIA_TYPE_IMAGE,
        "to_user_id": to_user_id,
        "rawsize": rawsize,
        "rawfilemd5": rawfilemd5,
        "filesize": filesize,
        "no_need_thumb": true,
        "aeskey": aeskey_hex,
        "base_info": {
            "channel_version": CHANNEL_VERSION,
            "bot_agent": if bot_agent.trim().is_empty() {
                "xianyu-proxy/1.0".to_string()
            } else {
                bot_agent.to_string()
            },
        },
    });
    let body_str = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());

    let raw = curl_post_authed(&endpoint, token, &body_str, 15)
        .await
        .map_err(|err| format!("ilink getuploadurl transport error: {err}"))?;
    let payload: Value = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "ilink getuploadurl decode error: {err}; body_len={}",
            raw.chars().count()
        )
    })?;

    if let Some(ret) = payload.get("ret").and_then(Value::as_i64) {
        if ret != 0 {
            let errcode = payload.get("errcode").cloned();
            let errmsg = payload.get("errmsg").and_then(Value::as_str);
            return Err(format!(
                "ilink getuploadurl ret={ret} errcode={errcode:?} errmsg={errmsg:?}"
            ));
        }
    }

    let upload_full_url = payload
        .get("upload_full_url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(full) = upload_full_url {
        return Ok(full.to_string());
    }

    let upload_param = payload
        .get("upload_param")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("ilink getuploadurl missing upload URL (resp={raw})"))?;
    Ok(format!(
        "{}/upload?encrypted_query_param={}&filekey={}",
        DEFAULT_CDN_BASE_URL,
        urlencoding::encode(upload_param),
        urlencoding::encode(filekey),
    ))
}

/// POST `body` as `application/octet-stream`, return the
/// `x-encrypted-param` response header on 2xx, or an error otherwise.
///
/// `curl -D -` dumps response headers to stdout before the body — we capture
/// only the headers (`-o /dev/null` for the body since the CDN's body is just
/// a sentinel string). The ciphertext is streamed via stdin to avoid stuffing
/// it into argv.
async fn curl_post_octet_stream(
    url: &str,
    body: &[u8],
    timeout_secs: u32,
) -> Result<String, String> {
    let url = url.to_string();
    let body = body.to_vec();
    tokio::task::spawn_blocking(move || -> Result<String, String> {
        use std::io::Write as _;
        use std::process::Stdio;

        let mut child = std::process::Command::new("curl")
            .arg("-sS")
            .arg("--max-time")
            .arg(timeout_secs.to_string())
            .arg("--noproxy")
            .arg("*")
            .arg("-D")
            .arg("-")
            .arg("-o")
            .arg("/dev/null")
            .arg("-X")
            .arg("POST")
            .arg("-H")
            .arg("Content-Type: application/octet-stream")
            .arg("--data-binary")
            .arg("@-")
            .arg(&url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("spawn curl: {err}"))?;

        // Stream the ciphertext into curl's stdin.
        {
            let mut stdin = child.stdin.take().ok_or("curl stdin unavailable")?;
            stdin
                .write_all(&body)
                .map_err(|err| format!("curl stdin write: {err}"))?;
        }

        let out = child
            .wait_with_output()
            .map_err(|err| format!("curl wait: {err}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("curl exit {} stderr={}", out.status, stderr));
        }

        let headers_blob = String::from_utf8_lossy(&out.stdout).to_string();
        parse_curl_response(&headers_blob)
    })
    .await
    .map_err(|err| format!("spawn_blocking join: {err}"))?
}

/// Pull out the HTTP status + `x-encrypted-param` from a `curl -D -` dump.
///
/// On redirect/chained responses there can be multiple status lines; we
/// take the last one and the matching header below it.
fn parse_curl_response(blob: &str) -> Result<String, String> {
    // Split on blank lines so each chunk is one HTTP response head.
    let chunks: Vec<&str> = blob
        .split("\r\n\r\n")
        .filter(|c| !c.trim().is_empty())
        .collect();
    let last = chunks
        .last()
        .ok_or_else(|| "empty curl response".to_string())?;

    let mut lines = last.lines();
    let status_line = lines.next().unwrap_or_default().trim();
    // HTTP/1.1 200 OK / HTTP/2 200
    let mut parts = status_line.split_whitespace();
    let _proto = parts.next();
    let status_code = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    let mut x_encrypted_param: Option<String> = None;
    let mut x_error_message: Option<String> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            match name.as_str() {
                "x-encrypted-param" => x_encrypted_param = Some(value),
                "x-error-message" => x_error_message = Some(value),
                _ => {}
            }
        }
    }

    if !(200..300).contains(&status_code) {
        return Err(format!(
            "CDN upload HTTP {status_code} x-error-message={x_error_message:?}"
        ));
    }
    x_encrypted_param.ok_or_else(|| "CDN response missing x-encrypted-param header".to_string())
}

// ---------------------------------------------------------------------------
// Crypto / random helpers
// ---------------------------------------------------------------------------

/// AES-128-ECB encrypt with PKCS7 padding. Matches Node's
/// `createCipheriv("aes-128-ecb", key, null)` (PKCS7 is the default for
/// block-aligned modes; openclaw's `aesEcbPaddedSize` accounts for the
/// always-appended pad block when input length is already a multiple of 16).
fn aes128_ecb_encrypt_pkcs7(plaintext: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let pad_len = 16 - (plaintext.len() % 16);
    let pad_byte = pad_len as u8;

    let mut buf = Vec::with_capacity(plaintext.len() + pad_len);
    buf.extend_from_slice(plaintext);
    buf.extend(std::iter::repeat(pad_byte).take(pad_len));

    for chunk in buf.chunks_mut(16) {
        let block = GenericArray::from_mut_slice(chunk);
        cipher.encrypt_block(block);
    }
    buf
}

/// 16 random bytes — the raw AES key.
fn random_bytes_16() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// `len` random bytes encoded as lowercase hex (output is `2 * len` chars).
fn random_hex(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_ecb_pkcs7_block_aligned_pads_full_block() {
        // 16-byte plaintext → 32-byte ciphertext (one block + one pad block
        // of 0x10 bytes). Mirrors openclaw `aesEcbPaddedSize(16) === 32`.
        let key = [0x42u8; 16];
        let plaintext = [0u8; 16];
        let ciphertext = aes128_ecb_encrypt_pkcs7(&plaintext, &key);
        assert_eq!(ciphertext.len(), 32);
    }

    #[test]
    fn aes_ecb_pkcs7_short_block_pads_to_16() {
        let key = [0x11u8; 16];
        let plaintext = b"abc";
        let ciphertext = aes128_ecb_encrypt_pkcs7(plaintext, &key);
        assert_eq!(ciphertext.len(), 16);
    }

    #[test]
    fn parse_curl_response_extracts_x_encrypted_param() {
        let blob = "HTTP/2 200\r\nx-encrypted-param: abc123\r\ncontent-length: 9\r\n\r\n";
        let value = parse_curl_response(blob).unwrap();
        assert_eq!(value, "abc123");
    }

    #[test]
    fn parse_curl_response_surfaces_x_error_message_on_4xx() {
        let blob = "HTTP/1.1 403 Forbidden\r\nx-error-message: bad sig\r\n\r\n";
        let err = parse_curl_response(blob).unwrap_err();
        assert!(err.contains("403"));
        assert!(err.contains("bad sig"));
    }
}
