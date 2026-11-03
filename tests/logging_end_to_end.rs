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
    models::operations::OperationLogFilter,
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static LOG_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn canonical_traceability_flow_links_runtime_audit_incident_and_upload() {
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
    let legacy_admin_token = encode_access_token(&config.auth, "admin", "admin", true).unwrap();
    drain_existing_uploadable_logs(&state, "runtime").await;
    drain_existing_uploadable_logs(&state, "usage").await;

    sqlx::query(
        "INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at) VALUES (?, 'admin', ?, 'active', '', '{}', 1, 1)",
    )
    .bind("uid-e2e")
    .bind("端到端账号")
    .execute(state.storage().accounts())
    .await
    .unwrap();

    let trace_id = "trace-e2e-1";
    let request_id = "request-e2e-1";

    // The Tauri host and backend share the same structured runtime envelope.
    // This test injects one host-side event into that shared layer so the
    // resulting correlation can be followed alongside frontend and audit data.
    tracing::warn!(
        target: "tauri.runtime",
        stream = "runtime",
        trace_id = trace_id,
        request_id = request_id,
        module = "tauri-runtime",
        action = "host_bootstrap",
        outcome = "degraded",
        error_code = "host_bootstrap_probe",
        "tauri host bootstrap probe observed"
    );

    let client_event_response = app
        .clone()
        .oneshot(
            Request::post("/api/logs/client-events")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .header("x-trace-id", trace_id)
                .header("x-request-id", request_id)
                .body(Body::from(
                    json!({
                        "level": "error",
                        "stream": "runtime",
                        "logger": "frontend.action",
                        "module": "messages",
                        "action": "submit_task",
                        "route": "/tasks",
                        "traceId": trace_id,
                        "requestId": request_id,
                        "errorCode": "task_submit_failed",
                        "message": "用户提交任务失败",
                        "detail": "网络抖动导致请求失败",
                        "retryable": true,
                        "promoteToIncident": true,
                        "context": {
                            "accountUid": "uid-e2e",
                            "sessionId": "session-e2e",
                            "view": "tasks"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(client_event_response.status(), StatusCode::OK);
    let client_event_payload = json_body(client_event_response).await;
    let incident_id = client_event_payload["incidentId"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(!incident_id.is_empty());
    let queued_incidents = state
        .storage()
        .next_incident_upload_batch(10)
        .await
        .unwrap();
    assert!(queued_incidents
        .iter()
        .any(|report| report.incident_id == incident_id));
    let queued_incident: (String, i64) = sqlx::query_as(
        "SELECT status, attempt_count FROM incident_upload_queue WHERE incident_id = ?",
    )
    .bind(&incident_id)
    .fetch_one(state.storage().xianyu())
    .await
    .unwrap();
    assert_eq!(queued_incident.0, "queued");
    assert_eq!(queued_incident.1, 0);

    let create_job_response = app
        .clone()
        .oneshot(
            Request::post("/api/operation-jobs")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .header("x-trace-id", trace_id)
                .header("x-request-id", request_id)
                .body(Body::from(
                    json!({
                        "kind": "manual_message_task",
                        "module": "messages",
                        "action": "submit_task",
                        "operator": "admin",
                        "summary": {
                            "title": "任务提交",
                            "source": "logging-end-to-end"
                        },
                        "items": [{
                            "uid": "uid-e2e",
                            "target_type": "item",
                            "target_id": "item-e2e",
                            "target_title": "端到端商品",
                            "params": {
                                "sessionId": "session-e2e"
                            }
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_job_response.status(), StatusCode::OK);
    let create_job_payload = json_body(create_job_response).await;
    let job_id = create_job_payload["job_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(!job_id.is_empty());
    let operation_job_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/operation-jobs/{job_id}"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(operation_job_response.status(), StatusCode::OK);
    let operation_job_payload = json_body(operation_job_response).await;
    assert_eq!(operation_job_payload["job"]["id"], job_id);

    state.storage().sync_runtime_log_buffer().await.unwrap();

    let runtime_response = app
        .clone()
        .oneshot(
            Request::get(format!(
                "/api/logs?stream=runtime&trace_id={trace_id}&request_id={request_id}&limit=20"
            ))
            .header("authorization", bearer(&admin_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    let runtime_rows = runtime_payload["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(runtime_rows
        .iter()
        .any(|row| row["logger"] == "client.telemetry.action"));
    assert!(runtime_rows
        .iter()
        .any(|row| row["logger"] == "tauri.runtime"));

    let incident_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/logs/incidents/{incident_id}"))
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(incident_response.status(), StatusCode::OK);
    let incident_payload = json_body(incident_response).await;
    assert_eq!(incident_payload["found"], true);
    assert_eq!(incident_payload["data"]["contextJson"]["traceId"], trace_id);
    assert_eq!(
        incident_payload["data"]["contextJson"]["requestId"],
        request_id
    );
    state
        .storage()
        .mark_incident_upload_started(&incident_id)
        .await
        .unwrap();
    state
        .storage()
        .mark_incident_upload_result(&incident_id, true, true, None)
        .await
        .unwrap();
    let uploaded_incident: (String, i64, f64) = sqlx::query_as(
        "SELECT status, attempt_count, uploaded_at FROM incident_upload_queue WHERE incident_id = ?",
    )
    .bind(&incident_id)
    .fetch_one(state.storage().xianyu())
    .await
    .unwrap();
    assert_eq!(uploaded_incident.0, "uploaded");
    assert_eq!(uploaded_incident.1, 1);
    assert!(uploaded_incident.2 > 0.0);
    assert!(!state
        .storage()
        .next_incident_upload_batch(10)
        .await
        .unwrap()
        .iter()
        .any(|report| report.incident_id == incident_id));

    let operation_logs_response = app
        .clone()
        .oneshot(
            Request::get(
                "/api/operation-logs?module=messages&action=submit_task&status=queued&limit=20",
            )
            .header("authorization", bearer(&legacy_admin_token))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(operation_logs_response.status(), StatusCode::OK);
    let operation_logs_payload = json_body(operation_logs_response).await;
    let operation_logs = operation_logs_payload["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let public_audit_entry = operation_logs
        .iter()
        .find(|entry| entry["target_id"] == job_id)
        .expect("public operation log entry for job");
    assert_eq!(public_audit_entry["extra"]["traceId"], trace_id);
    assert_eq!(public_audit_entry["extra"]["requestId"], request_id);

    let audit_entries = state
        .storage()
        .list_operation_logs(&OperationLogFilter {
            module: "messages".to_string(),
            action: "submit_task".to_string(),
            uid: String::new(),
            status: "queued".to_string(),
            keyword: String::new(),
            limit: 20,
            since: 0.0,
        })
        .await
        .unwrap();
    let audit_entry = audit_entries
        .iter()
        .find(|entry| entry.target_id == job_id)
        .expect("operation log entry for job");
    assert_eq!(audit_entry.extra["traceId"], trace_id);
    assert_eq!(audit_entry.extra["requestId"], request_id);

    let runtime_batch = state
        .storage()
        .next_control_plane_log_batch("runtime", 50)
        .await
        .unwrap();
    assert!(runtime_batch.iter().any(|entry| {
        entry.trace_id.as_deref() == Some(trace_id)
            && entry.request_id.as_deref() == Some(request_id)
            && entry.logger == "client.telemetry.action"
    }));
    assert!(runtime_batch.iter().any(|entry| {
        entry.trace_id.as_deref() == Some(trace_id)
            && entry.request_id.as_deref() == Some(request_id)
            && entry.logger == "tauri.runtime"
    }));
    let last_runtime_sequence = runtime_batch.last().unwrap().sequence;
    state
        .storage()
        .mark_control_plane_logs_uploaded("runtime", last_runtime_sequence)
        .await
        .unwrap();
    let remaining_runtime_batch = state
        .storage()
        .next_control_plane_log_batch("runtime", 50)
        .await
        .unwrap();
    assert!(!remaining_runtime_batch.iter().any(|entry| {
        entry.trace_id.as_deref() == Some(trace_id)
            && entry.request_id.as_deref() == Some(request_id)
    }));
    let uploaded_runtime_rows: Vec<String> = sqlx::query_scalar(
        "SELECT upload_status FROM runtime_logs WHERE trace_id = ? AND request_id = ? ORDER BY sequence ASC",
    )
    .bind(trace_id)
    .bind(request_id)
    .fetch_all(state.storage().xianyu())
    .await
    .unwrap();
    assert!(!uploaded_runtime_rows.is_empty());
    assert!(uploaded_runtime_rows
        .iter()
        .all(|status| status == "uploaded"));

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
    set_env("JWT_SECRET", "logging-end-to-end-secret");
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
