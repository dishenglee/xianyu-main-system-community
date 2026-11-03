//! Integration test for `prefetch_account_message_history`.
//!
//! Verifies that the startup history prefetch path:
//!   1. calls the upstream session-sync endpoint to discover sessions
//!   2. calls the upstream message-sync endpoint once per session
//!   3. persists the returned messages to `message_history_cache`
//!
//! The upstream is a small stub Axum router that returns canned payloads.
//! This test does not start the RuntimeSupervisor — it calls the helper
//! directly so we can assert on its return value and the side effects.

use std::{
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Bytes,
    extract::{Path, State},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use xianyu_rust_backend_full::{
    config::AppConfig, routes::accounts::prefetch_account_message_history, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Default)]
struct UpstreamState {
    requests: Arc<Mutex<Vec<String>>>,
}

#[tokio::test]
async fn prefetch_populates_message_history_cache() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let upstream = UpstreamState::default();
    let (base_url, shutdown_tx) = spawn_upstream(upstream.clone()).await;
    env::set_var("XIANYU_H5_GATEWAY", format!("{base_url}/h5"));

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let count = prefetch_account_message_history(&state, "uid-prefetch")
        .await
        .expect("prefetch should succeed with a working upstream");

    // Two sessions in the stubbed response; both should be warmed.
    assert_eq!(count, 2, "both sessions should be prefetched");

    // Every session's first page should have landed in message_history_cache.
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let total_rows =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM message_history_cache WHERE uid = ?")
            .bind("uid-prefetch")
            .fetch_one(&live_state_pool)
            .await
            .unwrap();
    assert!(
        total_rows >= 2,
        "expected at least 2 cached messages across the 2 sessions, got {total_rows}"
    );

    // We should hit both PC and WX session-sync to match the runtime list
    // strategy, plus message-sync once per session.
    let requests = upstream.requests.lock().unwrap().clone();
    let session_hits = requests
        .iter()
        .filter(|p| p.contains("session.sync"))
        .count();
    let message_hits = requests
        .iter()
        .filter(|p| p.contains("message.sync"))
        .count();
    assert_eq!(session_hits, 2, "PC and WX sessions synced once each");
    assert!(
        message_hits >= 2,
        "each session should trigger a message sync (got {message_hits})"
    );

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
    Path((api, _version)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    state
        .requests
        .lock()
        .unwrap()
        .push(format!("{api} body_len={}", body.len()));

    match api.as_str() {
        "mtop.taobao.wx.idlemessage.session.sync" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "sessions": [
                    session_stub("sess-A", "买家A", "iPhone 15"),
                    session_stub("sess-B", "买家B", "iPad"),
                ]
            }
        }))
        .into_response(),

        "mtop.taobao.idlemessage.pc.message.sync" | "mtop.taobao.idlemessage.message.sync" => {
            // Stub has to echo back the caller-requested sessionId or the
            // real code's session-mismatch guard rejects the payload.
            let body_str = String::from_utf8_lossy(&body);
            let session_id = if body_str.contains("sess-B") {
                "sess-B"
            } else {
                "sess-A"
            };
            Json(json!({
                "ret": ["SUCCESS::调用成功"],
                "data": {
                    "sessionId": session_id,
                    "type": 1,
                    "hasMore": false,
                    "messages": [message_stub_for(session_id, "msg-pf-1", "在的")]
                }
            }))
            .into_response()
        }

        "mtop.taobao.idlemessage.user.query" => Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "userInfo": { "userId": "buyer-x", "fishNick": "买家" } }
        }))
        .into_response(),

        _ => Json(json!({ "ret": ["SUCCESS::ok"], "data": {} })).into_response(),
    }
}

fn session_stub(session_id: &str, peer_nick: &str, item_title: &str) -> Value {
    json!({
        "session": {
            "sessionId": session_id,
            "sessionType": 1,
            "users": [
                {"userId": "uid-prefetch", "nick": "我", "fishNick": "我"},
                {"userId": "buyer-x", "nick": peer_nick, "fishNick": peer_nick}
            ],
            "attributeMap": { format!("squadName_buyer-x"): item_title },
            "itemInfo": {"itemId": "item-pf"}
        },
        "message": {
            "summary": {
                "summary": "最新消息",
                "unread": 0,
                "ts": 1713330000000_i64,
                "version": 1713330000000_i64,
                "sortIndex": 1713330000000_i64
            }
        }
    })
}

fn message_stub_for(session_id: &str, uuid: &str, text: &str) -> Value {
    json!({
        "messageUuid": uuid,
        "version": 1713330000000_i64,
        "timeStamp": 1713330000000_i64,
        "content": {
            "contentType": 1,
            "text": { "text": text }
        },
        "senderInfo": {"userId": "buyer-x", "nick": "买家", "fishNick": "买家"},
        "sessionInfo": {
            "sessionId": session_id,
            "sessionType": 1,
            "users": [],
            "attributeMap": {},
            "itemInfo": {"itemId": "item-pf"}
        }
    })
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid, nick, token, sid, x_umt, cookies, browser_cookie_jar,
            app_info, status, created_at, updated_at, cookie_refreshed_at,
            last_session_check_at, last_session_ok_at, first_session_fail_at,
            last_session_status, last_session_reason, last_session_evidence,
            extra, security_score, security_score_updated_at, restriction_end,
            coin_balance, violation_count, fish_power, seller_30d_sold
        )
        VALUES (?, ?, ?, ?, '', ?, '[]', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '', ?, -1, 0, '', 0, 0, 0, '')
        "#,
    )
    .bind("uid-prefetch")
    .bind("Prefetch Seller")
    .bind("seed-token_1700000000000")
    .bind("seed-cookie2")
    .bind(
        json!({
            "_m_h5_tk": "seed-token_1700000000000",
            "_m_h5_tk_enc": "seed-enc",
            "cookie2": "seed-cookie2",
            "unb": "uid-prefetch",
            "tracknick": "Prefetch Seller"
        })
        .to_string(),
    )
    .bind(json!({}).to_string())
    .execute(pool)
    .await
    .unwrap();
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
}

fn clear_env() {
    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("ADMIN_USERNAME");
    env::remove_var("ADMIN_PASSWORD");
    env::remove_var("XIANYU_H5_GATEWAY");
}
