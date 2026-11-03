use std::{env, time::Duration};

use futures_util::{SinkExt, StreamExt};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::Mutex};
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
};
use xianyu_rust_backend_full::{
    app::build_app,
    config::AppConfig,
    license_runtime::{set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[tokio::test]
async fn websocket_connects_without_user_credentials() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe { env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap()) };
    unsafe { env::set_var("JWT_SECRET", "ws-test-secret") };

    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws"))
        .await
        .expect("local websocket should not require a product token");
    let connected = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(connected.contains("\"username\":\"local-app\""));
    socket
        .send(Message::Text("{\"cmd\":\"ping\"}".into()))
        .await
        .unwrap();
    let pong = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(pong.contains("\"type\":\"pong\""));
    socket.close(None).await.unwrap();

    server.abort();
    unsafe { env::remove_var("APP_DATA_DIR") };
    unsafe { env::remove_var("JWT_SECRET") };
}

#[tokio::test]
async fn websocket_ignores_legacy_query_tokens() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe {
        env::set_var("ADMIN_USERNAME", "compat-admin");
        env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        env::set_var("JWT_SECRET", "ws-test-secret");
    }

    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws?token=not-a-real-token"))
        .await
        .expect("valid token should connect");

    let connected = socket
        .next()
        .await
        .expect("connected event")
        .expect("connected message");
    let connected_text = connected.into_text().expect("connected text");
    assert!(connected_text.contains("\"type\":\"connection.connected\""));
    assert!(connected_text.contains("\"username\":\"local-app\""));

    socket
        .send(Message::Text("{\"cmd\":\"ping\"}".into()))
        .await
        .expect("ping sends");

    let pong = socket
        .next()
        .await
        .expect("pong event")
        .expect("pong message");
    let pong_text = pong.into_text().expect("pong text");
    assert!(pong_text.contains("\"type\":\"pong\""));

    socket.close(None).await.expect("socket closes");
    server.abort();
    unsafe {
        env::remove_var("ADMIN_USERNAME");
        env::remove_var("APP_DATA_DIR");
        env::remove_var("JWT_SECRET");
    }
}

#[tokio::test]
async fn websocket_subscribe_receives_persisted_connection_snapshots() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe {
        env::set_var("ADMIN_USERNAME", "compat-admin");
        env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        env::set_var("JWT_SECRET", "ws-test-secret");
    }

    let config = AppConfig::from_env().expect("config loads");
    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let accounts_pool = connect_accounts_pool(&config).await;
    let live_state_pool = connect_live_state_pool(&config).await;
    seed_account(&accounts_pool, "seller-001", "compat-admin").await;

    sqlx::query(
        r#"
        INSERT INTO live_connection_state (uid, status, last_connected_at, last_disconnected_at, last_error, reconnect_attempts, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("seller-001")
    .bind("connected")
    .bind(1710000000.0_f64)
    .bind(0.0_f64)
    .bind("")
    .bind(0_i64)
    .bind(1710000000.0_f64)
    .execute(&live_state_pool)
    .await
    .expect("seed connection state");

    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws"))
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
                "cmd": "subscribe",
                "uids": ["seller-001"]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("subscribe sends");

    let snapshot = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("snapshot response should arrive")
        .expect("snapshot event")
        .expect("snapshot message");
    let snapshot_text = snapshot.into_text().expect("snapshot text");
    assert!(snapshot_text.contains("\"type\":\"connection.connected\""));
    assert!(snapshot_text.contains("\"uid\":\"seller-001\""));
    assert!(snapshot_text.contains("\"snapshot\":true"));

    socket.close(None).await.expect("socket closes");
    server.abort();
    unsafe {
        env::remove_var("ADMIN_USERNAME");
        env::remove_var("APP_DATA_DIR");
        env::remove_var("JWT_SECRET");
    }
}

#[tokio::test]
async fn websocket_bootstrap_token_receives_all_account_snapshots() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe {
        env::set_var("ADMIN_USERNAME", "compat-admin");
        env::set_var("APP_BEARER_TOKEN", "bootstrap-test-token");
        env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        env::set_var("JWT_SECRET", "ws-test-secret");
    }

    let config = AppConfig::from_env().expect("config loads");
    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let accounts_pool = connect_accounts_pool(&config).await;
    let live_state_pool = connect_live_state_pool(&config).await;

    seed_account(
        &accounts_pool,
        "seller-owned-by-real-user",
        "real-license-user",
    )
    .await;
    seed_live_connection_state(&live_state_pool, "seller-owned-by-real-user").await;

    let (mut socket, _) =
        connect_async(format!("{base_url}/api/live/ws"))
            .await
            .expect("bootstrap token should connect");

    socket
        .next()
        .await
        .expect("connected event")
        .expect("connected message");

    socket
        .send(Message::Text(
            serde_json::json!({
                "cmd": "subscribe"
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("subscribe sends");

    let snapshot = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("bootstrap snapshot should arrive")
        .expect("snapshot event")
        .expect("snapshot message");
    let snapshot_text = snapshot.into_text().expect("snapshot text");
    assert!(snapshot_text.contains("\"uid\":\"seller-owned-by-real-user\""));
    assert!(snapshot_text.contains("\"snapshot\":true"));

    socket.close(None).await.expect("socket closes");
    server.abort();
    unsafe {
        env::remove_var("ADMIN_USERNAME");
        env::remove_var("APP_BEARER_TOKEN");
        env::remove_var("APP_DATA_DIR");
        env::remove_var("JWT_SECRET");
    }
}

#[tokio::test]
async fn websocket_single_user_subscription_includes_all_local_uids() {
    let _guard = ENV_LOCK.lock().await;
    let temp_dir = TempDir::new().unwrap();

    unsafe {
        env::set_var("ADMIN_USERNAME", "compat-admin");
        env::set_var("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        env::set_var("JWT_SECRET", "ws-test-secret");
    }

    let config = AppConfig::from_env().expect("config loads");
    let (base_url, server, _temp_dir) = spawn_test_server(temp_dir).await;
    let accounts_pool = connect_accounts_pool(&config).await;
    let live_state_pool = connect_live_state_pool(&config).await;

    seed_account(&accounts_pool, "uid-a", "user-a").await;
    seed_account(&accounts_pool, "uid-b", "user-b").await;
    seed_live_connection_state(&live_state_pool, "uid-a").await;
    seed_live_connection_state(&live_state_pool, "uid-b").await;

    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws"))
        .await
        .expect("regular user token should connect");

    socket
        .next()
        .await
        .expect("connected event")
        .expect("connected message");

    socket
        .send(Message::Text(
            serde_json::json!({
                "cmd": "subscribe",
                "uids": ["uid-a", "uid-b"]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("subscribe sends");

    let snapshot = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("own snapshot should arrive")
        .expect("snapshot event")
        .expect("snapshot message");
    let first = snapshot.into_text().expect("snapshot text");
    let second = tokio::time::timeout(Duration::from_secs(1), socket.next())
        .await
        .expect("second local snapshot should arrive")
        .expect("snapshot event")
        .expect("snapshot message")
        .into_text()
        .expect("snapshot text");
    let snapshots = format!("{first}{second}");
    assert!(snapshots.contains("\"uid\":\"uid-a\""));
    assert!(snapshots.contains("\"uid\":\"uid-b\""));

    socket.close(None).await.expect("socket closes");
    server.abort();
    unsafe {
        env::remove_var("ADMIN_USERNAME");
        env::remove_var("APP_DATA_DIR");
        env::remove_var("JWT_SECRET");
    }
}

async fn spawn_test_server(temp_dir: TempDir) -> (String, tokio::task::JoinHandle<()>, TempDir) {
    set_sign_license_context(Some(SignLicenseContext {
        server_url: "https://license.example.test".to_string(),
        auth_token: "test-auth-token".to_string(),
        signed_token: "test-signed-token".to_string(),
        license_id: 1,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "test-device".to_string(),
    }));
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

async fn connect_accounts_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .expect("accounts db connects")
}

async fn connect_live_state_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.live_state_db_path.display()
    ))
    .await
    .expect("live_state db connects")
}

async fn seed_account(pool: &SqlitePool, uid: &str, owner_username: &str) {
    sqlx::query(
        "INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at) VALUES (?, ?, ?, 'active', '', '{}', 1, 1)",
    )
    .bind(uid)
    .bind(owner_username)
    .bind(uid)
    .execute(pool)
    .await
    .expect("seed account");
}

async fn seed_live_connection_state(pool: &SqlitePool, uid: &str) {
    sqlx::query(
        r#"
        INSERT INTO live_connection_state (uid, status, last_connected_at, last_disconnected_at, last_error, reconnect_attempts, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(uid)
    .bind("connected")
    .bind(1710000000.0_f64)
    .bind(0.0_f64)
    .bind("")
    .bind(0_i64)
    .bind(1710000000.0_f64)
    .execute(pool)
    .await
    .expect("seed connection state");
}
