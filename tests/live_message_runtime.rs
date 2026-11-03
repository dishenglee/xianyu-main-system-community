use std::{collections::BTreeSet, env, time::Duration};

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex, Notify},
};
use tokio_tungstenite::{accept_async, connect_async, tungstenite::Message};
use xianyu_rust_backend_full::{
    app::build_app,
    auth::encode_access_token,
    config::AppConfig,
    license_runtime::{set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_broadcasts_incoming_live_messages_to_subscribed_clients() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "sessionId": "sess-live-incoming",
        "sessionType": 1,
        "messageUuid": "msg-live-incoming",
        "version": 1_776_390_000_001_i64,
        "timeStamp": 1_776_390_000_001_i64,
        "content": {
            "contentType": 1,
            "text": { "text": "你好，这个还在吗？" }
        },
        "senderInfo": {
            "userId": "buyer-1001",
            "nick": "咨询买家",
            "fishNick": "咨询买家",
            "logo": "https://img.example/buyer.png"
        },
        "sessionInfo": {
            "sessionId": "sess-live-incoming",
            "sessionType": 1,
            "users": [
                {
                    "userId": "seller-live",
                    "nick": "卖家自己",
                    "fishNick": "卖家自己",
                    "logo": "https://img.example/self.png"
                },
                {
                    "userId": "buyer-1001",
                    "nick": "咨询买家",
                    "fishNick": "咨询买家",
                    "logo": "https://img.example/buyer.png"
                }
            ],
            "itemInfo": {
                "itemId": "item-1001",
                "title": "复古台灯",
                "mainPic": "https://img.example/item.png",
                "sellerInfo": {
                    "userId": "seller-live"
                }
            },
            "attributeMap": {
                "squadName_buyer-1001": "复古台灯"
            }
        }
    });

    let event =
        run_live_runtime_case(&temp_dir, "seller-live", sync_message, "message.received").await;

    assert_eq!(event["uid"], "seller-live");
    assert_eq!(event["sessionId"], "sess-live-incoming");
    assert_eq!(event["peerUid"], "buyer-1001");
    assert_eq!(event["peerNick"], "咨询买家");
    assert_eq!(event["peerRole"], "buyer");
    assert_eq!(event["selfRole"], "seller");
    assert_eq!(event["itemId"], "item-1001");
    assert_eq!(event["itemTitle"], "复古台灯");
    assert_eq!(event["text"], "你好，这个还在吗？");
    assert_eq!(event["isSelf"], false);

    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_marks_buyer_outgoing_messages_as_self_and_preserves_roles() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "sessionId": "sess-live-outgoing",
        "sessionType": 1,
        "messageUuid": "msg-live-outgoing",
        "version": 1_776_390_000_099_i64,
        "timeStamp": 1_776_390_000_099_i64,
        "content": {
            "contentType": 1,
            "text": { "text": "我想买，能包邮吗？" }
        },
        "senderInfo": {
            "userId": "buyer-live",
            "nick": "我自己",
            "fishNick": "我自己",
            "logo": "https://img.example/self.png"
        },
        "sessionInfo": {
            "sessionId": "sess-live-outgoing",
            "sessionType": 1,
            "users": [
                {
                    "userId": "buyer-live",
                    "nick": "我自己",
                    "fishNick": "我自己",
                    "logo": "https://img.example/self.png"
                },
                {
                    "userId": "seller-2002",
                    "nick": "卖家老王",
                    "fishNick": "卖家老王",
                    "logo": "https://img.example/seller.png"
                }
            ],
            "itemInfo": {
                "itemId": "item-2002",
                "title": "二手显示器",
                "mainPic": "https://img.example/item-2002.png",
                "sellerInfo": {
                    "userId": "seller-2002"
                }
            },
            "attributeMap": {
                "squadName_seller-2002": "二手显示器"
            }
        }
    });

    let event = run_live_runtime_case(&temp_dir, "buyer-live", sync_message, "message.sent").await;

    assert_eq!(event["uid"], "buyer-live");
    assert_eq!(event["sessionId"], "sess-live-outgoing");
    assert_eq!(event["peerUid"], "seller-2002");
    assert_eq!(event["peerNick"], "卖家老王");
    assert_eq!(event["peerRole"], "seller");
    assert_eq!(event["selfRole"], "buyer");
    assert_eq!(event["text"], "我想买，能包邮吗？");
    assert_eq!(event["isSelf"], true);

    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_processes_all_messages_from_one_user_message_models_frame() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "body": {
            "userMessageModels": [
                {
                    "message": {
                        "cid": "61826145707@goofish",
                        "sessionType": 1,
                        "messageId": "msg-chen-1.PNM",
                        "messageUuid": "msg-chen-1",
                        "createAt": 1_779_511_307_168_i64,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "露营车还在吗" }
                        },
                        "sender": {
                            "uid": "peer-chen@goofish",
                            "nick": "Chen"
                        },
                        "extension": {
                            "sessionType": 1
                        }
                    }
                },
                {
                    "message": {
                        "cid": "61696947021@goofish",
                        "sessionType": 1,
                        "messageId": "msg-haishen-1.PNM",
                        "messageUuid": "msg-haishen-1",
                        "createAt": 1_779_511_307_169_i64,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "这个露营车最低多少" }
                        },
                        "sender": {
                            "uid": "peer-haishen@goofish",
                            "nick": "南唐安静的海参花"
                        },
                        "extension": {
                            "sessionType": 1
                        }
                    }
                }
            ]
        }
    });

    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws(sync_message).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": ["seller-live"]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let supervisor = state.start_runtime_supervisor();
    let events =
        read_events_of_type(&mut socket, "message.received", 2, Duration::from_secs(6)).await;

    let session_ids = events
        .iter()
        .map(|event| event["sessionId"].as_str().unwrap_or_default().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        session_ids,
        BTreeSet::from(["61696947021".to_string(), "61826145707".to_string(),])
    );

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let live_rows = sqlx::query(
        "SELECT session_id, peer_nick, summary FROM message_live_state \
         WHERE uid = ? ORDER BY session_id ASC",
    )
    .bind("seller-live")
    .fetch_all(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(live_rows.len(), 2);
    assert_eq!(live_rows[0].get::<String, _>("session_id"), "61696947021");
    assert_eq!(
        live_rows[0].get::<String, _>("peer_nick"),
        "南唐安静的海参花"
    );
    assert_eq!(
        live_rows[0].get::<String, _>("summary"),
        "这个露营车最低多少"
    );
    assert_eq!(live_rows[1].get::<String, _>("session_id"), "61826145707");
    assert_eq!(live_rows[1].get::<String, _>("peer_nick"), "Chen");
    assert_eq!(live_rows[1].get::<String, _>("summary"), "露营车还在吗");

    let history_rows = sqlx::query(
        "SELECT session_id, text FROM message_history_cache WHERE uid = ? ORDER BY session_id ASC",
    )
    .bind("seller-live")
    .fetch_all(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(history_rows.len(), 2);
    assert_eq!(
        history_rows[0].get::<String, _>("session_id"),
        "61696947021"
    );
    assert_eq!(
        history_rows[0].get::<String, _>("text"),
        "这个露营车最低多少"
    );
    assert_eq!(
        history_rows[1].get::<String, _>("session_id"),
        "61826145707"
    );
    assert_eq!(history_rows[1].get::<String, _>("text"), "露营车还在吗");

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_persists_session_arouse_metadata_with_activity_timestamp() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "chatType": 1,
        "incrementType": 1,
        "operation": {
            "content": {
                "contentType": 8,
                "sessionArouse": {
                    "sessionArouseInfo": {
                        "arouseTimeStamp": 1_776_390_123_456_i64
                    }
                }
            },
            "sessionInfo": {
                "sessionId": "sess-arouse-only",
                "sessionType": 1,
                "createTime": 1_776_390_120_000_i64,
                "extensions": {
                    "ownerUserId": "seller-live",
                    "extUserId": "buyer-arouse",
                    "itemSellerId": "seller-live",
                    "itemId": "item-arouse",
                    "itemTitle": "西数机械硬盘",
                    "itemMainPic": "https://img.example/disk.png",
                    "squadName_seller-live": "西数机械硬盘"
                }
            }
        },
        "sessionId": "sess-arouse-only"
    });

    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws(sync_message).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url.clone());
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    let supervisor = state.start_runtime_supervisor();

    tokio::time::sleep(Duration::from_secs(2)).await;

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let row = sqlx::query(
        "SELECT latest_version, last_message_ts, peer_uid, item_id, item_title \
         FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("seller-live")
    .bind("sess-arouse-only")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();

    assert_eq!(row.get::<i64, _>("latest_version"), 1_776_390_123_456_i64);
    assert_eq!(row.get::<i64, _>("last_message_ts"), 1_776_390_123_456_i64);
    assert_eq!(row.get::<String, _>("peer_uid"), "buyer-arouse");
    assert_eq!(row.get::<String, _>("item_id"), "item-arouse");
    assert_eq!(row.get::<String, _>("item_title"), "西数机械硬盘");

    supervisor.shutdown_and_wait().await;
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_session_arouse_does_not_bump_existing_session_activity() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "chatType": 1,
        "incrementType": 1,
        "operation": {
            "content": {
                "contentType": 8,
                "sessionArouse": {
                    "sessionArouseInfo": {
                        "arouseTimeStamp": 1_999_999_999_999_i64
                    }
                }
            },
            "sessionInfo": {
                "sessionId": "sess-arouse-existing",
                "sessionType": 1,
                "createTime": 1_776_390_120_000_i64,
                "extensions": {
                    "ownerUserId": "seller-live",
                    "extUserId": "buyer-existing",
                    "itemSellerId": "seller-live",
                    "itemId": "item-existing",
                    "itemTitle": "旧门票会话",
                    "itemMainPic": "https://img.example/item-existing.png",
                    "squadName_seller-live": "旧门票会话"
                }
            }
        },
        "sessionId": "sess-arouse-existing"
    });

    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws(sync_message).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url.clone());
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    sqlx::query(
        "INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_http_compensate_at, updated_at
        ) VALUES (
            ?1, ?2, 1, ?3, 'old-msg', 0,
            'buyer-existing', '旧客户', '', '', '', 'item-existing', '旧门票会话', '',
            '旧消息摘要', ?4, unixepoch('subsec'), unixepoch('subsec'), unixepoch('subsec')
        )",
    )
    .bind("seller-live")
    .bind("sess-arouse-existing")
    .bind(1_776_390_123_456_i64)
    .bind(1_776_390_123_456_i64)
    .execute(&live_state_pool)
    .await
    .unwrap();

    let supervisor = state.start_runtime_supervisor();
    tokio::time::sleep(Duration::from_secs(2)).await;

    let row = sqlx::query(
        "SELECT latest_version, last_message_ts, peer_uid, item_title \
         FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("seller-live")
    .bind("sess-arouse-existing")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();

    assert_eq!(row.get::<i64, _>("latest_version"), 1_776_390_123_456_i64);
    assert_eq!(row.get::<i64, _>("last_message_ts"), 1_776_390_123_456_i64);
    assert_eq!(row.get::<String, _>("peer_uid"), "buyer-existing");
    assert_eq!(row.get::<String, _>("item_title"), "旧门票会话");

    supervisor.shutdown_and_wait().await;
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_session_arouse_backfills_latest_history_message() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "chatType": 1,
        "incrementType": 1,
        "operation": {
            "content": {
                "contentType": 8,
                "sessionArouse": {
                    "sessionArouseInfo": {
                        "arouseTimeStamp": 1_776_390_223_456_i64
                    }
                }
            },
            "sessionInfo": {
                "sessionId": "sess-arouse-backfill",
                "sessionType": 1,
                "createTime": 1_776_390_220_000_i64,
                "extensions": {
                    "ownerUserId": "seller-live",
                    "extUserId": "buyer-backfill",
                    "itemSellerId": "seller-live",
                    "itemId": "item-backfill",
                    "itemTitle": "GoMoney 测试商品",
                    "itemMainPic": "https://img.example/gomoney.png",
                    "squadName_seller-live": "GoMoney 测试商品"
                }
            }
        },
        "sessionId": "sess-arouse-backfill"
    });

    let send_state = LiveWsState::default();
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws_with_state(sync_message, send_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": ["seller-live"]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let supervisor = state.start_runtime_supervisor();
    let event = read_until_type(&mut socket, "message.received", Duration::from_secs(6)).await;
    assert_eq!(event["sessionId"], "sess-arouse-backfill");
    assert_eq!(event["peerUid"], "buyer-backfill");
    assert_eq!(event["peerNick"], "补抓买家");
    assert_eq!(event["itemTitle"], "GoMoney 测试商品");
    assert_eq!(event["text"], "还在吗");

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let session_row = sqlx::query(
        "SELECT latest_version, latest_message_id, peer_uid, peer_nick, item_title, summary \
         FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("seller-live")
    .bind("sess-arouse-backfill")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(session_row.get::<String, _>("peer_uid"), "buyer-backfill");
    assert_eq!(session_row.get::<String, _>("peer_nick"), "补抓买家");
    assert_eq!(
        session_row.get::<String, _>("item_title"),
        "GoMoney 测试商品"
    );
    assert_eq!(session_row.get::<String, _>("summary"), "还在吗");
    assert_eq!(
        session_row.get::<i64, _>("latest_version"),
        1_776_390_223_999_i64
    );

    let history_row = sqlx::query(
        "SELECT text, sender_nick, is_self FROM message_history_cache WHERE uid = ? AND session_id = ? ORDER BY version DESC LIMIT 1",
    )
    .bind("seller-live")
    .bind("sess-arouse-backfill")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(history_row.get::<String, _>("text"), "还在吗");
    assert_eq!(history_row.get::<String, _>("sender_nick"), "补抓买家");
    assert_eq!(history_row.get::<i64, _>("is_self"), 0);

    let list_payload = wait_for_upstream_payload(
        &send_state,
        "/r/MessageManager/listUserMessages",
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(list_payload["body"][0], "sess-arouse-backfill@goofish");

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_session_arouse_backfill_keeps_sse_without_notification_channel() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "chatType": 1,
        "incrementType": 1,
        "operation": {
            "content": {
                "contentType": 8,
                "sessionArouse": {
                    "sessionArouseInfo": {
                        "arouseTimeStamp": 1_776_390_223_456_i64
                    }
                }
            },
            "sessionInfo": {
                "sessionId": "sess-arouse-backfill",
                "sessionType": 1,
                "createTime": 1_776_390_220_000_i64,
                "extensions": {
                    "ownerUserId": "seller-live",
                    "extUserId": "buyer-backfill",
                    "itemSellerId": "seller-live",
                    "itemId": "item-backfill",
                    "itemTitle": "GoMoney 测试商品",
                    "itemMainPic": "https://img.example/gomoney.png",
                    "squadName_seller-live": "GoMoney 测试商品"
                }
            }
        },
        "sessionId": "sess-arouse-backfill"
    });

    let send_state = LiveWsState::default();
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws_with_state(sync_message, send_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;

    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": ["seller-live"]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let supervisor = state.start_runtime_supervisor();
    let event = read_until_type(&mut socket, "message.received", Duration::from_secs(6)).await;
    assert_eq!(event["sessionId"], "sess-arouse-backfill");
    assert_eq!(event["text"], "还在吗");

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_session_arouse_backfill_keeps_sse_when_list_messages_mid_mismatches() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "chatType": 1,
        "incrementType": 1,
        "operation": {
            "content": {
                "contentType": 8,
                "sessionArouse": {
                    "sessionArouseInfo": {
                        "arouseTimeStamp": 1_776_390_223_456_i64
                    }
                }
            },
            "sessionInfo": {
                "sessionId": "sess-arouse-backfill",
                "sessionType": 1,
                "createTime": 1_776_390_220_000_i64,
                "extensions": {
                    "ownerUserId": "seller-live",
                    "extUserId": "buyer-backfill",
                    "itemSellerId": "seller-live",
                    "itemId": "item-backfill",
                    "itemTitle": "GoMoney 测试商品",
                    "itemMainPic": "https://img.example/gomoney.png",
                    "squadName_seller-live": "GoMoney 测试商品"
                }
            }
        },
        "sessionId": "sess-arouse-backfill"
    });

    let send_state = LiveWsState::default();
    with_list_messages_response_mid_override(&send_state, "server-mid-mismatch");
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws_with_state(sync_message, send_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;

    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": ["seller-live"]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let supervisor = state.start_runtime_supervisor();
    let event = read_until_type(&mut socket, "message.received", Duration::from_secs(6)).await;
    assert_eq!(event["sessionId"], "sess-arouse-backfill");
    assert_eq!(event["text"], "还在吗");

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_duplicate_live_message_with_same_upstream_id_is_suppressed() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "sessionId": "sess-dup-upstream",
        "sessionType": 1,
        "messageUuid": "msg-dup-runtime-new",
        "messageId": "msg-dup-runtime.PNM",
        "version": 1_776_390_555_000_i64,
        "timeStamp": 1_776_390_555_000_i64,
        "content": {
            "contentType": 1,
            "text": { "text": "这条是旧消息重放" }
        },
        "senderInfo": {
            "userId": "buyer-dup",
            "nick": "重复买家",
            "fishNick": "重复买家",
            "logo": "https://img.example/buyer-dup.png"
        },
        "sessionInfo": {
            "sessionId": "sess-dup-upstream",
            "sessionType": 1,
            "users": [
                {
                    "userId": "seller-live",
                    "nick": "卖家自己",
                    "fishNick": "卖家自己",
                    "logo": "https://img.example/self.png"
                },
                {
                    "userId": "buyer-dup",
                    "nick": "重复买家",
                    "fishNick": "重复买家",
                    "logo": "https://img.example/buyer-dup.png"
                }
            ],
            "itemInfo": {
                "itemId": "item-dup",
                "title": "重复测试商品",
                "mainPic": "https://img.example/item-dup.png",
                "sellerInfo": {
                    "userId": "seller-live"
                }
            },
            "attributeMap": {
                "squadName_seller-live": "重复测试商品"
            }
        }
    });

    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws(sync_message).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_http_compensate_at, updated_at
        ) VALUES (
            ?1, ?2, 1, ?3, ?4, 0,
            'buyer-dup', '重复买家', 'buyer', 'https://img.example/buyer-dup.png', 'seller',
            'item-dup', '重复测试商品', 'https://img.example/item-dup.png',
            '这条是旧消息重放', ?3, unixepoch('subsec'), unixepoch('subsec'), unixepoch('subsec')
        )
        "#,
    )
    .bind("seller-live")
    .bind("sess-dup-upstream")
    .bind(1_776_390_555_000_i64)
    .bind("cached-old-uuid")
    .execute(&live_state_pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO message_history_cache (
            uid, session_id, session_type, message_id, upstream_message_id, version, ts, kind,
            title, text, image_url, image_urls, content_type, preview, sender_nick, sender_logo,
            is_self, card_meta, reply_to_message_id, reply_to_text, cached_at
        ) VALUES (
            ?1, ?2, 1, ?3, ?4, ?5, ?5, 'text',
            '', '这条是旧消息重放', '', '[]', 1, '这条是旧消息重放', '重复买家',
            'https://img.example/buyer-dup.png', 0, '', '', '', unixepoch('subsec')
        )
        "#,
    )
    .bind("seller-live")
    .bind("sess-dup-upstream")
    .bind("cached-old-uuid")
    .bind("msg-dup-runtime.PNM")
    .bind(1_776_390_555_000_i64)
    .execute(&live_state_pool)
    .await
    .unwrap();

    let supervisor = state.start_runtime_supervisor();
    tokio::time::sleep(Duration::from_secs(2)).await;

    supervisor.shutdown_and_wait().await;
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_duplicate_live_message_with_same_upstream_id_is_suppressed_even_when_version_changes(
) {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let sync_message = json!({
        "sessionId": "sess-dup-version-shift",
        "sessionType": 1,
        "messageUuid": "msg-dup-runtime-new-version",
        "messageId": "msg-dup-version.PNM",
        "version": 1_776_390_666_000_i64,
        "timeStamp": 1_776_390_666_000_i64,
        "content": {
            "contentType": 1,
            "text": { "text": "这条旧消息换了版本号" }
        },
        "senderInfo": {
            "userId": "buyer-dup-version",
            "nick": "重复买家2",
            "fishNick": "重复买家2",
            "logo": "https://img.example/buyer-dup-2.png"
        },
        "sessionInfo": {
            "sessionId": "sess-dup-version-shift",
            "sessionType": 1,
            "users": [
                {
                    "userId": "seller-live",
                    "nick": "卖家自己",
                    "fishNick": "卖家自己",
                    "logo": "https://img.example/self.png"
                },
                {
                    "userId": "buyer-dup-version",
                    "nick": "重复买家2",
                    "fishNick": "重复买家2",
                    "logo": "https://img.example/buyer-dup-2.png"
                }
            ],
            "itemInfo": {
                "itemId": "item-dup-2",
                "title": "重复测试商品2",
                "mainPic": "https://img.example/item-dup-2.png",
                "sellerInfo": {
                    "userId": "seller-live"
                }
            },
            "attributeMap": {
                "squadName_seller-live": "重复测试商品2"
            }
        }
    });

    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws(sync_message).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_http_compensate_at, updated_at
        ) VALUES (
            ?1, ?2, 1, ?3, ?4, 0,
            'buyer-dup-version', '重复买家2', 'buyer', 'https://img.example/buyer-dup-2.png', 'seller',
            'item-dup-2', '重复测试商品2', 'https://img.example/item-dup-2.png',
            '这条旧消息换了版本号', ?5, unixepoch('subsec'), unixepoch('subsec'), unixepoch('subsec')
        )
        "#,
    )
    .bind("seller-live")
    .bind("sess-dup-version-shift")
    .bind(1_776_390_555_000_i64)
    .bind("cached-old-uuid-version")
    .bind(1_776_390_555_000_i64)
    .execute(&live_state_pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO message_history_cache (
            uid, session_id, session_type, message_id, upstream_message_id, version, ts, kind,
            title, text, image_url, image_urls, content_type, preview, sender_nick, sender_logo,
            is_self, card_meta, reply_to_message_id, reply_to_text, cached_at
        ) VALUES (
            ?1, ?2, 1, ?3, ?4, ?5, ?6, 'text',
            '', '这条旧消息换了版本号', '', '[]', 1, '这条旧消息换了版本号', '重复买家2',
            'https://img.example/buyer-dup-2.png', 0, '', '', '', unixepoch('subsec')
        )
        "#,
    )
    .bind("seller-live")
    .bind("sess-dup-version-shift")
    .bind("cached-old-uuid-version")
    .bind("msg-dup-version.PNM")
    .bind(1_776_390_555_000_i64)
    .bind(1_776_390_555_000_i64)
    .execute(&live_state_pool)
    .await
    .unwrap();

    let supervisor = state.start_runtime_supervisor();
    tokio::time::sleep(Duration::from_secs(2)).await;

    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM message_history_cache WHERE uid = ? AND session_id = ? AND upstream_message_id = ?",
    )
    .bind("seller-live")
    .bind("sess-dup-version-shift")
    .bind("msg-dup-version.PNM")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);

    supervisor.shutdown_and_wait().await;
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_send_command_returns_send_result() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let send_state = LiveWsState::default();
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws_with_state(json!({}), send_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    seed_trusted_message_session(
        &config,
        "seller-live",
        "sess-send-1",
        "buyer-3001",
        "发送测试买家",
    )
    .await;
    let supervisor = state.start_runtime_supervisor();
    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": ["seller-live"]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    wait_for_connection_snapshot(
        &mut socket,
        "seller-live",
        "connected",
        Duration::from_secs(6),
    )
    .await;

    socket
        .send(Message::Text(
            json!({
                "cmd": "send",
                "uid": "seller-live",
                "sessionId": "sess-send-1",
                "toUid": "buyer-3001",
                "content": "你好，在的"
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let event = read_until_type(&mut socket, "send.result", Duration::from_secs(6)).await;
    assert_eq!(event["ok"], true);
    assert_eq!(event["sessionId"], "sess-send-1");

    let send_payload = wait_for_upstream_payload(
        &send_state,
        "/r/MessageSend/sendByReceiverScope",
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(send_payload["body"][0]["content"]["custom"]["type"], 1);

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_send_image_command_returns_send_image_result() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let send_state = LiveWsState::default();
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws_with_state(json!({}), send_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    seed_trusted_message_session(
        &config,
        "seller-live",
        "sess-send-image-1",
        "buyer-3001",
        "图片测试买家",
    )
    .await;
    let supervisor = state.start_runtime_supervisor();
    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": ["seller-live"]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    wait_for_connection_snapshot(
        &mut socket,
        "seller-live",
        "connected",
        Duration::from_secs(6),
    )
    .await;

    socket
        .send(Message::Text(
            json!({
                "cmd": "send_image",
                "uid": "seller-live",
                "sessionId": "sess-send-image-1",
                "toUid": "buyer-3001",
                "imageUrl": "https://img.alicdn.com/example.png",
                "width": 720,
                "height": 1280
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let event = read_until_type(&mut socket, "send_image.result", Duration::from_secs(6)).await;
    assert_eq!(event["ok"], true);
    assert_eq!(event["sessionId"], "sess-send-image-1");

    let send_payload = wait_for_upstream_payload(
        &send_state,
        "/r/MessageSend/sendByReceiverScope",
        Duration::from_secs(2),
    )
    .await;
    assert_eq!(send_payload["body"][0]["content"]["custom"]["type"], 2);

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_reuses_sync_cursor_on_reconnect_after_sync_push() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let reconnect_state = ReconnectWsState::default();
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_reconnect_probe_ws(reconnect_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    let supervisor = state.start_runtime_supervisor();

    tokio::time::timeout(
        Duration::from_secs(8),
        reconnect_state.second_reg_seen.notified(),
    )
    .await
    .expect("expected worker to reconnect and send second /reg");

    let reg_syncs = reconnect_state.reg_syncs.lock().await.clone();
    assert!(
        reg_syncs.len() >= 2,
        "expected at least two /reg payloads, got {reg_syncs:?}"
    );
    assert_eq!(reg_syncs[0], "0,0;0;0;");
    assert_eq!(reg_syncs[1], "987654321,123;0;0;");

    supervisor.shutdown_and_wait().await;
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_restores_sync_cursor_from_live_connection_state_on_worker_start() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let reconnect_state = ReconnectWsState::default();
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_reconnect_probe_ws(reconnect_state.clone()).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url);
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-live").await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    sqlx::query(
        r#"
        INSERT INTO live_connection_state (
            uid,
            status,
            last_connected_at,
            last_disconnected_at,
            last_error,
            reconnect_attempts,
            updated_at,
            last_pts,
            last_high_pts
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("seller-live")
    .bind("disconnected")
    .bind(0.0_f64)
    .bind(0.0_f64)
    .bind("")
    .bind(0_i64)
    .bind(0.0_f64)
    .bind(111_i64)
    .bind(7_i64)
    .execute(&live_state_pool)
    .await
    .unwrap();

    let supervisor = state.start_runtime_supervisor();

    tokio::time::timeout(
        Duration::from_secs(8),
        reconnect_state.second_reg_seen.notified(),
    )
    .await
    .expect("expected worker to reconnect and send second /reg");

    let reg_syncs = reconnect_state.reg_syncs.lock().await.clone();
    assert!(
        reg_syncs.len() >= 2,
        "expected at least two /reg payloads, got {reg_syncs:?}"
    );
    assert_eq!(reg_syncs[0], "111,7;0;0;");
    assert_eq!(reg_syncs[1], "987654321,123;0;0;");

    let persisted: (i64, i64) =
        sqlx::query_as("SELECT last_pts, last_high_pts FROM live_connection_state WHERE uid = ?")
            .bind("seller-live")
            .fetch_one(&live_state_pool)
            .await
            .unwrap();
    assert_eq!(persisted, (987654321_i64, 123_i64));

    supervisor.shutdown_and_wait().await;
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    clear_env();
}

async fn run_live_runtime_case(
    _temp_dir: &TempDir,
    uid: &str,
    sync_message: Value,
    expected_type: &str,
) -> Value {
    let (gateway_url, shutdown_http) = spawn_upstream().await;
    let (ws_url, shutdown_ws) = spawn_live_ws(sync_message).await;
    env::set_var("XIANYU_WX_GATEWAY", gateway_url.clone());
    env::set_var("XIANYU_WX_WS_URL", ws_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), uid).await;
    let token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_app(state.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{addr}/api/live/ws?token={token}"))
        .await
        .unwrap();
    let _ = socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "subscribe",
                "uids": [uid]
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let supervisor = state.start_runtime_supervisor();
    let event = read_until_type(&mut socket, expected_type, Duration::from_secs(6)).await;

    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let row = sqlx::query(
        "SELECT peer_role, self_role, peer_uid, peer_nick FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind(uid)
    .bind(event["sessionId"].as_str().unwrap())
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(
        row.get::<String, _>("peer_role"),
        event["peerRole"].as_str().unwrap_or_default()
    );
    assert_eq!(
        row.get::<String, _>("self_role"),
        event["selfRole"].as_str().unwrap_or_default()
    );
    assert_eq!(
        row.get::<String, _>("peer_uid"),
        event["peerUid"].as_str().unwrap_or_default()
    );
    assert_eq!(
        row.get::<String, _>("peer_nick"),
        event["peerNick"].as_str().unwrap_or_default()
    );

    supervisor.shutdown_and_wait().await;
    server.abort();
    let _ = shutdown_http.send(());
    let _ = shutdown_ws.send(());
    event
}

async fn read_until_type(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_type: &str,
    wait: Duration,
) -> Value {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next = tokio::time::timeout(remaining, socket.next())
            .await
            .expect("event should arrive before timeout")
            .expect("websocket should stay open")
            .expect("websocket frame should decode");
        let text = next.into_text().unwrap();
        let payload: Value = serde_json::from_str(&text).unwrap();
        if payload.get("type").and_then(Value::as_str) == Some(expected_type) {
            return payload;
        }
    }
}

async fn read_events_of_type(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected_type: &str,
    expected_count: usize,
    wait: Duration,
) -> Vec<Value> {
    let deadline = tokio::time::Instant::now() + wait;
    let mut events = Vec::with_capacity(expected_count);
    while events.len() < expected_count {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next = tokio::time::timeout(remaining, socket.next())
            .await
            .expect("event should arrive before timeout")
            .expect("websocket should stay open")
            .expect("websocket frame should decode");
        let text = next.into_text().unwrap();
        let payload: Value = serde_json::from_str(&text).unwrap();
        if payload.get("type").and_then(Value::as_str) == Some(expected_type) {
            events.push(payload);
        }
    }
    events
}

async fn wait_for_connection_snapshot(
    socket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    uid: &str,
    status: &str,
    wait: Duration,
) -> Value {
    let expected_type = format!("connection.{status}");
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next = tokio::time::timeout(remaining, socket.next())
            .await
            .expect("connection snapshot should arrive before timeout")
            .expect("websocket should stay open")
            .expect("websocket frame should decode");
        let text = next.into_text().unwrap();
        let payload: Value = serde_json::from_str(&text).unwrap();
        if payload.get("type").and_then(Value::as_str) == Some(expected_type.as_str())
            && payload.get("uid").and_then(Value::as_str) == Some(uid)
        {
            return payload;
        }
    }
}

async fn wait_for_upstream_payload(state: &LiveWsState, lwp: &str, wait: Duration) -> Value {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        {
            let payloads = state.sent_payloads.lock().await;
            if let Some(payload) = payloads
                .iter()
                .find(|payload| payload["lwp"].as_str() == Some(lwp))
            {
                return payload.clone();
            }
        }

        if tokio::time::Instant::now() >= deadline {
            panic!("upstream payload {lwp} should be forwarded before timeout");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn spawn_upstream() -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/h5/:api/:version/:minor/", post(h5_handler))
        .route(
            "/api/licenses/runtime-challenge",
            post(runtime_challenge_handler),
        )
        .route(
            "/api/me/xianyu-account-bindings/resolve",
            post(resolve_binding_handler),
        )
        .route("/sign", post(sign_service_handler))
        .route("/gw/:api/:version/", post(app_mtop_handler))
        .with_state(());
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
    let base_url = format!("http://{addr}");
    env::set_var("XIANYU_SIGN_SERVICE_URL", &base_url);
    env::set_var("XIANYU_LICENSE_SERVER_URL", &base_url);
    env::set_var("XIANYU_APP_MTOP_GATEWAY", format!("{base_url}/gw"));
    set_sign_license_context(Some(SignLicenseContext {
        server_url: base_url.clone(),
        auth_token: "runtime-auth-token".to_string(),
        signed_token: "runtime-signed-token".to_string(),
        license_id: 99,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "runtime-device".to_string(),
    }));
    (base_url, shutdown_tx)
}

async fn h5_handler(
    State(()): State<()>,
    Path((api, version, minor)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let path = format!("/h5/{api}/{version}/{minor}/");
    match path.as_str() {
        "/h5/mtop.taobao.idlemessage.wx.login.token/1.0/2.0/" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "accessToken": "runtime-access-token"
            }
        }))
        .into_response(),
        _ => Json(json!({
            "ret": ["FAIL_SYS_NOT_FOUND::not found"],
            "data": {}
        }))
        .into_response(),
    }
}

async fn runtime_challenge_handler() -> impl IntoResponse {
    Json(json!({
        "challenge_id": "runtime-challenge"
    }))
    .into_response()
}

async fn resolve_binding_handler(Json(payload): Json<Value>) -> impl IntoResponse {
    let rows = payload
        .get("account_uids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|uid| {
            json!({
                "account_uid": uid,
                "status": "owned_by_me",
                "owner_username": "test-user",
                "message": ""
            })
        })
        .collect::<Vec<_>>();
    Json(Value::Array(rows)).into_response()
}

async fn sign_service_handler() -> impl IntoResponse {
    Json(json!({
        "signHeaders": {
            "x-sign": "runtime-sign",
            "x-sgext": "runtime-sgext",
            "x-mini-wua": "runtime-mini-wua",
            "x-umt": "runtime-umt"
        },
        "updatedInput": {
            "t": "1770000000"
        },
        "requestHeaders": {
            "x-appkey": "258",
            "x-t": "1770000000"
        }
    }))
    .into_response()
}

async fn app_mtop_handler(Path((api, _version)): Path<(String, String)>) -> impl IntoResponse {
    match api.as_str() {
        "mtop.idle.user.page.my.adapter" => Json(json!({
            "ret": "SUCCESS",
            "data": {
                "base": {
                    "cs": {
                        "score": 785
                    }
                },
                "container": {
                    "sections": [
                        {
                            "sectionBizCode": "trade2026",
                            "item": {
                                "top": {
                                    "exContent": {
                                        "income": "3.46万",
                                        "sold": "131",
                                        "title": "我的交易",
                                        "subTitle": "在闲鱼赚了34670.78元"
                                    },
                                    "itemBizCode": "top",
                                    "valid": "true"
                                }
                            }
                        }
                    ]
                }
            }
        }))
        .into_response(),
        _ => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {}
        }))
        .into_response(),
    }
}

async fn spawn_live_ws(sync_message: Value) -> (String, oneshot::Sender<()>) {
    spawn_live_ws_with_state(sync_message, LiveWsState::default()).await
}

#[derive(Clone, Default)]
struct LiveWsState {
    sent_payloads: std::sync::Arc<tokio::sync::Mutex<Vec<Value>>>,
    list_messages_response_mid_override: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

#[derive(Clone, Default)]
struct ReconnectWsState {
    reg_syncs: std::sync::Arc<tokio::sync::Mutex<Vec<String>>>,
    second_reg_seen: std::sync::Arc<Notify>,
}

async fn spawn_live_ws_with_state(
    sync_message: Value,
    state: LiveWsState,
) -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else {
                        break;
                    };
                    let message = sync_message.clone();
                    let state = state.clone();
                    tokio::spawn(async move {
                        let mut websocket = accept_async(stream).await.unwrap();
                        while let Some(frame) = websocket.next().await {
                            let frame = frame.unwrap();
                            let Message::Text(text) = frame else {
                                continue;
                            };
                            let payload: Value = serde_json::from_str(&text).unwrap();
                            state.sent_payloads.lock().await.push(payload.clone());
                            let lwp = payload.get("lwp").and_then(Value::as_str).unwrap_or_default();
                            let mid = payload
                                .get("headers")
                                .and_then(Value::as_object)
                                .and_then(|headers| headers.get("mid"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();

                            match lwp {
                                "/reg" | "/r/SyncStatus/ackDiff" => {
                                    websocket
                                        .send(Message::Text(json!({
                                            "code": 200,
                                            "headers": {
                                                "mid": mid,
                                                "sid": "runtime-sid"
                                            }
                                        }).to_string()))
                                        .await
                                        .unwrap();
                                    if lwp == "/r/SyncStatus/ackDiff" {
                                        let encoded = base64::engine::general_purpose::STANDARD
                                            .encode(message.to_string());
                                        websocket
                                            .send(Message::Text(json!({
                                                "headers": {
                                                    "mid": "sync-push-runtime",
                                                    "sid": "runtime-sid"
                                                },
                                                "body": {
                                                    "syncPushPackage": {
                                                        "data": [
                                                            { "data": encoded }
                                                        ]
                                                    }
                                                }
                                            }).to_string()))
                                            .await
                                            .unwrap();
                                    }
                                }
                                "/r/MessageManager/listUserMessages" => {
                                    let session_cid = payload["body"][0]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_string();
                                    let session_id = session_cid
                                        .strip_suffix("@goofish")
                                        .unwrap_or(session_cid.as_str())
                                        .to_string();
                                    let body = if session_id == "sess-arouse-backfill" {
                                        json!({
                                            "sessionId": session_id,
                                            "type": 1,
                                            "hasMore": false,
                                            "messages": [{
                                                "messageUuid": "msg-arouse-backfill-1",
                                                "upstreamMessageId": "1776390223999.PNM",
                                                "version": 1_776_390_223_999_i64,
                                                "timeStamp": 1_776_390_223_999_i64,
                                                "content": {
                                                    "contentType": 1,
                                                    "text": { "text": "还在吗" }
                                                },
                                                "senderInfo": {
                                                    "userId": "buyer-backfill",
                                                    "nick": "补抓买家",
                                                    "fishNick": "补抓买家",
                                                    "logo": "https://img.example/buyer-backfill.png"
                                                },
                                                "sessionInfo": {
                                                    "sessionId": session_id,
                                                    "sessionType": 1,
                                                    "users": [
                                                        {
                                                            "userId": "seller-live",
                                                            "nick": "卖家自己",
                                                            "fishNick": "卖家自己"
                                                        },
                                                        {
                                                            "userId": "buyer-backfill",
                                                            "nick": "补抓买家",
                                                            "fishNick": "补抓买家"
                                                        }
                                                    ],
                                                    "itemInfo": {
                                                        "itemId": "item-backfill",
                                                        "title": "GoMoney 测试商品",
                                                        "mainPic": "https://img.example/gomoney.png",
                                                        "sellerInfo": {
                                                            "userId": "seller-live"
                                                        }
                                                    },
                                                    "attributeMap": {
                                                        "squadName_seller-live": "GoMoney 测试商品"
                                                    }
                                                }
                                            }]
                                        })
                                    } else {
                                        json!({
                                            "sessionId": session_id,
                                            "type": 1,
                                            "hasMore": false,
                                            "messages": []
                                        })
                                    };
                                    let response_mid = state
                                        .list_messages_response_mid_override
                                        .lock()
                                        .unwrap()
                                        .clone()
                                        .unwrap_or_else(|| mid.clone());
                                    websocket
                                        .send(Message::Text(json!({
                                            "headers": {
                                                "mid": response_mid,
                                                "sid": "runtime-sid"
                                            },
                                            "body": body
                                        }).to_string()))
                                        .await
                                        .unwrap();
                                }
                                "/r/MessageSend/sendByReceiverScope" => {
                                    websocket
                                        .send(Message::Text(json!({
                                            "headers": {
                                                "mid": mid,
                                                "sid": "runtime-sid"
                                            },
                                            "body": {
                                                "messageId": format!("ack-{mid}"),
                                                "ts": 1_776_390_123_456_i64
                                            }
                                        }).to_string()))
                                        .await
                                        .unwrap();
                                }
                                "/!" => {
                                    websocket
                                        .send(Message::Text(json!({
                                            "code": 200,
                                            "headers": {
                                                "mid": mid,
                                                "sid": "runtime-sid"
                                            }
                                        }).to_string()))
                                        .await
                                        .unwrap();
                                }
                                _ => {}
                            }
                        }
                    });
                }
            }
        }
    });
    (format!("ws://{addr}"), shutdown_tx)
}

fn with_list_messages_response_mid_override(state: &LiveWsState, mid: impl Into<String>) {
    *state.list_messages_response_mid_override.lock().unwrap() = Some(mid.into());
}

async fn spawn_reconnect_probe_ws(state: ReconnectWsState) -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => break,
                accepted = listener.accept() => {
                    let Ok((stream, _)) = accepted else {
                        break;
                    };
                    let state = state.clone();
                    tokio::spawn(async move {
                        let mut websocket = accept_async(stream).await.unwrap();
                        let mut sent_sync_push = false;
                        while let Some(frame) = websocket.next().await {
                            let frame = frame.unwrap();
                            let Message::Text(text) = frame else {
                                continue;
                            };
                            let payload: Value = serde_json::from_str(&text).unwrap();
                            let lwp = payload.get("lwp").and_then(Value::as_str).unwrap_or_default();
                            let mid = payload
                                .get("headers")
                                .and_then(Value::as_object)
                                .and_then(|headers| headers.get("mid"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();

                            match lwp {
                                "/reg" => {
                                    let sync = payload
                                        .get("headers")
                                        .and_then(Value::as_object)
                                        .and_then(|headers| headers.get("sync"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string();
                                    let should_notify = {
                                        let mut reg_syncs = state.reg_syncs.lock().await;
                                        reg_syncs.push(sync);
                                        reg_syncs.len() >= 2
                                    };
                                    websocket
                                        .send(Message::Text(json!({
                                            "code": 200,
                                            "headers": {
                                                "mid": mid,
                                                "sid": "runtime-sid"
                                            }
                                        }).to_string()))
                                        .await
                                        .unwrap();
                                    if should_notify {
                                        state.second_reg_seen.notify_waiters();
                                    }
                                }
                                "/r/SyncStatus/ackDiff" => {
                                    websocket
                                        .send(Message::Text(json!({
                                            "code": 200,
                                            "headers": {
                                                "mid": mid,
                                                "sid": "runtime-sid"
                                            }
                                        }).to_string()))
                                        .await
                                        .unwrap();

                                    if !sent_sync_push {
                                        sent_sync_push = true;
                                        let encoded = base64::engine::general_purpose::STANDARD
                                            .encode(json!({
                                                "sessionId": "sess-reconnect-cursor",
                                                "sessionType": 1,
                                                "messageUuid": "msg-reconnect-cursor",
                                                "version": 1_776_390_555_000_i64,
                                                "timeStamp": 1_776_390_555_000_i64,
                                                "content": {
                                                    "contentType": 1,
                                                    "text": { "text": "重连游标探针" }
                                                },
                                                "senderInfo": {
                                                    "userId": "buyer-reconnect",
                                                    "nick": "买家探针",
                                                    "fishNick": "买家探针"
                                                },
                                                "sessionInfo": {
                                                    "sessionId": "sess-reconnect-cursor",
                                                    "sessionType": 1
                                                }
                                            }).to_string());
                                        websocket
                                            .send(Message::Text(json!({
                                                "headers": {
                                                    "mid": "sync-push-reconnect",
                                                    "sid": "runtime-sid"
                                                },
                                                "body": {
                                                    "syncPushPackage": {
                                                        "maxPts": 987654321_i64,
                                                        "maxHighPts": 123_i64,
                                                        "data": [
                                                            { "data": encoded }
                                                        ]
                                                    }
                                                }
                                            }).to_string()))
                                            .await
                                            .unwrap();
                                    } else {
                                        // Extra ackDiff frames after a live sync push used to
                                        // be required by this fake server, but the real ACCS
                                        // gateway resets sockets when we send them. Keep the
                                        // probe focused on initial/reregister ackDiff only.
                                    }
                                }
                                _ if payload.get("code").and_then(Value::as_i64) == Some(200) => {
                                    if sent_sync_push {
                                        let _ = websocket.close(None).await;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                    });
                }
            }
        }
    });
    (format!("ws://{addr}"), shutdown_tx)
}

async fn seed_account(pool: &SqlitePool, uid: &str) {
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
            owner_username,
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, ?, '', ?, '[]', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '', ?, '{}', -1, 0, '', 0, 0, 0, '')
        "#,
    )
    .bind(uid)
    .bind("Runtime User")
    .bind("seed-token_1700000000000")
    .bind("seed-cookie2")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "seed-cookie2",
            "unb": uid,
            "tracknick": "Runtime User",
            "sgcookie": "seed-sgcookie",
            "csg": "seed-csg"
        })
        .to_string(),
    )
    .bind("test-user")
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_trusted_message_session(
    config: &AppConfig,
    uid: &str,
    session_id: &str,
    peer_uid: &str,
    peer_nick: &str,
) {
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id, unread_count,
            peer_uid, peer_nick, peer_role, peer_logo, self_role, item_id, item_title, item_pic,
            summary, last_message_ts, last_sync_at, last_http_compensate_at, updated_at
        ) VALUES (
            ?1, ?2, 1, ?3, ?4, 0,
            ?5, ?6, 'buyer', '', 'seller', '', '', '',
            '', ?3, unixepoch('subsec'), unixepoch('subsec'), unixepoch('subsec')
        )
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(1_776_390_555_000_i64)
    .bind(format!("{session_id}-seed.PNM"))
    .bind(peer_uid)
    .bind(peer_nick)
    .execute(&live_state_pool)
    .await
    .unwrap();
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    set_sign_license_context(None);
    env::set_var(
        "APP_DATA_DIR",
        temp_dir.path().join("data").display().to_string(),
    );
    env::set_var(
        "XIANYU_LIVE_RUNTIME_LOCK_DIR",
        temp_dir.path().join("runtime-lock").display().to_string(),
    );
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
}

fn clear_env() {
    set_sign_license_context(None);
    env::remove_var("APP_DATA_DIR");
    env::remove_var("XIANYU_LIVE_RUNTIME_LOCK_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    env::remove_var("XIANYU_WX_GATEWAY");
    env::remove_var("XIANYU_WX_WS_URL");
    env::remove_var("XIANYU_SIGN_SERVICE_URL");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    env::remove_var("XIANYU_APP_MTOP_GATEWAY");
}
