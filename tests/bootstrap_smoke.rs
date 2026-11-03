use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app,
    config::AppConfig,
    models::runtime::{StartupDependencyProbe, StartupDependencyStatus},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "bootstrap-token";

#[tokio::test]
async fn health_reports_ok_status() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "test-bootstrap-secret");

    let config = AppConfig::from_env().unwrap();
    let expected_service_name = config.service_name.clone();
    let app = build_app(AppState::bootstrap(config).await.unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["ready"], true);
    assert_eq!(payload["startup_blocked"], false);
    assert_eq!(payload["service"], expected_service_name);
    assert_eq!(payload["state_backend"], "sqlite");
    assert_eq!(
        payload["dependencies"]["summary"],
        "startup dependency checks pending"
    );
    assert_eq!(
        payload["started_at"].as_str().map(|s| !s.is_empty()),
        Some(true)
    );

    clear_env("APP_DATA_DIR");
    clear_env("JWT_SECRET");
}

#[tokio::test]
async fn cors_preflight_does_not_allow_removed_auth_headers() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config).await.unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/api/qr/generate")
                .header("origin", "http://127.0.0.1:5173")
                .header("access-control-request-method", "POST")
                .header(
                    "access-control-request-headers",
                    "authorization,content-type,x-license-auth-token",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let allow_headers = response
        .headers()
        .get("access-control-allow-headers")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(
        !allow_headers.contains("authorization") && !allow_headers.contains("x-license-auth-token"),
        "removed auth headers must not be allowed, got {allow_headers}"
    );

    clear_env("APP_DATA_DIR");
}

#[tokio::test]
async fn runtime_reports_bootstrap_state() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "test-bootstrap-secret");
    set_env("API_TOKEN", TEST_BEARER);

    let config = AppConfig::from_env().unwrap();
    let expected_service_name = config.service_name.clone();
    let app = build_app(AppState::bootstrap(config).await.unwrap());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/runtime")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(payload["service"], expected_service_name);
    assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(payload["state_backend"], "sqlite");
    assert_eq!(payload["auth_configured"], false);
    assert_eq!(payload["ready"], true);
    assert_eq!(payload["startup_count"], 1);
    assert_eq!(payload["databases"].as_array().map(Vec::len), Some(4));
    assert_eq!(
        payload["started_at"].as_str().map(|s| !s.is_empty()),
        Some(true)
    );

    clear_env("APP_DATA_DIR");
    clear_env("JWT_SECRET");
    clear_env("API_TOKEN");
}

#[tokio::test]
async fn health_reports_blocked_status_when_dependencies_fail() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "test-bootstrap-secret");

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    state
        .runtime()
        .update_startup_dependencies(StartupDependencyStatus {
            startup_blocked: true,
            summary: "签名服务异常: timeout".to_string(),
            sign_service: StartupDependencyProbe {
                name: "sign_service".to_string(),
                url: "https://sign.example.com/health".to_string(),
                ok: false,
                critical: true,
                status_code: None,
                detail: "timeout".to_string(),
                checked_at: "now".to_string(),
            },
            license_service: StartupDependencyProbe {
                name: "license_service".to_string(),
                url: "https://license.example.com/health".to_string(),
                ok: true,
                critical: true,
                status_code: Some(200),
                detail: "ok".to_string(),
                checked_at: "now".to_string(),
            },
            version_policy: StartupDependencyProbe {
                name: "version_policy".to_string(),
                url: "https://license.example.com/api/client-version-policy".to_string(),
                ok: true,
                critical: true,
                status_code: Some(200),
                detail: "ok".to_string(),
                checked_at: "now".to_string(),
            },
        });
    let app = build_app(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(payload["status"], "blocked");
    assert_eq!(payload["ready"], false);
    assert_eq!(payload["startup_blocked"], true);
    assert_eq!(payload["dependencies"]["summary"], "签名服务异常: timeout");

    clear_env("APP_DATA_DIR");
    clear_env("JWT_SECRET");
}

#[tokio::test]
async fn bootstrap_installs_in_process_feishu_native_runtime_without_node() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "test-bootstrap-secret");

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();

    let health = state.feishu_native().runtime_health().await.unwrap();
    assert_eq!(health.status, "ok");
    assert_eq!(health.transport, "rust-native");

    clear_env("APP_DATA_DIR");
    clear_env("JWT_SECRET");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env(key: &str) {
    unsafe { env::remove_var(key) }
}
