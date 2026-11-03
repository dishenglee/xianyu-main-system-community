use std::{
    env,
    sync::{Arc, Mutex},
};

use axum::{extract::State, http::Uri, routing::post, Json, Router};
use serde_json::{json, Map, Value};
use tempfile::TempDir;
use xianyu_rust_backend_full::{
    config::AppConfig,
    notifications::{
        dispatch_message_event_to_channels,
        feishu_native::{
            FeishuNativeCallFuture, FeishuNativeRequest, FeishuNativeResponse, FeishuNativeRuntime,
            FeishuNativeTransport,
        },
        manager::restart_worker_for_channel,
    },
    routes::accounts::LiveMessageEvent,
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn dispatcher_openclaw_context_token_failure_clears_stale_token_and_records_restart_runtime()
{
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    let mock = start_openclaw_error_mock_server().await;
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    seed_account(&state, "seller-openclaw", "微信卖家").await;

    let channel_id = create_channel(
        &state,
        "Openclaw Restart",
        "openclaw_weixin",
        Map::from_iter([
            (
                "base_url".to_string(),
                Value::String(mock.base_url().to_string()),
            ),
            (
                "bot_token".to_string(),
                Value::String("wx-bot-token".to_string()),
            ),
            (
                "ilink_user_id".to_string(),
                Value::String("wx-target-001".to_string()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-openclaw", channel_id, true)
        .await
        .unwrap();
    state
        .storage()
        .patch_notification_runtime(
            channel_id,
            &Map::from_iter([(
                "context_tokens".to_string(),
                Value::Object(Map::from_iter([(
                    "wx-target-001".to_string(),
                    Value::String("stale-ctx-token".to_string()),
                )])),
            )]),
        )
        .await
        .unwrap();

    restart_worker_for_channel(state.clone(), channel_id).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let event = LiveMessageEvent {
        session_id: "session-openclaw".to_string(),
        peer_uid: "buyer-openclaw".to_string(),
        peer_nick: "微信买家".to_string(),
        item_id: "item-openclaw".to_string(),
        item_title: "微信商品".to_string(),
        text: "openclaw stale context should self-heal".to_string(),
        preview: "openclaw stale context should self-heal".to_string(),
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-openclaw", &event).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(runtime.last_error, "");
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("spawned")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_source")
            .and_then(Value::as_str),
        Some("wechat.manager")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_channel_type")
            .and_then(Value::as_str),
        Some("openclaw_weixin")
    );
    let context_tokens = runtime
        .runtime_json
        .get("context_tokens")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert!(context_tokens.get("wx-target-001").is_none());

    clear_env();
}

#[tokio::test]
async fn dispatcher_feishu_channel_sends_via_native_and_records_runtime_success() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingFeishuTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-feishu", "飞书卖家").await;

    let channel_id = create_channel(
        &state,
        "Feishu Notify",
        "feishu",
        Map::from_iter([
            (
                "receive_id".to_string(),
                Value::String("oc_notify".to_string()),
            ),
            (
                "receive_id_type".to_string(),
                Value::String("chat_id".to_string()),
            ),
            (
                "im_aggregate_window_ms".to_string(),
                Value::Number(0.into()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-feishu", channel_id, true)
        .await
        .unwrap();

    let event = LiveMessageEvent {
        session_id: "session-feishu".to_string(),
        peer_uid: "buyer-feishu".to_string(),
        peer_nick: "飞书买家".to_string(),
        item_id: "item-feishu".to_string(),
        item_title: "飞书商品".to_string(),
        text: "Feishu native should receive this Xianyu IM".to_string(),
        preview: "Feishu native should receive this Xianyu IM".to_string(),
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-feishu", &event).await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "message.send");
    assert_eq!(calls[0].params["channel_id"], channel_id);
    assert_eq!(calls[0].params["receive_id"], "oc_notify");
    assert_eq!(calls[0].params["receive_id_type"], "chat_id");
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("Feishu native should receive this Xianyu IM"));

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(runtime.last_error, "");
    assert!(runtime.last_attempt_at > 0.0);
    assert!(runtime.last_success_at > 0.0);
    assert_eq!(
        runtime
            .runtime_json
            .get("last_feishu_notify_message_id")
            .and_then(Value::as_str),
        Some("om_dispatch")
    );
    let route = state
        .storage()
        .get_feishu_im_reply_route(channel_id, "om_dispatch")
        .await
        .unwrap()
        .expect("Feishu reply route should be persisted");
    assert_eq!(route.channel_id, channel_id);
    assert_eq!(route.owner_username, "");
    assert_eq!(route.feishu_message_id, "om_dispatch");
    assert_eq!(route.chat_id, "oc_notify");
    assert_eq!(route.uid, "seller-feishu");
    assert_eq!(route.session_id, "session-feishu");
    assert_eq!(route.peer_uid, "buyer-feishu");
    assert_eq!(route.customer_name, "飞书买家");
    assert_eq!(route.item_id, "item-feishu");
    assert_eq!(route.item_title, "飞书商品");

    clear_env();
}

#[tokio::test]
async fn dispatcher_creates_group_sends_main_card_and_first_incremental_message() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingFeishuTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-feishu-group", "七七书屋11").await;

    let channel_id = create_channel(
        &state,
        "Feishu Group Notify",
        "feishu",
        Map::from_iter([
            ("app_id".to_string(), Value::String("cli-a".to_string())),
            ("app_secret".to_string(), Value::String("sec-a".to_string())),
            (
                "im_aggregate_window_ms".to_string(),
                Value::Number(0.into()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-feishu-group", channel_id, true)
        .await
        .unwrap();
    seed_latest_operator(&state, channel_id, "", "ou_operator_1", "Alice").await;

    let event = LiveMessageEvent {
        session_id: "61736334897".to_string(),
        session_type: 1,
        peer_uid: "buyer-001".to_string(),
        peer_nick: "测试用户甲".to_string(),
        item_id: "1054950360553".to_string(),
        item_title: "苹果x 256G 白色 国行双卡 插卡即用".to_string(),
        item_pic: "https://img.example/item.png".to_string(),
        text: "你好".to_string(),
        preview: "你好".to_string(),
        ts: 1_719_225_600,
        kind: "text".to_string(),
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-feishu-group", &event).await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 3, "{calls:#?}");
    assert_eq!(calls[0].method, "chat.create");
    assert_eq!(calls[1].method, "message.send");
    assert_eq!(calls[2].method, "message.send");
    assert_eq!(calls[1].params["receive_id"], "oc_group_dispatch");
    assert_eq!(calls[2].params["receive_id"], "oc_group_dispatch");

    let mapping = state
        .storage()
        .get_feishu_session_group_chat_by_session(
            channel_id,
            "",
            "seller-feishu-group",
            "61736334897",
        )
        .await
        .unwrap()
        .expect("dispatcher should persist group mapping");
    assert_eq!(mapping.feishu_chat_id, "oc_group_dispatch");

    clear_env();
}

#[tokio::test]
async fn dispatcher_does_not_fallback_to_legacy_card_when_group_avatar_patch_fails() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingFeishuTransport::with_failed_chat_patch();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-feishu-group", "七七书屋11").await;

    let channel_id = create_channel(
        &state,
        "Feishu Group Notify",
        "feishu",
        Map::from_iter([
            ("app_id".to_string(), Value::String("cli-a".to_string())),
            ("app_secret".to_string(), Value::String("sec-a".to_string())),
            (
                "im_aggregate_window_ms".to_string(),
                Value::Number(0.into()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-feishu-group", channel_id, true)
        .await
        .unwrap();
    seed_latest_operator(&state, channel_id, "", "ou_operator_1", "Alice").await;

    let first = LiveMessageEvent {
        session_id: "61736334897".to_string(),
        session_type: 1,
        peer_uid: "buyer-001".to_string(),
        peer_nick: "测试用户甲".to_string(),
        item_id: "1054950360553".to_string(),
        item_title: "苹果x 256G 白色 国行双卡 插卡即用".to_string(),
        item_pic: "https://img.example/item.png".to_string(),
        text: "你好".to_string(),
        preview: "你好".to_string(),
        ts: 1_719_225_600,
        kind: "text".to_string(),
        ..Default::default()
    };
    let second = LiveMessageEvent {
        peer_logo: "https://img.example/buyer-avatar.png".to_string(),
        text: "不行".to_string(),
        preview: "不行".to_string(),
        ts: 1_719_225_700,
        ..first.clone()
    };

    dispatch_message_event_to_channels(&state, "seller-feishu-group", &first).await;
    dispatch_message_event_to_channels(&state, "seller-feishu-group", &second).await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 5, "{calls:#?}");
    assert_eq!(calls[3].method, "chat.patch");
    assert_eq!(calls[4].method, "message.send");
    assert_eq!(calls[4].params["msg_type"], "text");
    assert_eq!(calls[4].params["receive_id"], "oc_group_dispatch");

    clear_env();
}

#[tokio::test]
async fn dispatcher_skips_platform_notification_sessions_when_global_toggle_is_off() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingFeishuTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-platform-off", "平台消息卖家").await;

    let channel_id = create_channel(
        &state,
        "Feishu Platform Toggle",
        "feishu",
        Map::from_iter([
            (
                "receive_id".to_string(),
                Value::String("oc_platform_toggle".to_string()),
            ),
            (
                "receive_id_type".to_string(),
                Value::String("chat_id".to_string()),
            ),
            (
                "im_aggregate_window_ms".to_string(),
                Value::Number(0.into()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-platform-off", channel_id, true)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO ai_reply_settings (cookie_id, platform_messages_enabled, updated_at) VALUES ('__global__', 0, 1)
         ON CONFLICT(cookie_id) DO UPDATE SET platform_messages_enabled = excluded.platform_messages_enabled, updated_at = excluded.updated_at",
    )
    .execute(state.storage().accounts())
    .await
    .unwrap();

    let interaction_event = LiveMessageEvent {
        session_id: "session-interaction-off".to_string(),
        session_type: 6,
        peer_uid: "buyer-interaction-off".to_string(),
        peer_nick: "互动买家".to_string(),
        item_id: "item-interaction-off".to_string(),
        item_title: "互动商品".to_string(),
        text: "这条互动消息不该再通知".to_string(),
        preview: "这条互动消息不该再通知".to_string(),
        ..Default::default()
    };
    let workbench_event = LiveMessageEvent {
        session_id: "session-workbench-off".to_string(),
        session_type: 53,
        peer_uid: "2222216136891".to_string(),
        peer_nick: "工作台通知".to_string(),
        text: "这条工作台通知也不该再通知".to_string(),
        preview: "这条工作台通知也不该再通知".to_string(),
        ..Default::default()
    };
    let normal_chat_event = LiveMessageEvent {
        session_id: "session-chat-on".to_string(),
        session_type: 1,
        peer_uid: "buyer-chat-on".to_string(),
        peer_nick: "正常买家".to_string(),
        item_id: "item-chat-on".to_string(),
        item_title: "普通商品".to_string(),
        text: "普通买家消息仍然要通知".to_string(),
        preview: "普通买家消息仍然要通知".to_string(),
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-platform-off", &interaction_event).await;
    dispatch_message_event_to_channels(&state, "seller-platform-off", &workbench_event).await;
    assert_eq!(transport.calls().len(), 0);

    dispatch_message_event_to_channels(&state, "seller-platform-off", &normal_chat_event).await;
    assert_eq!(transport.calls().len(), 1);
    assert!(transport.calls()[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("普通买家消息仍然要通知"));

    clear_env();
}

#[tokio::test]
async fn dispatcher_feishu_batches_multiple_im_events_into_one_interactive_summary_card() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingFeishuTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-a", "一号卖家").await;
    seed_account(&state, "seller-b", "二号卖家").await;

    let channel_id = create_channel(
        &state,
        "Feishu Aggregate",
        "feishu",
        Map::from_iter([
            (
                "receive_id".to_string(),
                Value::String("oc_aggregate".to_string()),
            ),
            (
                "receive_id_type".to_string(),
                Value::String("chat_id".to_string()),
            ),
            (
                "im_aggregate_window_ms".to_string(),
                Value::Number(20.into()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-a", channel_id, true)
        .await
        .unwrap();
    state
        .storage()
        .set_notification_binding("seller-b", channel_id, true)
        .await
        .unwrap();

    let first = LiveMessageEvent {
        session_id: "session-a".to_string(),
        peer_uid: "buyer-a".to_string(),
        peer_nick: "买家A".to_string(),
        item_id: "item-shared".to_string(),
        item_title: "同款商品".to_string(),
        text: "第一条咨询".to_string(),
        ts: 1_779_000_000_000,
        ..Default::default()
    };
    let second = LiveMessageEvent {
        message_id: "xianyu-second".to_string(),
        upstream_message_id: "pnm-second".to_string(),
        text: "同会话追问".to_string(),
        ts: 1_779_000_001_000,
        ..first.clone()
    };
    let third = LiveMessageEvent {
        session_id: "session-b".to_string(),
        message_id: "xianyu-third".to_string(),
        upstream_message_id: "pnm-third".to_string(),
        peer_uid: "buyer-b".to_string(),
        peer_nick: "买家B".to_string(),
        item_id: "item-b".to_string(),
        item_title: "另一件商品".to_string(),
        text: "二号账号也来了消息".to_string(),
        ts: 1_779_000_002_000,
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-a", &first).await;
    dispatch_message_event_to_channels(&state, "seller-a", &second).await;
    dispatch_message_event_to_channels(&state, "seller-b", &third).await;
    assert_eq!(transport.calls().len(), 0);

    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "message.send");
    assert_eq!(calls[0].params["receive_id"], "oc_aggregate");
    assert_eq!(calls[0].params["msg_type"], "interactive");
    let card_text = calls[0].params["content"].to_string();
    assert!(card_text.contains("闲鱼消息收件箱"), "{card_text}");
    assert!(card_text.contains("一号卖家"), "{card_text}");
    assert!(card_text.contains("二号卖家"), "{card_text}");
    assert!(card_text.contains("同会话追问"), "{card_text}");
    assert!(card_text.contains("二号账号也来了消息"), "{card_text}");
    assert!(
        card_text.contains("本轮共收到 3 条消息，已归并成 2 个会话"),
        "{card_text}"
    );
    assert!(
        card_text.contains("像 IM 收件箱一样先选会话，再进入聊天"),
        "{card_text}"
    );
    assert!(card_text.contains("进入聊天 1"), "{card_text}");
    assert!(card_text.contains("进入聊天 2"), "{card_text}");
    assert!(
        card_text.contains("点“进入聊天”后，会打开会话主卡"),
        "{card_text}"
    );

    let route = state
        .storage()
        .get_feishu_im_reply_route(channel_id, "om_dispatch")
        .await
        .unwrap()
        .expect("aggregate card should still persist a latest reply route");
    assert_eq!(route.uid, "seller-b");
    assert_eq!(route.session_id, "session-b");
    assert_eq!(route.peer_uid, "buyer-b");

    let commands = select_open_chat_commands(&calls[0].params["content"]);
    assert_eq!(commands.len(), 2, "{commands:?}");
    let first_route_id = commands[0]
        .strip_prefix("打开会话 ")
        .expect("select reply command prefix");
    let second_route_id = commands[1]
        .strip_prefix("打开会话 ")
        .expect("select reply command prefix");

    let first_route = state
        .storage()
        .get_feishu_im_reply_route(channel_id, first_route_id)
        .await
        .unwrap()
        .expect("first aggregate row route should exist");
    assert_eq!(first_route.uid, "seller-b");
    assert_eq!(first_route.session_id, "session-b");
    assert_eq!(first_route.peer_uid, "buyer-b");

    let second_route = state
        .storage()
        .get_feishu_im_reply_route(channel_id, second_route_id)
        .await
        .unwrap()
        .expect("second aggregate row route should exist");
    assert_eq!(second_route.uid, "seller-a");
    assert_eq!(second_route.session_id, "session-a");
    assert_eq!(second_route.xianyu_message_id, "xianyu-second");

    clear_env();
}

#[tokio::test]
async fn dispatcher_feishu_aggregate_latest_route_uses_highest_ts_when_message_id_is_empty() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingFeishuTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    seed_account(&state, "seller-empty-mid", "空消息ID卖家").await;

    let channel_id = create_channel(
        &state,
        "Feishu Empty MID Aggregate",
        "feishu",
        Map::from_iter([
            (
                "receive_id".to_string(),
                Value::String("oc_empty_mid".to_string()),
            ),
            (
                "receive_id_type".to_string(),
                Value::String("chat_id".to_string()),
            ),
            (
                "im_aggregate_window_ms".to_string(),
                Value::Number(20.into()),
            ),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-empty-mid", channel_id, true)
        .await
        .unwrap();

    let old = LiveMessageEvent {
        session_id: "session-old-empty-mid".to_string(),
        peer_uid: "buyer-old-empty-mid".to_string(),
        peer_nick: "旧买家".to_string(),
        item_id: "item-empty-mid".to_string(),
        item_title: "空消息ID商品".to_string(),
        text: "旧消息没有 message_id".to_string(),
        ts: 1_779_000_000_000,
        ..Default::default()
    };
    let new = LiveMessageEvent {
        session_id: "session-new-empty-mid".to_string(),
        peer_uid: "buyer-new-empty-mid".to_string(),
        peer_nick: "新买家".to_string(),
        text: "新消息也没有 message_id".to_string(),
        ts: 1_779_000_001_000,
        ..old.clone()
    };

    dispatch_message_event_to_channels(&state, "seller-empty-mid", &old).await;
    dispatch_message_event_to_channels(&state, "seller-empty-mid", &new).await;
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;

    let route = state
        .storage()
        .get_feishu_im_reply_route(channel_id, "om_dispatch")
        .await
        .unwrap()
        .expect("aggregate card should persist latest route");
    assert_eq!(route.session_id, "session-new-empty-mid");
    assert_eq!(route.peer_uid, "buyer-new-empty-mid");

    clear_env();
}

#[tokio::test]
async fn dispatcher_dingtalk_channel_sends_via_retained_robot_url_driver() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    let mock = start_robot_mock_server().await;
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    seed_account(&state, "seller-dingtalk", "钉钉卖家").await;

    let channel_id = create_channel(
        &state,
        "DingTalk Robot",
        "dingtalk",
        Map::from_iter([
            (
                "robot_url".to_string(),
                Value::String(format!("{}/robot/send", mock.base_url())),
            ),
            ("secret".to_string(), Value::String("SEC-test".to_string())),
        ]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-dingtalk", channel_id, true)
        .await
        .unwrap();

    let event = LiveMessageEvent {
        session_id: "session-dingtalk".to_string(),
        peer_uid: "buyer-dingtalk".to_string(),
        peer_nick: "钉钉买家".to_string(),
        item_id: "item-dingtalk".to_string(),
        item_title: "钉钉商品".to_string(),
        text: "DingTalk robot should receive this Xianyu IM".to_string(),
        preview: "DingTalk robot should receive this Xianyu IM".to_string(),
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-dingtalk", &event).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].query.contains("timestamp="));
    assert!(calls[0].query.contains("sign="));
    assert_eq!(calls[0].payload["msgtype"], "text");
    assert!(calls[0].payload["text"]["content"]
        .as_str()
        .unwrap()
        .contains("DingTalk robot should receive this Xianyu IM"));

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(runtime.last_error, "");
    assert_eq!(
        runtime
            .runtime_json
            .get("last_robot_driver")
            .and_then(Value::as_str),
        Some("dingtalk")
    );

    clear_env();
}

#[tokio::test]
async fn dispatcher_enterprise_wechat_channel_sends_via_retained_robot_url_driver() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    let mock = start_robot_mock_server().await;
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    seed_account(&state, "seller-wechat", "企业微信卖家").await;

    let channel_id = create_channel(
        &state,
        "Enterprise WeChat Robot",
        "wechat",
        Map::from_iter([(
            "robot_url".to_string(),
            Value::String(format!("{}/robot/send", mock.base_url())),
        )]),
    )
    .await;
    state
        .storage()
        .set_notification_binding("seller-wechat", channel_id, true)
        .await
        .unwrap();

    let event = LiveMessageEvent {
        session_id: "session-wechat".to_string(),
        peer_uid: "buyer-wechat".to_string(),
        peer_nick: "企业微信买家".to_string(),
        item_id: "item-wechat".to_string(),
        item_title: "企业微信商品".to_string(),
        text: "Enterprise WeChat robot should receive this Xianyu IM".to_string(),
        preview: "Enterprise WeChat robot should receive this Xianyu IM".to_string(),
        ..Default::default()
    };

    dispatch_message_event_to_channels(&state, "seller-wechat", &event).await;

    let calls = mock.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].query, "");
    assert_eq!(calls[0].payload["msgtype"], "text");
    assert!(calls[0].payload["text"]["content"]
        .as_str()
        .unwrap()
        .contains("Enterprise WeChat robot should receive this Xianyu IM"));

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(runtime.last_error, "");
    assert_eq!(
        runtime
            .runtime_json
            .get("last_robot_driver")
            .and_then(Value::as_str),
        Some("wechat")
    );

    clear_env();
}

async fn create_channel(
    state: &AppState,
    name: &str,
    channel_type: &str,
    config: Map<String, Value>,
) -> i64 {
    state
        .storage()
        .create_notification_channel(name, channel_type, &config, true)
        .await
        .unwrap()
        .id
}

async fn seed_account(state: &AppState, uid: &str, nick: &str) {
    sqlx::query("INSERT OR REPLACE INTO accounts (uid, nick) VALUES (?, ?)")
        .bind(uid)
        .bind(nick)
        .execute(state.storage().accounts())
        .await
        .unwrap();
}

async fn seed_latest_operator(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    sender_name: &str,
) {
    state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            "",
            "evt-seed",
        )
        .await
        .unwrap();
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "notifications-dispatcher-secret");
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

struct MockOpenclawErrorServer {
    addr: std::net::SocketAddr,
}

impl MockOpenclawErrorServer {
    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

async fn start_openclaw_error_mock_server() -> MockOpenclawErrorServer {
    async fn send_message() -> Json<Value> {
        Json(json!({
            "ret": -2,
            "errmsg": "stale context token"
        }))
    }

    async fn get_updates() -> Json<Value> {
        Json(json!({
            "ret": 0,
            "errcode": 0,
            "errmsg": "ok",
            "msgs": [],
            "get_updates_buf": "cursor-001",
            "longpolling_timeout_ms": 1
        }))
    }

    let app = Router::new()
        .route("/ilink/bot/sendmessage", post(send_message))
        .route("/ilink/bot/getupdates", post(get_updates));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    MockOpenclawErrorServer { addr }
}

#[derive(Clone, Debug)]
struct RobotCall {
    query: String,
    payload: Value,
}

#[derive(Clone)]
struct MockRobotServer {
    addr: std::net::SocketAddr,
    calls: Arc<Mutex<Vec<RobotCall>>>,
}

impl MockRobotServer {
    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn calls(&self) -> Vec<RobotCall> {
        self.calls.lock().unwrap().clone()
    }
}

async fn record_robot_call(
    State(calls): State<Arc<Mutex<Vec<RobotCall>>>>,
    uri: Uri,
    Json(payload): Json<Value>,
) -> Json<Value> {
    calls.lock().unwrap().push(RobotCall {
        query: uri.query().unwrap_or("").to_string(),
        payload,
    });
    Json(json!({
        "errcode": 0,
        "errmsg": "ok"
    }))
}

async fn start_robot_mock_server() -> MockRobotServer {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route("/robot/send", post(record_robot_call))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    MockRobotServer { addr, calls }
}

#[derive(Clone, Default)]
struct RecordingFeishuTransport {
    calls: Arc<Mutex<Vec<FeishuNativeRequest>>>,
    fail_chat_patch: bool,
}

impl RecordingFeishuTransport {
    fn calls(&self) -> Vec<FeishuNativeRequest> {
        self.calls.lock().unwrap().clone()
    }

    fn with_failed_chat_patch() -> Self {
        Self {
            fail_chat_patch: true,
            ..Self::default()
        }
    }
}

impl FeishuNativeTransport for RecordingFeishuTransport {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        self.calls.lock().unwrap().push(request.clone());
        let fail_chat_patch = self.fail_chat_patch;
        Box::pin(async move {
            if request.method == "chat.patch" && fail_chat_patch {
                return Err(
                    xianyu_rust_backend_full::notifications::feishu_native::FeishuNativeError::Api(
                        "chat.patch HTTP 500".to_string(),
                    ),
                );
            }
            Ok(FeishuNativeResponse::success(
                match request.method.as_str() {
                    "chat.create" => json!({
                        "status": "created",
                        "chat_id": "oc_group_dispatch",
                        "name": request.params["name"],
                    }),
                    "message.send" => json!({
                        "status": "sent",
                        "message_id": "om_dispatch",
                        "fell_back": false
                    }),
                    "chat.patch" => json!({
                        "status": "ok",
                        "chat_id": request.params["chat_id"],
                        "name": request.params["name"]
                    }),
                    other => panic!("unexpected method {other}"),
                },
            ))
        })
    }
}

fn select_open_chat_commands(card: &Value) -> Vec<String> {
    card.get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|element| element.get("actions").and_then(Value::as_array))
        .flatten()
        .filter_map(|action| {
            action
                .get("value")
                .and_then(|value| value.get("command"))
                .and_then(Value::as_str)
        })
        .filter(|command| command.starts_with("打开会话 "))
        .map(str::to_string)
        .collect()
}
