use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "m017-business-automation";

#[tokio::test]
async fn account_settings_patch_updates_account_extra_and_ai_flags() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(state.storage().accounts(), "seller-001").await;
    let app = build_app(state);
    let accounts_pool = connect_accounts_pool(&config).await;

    let response = app
        .clone()
        .oneshot(
            Request::patch("/api/accounts/seller-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "autoReply": true,
                        "autoShip": true,
                        "aiEnabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["autoReply"], true);
    assert_eq!(payload["autoShip"], true);
    assert_eq!(payload["aiEnabled"], true);

    let stored_extra: String = sqlx::query_scalar("SELECT extra FROM accounts WHERE uid = ?")
        .bind("seller-001")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert!(stored_extra.contains("\"autoReply\":true"));
    assert!(stored_extra.contains("\"autoShip\":true"));
    assert!(stored_extra.contains("\"aiEnabled\":true"));

    let ai_flags: (i64, i64, i64) = sqlx::query_as(
        "SELECT ai_enabled, auto_reply_enabled, auto_ship_enabled FROM ai_reply_settings WHERE cookie_id = ?",
    )
    .bind("seller-001")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(ai_flags, (1, 1, 1));

    clear_env();
}

#[tokio::test]
async fn new_business_automation_tables_exist_after_bootstrap() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let _state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_accounts_pool(&config).await;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('ai_conversations', 'default_reply_records', 'automation_message_dedup', 'item_reply_task_runs') ORDER BY name",
    )
    .fetch_all(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(
        tables,
        vec![
            "ai_conversations".to_string(),
            "automation_message_dedup".to_string(),
            "default_reply_records".to_string(),
            "item_reply_task_runs".to_string(),
        ]
    );

    clear_env();
}

#[tokio::test]
async fn item_reply_task_run_limit_blocks_after_reaching_cap() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();

    assert!(state
        .storage()
        .try_consume_item_reply_task_run("seller-001", "session-1", "task:reply-a", 2)
        .await
        .unwrap());
    assert!(state
        .storage()
        .try_consume_item_reply_task_run("seller-001", "session-1", "task:reply-a", 2)
        .await
        .unwrap());
    assert!(!state
        .storage()
        .try_consume_item_reply_task_run("seller-001", "session-1", "task:reply-a", 2)
        .await
        .unwrap());
    assert!(state
        .storage()
        .try_consume_item_reply_task_run("seller-001", "session-2", "task:reply-a", 2)
        .await
        .unwrap());

    clear_env();
}

async fn seed_account(pool: &SqlitePool, uid: &str) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid, nick, token, sid, cookies, browser_cookie_jar, status,
            created_at, updated_at, extra
        )
        VALUES (?, 'Seller', '', '', '{}', '[]', 'active', 1, 1, '{}')
        "#,
    )
    .bind(uid)
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn connect_accounts_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "m017-business-automation-secret");
    set_env("API_TOKEN", TEST_BEARER);
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("API_TOKEN");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
