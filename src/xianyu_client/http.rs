use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, OnceLock},
    time::Instant,
};

use reqwest::{
    header::{HeaderMap, HeaderValue, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT},
    multipart::{Form, Part},
    Client, StatusCode,
};
use serde_json::{json, Map, Value};
use sqlx::{Pool, Sqlite};

use crate::config::AppConfig;
use crate::control_plane_auth::{
    canonical_message, random_nonce_hex, sha256_hex, sign_message_hex, unix_now_secs,
    HEADER_CLIENT, HEADER_KEY_ID, HEADER_NONCE, HEADER_SIG, HEADER_TS,
};
use crate::control_plane_tls::verify_response_pin;
use crate::license_runtime::{apply_client_version_headers, get_sign_license_context, ProofMode};

use super::{
    session::{apply_response_cookies, extract_sign_token, is_token_expired, load_account_session},
    utils::{
        app_headers_for_account_or_default, app_template_header, app_template_lat_lng,
        percent_decode_lossy, percent_encode_component, set_header, trim_trailing_slash,
        unix_millis_now,
    },
    H5RequestProfile, UploadResult, APP_USER_AGENT, CHAT_USER_AGENT, DEFAULT_H5_JSV,
    DEFAULT_USER_AGENT, STREAM_USER_AGENT, WX_USER_AGENT,
};

const SIGN_CACHE_TTL_SECS: u64 = 420;
const SIGN_CACHE_MAX_ENTRIES: usize = 1024;
const TRANSIENT_SERVICE_RETRY_COUNT: usize = 3;

#[derive(serde::Deserialize)]
struct RuntimeSignChallengeResponse {
    challenge_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeChallengeMode {
    Required(String),
    Skip,
}

fn is_gateway_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT
    )
}

fn body_mentions_transient_gateway(body: &str) -> bool {
    let normalized = body.to_ascii_lowercase();
    normalized.contains("bad gateway")
        || normalized.contains("http 502")
        || normalized.contains("502 bad gateway")
        || normalized.contains("cloudflare")
        || normalized.contains("connection reset")
        || normalized.contains("timeout")
}

fn should_retry_service_response(status: StatusCode, body: &str) -> bool {
    is_gateway_status(status) || body_mentions_transient_gateway(body)
}

fn sanitize_service_body(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "服务暂时不可用，请稍后重试".to_string();
    }
    if trimmed.contains("<!DOCTYPE html")
        || trimmed.contains("<html")
        || body_mentions_transient_gateway(trimmed)
    {
        return "服务暂时不可用，请稍后重试".to_string();
    }
    trimmed.chars().take(120).collect()
}

fn runtime_challenge_error(status: StatusCode, body: &str) -> String {
    if should_retry_service_response(status, body) {
        "鉴权服务暂时不可用，请稍后重试".to_string()
    } else {
        format!(
            "runtime challenge returned HTTP {status}: {}",
            sanitize_service_body(body)
        )
    }
}

fn sign_service_error(status: StatusCode, body: &str) -> String {
    if body.to_ascii_lowercase().contains("license server")
        && should_retry_service_response(status, body)
    {
        return "鉴权服务暂时不可用，请稍后重试".to_string();
    }
    if should_retry_service_response(status, body) {
        "签名服务暂时不可用，请稍后重试".to_string()
    } else {
        format!(
            "sign service returned HTTP {status}: {}",
            sanitize_service_body(body)
        )
    }
}

async fn sleep_before_transient_retry(attempt: usize) {
    let millis = 250_u64.saturating_mul((attempt as u64) + 1);
    tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
}

fn license_proof_header_license_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-id")
}
fn license_proof_header_device_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-device-id")
}
fn license_proof_header_ts() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-proof-ts")
}
fn license_proof_header_nonce() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-proof-nonce")
}
fn license_proof_header_sig() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-proof-signature")
}
fn device_proof_header_public_key() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-public-key")
}
fn device_proof_header_signature() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-signature")
}
fn device_proof_header_timestamp() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-ts")
}
fn device_proof_header_nonce() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-nonce")
}
fn device_proof_header_license_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-id")
}
fn device_proof_header_device_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-device-id")
}

struct SignCacheEntry {
    sign_headers: Map<String, Value>,
    request_headers: Map<String, Value>,
    current_t: String,
    created: Instant,
}

fn sign_cache() -> &'static Mutex<HashMap<String, SignCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, SignCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_cached_sign(
    scope: &str,
    uid: &str,
    api: &str,
    data: &str,
) -> Option<(Map<String, Value>, Map<String, Value>, String)> {
    let key = format!("{scope}\x00{uid}\x00{api}\x00{data}");
    let cache = sign_cache().lock().ok()?;
    let entry = cache.get(&key)?;
    if entry.created.elapsed().as_secs() >= SIGN_CACHE_TTL_SECS {
        return None;
    }
    Some((
        entry.sign_headers.clone(),
        entry.request_headers.clone(),
        entry.current_t.clone(),
    ))
}

fn set_cached_sign(
    scope: &str,
    uid: &str,
    api: &str,
    data: &str,
    sign_headers: &Map<String, Value>,
    request_headers: &Map<String, Value>,
    current_t: &str,
) {
    let key = format!("{scope}\x00{uid}\x00{api}\x00{data}");
    if let Ok(mut cache) = sign_cache().lock() {
        if cache.len() >= SIGN_CACHE_MAX_ENTRIES {
            let expired: Vec<String> = cache
                .iter()
                .filter(|(_, v)| v.created.elapsed().as_secs() >= SIGN_CACHE_TTL_SECS)
                .map(|(k, _)| k.clone())
                .collect();
            for k in &expired {
                cache.remove(k);
            }
            if cache.len() >= SIGN_CACHE_MAX_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(
            key,
            SignCacheEntry {
                sign_headers: sign_headers.clone(),
                request_headers: request_headers.clone(),
                current_t: current_t.to_string(),
                created: Instant::now(),
            },
        );
    }
}

// 全局共享 reqwest::Client —— `Client` 内部是 `Arc`，设计上就要全程复用。
// 之前每次 MTOP 调用都 `Client::builder().build()` 新建一个，每个新 client
// 自己的连接池 / DNS 缓存 / TLS session 全部从零开始。20 个账号并发刷新就是
// 20 次 TLS 握手到 h5api.m.goofish.com，把 tokio worker pool 全占住，
// `/api/accounts` 这种本地 SQL 的 endpoint 都拿不到 worker 跑。
//
// 单例 client：所有调用共享一个 connection pool（HTTP/1.1 keep-alive 或
// HTTP/2 multiplexing），单 host 默认上限 reqwest 给的就够用了。后续刷新
// 复用已存在的 TCP/TLS，开销从「200-500ms 握手」降到「~20ms」。
fn build_global_client() -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        // 每个 host 最多保 16 个空闲连接——20 个账号并发也能复用。
        .pool_max_idle_per_host(16)
        // idle 太久 goofish 会断我们这边，3 分钟够撑刷新间隔。
        .pool_idle_timeout(std::time::Duration::from_secs(180))
        // TLS 握手 10s 超时——网络抖动时不至于跟着挂太久。
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())
}

fn build_control_plane_client() -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .tls_info(true)
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())
}

pub(crate) fn http_client() -> Result<Client, String> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT.get_or_init(build_global_client).clone()
}

fn control_plane_client() -> Result<Client, String> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT.get_or_init(build_control_plane_client).clone()
}

fn apply_sign_service_access_headers(
    builder: reqwest::RequestBuilder,
    config: &AppConfig,
) -> reqwest::RequestBuilder {
    let builder = apply_client_version_headers(builder);
    let client_id = config.xianyu.sign_cf_access_client_id.trim();
    let client_secret = config.xianyu.sign_cf_access_client_secret.trim();
    if client_id.is_empty() || client_secret.is_empty() {
        return builder;
    }
    builder
        .header("CF-Access-Client-Id", client_id)
        .header("CF-Access-Client-Secret", client_secret)
}

fn apply_license_proof_headers(
    builder: reqwest::RequestBuilder,
    license_context: &crate::license_runtime::SignLicenseContext,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<reqwest::RequestBuilder, String> {
    let timestamp = unix_now_secs();
    let nonce = random_nonce_hex();
    let body_sha = sha256_hex(body);
    let message = canonical_message(
        method,
        path,
        &body_sha,
        timestamp,
        &nonce,
        &license_context.device_id,
    );
    let signature = sign_message_hex(&license_context.hmac_secret_hex, &message)?;
    Ok(builder
        .header(
            license_proof_header_license_id(),
            license_context.license_id.to_string(),
        )
        .header(license_proof_header_device_id(), &license_context.device_id)
        .header(license_proof_header_ts(), timestamp.to_string())
        .header(license_proof_header_nonce(), nonce)
        .header(license_proof_header_sig(), signature))
}

fn apply_runtime_proof_headers(
    builder: reqwest::RequestBuilder,
    proof_mode: &ProofMode,
    license_context: &crate::license_runtime::SignLicenseContext,
) -> reqwest::RequestBuilder {
    match proof_mode {
        ProofMode::Device(proof) => builder
            .header(device_proof_header_public_key(), &proof.public_key_hex)
            .header(device_proof_header_signature(), &proof.signature_hex)
            .header(device_proof_header_timestamp(), proof.timestamp.to_string())
            .header(device_proof_header_nonce(), &proof.nonce)
            .header(
                device_proof_header_license_id(),
                proof.license_id.to_string(),
            )
            .header(device_proof_header_device_id(), &proof.device_id),
        ProofMode::LegacyLicenseHmac {
            timestamp,
            nonce,
            signature_hex,
        } => builder
            .header(
                license_proof_header_license_id(),
                license_context.license_id.to_string(),
            )
            .header(license_proof_header_device_id(), &license_context.device_id)
            .header(license_proof_header_ts(), timestamp.to_string())
            .header(license_proof_header_nonce(), nonce)
            .header(license_proof_header_sig(), signature_hex),
    }
}

fn apply_control_plane_headers(
    builder: reqwest::RequestBuilder,
    config: &AppConfig,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<reqwest::RequestBuilder, String> {
    let secret = config.xianyu.control_plane_shared_secret_hex.trim();
    if secret.is_empty() {
        return Ok(builder);
    }
    let timestamp = unix_now_secs();
    let nonce = random_nonce_hex();
    let body_sha = sha256_hex(body);
    let message = canonical_message(
        method,
        path,
        &body_sha,
        timestamp,
        &nonce,
        &config.xianyu.control_plane_client_id,
    );
    let signature = sign_message_hex(secret, &message)?;
    Ok(builder
        .header(HEADER_CLIENT, &config.xianyu.control_plane_client_id)
        .header(HEADER_KEY_ID, &config.xianyu.control_plane_key_id)
        .header(HEADER_TS, timestamp.to_string())
        .header(HEADER_NONCE, nonce)
        .header(HEADER_SIG, signature))
}

async fn fetch_runtime_sign_challenge(
    client: &Client,
    config: &AppConfig,
    license_server_url: &str,
    license_context: &crate::license_runtime::SignLicenseContext,
    data_dir: &std::path::Path,
) -> Result<RuntimeChallengeMode, String> {
    let body = json!({
        "device_id": license_context.device_id,
    });
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|error| format!("runtime challenge body JSON: {error}"))?;
    let url = format!(
        "{}{}",
        trim_trailing_slash(license_server_url),
        crate::sealed_protocol::sealed_str!("/api/licenses/runtime-challenge")
    );
    for attempt in 0..TRANSIENT_SERVICE_RETRY_COUNT {
        let proof_mode = crate::license_runtime::select_runtime_proof_mode(
            client,
            license_server_url,
            license_context,
            data_dir,
            "POST",
            crate::sealed_protocol::sealed_str!("/api/licenses/runtime-challenge"),
            &body_bytes,
        )
        .await?;
        let response = match apply_runtime_proof_headers(
            apply_client_version_headers(client.post(&url))
                .bearer_auth(&license_context.auth_token)
                .json(&body),
            &proof_mode,
            license_context,
        )
        .send()
        .await
        {
            Ok(response) => response,
            Err(error) => {
                if attempt + 1 < TRANSIENT_SERVICE_RETRY_COUNT
                    && (error.is_timeout() || error.is_connect())
                {
                    sleep_before_transient_retry(attempt).await;
                    continue;
                }
                return Err(format!("runtime challenge request failed: {error}"));
            }
        };
        if !is_loopback_control_plane_url(license_server_url) {
            verify_response_pin(
                &response,
                &config.xianyu.control_plane_cert_pins,
                "license_service",
                license_server_url,
            )?;
        }
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("runtime challenge response read failed: {error}"))?;
        if !status.is_success() {
            if status == reqwest::StatusCode::NOT_FOUND {
                return Ok(RuntimeChallengeMode::Skip);
            }
            if attempt + 1 < TRANSIENT_SERVICE_RETRY_COUNT
                && should_retry_service_response(status, &body)
            {
                sleep_before_transient_retry(attempt).await;
                continue;
            }
            return Err(runtime_challenge_error(status, &body));
        }
        let payload: RuntimeSignChallengeResponse = serde_json::from_str(&body)
            .map_err(|error| format!("runtime challenge JSON parse: {error}"))?;
        let challenge_id = payload.challenge_id.trim();
        if challenge_id.is_empty() {
            return Ok(RuntimeChallengeMode::Skip);
        }
        return Ok(RuntimeChallengeMode::Required(challenge_id.to_string()));
    }
    Err("鉴权服务暂时不可用，请稍后重试".to_string())
}

fn is_loopback_control_plane_url(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .map(|host| host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1")
        .unwrap_or(false)
}

pub(super) fn cookie_header_value(
    cookies: &BTreeMap<String, String>,
) -> Result<HeaderValue, String> {
    let value = cookies
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    HeaderValue::from_str(&value).map_err(|error| error.to_string())
}

fn h5_origin(profile: H5RequestProfile) -> &'static str {
    match profile {
        H5RequestProfile::Default => crate::sealed_protocol::sealed_str!("https://www.goofish.com"),
        H5RequestProfile::FishAd => crate::sealed_protocol::sealed_str!("https://dayu.goofish.com"),
        H5RequestProfile::SellerCommonPro => {
            crate::sealed_protocol::sealed_str!("https://seller.goofish.com")
        }
        H5RequestProfile::AwesomeDetailMobile => {
            crate::sealed_protocol::sealed_str!("https://h5.m.goofish.com")
        }
    }
}

fn h5_referer(profile: H5RequestProfile) -> &'static str {
    match profile {
        H5RequestProfile::Default => {
            crate::sealed_protocol::sealed_str!("https://www.goofish.com/")
        }
        H5RequestProfile::FishAd => {
            crate::sealed_protocol::sealed_str!("https://dayu.goofish.com/")
        }
        H5RequestProfile::SellerCommonPro => {
            crate::sealed_protocol::sealed_str!("https://seller.goofish.com/")
        }
        H5RequestProfile::AwesomeDetailMobile => {
            crate::sealed_protocol::sealed_str!("https://h5.m.goofish.com/")
        }
    }
}

pub(super) fn h5_headers(
    cookies: &BTreeMap<String, String>,
    profile: H5RequestProfile,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(DEFAULT_USER_AGENT));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    headers.insert("accept", HeaderValue::from_static("application/json"));
    headers.insert(
        "accept-language",
        HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static(h5_origin(profile)));
    headers.insert(REFERER, HeaderValue::from_static(h5_referer(profile)));
    headers.insert(
        "sec-ch-ua",
        HeaderValue::from_static(
            "\"Not(A:Brand\";v=\"99\", \"Google Chrome\";v=\"133\", \"Chromium\";v=\"133\"",
        ),
    );
    headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"macOS\""));
    headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-site"));
    if profile == H5RequestProfile::FishAd {
        headers.insert("idle_site_biz_code", HeaderValue::from_static("FISHAD"));
    }
    if profile == H5RequestProfile::SellerCommonPro {
        headers.insert("idle_site_biz_code", HeaderValue::from_static("COMMONPRO"));
        headers.insert("idle_user_group_member_id", HeaderValue::from_static(""));
    }
    if profile == H5RequestProfile::AwesomeDetailMobile {
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 15_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.4 Mobile/15E148 Safari/604.1",
            ),
        );
        headers.insert("accept-language", HeaderValue::from_static("zh-CN"));
        headers.insert(
            "sec-ch-ua",
            HeaderValue::from_static(
                "\"Google Chrome\";v=\"149\", \"Chromium\";v=\"149\", \"Not)A;Brand\";v=\"24\"",
            ),
        );
        headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"iOS\""));
        headers.remove(ORIGIN);
        headers.remove("sec-fetch-dest");
        headers.remove("sec-fetch-mode");
        headers.remove("sec-fetch-site");
    }
    headers.insert(COOKIE, cookie_header_value(cookies)?);
    Ok(headers)
}

pub(super) fn default_h5_headers(cookies: &BTreeMap<String, String>) -> Result<HeaderMap, String> {
    h5_headers(cookies, H5RequestProfile::Default)
}

pub(super) fn default_wx_headers(cookies: &BTreeMap<String, String>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(WX_USER_AGENT));
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://servicewechat.com/wx9882f2a891880616/76/page-frame.html"),
    );
    headers.insert(COOKIE, cookie_header_value(cookies)?);
    headers.insert("x-tap", HeaderValue::from_static("wx"));
    headers.insert(
        "accept",
        HeaderValue::from_static("application/json, text/plain, */*"),
    );
    headers.insert(
        "accept-language",
        HeaderValue::from_static("zh-CN,zh;q=0.9"),
    );
    Ok(headers)
}

pub(super) async fn send_h5_once(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    session: &mut super::session::AccountSession,
    api: &str,
    version: &str,
    data_str: &str,
    extra_params: &[(String, String)],
    profile: H5RequestProfile,
) -> Result<Value, String> {
    let token = extract_sign_token(session);
    if token.trim().is_empty() {
        return Err("missing h5 token".to_string());
    }
    let timestamp = unix_millis_now().to_string();
    let sign = format!(
        "{:x}",
        md5::compute(format!(
            "{}&{}&{}&{}",
            token, timestamp, config.xianyu.h5_app_key, data_str
        ))
    );
    let mut params = vec![
        ("jsv".to_string(), DEFAULT_H5_JSV.to_string()),
        ("appKey".to_string(), config.xianyu.h5_app_key.clone()),
        ("t".to_string(), timestamp),
        ("sign".to_string(), sign),
        ("v".to_string(), version.to_string()),
        ("type".to_string(), "originaljson".to_string()),
        ("dataType".to_string(), "json".to_string()),
        ("timeout".to_string(), "20000".to_string()),
        ("api".to_string(), api.to_string()),
        ("sessionOption".to_string(), "AutoLoginOnly".to_string()),
    ];
    params.extend(extra_params.iter().cloned());
    let url = format!(
        "{}/{}/{}/",
        trim_trailing_slash(&config.xianyu.h5_gateway),
        api,
        version
    );

    let response = client
        .post(url)
        .query(&params)
        .headers(h5_headers(&session.cookies, profile)?)
        .form(&[("data", data_str)])
        .send()
        .await
        .map_err(|error| error.to_string())?;
    apply_response_cookies(accounts, session, response.headers()).await?;
    let text = response.text().await.map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Err(format!("{api} returned empty response"));
    }
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

pub(super) async fn send_h5_with_token_retry(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    session: &mut super::session::AccountSession,
    api: &str,
    version: &str,
    data_str: &str,
    extra_params: &[(String, String)],
    profile: H5RequestProfile,
) -> Result<Value, String> {
    let first = send_h5_once(
        client,
        config,
        accounts,
        session,
        api,
        version,
        data_str,
        extra_params,
        profile,
    )
    .await?;
    if is_token_expired(&first) {
        return send_h5_once(
            client,
            config,
            accounts,
            session,
            api,
            version,
            data_str,
            extra_params,
            profile,
        )
        .await;
    }
    Ok(first)
}

pub(super) async fn send_wx_once(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    session: &mut super::session::AccountSession,
    api: &str,
    version: &str,
    data_str: &str,
) -> Result<Value, String> {
    let token = extract_sign_token(session);
    if token.trim().is_empty() {
        return Err("missing h5 token".to_string());
    }
    let timestamp = unix_millis_now().to_string();
    let sign = format!(
        "{:x}",
        md5::compute(format!(
            "{}&{}&{}&{}",
            token, timestamp, config.xianyu.wx_app_key, data_str
        ))
    );
    let c_value = format!(
        "{};{}",
        session.cookies.get("_m_h5_tk").cloned().unwrap_or_default(),
        session
            .cookies
            .get("_m_h5_tk_enc")
            .cloned()
            .unwrap_or_default()
    );
    let params = vec![
        ("jsv".to_string(), config.xianyu.wx_jsv.clone()),
        ("appKey".to_string(), config.xianyu.wx_app_key.clone()),
        ("t".to_string(), timestamp),
        ("sign".to_string(), sign),
        ("c".to_string(), c_value),
        ("v".to_string(), version.to_string()),
        ("type".to_string(), "originaljson".to_string()),
        ("accountSite".to_string(), "xianyu".to_string()),
        ("dataType".to_string(), "json".to_string()),
        ("timeout".to_string(), "20000".to_string()),
        ("api".to_string(), api.to_string()),
        ("needLogin".to_string(), "true".to_string()),
        ("ecode".to_string(), "1".to_string()),
        ("_bx-m".to_string(), "0.0.11".to_string()),
    ];

    let url = format!(
        "{}/h5/{}/{}/",
        trim_trailing_slash(&config.xianyu.wx_gateway),
        api,
        version
    );
    let response = client
        .post(url)
        .query(&params)
        .headers(default_wx_headers(&session.cookies)?)
        .form(&[("data", data_str)])
        .send()
        .await
        .map_err(|error| error.to_string())?;
    apply_response_cookies(accounts, session, response.headers()).await?;
    let text = response.text().await.map_err(|error| error.to_string())?;
    if text.trim().is_empty() {
        return Err(format!("{api} returned empty response"));
    }
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

pub(super) async fn send_wx_with_token_retry(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    session: &mut super::session::AccountSession,
    api: &str,
    version: &str,
    data_str: &str,
) -> Result<Value, String> {
    let first = send_wx_once(client, config, accounts, session, api, version, data_str).await?;
    if is_token_expired(&first) {
        return send_wx_once(client, config, accounts, session, api, version, data_str).await;
    }
    Ok(first)
}

pub(super) async fn send_app_once(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data_str: &str,
) -> Result<Value, String> {
    let session = load_account_session(accounts, uid).await?;
    let sid = session.cookies.get("cookie2").cloned().unwrap_or_default();
    let license_context = get_sign_license_context()
        .ok_or_else(|| "signing blocked: no active licensed context".to_string())?;
    let control_client = control_plane_client()?;
    let sign_cache_scope = format!(
        "{:x}",
        md5::compute(format!(
            "{}\x00{}\x00{}",
            license_context.server_url, license_context.device_id, license_context.signed_token
        ))
    );
    let account_app_headers = super::utils::load_account_app_headers(accounts, uid).await?;

    let sign_profile_headers = app_headers_for_account_or_default(account_app_headers.as_ref());
    let (sign_headers, request_headers, current_t) = if let Some(cached) =
        get_cached_sign(&sign_cache_scope, uid, api, data_str)
    {
        cached
    } else {
        let timestamp = unix_millis_now() / 1000;
        let sign_url = format!(
            "{}/sign",
            trim_trailing_slash(&config.xianyu.sign_service_url)
        );
        let x_location = percent_decode_lossy(
            sign_profile_headers
                .get("x-location")
                .map(String::as_str)
                .unwrap_or(""),
        );
        let (sign_lat, sign_lng) = if let Some((lng, lat)) = x_location.split_once(',') {
            (lat.to_string(), lng.to_string())
        } else {
            app_template_lat_lng()
        };
        let challenge_mode = fetch_runtime_sign_challenge(
            &control_client,
            config,
            &license_context.server_url,
            &license_context,
            &config.sqlite.data_dir,
        )
        .await?;
        let mut sign_body = json!({
            "api": api,
            "data": data_str,
            "version": version,
            "t": timestamp.to_string(),
            "uid": uid,
            "sid": sid,
            "utdid": sign_profile_headers.get("x-utdid").cloned().unwrap_or_default(),
            "ttid": percent_decode_lossy(sign_profile_headers.get("x-ttid").map(String::as_str).unwrap_or("")),
            "appkey": sign_profile_headers.get("x-appkey").cloned().unwrap_or_default(),
            "features": sign_profile_headers.get("x-features").cloned().unwrap_or_default(),
            "extdata": percent_decode_lossy(sign_profile_headers.get("x-extdata").map(String::as_str).unwrap_or("")),
            "lat": sign_lat,
            "lng": sign_lng,
            "timeout": 15,
        });
        if let RuntimeChallengeMode::Required(challenge_id) = challenge_mode {
            sign_body["challengeId"] = Value::String(challenge_id);
        }
        let sign_body_bytes = serde_json::to_vec(&sign_body)
            .map_err(|error| format!("sign service body JSON: {error}"))?;
        let mut sign_text = String::new();
        for attempt in 0..TRANSIENT_SERVICE_RETRY_COUNT {
            let proof_mode = crate::license_runtime::select_runtime_proof_mode(
                &control_client,
                &license_context.server_url,
                &license_context,
                &config.sqlite.data_dir,
                "POST",
                "/sign",
                &sign_body_bytes,
            )
            .await?;
            let sign_resp = match apply_runtime_proof_headers(
                apply_sign_service_access_headers(
                    client
                        .post(&sign_url)
                        .bearer_auth(&license_context.auth_token)
                        .json(&sign_body)
                        .timeout(std::time::Duration::from_secs(20)),
                    config,
                ),
                &proof_mode,
                &license_context,
            )
            .send()
            .await
            {
                Ok(response) => response,
                Err(error) => {
                    if attempt + 1 < TRANSIENT_SERVICE_RETRY_COUNT
                        && (error.is_timeout() || error.is_connect())
                    {
                        sleep_before_transient_retry(attempt).await;
                        continue;
                    }
                    return Err(format!("sign service request failed: {error}"));
                }
            };
            let sign_status = sign_resp.status();
            sign_text = sign_resp
                .text()
                .await
                .map_err(|error| format!("sign service response read failed: {error}"))?;
            if sign_status.is_success() {
                break;
            }
            if attempt + 1 < TRANSIENT_SERVICE_RETRY_COUNT
                && should_retry_service_response(sign_status, &sign_text)
            {
                sleep_before_transient_retry(attempt).await;
                continue;
            }
            return Err(sign_service_error(sign_status, &sign_text));
        }
        let sign_result: Value = serde_json::from_str(&sign_text)
            .map_err(|error| format!("sign service JSON parse: {error}"))?;
        if let Some(err) = sign_result.get("error").and_then(Value::as_str) {
            return Err(format!("sign service error: {err}"));
        }
        let sh = sign_result
            .get("signHeaders")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let updated_input = sign_result
            .get("updatedInput")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let rh = sign_result
            .get("requestHeaders")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let ct = rh
            .get("x-t")
            .and_then(Value::as_str)
            .or_else(|| updated_input.get("t").and_then(Value::as_str))
            .unwrap_or_else(|| {
                sign_body
                    .get("t")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            })
            .to_string();
        set_cached_sign(&sign_cache_scope, uid, api, data_str, &sh, &rh, &ct);
        (sh, rh, ct)
    };

    let mtop_url = format!(
        "{}/{}/{}/",
        trim_trailing_slash(&config.xianyu.app_mtop_gateway),
        api,
        version
    );
    let mut headers = HeaderMap::new();
    let template_headers = app_headers_for_account_or_default(account_app_headers.as_ref());
    if template_headers.is_empty() {
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded;charset=UTF-8"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(APP_USER_AGENT));
        headers.insert(
            "accept",
            HeaderValue::from_static("application/json, text/plain, */*"),
        );
        for (key, val) in &request_headers {
            if let Some(v) = val.as_str() {
                if !v.is_empty() {
                    if let (Ok(hname), Ok(hval)) = (
                        reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                        HeaderValue::from_str(v),
                    ) {
                        headers.insert(hname, hval);
                    }
                }
            }
        }
    } else {
        for (key, value) in template_headers {
            if key.eq_ignore_ascii_case("content-length") || value.is_empty() {
                continue;
            }
            if let (Ok(hname), Ok(hval)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.insert(hname, hval);
            }
        }
    }
    headers.insert(COOKIE, cookie_header_value(&session.cookies)?);

    let now_secs = unix_millis_now() / 1000;
    let now_millis = unix_millis_now();
    let launch_info = format!("0,0,{now_millis},{},2", now_millis.saturating_sub(2000));
    set_header(&mut headers, "c-launch-info", &launch_info)?;
    let traceid = format!("null{now_millis}0{}", now_millis % 100000);
    set_header(&mut headers, "x-c-traceid", &traceid)?;
    set_header(&mut headers, "x-nettype", "WIFI")?;
    set_header(&mut headers, "x-nq", "WIFI")?;
    set_header(&mut headers, "x-custom-cache-uid", uid)?;

    set_header(&mut headers, "x-t", &current_t)?;
    set_header(&mut headers, "x-uid", uid)?;
    set_header(&mut headers, "x-sid", &sid)?;
    set_header(&mut headers, "m-pv", "6.3")?;

    let appkey = request_headers
        .get("x-appkey")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            template_headers
                .get("x-appkey")
                .cloned()
                .unwrap_or_else(|| app_template_header("x-appkey"))
        });
    if !appkey.is_empty() {
        set_header(&mut headers, "m-appkey", &appkey)?;
    }
    if !current_t.is_empty() {
        set_header(&mut headers, "m-t", &current_t)?;
    }

    if let Some(value) = sign_headers.get("x-sign").and_then(Value::as_str) {
        let encoded = percent_encode_component(value);
        set_header(&mut headers, "x-sign", &encoded)?;
        set_header(&mut headers, "m-sign", value)?;
    }
    if let Some(value) = sign_headers.get("x-sgext").and_then(Value::as_str) {
        set_header(&mut headers, "x-sgext", &percent_encode_component(value))?;
    }
    if let Some(value) = sign_headers.get("x-mini-wua").and_then(Value::as_str) {
        set_header(&mut headers, "x-mini-wua", &percent_encode_component(value))?;
    }
    if let Some(value) = sign_headers.get("x-umt").and_then(Value::as_str) {
        let encoded = percent_encode_component(value);
        set_header(&mut headers, "x-umt", &encoded)?;
        set_header(&mut headers, "umid", value)?;
    }

    let body = format!("data={}", percent_encode_component(data_str));

    let response = client
        .post(&mtop_url)
        .headers(headers)
        .body(body)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| format!("mtop app request failed: {error}"))?;
    let bxpunish = response
        .headers()
        .get("bxpunish")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        == "1";
    let text = response
        .text()
        .await
        .map_err(|error| format!("mtop app response read failed: {error}"))?;
    if bxpunish {
        return Err(format!(
            "device punished by anti-bot (bxpunish=1) for {api}, need CAPTCHA verification on emulator"
        ));
    }
    if text.trim().is_empty() {
        return Err(format!("{api} app request returned empty response"));
    }
    serde_json::from_str(&text).map_err(|error| format!("mtop app response JSON parse: {error}"))
}

pub(super) async fn do_stream_upload(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    filename: &str,
    content_type: &str,
    bytes: Vec<u8>,
    chat: bool,
) -> Result<UploadResult, String> {
    let mut session = load_account_session(accounts, uid).await?;
    let params = if chat {
        vec![
            ("floderId".to_string(), "0".to_string()),
            ("appkey".to_string(), "xy_chat".to_string()),
            ("_input_charset".to_string(), "utf-8".to_string()),
        ]
    } else {
        vec![
            ("floderId".to_string(), "0".to_string()),
            ("appkey".to_string(), "fleamarket".to_string()),
            ("_input_charset".to_string(), "utf-8".to_string()),
        ]
    };

    let part = Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str(content_type)
        .map_err(|error| error.to_string())?;
    let form = if chat {
        Form::new().part("file", part)
    } else {
        Form::new()
            .text("bizCode", "fleamarket")
            .text("name", "fileFromAlbum")
            .text("content-type", "multipart/form-data")
            .part("file", part)
    };

    let user_agent = if chat {
        CHAT_USER_AGENT
    } else {
        STREAM_USER_AGENT
    };
    let referer = if chat {
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/")
    } else {
        "https://servicewechat.com/wx9882f2a891880616/75/page-frame.html"
    };
    let origin = if chat {
        crate::sealed_protocol::sealed_str!("https://www.goofish.com")
    } else {
        "https://servicewechat.com"
    };
    let response = client
        .post(&config.xianyu.stream_upload_url)
        .query(&params)
        .header(USER_AGENT, user_agent)
        .header(REFERER, referer)
        .header(ORIGIN, origin)
        .header(COOKIE, cookie_header_value(&session.cookies)?)
        .multipart(form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    apply_response_cookies(accounts, &mut session, response.headers()).await?;
    let status = response.status();
    let payload = read_upload_json_payload(response, "stream upload").await?;
    let object = payload
        .get("object")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let url = object
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if url.is_empty() {
        return Err(format!(
            "upload response missing url (status={status}, body={})",
            truncate_debug_json(&payload)
        ));
    }
    let (width, height) = super::utils::parse_pix(
        object
            .get("pix")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    Ok(UploadResult { url, width, height })
}

pub(super) async fn do_taobao_upload(
    client: &Client,
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    filename: &str,
    content_type: &str,
    bytes: Vec<u8>,
) -> Result<UploadResult, String> {
    let mut session = load_account_session(accounts, uid).await?;
    let tb_token = session
        .cookies
        .get("_tb_token_")
        .cloned()
        .unwrap_or_default();
    let part = Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str(content_type)
        .map_err(|error| error.to_string())?;
    let form = Form::new()
        .text("type", "pic")
        .text("_tb_token_", tb_token)
        .text("bizCode", "fleamarket")
        .part("file", part);
    let response = client
        .post(&config.xianyu.upload_taobao_url)
        .header(USER_AGENT, WX_USER_AGENT)
        .header(
            REFERER,
            crate::sealed_protocol::sealed_str!("https://h5.m.goofish.com/"),
        )
        .header(
            ORIGIN,
            crate::sealed_protocol::sealed_str!("https://h5.m.goofish.com"),
        )
        .header(COOKIE, cookie_header_value(&session.cookies)?)
        .multipart(form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    apply_response_cookies(accounts, &mut session, response.headers()).await?;
    let status = response.status();
    let payload = read_upload_json_payload(response, "taobao upload").await?;
    let url = payload
        .get("url")
        .or_else(|| payload.get("imgUrl"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if url.is_empty() {
        return Err(format!(
            "taobao upload response missing url (status={status}, body={})",
            truncate_debug_json(&payload)
        ));
    }
    Ok(UploadResult {
        url,
        width: 0,
        height: 0,
    })
}

async fn read_upload_json_payload(
    response: reqwest::Response,
    label: &str,
) -> Result<Value, String> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("{label} response read failed: {error}"))?;
    let body = String::from_utf8_lossy(&bytes).to_string();
    serde_json::from_slice::<Value>(&bytes).map_err(|error| {
        format!(
            "{label} response JSON parse failed (status={status}): {error}; body={}",
            truncate_debug_text(&body)
        )
    })
}

fn truncate_debug_json(value: &Value) -> String {
    truncate_debug_text(&value.to_string())
}

fn truncate_debug_text(text: &str) -> String {
    const LIMIT: usize = 400;
    let trimmed = text.trim();
    if trimmed.chars().count() <= LIMIT {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(LIMIT).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_runtime_proof_headers, device_proof_header_device_id, device_proof_header_license_id,
        device_proof_header_nonce, device_proof_header_public_key, device_proof_header_signature,
        device_proof_header_timestamp, runtime_challenge_error, sign_service_error,
        RuntimeChallengeMode,
    };
    use crate::license_runtime::{DeviceProof, ProofMode, SignLicenseContext};
    use reqwest::header::HeaderValue;
    use reqwest::StatusCode;

    #[test]
    fn runtime_challenge_404_uses_skip_mode() {
        let mode = RuntimeChallengeMode::Skip;
        assert!(matches!(mode, RuntimeChallengeMode::Skip));
    }

    #[test]
    fn runtime_challenge_with_id_is_required_mode() {
        let mode = RuntimeChallengeMode::Required("challenge-123".to_string());
        match mode {
            RuntimeChallengeMode::Required(value) => assert_eq!(value, "challenge-123"),
            RuntimeChallengeMode::Skip => panic!("unexpected skip mode"),
        }
    }

    #[test]
    fn runtime_challenge_gateway_error_is_sanitized() {
        let error = runtime_challenge_error(
            StatusCode::BAD_GATEWAY,
            "<!DOCTYPE html><html><body>Bad Gateway</body></html>",
        );
        assert_eq!(error, "鉴权服务暂时不可用，请稍后重试");
    }

    #[test]
    fn sign_service_license_gateway_error_is_sanitized() {
        let error = sign_service_error(
            StatusCode::FORBIDDEN,
            r#"{"error":"signing blocked by license server HTTP 502: <!DOCTYPE html>"}"#,
        );
        assert_eq!(error, "鉴权服务暂时不可用，请稍后重试");
    }

    #[test]
    fn device_proof_headers_use_server_protocol_names() {
        let client = reqwest::Client::new();
        let proof_mode = ProofMode::Device(DeviceProof {
            public_key_hex: "pubkey".to_string(),
            signature_hex: "signature".to_string(),
            timestamp: 1_717_171_717,
            nonce: "nonce".to_string(),
            license_id: 42,
            device_id: "device-1".to_string(),
        });
        let license_context = SignLicenseContext {
            server_url: "https://license.example.com".to_string(),
            auth_token: "token".to_string(),
            signed_token: "signed".to_string(),
            license_id: 42,
            username: "test-user".into(),
            sku: "monthly".to_string(),
            account_limit: 10,
            hmac_secret_hex: "11".repeat(32),
            device_id: "device-1".to_string(),
        };

        let request = apply_runtime_proof_headers(
            client.post("http://127.0.0.1/runtime-challenge"),
            &proof_mode,
            &license_context,
        )
        .build()
        .expect("build request");
        let headers = request.headers();

        assert_eq!(
            headers.get(device_proof_header_public_key()),
            Some(&HeaderValue::from_static("pubkey"))
        );
        assert_eq!(
            headers.get(device_proof_header_signature()),
            Some(&HeaderValue::from_static("signature"))
        );
        assert_eq!(
            headers.get(device_proof_header_timestamp()),
            Some(&HeaderValue::from_static("1717171717"))
        );
        assert_eq!(
            headers.get(device_proof_header_nonce()),
            Some(&HeaderValue::from_static("nonce"))
        );
        assert_eq!(
            headers.get(device_proof_header_license_id()),
            Some(&HeaderValue::from_static("42"))
        );
        assert_eq!(
            headers.get(device_proof_header_device_id()),
            Some(&HeaderValue::from_static("device-1"))
        );

        assert!(headers.get("x-device-signature").is_none());
        assert!(headers.get("x-device-timestamp").is_none());
        assert!(headers.get("x-device-nonce").is_none());
        assert!(headers.get("x-device-license-id").is_none());
        assert!(headers.get("x-device-id").is_none());
    }
}
