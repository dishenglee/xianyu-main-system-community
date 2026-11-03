//! Fan-out dispatcher — turn a `LiveMessageEvent` into outbound pushes on each
//! channel bound to the receiving account.
//!
//! Current retained send paths include `openclaw_weixin`, Feishu/Lark through
//! the Rust native runtime, and DingTalk/Enterprise WeChat through their own
//! robot URL protocols. Removed generic notification channels are rejected
//! before reaching this dispatcher.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
};

use serde_json::{Map, Value};
use tokio::{
    sync::Mutex,
    time::{sleep, Duration},
};

use crate::{routes::accounts::LiveMessageEvent, state::AppState};

use super::{
    driver::{
        config_string, openclaw_context_token_for_send, patch_runtime_error, render_driver_content,
        resolve_driver, send_feishu_interactive_with_runtime_parts, send_text_with_runtime_parts,
        send_text_with_runtime_parts_and_error_adapter, DriverContentRequest,
    },
    feishu_group_router::{
        append_group_message, find_or_create_group_for_session, FindOrCreateGroupOutcome,
    },
    feishu_notify_cards::{
        build_feishu_im_aggregate_card, latest_card_event, FeishuImNotificationCardEvent,
    },
    openclaw_cdn::upload_image_via_cdn,
    openclaw_context_cache,
    openclaw_weixin::IlinkBotClient,
    template::{NotificationContentKind, NotificationTemplate, RenderedNotificationContent},
};

/// Inbound event payload reserved for phase 2 (commands, daily reports, etc.).
/// Kept as an opaque enum so callers can already declare a `match` arm against
/// it without us locking in the field shape.
#[derive(Debug, Clone)]
pub enum NotificationPayload {
    IncomingMessage {/* future fields */},
}

#[derive(Debug, Clone)]
struct PendingFeishuImNotification {
    owner_username: String,
    uid: String,
    account_name: String,
    channel_id: i64,
    channel_type: String,
    config: Map<String, Value>,
    event: LiveMessageEvent,
}

#[derive(Debug, Default)]
struct FeishuAggregateBucket {
    flush_scheduled: bool,
    items: Vec<PendingFeishuImNotification>,
}

fn feishu_aggregate_store() -> &'static Mutex<HashMap<String, FeishuAggregateBucket>> {
    static STORE: OnceLock<Mutex<HashMap<String, FeishuAggregateBucket>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_platform_notification_event(event: &LiveMessageEvent) -> bool {
    matches!(event.session_type, 3 | 6 | 53)
        || event.peer_nick == "工作台通知"
        || event.peer_nick.starts_with("通知中心")
}

/// Walk all enabled notification routes for `uid` and fan out the IM event to
/// every matching channel. Two routing modes are honored:
///
/// 1. **`bind_all_accounts: true`** in the channel config — channel sees
///    events from EVERY active uid without needing per-account binding rows.
///    This is what the WeChat QR-shortcut auto-provisions.
/// 2. Explicit `account_notification_bindings` rows for this uid — the
///    "manual" per-account path used by every other channel type.
///
/// Both lists are merged and deduped by channel_id so a channel that is
/// both "bind-all" AND explicitly bound only sends once per event.
///
/// Fire-and-forget at the caller's level — this function awaits each
/// downstream send sequentially so a slow channel only holds up *its own*
/// notification, not the WS event pipeline (the IM event processor spawns
/// this whole call onto a tokio task).
///
/// Channels of retained types other than `openclaw_weixin` are handled by the
/// driver layer if implemented, otherwise they fail closed and update runtime.
pub async fn dispatch_message_event_to_channels(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
) {
    if is_platform_notification_event(event) {
        match state
            .storage()
            .get_platform_messages_enabled_for_uid(uid)
            .await
        {
            Ok(false) => {
                eprintln!(
                    "[notify.dispatch] uid={uid} session={} skip_platform_messages_disabled",
                    event.session_id
                );
                return;
            }
            Ok(true) => {}
            Err(err) => {
                eprintln!(
                    "[notify.dispatch] uid={uid} session={} keep_platform_message_on_setting_error: {err}",
                    event.session_id
                );
            }
        }
    }

    let owner = match state.storage().notification_owner_for_uid(uid).await {
        Ok(owner) => owner,
        Err(err) => {
            eprintln!("[notify.dispatch] uid={uid} skip — failed to resolve owner: {err}");
            return;
        }
    };
    let bindings = match state
        .storage()
        .list_notification_bindings_for_owner(Some(uid), Some(owner.as_str()))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("[notify.dispatch] uid={uid} skip — failed to list bindings: {err}");
            Vec::new()
        }
    };

    let bind_all_channels = match state
        .storage()
        .list_bind_all_notification_channels_for_uid(uid)
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("[notify.dispatch] uid={uid} skip bind-all — failed to list channels: {err}");
            Vec::new()
        }
    };

    if bindings.is_empty() && bind_all_channels.is_empty() {
        return;
    }

    if should_skip_degenerate_account_notification(uid, event) {
        eprintln!(
            "[notify.dispatch] uid={uid} session={} skip_same_peer peer_uid={} peer_nick={:?}",
            event.session_id, event.peer_uid, event.peer_nick
        );
        return;
    }

    eprintln!(
        "[notify.dispatch] uid={uid} session={} is_self={} bind_all={} explicit_bindings={}",
        event.session_id,
        event.is_self,
        bind_all_channels.len(),
        bindings.len()
    );
    let mut seen: HashSet<i64> = HashSet::new();

    // 1) bind-all channels — every active uid receives these.
    for channel in &bind_all_channels {
        if !channel.enabled {
            continue;
        }
        if !seen.insert(channel.id) {
            continue;
        }
        let rendered = render_incoming_message_for_channel(
            state,
            uid,
            &channel.channel_type,
            &channel.config,
            event,
        )
        .await;
        dispatch_one(
            state,
            &owner,
            uid,
            channel.id,
            &channel.channel_type,
            &channel.config,
            &rendered,
            event,
        )
        .await;
    }

    // 2) explicit per-uid bindings — skip channels already covered above.
    for binding in bindings {
        if !binding.enabled || !binding.channel_enabled {
            continue;
        }
        if !seen.insert(binding.channel_id) {
            continue;
        }
        let rendered = render_incoming_message_for_channel(
            state,
            uid,
            &binding.channel_type,
            &binding.channel_config,
            event,
        )
        .await;
        dispatch_one(
            state,
            &owner,
            uid,
            binding.channel_id,
            &binding.channel_type,
            &binding.channel_config,
            &rendered,
            event,
        )
        .await;
    }
}

/// Single-channel dispatch dispatcher — branches on channel_type. Kept
/// separate from the loop so both the per-uid path and the bind-all path
/// share the same arms.
async fn dispatch_one(
    state: &AppState,
    owner_username: &str,
    uid: &str,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    rendered: &RenderedNotificationContent,
    event: &LiveMessageEvent,
) {
    if matches!(channel_type, "feishu" | "lark") && should_route_feishu_session_group(event) {
        dispatch_feishu_session_group(
            state,
            owner_username,
            uid,
            channel_id,
            channel_type,
            config,
            rendered,
            event,
        )
        .await;
        return;
    }

    dispatch_legacy_channel_notification(
        state,
        owner_username,
        uid,
        channel_id,
        channel_type,
        config,
        rendered,
        event,
    )
    .await;
}

async fn dispatch_feishu_session_group(
    state: &AppState,
    owner_username: &str,
    uid: &str,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    rendered: &RenderedNotificationContent,
    event: &LiveMessageEvent,
) {
    let channel = match state.storage().get_notification_channel(channel_id).await {
        Ok(Some(channel)) => channel,
        Ok(None) => {
            eprintln!(
                "[notify.dispatch] uid={uid} channel={channel_id} type={channel_type} fallback legacy — channel missing"
            );
            dispatch_legacy_channel_notification(
                state,
                owner_username,
                uid,
                channel_id,
                channel_type,
                config,
                rendered,
                event,
            )
            .await;
            return;
        }
        Err(err) => {
            eprintln!(
                "[notify.dispatch] uid={uid} channel={channel_id} type={channel_type} fallback legacy — failed to load channel: {err}"
            );
            dispatch_legacy_channel_notification(
                state,
                owner_username,
                uid,
                channel_id,
                channel_type,
                config,
                rendered,
                event,
            )
            .await;
            return;
        }
    };

    match find_or_create_group_for_session(state, &channel, owner_username, uid, event).await {
        Ok(FindOrCreateGroupOutcome::Existing { mapping })
        | Ok(FindOrCreateGroupOutcome::Created { mapping }) => {
            if let Err(err) = append_group_message(state, &channel, &mapping, event).await {
                eprintln!(
                    "[notify.dispatch] uid={uid} channel={channel_id} session={} fallback legacy — group append failed: {err}",
                    event.session_id
                );
                dispatch_legacy_channel_notification(
                    state,
                    owner_username,
                    uid,
                    channel_id,
                    channel_type,
                    config,
                    rendered,
                    event,
                )
                .await;
            }
        }
        Ok(FindOrCreateGroupOutcome::Fallback { reason }) => {
            eprintln!(
                "[notify.dispatch] uid={uid} channel={channel_id} session={} fallback legacy — {reason}",
                event.session_id
            );
            dispatch_legacy_channel_notification(
                state,
                owner_username,
                uid,
                channel_id,
                channel_type,
                config,
                rendered,
                event,
            )
            .await;
        }
        Err(err) => {
            eprintln!(
                "[notify.dispatch] uid={uid} channel={channel_id} session={} fallback legacy — group route error: {err}",
                event.session_id
            );
            dispatch_legacy_channel_notification(
                state,
                owner_username,
                uid,
                channel_id,
                channel_type,
                config,
                rendered,
                event,
            )
            .await;
        }
    }
}

async fn dispatch_legacy_channel_notification(
    state: &AppState,
    owner_username: &str,
    uid: &str,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    rendered: &RenderedNotificationContent,
    event: &LiveMessageEvent,
) {
    match channel_type {
        "openclaw_weixin" => {
            dispatch_openclaw_weixin(
                state,
                uid,
                channel_id,
                channel_type,
                config,
                rendered,
                event,
            )
            .await;
        }
        "feishu" | "lark" if feishu_aggregate_window_ms(config) > 0 => {
            dispatch_feishu_aggregated(
                state,
                owner_username,
                uid,
                channel_id,
                channel_type,
                config,
                event,
            )
            .await;
        }
        _ => {
            let outcome = send_text_with_runtime_parts(
                state,
                channel_id,
                channel_type,
                config,
                rendered.body.as_str(),
                Some(rendered),
                &Map::new(),
            )
            .await;
            if matches!(channel_type, "feishu" | "lark") {
                if let Ok(outcome) = outcome {
                    if let Some(feishu_message_id) = outcome.message_id {
                        persist_feishu_reply_route(
                            state,
                            owner_username,
                            uid,
                            channel_id,
                            channel_type,
                            config,
                            event,
                            &feishu_message_id,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

fn should_route_feishu_session_group(event: &LiveMessageEvent) -> bool {
    event.session_type == 1 && !event.is_self && !is_platform_notification_event(event)
}

async fn dispatch_feishu_aggregated(
    state: &AppState,
    owner_username: &str,
    uid: &str,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    event: &LiveMessageEvent,
) {
    let window_ms = feishu_aggregate_window_ms(config);
    let key = feishu_aggregate_key(channel_id, config);
    let item = PendingFeishuImNotification {
        owner_username: owner_username.to_string(),
        uid: uid.to_string(),
        account_name: fetch_account_name(state, uid).await,
        channel_id,
        channel_type: channel_type.to_string(),
        config: config.clone(),
        event: event.clone(),
    };

    let should_spawn = {
        let mut store = feishu_aggregate_store().lock().await;
        let bucket = store.entry(key.clone()).or_default();
        bucket.items.push(item);
        if bucket.flush_scheduled {
            false
        } else {
            bucket.flush_scheduled = true;
            true
        }
    };

    if should_spawn {
        let state = state.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(window_ms)).await;
            flush_feishu_aggregate(&state, key).await;
        });
    }
}

async fn flush_feishu_aggregate(state: &AppState, key: String) {
    let items = {
        let mut store = feishu_aggregate_store().lock().await;
        store
            .remove(&key)
            .map(|bucket| bucket.items)
            .unwrap_or_default()
    };
    if items.is_empty() {
        return;
    }

    let batch_id = feishu_aggregate_batch_id(latest_channel_id(&items));
    let card_events = items
        .iter()
        .enumerate()
        .map(|(index, item)| FeishuImNotificationCardEvent {
            uid: item.uid.clone(),
            account_name: item.account_name.clone(),
            reply_route_key: feishu_aggregate_reply_route_key(&batch_id, index),
            event: item.event.clone(),
        })
        .collect::<Vec<_>>();
    let content = build_feishu_im_aggregate_card(&card_events);
    let latest_index = latest_card_event(&card_events)
        .and_then(|latest| {
            card_events
                .iter()
                .position(|item| item.reply_route_key == latest.reply_route_key)
        })
        .unwrap_or_else(|| items.len().saturating_sub(1));
    let latest = items
        .get(latest_index)
        .unwrap_or_else(|| items.last().expect("non-empty items"));
    let mut runtime_patch = Map::new();
    runtime_patch.insert(
        "last_feishu_notify_card_kind".to_string(),
        Value::String("im_aggregate".to_string()),
    );
    runtime_patch.insert(
        "last_feishu_notify_aggregate_count".to_string(),
        Value::Number((items.len() as i64).into()),
    );

    match send_feishu_interactive_with_runtime_parts(
        state,
        latest.channel_id,
        &latest.config,
        content,
        &runtime_patch,
    )
    .await
    {
        Ok(outcome) => {
            if let Some(feishu_message_id) = outcome.message_id {
                persist_feishu_reply_route(
                    state,
                    &latest.owner_username,
                    &latest.uid,
                    latest.channel_id,
                    &latest.channel_type,
                    &latest.config,
                    &latest.event,
                    &feishu_message_id,
                )
                .await;
                for (item, card_event) in items.iter().zip(card_events.iter()) {
                    persist_feishu_reply_route(
                        state,
                        &item.owner_username,
                        &item.uid,
                        item.channel_id,
                        &item.channel_type,
                        &item.config,
                        &item.event,
                        &card_event.reply_route_key,
                    )
                    .await;
                }
            }
        }
        Err(err) => {
            eprintln!(
                "[notify.dispatch] channel={} type={} failed to send Feishu aggregate card: {err}",
                latest.channel_id, latest.channel_type
            );
        }
    }
}

fn feishu_aggregate_key(channel_id: i64, config: &Map<String, Value>) -> String {
    let receive_id_type = config_string(config, "receive_id_type");
    let receive_id = config_string(config, "receive_id");
    format!("{channel_id}\t{receive_id_type}\t{receive_id}")
}

fn latest_channel_id(items: &[PendingFeishuImNotification]) -> i64 {
    items.last().map(|item| item.channel_id).unwrap_or_default()
}

fn feishu_aggregate_batch_id(channel_id: i64) -> String {
    static NEXT_BATCH_ID: AtomicU64 = AtomicU64::new(1);
    let sequence = NEXT_BATCH_ID.fetch_add(1, Ordering::Relaxed);
    format!("agg-{channel_id}-{sequence}")
}

fn feishu_aggregate_reply_route_key(batch_id: &str, index: usize) -> String {
    format!("{batch_id}-{}", index + 1)
}

fn feishu_aggregate_window_ms(config: &Map<String, Value>) -> u64 {
    let value = config
        .get("im_aggregate_window_ms")
        .or_else(|| config.get("feishu_im_aggregate_window_ms"));
    let configured = match value {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse::<u64>().unwrap_or(0),
        None => 2_000,
        _ => 0,
    };
    configured.min(30_000)
}

async fn persist_feishu_reply_route(
    state: &AppState,
    owner_username: &str,
    uid: &str,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    event: &LiveMessageEvent,
    feishu_message_id: &str,
) {
    let chat_id = feishu_reply_route_chat_id(state, channel_id, config).await;
    if chat_id.trim().is_empty() {
        eprintln!(
            "[notify.dispatch] uid={uid} channel={channel_id} type={channel_type} skip Feishu reply route — missing chat_id"
        );
        return;
    }
    if let Err(err) = state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            owner_username,
            feishu_message_id,
            &chat_id,
            uid,
            &event.session_id,
            &event.peer_uid,
            &event.peer_nick,
            &event.item_id,
            &event.item_title,
            &event.message_id,
            &event.upstream_message_id,
        )
        .await
    {
        eprintln!(
            "[notify.dispatch] uid={uid} channel={channel_id} type={channel_type} failed to persist Feishu reply route: {err}"
        );
    }
}

async fn feishu_reply_route_chat_id(
    state: &AppState,
    channel_id: i64,
    config: &Map<String, Value>,
) -> String {
    let receive_id_type = config_string(config, "receive_id_type");
    let receive_id = config_string(config, "receive_id");
    if (receive_id_type.is_empty() || receive_id_type == "chat_id") && !receive_id.is_empty() {
        return receive_id;
    }
    state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .ok()
        .and_then(|runtime| {
            runtime
                .runtime_json
                .get("last_chat_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_default()
}

async fn render_incoming_message_for_channel(
    state: &AppState,
    uid: &str,
    channel_type: &str,
    config: &Map<String, Value>,
    event: &LiveMessageEvent,
) -> RenderedNotificationContent {
    let template = build_incoming_message_template(state, uid, config, event).await;
    render_template_for_channel(channel_type, &template)
}

async fn build_incoming_message_template(
    state: &AppState,
    uid: &str,
    config: &Map<String, Value>,
    event: &LiveMessageEvent,
) -> NotificationTemplate {
    let account_name = fetch_account_name(state, uid).await;
    let customer_name = if !event.peer_nick.trim().is_empty() {
        event.peer_nick.trim().to_string()
    } else if !event.peer_uid.trim().is_empty() {
        event.peer_uid.trim().to_string()
    } else {
        "买家".to_string()
    };
    let body = incoming_message_body(event);
    let item_display = incoming_message_item_display(event);
    let time_display = format_timestamp_ms(event.ts);
    let style_preset = config_string(config, "style_preset");

    let plain = render_incoming_message_plain_card(
        &style_preset,
        event,
        &account_name,
        &customer_name,
        &item_display,
        &time_display,
        &body,
    );
    let html = render_incoming_message_html_card(
        &style_preset,
        event,
        &account_name,
        &customer_name,
        &item_display,
        &time_display,
        &body,
    );
    let markdown = render_incoming_message_markdown(
        event,
        &account_name,
        &customer_name,
        &item_display,
        &time_display,
        &body,
    );

    NotificationTemplate::new(plain.clone())
        .with_text(plain)
        .with_html(html)
        .with_markdown(markdown)
}

fn render_template_for_channel(
    channel_type: &str,
    template: &NotificationTemplate,
) -> RenderedNotificationContent {
    match resolve_driver(channel_type) {
        Ok(resolution) => {
            let requested = match resolution.normalized_type.as_str() {
                "openclaw_weixin" => DriverContentRequest::markdown(),
                _ => DriverContentRequest::text(),
            };
            render_driver_content(&resolution, template, requested)
        }
        Err(_) => RenderedNotificationContent {
            requested_kind: NotificationContentKind::Text,
            selected_kind: NotificationContentKind::Text,
            body: template
                .text()
                .unwrap_or(template.fallback_text())
                .to_string(),
            fallback_text: template.fallback_text().to_string(),
        },
    }
}

fn incoming_message_body(event: &LiveMessageEvent) -> String {
    if !event.text.trim().is_empty() {
        event.text.trim().to_string()
    } else if !event.title.trim().is_empty() {
        event.title.trim().to_string()
    } else if !event.preview.trim().is_empty() {
        event.preview.trim().to_string()
    } else {
        format!("[{}]", event.kind)
    }
}

fn incoming_message_item_display(event: &LiveMessageEvent) -> String {
    if !event.item_title.trim().is_empty() {
        if event.item_id.trim().is_empty() {
            event.item_title.trim().to_string()
        } else {
            format!("{} ({})", event.item_title.trim(), event.item_id.trim())
        }
    } else if !event.item_id.trim().is_empty() {
        event.item_id.trim().to_string()
    } else {
        "未知商品".to_string()
    }
}

fn render_incoming_message_markdown(
    event: &LiveMessageEvent,
    account_name: &str,
    customer_name: &str,
    item_display: &str,
    time_display: &str,
    body: &str,
) -> String {
    let quoted_body = body
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    let quote_title = format!("客户消息 {}", event.session_id.trim());

    let mut out = format!("## 🔔 新客户消息 {quote_title}\n\n");
    out.push_str(&format!("- 🧵 对话ID：`{}`\n", event.session_id.trim()));
    out.push_str(&format!("- 🏠 账号名称：**{}**\n", account_name));
    out.push_str(&format!("- 👤 客户名称：**{customer_name}**\n"));
    out.push_str(&format!("- 📦 咨询商品：`{item_display}`\n"));
    out.push_str(&format!("- 🕒 时间：`{time_display}`\n\n"));
    out.push_str("### 💬 消息内容\n");
    out.push_str(&quoted_body);
    out.push_str("\n\n> 直接在微信里引用这条通知并回复，系统会自动转发给客户。");
    out
}

fn render_incoming_message_html_card(
    style_preset: &str,
    event: &LiveMessageEvent,
    account_name: &str,
    customer_name: &str,
    item_display: &str,
    time_display: &str,
    body: &str,
) -> String {
    let body_html = escape_html_content(body);
    let account_html = escape_html_content(account_name);
    let customer_html = escape_html_content(customer_name);
    let item_html = escape_html_content(item_display);
    let session_html = escape_html_content(event.session_id.trim());
    let time_html = escape_html_content(time_display);
    if style_preset == "minimal" {
        return format!(
            "🔔 <b>{customer_html}</b>\n商品: <code>{item_html}</code>\n内容: <blockquote>{body_html}</blockquote>"
        );
    }
    if style_preset == "compact" {
        return format!(
            "🔔 <b>新客户消息</b>\n账号: <b>{account_html}</b>\n客户: <b>{customer_html}</b>\n商品: <code>{item_html}</code>\n内容: <blockquote>{body_html}</blockquote>"
        );
    }
    format!(
        "🔔 <b>新客户消息</b>\n\n🏠 <b>账号名称</b>: <b>{account_html}</b>\n👤 <b>客户名称</b>: <b>{customer_html}</b>\n📦 <b>咨询商品</b>: <code>{item_html}</code>\n🧵 <b>对话ID</b>: <code>{session_html}</code>\n🕒 <b>时间</b>: <code>{time_html}</code>\n\n💬 <b>消息内容</b>\n<blockquote>{body_html}</blockquote>"
    )
}

fn render_incoming_message_plain_card(
    style_preset: &str,
    event: &LiveMessageEvent,
    account_name: &str,
    customer_name: &str,
    item_display: &str,
    time_display: &str,
    body: &str,
) -> String {
    if style_preset == "minimal" {
        return format!("🔔 {customer_name}\n商品: {item_display}\n内容: {body}");
    }
    if style_preset == "compact" {
        return format!(
            "🔔 新客户消息\n账号: {account_name}\n客户: {customer_name}\n商品: {item_display}\n内容: {body}"
        );
    }
    format!(
        "🔔 新客户消息\n\n🏠 账号名称: {account_name}\n👤 客户名称: {customer_name}\n📦 咨询商品: {item_display}\n🧵 对话ID: {}\n🕒 时间: {time_display}\n\n💬 消息内容\n{}",
        event.session_id.trim(),
        body
    )
}

fn escape_html_content(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

async fn fetch_account_name(state: &AppState, uid: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT COALESCE(nick, '') FROM accounts WHERE uid = ?")
        .bind(uid)
        .fetch_optional(state.storage().accounts())
        .await
        .ok()
        .flatten()
        .filter(|nick| !nick.trim().is_empty())
        .unwrap_or_else(|| uid.trim().to_string())
}

fn should_skip_degenerate_account_notification(uid: &str, event: &LiveMessageEvent) -> bool {
    let owner_uid = uid.trim();
    let peer_uid = event.peer_uid.trim();
    if owner_uid.is_empty() || peer_uid.is_empty() {
        return false;
    }
    owner_uid == peer_uid
}

/// Single-channel dispatch for `openclaw_weixin` — pushes through the direct
/// Tencent iLink HTTP client. On success / failure, patches the channel's
/// runtime row so the operator sees last attempt / error in the notifications
/// page.
///
/// Config schema (post-rewrite):
///   * `base_url`       — per-account host returned by QR-confirm
///   * `bot_token`      — long-lived bearer token
///   * `ilink_user_id`  — scanner's user id (default target if `to_user_id`
///                        is empty, i.e. send-DM-to-yourself)
///   * `to_user_id`     — optional override
///   * `bot_agent`      — optional, defaults to `xianyu-proxy/1.0`
///   * `context_token`  — optional, threads replies in the same session
async fn dispatch_openclaw_weixin(
    state: &AppState,
    uid: &str,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    rendered: &RenderedNotificationContent,
    event: &LiveMessageEvent,
) {
    let base_url = config_string(config, "base_url");
    let bot_token = config_string(config, "bot_token");
    let bot_agent_value = config_string(config, "bot_agent");
    let ilink_user_id = config_string(config, "ilink_user_id");
    let explicit_to = config_string(config, "to_user_id");
    let to_user_id = if explicit_to.is_empty() {
        ilink_user_id.clone()
    } else {
        explicit_to
    };
    let bot_agent = {
        if bot_agent_value.is_empty() {
            None
        } else {
            Some(bot_agent_value.clone())
        }
    };

    if base_url.is_empty() || bot_token.is_empty() || to_user_id.is_empty() {
        eprintln!(
            "[notify.openclaw] uid={uid} channel={channel_id} skip — missing base_url / bot_token / to_user_id"
        );
        patch_runtime_error(
            state,
            channel_id,
            "missing base_url / bot_token / to_user_id in channel config",
            &Map::new(),
        )
        .await;
        return;
    }

    // Look up context_token from the long-poll cache. Falls back to the
    // legacy `context_token` config field for back-compat with any
    // pre-Phase-2A channel rows. If both are empty the upstream will return
    // `ret=-2` — we surface a friendlier hint below.
    let context_token = openclaw_context_token_for_send(channel_id, config, &to_user_id);

    if let Some(item_pic_url) = best_effort_item_pic_url(event) {
        if let Ok(bytes) = download_remote_image(&item_pic_url).await {
            if let Ok(uploaded) =
                upload_image_via_cdn(&base_url, &bot_token, &bot_agent_value, &to_user_id, &bytes)
                    .await
            {
                let image_client =
                    IlinkBotClient::new(base_url.clone(), bot_token.clone(), bot_agent.clone());
                let _ = image_client
                    .send_image(&to_user_id, &context_token, &uploaded)
                    .await;
            }
        }
    }

    eprintln!(
        "[notify.openclaw] uid={uid} channel={channel_id} session={} to={} ctx_len={} kind={} item_id={} peer_uid={}",
        event.session_id,
        to_user_id,
        context_token.len(),
        event.kind,
        event.item_id,
        event.peer_uid
    );
    // Route all listener self-heal through the unified manager lifecycle
    // surface so stale context-token recovery shares the same runtime fields
    // as startup and route-triggered restarts.
    match send_text_with_runtime_parts_and_error_adapter(
        state,
        channel_id,
        channel_type,
        config,
        rendered.body.as_str(),
        Some(rendered),
        &Map::new(),
        |err| {
            if err.contains("ret=-2") {
                format!(
                    "{err} 提示：WeChat 用户尚未给 bot 发过任何消息——他需要先在微信里发条「你好」，bot 才能拿到 context_token 来回复"
                )
            } else {
                err.to_string()
            }
        },
    )
    .await
    {
        Ok(_) => {
            eprintln!(
                "[notify.openclaw] uid={uid} channel={channel_id} session={} send_text ok",
                event.session_id
            );
        }
        Err(err) => {
            eprintln!(
                "[notify.openclaw] uid={uid} channel={channel_id} send_text failed: {err}"
            );
            if err.contains("ret=-2") {
                if let Err(clear_err) =
                    openclaw_context_cache::clear(state, channel_id, &to_user_id).await
                {
                    eprintln!(
                        "[notify.openclaw] channel={channel_id} clear stale context_token failed: {clear_err}"
                    );
                }
                tokio::spawn(crate::notifications::manager::restart_worker_for_channel(
                    state.clone(),
                    channel_id,
                ));
            }
        }
    }
}

fn best_effort_item_pic_url(event: &LiveMessageEvent) -> Option<String> {
    let value = event.item_pic.trim();
    if value.starts_with("http://") || value.starts_with("https://") {
        Some(value.to_string())
    } else {
        None
    }
}

async fn download_remote_image(url: &str) -> Result<Vec<u8>, String> {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let out = std::process::Command::new("curl")
            .arg("-sS")
            .arg("--max-time")
            .arg("15")
            .arg("--noproxy")
            .arg("*")
            .arg("-L")
            .arg(&url)
            .output()
            .map_err(|err| format!("spawn curl: {err}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("curl exit {} stderr={}", out.status, stderr));
        }
        Ok(out.stdout)
    })
    .await
    .map_err(|err| format!("spawn_blocking join: {err}"))?
}

fn format_timestamp_ms(ts: i64) -> String {
    if ts <= 0 {
        return "未知时间".to_string();
    }
    let secs = ts / 1000;
    let output = std::process::Command::new("date")
        .arg("-r")
        .arg(secs.to_string())
        .arg("+%Y-%m-%d %H:%M:%S")
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => ts.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_state() -> (TempDir, crate::state::AppState) {
        let temp_dir = TempDir::new().expect("temp dir");
        let mut config = crate::config::AppConfig::default();
        config.sqlite.data_dir = temp_dir.path().to_path_buf();
        config.sqlite.accounts_db_path = temp_dir.path().join("accounts.db");
        config.sqlite.live_state_db_path = temp_dir.path().join("live_state.db");
        config.sqlite.notifications_db_path = temp_dir.path().join("notifications.db");
        config.sqlite.xianyu_db_path = temp_dir.path().join("xianyu.db");
        let state = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(crate::state::AppState::bootstrap(config))
            .expect("state");
        (temp_dir, state)
    }

    #[test]
    fn incoming_message_notification_contains_required_fields() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (_temp_dir, state) = test_state();
        rt.block_on(async {
            sqlx::query("INSERT OR REPLACE INTO accounts (uid, nick) VALUES (?, ?)")
                .bind("seller-uid")
                .bind("测试卖家")
                .execute(state.storage().accounts())
                .await
                .expect("insert account");
        });
        let event = LiveMessageEvent {
            session_id: "60334614175".to_string(),
            peer_uid: "99887766".to_string(),
            peer_nick: "测试客户".to_string(),
            item_id: "123".to_string(),
            item_title: "测试商品".to_string(),
            text: "在吗".to_string(),
            ts: 1_778_896_000_000,
            ..Default::default()
        };
        let rendered = rt.block_on(async {
            render_incoming_message_for_channel(
                &state,
                "seller-uid",
                "openclaw_weixin",
                &Map::new(),
                &event,
            )
            .await
            .body
        });
        assert!(rendered.contains("🏠"));
        assert!(rendered.contains("👤"));
        assert!(rendered.contains("📦"));
        assert!(rendered.contains("🧵"));
        assert!(rendered.contains("💬"));
        assert!(rendered.contains("对话ID"));
        assert!(rendered.contains("账号名称"));
        assert!(rendered.contains("测试卖家"));
        assert!(rendered.contains("客户名称"));
        assert!(rendered.contains("咨询商品"));
        assert!(rendered.contains("消息内容"));
        assert!(rendered.contains("时间"));
        assert!(!rendered.contains("账号UID"));
        assert!(!rendered.contains("客户UID"));
    }

    #[test]
    fn incoming_message_template_keeps_html_variant_without_removed_channel_copy() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (_temp_dir, state) = test_state();
        rt.block_on(async {
            sqlx::query("INSERT OR REPLACE INTO accounts (uid, nick) VALUES (?, ?)")
                .bind("seller-uid-html")
                .bind("HTML卖家")
                .execute(state.storage().accounts())
                .await
                .expect("insert account");
        });
        let event = LiveMessageEvent {
            session_id: "61475741054".to_string(),
            peer_nick: "时空赛马".to_string(),
            item_id: "1052090088362".to_string(),
            item_title: "西数 1TB 硬盘".to_string(),
            text: "认盘吗".to_string(),
            ts: 1_778_896_000_000,
            ..Default::default()
        };
        let mut cfg = Map::new();
        cfg.insert("parse_mode".into(), Value::String("HTML".to_string()));
        cfg.insert(
            "style_preset".into(),
            Value::String("rich_card".to_string()),
        );
        let rendered = rt
            .block_on(build_incoming_message_template(
                &state,
                "seller-uid-html",
                &cfg,
                &event,
            ))
            .html()
            .expect("html template")
            .to_string();
        assert!(rendered.contains("<b>新客户消息</b>"));
        assert!(rendered.contains("🏠 <b>账号名称</b>"));
        assert!(rendered.contains("👤 <b>客户名称</b>"));
        assert!(rendered.contains("📦 <b>咨询商品</b>"));
        assert!(rendered.contains("🧵 <b>对话ID</b>"));
        assert!(rendered.contains("💬 <b>消息内容</b>"));
        assert!(rendered.contains("HTML卖家"));
        assert!(rendered.contains("时空赛马"));
        assert!(rendered.contains("<blockquote>认盘吗</blockquote>"));
        assert!(!rendered.contains("旧渠道"));
        assert!(!rendered.contains("直接在微信里引用"));
    }

    #[test]
    fn openclaw_notification_uses_markdown_variant_without_removed_channel_escape_rules() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let (_temp_dir, state) = test_state();
        rt.block_on(async {
            sqlx::query("INSERT OR REPLACE INTO accounts (uid, nick) VALUES (?, ?)")
                .bind("seller-uid-md")
                .bind("Markdown卖家")
                .execute(state.storage().accounts())
                .await
                .expect("insert account");
        });
        let event = LiveMessageEvent {
            session_id: "10001".to_string(),
            peer_nick: "买家_1".to_string(),
            item_title: "特殊字符 [硬盘]".to_string(),
            text: "25要了!".to_string(),
            ts: 1_778_896_000_000,
            ..Default::default()
        };
        let mut cfg = Map::new();
        cfg.insert("style_preset".into(), Value::String("compact".to_string()));
        let rendered = rt.block_on(async {
            render_incoming_message_for_channel(
                &state,
                "seller-uid-md",
                "openclaw_weixin",
                &cfg,
                &event,
            )
            .await
            .body
        });
        assert!(rendered.contains("## 🔔 新客户消息"));
        assert!(rendered.contains("- 🏠 账号名称：**Markdown卖家**"));
        assert!(rendered.contains("- 👤 客户名称：**买家_1**"));
        assert!(rendered.contains("- 📦 咨询商品：`特殊字符 [硬盘]`"));
        assert!(rendered.contains("> 25要了!"));
        assert!(rendered.contains("直接在微信里引用"));
        assert!(!rendered.contains("旧渠道"));
    }

    #[test]
    fn same_account_peer_notifications_are_suppressed() {
        let same_peer_event = LiveMessageEvent {
            session_id: "same-peer-session".to_string(),
            peer_uid: "seller-a".to_string(),
            peer_nick: "卖家A".to_string(),
            text: "异常消息".to_string(),
            ..Default::default()
        };
        let mirrored_event = LiveMessageEvent {
            session_id: "mirrored-session".to_string(),
            peer_uid: "seller-b".to_string(),
            peer_nick: "卖家B".to_string(),
            text: "镜像消息".to_string(),
            ..Default::default()
        };

        assert!(should_skip_degenerate_account_notification(
            "seller-a",
            &same_peer_event
        ));
        assert!(!should_skip_degenerate_account_notification(
            "seller-a",
            &mirrored_event
        ));
    }

    #[test]
    fn legacy_feishu_channels_default_to_interactive_aggregate_window() {
        assert_eq!(feishu_aggregate_window_ms(&Map::new()), 2_000);
    }
}
