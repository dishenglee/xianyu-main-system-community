use std::{
    env,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::util::ServiceExt;
use tracing_subscriber::{layer::SubscriberExt, Registry};
use xianyu_rust_backend_full::{
    app::build_app,
    auth::encode_access_token,
    config::AppConfig,
    logging::{clear_logs, memory_log_layer},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static LOG_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn diagnostic_policy_admin_api_round_trips_with_stable_shape() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _log_guard = LOG_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    clear_logs();

    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state);
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();

    let default_response = app
        .clone()
        .oneshot(
            Request::get("/api/logs/diagnostic-policy")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(default_response.status(), StatusCode::OK);
    assert_eq!(
        json_body(default_response).await,
        json!({
            "data": {
                "enabled": false,
                "level": "DEBUG",
                "modules": [],
                "usernames": [],
                "deviceIds": [],
                "accountUids": [],
                "sessionIds": [],
                "startsAt": Value::Null,
                "endsAt": Value::Null,
                "includePayloads": false,
                "sampleRate": 1.0
            }
        })
    );

    let payload = json!({
        "enabled": true,
        "level": "warn",
        "modules": [" messages ", "live.worker", "messages"],
        "usernames": [" owner-a "],
        "deviceIds": [" device-a "],
        "accountUids": [" uid-a "],
        "sessionIds": [" session-a "],
        "startsAt": 100.0,
        "endsAt": 200.0,
        "includePayloads": true,
        "sampleRate": 0.5
    });
    let update_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/logs/diagnostic-policy")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    assert_eq!(
        json_body(update_response).await,
        json!({
            "data": {
                "enabled": true,
                "level": "WARN",
                "modules": ["messages", "live.worker"],
                "usernames": ["owner-a"],
                "deviceIds": ["device-a"],
                "accountUids": ["uid-a"],
                "sessionIds": ["session-a"],
                "startsAt": 100.0,
                "endsAt": 200.0,
                "includePayloads": true,
                "sampleRate": 0.5
            }
        })
    );

    let persisted_response = app
        .clone()
        .oneshot(
            Request::get("/api/logs/diagnostic-policy")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(persisted_response.status(), StatusCode::OK);
    assert_eq!(
        json_body(persisted_response).await,
        json!({
            "data": {
                "enabled": true,
                "level": "WARN",
                "modules": ["messages", "live.worker"],
                "usernames": ["owner-a"],
                "deviceIds": ["device-a"],
                "accountUids": ["uid-a"],
                "sessionIds": ["session-a"],
                "startsAt": 100.0,
                "endsAt": 200.0,
                "includePayloads": true,
                "sampleRate": 0.5
            }
        })
    );

    clear_env();
}

#[tokio::test]
async fn diagnostic_policy_matcher_can_promote_info_events_into_persisted_runtime_diagnostics() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _log_guard = LOG_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    clear_logs();

    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let now = unix_now();

    state
        .storage()
        .save_runtime_diagnostic_policy(
            true,
            "debug",
            &["messages".to_string()],
            &["owner-a".to_string()],
            &["device-a".to_string()],
            &["uid-a".to_string()],
            &["session-a".to_string()],
            Some(now - 10.0),
            Some(now + 10.0),
            true,
            1.0,
        )
        .await
        .unwrap();

    let matched = state
        .storage()
        .runtime_diagnostic_policy_decision(
            Some("messages"),
            Some("owner-a"),
            Some("device-a"),
            Some("uid-a"),
            Some("session-a"),
            Some(now),
            Some("trace-diagnostic-1"),
        )
        .await
        .unwrap();
    assert_eq!(
        matched,
        json!({
            "matched": true,
            "level": "DEBUG",
            "includePayloads": true,
            "sampleRate": 1.0
        })
    );

    let window_miss = state
        .storage()
        .runtime_diagnostic_policy_decision(
            Some("messages"),
            Some("owner-a"),
            Some("device-a"),
            Some("uid-a"),
            Some("session-a"),
            Some(now + 30.0),
            Some("trace-diagnostic-1"),
        )
        .await
        .unwrap();
    assert_eq!(
        window_miss,
        json!({
            "matched": false,
            "level": Value::Null,
            "includePayloads": false,
            "sampleRate": 1.0
        })
    );

    tracing::info!(
        target: "live.worker",
        stream = "runtime",
        trace_id = "trace-diagnostic-1",
        username = "owner-a",
        device_id = "device-a",
        account_uid = "uid-a",
        session_id = "session-a",
        module = "messages",
        action = "diagnostic_probe",
        "info-level event promoted by diagnostic policy"
    );

    state.storage().sync_runtime_log_buffer().await.unwrap();

    let stored_levels: Vec<String> = sqlx::query_scalar(
        "SELECT level FROM runtime_logs WHERE trace_id = 'trace-diagnostic-1' ORDER BY sequence ASC",
    )
        .fetch_all(state.storage().xianyu())
        .await
        .unwrap();
    assert_eq!(stored_levels, vec!["DEBUG".to_string()]);

    let upload_batch = state
        .storage()
        .next_control_plane_log_batch("runtime", 10)
        .await
        .unwrap();
    assert!(upload_batch
        .iter()
        .any(|entry| entry.trace_id.as_deref() == Some("trace-diagnostic-1")));

    clear_env();
}

#[tokio::test]
async fn diagnostic_policy_sampling_bounds_are_respected() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _log_guard = LOG_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    clear_logs();

    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let now = unix_now();

    state
        .storage()
        .save_runtime_diagnostic_policy(
            true,
            "debug",
            &["messages".to_string()],
            &["owner-a".to_string()],
            &["device-a".to_string()],
            &["uid-a".to_string()],
            &["session-a".to_string()],
            Some(now - 10.0),
            Some(now + 10.0),
            false,
            0.0,
        )
        .await
        .unwrap();

    let suppressed = state
        .storage()
        .runtime_diagnostic_policy_decision(
            Some("messages"),
            Some("owner-a"),
            Some("device-a"),
            Some("uid-a"),
            Some("session-a"),
            Some(now),
            Some("sample-key"),
        )
        .await
        .unwrap();
    assert_eq!(suppressed["matched"], false);

    state
        .storage()
        .save_runtime_diagnostic_policy(
            true,
            "debug",
            &["messages".to_string()],
            &["owner-a".to_string()],
            &["device-a".to_string()],
            &["uid-a".to_string()],
            &["session-a".to_string()],
            Some(now - 10.0),
            Some(now + 10.0),
            false,
            1.0,
        )
        .await
        .unwrap();

    let matched = state
        .storage()
        .runtime_diagnostic_policy_decision(
            Some("messages"),
            Some("owner-a"),
            Some("device-a"),
            Some("uid-a"),
            Some("session-a"),
            Some(now),
            Some("sample-key"),
        )
        .await
        .unwrap();
    assert_eq!(matched["matched"], true);

    clear_env();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "runtime-diagnostic-policy-secret");
    set_env("ADMIN_USERNAME", "admin");
    set_env("ADMIN_PASSWORD", "admin123");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
