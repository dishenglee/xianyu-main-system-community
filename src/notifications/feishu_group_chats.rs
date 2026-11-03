use serde_json::Value;

use crate::{
    models::notifications::{FeishuSessionGroupChat, NotificationChannel},
    routes::accounts::messages::LiveMessageEvent,
    state::AppState,
};

use super::{
    driver::config_string,
    feishu_group_cards::{
        build_feishu_group_main_card, build_feishu_group_status_card, FeishuGroupCardContext,
    },
    feishu_native::{
        ChatCreateRequest, ChatPatchRequest, FeishuNativeError, MessageSendRequest,
        MessageSendResponse,
    },
};

pub fn build_group_name(customer_name: &str, item_title: &str, status: &str) -> String {
    let customer = truncate_single_line(customer_name, 18);
    let item = truncate_single_line(item_title, 28);
    let base = format!("{customer}｜{item}");
    match status {
        "won" => format!("{base}【已成交】"),
        "closed" => format!("{base}【已结束】"),
        _ => base,
    }
}

pub fn item_title_or_fallback(item_title: &str, item_id: &str) -> String {
    if !item_title.trim().is_empty() {
        item_title.trim().to_string()
    } else if !item_id.trim().is_empty() {
        item_id.trim().to_string()
    } else {
        "未知商品".to_string()
    }
}

pub async fn create_group_for_session(
    state: &AppState,
    channel: &NotificationChannel,
    member_ids: &[String],
    event: &LiveMessageEvent,
) -> Result<(String, String), String> {
    let requested_name = build_group_name(&customer_name(event), &item_title(event), "active");
    let member_ids = member_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if member_ids.is_empty() {
        return Err("missing Feishu group member ids".to_string());
    }
    let response = state
        .feishu_native()
        .chat_create(ChatCreateRequest {
            channel_id: channel.id,
            name: requested_name.clone(),
            member_id_type: "open_id".to_string(),
            member_ids,
            avatar_url: non_empty(&event.peer_logo),
            app_id: non_empty(&config_string(&channel.config, "app_id")),
            app_secret: non_empty(&config_string(&channel.config, "app_secret")),
            domain: non_empty(&config_string(&channel.config, "domain")),
        })
        .await
        .map_err(native_error_to_string)?;

    Ok((response.chat_id, response.name))
}

pub async fn send_group_main_card(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    account_name: &str,
) -> Result<MessageSendResponse, String> {
    send_group_card(
        state,
        channel,
        &mapping.feishu_chat_id,
        build_feishu_group_main_card(group_context(mapping, account_name), &[]),
    )
    .await
}

pub async fn send_group_incremental_card(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    _account_name: &str,
    event: &LiveMessageEvent,
) -> Result<MessageSendResponse, String> {
    if let Err(error) = patch_group_avatar_if_needed(state, channel, mapping, event).await {
        eprintln!(
            "[feishu.group] chat_id={} avatar patch skipped: {}",
            mapping.feishu_chat_id, error
        );
    }
    send_group_text_message(
        state,
        channel,
        &mapping.feishu_chat_id,
        &incremental_message_text(event),
    )
    .await
}

pub async fn patch_group_status(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    status: &str,
    status_label: &str,
    account_name: &str,
) -> Result<(), String> {
    let group_name = build_group_name(&mapping.customer_name, &mapping.item_title, status);
    if !mapping.feishu_chat_id.trim().is_empty() {
        state
            .feishu_native()
            .chat_patch(ChatPatchRequest {
                channel_id: channel.id,
                chat_id: mapping.feishu_chat_id.clone(),
                name: group_name.clone(),
                avatar_url: None,
                app_id: non_empty(&config_string(&channel.config, "app_id")),
                app_secret: non_empty(&config_string(&channel.config, "app_secret")),
                domain: non_empty(&config_string(&channel.config, "domain")),
            })
            .await
            .map_err(native_error_to_string)?;
        let _ = send_group_card(
            state,
            channel,
            &mapping.feishu_chat_id,
            build_feishu_group_status_card(
                &group_context(
                    &FeishuSessionGroupChat {
                        feishu_chat_name: group_name.clone(),
                        status: status.to_string(),
                        ..mapping.clone()
                    },
                    account_name,
                ),
                status_label,
            ),
        )
        .await;
    }
    state
        .storage()
        .update_feishu_session_group_status(
            channel.id,
            &mapping.session_id,
            status,
            &group_name,
            Some(event_now()),
        )
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

pub async fn patch_group_identity(
    state: &AppState,
    channel: &NotificationChannel,
    chat_id: &str,
    group_name: &str,
    avatar_url: Option<&str>,
) -> Result<(), String> {
    if chat_id.trim().is_empty() {
        return Ok(());
    }
    state
        .feishu_native()
        .chat_patch(ChatPatchRequest {
            channel_id: channel.id,
            chat_id: chat_id.trim().to_string(),
            name: group_name.trim().to_string(),
            avatar_url: avatar_url.and_then(non_empty),
            app_id: non_empty(&config_string(&channel.config, "app_id")),
            app_secret: non_empty(&config_string(&channel.config, "app_secret")),
            domain: non_empty(&config_string(&channel.config, "domain")),
        })
        .await
        .map_err(native_error_to_string)?;
    Ok(())
}

fn group_context(mapping: &FeishuSessionGroupChat, account_name: &str) -> FeishuGroupCardContext {
    FeishuGroupCardContext {
        uid: mapping.uid.clone(),
        account_name: account_name.to_string(),
        customer_name: mapping.customer_name.clone(),
        session_id: mapping.session_id.clone(),
        item_id: mapping.item_id.clone(),
        item_title: mapping.item_title.clone(),
        item_pic: mapping.item_pic.clone(),
        item_price: mapping.item_price.clone(),
        feishu_chat_name: mapping.feishu_chat_name.clone(),
        status: mapping.status.clone(),
    }
}

async fn send_group_card(
    state: &AppState,
    channel: &NotificationChannel,
    chat_id: &str,
    card: Value,
) -> Result<MessageSendResponse, String> {
    state
        .feishu_native()
        .message_send(MessageSendRequest {
            channel_id: channel.id,
            receive_id: chat_id.trim().to_string(),
            receive_id_type: "chat_id".to_string(),
            msg_type: "interactive".to_string(),
            content: card,
            app_id: non_empty(&config_string(&channel.config, "app_id")),
            app_secret: non_empty(&config_string(&channel.config, "app_secret")),
            domain: non_empty(&config_string(&channel.config, "domain")),
            reply_to_message_id: None,
            reply_in_thread: None,
            fallback_to_plain_chat: Some(true),
            source_event_id: None,
        })
        .await
        .map_err(native_error_to_string)
}

async fn send_group_text_message(
    state: &AppState,
    channel: &NotificationChannel,
    chat_id: &str,
    text: &str,
) -> Result<MessageSendResponse, String> {
    state
        .feishu_native()
        .message_send(MessageSendRequest {
            channel_id: channel.id,
            receive_id: chat_id.trim().to_string(),
            receive_id_type: "chat_id".to_string(),
            msg_type: "text".to_string(),
            content: serde_json::json!({ "text": text }),
            app_id: non_empty(&config_string(&channel.config, "app_id")),
            app_secret: non_empty(&config_string(&channel.config, "app_secret")),
            domain: non_empty(&config_string(&channel.config, "domain")),
            reply_to_message_id: None,
            reply_in_thread: None,
            fallback_to_plain_chat: Some(true),
            source_event_id: None,
        })
        .await
        .map_err(native_error_to_string)
}

async fn patch_group_avatar_if_needed(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    event: &LiveMessageEvent,
) -> Result<(), String> {
    let Some(avatar_url) = non_empty(&event.peer_logo) else {
        return Ok(());
    };
    let group_name = build_group_name(
        &mapping.customer_name,
        &item_title_or_fallback(&mapping.item_title, &mapping.item_id),
        &mapping.status,
    );
    patch_group_identity(
        state,
        channel,
        &mapping.feishu_chat_id,
        &group_name,
        Some(&avatar_url),
    )
    .await
}

fn customer_name(event: &LiveMessageEvent) -> String {
    if !event.peer_nick.trim().is_empty() {
        event.peer_nick.trim().to_string()
    } else if !event.peer_uid.trim().is_empty() {
        event.peer_uid.trim().to_string()
    } else {
        "客户".to_string()
    }
}

fn item_title(event: &LiveMessageEvent) -> String {
    item_title_or_fallback(&event.item_title, &event.item_id)
}

fn event_message_body(event: &LiveMessageEvent) -> String {
    if !event.text.trim().is_empty() {
        event.text.trim().to_string()
    } else if !event.preview.trim().is_empty() {
        event.preview.trim().to_string()
    } else if !event.title.trim().is_empty() {
        event.title.trim().to_string()
    } else {
        format!("[{}]", event.kind)
    }
}

fn incremental_message_text(event: &LiveMessageEvent) -> String {
    format!(
        "{}：{}",
        if event.is_self { "我" } else { "客户" },
        event_message_body(event)
    )
}

fn truncate_single_line(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let fallback = if normalized.is_empty() {
        "未知".to_string()
    } else {
        normalized
    };
    if fallback.chars().count() <= max_chars {
        fallback
    } else {
        format!(
            "{}...",
            fallback
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        )
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn native_error_to_string(error: FeishuNativeError) -> String {
    error.to_string()
}

fn event_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
