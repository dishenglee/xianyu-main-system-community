use std::{
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::{Body, Bytes},
    extract::{Path, State},
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "m004-messages-upstream";

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

#[derive(Debug, Clone)]
struct CapturedRequest {
    path: String,
    body: String,
}

#[tokio::test]
async fn message_routes_fetch_upstream_and_persist_live_cache() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let batch = request_json(app.clone(), "/api/messages/pc/sessions/batch?num=30").await;
    assert_eq!(
        batch["accounts"]["uid-upstream"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(batch["accounts"]["uid-upstream"][0]["peerNick"], "买家小王");

    let sessions = request_json(
        app.clone(),
        "/api/messages/pc/sessions?uid=uid-upstream&types=%5B1,3,6%5D&num=30&force=true",
    )
    .await;
    assert_eq!(
        sessions["normalized"]["sessions"].as_array().map(Vec::len),
        Some(2)
    );
    let session_values = sessions["normalized"]["sessions"].as_array().unwrap();
    let keyboard_session = session_values
        .iter()
        .find(|session| session["sessionId"] == "sess-1")
        .expect("pc session should be present after force refresh");
    assert_eq!(keyboard_session["itemTitle"], "罗技键盘");
    assert_eq!(keyboard_session["itemId"], "item-1");
    let camper_session = session_values
        .iter()
        .find(|session| session["sessionId"] == "sess-reminder")
        .expect("wx-only session should be present after force refresh");
    assert_eq!(camper_session["peerNick"], "露营车买家");
    assert_eq!(camper_session["itemId"], "1042662837101");
    assert_eq!(camper_session["itemTitle"], "露营车");

    let history = request_json(
        app.clone(),
        "/api/messages/pc/history?uid=uid-upstream&session_id=sess-1&session_type=1&num=30&ts=0",
    )
    .await;
    assert_eq!(
        history["normalized"]["messages"].as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(history["normalized"]["peerNick"], "买家小王");
    assert_eq!(
        history["normalized"]["messages"][0]["text"],
        "你好，还在吗？"
    );
    assert_eq!(
        history["normalized"]["messages"][1]["imageUrl"],
        "https://img.example/chat.png"
    );

    let session_info = request_json(
        app.clone(),
        "/api/messages/pc/session-info?uid=uid-upstream&session_id=sess-1&session_type=1",
    )
    .await;
    assert_eq!(session_info["peerNick"], "买家小王");
    assert_eq!(session_info["itemTitle"], "罗技键盘");
    assert_eq!(session_info["itemId"], "item-1");
    assert_eq!(session_info["itemPic"], "https://img.example/item.png");

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let session_row = sqlx::query(
        "SELECT latest_version, peer_nick, item_id, item_title, item_pic, summary FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-upstream")
    .bind("sess-1")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(
        session_row.get::<i64, _>("latest_version"),
        1713330002000_i64
    );
    assert_eq!(session_row.get::<String, _>("peer_nick"), "买家小王");
    assert_eq!(session_row.get::<String, _>("item_id"), "item-1");
    assert_eq!(session_row.get::<String, _>("item_title"), "罗技键盘");
    assert_eq!(
        session_row.get::<String, _>("item_pic"),
        "https://img.example/item.png"
    );
    assert_eq!(session_row.get::<String, _>("summary"), "[图片]");

    let history_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM message_history_cache WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-upstream")
    .bind("sess-1")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(history_count, 2);

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idlemessage.pc.session.sync/3.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/"));
    let wx_session_sync = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/")
        .expect("wx session.sync request captured");
    let decoded_wx_body = urlencoding::decode(&wx_session_sync.body).unwrap();
    assert!(decoded_wx_body.contains(r#""sessionTypes":"[1,3,6]""#));
    assert!(decoded_wx_body.contains(r#""fetchNum":30"#));
    assert!(!decoded_wx_body.contains("syncTimestamp"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idlemessage.pc.message.sync/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idlemessage.user.query/1.0/"));

    let _ = shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn session_info_falls_back_to_cached_item_identity_when_history_is_sparse() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;

    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_http_compensate_at, updated_at
        ) VALUES (
            ?, ?, 1, ?, ?, 0,
            ?, ?, '', '', '', ?, ?, ?,
            ?, ?, unixepoch('subsec'), unixepoch('subsec'), unixepoch('subsec')
        )
        "#,
    )
    .bind("uid-upstream")
    .bind("sess-cached")
    .bind(1713330002000_i64)
    .bind("cached-msg-1")
    .bind("buyer-1")
    .bind("买家小王")
    .bind("item-1")
    .bind("罗技键盘")
    .bind("https://img.example/item.png")
    .bind("[图片]")
    .bind(1713330002000_i64)
    .execute(&live_state_pool)
    .await
    .unwrap();

    let session_info = request_json(
        app,
        "/api/messages/pc/session-info?uid=uid-upstream&session_id=sess-cached&session_type=1",
    )
    .await;
    assert_eq!(session_info["peerNick"], "买家小王");
    assert_eq!(session_info["itemId"], "item-1");
    assert_eq!(session_info["itemTitle"], "罗技键盘");
    assert_eq!(session_info["itemPic"], "https://img.example/item.png");

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.idlemessage.pc.message.sync/1.0/"));

    let _ = shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn session_info_resolves_item_id_from_wx_session_attribute_map_string() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let sessions = request_json(
        app.clone(),
        "/api/messages/pc/sessions?uid=uid-upstream&types=%5B1,3,6%5D&num=30&force=true",
    )
    .await;
    let session_values = sessions["normalized"]["sessions"].as_array().unwrap();
    assert!(session_values
        .iter()
        .any(|session| session["sessionId"] == "sess-reminder"));

    let session_info = request_json(
        app,
        "/api/messages/pc/session-info?uid=uid-upstream&session_id=sess-reminder&session_type=1",
    )
    .await;
    assert_eq!(session_info["peerNick"], "露营车买家");
    assert_eq!(session_info["itemId"], "1042662837101");
    assert_eq!(session_info["itemTitle"], "露营车");
    assert_eq!(session_info["itemPic"], "https://img.example/camper.png");

    let captured = upstream.requests.lock().unwrap().clone();
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/"));
    assert!(captured
        .iter()
        .any(|request| request.path == "/h5/mtop.idle.trade.wx.message.headinfo/1.0/"));

    let _ = shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn pc_sessions_force_reconciles_stale_chat_unread_with_latest_history() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_unread_reconcile_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let sessions = request_json(
        app,
        "/api/messages/pc/sessions?uid=uid-upstream&types=%5B1,3,6%5D&num=30&force=true",
    )
    .await;

    let stale_session = sessions["normalized"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["sessionId"] == "sess-stale")
        .expect("stale chat session should be present");
    assert_eq!(stale_session["unread"], 0);

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let unread = sqlx::query_scalar::<_, i64>(
        "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-upstream")
    .bind("sess-stale")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(unread, 0);

    let _ = shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn pc_sessions_force_reconciles_stale_notification_unread_with_latest_history() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) =
        spawn_notification_unread_reconcile_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let sessions = request_json(
        app,
        "/api/messages/pc/sessions?uid=uid-upstream&types=%5B1,3,6%5D&num=30&force=true",
    )
    .await;

    let stale_session = sessions["normalized"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["sessionId"] == "sess-notice-stale")
        .expect("stale notification session should be present");
    assert_eq!(stale_session["unread"], 0);

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let unread = sqlx::query_scalar::<_, i64>(
        "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-upstream")
    .bind("sess-notice-stale")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(unread, 0);

    let _ = shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn live_mark_session_read_uses_pnm_ids_and_clears_cached_incoming_unreads() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_mark_read_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, summary, last_message_ts, updated_at
        ) VALUES (?, ?, 1, 300, 'uuid-3', 3, 'buyer-read', '买家已读', '还卖吗？', 300, unixepoch('subsec'))
        "#,
    )
    .bind("uid-upstream")
    .bind("sess-read")
    .execute(&live_state_pool)
    .await
    .unwrap();

    for (version, message_id, upstream_message_id, is_self, unread_count, preview) in [
        (
            100_i64,
            "uuid-1",
            "pnm-1.PNM",
            0_i64,
            1_i64,
            "700可以出吗？",
        ),
        (200_i64, "uuid-2", "pnm-2.PNM", 0_i64, 1_i64, "现在拉走"),
        (300_i64, "uuid-3", "pnm-3.PNM", 0_i64, 1_i64, "还卖吗？"),
        (301_i64, "uuid-self", "pnm-self.PNM", 1_i64, 5_i64, "好的"),
    ] {
        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 1, ?, ?, ?, ?, 'text', ?, ?, ?, ?, unixepoch('subsec'))
            "#,
        )
        .bind("uid-upstream")
        .bind("sess-read")
        .bind(message_id)
        .bind(upstream_message_id)
        .bind(version)
        .bind(version)
        .bind(preview)
        .bind(preview)
        .bind(is_self)
        .bind(unread_count)
        .execute(&live_state_pool)
        .await
        .unwrap();
    }

    let response = request_post_json(
        app.clone(),
        "/api/messages/session/read",
        json!({
            "uid": "uid-upstream",
            "session_id": "sess-read",
            "msg_id_list": ["uuid-3"],
            "peer_uid": "buyer-read"
        }),
    )
    .await;
    assert_eq!(response["ok"], true);

    let captured = upstream.requests.lock().unwrap().clone();
    let read_request = captured
        .iter()
        .find(|request| request.path == "/h5/mtop.taobao.idlemessage.message.read/1.0/")
        .expect("message.read request should be sent");
    assert!(read_request.body.contains("pnm-1.PNM"));
    assert!(read_request.body.contains("pnm-2.PNM"));
    assert!(read_request.body.contains("pnm-3.PNM"));
    assert!(!read_request.body.contains("uuid-3"));

    let session_unread = sqlx::query_scalar::<_, i64>(
        "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("uid-upstream")
    .bind("sess-read")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(session_unread, 0);

    let incoming_unread = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM message_history_cache WHERE uid = ? AND session_id = ? AND is_self = 0 AND unread_count > 0",
    )
    .bind("uid-upstream")
    .bind("sess-read")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(incoming_unread, 0);

    let outgoing_unread = sqlx::query_scalar::<_, i64>(
        "SELECT unread_count FROM message_history_cache WHERE uid = ? AND session_id = ? AND message_id = 'uuid-self'",
    )
    .bind("uid-upstream")
    .bind("sess-read")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(outgoing_unread, 5);

    let _ = shutdown_tx.send(());
    clear_env();
}

async fn spawn_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn h5_handler(
    State(state): State<UpstreamState>,
    Path((api, version)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let path = format!("/h5/{api}/{version}/");
    let body = String::from_utf8_lossy(&body).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: body.clone(),
    });

    match path.as_str() {
        "/h5/mtop.taobao.idlemessage.pc.session.sync/3.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessions": [
                    {
                        "session": {
                            "sessionId": "sess-1",
                            "sessionType": 1,
                            "users": [
                                {
                                    "userId": "uid-upstream",
                                    "nick": "卖家自己",
                                    "fishNick": "卖家自己",
                                    "logo": "https://img.example/self.png"
                                },
                                {
                                    "userId": "buyer-1",
                                    "nick": "买家小王",
                                    "fishNick": "买家小王",
                                    "logo": "https://img.example/buyer.png"
                                }
                            ],
                            "attributeMap": {
                                "squadName_buyer-1": "罗技键盘"
                            },
                            "itemInfo": {}
                        },
                        "message": {
                            "summary": {
                                "summary": "[图片]",
                                "unread": 2,
                                "ts": 1713330002000_i64,
                                "version": 1713330002000_i64,
                                "sortIndex": 1713330002000_i64
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessions": [
                    {
                        "session": {
                            "sessionId": "sess-1",
                            "sessionType": 1,
                            "users": [
                                {
                                    "userId": "uid-upstream",
                                    "nick": "卖家自己",
                                    "fishNick": "卖家自己",
                                    "logo": "https://img.example/self.png"
                                },
                                {
                                    "userId": "buyer-1",
                                    "nick": "买家小王",
                                    "fishNick": "买家小王",
                                    "logo": "https://img.example/buyer.png"
                                }
                            ],
                            "attributeMap": {
                                "squadName_buyer-1": "罗技键盘"
                            },
                            "itemInfo": {
                                "itemId": "item-1",
                                "mainPic": "https://img.example/item.png"
                            }
                        },
                        "message": {
                            "summary": {
                                "summary": "[图片]",
                                "unread": 2,
                                "ts": 1713330002000_i64,
                                "version": 1713330002000_i64,
                                "sortIndex": 1713330002000_i64
                            }
                        }
                    }
                    ,
                    {
                        "session": {
                            "sessionId": "sess-reminder",
                            "sessionType": 1,
                            "users": [
                                {
                                    "userId": "uid-upstream",
                                    "nick": "卖家自己",
                                    "fishNick": "卖家自己",
                                    "logo": "https://img.example/self.png"
                                },
                                {
                                    "userId": "buyer-reminder",
                                    "nick": "露营车买家",
                                    "fishNick": "露营车买家",
                                    "logo": "https://img.example/buyer-reminder.png"
                                }
                            ],
                            "attributeMap": {
                                "squadName_buyer-reminder": "露营车",
                                "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=buyer-reminder&sid=sess-reminder"
                            },
                            "itemInfo": {}
                        },
                        "message": {
                            "summary": {
                                "summary": "露营车还在吗",
                                "unread": 1,
                                "ts": 1713330003000_i64,
                                "version": 1713330003000_i64,
                                "sortIndex": 1713330003000_i64
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.pc.message.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessionId": if body.contains("sess-reminder") { "sess-reminder" } else { "sess-1" },
                "type": 1,
                "hasMore": false,
                "messages": if body.contains("sess-reminder") {
                    json!([
                        {
                            "messageUuid": "msg-reminder-1",
                            "version": 1713330003000_i64,
                            "timeStamp": 1713330003000_i64,
                            "content": {
                                "contentType": 1,
                                "text": { "text": "露营车还在吗" }
                            },
                            "senderInfo": {
                                "userId": "buyer-reminder",
                                "nick": "露营车买家",
                                "fishNick": "露营车买家",
                                "logo": "https://img.example/buyer-reminder.png"
                            },
                            "sessionInfo": {
                                "sessionId": "sess-reminder",
                                "sessionType": 1,
                                "users": [
                                    {
                                        "userId": "uid-upstream",
                                        "nick": "卖家自己",
                                        "fishNick": "卖家自己",
                                        "logo": "https://img.example/self.png"
                                    },
                                    {
                                        "userId": "buyer-reminder",
                                        "nick": "露营车买家",
                                        "fishNick": "露营车买家",
                                        "logo": "https://img.example/buyer-reminder.png"
                                    }
                                ],
                                "attributeMap": {
                                    "squadName_buyer-reminder": "露营车"
                                },
                                "itemInfo": {}
                            }
                        }
                    ])
                } else {
                    json!([
                        {
                            "messageUuid": "msg-1",
                            "version": 1713330001000_i64,
                            "timeStamp": 1713330001000_i64,
                            "content": {
                                "contentType": 1,
                                "text": { "text": "你好，还在吗？" }
                            },
                            "senderInfo": {
                                "userId": "buyer-1",
                                "nick": "买家小王",
                                "fishNick": "买家小王",
                                "logo": "https://img.example/buyer.png"
                            },
                            "sessionInfo": {
                                "sessionId": "sess-1",
                                "sessionType": 1,
                                "users": [
                                    {
                                        "userId": "uid-upstream",
                                        "nick": "卖家自己",
                                        "fishNick": "卖家自己",
                                        "logo": "https://img.example/self.png"
                                    },
                                    {
                                        "userId": "buyer-1",
                                        "nick": "买家小王",
                                        "fishNick": "买家小王",
                                        "logo": "https://img.example/buyer.png"
                                    }
                                ],
                                "attributeMap": {
                                    "squadName_buyer-1": "罗技键盘"
                                },
                                "itemInfo": {}
                            }
                        },
                        {
                            "messageUuid": "msg-2",
                            "version": 1713330002000_i64,
                            "timeStamp": 1713330002000_i64,
                            "content": {
                                "contentType": 2,
                                "image": {
                                    "pics": [
                                        { "url": "https://img.example/chat.png" }
                                    ]
                                }
                            },
                            "senderInfo": {
                                "userId": "uid-upstream",
                                "nick": "卖家自己",
                                "fishNick": "卖家自己",
                                "logo": "https://img.example/self.png"
                            },
                            "sessionInfo": {
                                "sessionId": "sess-1",
                                "sessionType": 1,
                                "users": [
                                    {
                                        "userId": "uid-upstream",
                                        "nick": "卖家自己",
                                        "fishNick": "卖家自己",
                                        "logo": "https://img.example/self.png"
                                    },
                                    {
                                        "userId": "buyer-1",
                                        "nick": "买家小王",
                                        "fishNick": "买家小王",
                                        "logo": "https://img.example/buyer.png"
                                    }
                                ],
                                "attributeMap": {
                                    "squadName_buyer-1": "罗技键盘"
                                },
                                "itemInfo": {}
                            }
                        }
                    ])
                }
            }
        }))
        .into_response(),
        "/h5/mtop.idle.trade.wx.message.headinfo/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "messageHeadVO": {
                    "title": if body.contains("1042662837101") { "399.00" } else { "1,299.00" },
                    "semanticsDesc": if body.contains("1042662837101") { "露营车" } else { "罗技键盘" },
                    "picUrl": if body.contains("1042662837101") { "https://img.example/camper.png" } else { "https://img.example/item.png" }
                },
                "commonData": {
                    "itemId": if body.contains("1042662837101") { "1042662837101" } else { "item-1" },
                    "itemPreInfo": {
                        "itemId": if body.contains("1042662837101") { "1042662837101" } else { "item-1" },
                        "title": if body.contains("1042662837101") { "露营车" } else { "罗技键盘" },
                        "imageInfos": [
                            {
                                "url": if body.contains("1042662837101") { "https://img.example/camper.png" } else { "https://img.example/item.png" }
                            }
                        ]
                    }
                }
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.user.query/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "userInfo": {
                    "userId": if body.contains("buyer-reminder") { "buyer-reminder" } else { "buyer-1" },
                    "fishNick": if body.contains("buyer-reminder") { "露营车买家" } else { "买家小王" },
                    "logo": if body.contains("buyer-reminder") { "https://img.example/buyer-reminder.png" } else { "https://img.example/buyer.png" }
                }
            }
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn spawn_unread_reconcile_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_unread_reconcile_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn spawn_notification_unread_reconcile_upstream(
    state: UpstreamState,
) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route(
            "/h5/:api/:version/",
            post(h5_notification_unread_reconcile_handler),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn spawn_mark_read_upstream(state: UpstreamState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/", post(h5_mark_read_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn h5_unread_reconcile_handler(
    State(state): State<UpstreamState>,
    Path((api, version)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let path = format!("/h5/{api}/{version}/");
    let body = String::from_utf8_lossy(&body).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: body.clone(),
    });

    match path.as_str() {
        "/h5/mtop.taobao.idlemessage.pc.session.sync/3.0/"
        | "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessions": [
                    {
                        "session": {
                            "sessionId": "sess-stale",
                            "sessionType": 1,
                            "users": [
                                {
                                    "userId": "uid-upstream",
                                    "nick": "卖家自己",
                                    "fishNick": "卖家自己"
                                },
                                {
                                    "userId": "buyer-stale",
                                    "nick": "旧买家",
                                    "fishNick": "旧买家"
                                }
                            ],
                            "attributeMap": {
                                "squadName_buyer-stale": "旧聊天"
                            },
                            "itemInfo": {
                                "itemId": "item-stale"
                            }
                        },
                        "message": {
                            "summary": {
                                "summary": "这条在官方已经读掉",
                                "unread": 1,
                                "ts": 1713330002000_i64,
                                "version": 1713330002000_i64,
                                "sortIndex": 1713330002000_i64
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.pc.message.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessionId": "sess-stale",
                "type": 1,
                "hasMore": false,
                "messages": [
                    {
                        "messageUuid": "msg-stale-latest",
                        "version": 10002,
                        "timeStamp": 1713330002000_i64,
                        "unreadCount": 0,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "这条在官方已经读掉" }
                        },
                        "senderInfo": {
                            "userId": "buyer-stale",
                            "nick": "旧买家",
                            "fishNick": "旧买家"
                        },
                        "sessionInfo": {
                            "sessionId": "sess-stale",
                            "sessionType": 1,
                            "users": [
                                {
                                    "userId": "uid-upstream",
                                    "nick": "卖家自己",
                                    "fishNick": "卖家自己"
                                },
                                {
                                    "userId": "buyer-stale",
                                    "nick": "旧买家",
                                    "fishNick": "旧买家"
                                }
                            ],
                            "attributeMap": {
                                "squadName_buyer-stale": "旧聊天"
                            },
                            "itemInfo": {
                                "itemId": "item-stale"
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.user.query/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "userInfo": {
                    "userId": "buyer-stale",
                    "fishNick": "旧买家",
                    "logo": "https://img.example/buyer-stale.png"
                }
            }
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn h5_notification_unread_reconcile_handler(
    State(state): State<UpstreamState>,
    Path((api, version)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let path = format!("/h5/{api}/{version}/");
    let body = String::from_utf8_lossy(&body).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: body.clone(),
    });

    match path.as_str() {
        "/h5/mtop.taobao.idlemessage.pc.session.sync/3.0/"
        | "/h5/mtop.taobao.wx.idlemessage.session.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessions": [
                    {
                        "session": {
                            "sessionId": "sess-notice-stale",
                            "sessionType": 3,
                            "ownerInfo": {
                                "userId": "uid-upstream",
                                "nick": "卖家自己",
                                "fishNick": "卖家自己"
                            },
                            "userInfo": {
                                "userId": "notice-center",
                                "nick": "系统消息",
                                "fishNick": "系统消息"
                            }
                        },
                        "message": {
                            "summary": {
                                "summary": "有人@我，快来0.01元兑好礼🧧",
                                "unread": 2,
                                "ts": 1782173948000_i64,
                                "version": 1782173948000_i64,
                                "sortIndex": 1782173948000_i64
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.pc.message.sync/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessionId": "sess-notice-stale",
                "type": 3,
                "hasMore": false,
                "messages": [
                    {
                        "messageUuid": "notice-msg-latest",
                        "version": 1782173948000_i64,
                        "timeStamp": 1782173948822_i64,
                        "unreadCount": 0,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "有人@我，快来0.01元兑好礼🧧" }
                        },
                        "senderInfo": {
                            "userId": "notice-center",
                            "nick": "系统消息",
                            "fishNick": "系统消息"
                        },
                        "sessionInfo": {
                            "sessionId": "sess-notice-stale",
                            "sessionType": 3,
                            "ownerInfo": {
                                "userId": "uid-upstream",
                                "nick": "卖家自己",
                                "fishNick": "卖家自己"
                            },
                            "userInfo": {
                                "userId": "notice-center",
                                "nick": "系统消息",
                                "fishNick": "系统消息"
                            }
                        }
                    }
                ]
            }
        }))
        .into_response(),
        "/h5/mtop.taobao.idlemessage.user.query/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "userInfo": {
                    "userId": "notice-center",
                    "fishNick": "系统消息",
                    "logo": "https://img.example/system.png"
                }
            }
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn h5_mark_read_handler(
    State(state): State<UpstreamState>,
    Path((api, version)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let path = format!("/h5/{api}/{version}/");
    let body = String::from_utf8_lossy(&body).to_string();
    state.requests.lock().unwrap().push(CapturedRequest {
        path: path.clone(),
        body: body.clone(),
    });

    match path.as_str() {
        "/h5/mtop.taobao.idlemessage.message.read/1.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {}
        }))
        .into_response(),
        _ => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

async fn request_json(app: Router, uri: &str) -> Value {
    let response = app
        .oneshot(
            Request::get(uri)
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    if status != StatusCode::OK {
        let payload = json_body(response).await;
        panic!("request {uri} returned {status}: {payload}");
    }
    json_body(response).await
}

async fn request_post_json(app: Router, uri: &str, payload: Value) -> Value {
    let response = app
        .oneshot(
            Request::post(uri)
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    if status != StatusCode::OK {
        let payload = json_body(response).await;
        panic!("request {uri} returned {status}: {payload}");
    }
    json_body(response).await
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid,
            nick,
            token,
            sid,
            x_umt,
            cookies,
            browser_cookie_jar,
            app_info,
            status,
            created_at,
            updated_at,
            cookie_refreshed_at,
            last_session_check_at,
            last_session_ok_at,
            first_session_fail_at,
            last_session_status,
            last_session_reason,
            last_session_evidence,
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, ?, '', ?, '[]', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '', ?, -1, 0, '', 0, 0, 0, '')
        "#,
    )
    .bind("uid-upstream")
    .bind("Seller Upstream")
    .bind("seed-token_1700000000000")
    .bind("seed-cookie2")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "seed-cookie2",
            "unb": "uid-upstream",
            "tracknick": "Seller Upstream"
        })
        .to_string(),
    )
    .bind(
        json!({
            "avatar": "https://img.example/self.png"
        })
        .to_string(),
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::set_var("API_TOKEN", TEST_BEARER);
}

fn clear_env() {
    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    env::remove_var("API_TOKEN");
    env::remove_var("XIANYU_H5_GATEWAY");
}
