use std::{env, sync::Mutex, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use sqlx::{query_scalar, SqlitePool};
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app, auth::encode_access_token, config::AppConfig, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_supervisor_publishes_worker_status_and_persists_heartbeat() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();
    let supervisor = state.start_runtime_supervisor();

    tokio::time::sleep(Duration::from_millis(3400)).await;

    let app = build_app(state.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri("/runtime")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    let managers = payload["managers"].as_array().unwrap();

    assert_eq!(managers.len(), 11);
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "service_heartbeat"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "sqlite_monitor"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "cache_sync_runtime"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "comment_delete_runtime"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "live_message_runtime"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "live_history_prefetch"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "cookie_refresh"));
    assert!(managers
        .iter()
        .any(|manager| manager["name"] == "operation_jobs"));
    // history prefetch is delayed, and the other background managers should
    // tick on their schedules even when there are no bound accounts yet.
    let managers_without_ticks = managers
        .iter()
        .filter(|manager| {
            manager["name"] != "live_history_prefetch" && manager["name"] != "live_message_runtime"
        })
        .filter(|manager| manager["tick_count"].as_u64().unwrap_or_default() < 1)
        .filter_map(|manager| manager["name"].as_str())
        .collect::<Vec<_>>();
    assert!(
        managers_without_ticks.is_empty(),
        "managers without ticks: {managers_without_ticks:?}"
    );

    let accounts_url = format!("sqlite://{}", config.sqlite.accounts_db_path.display());
    let accounts = SqlitePool::connect(&accounts_url).await.unwrap();
    let worker_rows = query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM service_worker_state WHERE status = 'running'",
    )
    .fetch_one(&accounts)
    .await
    .unwrap();
    assert_eq!(worker_rows, 10);

    supervisor.shutdown_and_wait().await;
    clear_env();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_supervisor_stops_workers_cleanly() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let supervisor = state.start_runtime_supervisor();

    tokio::time::sleep(Duration::from_millis(1200)).await;
    supervisor.shutdown_and_wait().await;

    let inspection = state.runtime().inspection(config.service_name.clone());
    assert_eq!(inspection.managers.len(), 11);
    assert!(inspection
        .managers
        .iter()
        .all(|manager| manager.status == "stopped"));

    clear_env();
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "runtime-supervisor-secret");
    set_env(
        "XIANYU_LIVE_RUNTIME_LOCK_DIR",
        temp_dir.path().join("live-runtime-locks").to_str().unwrap(),
    );
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("XIANYU_LIVE_RUNTIME_LOCK_DIR");
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
