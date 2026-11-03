//! 小程序 ACCS 通道 (wx live) 的 MTOP 入口/WebSocket 辅助 (split from accounts/mod.rs in B4.6).

use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use serde_json::{json, Map, Value};
use sqlx::Row;
use tracing::{info, warn};

use crate::{protocol_catalog::opcodes, state::AppState};

use super::runtime_access::{ensure_account_can_call_upstream, load_account_owner_username};
use super::*;

pub(crate) async fn fetch_wx_live_access_token(
    state: &AppState,
    uid: &str,
) -> Result<(String, String), ResponseError> {
    let owner_username = load_account_owner_username(state, uid).await?;
    ensure_account_can_call_upstream(state, owner_username.as_deref(), uid).await?;
    let row = load_account_row(state.storage().accounts(), uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let mut cookies = parse_json_object(row.get::<String, _>("cookies").as_str());
    let device_id = get_or_create_wx_device_id(state.storage().accounts(), uid)
        .await
        .map_err(ResponseError::internal)?;
    let data = json!({
        "utdid": WX_LIVE_UTDID,
        "platform": "ios",
        "miniAppVersion": WX_LIVE_MINI_APP_VERSION,
        "appKey": WX_LIVE_ACCS_APP_KEY,
        "deviceId": device_id,
    });

    let protocol = resolve_account_protocol(state, opcodes::ACCOUNT_WX_LOGIN_TOKEN)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let mut payload =
        send_wx_live_login_token_request(&cookies, &data, None, protocol.api(), protocol.version())
            .await?;
    let kind = state.record_mtop_response(protocol.api(), &payload);
    if kind.is_auth_recoverable() {
        let new_c = payload.get("c").and_then(Value::as_str).unwrap_or_default();
        if !new_c.trim().is_empty() {
            apply_wx_live_c_to_cookies(&mut cookies, new_c);
            // Python parity (`xianyu_wx_live_client.py:344-351 + 567-588`):
            // when `_m_h5_tk` rotates server-side via the round-2 new-c retry,
            // the new token MUST be persisted to `accounts.cookies` JSON,
            // otherwise next process restart pays the round-1 401 penalty
            // again (and again, every restart, for every account). Python
            // does this via `_apply_new_c_to_cookies` (memory) + immediate
            // `_persist_runtime_cookies` (DB). Rust previously updated only
            // the local `cookies` Map and dropped it on function return.
            if let Err(e) = persist_wx_live_cookies(state.storage().accounts(), uid, &cookies).await
            {
                warn!(
                    target: "accounts.wx_live",
                    stream = "runtime",
                    module = "wx-live",
                    action = "persist_runtime_cookies_failed",
                    outcome = "warn",
                    account_uid = %uid,
                    error_code = "persist_runtime_cookies_failed",
                    retryable = true,
                    detail = %e,
                    "failed to persist refreshed wx live cookies"
                );
            }
            payload = send_wx_live_login_token_request(
                &cookies,
                &data,
                Some(new_c.to_string()),
                protocol.api(),
                protocol.version(),
            )
            .await?;
            state.record_mtop_response(protocol.api(), &payload);
        }
    }
    if !payload_ret_success(&payload) {
        return Err(ResponseError::internal_message(format!(
            "wx.login.token failed: {}",
            first_ret_string(&payload)
        )));
    }

    let access_token = coerce_mtop_data(&payload)
        .get("accessToken")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ResponseError::internal_message("wx.login.token missing accessToken"))?;
    Ok((access_token, device_id))
}

async fn send_wx_live_login_token_request(
    cookies: &Map<String, Value>,
    data: &Value,
    c_override: Option<String>,
    api: &str,
    version: &str,
) -> Result<Value, ResponseError> {
    let data_str = serde_json::to_string(data).map_err(ResponseError::internal)?;
    let timestamp = unix_millis_now().to_string();
    let c_value = c_override.unwrap_or_else(|| {
        format!(
            "{};{}",
            json_string(cookies.get("_m_h5_tk")),
            json_string(cookies.get("_m_h5_tk_enc"))
        )
    });
    let token_part = c_value
        .split(';')
        .next()
        .unwrap_or_default()
        .split('_')
        .next()
        .unwrap_or_default()
        .to_string();
    let sign = format!(
        "{:x}",
        md5::compute(format!(
            "{}&{}&{}&{}",
            token_part, timestamp, WX_LIVE_MTOP_APP_KEY, data_str
        ))
    );
    let url = format!(
        "{}/h5/{}/{}/2.0/",
        trim_trailing_slash(wx_live_gateway().as_str()),
        api,
        version,
    );
    let client = reqwest::Client::builder()
        .timeout(wx_live_http_timeout())
        .build()
        .map_err(ResponseError::internal)?;
    let response = client
        .post(url)
        .query(&[
            ("jsv", WX_LIVE_JSV),
            ("appKey", WX_LIVE_MTOP_APP_KEY),
            ("t", timestamp.as_str()),
            ("sign", sign.as_str()),
            ("v", version),
            ("type", "originaljson"),
            ("accountSite", "xianyu"),
            ("dataType", "json"),
            ("timeout", "20000"),
            ("needLogin", "true"),
            ("ecode", "1"),
            ("api", api),
            ("c", c_value.as_str()),
        ])
        .header("Accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-tap", "wx")
        .header("x-smallstc", build_wx_live_smallstc(cookies))
        .header("User-Agent", WX_LIVE_USER_AGENT)
        .header("Referer", WX_LIVE_REFERER)
        .form(&[("data", data_str)])
        .send()
        .await
        .map_err(ResponseError::internal)?;
    response
        .json::<Value>()
        .await
        .map_err(ResponseError::internal)
}

/// Python parity: `_do_session_keepalive` in `xianyu_wx_live_client.py:540-565`.
///
/// Calls the lightweight `mtop.taobao.idlemessage.user.query` endpoint every
/// 10 minutes to keep the cookie session warm. If the response says
/// `_m_h5_tk` is expired, the server hands back a fresh `c` (token;token_enc)
/// — apply it locally **and** persist it to `accounts.cookies` JSON so the
/// next process restart doesn't pay the round-1 401 penalty.
///
/// Replaces the previous keepalive that re-fetched `mtop.wx.login.token` —
/// hitting login.token every 10 minutes is itself a risk-control signal
/// (looks like a client that can't keep its accessToken alive).
pub(crate) async fn do_wx_live_session_keepalive(
    state: &AppState,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner_username = load_account_owner_username(state, uid).await?;
    ensure_account_can_call_upstream(state, owner_username.as_deref(), uid).await?;
    let row = load_account_row(state.storage().accounts(), uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let mut cookies = parse_json_object(row.get::<String, _>("cookies").as_str());

    // Python sends only utdid / platform / miniAppVersion in the keepalive
    // body — no appKey, no deviceId. Mirror that exactly.
    let data = json!({
        "utdid": WX_LIVE_UTDID,
        "platform": "ios",
        "miniAppVersion": WX_LIVE_MINI_APP_VERSION,
    });

    let protocol = resolve_account_protocol(state, opcodes::ACCOUNT_IDLEMESSAGE_USER_QUERY)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let payload =
        send_wx_live_user_query_request(&cookies, &data, None, protocol.api(), protocol.version())
            .await?;
    let ret = payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if ret.to_uppercase().contains("TOKEN") && !ret.contains("SUCCESS") {
        // Server included a fresh `c` for our next request. Apply it to the
        // local Map AND write the whole cookies JSON back to `accounts.cookies`
        // so it survives process restart. Python does this via
        // `_persist_runtime_cookies` → `auth_bridge.persist_m_h5_tk`.
        let new_c = payload.get("c").and_then(Value::as_str).unwrap_or_default();
        if !new_c.trim().is_empty() {
            apply_wx_live_c_to_cookies(&mut cookies, new_c);
            persist_wx_live_cookies(state.storage().accounts(), uid, &cookies).await?;
            info!(
                target: "accounts.wx_live",
                stream = "runtime",
                module = "wx-live",
                action = "session_keepalive_cookie_refreshed",
                outcome = "success",
                account_uid = %uid,
                "wx live keepalive refreshed and persisted _m_h5_tk"
            );
        }
    }
    Ok(())
}

async fn send_wx_live_user_query_request(
    cookies: &Map<String, Value>,
    data: &Value,
    c_override: Option<String>,
    api: &str,
    version: &str,
) -> Result<Value, ResponseError> {
    let data_str = serde_json::to_string(data).map_err(ResponseError::internal)?;
    let timestamp = unix_millis_now().to_string();
    let c_value = c_override.unwrap_or_else(|| {
        format!(
            "{};{}",
            json_string(cookies.get("_m_h5_tk")),
            json_string(cookies.get("_m_h5_tk_enc"))
        )
    });
    let token_part = c_value
        .split(';')
        .next()
        .unwrap_or_default()
        .split('_')
        .next()
        .unwrap_or_default()
        .to_string();
    let sign = format!(
        "{:x}",
        md5::compute(format!(
            "{}&{}&{}&{}",
            token_part, timestamp, WX_LIVE_MTOP_APP_KEY, data_str
        ))
    );
    let url = format!(
        "{}/h5/{}/{}/2.0/",
        trim_trailing_slash(wx_live_gateway().as_str()),
        api,
        version,
    );
    let client = reqwest::Client::builder()
        .timeout(wx_live_http_timeout())
        .build()
        .map_err(ResponseError::internal)?;
    let response = client
        .post(url)
        .query(&[
            ("jsv", WX_LIVE_JSV),
            ("appKey", WX_LIVE_MTOP_APP_KEY),
            ("t", timestamp.as_str()),
            ("sign", sign.as_str()),
            ("v", version),
            ("type", "originaljson"),
            ("accountSite", "xianyu"),
            ("dataType", "json"),
            ("timeout", "10000"),
            ("needLogin", "true"),
            ("ecode", "1"),
            ("api", api),
            ("c", c_value.as_str()),
        ])
        .header("Accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-tap", "wx")
        .header("x-smallstc", build_wx_live_smallstc(cookies))
        .header("User-Agent", WX_LIVE_USER_AGENT)
        .header("Referer", WX_LIVE_REFERER)
        .form(&[("data", data_str)])
        .send()
        .await
        .map_err(ResponseError::internal)?;
    response
        .json::<Value>()
        .await
        .map_err(ResponseError::internal)
}

/// Write a fresh cookies snapshot back to `accounts.cookies` JSON column.
/// Used by `do_wx_live_session_keepalive` when the server hands us a new
/// `c` (refreshed `_m_h5_tk`/`_m_h5_tk_enc`). Updating just the cookies
/// column + `cookie_refreshed_at` — this is **not** a deep refresh
/// (no havana/hasLogin), only a token rotate.
async fn persist_wx_live_cookies(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    cookies: &Map<String, Value>,
) -> Result<(), ResponseError> {
    let cookies_json =
        serde_json::to_string(&Value::Object(cookies.clone())).map_err(ResponseError::internal)?;
    let token = cookies
        .get("_m_h5_tk")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let now = unix_millis_now() as f64 / 1000.0;
    sqlx::query(
        r#"
        UPDATE accounts
        SET token = ?,
            cookies = ?,
            cookie_refreshed_at = ?,
            updated_at = ?
        WHERE uid = ?
        "#,
    )
    .bind(token)
    .bind(cookies_json)
    .bind(now)
    .bind(now)
    .bind(uid)
    .execute(pool)
    .await
    .map_err(ResponseError::internal)?;
    Ok(())
}

/// Python parity (DEPLOYED `xianyu_wx_live_client.py:96-130 build_smallstc`,
/// NOT the older worktree which only had 5 fields).
///
/// Comment from deployed source:
///   "POC 阶段曾验证 4 字段 (unb/sid/cookie2/sgcookie) 就能过, 但那是小程
///   序登录账号; web QR 扫码登录的账号服务端会返回 FAIL_SYS_SESSION_EXPIRED,
///   需要补齐 POC 抓包里完整的'小程序登录状态'装饰字段, 让服务端相信这是
///   个 miniProgram session."
///
/// 18 fields total. Order doesn't matter (server parses as JSON object).
fn build_wx_live_smallstc(cookies: &Map<String, Value>) -> String {
    let unb = json_string(cookies.get("unb")).parse::<i64>().unwrap_or(0);
    let sid = json_string(cookies.get("cookie2"));
    json!({
        "st": "success",
        "loginScene": "miniProgramLogin",
        "unb": unb,
        "munb": unb,
        "sid": sid,
        "cookie2": sid,
        "sgcookie": json_string(cookies.get("sgcookie")),
        "csg": json_string(cookies.get("csg")),
        "snsType": "weixin_mini_program",
        "appEntrance": "wx_goofish",
        "resultCode": 100,
        "processFinished": true,
        "bindTag": "existed",
        "loginResult": "success",
        "h5LoginFlag": true,
        "miniProgramCookieSync": true,
        "smartlock": false,
    })
    .to_string()
}

fn apply_wx_live_c_to_cookies(cookies: &mut Map<String, Value>, c_value: &str) {
    let mut parts = c_value.split(';');
    if let Some(token) = parts.next() {
        if !token.trim().is_empty() {
            cookies.insert(
                "_m_h5_tk".to_string(),
                Value::String(token.trim().to_string()),
            );
        }
    }
    if let Some(token_enc) = parts.next() {
        if !token_enc.trim().is_empty() {
            cookies.insert(
                "_m_h5_tk_enc".to_string(),
                Value::String(token_enc.trim().to_string()),
            );
        }
    }
}

fn wx_live_gateway() -> String {
    env::var("XIANYU_WX_GATEWAY").unwrap_or_else(|_| WX_LIVE_MTOP_GATEWAY().to_string())
}

pub(crate) fn wx_live_ws_url() -> String {
    env::var("XIANYU_WX_WS_URL").unwrap_or_else(|_| WX_LIVE_WS_URL.to_string())
}

pub(crate) fn wx_live_http_timeout() -> Duration {
    duration_from_millis_env("XIANYU_WX_LIVE_HTTP_TIMEOUT_MS", Duration::from_secs(25))
}

pub(crate) fn wx_live_ws_connect_timeout() -> Duration {
    duration_from_millis_env(
        "XIANYU_WX_LIVE_WS_CONNECT_TIMEOUT_MS",
        Duration::from_secs(15),
    )
}

fn duration_from_millis_env(key: &str, default: Duration) -> Duration {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or(default)
}

fn trim_trailing_slash(value: &str) -> &str {
    value.trim_end_matches('/')
}

pub(crate) fn generate_live_mid() -> String {
    static LIVE_MID_COUNTER: AtomicU64 = AtomicU64::new(1);
    let counter = LIVE_MID_COUNTER.fetch_add(1, Ordering::Relaxed) % 1000;
    format!("{}{} 0", counter, unix_millis_now())
}

/// Generate a fresh random UUID-shaped device_id, e.g.
/// `65B65936-4CD8-1402-4507-A53D104BE408-17295534829`.
fn random_wx_device_id(uid: &str) -> String {
    use std::fs::File;
    use std::io::Read;
    let mut bytes = [0u8; 16];
    if File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .is_err()
    {
        // Best-effort fallback if /dev/urandom is unavailable.
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id() as u128;
        let mix1 = (nanos ^ (pid << 64)) as u64;
        let mix2 = ((nanos >> 64) ^ pid) as u64;
        bytes[0..8].copy_from_slice(&mix1.to_le_bytes());
        bytes[8..16].copy_from_slice(&mix2.to_le_bytes());
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}-{uid}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32],
    )
}

/// Fetch the persistent ACCS `device_id` for `uid`, generating a new random
/// UUID-shaped one on first use (or whenever the operator has cleared the
/// column to "rotate" the device identity).
///
/// Why this is a per-account column:
///   * Server does sticky routing by device_id. The previous Rust scheme
///     (`08E8FE9E-WXIM-{uid}`, a non-UUID literal) and the SHA256(uid) scheme
///     both deterministically produced one device_id per account, which means
///     once the server flagged a (uid, device_id) pair into "cycling" state
///     there was no way to recover.
///   * Storing the device_id lets us regenerate it for any account that ends
///     up stuck in cycling — the next reconnect picks up the new value and
///     gets a fresh server-side reputation.
///   * Persistence prevents unintended rotation: a benign restart should not
///     change the device_id, only an explicit regenerate (or first-time use).
pub(crate) async fn get_or_create_wx_device_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
) -> Result<String, sqlx::Error> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT wx_device_id FROM accounts WHERE uid = ?")
            .bind(uid)
            .fetch_optional(pool)
            .await?;
    if let Some(value) = existing {
        if !value.trim().is_empty() {
            return Ok(value);
        }
    }
    let new_did = random_wx_device_id(uid);
    sqlx::query("UPDATE accounts SET wx_device_id = ? WHERE uid = ?")
        .bind(&new_did)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(new_did)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, time::Duration};
    use tokio::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::const_new(());

    #[tokio::test]
    async fn wx_live_http_timeout_can_be_configured_for_tests() {
        let _guard = ENV_LOCK.lock().await;

        env::set_var("XIANYU_WX_LIVE_HTTP_TIMEOUT_MS", "50");
        assert_eq!(wx_live_http_timeout(), Duration::from_millis(50));

        env::remove_var("XIANYU_WX_LIVE_HTTP_TIMEOUT_MS");
    }

    #[tokio::test]
    async fn wx_live_ws_connect_timeout_can_be_configured_for_tests() {
        let _guard = ENV_LOCK.lock().await;

        env::set_var("XIANYU_WX_LIVE_WS_CONNECT_TIMEOUT_MS", "75");
        assert_eq!(wx_live_ws_connect_timeout(), Duration::from_millis(75));

        env::remove_var("XIANYU_WX_LIVE_WS_CONNECT_TIMEOUT_MS");
    }
}

/// Force-rotate this account's device_id (used when the worker observes an
/// account stuck in cycling). The next connect attempt will pick up the new
/// value via `get_or_create_wx_device_id`.
pub(crate) async fn regenerate_wx_device_id(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
) -> Result<String, sqlx::Error> {
    let new_did = random_wx_device_id(uid);
    sqlx::query("UPDATE accounts SET wx_device_id = ? WHERE uid = ?")
        .bind(&new_did)
        .bind(uid)
        .execute(pool)
        .await?;
    Ok(new_did)
}
