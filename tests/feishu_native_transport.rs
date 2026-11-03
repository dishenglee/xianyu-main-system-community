use std::sync::{Arc, Mutex};

use serde_json::json;
use xianyu_rust_backend_full::notifications::feishu_native::{
    ChannelStartRequest, ChannelStopRequest, ChatCreateRequest, ChatMembersAddRequest,
    ChatPatchRequest, FeishuNativeCallFuture, FeishuNativeError, FeishuNativeRequest,
    FeishuNativeResponse, FeishuNativeRuntime, FeishuNativeTransport, MessageSendRequest,
    RegisterPollRequest, RegisterStartRequest,
};

#[tokio::test]
async fn native_runtime_frames_core_methods_without_node_sidecar() {
    let transport = RecordingTransport::default();
    let runtime = FeishuNativeRuntime::with_transport(Arc::new(transport.clone()));

    let start = runtime
        .register_start(RegisterStartRequest::default())
        .await
        .unwrap();
    assert_eq!(start.device_code, "device-001");

    let poll = runtime
        .register_poll(RegisterPollRequest {
            device_code: "device-001".to_string(),
            domain: "feishu".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(poll.status, "pending");

    let started = runtime
        .channel_start(ChannelStartRequest {
            channel_id: 42,
            app_id: "cli-a".to_string(),
            app_secret: "sec-a".to_string(),
            domain: "feishu".to_string(),
            bot_open_id: Some("ou_bot".to_string()),
            bot_user_id: None,
        })
        .await
        .unwrap();
    assert_eq!(started.status, "started");

    let stopped = runtime
        .channel_stop(ChannelStopRequest { channel_id: 42 })
        .await
        .unwrap();
    assert_eq!(stopped.status, "stopped");

    let sent = runtime
        .message_send(MessageSendRequest {
            channel_id: 42,
            receive_id: "oc_chat".to_string(),
            receive_id_type: "chat_id".to_string(),
            msg_type: "text".to_string(),
            content: json!({ "text": "hello" }),
            app_id: Some("cli-a".to_string()),
            app_secret: Some("sec-a".to_string()),
            domain: Some("feishu".to_string()),
            reply_to_message_id: Some("om_source".to_string()),
            reply_in_thread: Some(true),
            fallback_to_plain_chat: Some(true),
            source_event_id: Some("evt-source".to_string()),
        })
        .await
        .unwrap();
    assert_eq!(sent.message_id, "om_001");

    let health = runtime.runtime_health().await.unwrap();
    assert_eq!(health.status, "ok");

    let calls = transport.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec![
            "register.start",
            "register.poll",
            "channel.start",
            "channel.stop",
            "message.send",
            "runtime.health",
        ]
    );
    assert_eq!(calls[2].params["app_id"], "cli-a");
    assert_eq!(calls[4].params["reply_to_message_id"], "om_source");
}

#[tokio::test]
async fn default_native_runtime_reports_in_process_health() {
    let runtime = FeishuNativeRuntime::default();
    let health = runtime.runtime_health().await.unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.transport, "rust-native");
}

#[tokio::test]
async fn native_runtime_surfaces_bounded_api_errors() {
    let runtime = FeishuNativeRuntime::with_transport(Arc::new(FailingTransport));
    let error = runtime.runtime_health().await.unwrap_err();
    assert!(matches!(error, FeishuNativeError::Api(_)));
    assert!(error
        .to_string()
        .contains("Feishu native transport API error"));
}

#[tokio::test]
async fn native_transport_supports_group_chat_methods() {
    let transport = RecordingTransport::default();
    let runtime = FeishuNativeRuntime::with_transport(Arc::new(transport.clone()));

    let created = runtime
        .chat_create(ChatCreateRequest {
            channel_id: 9,
            name: "客户甲｜MacBook Air".to_string(),
            member_id_type: "open_id".to_string(),
            member_ids: vec!["ou_operator_1".to_string()],
            avatar_url: None,
            app_id: Some("cli_xxx".to_string()),
            app_secret: Some("sec_xxx".to_string()),
            domain: Some("feishu".to_string()),
        })
        .await
        .unwrap();

    assert_eq!(created.chat_id, "oc_group_a");

    runtime
        .chat_members_add(ChatMembersAddRequest {
            channel_id: 9,
            chat_id: "oc_group_a".to_string(),
            member_id_type: "open_id".to_string(),
            member_ids: vec!["ou_operator_2".to_string()],
            app_id: Some("cli_xxx".to_string()),
            app_secret: Some("sec_xxx".to_string()),
            domain: Some("feishu".to_string()),
        })
        .await
        .unwrap();

    runtime
        .chat_patch(ChatPatchRequest {
            channel_id: 9,
            chat_id: "oc_group_a".to_string(),
            name: "客户甲｜MacBook Air【已结束】".to_string(),
            avatar_url: None,
            app_id: Some("cli_xxx".to_string()),
            app_secret: Some("sec_xxx".to_string()),
            domain: Some("feishu".to_string()),
        })
        .await
        .unwrap();
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
        let result = match request.method.as_str() {
            "register.start" => json!({
                "device_code": "device-001",
                "qr_url": "https://accounts.feishu.cn/qrcode",
                "expires_at_ms": 123,
                "poll_interval_ms": 2000,
                "domain": "feishu"
            }),
            "register.poll" => json!({
                "status": "pending",
                "device_code": request.params["device_code"],
                "domain": request.params["domain"]
            }),
            "channel.start" => json!({
                "status": "started",
                "channel_id": request.params["channel_id"]
            }),
            "channel.stop" => json!({
                "status": "stopped",
                "channel_id": request.params["channel_id"]
            }),
            "message.send" => json!({
                "status": "sent",
                "message_id": "om_001",
                "fell_back": false
            }),
            "runtime.health" => json!({
                "status": "ok",
                "transport": "fake-native"
            }),
            "chat.create" => json!({
                "status": "created",
                "chat_id": "oc_group_a",
                "name": request.params["name"]
            }),
            "chat.members.add" => json!({
                "status": "ok",
                "chat_id": request.params["chat_id"],
                "member_ids": request.params["member_ids"]
            }),
            "chat.patch" => json!({
                "status": "ok",
                "chat_id": request.params["chat_id"],
                "name": request.params["name"]
            }),
            other => panic!("unexpected method {other}"),
        };
        Box::pin(async move { Ok(FeishuNativeResponse::success(result)) })
    }
}

#[derive(Clone)]
struct FailingTransport;

impl FeishuNativeTransport for FailingTransport {
    fn call(&self, _request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        Box::pin(async {
            Err(FeishuNativeError::Api(
                "app_secret=sec-test token=raw-token".to_string(),
            ))
        })
    }
}
