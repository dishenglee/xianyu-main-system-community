use serde_json::{Map, Value};

use crate::{
    models::notifications::{FeishuCollaboratorInvite, FeishuUserBinding, NotificationChannel},
    state::AppState,
};

pub(crate) const FEISHU_ROLE_PRIMARY: &str = "primary";
pub(crate) const FEISHU_ROLE_COLLABORATOR: &str = "collaborator";
pub(crate) const FEISHU_JOINED_VIA_QR_OWNER: &str = "qr_owner";
pub(crate) const FEISHU_JOINED_VIA_ALLOWLIST_AUTO_BIND: &str = "allowlist_auto_bind";
pub(crate) const FEISHU_JOINED_VIA_MANUAL_BIND: &str = "manual_bind";
pub(crate) const FEISHU_JOINED_VIA_INVITE_CODE: &str = "invite_code";

pub(crate) enum JoinCollaborationOutcome {
    AlreadyJoined(FeishuUserBinding),
    Joined(FeishuUserBinding),
}

pub(crate) fn member_is_primary(binding: &FeishuUserBinding) -> bool {
    binding.role.trim() == FEISHU_ROLE_PRIMARY
}

pub(crate) fn member_can_manage_channel(binding: &FeishuUserBinding) -> bool {
    member_is_primary(binding)
}

pub(crate) fn feishu_sender_allowed_to_bind(config: &Map<String, Value>, sender_id: &str) -> bool {
    let sender_id = sender_id.trim();
    if sender_id.is_empty() {
        return false;
    }
    let mut allowed = Vec::new();
    for key in [
        "allowed_operator_ids",
        "allowed_sender_ids",
        "authorized_sender_ids",
        "operator_user_ids",
        "operator_ids",
        "operator_open_id",
    ] {
        if let Some(value) = config.get(key) {
            push_config_string_values(value, &mut allowed);
        }
    }
    !allowed.is_empty() && allowed.iter().any(|candidate| candidate == sender_id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn resolve_member_for_sender(
    state: &AppState,
    channel: &NotificationChannel,
    sender_id: &str,
    sender_name: &str,
    chat_id: &str,
    chat_type: &str,
    message_id: &str,
    event_id: &str,
) -> Result<Option<FeishuUserBinding>, String> {
    let sender_id = sender_id.trim();
    if sender_id.is_empty() {
        return Ok(None);
    }

    let existing = state
        .storage()
        .get_feishu_user_binding(channel.id, sender_id)
        .await
        .map_err(|err| err.to_string())?;
    if let Some(binding) = existing {
        let channel_owner = channel.owner_username.trim();
        let binding_owner = binding.owner_username.trim();
        if binding.enabled && !binding_owner.is_empty() && binding_owner == channel_owner {
            let touched = state
                .storage()
                .touch_feishu_user_binding_seen(
                    channel.id,
                    sender_id,
                    sender_name,
                    chat_id,
                    chat_type,
                    message_id,
                    event_id,
                )
                .await
                .map_err(|err| err.to_string())?
                .unwrap_or(binding);
            return Ok(Some(touched));
        }
    }

    provision_allowed_member_binding(
        state,
        channel,
        sender_id,
        sender_name,
        chat_id,
        chat_type,
        message_id,
        event_id,
        FEISHU_JOINED_VIA_ALLOWLIST_AUTO_BIND,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn provision_allowed_member_binding(
    state: &AppState,
    channel: &NotificationChannel,
    sender_id: &str,
    sender_name: &str,
    chat_id: &str,
    chat_type: &str,
    message_id: &str,
    event_id: &str,
    joined_via: &str,
) -> Result<Option<FeishuUserBinding>, String> {
    let owner = channel.owner_username.trim();
    let sender_id = sender_id.trim();
    if owner.is_empty() || sender_id.is_empty() {
        return Ok(None);
    }
    if !feishu_sender_allowed_to_bind(&channel.config, sender_id) {
        return Ok(None);
    }

    let role = match state
        .storage()
        .get_primary_feishu_user_binding_for_owner(channel.id, owner)
        .await
        .map_err(|err| err.to_string())?
    {
        Some(primary) if primary.sender_id.trim() != sender_id => FEISHU_ROLE_COLLABORATOR,
        _ => FEISHU_ROLE_PRIMARY,
    };

    let binding = state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel.id,
            owner,
            sender_id,
            sender_name,
            chat_id,
            chat_type,
            message_id,
            event_id,
            role,
            joined_via,
            None,
            None,
            true,
        )
        .await
        .map_err(|err| err.to_string())?;

    Ok(Some(binding))
}

pub(crate) async fn bind_qr_owner_primary(
    state: &AppState,
    channel: &NotificationChannel,
    sender_id: &str,
) -> Result<Option<FeishuUserBinding>, String> {
    let owner = channel.owner_username.trim();
    let sender_id = sender_id.trim();
    if owner.is_empty() || sender_id.is_empty() {
        return Ok(None);
    }

    let binding = state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel.id,
            owner,
            sender_id,
            "",
            "",
            "p2p",
            "",
            "",
            FEISHU_ROLE_PRIMARY,
            FEISHU_JOINED_VIA_QR_OWNER,
            None,
            None,
            true,
        )
        .await
        .map_err(|err| err.to_string())?;

    Ok(Some(binding))
}

pub(crate) async fn generate_collaborator_invite(
    state: &AppState,
    channel: &NotificationChannel,
    sender_id: &str,
    sender_name: &str,
) -> Result<FeishuCollaboratorInvite, String> {
    let owner = channel.owner_username.trim();
    for _ in 0..5 {
        let code = format!("{:06X}", rand::random::<u32>() & 0xFF_FFFF);
        match state
            .storage()
            .create_feishu_collaborator_invite(
                channel.id,
                owner,
                &code,
                sender_id,
                sender_name,
                1800.0,
            )
            .await
        {
            Ok(invite) => return Ok(invite),
            Err(err) => {
                let text = err.to_string();
                if text.contains("UNIQUE") {
                    continue;
                }
                return Err(text);
            }
        }
    }

    Err("生成邀请码失败，请稍后重试".to_string())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn join_collaboration_via_invite(
    state: &AppState,
    channel: &NotificationChannel,
    sender_id: &str,
    sender_name: &str,
    chat_id: &str,
    chat_type: &str,
    message_id: &str,
    event_id: &str,
    code: &str,
) -> Result<JoinCollaborationOutcome, String> {
    let sender_id = sender_id.trim();
    let owner = channel.owner_username.trim();
    if sender_id.is_empty() {
        return Err("缺少飞书用户 ID".to_string());
    }
    if owner.is_empty() {
        return Err("当前飞书渠道没有 owner".to_string());
    }

    if let Some(binding) = state
        .storage()
        .get_feishu_user_binding(channel.id, sender_id)
        .await
        .map_err(|err| err.to_string())?
    {
        if binding.enabled && binding.owner_username.trim() == owner {
            return Ok(JoinCollaborationOutcome::AlreadyJoined(binding));
        }
    }

    let invite = state
        .storage()
        .consume_feishu_collaborator_invite(channel.id, code, sender_id)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "邀请码不存在、已失效或已被使用".to_string())?;

    let binding = state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel.id,
            owner,
            sender_id,
            sender_name,
            chat_id,
            chat_type,
            message_id,
            event_id,
            FEISHU_ROLE_COLLABORATOR,
            FEISHU_JOINED_VIA_INVITE_CODE,
            Some(&invite.created_by_sender_id),
            Some(&invite.created_by_sender_name),
            true,
        )
        .await
        .map_err(|err| err.to_string())?;

    Ok(JoinCollaborationOutcome::Joined(binding))
}

fn push_config_string_values(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            for part in text.split([',', ';', '\n', '\r', '\t', ' ']) {
                let part = part.trim();
                if !part.is_empty() {
                    out.push(part.to_string());
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                push_config_string_values(value, out);
            }
        }
        Value::Number(number) => out.push(number.to_string()),
        _ => {}
    }
}
