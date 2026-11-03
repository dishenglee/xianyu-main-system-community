use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app,
    auth::encode_access_token,
    config::AppConfig,
    license_runtime::{set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn live_debug_senders_reports_memory_and_db_diff() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(Some(SignLicenseContext {
        server_url: "https://license.example.com".to_string(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "owner-a".to_string(),
        sku: "trial".to_string(),
        account_limit: 10,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));

    let config = AppConfig::from_env().expect("config loads");
    let token = encode_access_token(&config.auth, "admin", "admin", false).expect("token encodes");
    let state = AppState::bootstrap(config).await.expect("state bootstraps");
    sqlx::query(
        "INSERT INTO accounts (uid, nick, status, owner_username) VALUES (?, ?, 'active', ?)",
    )
    .bind("db-only")
    .bind("DB Only")
    .bind("owner-a")
    .execute(state.storage().accounts())
    .await
    .expect("active account");
    state
        .storage()
        .upsert_live_connection_state("db-only", "connected", 0, "")
        .await
        .expect("db connection state");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    state
        .live_senders()
        .register("live-uid".to_string(), tx)
        .await;

    let response = build_app(state)
        .oneshot(
            Request::get("/api/live/debug/senders")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let payload = json_body(response).await;
    assert_eq!(payload["memory"]["total"], 1);
    assert_eq!(payload["memory"]["alive"], 1);
    assert_eq!(payload["memory"]["uids"][0]["uid"], "live-uid");
    assert_eq!(payload["db"]["connectedUids"][0], "db-only");
    assert_eq!(payload["diff"]["dbConnectedMissingSender"][0], "db-only");
    assert_eq!(payload["diff"]["senderMissingDbConnected"][0], "live-uid");
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::remove_var("API_TOKEN");
    set_sign_license_context(None);
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
