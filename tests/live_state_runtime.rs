use std::{env, time::Duration};

use futures_util::{SinkExt, StreamExt};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::Mutex, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use xianyu_rust_backend_full::{
    app::build_app, auth::encode_access_token, config::AppConfig, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[tokio::test]
async fn websocket_browser_connection_does_not_pollute_account_connection_state() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe {
        env::set_var("ADMIN_USERNAME", "compat-admin");
        env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        env::set_var("JWT_SECRET", "ws-test-secret");
    }

    let config = AppConfig::from_env().expect("config loads");
    let token =
        encode_access_token(&config.auth, "compat-admin", "admin", false).expect("jwt encodes");
    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let live_state_pool = connect_live_state_pool(&config).await;
    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws?token={token}"))
        .await
        .expect("valid token should connect");

    socket
        .next()
        .await
        .expect("connected event")
        .expect("connected message");

    sleep(Duration::from_millis(100)).await;

    let connected_row: Option<(String, i64)> = sqlx::query_as(
        "SELECT status, reconnect_attempts FROM live_connection_state WHERE uid = ?",
    )
    .bind("compat-admin")
    .fetch_optional(&live_state_pool)
    .await
    .expect("connection state query should run");
    assert!(
        connected_row.is_none(),
        "browser live websocket must not create an IM account connection row"
    );

    socket.close(None).await.expect("socket closes");
    sleep(Duration::from_millis(150)).await;

    let disconnected_row: Option<(String, f64)> = sqlx::query_as(
        "SELECT status, last_disconnected_at FROM live_connection_state WHERE uid = ?",
    )
    .bind("compat-admin")
    .fetch_optional(&live_state_pool)
    .await
    .expect("connection state query should run");
    assert!(
        disconnected_row.is_none(),
        "browser live websocket must not create an IM account disconnect row"
    );

    server.abort();
    unsafe {
        env::remove_var("ADMIN_USERNAME");
        env::remove_var("APP_DATA_DIR");
        env::remove_var("JWT_SECRET");
    }
}

#[tokio::test]
async fn websocket_focus_updates_message_live_state_row() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe {
        env::set_var("ADMIN_USERNAME", "compat-admin");
        env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        env::set_var("JWT_SECRET", "ws-test-secret");
    }

    let config = AppConfig::from_env().expect("config loads");
    let token =
        encode_access_token(&config.auth, "compat-admin", "admin", false).expect("jwt encodes");
    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let live_state_pool = connect_live_state_pool(&config).await;
    let accounts_pool = connect_accounts_pool(&config).await;
    seed_account(&accounts_pool, "compat-admin", "compat-admin").await;
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id,
            peer_uid, peer_nick, item_id, item_title, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'))
        "#,
    )
    .bind("compat-admin")
    .bind("session-001")
    .bind(1_i64)
    .bind(1_776_390_123_456_i64)
    .bind("trusted-message-001")
    .bind("peer-001")
    .bind("Buyer")
    .bind("item-001")
    .bind("Vintage Lamp")
    .execute(&live_state_pool)
    .await
    .expect("seed trusted focused session");

    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws?token={token}"))
        .await
        .expect("valid token should connect");

    socket
        .next()
        .await
        .expect("connected event")
        .expect("connected message");

    socket
        .send(Message::Text(
            serde_json::json!({
                "cmd": "focus",
                "uid": "compat-admin",
                "sessionId": "session-001",
                "sessionType": 1,
                "peerUid": "peer-001",
                "peerNick": "Buyer",
                "itemId": "item-001",
                "itemTitle": "Vintage Lamp"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("focus sends");

    let focused = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("focus response should arrive")
        .expect("focus event")
        .expect("focus message");
    let focused_text = focused.into_text().expect("focus text");
    assert!(focused_text.contains("\"type\":\"session.focused\""));
    assert!(focused_text.contains("\"sessionId\":\"session-001\""));

    let row: (i64, String, String, String) = sqlx::query_as(
        r#"
        SELECT session_type, peer_uid, peer_nick, item_title
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        "#,
    )
    .bind("compat-admin")
    .bind("session-001")
    .fetch_one(&live_state_pool)
    .await
    .expect("focused session row should exist");
    assert_eq!(row.0, 1);
    assert_eq!(row.1, "peer-001");
    assert_eq!(row.2, "Buyer");
    assert_eq!(row.3, "Vintage Lamp");

    server.abort();
    unsafe {
        env::remove_var("ADMIN_USERNAME");
        env::remove_var("APP_DATA_DIR");
        env::remove_var("JWT_SECRET");
    }
}

async fn spawn_test_server(temp_dir: TempDir) -> (String, tokio::task::JoinHandle<()>, TempDir) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener binds");
    let addr = listener.local_addr().expect("local addr");
    let app = build_app(
        AppState::bootstrap(AppConfig::from_env().expect("config loads"))
            .await
            .expect("state bootstraps"),
    );

    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server runs");
    });

    (format!("ws://{}", addr), server, temp_dir)
}

async fn connect_live_state_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.live_state_db_path.display()
    ))
    .await
    .expect("live_state db connects")
}

async fn connect_accounts_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .expect("accounts db connects")
}

async fn seed_account(pool: &SqlitePool, uid: &str, owner_username: &str) {
    sqlx::query(
        "INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at) VALUES (?, ?, ?, 'active', '', '{}', 1, 1)",
    )
    .bind(uid)
    .bind(owner_username)
    .bind(format!("账号 {uid}"))
    .execute(pool)
    .await
    .expect("seed account");
}
