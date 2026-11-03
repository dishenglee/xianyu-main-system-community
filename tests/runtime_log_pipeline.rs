use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
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
async fn runtime_and_usage_logs_persist_with_filterable_queries_and_independent_upload_cursors() {
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
    let app = build_app(state.clone());
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();
    drain_existing_uploadable_logs(&state, "runtime").await;
    drain_existing_uploadable_logs(&state, "usage").await;

    tracing::warn!(
        target: "live.worker",
        stream = "runtime",
        trace_id = "trace-runtime-1",
        request_id = "request-runtime-1",
        incident_id = "incident-runtime-1",
        username = "owner-a",
        device_id = "device-a",
        account_uid = "uid-a",
        session_id = "session-a",
        item_id = "item-a",
        job_id = "job-a",
        module = "messages",
        action = "send",
        outcome = "failed",
        error_code = "send_failed",
        "runtime log for durable query"
    );
    tracing::info!(
        target: "client.telemetry.action",
        stream = "usage",
        trace_id = "trace-usage-1",
        request_id = "request-usage-1",
        username = "owner-a",
        account_uid = "uid-a",
        session_id = "session-a",
        item_id = "item-a",
        module = "items",
        action = "open_card",
        outcome = "success",
        "usage log for durable query"
    );

    state.storage().sync_runtime_log_buffer().await.unwrap();

    let runtime_response = app
        .clone()
        .oneshot(
            Request::get(
                "/api/logs?stream=runtime&level=WARN&logger=live.worker&module=messages&trace_id=trace-runtime-1&request_id=request-runtime-1&incident_id=incident-runtime-1&username=owner-a&device_id=device-a&account_uid=uid-a&session_id=session-a&item_id=item-a&job_id=job-a&action=send&outcome=failed&error_code=send_failed&limit=10",
            )
            .header("authorization", bearer(&admin_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    assert_eq!(runtime_payload["count"], 1);
    assert_eq!(runtime_payload["data"][0]["stream"], "runtime");
    assert_eq!(runtime_payload["data"][0]["traceId"], "trace-runtime-1");
    assert_eq!(runtime_payload["data"][0]["logger"], "live.worker");

    let usage_response = app
        .clone()
        .oneshot(
            Request::get(
                "/api/logs?stream=usage&trace_id=trace-usage-1&action=open_card&module=items&limit=10",
            )
            .header("authorization", bearer(&admin_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(usage_response.status(), StatusCode::OK);
    let usage_payload = json_body(usage_response).await;
    assert_eq!(usage_payload["count"], 1);
    assert_eq!(usage_payload["data"][0]["stream"], "usage");
    assert_eq!(usage_payload["data"][0]["traceId"], "trace-usage-1");

    let runtime_batch = state
        .storage()
        .next_control_plane_log_batch("runtime", 10)
        .await
        .unwrap();
    assert!(runtime_batch
        .iter()
        .any(|entry| entry.trace_id.as_deref() == Some("trace-runtime-1")));

    let usage_batch = state
        .storage()
        .next_control_plane_log_batch("usage", 10)
        .await
        .unwrap();
    assert!(usage_batch
        .iter()
        .any(|entry| entry.trace_id.as_deref() == Some("trace-usage-1")));

    let usage_last_sequence = usage_batch.last().unwrap().sequence;
    state
        .storage()
        .mark_control_plane_logs_uploaded("usage", usage_last_sequence)
        .await
        .unwrap();
    let usage_after_upload = state
        .storage()
        .next_control_plane_log_batch("usage", 10)
        .await
        .unwrap();
    assert!(usage_after_upload.is_empty());

    let runtime_after_usage_upload = state
        .storage()
        .next_control_plane_log_batch("runtime", 10)
        .await
        .unwrap();
    assert!(runtime_after_usage_upload
        .iter()
        .any(|entry| entry.trace_id.as_deref() == Some("trace-runtime-1")));

    clear_env();
}

#[tokio::test]
async fn incident_queue_and_retention_rules_are_backed_by_sqlite() {
    let _env_guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let _log_guard = LOG_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_env("XIANYU_RUNTIME_LOG_RETENTION_MAX_BYTES", "1200");
    clear_logs();

    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    drain_existing_uploadable_logs(&state, "runtime").await;
    drain_existing_uploadable_logs(&state, "usage").await;

    tracing::warn!(
        target: "live.worker",
        stream = "runtime",
        trace_id = "trace-old",
        module = "messages",
        action = "send",
        error_code = "old_failure",
        "{}",
        "x".repeat(600)
    );
    state.storage().sync_runtime_log_buffer().await.unwrap();
    sqlx::query("UPDATE runtime_logs SET occurred_at = 1 WHERE trace_id = 'trace-old'")
        .execute(state.storage().xianyu())
        .await
        .unwrap();

    tracing::warn!(
        target: "live.worker",
        stream = "runtime",
        trace_id = "trace-new",
        module = "messages",
        action = "send",
        error_code = "new_failure",
        "{}",
        "y".repeat(600)
    );
    state.storage().sync_runtime_log_buffer().await.unwrap();

    let incident = state
        .incident_reporter()
        .report_from_state(
            &state,
            xianyu_rust_backend_full::telemetry::IncidentInput {
                error_level: "error".to_string(),
                module: "messages".to_string(),
                action: "send".to_string(),
                route: "/api/messages/send".to_string(),
                error_code: "send_failed".to_string(),
                error_message: "send failed".to_string(),
                http_status: Some(503),
                upstream_ret: String::new(),
                retryable: true,
                context_json: json!({ "sessionId": "session-a" }),
                environment: None,
            },
        )
        .await;

    let queued: (i64, String) = sqlx::query_as(
        "SELECT COUNT(*), MIN(status) FROM incident_upload_queue WHERE incident_id = ?",
    )
    .bind(&incident.incident_id)
    .fetch_one(state.storage().xianyu())
    .await
    .unwrap();
    assert_eq!(queued.0, 1);
    assert_eq!(queued.1, "queued");

    let last_runtime_sequence = state
        .storage()
        .next_control_plane_log_batch("runtime", 10)
        .await
        .unwrap()
        .last()
        .unwrap()
        .sequence;
    state
        .storage()
        .mark_control_plane_logs_uploaded("runtime", last_runtime_sequence)
        .await
        .unwrap();

    state
        .storage()
        .enforce_runtime_log_retention()
        .await
        .unwrap();

    let remaining_traces: Vec<String> =
        sqlx::query_scalar("SELECT trace_id FROM runtime_logs ORDER BY sequence ASC")
            .fetch_all(state.storage().xianyu())
            .await
            .unwrap();
    assert!(!remaining_traces.iter().any(|trace| trace == "trace-old"));
    assert!(remaining_traces.iter().any(|trace| trace == "trace-new"));

    clear_env();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

async fn drain_existing_uploadable_logs(state: &AppState, stream: &str) {
    state.storage().sync_runtime_log_buffer().await.unwrap();
    loop {
        let batch = state
            .storage()
            .next_control_plane_log_batch(stream, 200)
            .await
            .unwrap();
        let Some(last_sequence) = batch.last().map(|entry| entry.sequence) else {
            break;
        };
        state
            .storage()
            .mark_control_plane_logs_uploaded(stream, last_sequence)
            .await
            .unwrap();
    }
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "runtime-log-pipeline-secret");
    set_env("ADMIN_USERNAME", "admin");
    set_env("ADMIN_PASSWORD", "admin123");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("XIANYU_RUNTIME_LOG_RETENTION_MAX_BYTES");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
