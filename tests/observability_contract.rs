use std::{env, sync::Mutex};

use axum::{
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    middleware,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use tower::ServiceExt;
use tracing_subscriber::{layer::SubscriberExt, Registry};
use xianyu_rust_backend_full::{
    auth::{encode_access_token, require_bearer_middleware},
    config::AppConfig,
    logging::{clear_logs, memory_log_layer, query_logs},
    models::auth::RequestIdentity,
    observability::{EventStream, RequestContext, StructuredEventEnvelope},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static LOG_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn structured_event_envelope_serializes_stable_top_level_keys() {
    let envelope = StructuredEventEnvelope {
        stream: EventStream::Runtime,
        trace_id: "trace-1".to_string(),
        request_id: Some("req-1".to_string()),
        incident_id: None,
        username: Some("alice".to_string()),
        device_id: None,
        account_uid: Some("uid-1".to_string()),
        session_id: None,
        item_id: Some("item-9".to_string()),
        job_id: None,
        module: Some("auth".to_string()),
        action: Some("verify".to_string()),
        outcome: Some("success".to_string()),
        error_code: None,
        retryable: Some(false),
        duration_ms: Some(42),
    };

    let value = serde_json::to_value(&envelope).expect("envelope serializes");
    let object = value.as_object().expect("envelope object");

    for key in [
        "stream",
        "trace_id",
        "request_id",
        "incident_id",
        "username",
        "device_id",
        "account_uid",
        "session_id",
        "item_id",
        "job_id",
        "module",
        "action",
        "outcome",
        "error_code",
        "retryable",
        "duration_ms",
    ] {
        assert!(object.contains_key(key), "missing serialized key {key}");
    }

    assert_eq!(value["stream"], "runtime");
    assert_eq!(value["trace_id"], "trace-1");
    assert_eq!(value["request_id"], "req-1");
    assert_eq!(value["incident_id"], Value::Null);
    assert_eq!(value["username"], "alice");
    assert_eq!(value["device_id"], Value::Null);
    assert_eq!(value["account_uid"], "uid-1");
    assert_eq!(value["session_id"], Value::Null);
    assert_eq!(value["item_id"], "item-9");
    assert_eq!(value["job_id"], Value::Null);
    assert_eq!(value["module"], "auth");
    assert_eq!(value["action"], "verify");
    assert_eq!(value["outcome"], "success");
    assert_eq!(value["error_code"], Value::Null);
    assert_eq!(value["retryable"], false);
    assert_eq!(value["duration_ms"], 42);
}

#[tokio::test]
async fn protected_route_preserves_header_trace_ids_and_identity_context() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _log_guard = LOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().unwrap();
    clear_logs();
    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    set_env("ADMIN_USERNAME", "compat-admin");
    set_env("ADMIN_PASSWORD", "compat-pass");
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "runtime-secret");

    let config = AppConfig::from_env().expect("config loads");
    let token =
        encode_access_token(&config.auth, "compat-admin", "admin", false).expect("jwt encodes");
    let state = AppState::bootstrap(config).await.expect("state bootstraps");
    let app = Router::new()
        .route("/protected", get(read_context))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer_middleware,
        ))
        .with_state(state);

    let response = app
        .oneshot(
            Request::get("/protected")
                .header("authorization", format!("Bearer {token}"))
                .header("x-trace-id", "trace-from-header")
                .header("x-request-id", "request-from-header")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = collect_json(response).await;
    assert_eq!(payload["context"]["trace_id"], "trace-from-header");
    assert_eq!(payload["context"]["request_id"], "request-from-header");
    assert_eq!(payload["identity"]["username"], "compat-admin");
    assert_eq!(payload["identity"]["trace_id"], "trace-from-header");
    assert_eq!(payload["identity"]["request_id"], "request-from-header");

    let entries = query_logs(
        None,
        Some("request context visible in handler"),
        None,
        10,
        0.0,
    );
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].trace_id.as_deref(), Some("trace-from-header"));
    assert_eq!(
        entries[0].request_id.as_deref(),
        Some("request-from-header")
    );

    clear_env("ADMIN_USERNAME");
    clear_env("ADMIN_PASSWORD");
    clear_env("APP_DATA_DIR");
    clear_env("JWT_SECRET");
}

#[tokio::test]
async fn protected_route_generates_missing_trace_and_request_ids() {
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _log_guard = LOG_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let temp_dir = TempDir::new().unwrap();
    clear_logs();
    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    set_env("ADMIN_USERNAME", "compat-admin");
    set_env("ADMIN_PASSWORD", "compat-pass");
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "runtime-secret");

    let config = AppConfig::from_env().expect("config loads");
    let token =
        encode_access_token(&config.auth, "compat-admin", "admin", false).expect("jwt encodes");
    let state = AppState::bootstrap(config).await.expect("state bootstraps");
    let app = Router::new()
        .route("/protected", get(read_context))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer_middleware,
        ))
        .with_state(state);

    let response = app
        .oneshot(
            Request::get("/protected")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = collect_json(response).await;
    let trace_id = payload["context"]["trace_id"]
        .as_str()
        .expect("trace id")
        .to_string();
    let request_id = payload["context"]["request_id"]
        .as_str()
        .expect("request id")
        .to_string();

    assert!(!trace_id.is_empty());
    assert!(!request_id.is_empty());
    assert_eq!(payload["identity"]["trace_id"], trace_id);
    assert_eq!(payload["identity"]["request_id"], request_id);

    clear_env("ADMIN_USERNAME");
    clear_env("ADMIN_PASSWORD");
    clear_env("APP_DATA_DIR");
    clear_env("JWT_SECRET");
}

#[test]
fn memory_log_capture_preserves_structured_observability_fields() {
    let _guard = LOG_LOCK.lock().unwrap();
    clear_logs();

    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    tracing::info!(
        target: "observability.contract",
        stream = "runtime",
        trace_id = "trace-log-1",
        request_id = "request-log-1",
        incident_id = "incident-log-1",
        username = "alice",
        device_id = "device-1",
        account_uid = "uid-1",
        session_id = "session-1",
        item_id = "item-1",
        job_id = "job-1",
        module = "auth",
        action = "verify",
        outcome = "success",
        error_code = "none",
        retryable = false,
        duration_ms = 12_u64,
        "structured observability log"
    );

    let entries = query_logs(None, Some("structured observability log"), None, 10, 0.0);
    assert_eq!(entries.len(), 1);

    let entry = &entries[0];
    assert_eq!(entry.message, "structured observability log");
    assert_eq!(entry.stream.as_deref(), Some("runtime"));
    assert_eq!(entry.trace_id.as_deref(), Some("trace-log-1"));
    assert_eq!(entry.request_id.as_deref(), Some("request-log-1"));
    assert_eq!(entry.incident_id.as_deref(), Some("incident-log-1"));
    assert_eq!(entry.username.as_deref(), Some("alice"));
    assert_eq!(entry.device_id.as_deref(), Some("device-1"));
    assert_eq!(entry.account_uid.as_deref(), Some("uid-1"));
    assert_eq!(entry.session_id.as_deref(), Some("session-1"));
    assert_eq!(entry.item_id.as_deref(), Some("item-1"));
    assert_eq!(entry.job_id.as_deref(), Some("job-1"));
    assert_eq!(entry.module.as_deref(), Some("auth"));
    assert_eq!(entry.action.as_deref(), Some("verify"));
    assert_eq!(entry.outcome.as_deref(), Some("success"));
    assert_eq!(entry.error_code.as_deref(), Some("none"));
    assert_eq!(entry.retryable, Some(false));
    assert_eq!(entry.duration_ms, Some(12));
    assert_eq!(entry.fields_json["trace_id"], "trace-log-1");
    assert_eq!(entry.fields_json["request_id"], "request-log-1");
    assert_eq!(entry.fields_json["module"], "auth");
    assert_eq!(entry.fields_json["action"], "verify");
}

async fn read_context(
    Extension(identity): Extension<RequestIdentity>,
    Extension(context): Extension<RequestContext>,
) -> impl IntoResponse {
    let envelope = context.envelope();
    match envelope.request_id.as_deref() {
        Some(request_id) => tracing::info!(
            target: "observability.contract",
            stream = envelope.stream.as_str(),
            trace_id = envelope.trace_id.as_str(),
            request_id,
            username = envelope.username.as_deref().unwrap_or_default(),
            module = "test",
            action = "read_context",
            outcome = "success",
            "request context visible in handler"
        ),
        None => tracing::info!(
            target: "observability.contract",
            stream = envelope.stream.as_str(),
            trace_id = envelope.trace_id.as_str(),
            username = envelope.username.as_deref().unwrap_or_default(),
            module = "test",
            action = "read_context",
            outcome = "success",
            "request context visible in handler"
        ),
    }

    Json(json!({
        "identity": {
            "username": identity.username,
            "trace_id": identity.correlation.trace_id,
            "request_id": identity.correlation.request_id,
        },
        "context": context.envelope(),
    }))
}

async fn collect_json(response: axum::response::Response) -> Value {
    use http_body_util::BodyExt;

    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes();
    serde_json::from_slice(&body).expect("json parses")
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env(key: &str) {
    unsafe { env::remove_var(key) }
}
