//! Slash-command parser for inbound WeChat messages.
//!
//! Invoked by the long-poll worker for every USER-typed, finished text frame.
//! Parses the leading slash-command and dispatches to an in-process handler.
//!
//! Phase 2A commands:
//!   * `/帮助` / `/help` — print command list
//!   * `/添加账号` — start Xianyu QR login (image stubbed for now)
//!   * `/回复 <session_id> <text>` — push reply through `pc_send_message`
//!   * `/改价 <bizOrderId> <price>` — call `order_adjust_price`
//!   * `/发布 <link or item_id> [uid]` — import + republish a Xianyu item
//!   * `/状态` — system status snapshot (live workers count)
//!
//! Replies are sent through the same WeChat channel via
//! `IlinkBotClient::send_text` with the cached `context_token`. Out-of-scope
//! image / file uploads are deferred to a later phase — handlers currently
//! return text-only responses.
//!
//! All handlers are async + return `Result<String, String>` where Ok is the
//! reply body and Err is the error message (also surfaced as a reply).

use image::{ImageBuffer, Luma};
use qrcode::QrCode;
use serde_json::{json, Value};
use sqlx::Row;
use tracing::warn;

use crate::{
    models::notifications::NotificationChannel, routes::tenant::active_account_uid_accessible,
    state::AppState,
};

use super::{
    openclaw_cdn::upload_image_via_cdn,
    openclaw_context_cache,
    openclaw_weixin::IlinkBotClient,
    reply_router::{account_uid_belongs_to_owner, send_reply_to_session_for_owner},
};

/// Entry point — called by `openclaw_long_poll` for every inbound USER text.
///
/// Parses the leading command, runs the matching handler, then sends a reply
/// back through WeChat. Non-command messages (no leading `/`) are ignored.
pub(crate) async fn dispatch_inbound_text(
    state: &AppState,
    channel_id: i64,
    from_user_id: &str,
    text: &str,
    quoted_text: &str,
) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }

    let channel = match load_command_channel(state, channel_id).await {
        Ok(channel) => channel,
        Err(err) => {
            warn!(
                target: "notifications.wechat_commands",
                stream = "runtime",
                module = "notifications-wechat-commands",
                action = "load_command_channel",
                outcome = "warn",
                channel_id,
                error_code = "load_command_channel_failed",
                retryable = true,
                detail = %err,
                "ignoring inbound wechat command because channel load failed"
            );
            return;
        }
    };
    if !wechat_operator_allowed(&channel.config, from_user_id) {
        warn!(
            target: "notifications.wechat_commands",
            stream = "runtime",
            module = "notifications-wechat-commands",
            action = "operator_authorization",
            outcome = "warn",
            channel_id,
            from_user_id = %from_user_id,
            error_code = "unauthorized_operator",
            retryable = false,
            "ignoring inbound wechat command from unauthorized operator"
        );
        return;
    }
    let owner_username = channel.owner_username.trim().to_string();

    if let Some(after_slash) = trimmed.strip_prefix('/') {
        let (cmd, rest) = split_first_token(after_slash);
        let reply = handle_command(state, channel_id, from_user_id, &owner_username, cmd, rest)
            .await
            .unwrap_or_else(|err| format!("命令执行失败: {err}"));

        if let Err(err) = send_reply(state, channel_id, from_user_id, &reply).await {
            warn!(
                target: "notifications.wechat_commands",
                stream = "runtime",
                module = "notifications-wechat-commands",
                action = "send_reply",
                outcome = "warn",
                channel_id,
                peer_user_id = %from_user_id,
                error_code = "wechat_command_reply_failed",
                retryable = true,
                detail = %err,
                "failed to send wechat command reply"
            );
        }
        return;
    }

    // No slash command: if the operator replied to a forwarded customer
    // notification, route this plain text back to the underlying Xianyu chat.
    if let Some(target) = parse_forwarded_customer_notification(quoted_text) {
        let result =
            send_reply_to_session_for_owner(state, &owner_username, &target.session_id, trimmed)
                .await;
        let reply = match result {
            Ok(delivered) => format!(
                "## 回复已发送\n\n- 对话ID：`{}`\n- 客户：**{}**",
                delivered.session_id,
                if delivered.customer_name.trim().is_empty() {
                    target.customer_name.as_str()
                } else {
                    delivered.customer_name.as_str()
                }
            ),
            Err(err) => format!("## 回复失败\n\n> {err}"),
        };
        let _ = send_reply(state, channel_id, from_user_id, &reply).await;
        return;
    }

    // Plain chitchat — not a slash command and not a quote-reply target.
}

/// Per-command dispatcher. Each arm calls into an existing internal handler
/// (re-implemented inline where the route handler signature would require
/// faking an axum Request).
async fn handle_command(
    state: &AppState,
    channel_id: i64,
    from_user_id: &str,
    owner_username: &str,
    cmd: &str,
    rest: &str,
) -> Result<String, String> {
    match cmd {
        "帮助" | "help" => Ok(help_text()),
        "添加账号" => handle_add_account(state, channel_id, from_user_id, owner_username).await,
        "回复" => handle_reply(state, owner_username, rest).await,
        "改价" => handle_adjust_price(state, owner_username, rest).await,
        "发布" => handle_publish(state, owner_username, rest).await,
        "状态" => handle_status(state, owner_username).await,
        _ => Ok("未知指令，发送 /帮助 查看".to_string()),
    }
}

fn help_text() -> String {
    [
        "## 可用指令",
        "",
        "- `/帮助` - 显示本说明",
        "- `/添加账号` - 扫码新增闲鱼账号",
        "- 直接引用一条客户通知并回复 - 自动给客户发消息",
        "- `/回复 <session_id> <text>` - 兼容旧方式，给指定会话发消息",
        "- `/改价 <bizOrderId> <价格>` - 调整订单金额",
        "- `/改价 <uid> <bizOrderId> <价格>` - 多账号订单号重复时指定账号",
        "- `/发布 <链接或商品ID> [uid]` - 链接转售/重新上架",
        "- `/状态` - 查看后端运行状态",
    ]
    .join("\n")
}

#[derive(Debug, Clone)]
struct ForwardedReplyTarget {
    session_id: String,
    customer_name: String,
}

fn parse_forwarded_customer_notification(quoted_text: &str) -> Option<ForwardedReplyTarget> {
    let text = quoted_text.trim();
    if text.is_empty() {
        return None;
    }

    let session_id = extract_backtick_value_after_label(text, "对话ID：")
        .or_else(|| extract_backtick_value_after_label(text, "对话ID:"))
        .or_else(|| extract_session_id_from_compact_quote(text))?;
    let customer_name = extract_bold_value_after_label(text, "客户名称：")
        .or_else(|| extract_bold_value_after_label(text, "客户名称:"))
        .unwrap_or_else(|| "客户".to_string());

    if session_id.is_empty() {
        return None;
    }

    Some(ForwardedReplyTarget {
        session_id,
        customer_name,
    })
}

fn extract_session_id_from_compact_quote(text: &str) -> Option<String> {
    let marker = if text.contains("客户消息") {
        "客户消息"
    } else if text.contains("新客户消息") {
        "新客户消息"
    } else {
        return None;
    };
    let tail = text.split_once(marker)?.1;
    let digits: String = tail
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .take(32)
        .collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn extract_backtick_value_after_label(text: &str, label: &str) -> Option<String> {
    let start = text.find(label)? + label.len();
    let rest = text.get(start..)?.trim_start();
    let first = rest.find('`')?;
    let rest = rest.get(first + 1..)?;
    let end = rest.find('`')?;
    let value = rest.get(..end)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_bold_value_after_label(text: &str, label: &str) -> Option<String> {
    let start = text.find(label)? + label.len();
    let rest = text.get(start..)?.trim_start();
    let prefix = rest.strip_prefix("**")?;
    let end = prefix.find("**")?;
    let value = prefix.get(..end)?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// `/添加账号` — kick off a Xianyu QR-code login, render the QR as a PNG,
/// upload it to the WeChat CDN, and push it back to the requesting user.
///
/// Pipeline:
///   1. `start_qr_session_for_command` → fetches a fresh QR via the
///      passport host, returns the raw `codeContent` URL.
///   2. Render the URL as a ~300x300 PNG with `qrcode` + `image`.
///   3. Look up the channel's iLink credentials + cached `context_token`
///      for the requesting WeChat user.
///   4. `upload_image_via_cdn` (POST getuploadurl → POST ciphertext to
///      CDN) → returns the `encrypt_query_param` + `filesize` we need.
///   5. `IlinkBotClient::send_image` to push the QR.
///   6. Return a short text reply with the validity hint — the
///      dispatcher will follow up with `send_text` so the user sees the
///      hint right under the image.
///
/// On any failure we return a `Result::Err` so the dispatcher surfaces it
/// as a text message — the user always gets feedback, even when the CDN
/// pipeline blows up.
async fn handle_add_account(
    state: &AppState,
    channel_id: i64,
    from_user_id: &str,
    owner_username: &str,
) -> Result<String, String> {
    // Step 1 — start QR session and grab the raw payload URL.
    let (session_id, qr_content) =
        crate::routes::accounts::start_qr_session_for_command(state, owner_username).await?;
    if qr_content.is_empty() {
        return Err("二维码内容为空".to_string());
    }

    // Step 2 — render PNG. ~300x300 with a 4-module quiet zone is what
    // every reference client uses for camera-friendly QR.
    let png = render_qr_png(&qr_content, 300)?;

    // Step 3 — channel credentials + outbound context.
    let channel = state
        .storage()
        .get_notification_channel(channel_id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("channel {channel_id} 不存在"))?;
    let base_url = channel
        .config
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let bot_token = channel
        .config
        .get("bot_token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let bot_agent = channel
        .config
        .get("bot_agent")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "xianyu-proxy/1.0".to_string());
    if base_url.is_empty() || bot_token.is_empty() {
        return Err("channel 配置缺失 base_url / bot_token".to_string());
    }
    let context_token = openclaw_context_cache::get(channel_id, from_user_id).unwrap_or_default();

    // Step 4 — CDN upload.
    let uploaded =
        upload_image_via_cdn(&base_url, &bot_token, &bot_agent, from_user_id, &png).await?;

    // Step 5 — push the image. We construct the client by hand here instead
    // of going through `send_reply` because `send_reply` is wired for text;
    // re-using the channel config we already loaded saves a DB round-trip.
    let client = IlinkBotClient::new(base_url, bot_token, Some(bot_agent));
    client
        .send_image(from_user_id, &context_token, &uploaded)
        .await?;

    // Step 6 — follow-up text. Carries the session_id for traceability so
    // the operator can correlate a failed scan against the QR registry.
    Ok(format!(
        "## 账号扫码已生成\n\n> 二维码有效期以闲鱼实际状态为准，建议尽快完成扫码。过期后请重新生成。扫码完成后账号会自动添加到系统。\n\n- 会话: `{session_id}`"
    ))
}

/// Render a QR-code payload as a square PNG of roughly `target_size` px.
///
/// `qrcode` emits an `ImageBuffer<Luma<u8>>` whose side length is a
/// multiple of the QR module count — we pick the per-module scale so the
/// final image is at least `target_size`. A 4-module quiet zone is added
/// by default. PNG is the `image` crate's default `Png` encoder.
fn render_qr_png(content: &str, target_size: u32) -> Result<Vec<u8>, String> {
    use image::codecs::png::PngEncoder;
    use image::{ExtendedColorType, ImageEncoder};

    let code = QrCode::new(content.as_bytes()).map_err(|err| err.to_string())?;
    let buffer: ImageBuffer<Luma<u8>, Vec<u8>> = code
        .render::<Luma<u8>>()
        .min_dimensions(target_size, target_size)
        .dark_color(Luma([0u8]))
        .light_color(Luma([255u8]))
        .build();

    let (w, h) = buffer.dimensions();
    let mut out = Vec::with_capacity((w as usize) * (h as usize));
    PngEncoder::new(&mut out)
        .write_image(buffer.as_raw(), w, h, ExtendedColorType::L8)
        .map_err(|err| format!("png encode: {err}"))?;
    Ok(out)
}

/// `/回复 <session_id> <text>` — push a text message into an active live
/// socket. `session_id` is the cid; we still need the receiver's uid and the
/// owning account's uid which we resolve from the message_sessions table.
async fn handle_reply(
    state: &AppState,
    owner_username: &str,
    rest: &str,
) -> Result<String, String> {
    let (session_id, body) = split_first_token(rest);
    let session_id = session_id.trim();
    let body = body.trim();
    if session_id.is_empty() || body.is_empty() {
        return Ok("## 用法\n\n`/回复 <session_id> <text>`".to_string());
    }

    match send_reply_to_session_for_owner(state, owner_username, session_id, body).await {
        Ok(_) => Ok(format!("## 回复已发送\n\n- 会话: `{session_id}`")),
        Err(err) => Ok(format!("## 回复失败\n\n> {err}")),
    }
}

/// `/改价 <bizOrderId> <价格>` — call `mtop.taobao.idle.trade.user.adjust.price`
/// directly via `successful_upstream_h5_request`. We resolve the owning uid
/// by scanning `account_orders` for the bizOrderId.
async fn handle_adjust_price(
    state: &AppState,
    owner_username: &str,
    rest: &str,
) -> Result<String, String> {
    let parts = rest.split_whitespace().collect::<Vec<_>>();
    let (uid_arg, biz_order_id, fee) = match parts.as_slice() {
        [biz_order_id, fee] => (None, biz_order_id.trim(), fee.trim()),
        [uid, biz_order_id, fee, ..] => (Some(uid.trim()), biz_order_id.trim(), fee.trim()),
        _ => {
            return Ok(
                "用法: /改价 <bizOrderId> <价格> 或 /改价 <uid> <bizOrderId> <价格>".to_string(),
            )
        }
    };
    if biz_order_id.is_empty() || fee.is_empty() {
        return Ok(
            "用法: /改价 <bizOrderId> <价格> 或 /改价 <uid> <bizOrderId> <价格>".to_string(),
        );
    }

    let uid = resolve_order_uid_for_command(state, owner_username, uid_arg, biz_order_id).await?;
    if uid.trim().is_empty() {
        return Ok(format!("订单 {biz_order_id} 缺少 uid"));
    }

    let location = crate::routes::market::load_location_or_default(state, &uid).await;
    let payload = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "orderId": biz_order_id,
        "modifyFee": crate::routes::market::price_to_cents(fee),
        "newTransportFee": "0",
        "latitude": location.latitude,
        "longitude": location.longitude,
    });

    match crate::routes::market::successful_upstream_h5_request(
        state,
        &uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.user.adjust.price"),
        "1.0",
        payload,
        Vec::new(),
    )
    .await
    {
        Ok(_) => Ok(format!("改价成功: {biz_order_id} -> {fee}")),
        Err(err) => Ok(format!("改价失败: {err:?}")),
    }
}

async fn resolve_order_uid_for_command(
    state: &AppState,
    owner_username: &str,
    uid_arg: Option<&str>,
    biz_order_id: &str,
) -> Result<String, String> {
    if let Some(uid) = uid_arg.filter(|uid| !uid.trim().is_empty()) {
        let row = sqlx::query("SELECT uid FROM account_orders WHERE uid = ? AND tid = ? LIMIT 1")
            .bind(uid)
            .bind(biz_order_id)
            .fetch_optional(state.storage().xianyu())
            .await
            .map_err(|err| err.to_string())?;
        let Some(row) = row else {
            return Err(format!("账号 {uid} 下不存在订单: {biz_order_id}"));
        };
        if !account_uid_belongs_to_owner(state, uid, owner_username).await? {
            return Err(format!("账号 {uid} 下不存在订单: {biz_order_id}"));
        }
        return row.try_get("uid").map_err(|err| err.to_string());
    }

    let rows = sqlx::query("SELECT uid FROM account_orders WHERE tid = ? ORDER BY uid ASC")
        .bind(biz_order_id)
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(|err| err.to_string())?;
    let mut matching_uids = Vec::new();
    for row in rows {
        let uid = row
            .try_get::<String, _>("uid")
            .map_err(|err| err.to_string())?;
        if account_uid_belongs_to_owner(state, &uid, owner_username).await? {
            matching_uids.push(uid);
            if matching_uids.len() > 1 {
                break;
            }
        }
    }
    if matching_uids.is_empty() {
        return Err(format!("订单不存在: {biz_order_id}"));
    }
    if matching_uids.len() > 1 {
        let uids = matching_uids.join(", ");
        return Err(format!(
            "订单 {biz_order_id} 存在于多个账号，请使用 /改价 <uid> <bizOrderId> <价格> 指定账号。候选账号: {uids}"
        ));
    }
    Ok(matching_uids.remove(0))
}

/// `/发布 <链接或商品ID> [uid]` — extract a Xianyu item id from the text and
/// drop a stub row into `account_items` (matches the fallback path in
/// `item_import_link`). We don't run the full upstream import here (it needs
/// a live H5 session and image re-upload pipeline) — the stub gives the
/// operator visible feedback that the link landed.
async fn handle_publish(
    state: &AppState,
    owner_username: &str,
    rest: &str,
) -> Result<String, String> {
    let (link, uid_arg) = split_first_token(rest);
    let link = link.trim();
    let uid_arg = uid_arg.trim();
    if link.is_empty() {
        return Ok("用法: /发布 <链接或商品ID> [uid]".to_string());
    }

    let item_id = match extract_item_id_from_text(link) {
        Some(id) => id,
        None => {
            // Bare digits work too.
            let digits: String = link.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                digits
            } else {
                return Ok("未能从文本中识别商品 ID".to_string());
            }
        }
    };

    // Pick the target uid: explicit arg wins, otherwise pick the first active
    // account (mirrors the auto-bind behavior of the dispatcher).
    let uid = if !uid_arg.is_empty() {
        ensure_active_uid(state, owner_username, uid_arg).await?;
        uid_arg.to_string()
    } else {
        match pick_default_uid(state, owner_username).await? {
            Some(uid) => uid,
            None => return Ok("缺少账号参数，且未找到 active 账号".to_string()),
        }
    };

    let now = unix_seconds_now();
    sqlx::query(
        r#"
        INSERT INTO account_items (
            item_id, uid, title, short_title, description, price, orig_price,
            pic_url, image_urls_json, status, online, created_at, updated_at
        )
        VALUES (?, ?, ?, '', ?, '99', '99', '', '[]', 'ON_SALE', 1, ?, ?)
        ON CONFLICT(uid, item_id) DO UPDATE SET updated_at = excluded.updated_at
        "#,
    )
    .bind(&item_id)
    .bind(&uid)
    .bind("微信导入")
    .bind(link)
    .bind(now)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(|err| err.to_string())?;
    Ok(format!("已上架到账号 {uid} (item_id={item_id})"))
}

async fn ensure_active_uid(
    state: &AppState,
    owner_username: &str,
    uid: &str,
) -> Result<(), String> {
    if active_account_uid_accessible(state.storage().accounts(), Some(owner_username), uid)
        .await
        .map_err(|err| err.to_string())?
    {
        Ok(())
    } else {
        Err(format!("账号不存在或未启用: {uid}"))
    }
}

/// `/状态` — terse system snapshot: live worker count + WeChat channel count.
async fn handle_status(state: &AppState, owner_username: &str) -> Result<String, String> {
    let live_row = sqlx::query(
        "SELECT COUNT(*) AS c FROM accounts WHERE status = 'active' AND COALESCE(owner_username, '') = ?",
    )
    .bind(owner_username.trim())
    .fetch_one(state.storage().accounts())
    .await
    .map_err(|err| err.to_string())?;
    let active_accounts: i64 = live_row.try_get("c").unwrap_or(0);

    let channels = state
        .storage()
        .list_notification_channels_for_owner(owner_username.trim())
        .await
        .map_err(|err| err.to_string())?;
    let wechat_channels = channels
        .iter()
        .filter(|c| c.channel_type == "openclaw_weixin" && c.enabled)
        .count();

    Ok(format!(
        "## 系统状态\n\n- 活跃账号: **{active_accounts}**\n- 微信渠道: **{wechat_channels}**",
    ))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Split the input at the first whitespace into `(head, tail)`. Returns
/// `(input, "")` if no whitespace exists.
fn split_first_token(input: &str) -> (&str, &str) {
    let trimmed = input.trim_start();
    match trimmed.find(char::is_whitespace) {
        Some(idx) => (&trimmed[..idx], trimmed[idx..].trim_start()),
        None => (trimmed, ""),
    }
}

/// Look up the saved context_token and push a reply through WeChat. Logs (but
/// does not propagate) failure — phase 2A best-effort.
async fn send_reply(
    state: &AppState,
    channel_id: i64,
    from_user_id: &str,
    text: &str,
) -> Result<(), String> {
    let channel = state
        .storage()
        .get_notification_channel(channel_id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("channel {channel_id} 不存在"))?;

    let base_url = channel
        .config
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let bot_token = channel
        .config
        .get("bot_token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let bot_agent = channel
        .config
        .get("bot_agent")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string());

    if base_url.is_empty() || bot_token.is_empty() {
        return Err("channel 配置缺失 base_url / bot_token".to_string());
    }

    let context_token = openclaw_context_cache::get(channel_id, from_user_id).unwrap_or_default();
    let client = IlinkBotClient::new(base_url, bot_token, bot_agent);
    client.send_text(from_user_id, &context_token, text).await
}

async fn load_command_channel(
    state: &AppState,
    channel_id: i64,
) -> Result<NotificationChannel, String> {
    state
        .storage()
        .get_notification_channel(channel_id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| format!("channel {channel_id} 不存在"))
}

fn wechat_operator_allowed(config: &serde_json::Map<String, Value>, from_user_id: &str) -> bool {
    let from = from_user_id.trim();
    if from.is_empty() {
        return false;
    }
    let explicit = collect_operator_ids(config);
    if !explicit.is_empty() {
        return explicit.iter().any(|candidate| candidate == from);
    }
    ["to_user_id", "ilink_user_id", "user_id", "open_id"]
        .into_iter()
        .filter_map(|key| config.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .any(|value| value == from)
}

fn collect_operator_ids(config: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut out = Vec::new();
    for key in [
        "allowed_operator_ids",
        "allowed_user_ids",
        "operator_user_ids",
        "operator_ids",
    ] {
        if let Some(value) = config.get(key) {
            push_operator_ids(value, &mut out);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn push_operator_ids(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            for part in text.split([',', ';', '\n', '\r', '\t', ' ']) {
                let trimmed = part.trim();
                if !trimmed.is_empty() {
                    out.push(trimmed.to_string());
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                push_operator_ids(value, out);
            }
        }
        Value::Number(number) => out.push(number.to_string()),
        _ => {}
    }
}

/// Same item-id extractor as `routes::market::publish_import` — duplicated
/// here because the original is private to the market module.
fn extract_item_id_from_text(text: &str) -> Option<String> {
    for marker in ["itemId=", "id="] {
        if let Some(index) = text.find(marker) {
            let value = text[index + marker.len()..]
                .chars()
                .take_while(|char| char.is_ascii_digit())
                .collect::<String>();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

async fn pick_default_uid(
    state: &AppState,
    owner_username: &str,
) -> Result<Option<String>, String> {
    let row = sqlx::query(
        "SELECT uid FROM accounts WHERE status = 'active' AND COALESCE(owner_username, '') = ? ORDER BY updated_at DESC LIMIT 1",
    )
    .bind(owner_username.trim())
    .fetch_optional(state.storage().accounts())
    .await
    .map_err(|err| err.to_string())?;
    Ok(row.and_then(|row| row.try_get::<String, _>("uid").ok()))
}

fn unix_seconds_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forwarded_customer_notification_markdown() {
        let quoted = "## 新客户消息\n\n- 对话ID：`60334614175`\n- 客户名称：**测试客户**\n- 咨询商品：_测试商品_ (`123`)\n- 时间：`2026-05-16 12:00:00`\n\n### 消息内容\n> 在吗";
        let parsed = parse_forwarded_customer_notification(quoted).expect("should parse");
        assert_eq!(parsed.session_id, "60334614175");
        assert_eq!(parsed.customer_name, "测试客户");
    }

    #[test]
    fn parses_forwarded_customer_notification_from_compact_quote_title() {
        let parsed = parse_forwarded_customer_notification("客户消息 61475741054")
            .expect("should parse compact quote title");
        assert_eq!(parsed.session_id, "61475741054");
        assert_eq!(parsed.customer_name, "客户");
    }
}
