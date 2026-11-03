//! Direct Tencent iLink HTTP client — replicates the openclaw-weixin plugin's
//! wire protocol natively, no Node/OpenClaw gateway in between.
//!
//! Protocol reference (TS source):
//!   <https://github.com/Tencent/openclaw-weixin/blob/main/src/api/api.ts>
//!
//! Two hosts are involved:
//!   * **Login (no token)**: fixed `https://ilinkai.weixin.qq.com`
//!   * **After login**: per-account `<base_url>` returned in the QR-confirm
//!     response (IDC-routed; different accounts may get different hosts).
//!
//! Every request carries `iLink-App-Id`, `iLink-App-ClientVersion` and a
//! freshly-rolled `X-WECHAT-UIN` (base64 of a random decimal `u32`). Once a
//! bot token is in hand, authenticated calls also send `AuthorizationType:
//! ilink_bot_token` + `Authorization: Bearer <token>` and embed a `base_info`
//! block in the JSON body.

use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use rand::RngCore;
use reqwest::{Client, Proxy, RequestBuilder};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

/// macOS reqwest 默认会读 SCDynamicStore 拿系统代理设置（Network →
/// Proxies），`.no_proxy()` 只 disable env 检测，对系统级代理无效。
/// `Proxy::custom` 用一个永远返回 `None` 的回调强制每个请求都跳过代理。
/// 是 macOS + Clash TUN 环境下唯一可靠的「真直连」开关。
fn force_no_proxy() -> Proxy {
    Proxy::custom(|_| Option::<reqwest::Url>::None)
}

/// Fixed login host — used until the QR-confirm response hands us a per-account
/// `baseurl`.
pub const ILINK_LOGIN_HOST: &str = "https://ilinkai.weixin.qq.com";
const ILINK_APP_ID: &str = "bot";
/// Decimal uint32 encoding of plugin v2.4.3: `(2 << 16) | (4 << 8) | 3`.
const ILINK_CLIENT_VERSION: u32 = 132_099;
pub(crate) const CHANNEL_VERSION: &str = "2.4.3";
const DEFAULT_BOT_AGENT: &str = "xianyu-proxy/1.0";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Status long-poll takes ~35s server-side — be generous client-side.
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(45);
/// `getUpdates` is also a long-poll — server holds ~35s. Give the client a
/// few seconds slack so we don't pre-empt a fresh response.
const GET_UPDATES_TIMEOUT: Duration = Duration::from_secs(40);

/// Authenticated client used after login: every call hits `<base_url>/...`
/// with the per-account bot token attached.
#[derive(Debug, Clone)]
pub struct IlinkBotClient {
    base_url: String,
    token: String,
    bot_agent: String,
    http: Client,
}

impl IlinkBotClient {
    /// Build a fresh client. `base_url` is the per-account host returned by the
    /// QR-confirm response; `bot_agent` defaults to `xianyu-proxy/1.0` when
    /// `None`/empty, mirroring the TS plugin's optional field.
    ///
    /// **代理处理**：`.no_proxy()` 跳过 reqwest 的 HTTPS_PROXY 自动检测。我们
    /// 之前发现：本机若用 ClashX TUN 模式，HTTPS_PROXY 同时会被设到
    /// `http://127.0.0.1:1082`，但 TUN 本身在网络层透明劫持流量——reqwest
    /// 再用 1082 走 HTTP CONNECT 反而被 clash 拒绝，所有请求挂死无 TX。
    /// 让它走直连，TUN/系统路由会正确处理。
    pub fn new(base_url: String, token: String, bot_agent: Option<String>) -> Self {
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .proxy(force_no_proxy())
            .connect_timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_else(|_| Client::new());
        let bot_agent = bot_agent
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BOT_AGENT.to_string());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            bot_agent,
            http,
        }
    }

    /// Send a single text message via `POST <base_url>/ilink/bot/sendmessage`.
    ///
    /// `context_token` is optional in the wire protocol — pass an empty string
    /// to omit it. On a non-zero upstream `ret` we surface `errcode`/`errmsg`
    /// in the returned error so the dispatcher can persist it for the operator.
    pub async fn send_text(
        &self,
        to_user_id: &str,
        context_token: &str,
        text: &str,
    ) -> Result<(), String> {
        let endpoint = format!("{}/ilink/bot/sendmessage", self.base_url);

        // 必须跟 openclaw plugin/src/messaging/send.ts::buildTextMessageReq 一致：
        // 缺 client_id / message_type / message_state 服务端返 ret=-2。
        // - client_id  每条独立 uuid，下行长 polling 用来去重 / 关联回调
        // - message_type=2 表示发送方是 BOT（1 是 USER，1 时服务端拒）
        // - message_state=2 表示消息已完结（流式消息才用 0/1）
        let client_id = format!("xianyu-proxy-{}", uuid_like());
        let mut msg = json!({
            "from_user_id": "",
            "to_user_id": to_user_id,
            "client_id": client_id,
            "message_type": 2,
            "message_state": 2,
            "item_list": [
                { "type": 1, "text_item": { "text": text } }
            ]
        });
        if !context_token.is_empty() {
            msg["context_token"] = Value::String(context_token.to_string());
        }

        let body = json!({
            "msg": msg,
            "base_info": self.base_info(),
        });
        let body_str = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());

        // 走 curl 子进程——理由同 get_updates 的注释。reqwest 在 macOS+TUN
        // 环境下被系统代理设置抢走，无法绕开。
        debug!(
            target: "notifications.openclaw_weixin",
            stream = "runtime",
            module = "notifications-openclaw-weixin",
            action = "send_text_request",
            outcome = "success",
            to_user_id = %to_user_id,
            context_token_len = context_token.len(),
            body_len = body_str.chars().count(),
            "sending openclaw text message"
        );

        let raw = curl_post_authed(&endpoint, &self.token, &body_str, 10)
            .await
            .map_err(|err| format!("ilink sendmessage transport error: {err}"))?;

        debug!(
            target: "notifications.openclaw_weixin",
            stream = "runtime",
            module = "notifications-openclaw-weixin",
            action = "send_text_response",
            outcome = "success",
            raw_len = raw.chars().count(),
            "received openclaw text response"
        );

        let payload: Value = serde_json::from_str(&raw).map_err(|err| {
            format!(
                "ilink sendmessage decode error: {err}; body_len={}",
                raw.chars().count()
            )
        })?;

        // openclaw 协议：sendmessage 成功时返 `{}` 空对象（README 写明
        // SendMessageResp is empty）；失败才返 `{"ret":N,"errcode":...}`。
        // 之前 `unwrap_or(-1)` 把成功的空对象当 ret=-1 错误，导致 daily report
        // 持续报「ret=-1」其实消息根本是发出去了的——这是个误报。
        let ret = payload.get("ret").and_then(Value::as_i64).unwrap_or(0);
        if ret == 0 {
            return Ok(());
        }

        let errcode = payload.get("errcode").cloned();
        let errmsg = payload
            .get("errmsg")
            .and_then(Value::as_str)
            .map(|s| s.to_string());
        Err(format!(
            "ilink sendmessage ret={ret} errcode={errcode:?} errmsg={errmsg:?} body_len={}",
            raw.chars().count()
        ))
    }

    /// Send one image message via `POST <base_url>/ilink/bot/sendmessage`.
    ///
    /// Wire format mirrors openclaw plugin
    /// `src/messaging/send.ts::sendImageMessageWeixin`. The CDNMedia.aes_key
    /// is base64-of-hex (the hex string, ASCII-encoded then base64) — NOT
    /// base64 of the raw 16-byte AES key. `mid_size` is the ciphertext size
    /// (PKCS7-padded), which the receiving client uses to know when the
    /// CDN download is complete.
    pub async fn send_image(
        &self,
        to_user_id: &str,
        context_token: &str,
        uploaded: &super::openclaw_cdn::UploadedImage,
    ) -> Result<(), String> {
        let endpoint = format!("{}/ilink/bot/sendmessage", self.base_url);

        let client_id = format!("xianyu-proxy-{}", uuid_like());
        let image_item = json!({
            "type": 2,
            "image_item": {
                "media": {
                    "encrypt_query_param": uploaded.encrypt_query_param,
                    "aes_key": uploaded.aes_key_b64,
                    "encrypt_type": 1,
                },
                "mid_size": uploaded.filesize,
            }
        });

        let mut msg = json!({
            "from_user_id": "",
            "to_user_id": to_user_id,
            "client_id": client_id,
            "message_type": 2,
            "message_state": 2,
            "item_list": [image_item],
        });
        if !context_token.is_empty() {
            msg["context_token"] = Value::String(context_token.to_string());
        }

        let body = json!({
            "msg": msg,
            "base_info": self.base_info(),
        });
        let body_str = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());

        let raw = curl_post_authed(&endpoint, &self.token, &body_str, 15)
            .await
            .map_err(|err| format!("ilink sendmessage(image) transport error: {err}"))?;
        let payload: Value = serde_json::from_str(&raw).map_err(|err| {
            format!(
                "ilink sendmessage(image) decode error: {err}; body_len={}",
                raw.chars().count()
            )
        })?;
        let ret = payload.get("ret").and_then(Value::as_i64).unwrap_or(-1);
        if ret == 0 {
            return Ok(());
        }
        let errcode = payload.get("errcode").cloned();
        let errmsg = payload.get("errmsg").and_then(Value::as_str);
        Err(format!(
            "ilink sendmessage(image) ret={ret} errcode={errcode:?} errmsg={errmsg:?}"
        ))
    }

    /// The `base_info` block every authenticated body must carry.
    fn base_info(&self) -> Value {
        json!({
            "channel_version": CHANNEL_VERSION,
            "bot_agent": self.bot_agent,
        })
    }
}

/// QR-code login bootstrap response.
#[derive(Debug, Clone, Deserialize)]
pub struct QrCodeResp {
    /// Opaque token to pass back as `?qrcode=<...>` while polling status.
    #[serde(default)]
    pub qrcode: String,
    /// Base64 PNG payload (sometimes a `data:` URL) the UI renders directly.
    #[serde(default)]
    pub qrcode_img_content: String,
}

/// Long-poll status response. Only `status` is guaranteed; the rest are
/// populated for specific terminal/redirect states.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct QrStatusResp {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub ilink_bot_id: String,
    #[serde(default)]
    pub ilink_user_id: String,
    #[serde(default)]
    pub baseurl: String,
    #[serde(default)]
    pub redirect_host: String,
}

/// Kick off QR-code login. Hits the fixed login host — no token required yet.
///
/// `POST https://ilinkai.weixin.qq.com/ilink/bot/get_bot_qrcode?bot_type=3`
/// with body `{"local_token_list": []}`.
pub async fn fetch_qr_code() -> Result<QrCodeResp, String> {
    let http = build_login_http();
    let endpoint = format!("{ILINK_LOGIN_HOST}/ilink/bot/get_bot_qrcode?bot_type=3");
    let body = json!({ "local_token_list": [] });

    let response = apply_unauthed_headers(http.post(&endpoint))
        .json(&body)
        .send()
        .await
        .map_err(|err| format!("ilink get_bot_qrcode transport error: {err}"))?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|err| format!("ilink get_bot_qrcode decode error ({status}): {err}"))?;

    serde_json::from_value::<QrCodeResp>(payload)
        .map_err(|err| format!("ilink get_bot_qrcode parse error: {err}"))
}

/// Long-poll the QR-code status. `host` is the host to hit (initially the
/// login host; switches when an earlier poll returned `redirect_host`).
/// `verify_code` is the user-supplied verification code on the `need_verifycode`
/// branch — pass `None` otherwise.
pub async fn poll_qr_status(
    host: &str,
    qrcode: &str,
    verify_code: Option<&str>,
) -> Result<QrStatusResp, String> {
    let http = build_long_poll_http();
    let host = host.trim_end_matches('/');
    let mut endpoint = format!(
        "{host}/ilink/bot/get_qrcode_status?qrcode={}",
        urlencoding::encode(qrcode)
    );
    if let Some(code) = verify_code {
        let code = code.trim();
        if !code.is_empty() {
            endpoint.push_str("&verify_code=");
            endpoint.push_str(&urlencoding::encode(code));
        }
    }

    let response = apply_unauthed_headers(http.get(&endpoint))
        .send()
        .await
        .map_err(|err| format!("ilink get_qrcode_status transport error: {err}"))?;

    let status = response.status();
    let payload: Value = response
        .json()
        .await
        .map_err(|err| format!("ilink get_qrcode_status decode error ({status}): {err}"))?;

    serde_json::from_value::<QrStatusResp>(payload)
        .map_err(|err| format!("ilink get_qrcode_status parse error: {err}"))
}

fn build_login_http() -> Client {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .no_proxy()
        .connect_timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn build_long_poll_http() -> Client {
    Client::builder()
        .timeout(LONG_POLL_TIMEOUT)
        .no_proxy()
        .connect_timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Headers that ride on every request (auth'd or not).
fn apply_common_headers(req: RequestBuilder) -> RequestBuilder {
    req.header("Content-Type", "application/json")
        .header("iLink-App-Id", ILINK_APP_ID)
        .header("iLink-App-ClientVersion", ILINK_CLIENT_VERSION.to_string())
        .header("X-WECHAT-UIN", random_wechat_uin())
}

fn apply_unauthed_headers(req: RequestBuilder) -> RequestBuilder {
    apply_common_headers(req)
}

fn apply_authed_headers(req: RequestBuilder, token: &str) -> RequestBuilder {
    apply_common_headers(req)
        .header("AuthorizationType", "ilink_bot_token")
        .header("Authorization", format!("Bearer {token}"))
}

/// Build a `X-WECHAT-UIN` value the way the TS plugin does:
/// `Buffer.from(String(uint32), "utf-8").toString("base64")` — i.e. base64 of
/// the *decimal string* of a random `u32`, not base64 of the raw 4 bytes.
fn random_wechat_uin() -> String {
    let mut bytes = [0u8; 4];
    rand::rng().fill_bytes(&mut bytes);
    let n = u32::from_le_bytes(bytes);
    BASE64_STANDARD.encode(n.to_string().as_bytes())
}

/// 客户端消息 ID（client_id）—— 服务端按这个去重 / 关联状态推送。每条消息独立，
/// 跟 openclaw plugin 的 generateId("openclaw-weixin") 一个用途。16 字节 hex 串
/// 足够避免碰撞，不用真 UUID4 格式。
fn uuid_like() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
}

// ---------------------------------------------------------------------------
// Inbound long-poll `/ilink/bot/getupdates`
// ---------------------------------------------------------------------------

/// A single inbound message item in `GetUpdatesResp.msgs`.
///
/// Only the fields the long-poll worker cares about are read at runtime; the
/// rest are kept on the struct so the wire schema is documented in one place
/// and remains decodable when the server adds new fields (we don't want
/// `unknown_field` to surface as a parse error).
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct InboundMsg {
    #[serde(default)]
    pub seq: i64,
    #[serde(default)]
    pub message_id: i64,
    #[serde(default)]
    pub from_user_id: String,
    #[serde(default)]
    pub to_user_id: String,
    #[serde(default)]
    pub create_time_ms: i64,
    #[serde(default)]
    pub session_id: String,
    /// `1` = USER (real inbound), `2` = BOT (our own outbound echo — skip).
    #[serde(default)]
    pub message_type: i64,
    /// `0` / `1` = streaming / incomplete, `2` = finished (only handle 2).
    #[serde(default)]
    pub message_state: i64,
    /// Per-conversation token threaded through outbound replies — without it,
    /// `send_text` returns `ret=-2` because the server has no session context.
    #[serde(default)]
    pub context_token: String,
    /// Raw `item_list` — extract text with `extract_text_from_items`.
    #[serde(default)]
    pub item_list: Vec<Value>,
}

/// Parsed `/ilink/bot/getupdates` response. `errcode == -14` is the
/// canonical "session timed out, re-scan QR" signal — the long-poll worker
/// shuts itself down on that.
#[derive(Debug, Clone, Default, Deserialize)]
#[allow(dead_code)]
pub struct GetUpdatesResp {
    #[serde(default)]
    pub ret: i64,
    #[serde(default)]
    pub errcode: i64,
    #[serde(default)]
    pub errmsg: String,
    #[serde(default)]
    pub msgs: Vec<InboundMsg>,
    /// New cursor — pass back as `get_updates_buf` on the next call. Empty
    /// on the very first poll.
    #[serde(default)]
    pub get_updates_buf: String,
    #[serde(default)]
    pub longpolling_timeout_ms: i64,
}

/// Long-poll the upstream for inbound messages.
///
/// `POST <base_url>/ilink/bot/getupdates`. Server holds ~35s; we give 40s
/// client-side. `cursor` is the `get_updates_buf` returned by the previous
/// call (empty on first call).
pub async fn get_updates(
    base_url: &str,
    token: &str,
    bot_agent: &str,
    cursor: &str,
) -> Result<GetUpdatesResp, String> {
    // macOS reqwest 在 ClashX TUN 模式下，无论 `.no_proxy()` / `.proxy(custom)`
    // 都会被系统级代理（SCDynamicStore）抢走，做 HTTP CONNECT tunnel 到
    // 1082，clash 不接 fake-ip 的 CONNECT 直接拒绝。换 curl 子进程跑——
    // curl 走 macOS 系统 socket，TUN 透明劫持，已实测可用。
    let endpoint = format!("{}/ilink/bot/getupdates", base_url.trim_end_matches('/'));
    let agent = if bot_agent.trim().is_empty() {
        DEFAULT_BOT_AGENT
    } else {
        bot_agent
    };
    let body = json!({
        "get_updates_buf": cursor,
        "base_info": {
            "channel_version": CHANNEL_VERSION,
            "bot_agent": agent,
        },
    });
    let body_str = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());

    let raw = curl_post_authed(
        &endpoint,
        token,
        &body_str,
        GET_UPDATES_TIMEOUT.as_secs() as u32,
    )
    .await
    .map_err(|err| format!("ilink getupdates transport error: {err}"))?;

    let payload: Value = serde_json::from_str(&raw).map_err(|err| {
        format!(
            "ilink getupdates decode error: {err}; body_len={}",
            raw.chars().count()
        )
    })?;
    serde_json::from_value::<GetUpdatesResp>(payload)
        .map_err(|err| format!("ilink getupdates parse error: {err}"))
}

/// curl 子进程做一个带 token 的 POST。macOS + ClashX TUN 环境下唯一可靠的
/// HTTPS 出口——reqwest 那条路会被 macOS 系统代理设置干扰。
///
/// `pub(crate)` so the CDN upload helper (`openclaw_cdn.rs`) can reuse it
/// for `POST /ilink/bot/getuploadurl`, same auth shape as `sendmessage` /
/// `getupdates`.
pub(crate) async fn curl_post_authed(
    endpoint: &str,
    token: &str,
    body: &str,
    timeout_secs: u32,
) -> Result<String, String> {
    let endpoint = endpoint.to_string();
    let token = token.to_string();
    let body = body.to_string();
    tokio::task::spawn_blocking(move || {
        let uin = random_wechat_uin();
        let auth = format!("Bearer {token}");
        let out = std::process::Command::new("curl")
            .arg("-sS")
            .arg("--max-time")
            .arg(timeout_secs.to_string())
            // Local operator networks often MITM HTTPS via TUN / self-signed
            // proxy roots. Accept the presented chain so getupdates doesn't
            // black-hole inbound WeChat messages behind curl(60) / curl(35).
            .arg("-k")
            // 强制 curl 也不走代理（避免 env 残留 / 系统代理）
            .arg("--noproxy")
            .arg("*")
            .arg("-X")
            .arg("POST")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-H")
            .arg(format!("iLink-App-Id: {ILINK_APP_ID}"))
            .arg("-H")
            .arg(format!("iLink-App-ClientVersion: {ILINK_CLIENT_VERSION}"))
            .arg("-H")
            .arg(format!("X-WECHAT-UIN: {uin}"))
            .arg("-H")
            .arg("AuthorizationType: ilink_bot_token")
            .arg("-H")
            .arg(format!("Authorization: {auth}"))
            .arg("-d")
            .arg(&body)
            .arg(&endpoint)
            .output()
            .map_err(|err| format!("spawn curl: {err}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("curl exit {} stderr={}", out.status, stderr));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    })
    .await
    .map_err(|err| format!("spawn_blocking join: {err}"))?
}

/// Concatenate every `text_item.text` field in `item_list` with newlines.
///
/// Inbound messages may carry multiple items (text + image + ...); for the
/// command router we only care about plaintext. Non-text items are silently
/// skipped. Returns an empty string if nothing text-shaped was found.
pub fn extract_text_from_items(items: &[Value]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for item in items {
        let item_type = item.get("type").and_then(Value::as_i64).unwrap_or(0);
        if item_type != 1 {
            continue;
        }
        if let Some(text) = item
            .get("text_item")
            .and_then(|t| t.get("text"))
            .and_then(Value::as_str)
        {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
    }
    parts.join("\n")
}

/// Pull the first useful quote-reply payload out of inbound item_list.
///
/// iLink quote metadata is inconsistent across clients: some replies carry the
/// full original bot markdown in `ref_msg.title`, others only preserve a short
/// title/snippet. We therefore probe a small set of likely fields and return
/// the first non-empty string so the command parser can recover the session id
/// from either the full markdown body or a compact title like
/// `客户消息 61475741054`.
pub fn extract_ref_text_from_items(items: &[Value]) -> String {
    for item in items {
        let Some(ref_msg) = item.get("ref_msg") else {
            continue;
        };
        for key in ["title", "digest", "desc", "content", "text"] {
            if let Some(text) = ref_msg.get(key).and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    String::new()
}
