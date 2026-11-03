use std::{env, sync::Mutex};

use axum::{body::Body, http::Request, http::StatusCode};
use http_body_util::BodyExt;
use tempfile::TempDir;
use tower::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn local_api_accepts_requests_without_user_credentials() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();
    env::set_var("APP_DATA_DIR", temp_dir.path());
    env::remove_var("APP_BIND_ADDR");
    env::set_var("JWT_SECRET", "unused-community-secret");
    env::remove_var("APP_BEARER_TOKEN");

    let state = AppState::bootstrap(AppConfig::from_env().expect("config loads"))
        .await
        .expect("state bootstraps");
    let app = build_app(state);

    let runtime = app
        .clone()
        .oneshot(Request::get("/runtime").body(Body::empty()).unwrap())
        .await
        .expect("runtime response");
    assert_eq!(runtime.status(), StatusCode::OK);
    let runtime_body = runtime.into_body().collect().await.unwrap().to_bytes();
    let runtime_payload: serde_json::Value = serde_json::from_slice(&runtime_body).unwrap();
    assert_eq!(runtime_payload["auth_configured"], false);

    let accounts = app
        .oneshot(Request::get("/api/accounts").body(Body::empty()).unwrap())
        .await
        .expect("accounts response");
    assert_eq!(accounts.status(), StatusCode::OK);
    let body = accounts.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
        serde_json::json!([])
    );

    let login = build_app(
        AppState::bootstrap(AppConfig::from_env().expect("config loads"))
            .await
            .expect("state bootstraps"),
    )
    .oneshot(
        Request::post("/api/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"username":"anyone","password":"anything"}"#))
            .unwrap(),
    )
    .await
    .expect("removed login route response");
    assert_eq!(login.status(), StatusCode::NOT_FOUND);

    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
}

#[test]
fn no_auth_service_refuses_non_loopback_bind_addresses() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var("APP_BIND_ADDR", "0.0.0.0:3000");

    let error = AppConfig::from_env().expect_err("network binding must be refused");
    assert!(error.to_string().contains("loopback"));

    env::remove_var("APP_BIND_ADDR");
}
