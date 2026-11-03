pub(crate) mod http;
mod session;
mod utils;

use std::collections::BTreeMap;

use reqwest::header::SET_COOKIE;
use serde_json::{json, Value};
use sqlx::{Pool, Sqlite};

use crate::config::AppConfig;

// ── Constants ────────────────────────────────────────────────────────────────

const DEFAULT_H5_JSV: &str = "2.7.2";
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";
const CHAT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";
const STREAM_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.50 NetType/WIFI Language/zh_CN";
const WX_USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.50";
const COOKIE_KEYS: &[&str] = &[
    "_m_h5_tk",
    "_m_h5_tk_enc",
    "_tb_token_",
    "cookie2",
    "sgcookie",
    "t",
    "unb",
];

const APP_USER_AGENT: &str = "MTOPSDK/3.1.1.7 (Android; appKey=21407387)";

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LocationContext {
    pub fish_ad_code: String,
    pub gps: String,
    pub latitude: String,
    pub longitude: String,
    pub name: String,
    pub address: String,
    pub city: String,
    pub province: String,
    pub area: String,
}

impl Default for LocationContext {
    fn default() -> Self {
        // 之前默认是「株洲荷塘区 430202」(老 Python 项目祖传值)。改成全空 —
        // 调用方（publish / message location / chat session）应当从账号的
        // `publish_location` 配置读取，配置缺失时应显式提示「请先设置发布地」
        // 而不是悄悄落到一个写死的随机城市。
        Self {
            fish_ad_code: String::new(),
            gps: String::new(),
            latitude: String::new(),
            longitude: String::new(),
            name: String::new(),
            address: String::new(),
            city: String::new(),
            province: String::new(),
            area: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UploadResult {
    pub url: String,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum H5RequestProfile {
    #[default]
    Default,
    FishAd,
    SellerCommonPro,
    AwesomeDetailMobile,
}

// ── Public API ────────────────────────────────────────────────────────────────

pub async fn load_location(accounts: &Pool<Sqlite>, uid: &str) -> Result<LocationContext, String> {
    session::load_location_from_db(accounts, uid).await
}

pub async fn h5_request(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    h5_request_inner(
        config,
        accounts,
        uid,
        api,
        version,
        data,
        extra_params,
        H5RequestProfile::Default,
    )
    .await
}

pub async fn h5_request_with_profile(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
    profile: H5RequestProfile,
) -> Result<Value, String> {
    h5_request_inner(
        config,
        accounts,
        uid,
        api,
        version,
        data,
        extra_params,
        profile,
    )
    .await
}

/// Calls an H5 endpoint that requires a platform-specific app key while
/// preserving the selected account session and the standard retry behavior.
pub async fn h5_request_with_app_key(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
    app_key: &str,
) -> Result<Value, String> {
    let mut request_config = config.clone();
    request_config.xianyu.h5_app_key = app_key.trim().to_string();
    h5_request_inner(
        &request_config,
        accounts,
        uid,
        api,
        version,
        data,
        extra_params,
        H5RequestProfile::Default,
    )
    .await
}

/// Makes an anonymous H5 request in an isolated public session. Public seller
/// timelines reject an account-bound cookie on some accounts, so this session
/// must not read from or write back to the selected seller account.
pub async fn anonymous_public_h5_request(
    config: &AppConfig,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    anonymous_public_h5_request_with_app_key_and_profile(
        config,
        api,
        version,
        data,
        extra_params,
        config.xianyu.h5_app_key.as_str(),
        H5RequestProfile::Default,
    )
    .await
}

/// Calls a public H5 endpoint with its required app key and browser profile.
/// This intentionally keeps cookies in memory only: public detail requests must
/// not inherit a selected seller account's risk-control state.
pub async fn anonymous_public_h5_request_with_app_key_and_profile(
    config: &AppConfig,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
    app_key: &str,
    profile: H5RequestProfile,
) -> Result<Value, String> {
    let mut request_config = config.clone();
    request_config.xianyu.h5_app_key = app_key.trim().to_string();
    let client = http::http_client()?;
    let data_str = serde_json::to_string(data).map_err(|error| error.to_string())?;
    let mut cookies = BTreeMap::new();

    // The first response establishes the short-lived `_m_h5_tk` used to sign
    // the actual public request. Its payload is intentionally ignored.
    let _ = anonymous_public_h5_once(
        &client,
        &request_config,
        api,
        version,
        &data_str,
        "undefined",
        &extra_params,
        &mut cookies,
        profile,
    )
    .await?;
    let token = public_h5_sign_token(&cookies)
        .ok_or_else(|| format!("anonymous public session bootstrap failed for {api}"))?;

    anonymous_public_h5_once(
        &client,
        &request_config,
        api,
        version,
        &data_str,
        &token,
        &extra_params,
        &mut cookies,
        profile,
    )
    .await
}

async fn anonymous_public_h5_once(
    client: &reqwest::Client,
    config: &AppConfig,
    api: &str,
    version: &str,
    data_str: &str,
    token: &str,
    extra_params: &[(String, String)],
    cookies: &mut BTreeMap<String, String>,
    profile: H5RequestProfile,
) -> Result<Value, String> {
    let timestamp = utils::unix_millis_now().to_string();
    let sign = format!(
        "{:x}",
        md5::compute(format!(
            "{token}&{timestamp}&{}&{data_str}",
            config.xianyu.h5_app_key
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
        utils::trim_trailing_slash(&config.xianyu.h5_gateway),
        api,
        version
    );
    let response = client
        .post(url)
        .query(&params)
        .headers(http::h5_headers(cookies, profile)?)
        .form(&[("data", data_str)])
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    merge_public_h5_cookies(cookies, response.headers());
    let body = response.text().await.map_err(|error| error.to_string())?;
    if body.trim().is_empty() {
        return Err(format!("{api} returned an empty response"));
    }
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

fn merge_public_h5_cookies(
    cookies: &mut BTreeMap<String, String>,
    headers: &reqwest::header::HeaderMap,
) {
    for header in headers.get_all(SET_COOKIE).iter() {
        let Ok(raw) = header.to_str() else {
            continue;
        };
        let Some((name, rest)) = raw.split_once('=') else {
            continue;
        };
        let value = rest.split(';').next().unwrap_or_default().trim();
        if !name.trim().is_empty() && !value.is_empty() {
            cookies.insert(name.trim().to_string(), value.to_string());
        }
    }
}

fn public_h5_sign_token(cookies: &BTreeMap<String, String>) -> Option<String> {
    cookies
        .get("_m_h5_tk")
        .map(|value| {
            value
                .split('_')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

pub(crate) async fn h5_request_no_session_repair(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    h5_request_inner(
        config,
        accounts,
        uid,
        api,
        version,
        data,
        extra_params,
        H5RequestProfile::Default,
    )
    .await
}

pub(crate) async fn h5_request_no_session_repair_with_profile(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
    profile: H5RequestProfile,
) -> Result<Value, String> {
    h5_request_inner(
        config,
        accounts,
        uid,
        api,
        version,
        data,
        extra_params,
        profile,
    )
    .await
}

async fn h5_request_inner(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
    extra_params: Vec<(String, String)>,
    profile: H5RequestProfile,
) -> Result<Value, String> {
    let client = http::http_client()?;
    let mut sess = session::load_account_session(accounts, uid).await?;
    let data_str = serde_json::to_string(data).map_err(|error| error.to_string())?;
    http::send_h5_with_token_retry(
        &client,
        config,
        accounts,
        &mut sess,
        api,
        version,
        &data_str,
        &extra_params,
        profile,
    )
    .await
}

pub async fn wx_request(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
) -> Result<Value, String> {
    wx_request_inner(config, accounts, uid, api, version, data).await
}

async fn wx_request_inner(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
) -> Result<Value, String> {
    let client = http::http_client()?;
    let mut sess = session::load_account_session(accounts, uid).await?;
    let data_str = serde_json::to_string(data).map_err(|error| error.to_string())?;
    http::send_wx_with_token_retry(
        &client, config, accounts, &mut sess, api, version, &data_str,
    )
    .await
}

pub async fn app_request(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    api: &str,
    version: &str,
    data: &Value,
) -> Result<Value, String> {
    let client = http::http_client()?;
    let data_str = serde_json::to_string(data).map_err(|error| error.to_string())?;
    http::send_app_once(&client, config, accounts, uid, api, version, &data_str).await
}

pub async fn stream_upload(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    filename: &str,
    content_type: &str,
    bytes: Vec<u8>,
    chat: bool,
) -> Result<UploadResult, String> {
    let client = http::http_client()?;
    http::do_stream_upload(
        &client,
        config,
        accounts,
        uid,
        filename,
        content_type,
        bytes,
        chat,
    )
    .await
}

pub async fn taobao_upload(
    config: &AppConfig,
    accounts: &Pool<Sqlite>,
    uid: &str,
    filename: &str,
    content_type: &str,
    bytes: Vec<u8>,
) -> Result<UploadResult, String> {
    let client = http::http_client()?;
    http::do_taobao_upload(
        &client,
        config,
        accounts,
        uid,
        filename,
        content_type,
        bytes,
    )
    .await
}

// ── Public helpers ────────────────────────────────────────────────────────────

pub fn location_json(location: &LocationContext) -> Value {
    json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "latitude": location.latitude,
        "longitude": location.longitude,
        "name": location.name,
        "address": location.address,
        "city": location.city,
        "province": location.province,
        "area": location.area,
    })
}

/// MTOP request signature.
///
/// `sign = md5(token + "&" + timestamp + "&" + appKey + "&" + data)` where
/// `token` is the leading segment of `_m_h5_tk` before the underscore. The
/// formula is Taobao's, not ours — do not "simplify". The unit tests pin the
/// exact byte layout so we notice if a crate upgrade (md5, format!) changes
/// behavior, and so we catch accidental drift in the inline call sites.
pub fn compute_mtop_sign(token: &str, timestamp: &str, app_key: &str, data: &str) -> String {
    format!(
        "{:x}",
        md5::compute(format!("{}&{}&{}&{}", token, timestamp, app_key, data))
    )
}

/// Strip the suffix from an `_m_h5_tk` value: `"abc123_1700000000000"` → `"abc123"`.
/// Empty or underscoreless input returns whatever precedes the first `_`
/// (or the whole input if none). Safe on empty/None cases.
pub fn sign_token_prefix(h5_tk: &str) -> &str {
    h5_tk.split('_').next().unwrap_or("")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xianyu_client::session::is_token_expired;

    #[test]
    fn sign_token_prefix_strips_suffix() {
        assert_eq!(sign_token_prefix("abc123_1700000000000"), "abc123");
    }

    #[test]
    fn sign_token_prefix_handles_no_underscore() {
        assert_eq!(sign_token_prefix("abc123"), "abc123");
    }

    #[test]
    fn sign_token_prefix_handles_empty() {
        assert_eq!(sign_token_prefix(""), "");
    }

    #[test]
    fn compute_mtop_sign_is_stable() {
        // Pins the exact Taobao MTOP signature formula. If this assertion
        // ever changes, Taobao's upstream will start returning
        // FAIL_SYS_ILLEGAL_ACCESS for every request — treat with care.
        let sign = compute_mtop_sign("abc123", "1700000000000", "34839810", r#"{"foo":"bar"}"#);
        // Pre-computed: md5("abc123&1700000000000&34839810&{\"foo\":\"bar\"}")
        assert_eq!(sign.len(), 32);
        assert!(sign.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministic reference value — any change means the formula drifted.
        assert_eq!(sign, "6af8a81454605276f5285c44e20fc058");
    }

    #[test]
    fn compute_mtop_sign_differs_on_any_field_change() {
        let base = compute_mtop_sign("tok", "1000", "key", "{}");
        assert_ne!(base, compute_mtop_sign("TOK", "1000", "key", "{}"));
        assert_ne!(base, compute_mtop_sign("tok", "1001", "key", "{}"));
        assert_ne!(base, compute_mtop_sign("tok", "1000", "KEY", "{}"));
        assert_ne!(base, compute_mtop_sign("tok", "1000", "key", "{ }"));
    }

    #[test]
    fn compute_mtop_sign_is_32_hex() {
        let sign = compute_mtop_sign("", "", "", "");
        // md5 of the literal "&&&" — known fixed value.
        assert_eq!(sign, "9c553730ef5b6c8c542bfd31b5e25b69");
    }

    #[test]
    fn is_token_expired_detects_typo_code() {
        // Taobao actually spells it EXOIRED (sic).
        let payload = serde_json::json!({"ret": ["FAIL_SYS_TOKEN_EXOIRED::令牌过期"]});
        assert!(is_token_expired(&payload));
    }

    #[test]
    fn is_token_expired_detects_empty() {
        let payload = serde_json::json!({"ret": ["FAIL_SYS_TOKEN_EMPTY::令牌为空"]});
        assert!(is_token_expired(&payload));
    }

    #[test]
    fn is_token_expired_skips_success() {
        let payload = serde_json::json!({"ret": ["SUCCESS::ok"]});
        assert!(!is_token_expired(&payload));
    }
}
