use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use qrcode::{render::svg, QrCode};

use axum::{
    extract::{Extension, Path, Query, Request, State},
    response::Json,
    routing::{delete, get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::{
    models::{
        auth::RequestIdentity,
        notifications::{
            normalize_notification_type, parse_object_value, MessageNotificationSettings,
            MessageNotificationSettingsUpdatePayload, NotificationBindingPayload,
            NotificationChannel, NotificationChannelPayload, NotificationChannelUpdatePayload,
            NotificationRuntimePatchPayload,
        },
    },
    notifications::{
        driver::{
            render_driver_content, resolve_driver, send_feishu_interactive_with_runtime_parts,
            send_text_with_runtime_parts, DriverContentRequest,
        },
        feishu_members::{bind_qr_owner_primary, generate_collaborator_invite, member_is_primary},
        feishu_native::{
            feishu_console_addons_url, safe_native_error_detail, AppScopeApplyRequest,
            AppScopeStatusRequest, FeishuNativeError, FeishuRegistrationCredentials,
            RegisterPollRequest, RegisterStartRequest, FEISHU_REGISTRATION_DOMAIN,
        },
        openclaw_weixin::{fetch_qr_code, poll_qr_status, ILINK_LOGIN_HOST},
        sanitize::{preserve_redacted_config_values, redact_binding, redact_channel},
        template::{NotificationContentKind, NotificationTemplate, RenderedNotificationContent},
    },
    routes::{
        accounts::messages::should_suppress_chat_unread,
        error::ResponseError,
        tenant::{ensure_account_uids_accessible, ensure_admin_or_legacy, owner_username_filter},
    },
    state::AppState,
};

/// Channel type emitted for QR-scan auto-provisioned channels.
const OPENCLAW_WEIXIN_TYPE: &str = "openclaw_weixin";
const FEISHU_REQUIRED_SCOPES: [&str; 4] = [
    "im:message.group_msg",
    "im:message.group_at_msg:readonly",
    "im:message.p2p_msg:readonly",
    "im:message:send_as_bot",
];

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels"),
            get(list_notification_channels).post(create_notification_channel),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/:channel_id"),
            get(get_notification_channel)
                .put(update_notification_channel)
                .delete(delete_notification_channel),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/:channel_id/runtime"),
            get(get_notification_channel_runtime).post(patch_notification_channel_runtime),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/:channel_id/test"),
            post(test_notification_channel),
        )
        .route(
            crate::sealed_protocol::sealed_str!(
                "/notification-channels/:channel_id/feishu/members"
            ),
            get(list_feishu_channel_members),
        )
        .route(
            crate::sealed_protocol::sealed_str!(
                "/notification-channels/:channel_id/feishu/invites"
            ),
            post(create_feishu_channel_invite),
        )
        .route(
            crate::sealed_protocol::sealed_str!(
                "/notification-channels/:channel_id/feishu/members/:sender_id"
            ),
            delete(delete_feishu_channel_member),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/wechat/qr-start"),
            post(start_wechat_qr_login),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/wechat/qr-poll"),
            post(poll_wechat_qr_login),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/feishu/qr-start"),
            post(start_feishu_qr_registration),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/feishu/qr-poll"),
            post(poll_feishu_qr_registration),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-channels/wechat/daily-report/run"),
            post(run_wechat_daily_report),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message-notifications"),
            get(get_message_notifications),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message-notifications/settings"),
            get(get_message_notification_settings).put(update_message_notification_settings),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message-notifications/:uid"),
            get(get_account_message_notifications)
                .post(set_message_notification)
                .delete(delete_message_notification_compat),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message-notifications/account/:uid"),
            delete(delete_account_notifications),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message-notifications/binding/:binding_id"),
            delete(delete_message_notification),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-hooks/wechat/:channel_id"),
            get(verify_wechat_notification_hook).post(handle_wechat_notification_hook),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-hooks/dingtalk/:channel_id"),
            post(handle_dingtalk_notification_hook),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-hooks/feishu/:channel_id"),
            post(handle_feishu_notification_hook),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-hooks/lark/:channel_id"),
            post(handle_lark_notification_hook),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/notification-hooks/openclaw_weixin/:channel_id"),
            post(handle_openclaw_weixin_notification_hook),
        )
}

async fn list_notification_channels(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<Vec<NotificationChannel>>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let channels = state
        .storage()
        .list_notification_channels_for_owner(&owner)
        .await
        .map_err(ResponseError::internal)?
        .into_iter()
        .map(redact_channel)
        .collect();
    Ok(Json(channels))
}

async fn create_notification_channel(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<NotificationChannelPayload>,
) -> Result<Json<Value>, ResponseError> {
    let channel_type = normalize_notification_type(&payload.channel_type)
        .ok_or_else(|| ResponseError::bad_request("unsupported notification channel type"))?;
    let config = parse_object_value(payload.config).map_err(ResponseError::bad_request)?;

    let owner = notification_owner_scope(&state, &identity);
    let channel = state
        .storage()
        .create_notification_channel_for_owner(
            &owner,
            payload.name.trim(),
            &channel_type,
            &config,
            payload.enabled,
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(Json(json!({
        "status": "ok",
        "channel": redact_channel(channel),
    })))
}

async fn get_notification_channel(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
) -> Result<Json<NotificationChannel>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let Some(channel) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };

    Ok(Json(redact_channel(channel)))
}

async fn update_notification_channel(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    Json(payload): Json<NotificationChannelUpdatePayload>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let Some(existing) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };

    let channel_type = match payload.channel_type.as_deref() {
        Some(value) => normalize_notification_type(value)
            .ok_or_else(|| ResponseError::bad_request("unsupported notification channel type"))?,
        None => existing.channel_type.clone(),
    };
    let config = match payload.config {
        Some(value) => {
            let parsed = parse_object_value(Some(value)).map_err(ResponseError::bad_request)?;
            if channel_type == existing.channel_type {
                preserve_redacted_config_values(&existing.config, parsed)
            } else {
                parsed
            }
        }
        None => existing.config.clone(),
    };
    let name = payload
        .name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(existing.name);
    let enabled = payload.enabled.unwrap_or(existing.enabled);

    let channel = state
        .storage()
        .update_notification_channel_for_owner(
            channel_id,
            &owner,
            &name,
            &channel_type,
            &config,
            enabled,
        )
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("通知渠道不存在"))?;

    if existing.channel_type != channel.channel_type
        || existing.enabled != channel.enabled
        || existing.config != channel.config
    {
        tokio::spawn(crate::notifications::manager::restart_worker_for_channel(
            state.clone(),
            channel.id,
        ));
    }

    Ok(Json(json!({
        "status": "ok",
        "channel": redact_channel(channel),
    })))
}

async fn delete_notification_channel(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let Some(existing) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };

    crate::notifications::manager::stop_worker_for_channel(state.clone(), &existing).await;

    let deleted = state
        .storage()
        .delete_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?;
    if !deleted {
        return Err(ResponseError::not_found("通知渠道不存在"));
    }

    Ok(Json(json!({ "status": "ok" })))
}

async fn get_notification_channel_runtime(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
) -> Result<Json<crate::models::notifications::NotificationRuntime>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    if state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
        .is_none()
    {
        return Err(ResponseError::not_found("通知渠道不存在"));
    }

    Ok(Json(
        state
            .storage()
            .get_notification_runtime(channel_id)
            .await
            .map_err(ResponseError::internal)?,
    ))
}

async fn patch_notification_channel_runtime(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    Json(payload): Json<NotificationRuntimePatchPayload>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    if state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
        .is_none()
    {
        return Err(ResponseError::not_found("通知渠道不存在"));
    }

    let runtime_patch = parse_object_value(payload.runtime).map_err(ResponseError::bad_request)?;
    let runtime = state
        .storage()
        .patch_notification_runtime(channel_id, &runtime_patch)
        .await
        .map_err(ResponseError::internal)?;

    Ok(Json(json!({
        "status": "ok",
        "runtime": runtime,
    })))
}

#[derive(Debug, Deserialize, Default)]
struct NotificationChannelTestPayload {
    message: Option<String>,
}

async fn test_notification_channel(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    Json(payload): Json<NotificationChannelTestPayload>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let Some(channel) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };

    let message = payload.message.unwrap_or_default().trim().to_string();
    let template = build_test_notification_template(&channel.name, &channel.channel_type, &message);
    let rendered = render_test_notification_for_channel(&channel.channel_type, &template);

    send_text_with_runtime_parts(
        &state,
        channel.id,
        &channel.channel_type,
        &channel.config,
        rendered.body.as_str(),
        Some(&rendered),
        &Map::new(),
    )
    .await
    .map_err(ResponseError::bad_request)?;

    Ok(Json(json!({ "status": "ok" })))
}

async fn list_feishu_channel_members(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let channel = owned_feishu_channel(&state, &owner, channel_id).await?;
    let members = state
        .storage()
        .list_feishu_user_bindings_for_owner(channel.id, &owner)
        .await
        .map_err(ResponseError::internal)?;
    let active_invite = state
        .storage()
        .list_active_feishu_collaborator_invites(channel.id, &owner)
        .await
        .map_err(ResponseError::internal)?
        .into_iter()
        .next();

    Ok(Json(json!({
        "status": "ok",
        "members": members,
        "active_invite": active_invite,
    })))
}

async fn create_feishu_channel_invite(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let channel = owned_feishu_channel(&state, &owner, channel_id).await?;
    let primary = state
        .storage()
        .get_primary_feishu_user_binding_for_owner(channel.id, &owner)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| {
            ResponseError::bad_request("当前飞书渠道还没有主用户，请先扫码完成主用户创建")
        })?;
    let sender_name = if primary.sender_name.trim().is_empty() {
        owner.as_str()
    } else {
        primary.sender_name.as_str()
    };
    let invite = generate_collaborator_invite(&state, &channel, &primary.sender_id, sender_name)
        .await
        .map_err(ResponseError::internal_message)?;

    Ok(Json(json!({
        "status": "ok",
        "invite": invite,
    })))
}

async fn delete_feishu_channel_member(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path((channel_id, sender_id)): Path<(i64, String)>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let channel = owned_feishu_channel(&state, &owner, channel_id).await?;
    let binding = state
        .storage()
        .get_feishu_user_binding(channel.id, &sender_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("飞书成员不存在"))?;
    if binding.owner_username.trim() != owner {
        return Err(ResponseError::not_found("飞书成员不存在"));
    }
    if member_is_primary(&binding) {
        return Err(ResponseError::bad_request("不能移除主用户，请使用重绑流程"));
    }

    state
        .storage()
        .delete_feishu_user_binding(channel.id, &sender_id)
        .await
        .map_err(ResponseError::internal)?;

    Ok(Json(json!({ "status": "ok" })))
}

async fn get_message_notifications(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<
    Json<BTreeMap<String, Vec<crate::models::notifications::NotificationBinding>>>,
    ResponseError,
> {
    let owner = notification_owner_scope(&state, &identity);
    let bindings = state
        .storage()
        .list_notification_bindings_for_owner(None, Some(&owner))
        .await
        .map_err(ResponseError::internal)?;
    let mut grouped = BTreeMap::new();
    for binding in bindings {
        let binding = redact_binding(binding);
        grouped
            .entry(binding.uid.clone())
            .or_insert_with(Vec::new)
            .push(binding);
    }
    Ok(Json(grouped))
}

async fn get_message_notification_settings(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<MessageNotificationSettings>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let platform_messages_enabled = state
        .storage()
        .get_platform_messages_enabled_for_owner(&owner)
        .await
        .map_err(ResponseError::internal)?;

    Ok(Json(MessageNotificationSettings {
        platform_messages_enabled,
    }))
}

async fn update_message_notification_settings(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<MessageNotificationSettingsUpdatePayload>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let platform_messages_enabled = state
        .storage()
        .upsert_platform_messages_enabled_for_owner(&owner, payload.platform_messages_enabled)
        .await
        .map_err(ResponseError::internal)?;
    if !platform_messages_enabled {
        clear_platform_message_unread_for_owner(
            state.storage().accounts(),
            state.storage().live_state(),
            &owner,
        )
        .await
        .map_err(ResponseError::internal)?;
    }

    Ok(Json(json!({
        "status": "ok",
        "settings": MessageNotificationSettings {
            platform_messages_enabled,
        },
    })))
}

async fn clear_platform_message_unread_for_owner(
    accounts_pool: &SqlitePool,
    live_pool: &SqlitePool,
    owner_username: &str,
) -> Result<u64, sqlx::Error> {
    let owner = owner_username.trim();
    let uids = if owner.is_empty() {
        sqlx::query_scalar::<_, String>(
            "SELECT uid FROM accounts WHERE COALESCE(owner_username, '') = ''",
        )
        .fetch_all(accounts_pool)
        .await?
    } else {
        sqlx::query_scalar::<_, String>("SELECT uid FROM accounts WHERE owner_username = ?")
            .bind(owner)
            .fetch_all(accounts_pool)
            .await?
    };
    if uids.is_empty() {
        return Ok(0);
    }

    let mut builder = QueryBuilder::<Sqlite>::new(
        "UPDATE message_live_state SET unread_count = 0 WHERE unread_count > 0 AND session_type IN (3, 6, 53) AND uid IN (",
    );
    {
        let mut separated = builder.separated(", ");
        for uid in &uids {
            separated.push_bind(uid);
        }
    }
    builder.push(")");

    let mut affected = builder.build().execute(live_pool).await?.rows_affected();

    let mut select_builder = QueryBuilder::<Sqlite>::new(
        "SELECT uid, session_id, session_type, COALESCE(summary, '') AS summary \
         FROM message_live_state WHERE unread_count > 0 AND session_type = 1 AND uid IN (",
    );
    {
        let mut separated = select_builder.separated(", ");
        for uid in &uids {
            separated.push_bind(uid);
        }
    }
    select_builder.push(")");
    let legacy_rows = select_builder.build().fetch_all(live_pool).await?;
    let legacy_sessions = legacy_rows
        .into_iter()
        .filter_map(|row| {
            let session_type = row.get::<i64, _>("session_type");
            let summary = row.get::<String, _>("summary");
            if should_suppress_chat_unread(session_type, &summary) {
                Some((
                    row.get::<String, _>("uid"),
                    row.get::<String, _>("session_id"),
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if legacy_sessions.is_empty() {
        return Ok(affected);
    }

    let mut clear_builder = QueryBuilder::<Sqlite>::new(
        "UPDATE message_live_state SET unread_count = 0 WHERE unread_count > 0 AND (",
    );
    let mut first = true;
    for (uid, session_id) in &legacy_sessions {
        if !first {
            clear_builder.push(" OR ");
        }
        clear_builder.push("(uid = ");
        clear_builder.push_bind(uid);
        clear_builder.push(" AND session_id = ");
        clear_builder.push_bind(session_id);
        clear_builder.push(")");
        first = false;
    }
    clear_builder.push(")");
    affected += clear_builder
        .build()
        .execute(live_pool)
        .await?
        .rows_affected();
    Ok(affected)
}

async fn get_account_message_notifications(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> Result<Json<Vec<crate::models::notifications::NotificationBinding>>, ResponseError> {
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        std::slice::from_ref(&uid),
    )
    .await?;
    let channel_owner = notification_owner_scope(&state, &identity);
    let bindings = state
        .storage()
        .list_notification_bindings_for_owner(Some(uid.as_str()), Some(&channel_owner))
        .await
        .map_err(ResponseError::internal)?
        .into_iter()
        .map(redact_binding)
        .collect();
    Ok(Json(bindings))
}

async fn set_message_notification(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
    Json(payload): Json<NotificationBindingPayload>,
) -> Result<Json<Value>, ResponseError> {
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        std::slice::from_ref(&uid),
    )
    .await?;
    let channel_owner = notification_owner_scope(&state, &identity);
    let binding = state
        .storage()
        .set_notification_binding_for_owner(
            &uid,
            payload.channel_id,
            payload.enabled,
            Some(&channel_owner),
        )
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::bad_request("notification channel does not exist"))?;

    Ok(Json(json!({
        "status": "ok",
        "binding": redact_binding(binding),
    })))
}

async fn delete_account_notifications(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> Result<Json<Value>, ResponseError> {
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        std::slice::from_ref(&uid),
    )
    .await?;
    let channel_owner = notification_owner_scope(&state, &identity);
    let deleted = state
        .storage()
        .delete_notification_uid_bindings_for_owner(&uid, Some(&channel_owner))
        .await
        .map_err(ResponseError::internal)?;

    Ok(Json(json!({
        "status": "ok",
        "deleted": deleted,
    })))
}

async fn delete_message_notification(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(binding_id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let deleted = state
        .storage()
        .delete_notification_binding_for_owner(binding_id, Some(&owner))
        .await
        .map_err(ResponseError::internal)?;
    if !deleted {
        return Err(ResponseError::not_found("通知绑定不存在"));
    }

    Ok(Json(json!({ "status": "ok" })))
}

async fn delete_message_notification_compat(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(binding_id): Path<String>,
) -> Result<Json<Value>, ResponseError> {
    let binding_id = binding_id
        .parse::<i64>()
        .map_err(|_| ResponseError::bad_request("通知绑定不存在"))?;
    delete_message_notification(State(state), Extension(identity), Path(binding_id)).await
}

async fn verify_wechat_notification_hook(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    Query(query): Query<BTreeMap<String, String>>,
) -> Result<String, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let Some(channel) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };
    if channel.channel_type != "wechat" {
        return Err(ResponseError::bad_request("通知渠道类型不匹配"));
    }

    let mut runtime_patch = Map::new();
    runtime_patch.insert(
        "last_callback_source".to_string(),
        Value::String("wechat".to_string()),
    );
    runtime_patch.insert(
        "last_callback_method".to_string(),
        Value::String("GET".to_string()),
    );
    if let Some(echostr) = query.get("echostr") {
        runtime_patch.insert(
            "last_callback_echostr".to_string(),
            Value::String(echostr.clone()),
        );
    }

    state
        .storage()
        .patch_notification_runtime(channel_id, &runtime_patch)
        .await
        .map_err(ResponseError::internal)?;

    Ok(query.get("echostr").cloned().unwrap_or_default())
}

async fn handle_wechat_notification_hook(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    request: Request,
) -> Result<Json<Value>, ResponseError> {
    handle_notification_callback(state, &identity, "wechat".to_string(), channel_id, request).await
}

async fn handle_dingtalk_notification_hook(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    request: Request,
) -> Result<Json<Value>, ResponseError> {
    handle_notification_callback(
        state,
        &identity,
        "dingtalk".to_string(),
        channel_id,
        request,
    )
    .await
}

async fn handle_feishu_notification_hook(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    request: Request,
) -> Result<Json<Value>, ResponseError> {
    handle_notification_callback(state, &identity, "feishu".to_string(), channel_id, request).await
}

async fn handle_lark_notification_hook(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    request: Request,
) -> Result<Json<Value>, ResponseError> {
    handle_notification_callback(state, &identity, "lark".to_string(), channel_id, request).await
}

async fn handle_openclaw_weixin_notification_hook(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(channel_id): Path<i64>,
    request: Request,
) -> Result<Json<Value>, ResponseError> {
    handle_notification_callback(
        state,
        &identity,
        "openclaw_weixin".to_string(),
        channel_id,
        request,
    )
    .await
}

/// Body for `POST /api/notification-channels/wechat/qr-poll`.
///
/// `host` carries the poll target host between rounds — after the first call
/// it's whatever `redirect_host` was returned (or the login host on the very
/// first poll). `verify_code` is only set when the previous poll returned
/// `need_verifycode` and the user has typed the code.
///
/// Once the upstream confirms the login, channel auto-provision still goes
/// through the same manager restart surface as route updates and dispatcher
/// self-heal, so worker runtime fields remain consistent.
#[derive(Debug, Deserialize)]
struct WechatQrPollPayload {
    #[serde(default)]
    qrcode: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    verify_code: Option<String>,
}

/// Kick off QR-code login against Tencent's iLink login host.
///
/// `POST /api/notification-channels/wechat/qr-start`
/// Returns `{qrcode, qrcode_img_content, host}` — the frontend renders the
/// image and starts polling `/qr-poll` with the same opaque `qrcode` token.
async fn start_wechat_qr_login(
    State(_state): State<AppState>,
    Extension(_identity): Extension<RequestIdentity>,
) -> Result<Json<Value>, ResponseError> {
    let resp = fetch_qr_code()
        .await
        .map_err(ResponseError::internal_message)?;

    // 腾讯返回的 `qrcode_img_content` 实际是个 https://liteapp.weixin.qq.com/q/...
    // 跳转 URL，不是 PNG。微信扫码后才会跳到登录确认页。前端直接 <img src=URL>
    // 拿不到图。后端这边把 URL 编码成 SVG QR data: URL，前端渲染那个二维码用户
    // 用微信扫即可。
    let qr_data_url = QrCode::new(resp.qrcode_img_content.as_bytes())
        .map_err(|err| ResponseError::internal_message(err.to_string()))
        .map(|code| {
            let svg = code
                .render::<svg::Color<'_>>()
                .min_dimensions(240, 240)
                .quiet_zone(true)
                .build();
            format!(
                "data:image/svg+xml;base64,{}",
                BASE64_STANDARD.encode(svg.as_bytes())
            )
        })?;

    Ok(Json(json!({
        "qrcode": resp.qrcode,
        // 同时返回原 URL（调试用）和已编码的 SVG data URL（前端实际渲染用）。
        "qrcode_img_content": qr_data_url,
        "qrcode_url": resp.qrcode_img_content,
        "host": ILINK_LOGIN_HOST,
    })))
}

/// Long-poll the QR-code login status.
///
/// `POST /api/notification-channels/wechat/qr-poll`
///
/// Once the iLink upstream returns `confirmed`, this endpoint immediately
/// auto-creates an `openclaw_weixin` channel with `bind_all_accounts: true`
/// in the channels table and returns the freshly-created row in `channel`.
/// The frontend's WeChat shortcut button relies on that so the user never
/// has to fill out a config form.
async fn poll_wechat_qr_login(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<WechatQrPollPayload>,
) -> Result<Json<Value>, ResponseError> {
    if payload.qrcode.trim().is_empty() {
        return Err(ResponseError::bad_request("qrcode is required"));
    }
    let host = if payload.host.trim().is_empty() {
        ILINK_LOGIN_HOST.to_string()
    } else {
        payload.host.trim().to_string()
    };
    let verify_code = payload
        .verify_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let resp = poll_qr_status(&host, payload.qrcode.trim(), verify_code)
        .await
        .map_err(ResponseError::internal_message)?;

    let mut out = json!({ "status": resp.status });
    if !resp.bot_token.is_empty() {
        out["bot_token"] = Value::String(resp.bot_token.clone());
    }
    if !resp.ilink_bot_id.is_empty() {
        out["ilink_bot_id"] = Value::String(resp.ilink_bot_id.clone());
    }
    if !resp.ilink_user_id.is_empty() {
        out["ilink_user_id"] = Value::String(resp.ilink_user_id.clone());
    }
    if !resp.baseurl.is_empty() {
        out["baseurl"] = Value::String(resp.baseurl.clone());
    }
    if !resp.redirect_host.is_empty() {
        out["redirect_host"] = Value::String(resp.redirect_host.clone());
    }

    if resp.status == "confirmed" && !resp.bot_token.is_empty() && !resp.ilink_user_id.is_empty() {
        let channel = auto_provision_openclaw_weixin_channel(
            &state,
            &notification_owner_scope(&state, &identity),
            &resp.bot_token,
            &resp.ilink_user_id,
            &resp.baseurl,
            &resp.ilink_bot_id,
        )
        .await?;
        out["channel"] =
            serde_json::to_value(redact_channel(channel)).map_err(ResponseError::internal)?;
    }

    Ok(Json(out))
}

#[derive(Debug, Deserialize)]
struct FeishuQrPollPayload {
    #[serde(default)]
    device_code: String,
    #[serde(default)]
    domain: String,
}

async fn start_feishu_qr_registration(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, &identity);
    let resp = state
        .feishu_native()
        .register_start(RegisterStartRequest::default())
        .await
        .map_err(map_feishu_native_error)?;
    state
        .feishu_qr_registrations()
        .remember(&resp.device_code, &owner, &resp.domain)
        .await;
    let qr_image = render_qr_data_url(&resp.qr_url)?;

    Ok(Json(json!({
        "device_code": resp.device_code,
        "qr_url": resp.qr_url,
        "qr_image": qr_image,
        "expires_at_ms": resp.expires_at_ms,
        "poll_interval_ms": resp.poll_interval_ms,
        "domain": resp.domain,
        "required_scopes": feishu_required_scopes_json(),
        "publish_required": true,
    })))
}

async fn poll_feishu_qr_registration(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<FeishuQrPollPayload>,
) -> Result<Json<Value>, ResponseError> {
    let device_code = payload.device_code.trim();
    if device_code.is_empty() {
        return Err(ResponseError::bad_request("device_code is required"));
    }
    let owner = notification_owner_scope(&state, &identity);
    let Some(registration) = state.feishu_qr_registrations().get(device_code).await else {
        return Err(ResponseError::bad_request(
            "Feishu QR registration was not started by this session",
        ));
    };
    if registration.owner_username != owner {
        return Err(ResponseError::forbidden(
            "Feishu QR registration belongs to another owner",
        ));
    }
    let requested_domain = if payload.domain.trim().is_empty() {
        registration.domain.as_str()
    } else {
        payload.domain.as_str()
    };
    let domain = normalize_feishu_registration_domain(requested_domain).to_string();

    let resp = state
        .feishu_native()
        .register_poll(RegisterPollRequest {
            device_code: device_code.to_string(),
            domain,
        })
        .await
        .map_err(map_feishu_native_error)?;

    let confirmed = resp.status == "confirmed" || resp.credentials.is_some();
    let response_status = if confirmed {
        "confirmed"
    } else if resp.status.trim().is_empty() {
        "pending"
    } else {
        resp.status.as_str()
    };

    let mut out = json!({
        "status": response_status,
        "device_code": resp.device_code,
        "domain": resp.domain,
        "required_scopes": feishu_required_scopes_json(),
        "publish_required": true,
    });

    if confirmed {
        let credentials = resp.credentials.ok_or_else(|| {
            ResponseError::internal_message("confirmed Feishu registration missing credentials")
        })?;
        let scope_state = reconcile_feishu_registration_scopes(&state, &credentials).await?;
        out["publish_required"] = Value::Bool(scope_state.publish_required);
        if !scope_state.ready {
            out["status"] = Value::String(scope_state.status);
            if !scope_state.missing_scopes.is_empty() {
                out["missing_scopes"] = Value::Array(
                    scope_state
                        .missing_scopes
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                );
            }
            if let Some(qr_url) = scope_state.qr_url {
                out["qr_url"] = Value::String(qr_url.clone());
                out["qr_image"] = Value::String(render_qr_data_url(&qr_url)?);
            }
            if let Some(hint) = scope_state.hint {
                out["scope_hint"] = Value::String(hint);
            }
            return Ok(Json(out));
        }
        let operator_open_id = credentials.operator_open_id.trim().to_string();
        let (channel, should_send_bootstrap) = upsert_feishu_registration_channel(
            &state,
            &owner,
            &normalize_feishu_registration_credentials(credentials),
        )
        .await?;
        if !operator_open_id.is_empty() {
            bind_qr_owner_primary(&state, &channel, &operator_open_id)
                .await
                .map_err(ResponseError::internal_message)?;
        }
        if should_send_bootstrap {
            send_feishu_registration_bootstrap(&state, &channel).await;
        }
        out["channel"] =
            serde_json::to_value(redact_channel(channel)).map_err(ResponseError::internal)?;
    }
    if matches!(response_status, "expired" | "failed" | "denied") {
        state.feishu_qr_registrations().forget(device_code).await;
    }

    Ok(Json(out))
}

fn normalize_feishu_registration_credentials(
    credentials: FeishuRegistrationCredentials,
) -> FeishuRegistrationCredentials {
    FeishuRegistrationCredentials {
        domain: normalize_feishu_registration_domain(&credentials.domain).to_string(),
        ..credentials
    }
}

/// Manually fire the daily report — synchronously aggregates today's stats
/// and pushes through every bind-all WeChat channel. Lets the operator verify
/// the wire path without waiting for the scheduled trigger.
///
/// `POST /api/notification-channels/wechat/daily-report/run` — no body. Mounted
/// inside the protected router so the standard bearer-token middleware
/// (admin scope) gates access.
///
/// Returns `{status:"ok", sent: N, errors: [...]}`; `errors` is empty on a
/// clean fan-out.
async fn run_wechat_daily_report(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<Value>, ResponseError> {
    ensure_admin_or_legacy(&state, &identity)?;
    let outcome = crate::notifications::daily_report::run_daily_report(&state).await;
    Ok(Json(json!({
        "status": "ok",
        "sent": outcome.sent,
        "errors": outcome.errors,
    })))
}

/// Insert a fresh `openclaw_weixin` channel right after the user scans the
/// iLink login QR — captures everything needed for the dispatcher to forward
/// IM events without any further user input. Sets `bind_all_accounts: true`
/// so the dispatcher sprays to every active account without needing per-uid
/// binding rows.
///
/// Worker startup for the new channel is delegated to the unified manager
/// restart entrypoint so QR auto-provision, route updates, and dispatcher
/// self-heal all share the same stop/spawn/runtime semantics.
async fn auto_provision_openclaw_weixin_channel(
    state: &AppState,
    owner_username: &str,
    bot_token: &str,
    ilink_user_id: &str,
    baseurl: &str,
    ilink_bot_id: &str,
) -> Result<crate::models::notifications::NotificationChannel, ResponseError> {
    let base_url = if baseurl.trim().is_empty() {
        ILINK_LOGIN_HOST.to_string()
    } else {
        baseurl.trim().to_string()
    };

    // Short suffix from the iLink user id keeps the channel name readable
    // when the operator has multiple WeChat hooks bound. Falls back to the
    // full id if it's shorter than the slice window.
    let short = {
        let id = ilink_user_id.trim();
        if id.chars().count() > 6 {
            id.chars()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<String>()
        } else {
            id.to_string()
        }
    };
    let name = format!("微信 ({short})");

    let mut config: Map<String, Value> = Map::new();
    config.insert("base_url".to_string(), Value::String(base_url));
    config.insert(
        "bot_token".to_string(),
        Value::String(bot_token.to_string()),
    );
    config.insert(
        "ilink_user_id".to_string(),
        Value::String(ilink_user_id.to_string()),
    );
    if !ilink_bot_id.trim().is_empty() {
        config.insert(
            "ilink_bot_id".to_string(),
            Value::String(ilink_bot_id.trim().to_string()),
        );
    }
    config.insert("bind_all_accounts".to_string(), Value::Bool(true));

    let channel = state
        .storage()
        .create_notification_channel_for_owner(
            owner_username,
            name.trim(),
            OPENCLAW_WEIXIN_TYPE,
            &config,
            true,
        )
        .await
        .map_err(ResponseError::internal)?;

    // Kick off the long-poll worker for this freshly-bound channel so the
    // operator can immediately receive WeChat commands without needing to
    // restart the backend.
    tokio::spawn(crate::notifications::manager::restart_worker_for_channel(
        state.clone(),
        channel.id,
    ));

    Ok(channel)
}

async fn upsert_feishu_registration_channel(
    state: &AppState,
    owner_username: &str,
    credentials: &FeishuRegistrationCredentials,
) -> Result<(crate::models::notifications::NotificationChannel, bool), ResponseError> {
    let app_id = credentials.app_id.trim().to_string();
    let app_secret = credentials.app_secret.trim().to_string();
    let domain = normalize_feishu_registration_domain(&credentials.domain).to_string();
    let existing = state
        .storage()
        .list_notification_channels_for_owner(owner_username)
        .await
        .map_err(ResponseError::internal)?
        .into_iter()
        .find(|channel| {
            channel.channel_type == "feishu"
                && feishu_channel_matches_registration(channel, &app_id, &domain)
        });
    let is_new_channel = existing.is_none();
    let operator_open_id = credentials.operator_open_id.trim();
    let operator_already_allowed = if operator_open_id.is_empty() {
        false
    } else {
        existing.as_ref().is_some_and(|channel| {
            channel
                .config
                .get("allowed_operator_ids")
                .map(config_string_list)
                .unwrap_or_default()
                .into_iter()
                .any(|value| value == operator_open_id)
        })
    };

    let mut config = existing
        .as_ref()
        .map(|channel| channel.config.clone())
        .unwrap_or_default();
    config.insert("app_id".to_string(), Value::String(app_id.clone()));
    config.insert("app_secret".to_string(), Value::String(app_secret));
    config.insert("domain".to_string(), Value::String(domain));
    if !operator_open_id.is_empty() {
        merge_config_string_set(&mut config, "allowed_operator_ids", operator_open_id);
    }
    insert_feishu_config_defaults(&mut config);

    let channel = match existing {
        Some(channel) => state
            .storage()
            .update_notification_channel_for_owner(
                channel.id,
                owner_username,
                &channel.name,
                "feishu",
                &config,
                true,
            )
            .await
            .map_err(ResponseError::internal)?
            .ok_or_else(|| ResponseError::not_found("通知渠道不存在")),
        None => {
            let name = feishu_channel_name(&app_id);
            state
                .storage()
                .create_notification_channel_for_owner(
                    owner_username,
                    &name,
                    "feishu",
                    &config,
                    true,
                )
                .await
                .map_err(ResponseError::internal)
        }
    }?;

    tokio::spawn(crate::notifications::manager::restart_worker_for_channel(
        state.clone(),
        channel.id,
    ));

    Ok((
        channel,
        is_new_channel || (!operator_open_id.is_empty() && !operator_already_allowed),
    ))
}

fn merge_config_string_set(config: &mut Map<String, Value>, key: &str, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let mut values = config.get(key).map(config_string_list).unwrap_or_default();
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
    config.insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
}

fn config_string_list(value: &Value) -> Vec<String> {
    let mut values = Vec::new();
    push_config_string_values(value, &mut values);
    values.sort();
    values.dedup();
    values
}

fn push_config_string_values(value: &Value, values: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            for part in text.split([',', ';', '\n', '\r', '\t', ' ']) {
                let part = part.trim();
                if !part.is_empty() {
                    values.push(part.to_string());
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                push_config_string_values(item, values);
            }
        }
        Value::Number(number) => values.push(number.to_string()),
        _ => {}
    }
}

fn feishu_channel_matches_registration(
    channel: &crate::models::notifications::NotificationChannel,
    app_id: &str,
    domain: &str,
) -> bool {
    let existing_app_id = channel
        .config
        .get("app_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let existing_domain = channel
        .config
        .get("domain")
        .and_then(Value::as_str)
        .unwrap_or(FEISHU_REGISTRATION_DOMAIN);

    existing_app_id == app_id && normalize_feishu_registration_domain(existing_domain) == domain
}

fn insert_feishu_config_defaults(config: &mut Map<String, Value>) {
    config
        .entry("receive_id".to_string())
        .or_insert_with(|| Value::String(String::new()));
    config
        .entry("receive_id_type".to_string())
        .or_insert_with(|| Value::String("chat_id".to_string()));
    config
        .entry("bind_all_accounts".to_string())
        .or_insert_with(|| Value::Bool(true));
    config
        .entry("require_mention".to_string())
        .or_insert_with(|| Value::Bool(true));
    config
        .entry("enable_stream".to_string())
        .or_insert_with(|| Value::Bool(false));
    config
        .entry("im_aggregate_window_ms".to_string())
        .or_insert_with(|| Value::Number(2_000.into()));
    config.entry("capabilities".to_string()).or_insert_with(|| {
        json!({
            "im_reply": true,
            "item_actions": true,
            "order_actions": true
        })
    });
    config
        .entry("required_scopes".to_string())
        .or_insert_with(|| {
            Value::Array(
                FEISHU_REQUIRED_SCOPES
                    .iter()
                    .map(|value| Value::String((*value).to_string()))
                    .collect(),
            )
        });
    config
        .entry("scope_publish_required".to_string())
        .or_insert_with(|| Value::Bool(true));
    config
        .entry("group_message_without_mention_expected".to_string())
        .or_insert_with(|| Value::Bool(true));
}

fn feishu_required_scopes_json() -> Value {
    Value::Array(
        FEISHU_REQUIRED_SCOPES
            .iter()
            .map(|value| Value::String((*value).to_string()))
            .collect(),
    )
}

fn render_qr_data_url(value: &str) -> Result<String, ResponseError> {
    QrCode::new(value.as_bytes())
        .map_err(|err| ResponseError::internal_message(err.to_string()))
        .map(|code| {
            let svg = code
                .render::<svg::Color<'_>>()
                .min_dimensions(240, 240)
                .quiet_zone(true)
                .build();
            format!(
                "data:image/svg+xml;base64,{}",
                BASE64_STANDARD.encode(svg.as_bytes())
            )
        })
}

struct FeishuScopeReconcileState {
    ready: bool,
    status: String,
    publish_required: bool,
    missing_scopes: Vec<String>,
    qr_url: Option<String>,
    hint: Option<String>,
}

async fn reconcile_feishu_registration_scopes(
    state: &AppState,
    credentials: &FeishuRegistrationCredentials,
) -> Result<FeishuScopeReconcileState, ResponseError> {
    let request = AppScopeStatusRequest {
        app_id: credentials.app_id.trim().to_string(),
        app_secret: credentials.app_secret.trim().to_string(),
        domain: credentials.domain.trim().to_string(),
    };
    let status = state
        .feishu_native()
        .app_scope_status(request.clone())
        .await
        .map_err(map_feishu_native_error)?;
    let mut granted = BTreeMap::new();
    for scope in status.scopes {
        granted.insert(scope.scope_name, scope.grant_status);
    }
    let mut missing_manifest = Vec::new();
    let mut pending_apply = Vec::new();
    for scope in FEISHU_REQUIRED_SCOPES {
        match granted.get(scope).copied() {
            Some(1) => {}
            Some(_) => pending_apply.push(scope.to_string()),
            None => missing_manifest.push(scope.to_string()),
        }
    }
    if !pending_apply.is_empty() {
        let _ = state
            .feishu_native()
            .app_scope_apply(AppScopeApplyRequest {
                app_id: request.app_id.clone(),
                app_secret: request.app_secret.clone(),
                domain: request.domain.clone(),
            })
            .await;
        let refreshed = state
            .feishu_native()
            .app_scope_status(request)
            .await
            .map_err(map_feishu_native_error)?;
        let mut refreshed_granted = BTreeMap::new();
        for scope in refreshed.scopes {
            refreshed_granted.insert(scope.scope_name, scope.grant_status);
        }
        pending_apply
            .retain(|scope| refreshed_granted.get(scope).copied().unwrap_or_default() != 1);
    }
    if missing_manifest.is_empty() && pending_apply.is_empty() {
        return Ok(FeishuScopeReconcileState {
            ready: true,
            status: "confirmed".to_string(),
            publish_required: false,
            missing_scopes: Vec::new(),
            qr_url: None,
            hint: None,
        });
    }
    if !missing_manifest.is_empty() {
        let qr_url =
            feishu_console_addons_url(&credentials.app_id, &credentials.domain, &missing_manifest);
        return Ok(FeishuScopeReconcileState {
            ready: false,
            status: "scope_setup_required".to_string(),
            publish_required: false,
            missing_scopes: missing_manifest,
            qr_url,
            hint: Some(
                "请再扫一次这个补权二维码，飞书会把缺少的免审权限直接加到应用里。".to_string(),
            ),
        });
    }
    Ok(FeishuScopeReconcileState {
        ready: false,
        status: "scope_apply_pending".to_string(),
        publish_required: true,
        missing_scopes: pending_apply,
        qr_url: None,
        hint: Some("权限已加入应用，但仍待飞书侧完成授权或发布。".to_string()),
    })
}

fn normalize_feishu_registration_domain(domain: &str) -> &'static str {
    match domain.trim().to_ascii_lowercase().as_str() {
        "lark" | "accounts.larksuite.com" => "lark",
        _ => FEISHU_REGISTRATION_DOMAIN,
    }
}

fn feishu_channel_name(app_id: &str) -> String {
    let suffix = app_id.trim();
    if suffix.is_empty() {
        return "飞书".to_string();
    }
    let short = if suffix.chars().count() > 6 {
        suffix
            .chars()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    } else {
        suffix.to_string()
    };
    format!("飞书 ({short})")
}

async fn send_feishu_registration_bootstrap(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
) {
    if !matches!(channel.channel_type.as_str(), "feishu" | "lark") {
        return;
    }

    let mut runtime_patch = Map::new();
    runtime_patch.insert(
        "feishu_bootstrap_status".to_string(),
        Value::String("sent".to_string()),
    );
    runtime_patch.insert(
        "feishu_bootstrap_kind".to_string(),
        Value::String("welcome_card".to_string()),
    );

    let _ = send_feishu_interactive_with_runtime_parts(
        state,
        channel.id,
        &channel.config,
        build_feishu_registration_bootstrap_card(&channel.owner_username),
        &runtime_patch,
    )
    .await;
}

fn build_feishu_registration_bootstrap_card(owner_username: &str) -> Value {
    let owner_line = if owner_username.trim().is_empty() {
        "绑定成功，当前飞书号已经成为主用户。".to_string()
    } else {
        format!(
            "绑定成功，当前 owner：`{}`。当前飞书号已经成为主用户。",
            owner_username.trim()
        )
    };
    json!({
        "config": {
            "wide_screen_mode": true,
            "enable_forward": true
        },
        "header": {
            "template": "blue",
            "title": {
                "tag": "plain_text",
                "content": "鱼管家飞书助手"
            }
        },
        "elements": [
            {
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": format!(
                        "{}\n\n后续每个新会话都会自动创建飞书群聊。\n直接在群里发送回复内容即可回复客户。",
                        owner_line
                    )
                }
            }
        ]
    })
}

fn map_feishu_native_error(error: FeishuNativeError) -> ResponseError {
    match error {
        FeishuNativeError::Unavailable(message) => ResponseError::service_unavailable(
            safe_native_error_detail("Feishu native transport unavailable", &message),
        ),
        FeishuNativeError::Protocol(message) => ResponseError::internal_message(
            safe_native_error_detail("Feishu native transport protocol error", &message),
        ),
        FeishuNativeError::Api(message) => ResponseError::internal_message(
            safe_native_error_detail("Feishu native transport API error", &message),
        ),
        FeishuNativeError::Serde(_) => {
            ResponseError::internal_message("Feishu native transport JSON error")
        }
    }
}

fn notification_owner_scope(_state: &AppState, identity: &RequestIdentity) -> String {
    identity.username.trim().to_string()
}

async fn owned_feishu_channel(
    state: &AppState,
    owner_username: &str,
    channel_id: i64,
) -> Result<NotificationChannel, ResponseError> {
    let Some(channel) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, owner_username)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };
    if !matches!(channel.channel_type.as_str(), "feishu" | "lark") {
        return Err(ResponseError::bad_request("当前通知渠道不是飞书"));
    }
    Ok(channel)
}

async fn handle_notification_callback(
    state: AppState,
    identity: &RequestIdentity,
    channel_type: String,
    channel_id: i64,
    request: Request,
) -> Result<Json<Value>, ResponseError> {
    let owner = notification_owner_scope(&state, identity);
    let Some(channel) = state
        .storage()
        .get_notification_channel_for_owner(channel_id, &owner)
        .await
        .map_err(ResponseError::internal)?
    else {
        return Err(ResponseError::not_found("通知渠道不存在"));
    };

    let normalized_type = normalize_notification_type(&channel_type)
        .ok_or_else(|| ResponseError::bad_request("通知渠道类型不匹配"))?;
    if !callback_type_matches_channel(&normalized_type, &channel.channel_type) {
        return Err(ResponseError::bad_request("通知渠道类型不匹配"));
    }

    let (parts, body) = request.into_parts();
    let method = parts.method.to_string();
    let collected = axum::body::to_bytes(body, usize::MAX)
        .await
        .map_err(|_| ResponseError::internal_message("callback body read failed"))?;
    let payload_value =
        serde_json::from_slice::<Value>(&collected).unwrap_or(Value::Object(Map::new()));

    let mut runtime_patch = Map::new();
    runtime_patch.insert(
        "last_callback_source".to_string(),
        Value::String(channel.channel_type.clone()),
    );
    runtime_patch.insert(
        "last_callback_route_type".to_string(),
        Value::String(normalized_type.clone()),
    );
    runtime_patch.insert("last_callback_method".to_string(), Value::String(method));
    if let Some(summary) = callback_payload_summary(&payload_value) {
        runtime_patch.insert(
            "last_callback_payload_summary".to_string(),
            Value::Object(summary),
        );
    }

    state
        .storage()
        .patch_notification_runtime(channel_id, &runtime_patch)
        .await
        .map_err(ResponseError::internal)?;

    Ok(Json(json!({
        "status": "ok",
        "channel_id": channel_id,
    })))
}

fn callback_type_matches_channel(route_type: &str, channel_type: &str) -> bool {
    route_type == channel_type
        || ((route_type == "feishu" || route_type == "lark")
            && (channel_type == "feishu" || channel_type == "lark"))
}

fn callback_payload_summary(payload: &Value) -> Option<Map<String, Value>> {
    let object = payload.as_object()?;
    let mut summary = Map::new();
    for key in [
        "event",
        "type",
        "event_type",
        "message_id",
        "msg_id",
        "chat_type",
        "status",
    ] {
        if let Some(value) = object.get(key).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty() && value.len() <= 120 {
                summary.insert(key.to_string(), Value::String(value.to_string()));
            }
        }
    }
    (!summary.is_empty()).then_some(summary)
}

fn build_test_notification_template(
    channel_name: &str,
    channel_type: &str,
    message: &str,
) -> NotificationTemplate {
    let detail = if message.trim().is_empty() {
        "系统测试通知".to_string()
    } else {
        message.trim().to_string()
    };
    let time_display = now_display();
    let plain = format!(
        "✅ 测试通知\n渠道: {channel_name}\n类型: {channel_type}\n时间: {time_display}\n内容: {detail}"
    );
    let html = format!(
        "<b>测试通知</b><br/>渠道: <b>{}</b><br/>类型: <code>{}</code><br/>时间: <code>{}</code><br/><blockquote>{}</blockquote>",
        escape_html(channel_name),
        escape_html(channel_type),
        escape_html(&time_display),
        escape_html(&detail),
    );
    let markdown = format!(
        "## ✅ 测试通知\n\n- 渠道：**{}**\n- 类型：`{}`\n- 时间：`{}`\n\n> {}",
        channel_name, channel_type, time_display, detail,
    );

    NotificationTemplate::new(plain.clone())
        .with_text(plain)
        .with_html(html)
        .with_markdown(markdown)
}

fn render_test_notification_for_channel(
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

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn unix_now_f64() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn now_display() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let output = std::process::Command::new("date")
        .arg("-r")
        .arg(secs.to_string())
        .arg("+%Y-%m-%d %H:%M:%S")
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => secs.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{Row, SqlitePool};

    use super::clear_platform_message_unread_for_owner;

    #[tokio::test]
    async fn clear_platform_message_unread_only_touches_platform_sessions_for_owner() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();

        sqlx::query(
            r#"
            CREATE TABLE accounts (
                uid TEXT PRIMARY KEY,
                owner_username TEXT DEFAULT ''
            )
            "#,
        )
        .execute(&accounts)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE message_live_state (
                uid TEXT NOT NULL,
                session_id TEXT NOT NULL,
                session_type INTEGER DEFAULT 1,
                unread_count INTEGER DEFAULT 0,
                PRIMARY KEY (uid, session_id)
            )
            "#,
        )
        .execute(&live)
        .await
        .unwrap();

        for (uid, owner) in [("u1", "owner-a"), ("u2", "owner-b")] {
            sqlx::query("INSERT INTO accounts (uid, owner_username) VALUES (?, ?)")
                .bind(uid)
                .bind(owner)
                .execute(&accounts)
                .await
                .unwrap();
        }
        for (uid, session_id, session_type, unread_count) in [
            ("u1", "chat-1", 1, 5),
            ("u1", "chat-tip-1", 1, 6),
            ("u1", "sys-1", 3, 7),
            ("u1", "fav-1", 6, 4),
            ("u1", "wb-1", 53, 3),
            ("u2", "chat-tip-2", 1, 8),
            ("u2", "sys-2", 3, 9),
        ] {
            sqlx::query(
                "INSERT INTO message_live_state (uid, session_id, session_type, unread_count) VALUES (?, ?, ?, ?)",
            )
            .bind(uid)
            .bind(session_id)
            .bind(session_type)
            .bind(unread_count)
            .execute(&live)
            .await
            .unwrap();
        }
        sqlx::query("ALTER TABLE message_live_state ADD COLUMN summary TEXT DEFAULT ''")
            .execute(&live)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE message_live_state SET summary = '快给ta一个评价吧～' WHERE session_id = 'chat-tip-1'",
        )
        .execute(&live)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE message_live_state SET summary = '你已被禁言，点此查看详情' WHERE session_id = 'chat-tip-2'",
        )
        .execute(&live)
        .await
        .unwrap();

        clear_platform_message_unread_for_owner(&accounts, &live, "owner-a")
            .await
            .unwrap();

        let rows = sqlx::query(
            "SELECT uid, session_id, unread_count FROM message_live_state ORDER BY uid, session_id",
        )
        .fetch_all(&live)
        .await
        .unwrap();
        let unread = rows
            .into_iter()
            .map(|row| {
                (
                    row.get::<String, _>("uid"),
                    row.get::<String, _>("session_id"),
                    row.get::<i64, _>("unread_count"),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            unread,
            vec![
                ("u1".to_string(), "chat-1".to_string(), 5),
                ("u1".to_string(), "chat-tip-1".to_string(), 0),
                ("u1".to_string(), "fav-1".to_string(), 0),
                ("u1".to_string(), "sys-1".to_string(), 0),
                ("u1".to_string(), "wb-1".to_string(), 0),
                ("u2".to_string(), "chat-tip-2".to_string(), 8),
                ("u2".to_string(), "sys-2".to_string(), 9),
            ]
        );
    }
}
