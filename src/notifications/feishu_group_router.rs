use sqlx::query_scalar;

use crate::{
    models::notifications::{FeishuOperatorActivity, FeishuSessionGroupChat, NotificationChannel},
    routes::accounts::messages::LiveMessageEvent,
    state::AppState,
};

use super::feishu_group_chats::{
    build_group_name, create_group_for_session, item_title_or_fallback, patch_group_identity,
    patch_group_status, send_group_incremental_card, send_group_main_card,
};
use super::{
    driver::config_string,
    feishu_native::{ChatMembersAddRequest, FeishuNativeError},
};

#[derive(Debug, Clone)]
pub enum FindOrCreateGroupOutcome {
    Existing { mapping: FeishuSessionGroupChat },
    Created { mapping: FeishuSessionGroupChat },
    Fallback { reason: String },
}

pub async fn find_or_create_group_for_session(
    state: &AppState,
    channel: &NotificationChannel,
    owner_username: &str,
    uid: &str,
    event: &LiveMessageEvent,
) -> Result<FindOrCreateGroupOutcome, String> {
    if let Some(mapping) = state
        .storage()
        .get_feishu_session_group_chat_by_session(
            channel.id,
            owner_username,
            uid,
            &event.session_id,
        )
        .await
        .map_err(|err| err.to_string())?
    {
        if !mapping.feishu_chat_id.trim().is_empty() && mapping.status != "failed" {
            return Ok(FindOrCreateGroupOutcome::Existing { mapping });
        }
    }

    let Some(operator) = resolve_group_operator(state, channel.id, owner_username).await? else {
        return Ok(FindOrCreateGroupOutcome::Fallback {
            reason: "no recent Feishu operator activity or bound sender".to_string(),
        });
    };

    let member_ids =
        resolve_group_member_ids(state, channel.id, owner_username, &operator.sender_id).await?;

    match create_group_for_session(state, channel, &member_ids, event).await {
        Ok((chat_id, chat_name)) => {
            let mapping = state
                .storage()
                .upsert_feishu_session_group_chat(
                    channel.id,
                    owner_username,
                    uid,
                    &event.session_id,
                    &event.peer_uid,
                    &event.peer_nick,
                    &event.item_id,
                    &event.item_title,
                    &event.item_pic,
                    "",
                    &chat_id,
                    &chat_name,
                    &operator.sender_id,
                    "active",
                    event.ts as f64,
                )
                .await
                .map_err(|err| err.to_string())?;
            let account_name = load_account_name_for_uid(state, uid).await?;
            let _ = send_group_main_card(state, channel, &mapping, &account_name).await;
            Ok(FindOrCreateGroupOutcome::Created { mapping })
        }
        Err(err) => {
            state
                .storage()
                .upsert_feishu_session_group_chat(
                    channel.id,
                    owner_username,
                    uid,
                    &event.session_id,
                    &event.peer_uid,
                    &event.peer_nick,
                    &event.item_id,
                    &event.item_title,
                    &event.item_pic,
                    "",
                    "",
                    "",
                    "",
                    "failed",
                    event.ts as f64,
                )
                .await
                .map_err(|storage_err| storage_err.to_string())?;
            Ok(FindOrCreateGroupOutcome::Fallback { reason: err })
        }
    }
}

pub async fn append_group_message(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    event: &LiveMessageEvent,
) -> Result<String, String> {
    let account_name = load_account_name_for_uid(state, &mapping.uid).await?;
    let mapping =
        repair_existing_group_metadata(state, channel, mapping, &account_name, event, false)
            .await?
            .unwrap_or_else(|| mapping.clone());
    let response =
        send_group_incremental_card(state, channel, &mapping, &account_name, event).await?;
    state
        .storage()
        .upsert_feishu_session_group_chat(
            channel.id,
            &mapping.owner_username,
            &mapping.uid,
            &mapping.session_id,
            &prefer_non_empty(&mapping.peer_uid, &event.peer_uid),
            &prefer_non_empty(&mapping.customer_name, &event.peer_nick),
            &prefer_non_empty(&mapping.item_id, &event.item_id),
            &prefer_non_empty(&mapping.item_title, &event.item_title),
            &prefer_non_empty(&mapping.item_pic, &event.item_pic),
            &mapping.item_price,
            &mapping.feishu_chat_id,
            &mapping.feishu_chat_name,
            &mapping.feishu_operator_sender_id,
            &mapping.status,
            event.ts as f64,
        )
        .await
        .map_err(|err| err.to_string())?;
    Ok(response.message_id)
}

pub async fn repair_group_metadata_for_event(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
) -> Result<usize, String> {
    let mappings = state
        .storage()
        .list_active_feishu_session_group_chats_for_uid_session(uid, &event.session_id)
        .await
        .map_err(|err| err.to_string())?;
    if mappings.is_empty() {
        return Ok(0);
    }

    let account_name = load_account_name_for_uid(state, uid).await?;
    let mut repaired = 0usize;
    for mapping in mappings {
        let Some(channel) = state
            .storage()
            .get_notification_channel(mapping.channel_id)
            .await
            .map_err(|err| err.to_string())?
        else {
            continue;
        };
        if !matches!(channel.channel_type.as_str(), "feishu" | "lark") {
            continue;
        }
        if repair_existing_group_metadata(state, &channel, &mapping, &account_name, event, true)
            .await?
            .is_some()
        {
            repaired += 1;
        }
    }
    Ok(repaired)
}

pub async fn mark_group_closed(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
) -> Result<(), String> {
    let account_name = load_account_name_for_uid(state, &mapping.uid).await?;
    patch_group_status(
        state,
        channel,
        mapping,
        "closed",
        "会话已结束",
        &account_name,
    )
    .await
}

pub async fn mark_group_won(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
) -> Result<(), String> {
    let account_name = load_account_name_for_uid(state, &mapping.uid).await?;
    patch_group_status(state, channel, mapping, "won", "会话已成交", &account_name).await
}

pub async fn sync_member_into_active_groups(
    state: &AppState,
    channel: &NotificationChannel,
    owner_username: &str,
    sender_id: &str,
) -> Result<usize, String> {
    let sender_id = sender_id.trim();
    let owner_username = owner_username.trim();
    if sender_id.is_empty() || owner_username.is_empty() {
        return Ok(0);
    }

    let groups = state
        .storage()
        .list_active_feishu_session_group_chats_for_owner(channel.id, owner_username)
        .await
        .map_err(|err| err.to_string())?;
    if groups.is_empty() {
        return Ok(0);
    }

    for group in &groups {
        state
            .feishu_native()
            .chat_members_add(ChatMembersAddRequest {
                channel_id: channel.id,
                chat_id: group.feishu_chat_id.clone(),
                member_id_type: "open_id".to_string(),
                member_ids: vec![sender_id.to_string()],
                app_id: non_empty(&config_string(&channel.config, "app_id")),
                app_secret: non_empty(&config_string(&channel.config, "app_secret")),
                domain: non_empty(&config_string(&channel.config, "domain")),
            })
            .await
            .map_err(native_error_to_string)?;
    }

    Ok(groups.len())
}

pub fn build_group_name_for_status(customer_name: &str, item_title: &str, status: &str) -> String {
    build_group_name(customer_name, item_title, status)
}

async fn repair_existing_group_metadata(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    account_name: &str,
    event: &LiveMessageEvent,
    include_avatar_patch: bool,
) -> Result<Option<FeishuSessionGroupChat>, String> {
    let merged_peer_uid = prefer_non_empty(&mapping.peer_uid, &event.peer_uid);
    let merged_customer_name = prefer_non_empty(&mapping.customer_name, &event.peer_nick);
    let merged_item_id = prefer_non_empty(&mapping.item_id, &event.item_id);
    let merged_item_title = prefer_non_empty(&mapping.item_title, &event.item_title);
    let merged_item_pic = prefer_non_empty(&mapping.item_pic, &event.item_pic);
    let merged_chat_name = build_group_name(
        &merged_customer_name,
        &item_title_or_fallback(&merged_item_title, &merged_item_id),
        &mapping.status,
    );
    let metadata_changed = merged_peer_uid != mapping.peer_uid
        || merged_customer_name != mapping.customer_name
        || merged_item_id != mapping.item_id
        || merged_item_title != mapping.item_title
        || merged_item_pic != mapping.item_pic
        || merged_chat_name != mapping.feishu_chat_name;
    let avatar_url = non_empty(&event.peer_logo);
    if !metadata_changed && avatar_url.is_none() {
        return Ok(Some(mapping.clone()));
    }

    let refreshed = state
        .storage()
        .upsert_feishu_session_group_chat(
            channel.id,
            &mapping.owner_username,
            &mapping.uid,
            &mapping.session_id,
            &merged_peer_uid,
            &merged_customer_name,
            &merged_item_id,
            &merged_item_title,
            &merged_item_pic,
            &mapping.item_price,
            &mapping.feishu_chat_id,
            &merged_chat_name,
            &mapping.feishu_operator_sender_id,
            &mapping.status,
            mapping.last_message_ts.max(event.ts as f64),
        )
        .await
        .map_err(|err| err.to_string())?;

    if merged_chat_name != mapping.feishu_chat_name
        || (include_avatar_patch && avatar_url.is_some())
    {
        patch_group_identity(
            state,
            channel,
            &refreshed.feishu_chat_id,
            &merged_chat_name,
            if include_avatar_patch {
                avatar_url.as_deref()
            } else {
                None
            },
        )
        .await?;
    }

    if metadata_changed {
        let _ = send_group_main_card(state, channel, &refreshed, account_name).await;
    }

    Ok(Some(refreshed))
}

async fn load_account_name_for_uid(state: &AppState, uid: &str) -> Result<String, String> {
    let nick = query_scalar::<_, String>("SELECT nick FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid.trim())
        .fetch_optional(state.storage().accounts())
        .await
        .map_err(|err| err.to_string())?;
    Ok(nick.unwrap_or_else(|| uid.trim().to_string()))
}

async fn resolve_group_operator(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
) -> Result<Option<FeishuOperatorActivity>, String> {
    if let Some(operator) = state
        .storage()
        .get_latest_feishu_operator_activity(channel_id, owner_username)
        .await
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(operator));
    }

    let Some(binding) = state
        .storage()
        .get_latest_feishu_user_binding_for_owner(channel_id, owner_username)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };

    Ok(Some(FeishuOperatorActivity {
        id: 0,
        channel_id,
        owner_username: owner_username.trim().to_string(),
        sender_id: binding.sender_id,
        sender_name: binding.sender_name,
        last_chat_id: binding.chat_id,
        last_event_id: binding.last_event_id,
        last_active_at: binding.updated_at,
        created_at: binding.created_at,
        updated_at: binding.updated_at,
    }))
}

async fn resolve_group_member_ids(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    operator_sender_id: &str,
) -> Result<Vec<String>, String> {
    let mut member_ids = Vec::new();
    let owner_username = owner_username.trim();

    if !owner_username.is_empty() {
        let bindings = state
            .storage()
            .list_feishu_user_bindings_for_owner(channel_id, owner_username)
            .await
            .map_err(|err| err.to_string())?;
        for binding in bindings {
            if !binding.enabled {
                continue;
            }
            let sender_id = binding.sender_id.trim();
            if sender_id.is_empty() || member_ids.iter().any(|existing| existing == sender_id) {
                continue;
            }
            member_ids.push(sender_id.to_string());
        }
    }

    let operator_sender_id = operator_sender_id.trim();
    if !operator_sender_id.is_empty()
        && !member_ids
            .iter()
            .any(|existing| existing == operator_sender_id)
    {
        member_ids.push(operator_sender_id.to_string());
    }

    if member_ids.is_empty() {
        return Err("missing Feishu group member ids".to_string());
    }

    Ok(member_ids)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn prefer_non_empty(current: &str, incoming: &str) -> String {
    if !incoming.trim().is_empty() {
        incoming.trim().to_string()
    } else {
        current.trim().to_string()
    }
}

fn native_error_to_string(error: FeishuNativeError) -> String {
    error.to_string()
}
