use std::{env, sync::Mutex, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::time::sleep;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "business-runtime-token";

#[tokio::test]
async fn ai_settings_round_trip_persists_camel_case_fields() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;

    let global_response = app
        .clone()
        .oneshot(
            Request::put("/api/ai-settings/__global__")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "apiType": "openai",
                        "apiKey": "sk-test",
                        "baseUrl": "https://api.example.com",
                        "modelName": "gpt-test",
                        "maxBargainRounds": 3,
                        "maxDiscountPercent": 12.5,
                        "maxDiscountAmount": 18.0,
                        "customPrompts": {
                            "system": "be polite"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(global_response.status(), StatusCode::OK);
    let global_payload = json_body(global_response).await;
    assert_eq!(global_payload["status"], "ok");
    assert_eq!(
        global_payload["settings"]["customPrompts"]["system"],
        "be polite"
    );

    let flags_response = app
        .clone()
        .oneshot(
            Request::put("/api/ai-settings/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "aiEnabled": true,
                        "autoReplyEnabled": true,
                        "autoShipEnabled": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(flags_response.status(), StatusCode::OK);
    let flags_payload = json_body(flags_response).await;
    assert_eq!(flags_payload["settings"]["aiEnabled"], true);
    assert_eq!(flags_payload["settings"]["apiKey"], "sk-test");

    let get_response = app
        .clone()
        .oneshot(
            Request::get("/api/ai-settings/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload = json_body(get_response).await;
    assert_eq!(get_payload["aiEnabled"], true);
    assert_eq!(get_payload["apiKey"], "sk-test");

    let stored_global: (String, String) =
        sqlx::query_as("SELECT api_key, custom_prompts FROM ai_reply_settings WHERE cookie_id = ?")
            .bind("__global__")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(stored_global.0, "sk-test");
    assert!(stored_global.1.contains("\"system\":\"be polite\""));

    let stored_account_flags: (i64, i64, i64, String) = sqlx::query_as(
        "SELECT ai_enabled, auto_reply_enabled, auto_ship_enabled, api_key FROM ai_reply_settings WHERE cookie_id = ?",
    )
    .bind("cookie-001")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(stored_account_flags, (1, 1, 0, String::new()));

    let override_response = app
        .clone()
        .oneshot(
            Request::put("/api/ai-settings/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "apiKey": "sk-account",
                        "baseUrl": "https://account.example.com",
                        "modelName": "gpt-account",
                        "customPrompts": {
                            "system": "account override"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(override_response.status(), StatusCode::OK);

    let stored_account_config_after_override: (String, String, String, String) = sqlx::query_as(
        "SELECT api_key, base_url, model_name, custom_prompts FROM ai_reply_settings WHERE cookie_id = ?",
    )
    .bind("cookie-001")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(
        stored_account_config_after_override,
        (
            String::new(),
            String::new(),
            String::new(),
            "{}".to_string()
        )
    );

    let override_get_response = app
        .clone()
        .oneshot(
            Request::get("/api/ai-settings/cookie-001")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let override_payload = json_body(override_get_response).await;
    assert_eq!(override_payload["apiKey"], "sk-test");
    assert_eq!(override_payload["baseUrl"], "https://api.example.com");
    assert_eq!(override_payload["modelName"], "gpt-test");
    assert_eq!(override_payload["customPrompts"]["system"], "be polite");

    clear_env();
}

#[tokio::test]
async fn business_orders_are_scoped_by_cookie_id() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();

    let first = state
        .storage()
        .upsert_business_order(
            "same-order",
            "uid-a",
            "item-a",
            "buyer-a",
            "88.00",
            "delivered",
            100,
            200,
        )
        .await
        .unwrap();
    assert_eq!(first.cookie_id, "uid-a");
    assert_eq!(first.status, "delivered");

    let second = state
        .storage()
        .upsert_business_order(
            "same-order",
            "uid-b",
            "item-b",
            "buyer-b",
            "99.00",
            "processing",
            0,
            300,
        )
        .await
        .unwrap();
    assert_eq!(second.cookie_id, "uid-b");
    assert_eq!(second.status, "processing");

    let uid_a_order = state
        .storage()
        .get_business_order("uid-a", "same-order")
        .await
        .unwrap()
        .unwrap();
    let uid_b_order = state
        .storage()
        .get_business_order("uid-b", "same-order")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(uid_a_order.item_id, "item-a");
    assert_eq!(uid_a_order.status, "delivered");
    assert_eq!(uid_b_order.item_id, "item-b");
    assert_eq!(uid_b_order.status, "processing");
    assert!(state
        .storage()
        .get_business_order("uid-c", "same-order")
        .await
        .unwrap()
        .is_none());

    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_route_exposes_cache_and_delete_runtime_snapshots() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let supervisor = state.start_runtime_supervisor();
    sleep(Duration::from_millis(3400)).await;

    let app = build_app(state.clone());
    let xianyu_pool = connect_pool(&config.sqlite.xianyu_db_path).await;

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
    assert_eq!(runtime_payload["delete_runtime"]["action_capacity"], 4);

    let cache_row: (String, i64) =
        sqlx::query_as("SELECT status, total_entries FROM cache_sync_runtime_state WHERE id = 1")
            .fetch_one(&xianyu_pool)
            .await
            .unwrap();
    assert_eq!(cache_row.0, "idle");
    assert_eq!(cache_row.1, 0);

    let delete_row: (i64, i64) = sqlx::query_as(
        "SELECT runtime_ready, action_capacity FROM comment_delete_runtime_state WHERE id = 1",
    )
    .fetch_one(&xianyu_pool)
    .await
    .unwrap();
    assert_eq!(delete_row.0, 1);
    assert_eq!(delete_row.1, 4);

    supervisor.shutdown_and_wait().await;
    clear_env();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "business-runtime-secret");
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
