use std::{
    env,
    sync::{Arc, Mutex},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Map, Value};
use tempfile::TempDir;
use tokio::sync::mpsc;
use xianyu_rust_backend_full::{
    config::AppConfig,
    notifications::{
        feishu_events::{handle_feishu_im_event, FeishuImEvent},
        feishu_native::{
            FeishuNativeCallFuture, FeishuNativeError, FeishuNativeRequest, FeishuNativeResponse,
            FeishuNativeRuntime, FeishuNativeTransport,
        },
    },
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn help_message_replies_through_native_and_records_safe_runtime_metadata() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-help".to_string(),
            chat_id: "oc_chat".to_string(),
            message_id: "om_help".to_string(),
            thread_id: "omt_thread".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_sender".to_string(),
            sender_name: "Alice".to_string(),
            text: "/帮助".to_string(),
            mentioned_bot: false,
            mention_all: false,
            is_from_bot: false,
            action_form_values: Map::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "replied");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "message.send");
    assert_eq!(calls[0].params["channel_id"], channel_id);
    assert_eq!(calls[0].params["receive_id"], "oc_chat");
    assert_eq!(calls[0].params["receive_id_type"], "chat_id");
    assert_eq!(calls[0].params["reply_to_message_id"], "om_help");
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("加入"));

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(
        runtime
            .runtime_json
            .get("last_feishu_event_id")
            .and_then(Value::as_str),
        Some("evt-help")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("last_feishu_message_id")
            .and_then(Value::as_str),
        Some("om_help")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("last_chat_id")
            .and_then(Value::as_str),
        Some("oc_chat")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("last_sender_id")
            .and_then(Value::as_str),
        Some("ou_sender")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("last_payload")
            .and_then(Value::as_object),
        None
    );

    clear_env();
}

#[tokio::test]
async fn duplicate_feishu_event_id_is_ignored_before_reply_side_effects() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let event = FeishuImEvent {
        channel_id,
        event_id: "evt-help-dupe".to_string(),
        chat_id: "oc_chat_dupe".to_string(),
        message_id: "om_help_dupe".to_string(),
        chat_type: "p2p".to_string(),
        sender_id: "ou_sender_dupe".to_string(),
        sender_name: "重复用户".to_string(),
        text: "/帮助".to_string(),
        ..FeishuImEvent::default()
    };

    let first = handle_feishu_im_event(&state, event.clone()).await.unwrap();
    let second = handle_feishu_im_event(&state, event).await.unwrap();

    assert_eq!(first.status, "replied");
    assert_eq!(second.status, "duplicate_ignored");
    assert_eq!(transport.calls().len(), 1);

    clear_env();
}

#[tokio::test]
async fn duplicate_feishu_message_id_is_ignored_when_event_id_is_missing() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let event = FeishuImEvent {
        channel_id,
        event_id: "".to_string(),
        chat_id: "oc_chat_message_dupe".to_string(),
        message_id: "om_message_dupe".to_string(),
        chat_type: "p2p".to_string(),
        sender_id: "ou_sender_message_dupe".to_string(),
        sender_name: "消息重复用户".to_string(),
        text: "/帮助".to_string(),
        ..FeishuImEvent::default()
    };

    let first = handle_feishu_im_event(&state, event.clone()).await.unwrap();
    let second = handle_feishu_im_event(&state, event).await.unwrap();

    assert_eq!(first.status, "replied");
    assert_eq!(second.status, "duplicate_ignored");
    assert_eq!(transport.calls().len(), 1);

    clear_env();
}

#[tokio::test]
async fn feishu_event_receipt_retries_processing_errors_but_not_reply_errors() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_feishu_channel(&state).await;

    assert!(state
        .storage()
        .try_claim_feishu_event(
            channel_id,
            "evt-receipt-policy",
            "om-receipt-policy",
            "oc-receipt-policy",
            "ou-receipt-policy",
        )
        .await
        .unwrap());
    assert!(!state
        .storage()
        .try_claim_feishu_event(
            channel_id,
            "evt-receipt-policy",
            "om-receipt-policy-2",
            "oc-receipt-policy",
            "ou-receipt-policy",
        )
        .await
        .unwrap());

    state
        .storage()
        .finish_feishu_event(
            channel_id,
            "evt-receipt-policy",
            "om-receipt-policy-2",
            "processing_error",
        )
        .await
        .unwrap();
    assert!(state
        .storage()
        .try_claim_feishu_event(
            channel_id,
            "evt-receipt-policy",
            "om-receipt-policy-retry",
            "oc-receipt-policy",
            "ou-receipt-policy",
        )
        .await
        .unwrap());

    state
        .storage()
        .finish_feishu_event(
            channel_id,
            "evt-receipt-policy",
            "om-receipt-policy-retry",
            "reply_error",
        )
        .await
        .unwrap();
    assert!(!state
        .storage()
        .try_claim_feishu_event(
            channel_id,
            "evt-receipt-policy",
            "om-receipt-policy-reply-error-retry",
            "oc-receipt-policy",
            "ou-receipt-policy",
        )
        .await
        .unwrap());

    clear_env();
}

#[tokio::test]
async fn bind_command_persists_sender_owner_binding_and_replies_with_owner_scope() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-bind".to_string(),
            chat_id: "oc_bind_chat".to_string(),
            message_id: "om_bind".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_bind_sender".to_string(),
            sender_name: "绑定用户".to_string(),
            text: "绑定".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "bound");
    let binding = state
        .storage()
        .get_feishu_user_binding(channel_id, "ou_bind_sender")
        .await
        .unwrap()
        .expect("binding should be persisted");
    assert_eq!(binding.channel_id, channel_id);
    assert_eq!(binding.owner_username, "owner-a");
    assert_eq!(binding.sender_id, "ou_bind_sender");
    assert_eq!(binding.sender_name, "绑定用户");
    assert_eq!(binding.chat_id, "oc_bind_chat");
    assert!(binding.enabled);

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    let reply_text = calls[0].params["content"]["text"].as_str().unwrap();
    assert!(reply_text.contains("已加入 owner"));
    assert!(reply_text.contains("owner-a"));
    assert!(!reply_text.contains("尚未加入 owner"));

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .runtime_json
            .get("last_feishu_bound_owner")
            .and_then(Value::as_str),
        Some("owner-a")
    );

    clear_env();
}

#[tokio::test]
async fn allowed_qr_operator_auto_binds_on_first_menu_message() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-auto-bind-menu".to_string(),
            chat_id: "oc_auto_bind_chat".to_string(),
            message_id: "om_auto_bind_menu".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_bind_sender".to_string(),
            sender_name: "扫码用户".to_string(),
            text: "菜单".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_replied");
    let binding = state
        .storage()
        .get_feishu_user_binding(channel_id, "ou_bind_sender")
        .await
        .unwrap()
        .expect("allowed QR operator should be bound on first message");
    assert_eq!(binding.owner_username, "owner-a");
    assert_eq!(binding.role, "primary");
    assert_eq!(binding.joined_via, "allowlist_auto_bind");
    assert_eq!(binding.chat_id, "oc_auto_bind_chat");
    assert_eq!(binding.last_message_id, "om_auto_bind_menu");

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params["msg_type"], "interactive");
    let card = &calls[0].params["content"];
    assert!(card.to_string().contains("鱼管家飞书菜单"));
    assert!(card.to_string().contains("商品列表"));
    assert!(card.to_string().contains("订单列表"));
    assert!(!card.to_string().contains("尚未绑定"));

    clear_env();
}

#[tokio::test]
async fn collaborator_cannot_generate_invite_code_but_primary_can() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            "owner-a",
            "ou_primary_sender",
            "主用户",
            "oc_primary_chat",
            "p2p",
            "om_primary_seed",
            "evt_primary_seed",
            "primary",
            "qr_owner",
            None,
            None,
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            "owner-a",
            "ou_collaborator_sender",
            "协作者",
            "oc_collaborator_chat",
            "p2p",
            "om_collab_seed",
            "evt_collab_seed",
            "collaborator",
            "invite_code",
            Some("ou_primary_sender"),
            Some("主用户"),
            true,
        )
        .await
        .unwrap();

    let primary_outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-generate-invite-primary".to_string(),
            chat_id: "oc_primary_chat".to_string(),
            message_id: "om_generate_invite_primary".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_primary_sender".to_string(),
            sender_name: "主用户".to_string(),
            text: "生成邀请码".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(primary_outcome.status, "invite_generated");

    let collaborator_outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-generate-invite-collab".to_string(),
            chat_id: "oc_collaborator_chat".to_string(),
            message_id: "om_generate_invite_collab".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_collaborator_sender".to_string(),
            sender_name: "协作者".to_string(),
            text: "生成邀请码".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(collaborator_outcome.status, "permission_denied");

    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    let invite_body = calls[0].params["content"].to_string();
    assert_eq!(calls[0].params["msg_type"], "interactive");
    assert!(invite_body.contains("邀请码"));
    assert!(invite_body.contains("把这条消息直接转发给协作者"));
    assert!(invite_body.contains("点开下面链接进入机器人会话"));
    assert!(invite_body.contains("https://applink.feishu.cn/client/bot/open?appId="));
    assert!(invite_body.contains("发送「加入 "));
    assert!(calls[1].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("只有主用户"));

    clear_env();
}

#[tokio::test]
async fn joining_collaboration_syncs_existing_active_group_chats() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-collab-group", "owner-a").await;
    state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            "owner-a",
            "ou_primary_sender",
            "主用户",
            "oc_primary_chat",
            "p2p",
            "om_primary_seed",
            "evt_primary_seed",
            "primary",
            "qr_owner",
            None,
            None,
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            "owner-a",
            "seller-collab-group",
            "session-active",
            "buyer-active",
            "现有客户",
            "item-active",
            "现有商品",
            "https://img.example/item-active.png",
            "99.00",
            "oc_group_active",
            "现有客户｜现有商品",
            "ou_primary_sender",
            "active",
            1_719_225_600.0,
        )
        .await
        .unwrap();
    state
        .storage()
        .create_feishu_collaborator_invite(
            channel_id,
            "owner-a",
            "ABC123",
            "ou_primary_sender",
            "主用户",
            1800.0,
        )
        .await
        .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-join-collab".to_string(),
            chat_id: "oc_join_chat".to_string(),
            message_id: "om_join_collab".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_new_collaborator".to_string(),
            sender_name: "新客服".to_string(),
            text: "加入 ABC123".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "joined_collaboration");
    let calls = transport.calls();
    assert_eq!(calls[0].method, "chat.members.add");
    assert_eq!(calls[0].params["chat_id"], "oc_group_active");
    assert_eq!(
        calls[0].params["member_ids"],
        json!(["ou_new_collaborator"])
    );
    assert_eq!(calls[1].method, "message.send");
    assert!(calls[1].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已加入 owner"));

    clear_env();
}

#[tokio::test]
async fn bind_command_rejects_ownerless_feishu_channel_without_persisting_binding() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_ownerless_feishu_channel(&state).await;

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-ownerless-bind".to_string(),
            chat_id: "oc_ownerless".to_string(),
            message_id: "om_ownerless".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_ownerless_sender".to_string(),
            text: "绑定".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "bind_rejected");
    assert!(state
        .storage()
        .get_feishu_user_binding(channel_id, "ou_ownerless_sender")
        .await
        .unwrap()
        .is_none());

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    let reply_text = calls[0].params["content"]["text"].as_str().unwrap();
    assert!(reply_text.contains("无法加入"));

    clear_env();
}

#[tokio::test]
async fn bind_command_rejects_sender_not_in_channel_allowlist() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-bind-unauthorized".to_string(),
            chat_id: "oc_bind_chat".to_string(),
            message_id: "om_bind_unauthorized".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_intruder".to_string(),
            sender_name: "未授权用户".to_string(),
            text: "绑定".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "bind_rejected");
    assert!(state
        .storage()
        .get_feishu_user_binding(channel_id, "ou_intruder")
        .await
        .unwrap()
        .is_none());
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("授权名单"));

    clear_env();
}

#[tokio::test]
async fn unbind_command_deletes_only_current_feishu_sender_binding() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_unbind_sender",
            "解绑用户",
            "oc_unbind_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_other_sender",
            "其他用户",
            "oc_unbind_chat",
            "p2p",
            "om_other",
            "evt_other",
            true,
        )
        .await
        .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-unbind".to_string(),
            chat_id: "oc_unbind_chat".to_string(),
            message_id: "om_unbind".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_unbind_sender".to_string(),
            text: "解绑".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "unbound");
    assert!(state
        .storage()
        .get_feishu_user_binding(channel_id, "ou_unbind_sender")
        .await
        .unwrap()
        .is_none());
    assert!(state
        .storage()
        .get_feishu_user_binding(channel_id, "ou_other_sender")
        .await
        .unwrap()
        .is_some());

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已解绑"));

    clear_env();
}

#[tokio::test]
async fn bound_sender_quote_reply_routes_to_mapped_xianyu_session() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-route", "owner-a").await;
    state
        .storage()
        .upsert_message_live_state(
            "seller-route",
            "session-route",
            1,
            "buyer-route",
            "路由买家",
            "item-route",
            "路由商品",
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE message_live_state SET latest_version = 1, last_message_ts = 1 WHERE uid = ? AND session_id = ?",
    )
    .bind("seller-route")
    .bind("session-route")
    .execute(state.storage().live_state())
    .await
    .unwrap();
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_reply_sender",
            "回复用户",
            "oc_route_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "om_route",
            "oc_route_chat",
            "seller-route",
            "session-route",
            "buyer-route",
            "路由买家",
            "item-route",
            "路由商品",
            "msg-route",
            "pnm-route",
        )
        .await
        .unwrap();

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-route".to_string(), live_tx)
        .await;
    let waiters = state.live_response_waiters().clone();
    let ack_task = tokio::spawn(async move {
        let payload = live_rx.recv().await.expect("xianyu ws payload");
        let frame: Value = serde_json::from_str(&payload).expect("valid ws payload json");
        let mid = frame["headers"]["mid"].as_str().unwrap().to_string();
        let encoded = frame["body"][0]["content"]["custom"]["data"]
            .as_str()
            .unwrap()
            .to_string();
        let decoded = BASE64_STANDARD.decode(encoded).unwrap();
        let decoded: Value = serde_json::from_slice(&decoded).unwrap();
        let waiter = waiters
            .take("seller-route", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-001"
                }
            }))
            .unwrap();
        (payload, decoded)
    });

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-quote-reply".to_string(),
            chat_id: "oc_route_chat".to_string(),
            message_id: "om_feishu_reply".to_string(),
            thread_id: "om_route".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_reply_sender".to_string(),
            sender_name: "回复用户".to_string(),
            text: "这边马上处理".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "xianyu_replied");
    let (payload, decoded) = ack_task.await.unwrap();
    assert!(payload.contains("session-route@goofish"));
    assert!(payload.contains("buyer-route@goofish"));
    assert_eq!(decoded["text"]["text"], "这边马上处理");

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已回复"));

    clear_env();
}

#[tokio::test]
async fn bound_sender_interaction_updates_operator_activity_for_group_routing() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-operator-activity", "owner-a").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_operator_activity",
            "值班 Alice",
            "oc_operator_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-operator-activity".to_string(),
            chat_id: "oc_operator_chat".to_string(),
            message_id: "om_operator_activity".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_operator_activity".to_string(),
            sender_name: "值班 Alice".to_string(),
            text: "帮助".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "bound_reply");
    let activity = state
        .storage()
        .get_latest_feishu_operator_activity(channel_id, "owner-a")
        .await
        .unwrap()
        .expect("bound sender should be recorded as latest operator");
    assert_eq!(activity.sender_id, "ou_operator_activity");
    assert_eq!(activity.sender_name, "值班 Alice");
    assert_eq!(activity.last_chat_id, "oc_operator_chat");
    assert_eq!(activity.last_event_id, "evt-operator-activity");

    clear_env();
}

#[tokio::test]
async fn bound_sender_recent_reply_uses_latest_chat_route() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-recent", "owner-a").await;
    state
        .storage()
        .upsert_message_live_state(
            "seller-recent",
            "session-recent",
            1,
            "buyer-recent",
            "最近买家",
            "item-recent",
            "最近商品",
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_recent_sender",
            "最近回复用户",
            "oc_recent_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "om_recent_route",
            "oc_recent_chat",
            "seller-recent",
            "session-recent",
            "buyer-recent",
            "最近买家",
            "item-recent",
            "最近商品",
            "msg-recent",
            "pnm-recent",
        )
        .await
        .unwrap();

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-recent".to_string(), live_tx)
        .await;
    let waiters = state.live_response_waiters().clone();
    let ack_task = tokio::spawn(async move {
        let payload = live_rx.recv().await.expect("xianyu ws payload");
        let frame: Value = serde_json::from_str(&payload).expect("valid ws payload json");
        let mid = frame["headers"]["mid"].as_str().unwrap().to_string();
        let encoded = frame["body"][0]["content"]["custom"]["data"]
            .as_str()
            .unwrap()
            .to_string();
        let decoded = BASE64_STANDARD.decode(encoded).unwrap();
        let decoded: Value = serde_json::from_slice(&decoded).unwrap();
        let waiter = waiters
            .take("seller-recent", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-recent"
                }
            }))
            .unwrap();
        decoded
    });

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-recent-reply".to_string(),
            chat_id: "oc_recent_chat".to_string(),
            message_id: "om_recent_reply".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_recent_sender".to_string(),
            sender_name: "最近回复用户".to_string(),
            text: "回复 这条走最近会话".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "xianyu_replied");
    let decoded = ack_task.await.unwrap();
    assert_eq!(decoded["text"]["text"], "这条走最近会话");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已回复"));

    clear_env();
}

#[tokio::test]
async fn bound_sender_can_select_aggregate_card_row_before_recent_reply() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-select", "owner-a").await;
    for (session_id, peer_uid, peer_nick, item_id, item_title) in [
        (
            "session-old",
            "buyer-old",
            "旧会话买家",
            "item-old",
            "旧会话商品",
        ),
        (
            "session-new",
            "buyer-new",
            "新会话买家",
            "item-new",
            "新会话商品",
        ),
    ] {
        state
            .storage()
            .upsert_message_live_state(
                "seller-select",
                session_id,
                1,
                peer_uid,
                peer_nick,
                item_id,
                item_title,
            )
            .await
            .unwrap();
    }
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_select_sender",
            "选择回复用户",
            "oc_select_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "agg-route-old",
            "oc_select_chat",
            "seller-select",
            "session-old",
            "buyer-old",
            "旧会话买家",
            "item-old",
            "旧会话商品",
            "msg-old",
            "pnm-old",
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "agg-route-new",
            "oc_select_chat",
            "seller-select",
            "session-new",
            "buyer-new",
            "新会话买家",
            "item-new",
            "新会话商品",
            "msg-new",
            "pnm-new",
        )
        .await
        .unwrap();

    let selected = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-select-old".to_string(),
            chat_id: "oc_select_chat".to_string(),
            message_id: "om_select_card".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_select_sender".to_string(),
            sender_name: "选择回复用户".to_string(),
            text: "选择回复 agg-route-old".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(selected.status, "reply_route_selected");
    let selected_route = state
        .storage()
        .get_latest_feishu_im_reply_route_for_chat(channel_id, "oc_select_chat")
        .await
        .unwrap()
        .expect("selected route should become latest route");
    assert_eq!(selected_route.session_id, "session-old");

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-select".to_string(), live_tx)
        .await;
    let waiters = state.live_response_waiters().clone();
    let ack_task = tokio::spawn(async move {
        let payload = live_rx.recv().await.expect("xianyu ws payload");
        assert!(payload.contains("session-old@goofish"), "{payload}");
        assert!(payload.contains("buyer-old@goofish"), "{payload}");
        let frame: Value = serde_json::from_str(&payload).expect("valid ws payload json");
        let mid = frame["headers"]["mid"].as_str().unwrap().to_string();
        let encoded = frame["body"][0]["content"]["custom"]["data"]
            .as_str()
            .unwrap()
            .to_string();
        let decoded = BASE64_STANDARD.decode(encoded).unwrap();
        let decoded: Value = serde_json::from_slice(&decoded).unwrap();
        let waiter = waiters
            .take("seller-select", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-selected"
                }
            }))
            .unwrap();
        decoded
    });

    let replied = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-selected-reply".to_string(),
            chat_id: "oc_select_chat".to_string(),
            message_id: "om_selected_reply".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_select_sender".to_string(),
            sender_name: "选择回复用户".to_string(),
            text: "回复 这条发给旧会话".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(replied.status, "xianyu_replied");
    let decoded = ack_task.await.unwrap();
    assert_eq!(decoded["text"]["text"], "这条发给旧会话");
    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已选择"));
    assert!(calls[1].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已回复"));

    clear_env();
}

#[tokio::test]
async fn bound_sender_open_chat_card_selects_route_and_allows_plain_followup_reply() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-open-chat", "owner-a").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_open_chat_sender",
            "打开会话用户",
            "oc_open_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "agg-route-open",
            "oc_open_chat",
            "seller-open-chat",
            "session-open-chat",
            "buyer-open-chat",
            "会话买家",
            "item-open-chat",
            "会话商品",
            "msg-open-chat",
            "pnm-open-chat",
        )
        .await
        .unwrap();
    for (message_id, version, ts, text, is_self, sender_nick) in [
        (
            "history-1",
            100_i64,
            1_718_000_100_000_i64,
            "第一句咨询",
            0_i64,
            "会话买家",
        ),
        (
            "history-2",
            101_i64,
            1_718_000_110_000_i64,
            "这边在的",
            1_i64,
            "账号 seller-open-chat",
        ),
        (
            "history-3",
            102_i64,
            1_718_000_120_000_i64,
            "还想再看看",
            0_i64,
            "会话买家",
        ),
    ] {
        seed_history_message(
            &state,
            "seller-open-chat",
            "session-open-chat",
            message_id,
            version,
            ts,
            text,
            is_self,
            sender_nick,
        )
        .await;
    }

    let opened = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-open-chat".to_string(),
            chat_id: "oc_open_chat".to_string(),
            message_id: "om_open_chat_cmd".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_open_chat_sender".to_string(),
            sender_name: "打开会话用户".to_string(),
            text: "打开会话 agg-route-open".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(opened.status, "reply_route_opened");
    let selected_route = state
        .storage()
        .get_latest_feishu_im_reply_route_for_chat(channel_id, "oc_open_chat")
        .await
        .unwrap()
        .expect("open chat route should become latest route");
    assert_eq!(selected_route.session_id, "session-open-chat");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params["msg_type"], "interactive");
    let card_text = calls[0].params["content"].to_string();
    assert!(card_text.contains("客户会话"), "{card_text}");
    assert!(card_text.contains("第一句咨询"), "{card_text}");
    assert!(card_text.contains("这边在的"), "{card_text}");
    assert!(card_text.contains("还想再看看"), "{card_text}");
    assert!(card_text.contains("商品副卡"), "{card_text}");
    assert!(card_text.contains("当前会话已设为回复对象"), "{card_text}");

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-open-chat".to_string(), live_tx)
        .await;
    let waiters = state.live_response_waiters().clone();
    let ack_task = tokio::spawn(async move {
        let payload = live_rx.recv().await.expect("xianyu ws payload");
        assert!(payload.contains("session-open-chat@goofish"), "{payload}");
        assert!(payload.contains("buyer-open-chat@goofish"), "{payload}");
        let frame: Value = serde_json::from_str(&payload).expect("valid ws payload json");
        let mid = frame["headers"]["mid"].as_str().unwrap().to_string();
        let encoded = frame["body"][0]["content"]["custom"]["data"]
            .as_str()
            .unwrap()
            .to_string();
        let decoded = BASE64_STANDARD.decode(encoded).unwrap();
        let decoded: Value = serde_json::from_slice(&decoded).unwrap();
        let waiter = waiters
            .take("seller-open-chat", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-open-chat"
                }
            }))
            .unwrap();
        decoded
    });

    let replied = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-open-chat-reply".to_string(),
            chat_id: "oc_open_chat".to_string(),
            message_id: "om_open_chat_reply".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_open_chat_sender".to_string(),
            sender_name: "打开会话用户".to_string(),
            text: "可以，今晚发货".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(replied.status, "xianyu_replied");
    let decoded = ack_task.await.unwrap();
    assert_eq!(decoded["text"]["text"], "可以，今晚发货");
    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已回复"));

    clear_env();
}

#[tokio::test]
async fn bound_sender_recent_messages_shows_latest_chat_route_context() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-recent-menu", "owner-a").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_recent_menu_sender",
            "最近消息用户",
            "oc_recent_menu_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "om_recent_menu_route",
            "oc_recent_menu_chat",
            "seller-recent-menu",
            "session-recent-menu",
            "buyer-recent-menu",
            "最近菜单买家",
            "item-recent-menu",
            "最近菜单商品",
            "msg-recent-menu",
            "pnm-recent-menu",
        )
        .await
        .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-recent-menu".to_string(),
            chat_id: "oc_recent_menu_chat".to_string(),
            message_id: "om_recent_menu".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_recent_menu_sender".to_string(),
            sender_name: "最近消息用户".to_string(),
            text: "最近消息".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "recent_message_replied");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].params["msg_type"], "interactive");
    let reply_text = calls[0].params["content"].to_string();
    assert!(reply_text.contains("session-recent-menu"), "{reply_text}");
    assert!(reply_text.contains("最近菜单买家"), "{reply_text}");
    assert!(reply_text.contains("最近菜单商品"), "{reply_text}");
    assert!(reply_text.contains("回复 <内容>"), "{reply_text}");

    clear_env();
}

#[tokio::test]
async fn bound_sender_quote_reply_rejects_cross_owner_route_without_xianyu_send() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-foreign-route", "owner-b").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_cross_owner_sender",
            "跨 owner 用户",
            "oc_cross_owner_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-b",
            "om_foreign_route",
            "oc_cross_owner_chat",
            "seller-foreign-route",
            "session-foreign",
            "buyer-foreign",
            "外部买家",
            "item-foreign",
            "外部商品",
            "msg-foreign",
            "pnm-foreign",
        )
        .await
        .unwrap();

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-foreign-route".to_string(), live_tx)
        .await;

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-cross-owner-reply".to_string(),
            chat_id: "oc_cross_owner_chat".to_string(),
            message_id: "om_cross_owner_reply".to_string(),
            thread_id: "om_foreign_route".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_cross_owner_sender".to_string(),
            sender_name: "跨 owner 用户".to_string(),
            text: "不应发到外部 owner".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "xianyu_reply_rejected");
    assert!(live_rx.try_recv().is_err());
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("不属于当前绑定 owner"));

    clear_env();
}

#[tokio::test]
async fn expired_feishu_reply_route_is_ignored_for_bound_sender_reply() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-expired-route", "owner-a").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_expired_sender",
            "过期路由用户",
            "oc_expired_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_im_reply_route(
            channel_id,
            "owner-a",
            "om_expired_route",
            "oc_expired_chat",
            "seller-expired-route",
            "session-expired",
            "buyer-expired",
            "过期买家",
            "item-expired",
            "过期商品",
            "msg-expired",
            "pnm-expired",
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE feishu_im_reply_routes SET expires_at = 1 WHERE channel_id = ? AND feishu_message_id = ?",
    )
    .bind(channel_id)
    .bind("om_expired_route")
    .execute(state.storage().notifications())
    .await
    .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-expired-reply".to_string(),
            chat_id: "oc_expired_chat".to_string(),
            message_id: "om_expired_reply".to_string(),
            thread_id: "om_expired_route".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_expired_sender".to_string(),
            sender_name: "过期路由用户".to_string(),
            text: "不会发出去".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "bound_reply");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("引用鱼管家推送的客户消息"));

    clear_env();
}

#[tokio::test]
async fn bound_sender_can_read_owner_scoped_item_and_order_lists() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-business", "owner-a").await;
    seed_account(&state, "seller-foreign", "owner-b").await;
    seed_item(&state, "seller-business", "item-visible", "可见商品").await;
    seed_item(&state, "seller-foreign", "item-hidden", "不可见商品").await;
    seed_order(&state, "seller-business", "tid-visible", "可见订单").await;
    seed_order(&state, "seller-foreign", "tid-hidden", "不可见订单").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_business_sender",
            "业务用户",
            "oc_business_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    for (text, visible, hidden) in [
        ("商品列表", "可见商品", "不可见商品"),
        ("订单列表", "可见订单", "不可见订单"),
    ] {
        let outcome = handle_feishu_im_event(
            &state,
            FeishuImEvent {
                channel_id,
                event_id: format!("evt-{text}"),
                chat_id: "oc_business_chat".to_string(),
                message_id: format!("om-{text}"),
                chat_type: "p2p".to_string(),
                sender_id: "ou_business_sender".to_string(),
                sender_name: "业务用户".to_string(),
                text: text.to_string(),
                ..FeishuImEvent::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.status, "business_replied");
        let calls = transport.calls();
        assert_eq!(calls.last().unwrap().params["msg_type"], "interactive");
        let reply_text = calls.last().unwrap().params["content"].to_string();
        assert!(reply_text.contains(visible), "{reply_text}");
        assert!(!reply_text.contains(hidden), "{reply_text}");
    }

    clear_env();
}

#[tokio::test]
async fn bound_sender_item_and_order_lists_are_sent_as_action_cards() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-card", "owner-a").await;
    seed_item(&state, "seller-card", "item-card", "卡片商品").await;
    seed_order(&state, "seller-card", "tid-card", "卡片订单").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_card_sender",
            "卡片用户",
            "oc_card_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    for (text, expected) in [("商品列表", "卡片商品"), ("订单列表", "卡片订单")] {
        let outcome = handle_feishu_im_event(
            &state,
            FeishuImEvent {
                channel_id,
                event_id: format!("evt-card-{text}"),
                chat_id: "oc_card_chat".to_string(),
                message_id: format!("om-card-{text}"),
                chat_type: "p2p".to_string(),
                sender_id: "ou_card_sender".to_string(),
                sender_name: "卡片用户".to_string(),
                text: text.to_string(),
                ..FeishuImEvent::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.status, "business_replied");

        let calls = transport.calls();
        let params = &calls.last().unwrap().params;
        assert_eq!(params["msg_type"], "interactive");
        let card_text = params["content"].to_string();
        assert!(card_text.contains(expected), "{card_text}");
        assert!(card_text.contains("command"), "{card_text}");
        assert!(card_text.contains("最近消息"), "{card_text}");
        if text == "商品列表" {
            assert!(
                card_text.contains("商品详情 seller-card item-card"),
                "{card_text}"
            );
            assert!(
                card_text.contains("下架商品 seller-card item-card"),
                "{card_text}"
            );
        } else {
            assert!(
                card_text.contains("订单详情 seller-card tid-card"),
                "{card_text}"
            );
            assert!(
                card_text.contains("无需快递发货 seller-card tid-card"),
                "{card_text}"
            );
        }
    }

    clear_env();
}

#[tokio::test]
async fn bound_sender_can_use_menu_and_read_owner_scoped_item_order_details() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-detail", "owner-a").await;
    seed_account(&state, "seller-detail-foreign", "owner-b").await;
    seed_item(&state, "seller-detail", "item-detail", "可见详情商品").await;
    seed_item(
        &state,
        "seller-detail-foreign",
        "item-hidden",
        "不可见详情商品",
    )
    .await;
    seed_order(&state, "seller-detail", "tid-detail", "可见详情订单").await;
    seed_order(
        &state,
        "seller-detail-foreign",
        "tid-hidden",
        "不可见详情订单",
    )
    .await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_detail_sender",
            "详情用户",
            "oc_detail_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    for (text, expected, hidden) in [
        ("菜单", "商品详情 <uid> <item_id>", "不可见详情商品"),
        (
            "商品详情 seller-detail item-detail",
            "可见详情商品",
            "不可见详情商品",
        ),
        (
            "订单详情 seller-detail tid-detail",
            "可见详情订单",
            "不可见详情订单",
        ),
    ] {
        let outcome = handle_feishu_im_event(
            &state,
            FeishuImEvent {
                channel_id,
                event_id: format!("evt-detail-{text}"),
                chat_id: "oc_detail_chat".to_string(),
                message_id: format!("om-detail-{text}"),
                chat_type: "p2p".to_string(),
                sender_id: "ou_detail_sender".to_string(),
                sender_name: "详情用户".to_string(),
                text: text.to_string(),
                ..FeishuImEvent::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.status, "business_replied");
        let calls = transport.calls();
        let params = &calls.last().unwrap().params;
        let reply_text = params["content"].to_string();
        assert!(reply_text.contains(expected), "{reply_text}");
        assert!(!reply_text.contains(hidden), "{reply_text}");
    }

    clear_env();
}

#[tokio::test]
async fn bound_sender_item_downshelf_requires_nonce_confirmation_before_action() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    enable_local_business_action_fallback(&state, channel_id).await;
    seed_account(&state, "seller-action", "owner-a").await;
    seed_item(&state, "seller-action", "item-action", "待下架商品").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_action_sender",
            "动作用户",
            "oc_action_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-downshelf-request".to_string(),
            chat_id: "oc_action_chat".to_string(),
            message_id: "om_downshelf_request".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_action_sender".to_string(),
            sender_name: "动作用户".to_string(),
            text: "下架商品 seller-action item-action".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_action_pending");
    let item_state = item_status(&state, "seller-action", "item-action").await;
    assert_eq!(item_state, ("ON_SALE".to_string(), true));
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    let first_reply = calls[0].params["content"]["text"].as_str().unwrap();
    assert!(first_reply.contains("确认"));
    assert!(first_reply.contains("下架商品"));
    let nonce = extract_nonce(first_reply);

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-downshelf-confirm".to_string(),
            chat_id: "oc_action_chat".to_string(),
            message_id: "om_downshelf_confirm".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_action_sender".to_string(),
            sender_name: "动作用户".to_string(),
            text: format!("确认 {nonce}"),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_action_executed");
    let item_state = item_status(&state, "seller-action", "item-action").await;
    assert_eq!(item_state, ("SOLD_OUT".to_string(), false));
    let calls = transport.calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已执行"));

    clear_env();
}

#[tokio::test]
async fn duplicate_feishu_confirm_event_does_not_replay_business_action_or_reply() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    enable_local_business_action_fallback(&state, channel_id).await;
    seed_account(&state, "seller-dupe-action", "owner-a").await;
    seed_item(
        &state,
        "seller-dupe-action",
        "item-dupe-action",
        "重复确认商品",
    )
    .await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_dupe_action_sender",
            "重复确认用户",
            "oc_dupe_action_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    let request = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-dupe-action-request".to_string(),
            chat_id: "oc_dupe_action_chat".to_string(),
            message_id: "om_dupe_action_request".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_dupe_action_sender".to_string(),
            sender_name: "重复确认用户".to_string(),
            text: "下架商品 seller-dupe-action item-dupe-action".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(request.status, "business_action_pending");
    let nonce = extract_nonce(
        transport.calls()[0].params["content"]["text"]
            .as_str()
            .unwrap(),
    );

    let confirm_event = FeishuImEvent {
        channel_id,
        event_id: "evt-dupe-action-confirm".to_string(),
        chat_id: "oc_dupe_action_chat".to_string(),
        message_id: "om_dupe_action_confirm".to_string(),
        chat_type: "p2p".to_string(),
        sender_id: "ou_dupe_action_sender".to_string(),
        sender_name: "重复确认用户".to_string(),
        text: format!("确认 {nonce}"),
        ..FeishuImEvent::default()
    };
    let first_confirm = handle_feishu_im_event(&state, confirm_event.clone())
        .await
        .unwrap();
    let second_confirm = handle_feishu_im_event(&state, confirm_event).await.unwrap();

    assert_eq!(first_confirm.status, "business_action_executed");
    assert_eq!(second_confirm.status, "duplicate_ignored");
    assert_eq!(
        item_status(&state, "seller-dupe-action", "item-dupe-action").await,
        ("SOLD_OUT".to_string(), false)
    );
    assert_eq!(transport.calls().len(), 2);

    clear_env();
}

#[tokio::test]
async fn bound_sender_order_dummy_consign_requires_nonce_confirmation_before_action() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    enable_local_business_action_fallback(&state, channel_id).await;
    seed_account(&state, "seller-order-action", "owner-a").await;
    seed_order(&state, "seller-order-action", "tid-action", "待发货订单").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_order_action_sender",
            "订单动作用户",
            "oc_order_action_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-dummy-consign-request".to_string(),
            chat_id: "oc_order_action_chat".to_string(),
            message_id: "om_dummy_consign_request".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_order_action_sender".to_string(),
            sender_name: "订单动作用户".to_string(),
            text: "无需快递发货 seller-order-action tid-action".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_action_pending");
    assert_eq!(
        order_status_text(&state, "seller-order-action", "tid-action").await,
        ("WAIT_SELLER_SEND_GOODS".to_string(), "待发货".to_string())
    );
    let nonce = extract_nonce(
        transport.calls()[0].params["content"]["text"]
            .as_str()
            .unwrap(),
    );

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-dummy-consign-confirm".to_string(),
            chat_id: "oc_order_action_chat".to_string(),
            message_id: "om_dummy_consign_confirm".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_order_action_sender".to_string(),
            sender_name: "订单动作用户".to_string(),
            text: format!("确认 {nonce}"),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_action_executed");
    assert_eq!(
        order_status_text(&state, "seller-order-action", "tid-action").await,
        ("SELLER_CONSIGNED".to_string(), "无需快递".to_string())
    );

    clear_env();
}

#[tokio::test]
async fn bound_sender_remaining_business_mutations_require_nonce_confirmation() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    enable_local_business_action_fallback(&state, channel_id).await;
    seed_account(&state, "seller-more-actions", "owner-a").await;
    seed_item(&state, "seller-more-actions", "item-price", "待降价商品").await;
    seed_order(&state, "seller-more-actions", "tid-price", "待改价订单").await;
    seed_order(&state, "seller-more-actions", "tid-ship", "待快递发货订单").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_more_action_sender",
            "更多动作用户",
            "oc_more_action_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    request_and_confirm_business_action(
        &state,
        &transport,
        channel_id,
        "ou_more_action_sender",
        "oc_more_action_chat",
        "降价 seller-more-actions item-price 8.00",
        "evt-reduce-price",
        "om-reduce-price",
    )
    .await;
    assert_eq!(
        item_price(&state, "seller-more-actions", "item-price").await,
        "80.00"
    );

    request_and_confirm_business_action(
        &state,
        &transport,
        channel_id,
        "ou_more_action_sender",
        "oc_more_action_chat",
        "订单改价 seller-more-actions tid-price 166.00",
        "evt-order-adjust",
        "om-order-adjust",
    )
    .await;
    assert_eq!(
        order_price(&state, "seller-more-actions", "tid-price").await,
        "166.00"
    );

    request_and_confirm_business_action(
        &state,
        &transport,
        channel_id,
        "ou_more_action_sender",
        "oc_more_action_chat",
        "发货 seller-more-actions tid-ship SF 123456 顺丰速运",
        "evt-order-consign",
        "om-order-consign",
    )
    .await;
    assert_eq!(
        order_status_text(&state, "seller-more-actions", "tid-ship").await,
        ("SELLER_CONSIGNED".to_string(), "已发货".to_string())
    );
    assert_eq!(
        order_mail_no(&state, "seller-more-actions", "tid-ship").await,
        "123456"
    );

    clear_env();
}

#[tokio::test]
async fn business_action_nonce_is_sender_and_owner_scoped() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    enable_local_business_action_fallback(&state, channel_id).await;
    seed_account(&state, "seller-scoped", "owner-a").await;
    seed_item(&state, "seller-scoped", "item-scoped", "隔离商品").await;
    for sender in ["ou_nonce_owner", "ou_nonce_intruder"] {
        state
            .storage()
            .upsert_feishu_user_binding(
                channel_id,
                "owner-a",
                sender,
                sender,
                "oc_nonce_chat",
                "p2p",
                "om_seed",
                "evt_seed",
                true,
            )
            .await
            .unwrap();
    }

    handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-scoped-request".to_string(),
            chat_id: "oc_nonce_chat".to_string(),
            message_id: "om_scoped_request".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_nonce_owner".to_string(),
            sender_name: "动作用户".to_string(),
            text: "下架商品 seller-scoped item-scoped".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    let nonce = extract_nonce(
        transport.calls()[0].params["content"]["text"]
            .as_str()
            .unwrap(),
    );

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-scoped-intruder".to_string(),
            chat_id: "oc_nonce_chat".to_string(),
            message_id: "om_scoped_intruder".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_nonce_intruder".to_string(),
            sender_name: "其他用户".to_string(),
            text: format!("确认 {nonce}"),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_action_rejected");
    assert_eq!(
        item_status(&state, "seller-scoped", "item-scoped").await,
        ("ON_SALE".to_string(), true)
    );

    clear_env();
}

#[tokio::test]
async fn business_actions_are_rejected_in_group_chat_without_leaking_owner_data() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    enable_local_business_action_fallback(&state, channel_id).await;
    seed_account(&state, "seller-group", "owner-a").await;
    seed_item(&state, "seller-group", "item-group", "群聊不应泄露商品").await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_group_sender",
            "群聊用户",
            "oc_private_chat",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    for (text, expected_status) in [
        ("商品列表", "scope_rejected"),
        ("下架商品 seller-group item-group", "scope_rejected"),
    ] {
        let outcome = handle_feishu_im_event(
            &state,
            FeishuImEvent {
                channel_id,
                event_id: format!("evt-group-{text}"),
                chat_id: "oc_public_group".to_string(),
                message_id: format!("om-group-{text}"),
                chat_type: "group".to_string(),
                sender_id: "ou_group_sender".to_string(),
                sender_name: "群聊用户".to_string(),
                text: text.to_string(),
                mentioned_bot: true,
                ..FeishuImEvent::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.status, expected_status);
    }

    assert_eq!(
        item_status(&state, "seller-group", "item-group").await,
        ("ON_SALE".to_string(), true)
    );
    let replies = transport
        .calls()
        .into_iter()
        .map(|call| call.params["content"]["text"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 2);
    for reply in replies {
        assert!(reply.contains("私聊"));
        assert!(!reply.contains("群聊不应泄露商品"));
        assert!(!reply.contains("owner-a"));
    }

    clear_env();
}

#[tokio::test]
async fn business_action_upstream_failure_without_local_fallback_does_not_mutate_cache() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;
    seed_account(&state, "seller-remote-required", "owner-a").await;
    seed_item(
        &state,
        "seller-remote-required",
        "item-remote-required",
        "默认不本地伪成功",
    )
    .await;
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            "owner-a",
            "ou_remote_required",
            "远程确认用户",
            "oc_remote_required",
            "p2p",
            "om_seed",
            "evt_seed",
            true,
        )
        .await
        .unwrap();

    handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-remote-required-request".to_string(),
            chat_id: "oc_remote_required".to_string(),
            message_id: "om_remote_required_request".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_remote_required".to_string(),
            sender_name: "远程确认用户".to_string(),
            text: "下架商品 seller-remote-required item-remote-required".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    let nonce = extract_nonce(
        transport.calls()[0].params["content"]["text"]
            .as_str()
            .unwrap(),
    );

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-remote-required-confirm".to_string(),
            chat_id: "oc_remote_required".to_string(),
            message_id: "om_remote_required_confirm".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_remote_required".to_string(),
            sender_name: "远程确认用户".to_string(),
            text: format!("确认 {nonce}"),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "business_action_rejected");
    assert_eq!(
        item_status(&state, "seller-remote-required", "item-remote-required").await,
        ("ON_SALE".to_string(), true)
    );
    assert!(transport.calls()[1].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("执行失败"));

    clear_env();
}

#[tokio::test]
async fn rust_event_handler_defensively_ignores_bot_and_unmentioned_group_messages() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_feishu_channel(&state).await;

    let from_bot = FeishuImEvent {
        channel_id,
        event_id: "evt-bot".to_string(),
        chat_id: "oc_chat".to_string(),
        message_id: "om_bot".to_string(),
        chat_type: "p2p".to_string(),
        sender_id: "ou_bot".to_string(),
        is_from_bot: true,
        ..FeishuImEvent::default()
    };
    let unmentioned_group = FeishuImEvent {
        channel_id,
        event_id: "evt-group".to_string(),
        chat_id: "oc_group".to_string(),
        message_id: "om_group".to_string(),
        chat_type: "group".to_string(),
        sender_id: "ou_sender".to_string(),
        text: "hello".to_string(),
        mentioned_bot: false,
        mention_all: false,
        ..FeishuImEvent::default()
    };

    assert_eq!(
        handle_feishu_im_event(&state, from_bot)
            .await
            .unwrap()
            .status,
        "ignored"
    );
    assert_eq!(
        handle_feishu_im_event(&state, unmentioned_group)
            .await
            .unwrap()
            .status,
        "ignored"
    );
    assert!(transport.calls().is_empty());

    clear_env();
}

#[tokio::test]
async fn reply_errors_are_redacted_before_runtime_persistence() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            SecretErrorTransport,
        )));
    let channel_id = create_feishu_channel(&state).await;

    let err = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-secret-error".to_string(),
            chat_id: "oc_chat".to_string(),
            message_id: "om_secret_error".to_string(),
            chat_type: "p2p".to_string(),
            sender_id: "ou_sender".to_string(),
            text: "/帮助".to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .expect_err("send failure should be returned");

    assert_eq!(err, "Feishu native transport API error");
    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "error");
    assert_eq!(runtime.last_error, "Feishu native transport API error");
    assert!(!runtime.last_error.contains("app_secret"));
    assert!(!runtime.last_error.contains("token"));

    clear_env();
}

async fn create_feishu_channel(state: &AppState) -> i64 {
    state
        .storage()
        .create_notification_channel_for_owner(
            "owner-a",
            "Feishu",
            "feishu",
            &Map::from_iter([
                ("app_id".to_string(), Value::String("cli-a".to_string())),
                ("app_secret".to_string(), Value::String("sec-a".to_string())),
                ("domain".to_string(), Value::String("feishu".to_string())),
                (
                    "allowed_operator_ids".to_string(),
                    json!(["ou_bind_sender"]),
                ),
            ]),
            true,
        )
        .await
        .unwrap()
        .id
}

async fn create_ownerless_feishu_channel(state: &AppState) -> i64 {
    state
        .storage()
        .create_notification_channel(
            "Ownerless Feishu",
            "feishu",
            &Map::from_iter([
                (
                    "app_id".to_string(),
                    Value::String("cli-ownerless".to_string()),
                ),
                (
                    "app_secret".to_string(),
                    Value::String("sec-ownerless".to_string()),
                ),
            ]),
            true,
        )
        .await
        .unwrap()
        .id
}

async fn enable_local_business_action_fallback(state: &AppState, channel_id: i64) {
    sqlx::query(
        r#"
        UPDATE notification_channels
        SET config = ?
        WHERE id = ?
        "#,
    )
    .bind(
        json!({
            "app_id": "cli-a",
            "app_secret": "sec-a",
            "domain": "feishu",
            "allowed_operator_ids": ["ou_bind_sender"],
            "allow_local_business_action_fallback": true
        })
        .to_string(),
    )
    .bind(channel_id)
    .execute(state.storage().notifications())
    .await
    .unwrap();
}

async fn seed_account(state: &AppState, uid: &str, owner_username: &str) {
    sqlx::query(
        r#"
        INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at)
        VALUES (?, ?, ?, 'active', '', '{}', unixepoch('subsec'), unixepoch('subsec'))
        ON CONFLICT(uid) DO UPDATE SET
            owner_username = excluded.owner_username,
            nick = excluded.nick,
            status = excluded.status,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(uid)
    .bind(owner_username)
    .bind(format!("账号 {uid}"))
    .execute(state.storage().accounts())
    .await
    .unwrap();
}

async fn seed_item(state: &AppState, uid: &str, item_id: &str, title: &str) {
    sqlx::query(
        r#"
        INSERT INTO account_items (uid, item_id, title, price, status, online, created_at, updated_at)
        VALUES (?, ?, ?, '88.00', 'ON_SALE', 1, 1, 1)
        "#,
    )
    .bind(uid)
    .bind(item_id)
    .bind(title)
    .execute(state.storage().xianyu())
    .await
    .unwrap();
}

async fn seed_order(state: &AppState, uid: &str, tid: &str, title: &str) {
    sqlx::query(
        r#"
        INSERT INTO account_orders (uid, tid, title, price, status, status_text, buyer_nick, created_at, updated_at)
        VALUES (?, ?, ?, '188.00', 'WAIT_SELLER_SEND_GOODS', '待发货', '买家A', 1, 1)
        "#,
    )
    .bind(uid)
    .bind(tid)
    .bind(title)
    .execute(state.storage().xianyu())
    .await
    .unwrap();
}

async fn seed_history_message(
    state: &AppState,
    uid: &str,
    session_id: &str,
    message_id: &str,
    version: i64,
    ts: i64,
    text: &str,
    is_self: i64,
    sender_nick: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO message_history_cache (
            uid, session_id, session_type, message_id, version, ts, kind, title, text, image_url,
            image_urls, content_type, preview, sender_nick, sender_logo, is_self, cached_at
        )
        VALUES (?, ?, ?, ?, ?, ?, 'text', '', ?, '', '[]', 1, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(1_i64)
    .bind(message_id)
    .bind(version)
    .bind(ts)
    .bind(text)
    .bind(text)
    .bind(sender_nick)
    .bind("https://img.example/sender.png")
    .bind(is_self)
    .bind(1_718_000_130.0_f64)
    .execute(state.storage().live_state())
    .await
    .unwrap();
}

async fn item_status(state: &AppState, uid: &str, item_id: &str) -> (String, bool) {
    let row: (String, i64) =
        sqlx::query_as("SELECT status, online FROM account_items WHERE uid = ? AND item_id = ?")
            .bind(uid)
            .bind(item_id)
            .fetch_one(state.storage().xianyu())
            .await
            .unwrap();
    (row.0, row.1 != 0)
}

async fn order_status_text(state: &AppState, uid: &str, tid: &str) -> (String, String) {
    sqlx::query_as("SELECT status, status_text FROM account_orders WHERE uid = ? AND tid = ?")
        .bind(uid)
        .bind(tid)
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap()
}

async fn item_price(state: &AppState, uid: &str, item_id: &str) -> String {
    sqlx::query_scalar("SELECT price FROM account_items WHERE uid = ? AND item_id = ?")
        .bind(uid)
        .bind(item_id)
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap()
}

async fn order_price(state: &AppState, uid: &str, tid: &str) -> String {
    sqlx::query_scalar("SELECT price FROM account_orders WHERE uid = ? AND tid = ?")
        .bind(uid)
        .bind(tid)
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap()
}

async fn order_mail_no(state: &AppState, uid: &str, tid: &str) -> String {
    sqlx::query_scalar("SELECT mail_no FROM account_orders WHERE uid = ? AND tid = ?")
        .bind(uid)
        .bind(tid)
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap()
}

async fn request_and_confirm_business_action(
    state: &AppState,
    transport: &RecordingTransport,
    channel_id: i64,
    sender_id: &str,
    chat_id: &str,
    request_text: &str,
    request_event_id: &str,
    request_message_id: &str,
) {
    let outcome = handle_feishu_im_event(
        state,
        FeishuImEvent {
            channel_id,
            event_id: request_event_id.to_string(),
            chat_id: chat_id.to_string(),
            message_id: request_message_id.to_string(),
            chat_type: "p2p".to_string(),
            sender_id: sender_id.to_string(),
            sender_name: "动作用户".to_string(),
            text: request_text.to_string(),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome.status, "business_action_pending");

    let first_reply = transport
        .calls()
        .last()
        .expect("business action should reply with a nonce")
        .params["content"]["text"]
        .as_str()
        .unwrap()
        .to_string();
    let nonce = extract_nonce(&first_reply);
    let pending = state
        .storage()
        .get_feishu_pending_action_by_nonce(channel_id, &nonce)
        .await
        .unwrap()
        .expect("pending action should exist");
    assert_eq!(pending.sender_id, sender_id);
    assert_eq!(pending.chat_id, chat_id);

    let outcome = handle_feishu_im_event(
        state,
        FeishuImEvent {
            channel_id,
            event_id: format!("{request_event_id}-confirm"),
            chat_id: chat_id.to_string(),
            message_id: format!("{request_message_id}-confirm"),
            chat_type: "p2p".to_string(),
            sender_id: sender_id.to_string(),
            sender_name: "动作用户".to_string(),
            text: format!("确认 {nonce}"),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(outcome.status, "business_action_executed");
}

fn extract_nonce(reply: &str) -> String {
    reply
        .split_whitespace()
        .find(|part| part.len() == 6 && part.chars().all(|ch| ch.is_ascii_alphanumeric()))
        .expect("reply should include six-character nonce")
        .to_string()
}

#[derive(Clone)]
struct SecretErrorTransport;

impl FeishuNativeTransport for SecretErrorTransport {
    fn call(&self, _request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        Box::pin(async move {
            Err(FeishuNativeError::Api(
                "app_secret=sec-live token=raw-token".to_string(),
            ))
        })
    }
}

#[derive(Clone, Default)]
struct RecordingTransport {
    calls: Arc<Mutex<Vec<FeishuNativeRequest>>>,
}

impl RecordingTransport {
    fn calls(&self) -> Vec<FeishuNativeRequest> {
        self.calls.lock().unwrap().clone()
    }
}

impl FeishuNativeTransport for RecordingTransport {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        self.calls.lock().unwrap().push(request.clone());
        Box::pin(async move {
            Ok(FeishuNativeResponse::success(
                match request.method.as_str() {
                    "chat.members.add" => json!({
                        "status": "ok",
                        "chat_id": request.params["chat_id"],
                        "member_ids": request.params["member_ids"]
                    }),
                    "message.send" => json!({
                        "status": "sent",
                        "message_id": "om_reply",
                        "fell_back": false
                    }),
                    other => panic!("unexpected method {other}"),
                },
            ))
        })
    }
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "feishu-im-reply-secret");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
