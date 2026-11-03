use std::{env, sync::Mutex, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, time::sleep};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app, auth::encode_access_token, config::AppConfig, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "m003-runtime-token";

#[tokio::test]
async fn runtime_profile_and_notification_binding_delete_are_compatible() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    seed_account(&config, "account-m003", "").await;

    let profile_response = app
        .clone()
        .oneshot(
            Request::get("/api/runtime/profile")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(profile_response.status(), StatusCode::OK);
    let profile_payload = json_body(profile_response).await;
    assert_eq!(profile_payload["state_backend"], "sqlite");

    let channel_id = create_notification_channel(&app).await;
    let binding_id = create_notification_binding(&app, channel_id, "account-m003").await;

    let delete_response = app
        .clone()
        .oneshot(
            Request::delete(format!("/api/message-notifications/{binding_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);
    let delete_payload = json_body(delete_response).await;
    assert_eq!(delete_payload["status"], "ok");

    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn integrated_runtime_flow_exercises_live_notification_and_business_state() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_env("ADMIN_USERNAME", "compat-admin");

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let supervisor = state.start_runtime_supervisor();
    sleep(Duration::from_millis(3400)).await;

    let app = build_app(state.clone());
    let (base_url, server) = spawn_server(app.clone()).await;
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let notifications_pool = connect_pool(&config.sqlite.notifications_db_path).await;
    let live_state_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    seed_account(&config, "account-m003", "compat-admin").await;
    seed_trusted_message_session(&live_state_pool, "account-m003", "session-m003").await;

    let ai_response = app
        .clone()
        .oneshot(
            Request::put("/api/ai-settings/cookie-m003")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "aiEnabled": true,
                        "apiType": "openai",
                        "apiKey": "sk-m003"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ai_response.status(), StatusCode::OK);

    let channel_id = create_notification_channel(&app).await;
    let _binding_id = create_notification_binding(&app, channel_id, "account-m003").await;

    let token = encode_access_token(&config.auth, "compat-admin", "user", false).unwrap();
    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws?token={token}"))
        .await
        .unwrap();
    socket.next().await.unwrap().unwrap();
    socket
        .send(Message::Text(
            json!({
                "cmd": "focus",
                "uid": "account-m003",
                "sessionId": "session-m003",
                "sessionType": 1,
                "peerUid": "peer-m003",
                "peerNick": "Buyer",
                "itemId": "item-m003",
                "itemTitle": "Camera"
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();
    let focused = socket.next().await.unwrap().unwrap().into_text().unwrap();
    assert!(focused.contains("\"type\":\"session.focused\""));

    let runtime_response = app
        .clone()
        .oneshot(
            Request::get("/runtime")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    assert_eq!(runtime_payload["cache_sync"]["status"], "idle");
    assert_eq!(runtime_payload["delete_runtime"]["runtime_ready"], true);
    assert!(runtime_payload["managers"].as_array().unwrap().len() >= 4);

    let global_ai_row: Option<(i64, String)> =
        sqlx::query_as("SELECT ai_enabled, api_key FROM ai_reply_settings WHERE cookie_id = ?")
            .bind("__global__")
            .fetch_optional(&accounts_pool)
            .await
            .unwrap();
    assert!(global_ai_row.is_none());

    let ai_account_flags: (i64,) =
        sqlx::query_as("SELECT ai_enabled FROM ai_reply_settings WHERE cookie_id = ?")
            .bind("cookie-m003")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(ai_account_flags.0, 1);

    let binding_count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM account_notification_bindings WHERE uid = ?")
            .bind("account-m003")
            .fetch_one(&notifications_pool)
            .await
            .unwrap();
    assert_eq!(binding_count.0, 1);

    let live_row: (String,) = sqlx::query_as(
        "SELECT item_title FROM message_live_state WHERE uid = ? AND session_id = ?",
    )
    .bind("account-m003")
    .bind("session-m003")
    .fetch_one(&live_state_pool)
    .await
    .unwrap();
    assert_eq!(live_row.0, "Camera");

    socket.close(None).await.unwrap();
    server.abort();
    supervisor.shutdown_and_wait().await;
    clear_env();
}

async fn create_notification_channel(app: &axum::Router) -> i64 {
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/notification-channels")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "M003 Alerts",
                        "type": "feishu",
                        "config": {},
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["channel"]["id"].as_i64().unwrap()
}

async fn create_notification_binding(app: &axum::Router, channel_id: i64, uid: &str) -> i64 {
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/api/message-notifications/{uid}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "channel_id": channel_id,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["binding"]["id"].as_i64().unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn spawn_server(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{}", addr), server)
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

async fn seed_account(config: &AppConfig, uid: &str, owner_username: &str) {
    let pool = connect_pool(&config.sqlite.accounts_db_path).await;
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
    .execute(&pool)
    .await
    .unwrap();
}

async fn seed_trusted_message_session(pool: &SqlitePool, uid: &str, session_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, latest_message_id,
            peer_uid, peer_nick, item_id, item_title, updated_at
        )
        VALUES (?, ?, 1, ?, ?, ?, ?, ?, ?, unixepoch('subsec'))
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(1_776_390_123_456_i64)
    .bind("m003-trusted-message")
    .bind("peer-m003")
    .bind("Buyer")
    .bind("item-m003")
    .bind("Camera")
    .execute(pool)
    .await
    .unwrap();
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "m003-runtime-secret");
    set_env("API_TOKEN", TEST_BEARER);
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("API_TOKEN");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
