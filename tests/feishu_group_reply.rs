#[path = "../src/notifications/feishu_group_cards.rs"]
mod feishu_group_cards;

use std::{
    env,
    sync::{Arc, Mutex},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use feishu_group_cards::{
    build_feishu_group_incremental_message, build_feishu_group_main_card,
    build_feishu_group_status_card, FeishuGroupCardContext, FeishuGroupCardMessage,
};
use serde_json::{json, Map, Value};
use tempfile::TempDir;
use tokio::sync::mpsc;
use xianyu_rust_backend_full::{
    config::AppConfig,
    notifications::{
        feishu_events::{handle_feishu_im_event, FeishuImEvent},
        feishu_native::{
            FeishuNativeCallFuture, FeishuNativeRequest, FeishuNativeResponse, FeishuNativeRuntime,
            FeishuNativeTransport,
        },
    },
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn group_main_card_only_keeps_compact_summary() {
    let card = build_feishu_group_main_card(
        sample_context(),
        &[
            FeishuGroupCardMessage {
                ts: 1719225600,
                sender_label: "客户".to_string(),
                body: "你好".to_string(),
                is_self: false,
            },
            FeishuGroupCardMessage {
                ts: 1719225660,
                sender_label: "我".to_string(),
                body: "你好，在的".to_string(),
                is_self: true,
            },
        ],
    );

    let body = serde_json::to_string(&card).unwrap();
    assert!(body.contains("苹果x 256G 白色"));
    assert!(body.contains("七七书屋11"));
    assert!(body.contains("进行中"));
    assert!(body.contains("会话 61736334897"));
    assert!(body.contains("商品：苹果x 256G 白色"));
    assert!(body.contains("账号：七七书屋11"));
    assert!(!body.contains("group.reply.send"));
    assert!(!body.contains("商品详情"));
    assert!(!body.contains("订单详情"));
    assert!(!body.contains("reply_text"));
    assert!(!body.contains("你好，在的"));
    assert!(!body.contains("暂无历史消息"));
}

#[test]
fn group_incremental_card_only_keeps_compact_summary() {
    let card = build_feishu_group_incremental_message(
        &sample_context(),
        &FeishuGroupCardMessage {
            ts: 1719225600,
            sender_label: "客户".to_string(),
            body: "有人在吗".to_string(),
            is_self: false,
        },
    );

    let body = serde_json::to_string(&card).unwrap();
    assert!(body.contains("苹果x 256G 白色"));
    assert!(body.contains("七七书屋11"));
    assert!(body.contains("进行中"));
    assert!(body.contains("会话 61736334897"));
    assert!(body.contains("商品：苹果x 256G 白色"));
    assert!(body.contains("账号：七七书屋11"));
    assert!(!body.contains("有人在吗"));
    assert!(!body.contains("发送回复"));
    assert!(!body.contains("#商品"));
    assert!(!body.contains("#订单"));
}

#[test]
fn group_status_card_shows_terminal_state_in_compact_summary() {
    let mut context = sample_context();
    context.status = "closed".to_string();

    let card = build_feishu_group_status_card(&context, "会话已结束");
    let body = serde_json::to_string(&card).unwrap();

    assert!(body.contains("会话已结束"));
    assert!(body.contains("会话 61736334897"));
    assert!(body.contains("商品：苹果x 256G 白色"));
    assert!(body.contains("账号：七七书屋11"));
    assert!(body.contains("已结束"));
    assert!(!body.contains("商品详情"));
    assert!(!body.contains("订单详情"));
}

#[tokio::test]
async fn bound_operator_group_plain_text_replies_to_xianyu() {
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
    seed_account(&state, "seller-a", "owner-a", "七七书屋11").await;
    seed_group_mapping(
        &state,
        channel_id,
        "owner-a",
        "seller-a",
        "session-a",
        "oc_group_a",
    )
    .await;
    seed_bound_operator(
        &state,
        channel_id,
        "owner-a",
        "ou_operator_1",
        "Alice",
        "oc_group_a",
    )
    .await;

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-a".to_string(), live_tx)
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
            .take("seller-a", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-group-1"
                }
            }))
            .unwrap();
        decoded
    });

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-group-text".to_string(),
            chat_id: "oc_group_a".to_string(),
            message_id: "om_group_text".to_string(),
            chat_type: "group".to_string(),
            sender_id: "ou_operator_1".to_string(),
            sender_name: "Alice".to_string(),
            text: "你好，这边有货".to_string(),
            mentioned_bot: true,
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "xianyu_replied");
    let decoded = ack_task.await.unwrap();
    assert_eq!(decoded["text"]["text"], "你好，这边有货");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["reply_to_message_id"].is_null());
    assert!(calls[0].params["reply_in_thread"].is_null());
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已回复"));

    clear_env();
}

#[tokio::test]
async fn bound_operator_group_plain_text_without_mention_still_replies_to_xianyu() {
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
    seed_account(&state, "seller-a", "owner-a", "七七书屋11").await;
    seed_group_mapping(
        &state,
        channel_id,
        "owner-a",
        "seller-a",
        "session-a",
        "oc_group_a",
    )
    .await;
    seed_bound_operator(
        &state,
        channel_id,
        "owner-a",
        "ou_operator_1",
        "Alice",
        "oc_group_a",
    )
    .await;

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-a".to_string(), live_tx)
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
            .take("seller-a", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-group-no-mention"
                }
            }))
            .unwrap();
        decoded
    });

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-group-no-mention".to_string(),
            chat_id: "oc_group_a".to_string(),
            message_id: "om_group_no_mention".to_string(),
            chat_type: "group".to_string(),
            sender_id: "ou_operator_1".to_string(),
            sender_name: "Alice".to_string(),
            text: "直接群聊回复".to_string(),
            mentioned_bot: false,
            mention_all: false,
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "xianyu_replied");
    let decoded = ack_task.await.unwrap();
    assert_eq!(decoded["text"]["text"], "直接群聊回复");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["reply_to_message_id"].is_null());
    assert!(calls[0].params["reply_in_thread"].is_null());
    clear_env();
}

#[tokio::test]
async fn group_card_input_uses_reply_text_form_value() {
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
    seed_account(&state, "seller-a", "owner-a", "七七书屋11").await;
    seed_group_mapping(
        &state,
        channel_id,
        "owner-a",
        "seller-a",
        "session-a",
        "oc_group_a",
    )
    .await;
    seed_bound_operator(
        &state,
        channel_id,
        "owner-a",
        "ou_operator_1",
        "Alice",
        "oc_group_a",
    )
    .await;

    let (live_tx, mut live_rx) = mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("seller-a".to_string(), live_tx)
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
            .take("seller-a", &mid)
            .await
            .expect("registered reply waiter");
        waiter
            .tx
            .send(json!({
                "code": 200,
                "body": {
                    "messageId": "reply-pnm-group-2"
                }
            }))
            .unwrap();
        decoded
    });

    let outcome = handle_feishu_im_event(
        &state,
        FeishuImEvent {
            channel_id,
            event_id: "evt-group-card".to_string(),
            chat_id: "oc_group_a".to_string(),
            message_id: "om_group_card".to_string(),
            chat_type: "group".to_string(),
            sender_id: "ou_operator_1".to_string(),
            sender_name: "Alice".to_string(),
            text: "group.reply.send".to_string(),
            mentioned_bot: true,
            action_form_values: Map::from_iter([(
                "reply_text".to_string(),
                Value::String("卡片输入回复".to_string()),
            )]),
            ..FeishuImEvent::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.status, "xianyu_replied");
    let decoded = ack_task.await.unwrap();
    assert_eq!(decoded["text"]["text"], "卡片输入回复");
    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].params["content"]["text"]
        .as_str()
        .unwrap()
        .contains("已回复"));

    clear_env();
}

fn sample_context() -> FeishuGroupCardContext {
    FeishuGroupCardContext {
        uid: "seller-a".to_string(),
        account_name: "七七书屋11".to_string(),
        customer_name: "测试用户甲".to_string(),
        session_id: "61736334897".to_string(),
        item_id: "1054950360553".to_string(),
        item_title: "苹果x 256G 白色 国行双卡 插卡即用".to_string(),
        item_pic: "https://img.example/item.png".to_string(),
        item_price: "799".to_string(),
        feishu_chat_name: "测试用户甲｜苹果x 256G 白色".to_string(),
        status: "active".to_string(),
    }
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
            ]),
            true,
        )
        .await
        .unwrap()
        .id
}

async fn seed_account(state: &AppState, uid: &str, owner_username: &str, nick: &str) {
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
    .bind(nick)
    .execute(state.storage().accounts())
    .await
    .unwrap();
}

async fn seed_group_mapping(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    uid: &str,
    session_id: &str,
    chat_id: &str,
) {
    state
        .storage()
        .upsert_message_live_state(
            uid,
            session_id,
            1,
            "buyer-a",
            "测试用户甲",
            "1054950360553",
            "苹果x 256G 白色 国行双卡 插卡即用",
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE message_live_state SET latest_version = 1, last_message_ts = 1 WHERE uid = ? AND session_id = ?",
    )
    .bind(uid)
    .bind(session_id)
    .execute(state.storage().live_state())
    .await
    .unwrap();
    state
        .storage()
        .upsert_feishu_session_group_chat(
            channel_id,
            owner_username,
            uid,
            session_id,
            "buyer-a",
            "测试用户甲",
            "1054950360553",
            "苹果x 256G 白色 国行双卡 插卡即用",
            "https://img.example/item.png",
            "799",
            chat_id,
            "测试用户甲｜苹果x 256G 白色",
            "ou_operator_1",
            "active",
            1_719_225_600.0,
        )
        .await
        .unwrap();
}

async fn seed_bound_operator(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    sender_name: &str,
    chat_id: &str,
) {
    state
        .storage()
        .upsert_feishu_user_binding(
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            chat_id,
            "group",
            "om-seed",
            "evt-seed",
            true,
        )
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
        Box::pin(async move {
            Ok(FeishuNativeResponse::success(
                match request.method.as_str() {
                    "message.send" => json!({
                        "status": "sent",
                        "message_id": "om_group_reply",
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
    set_env("JWT_SECRET", "feishu-group-reply-secret");
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
