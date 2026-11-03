use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

use axum::{
    body::Body,
    extract::{Query, Request as AxumRequest, State},
    http::{header, HeaderValue, Request, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app,
    auth::encode_access_token,
    config::AppConfig,
    license_runtime::{set_sign_license_context, SignLicenseContext},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const TEST_BEARER: &str = "test-api-token";

#[tokio::test]
async fn account_compat_routes_expose_state_backed_baselines() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 10;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let live_pool = connect_pool(&config.sqlite.live_state_db_path).await;
    let user_token = encode_access_token(&config.auth, "test-user", "user", false).unwrap();

    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("test-user")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    seed_live_state(&live_pool).await;

    let accounts_response = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(accounts_response.status(), StatusCode::OK);
    let accounts_payload = json_body(accounts_response).await;
    assert_eq!(accounts_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(accounts_payload[0]["uid"], "uid-1");
    assert_eq!(accounts_payload[0]["location"]["city"], "Hangzhou");
    assert_eq!(accounts_payload[0]["liveStatus"], "connected");
    assert_eq!(
        accounts_payload[0]["liveLastConnectedAt"],
        json!(1_715_000_180.0_f64)
    );
    assert_eq!(
        accounts_payload[0]["liveLastDisconnectedAt"],
        json!(0.0_f64)
    );

    let location_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/uid-1/location")
                .header("authorization", format!("Bearer {user_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "city": "Shanghai",
                        "latitude": "30.2741",
                        "longitude": "120.1551"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(location_response.status(), StatusCode::OK);
    let location_payload = json_body(location_response).await;
    assert_eq!(location_payload["status"], "ok");
    assert_eq!(location_payload["location"]["gps"], "30.2741,120.1551");

    let summary_response = app
        .clone()
        .oneshot(
            Request::get("/api/account/summary?uid=uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(summary_response.status(), StatusCode::OK);
    let summary_payload = json_body(summary_response).await;
    assert_eq!(summary_payload["securityScore"]["totalScore"], 88);
    assert_eq!(summary_payload["coinBalance"], 123);
    assert_eq!(summary_payload["violations"]["totalCount"], 2);

    let profile_response = app
        .clone()
        .oneshot(
            Request::get("/api/account/profile?uid=uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(profile_response.status(), StatusCode::OK);
    let profile_payload = json_body(profile_response).await;
    assert_eq!(profile_payload["uid"], "uid-1");

    let security_score_response = app
        .clone()
        .oneshot(
            Request::get("/api/account/security_score?uid=uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(security_score_response.status(), StatusCode::OK);
    let security_score_payload = json_body(security_score_response).await;
    assert_eq!(security_score_payload["data"]["data"]["totalScore"], 88);

    let sessions_response = app
        .clone()
        .oneshot(
            Request::get("/api/messages/pc/sessions?uid=uid-1&num=30")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sessions_response.status(), StatusCode::OK);
    let sessions_payload = json_body(sessions_response).await;
    assert_eq!(
        sessions_payload["normalized"]["sessions"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        sessions_payload["normalized"]["sessions"][0]["itemTitle"],
        "Desk Lamp"
    );

    let batch_sessions_response = app
        .clone()
        .oneshot(
            Request::get("/api/messages/pc/sessions/batch?num=30")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(batch_sessions_response.status(), StatusCode::OK);
    let batch_sessions_payload = json_body(batch_sessions_response).await;
    assert_eq!(
        batch_sessions_payload["accounts"]["uid-1"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );

    let history_response = app
        .clone()
        .oneshot(
            Request::get(
                "/api/messages/pc/history?uid=uid-1&session_id=session-1&session_type=1&num=30&ts=0",
            )
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(history_response.status(), StatusCode::OK);
    let history_payload = json_body(history_response).await;
    assert_eq!(
        history_payload["normalized"]["messages"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(history_payload["normalized"]["latestCursor"], 101);

    let session_info_response = app
        .clone()
        .oneshot(
            Request::get(
                "/api/messages/pc/session-info?uid=uid-1&session_id=session-1&session_type=1",
            )
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session_info_response.status(), StatusCode::OK);
    let session_info_payload = json_body(session_info_response).await;
    assert_eq!(session_info_payload["peerNick"], "Buyer A");

    let health_response = app
        .clone()
        .oneshot(
            Request::get("/api/accounts/health")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health_response.status(), StatusCode::OK);
    let health_payload = json_body(health_response).await;
    assert_eq!(health_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(health_payload[0]["healthy"], true);

    sqlx::query("UPDATE accounts SET status = 'disabled' WHERE uid = ?")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();

    for request in [
        Request::patch("/api/accounts/uid-1")
            .header("authorization", format!("Bearer {user_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "autoReply": false }).to_string()))
            .unwrap(),
        Request::post("/api/accounts/uid-1/location")
            .header("authorization", format!("Bearer {user_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({ "city": "Ningbo" }).to_string()))
            .unwrap(),
        Request::delete("/api/accounts/uid-1")
            .header("authorization", format!("Bearer {user_token}"))
            .body(Body::empty())
            .unwrap(),
    ] {
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    sqlx::query("UPDATE accounts SET status = 'active' WHERE uid = ?")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();

    let refresh_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/uid-1/refresh")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refresh_response.status(), StatusCode::OK);
    let refresh_payload = json_body(refresh_response).await;
    // Refresh exercises real passport/MTOP hosts; in sealed test envs the
    // status can be either "ok" (network-reachable) or "error" (soft-failure
    // path). Both are valid smoke outcomes — what matters is the response
    // shape and that the route returns the canonical fields.
    assert!(
        refresh_payload["status"] == "ok" || refresh_payload["status"] == "error",
        "refresh status must be ok or error, got {}",
        refresh_payload["status"]
    );
    assert_eq!(refresh_payload["uid"], "uid-1");
    assert!(refresh_payload.get("detail").is_some());
    assert!(refresh_payload.get("reason").is_some());

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let qr_generate_payload = json_body(qr_generate_response).await;
    let session_id = qr_generate_payload["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(qr_generate_payload["qr_code_url"]
        .as_str()
        .unwrap()
        .starts_with("data:image/svg+xml"));

    let qr_status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_status_response.status(), StatusCode::OK);
    let qr_status_payload = json_body(qr_status_response).await;
    assert_eq!(qr_status_payload["status"], "waiting");

    let delete_response = app
        .clone()
        .oneshot(
            Request::delete("/api/accounts/uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let accounts_after_delete = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let accounts_after_delete_payload = json_body(accounts_after_delete).await;
    assert_eq!(
        accounts_after_delete_payload.as_array().map(Vec::len),
        Some(0)
    );

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[derive(Clone, Default)]
struct PassportState {
    query_count: Arc<Mutex<usize>>,
}

#[derive(Clone)]
struct LicenseServerState {
    approved: Arc<Mutex<bool>>,
    created: Arc<Mutex<Vec<Value>>>,
    reject_after: Arc<Mutex<Option<usize>>>,
    account_limit: Arc<Mutex<i64>>,
    healthy: Arc<Mutex<bool>>,
    claims: Arc<Mutex<Vec<Value>>>,
    claim_conflict: Arc<Mutex<bool>>,
    releases: Arc<Mutex<Vec<Value>>>,
    release_failure: Arc<Mutex<bool>>,
    runtime_access: Arc<Mutex<HashMap<String, Value>>>,
    unbind_owner_override: Arc<Mutex<Option<String>>>,
}

impl Default for LicenseServerState {
    fn default() -> Self {
        Self {
            approved: Arc::new(Mutex::new(false)),
            created: Arc::new(Mutex::new(Vec::new())),
            reject_after: Arc::new(Mutex::new(None)),
            account_limit: Arc::new(Mutex::new(0)),
            healthy: Arc::new(Mutex::new(true)),
            claims: Arc::new(Mutex::new(Vec::new())),
            claim_conflict: Arc::new(Mutex::new(false)),
            releases: Arc::new(Mutex::new(Vec::new())),
            release_failure: Arc::new(Mutex::new(false)),
            runtime_access: Arc::new(Mutex::new(HashMap::new())),
            unbind_owner_override: Arc::new(Mutex::new(None)),
        }
    }
}

#[tokio::test]
async fn qr_routes_generate_real_qr_and_poll_upstream_status() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 10;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let app = build_app(state);

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let qr_generate_payload = json_body(qr_generate_response).await;
    let qr_url = qr_generate_payload["qr_code_url"].as_str().unwrap();
    let session_id = qr_generate_payload["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(qr_generate_payload["expires_in"], 0);
    assert!(qr_url.starts_with("data:image/svg+xml"));
    assert!(!qr_url.contains("XY "));

    let qr_status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_status_response.status(), StatusCode::OK);
    let qr_status_payload = json_body(qr_status_response).await;
    assert_eq!(qr_status_payload["expires_in"], 0);
    assert_eq!(qr_status_payload["status"], "scanned");

    let qr_status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_status_response.status(), StatusCode::OK);
    let qr_status_payload = json_body(qr_status_response).await;
    assert_eq!(qr_status_payload["status"], "success");

    let imported_account_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE uid = ?")
            .bind("uid-qr")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(imported_account_count, 1);

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_import_uses_remote_license_token_for_web_user_quota() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 2;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-web", "user", false).unwrap();

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let payload = json_body(second).await;
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["account_uid"], "uid-qr");

    let owner: String = sqlx::query_scalar("SELECT owner_username FROM accounts WHERE uid = ?")
        .bind("uid-qr")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(owner, "user-web");

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_status_expires_using_local_ttl_and_reports_remaining_seconds() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    env::set_var("XIANYU_QR_LOCAL_TTL_SECS", "1");

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 10;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state);

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let qr_generate_payload = json_body(qr_generate_response).await;
    let session_id = qr_generate_payload["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(qr_generate_payload["expires_in"], 1);

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    let qr_status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_status_response.status(), StatusCode::OK);
    let qr_status_payload = json_body(qr_status_response).await;
    assert_eq!(qr_status_payload["status"], "expired");
    assert_eq!(qr_status_payload["expires_in"], 0);
    assert!(qr_status_payload["message"]
        .as_str()
        .unwrap()
        .contains("二维码已过期"));

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    env::remove_var("XIANYU_QR_LOCAL_TTL_SECS");
    clear_env();
}

#[tokio::test]
async fn qr_import_rejects_new_account_when_license_quota_is_full() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 1;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let app = build_app(state);

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let payload = json_body(second).await;
    assert_eq!(payload["status"], "import_error");
    assert!(payload["message"]
        .as_str()
        .unwrap_or_default()
        .contains("当前套餐最多添加 1 个账号"));

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_import_allows_existing_account_refresh_when_quota_is_full() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_existing_account_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 1;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    let app = build_app(state);

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let payload = json_body(second).await;
    assert_eq!(payload["status"], "success");

    let account_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(account_count, 1);

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_import_rejects_cross_owner_existing_uid_without_overwriting_owner() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_existing_account_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let user_a_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();
    let user_b_token = encode_access_token(&config.auth, "user-b", "user", false).unwrap();
    let app = build_app(state);

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let conflict_payload = json_body(second).await;
    assert_eq!(conflict_payload["status"], "owner_conflict");
    assert_eq!(conflict_payload["account_uid"], "uid-1");
    assert_eq!(conflict_payload["can_apply_unbind"], true);
    let conflict_message = conflict_payload["message"].as_str().unwrap_or_default();
    assert!(conflict_message.contains("该账号已绑定到其他用户"));
    assert!(conflict_message.contains("为防止该账号多端登录频繁请求引起不必要的风控"));
    assert!(conflict_message.contains("你可以提交解绑申请，管理员处理后再重新扫码添加"));

    let request_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/unbind-requests")
                .header("authorization", format!("Bearer {user_b_token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"account_uid": "uid-1"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(request_response.status(), StatusCode::OK);
    let request_payload = json_body(request_response).await;
    assert_eq!(request_payload["status"], "pending");
    assert_eq!(request_payload["account_uid"], "uid-1");
    assert_eq!(request_payload["id"], 9001);

    let created = license_state.created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["account_uid"], "uid-1");
    assert_eq!(created[0]["existing_owner_username"], "user-a");

    let row: (String, String, String) =
        sqlx::query_as("SELECT owner_username, cookies, status FROM accounts WHERE uid = ?")
            .bind("uid-1")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(row.0, "user-a");
    assert_eq!(row.1, r#"{"_m_h5_tk":"token-1","sid":"sid-1"}"#);
    assert_eq!(row.2, "active");

    *license_state.approved.lock().unwrap() = true;
    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let approved_session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let approved_poll = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{approved_session_id}"))
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approved_poll.status(), StatusCode::OK);
    let approved_payload = json_body(approved_poll).await;
    assert_eq!(approved_payload["status"], "success");

    let transferred: (String, String, String, f64, f64) = sqlx::query_as(
        "SELECT owner_username, cookies, status, created_at, bound_at FROM accounts WHERE uid = ?",
    )
    .bind("uid-1")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(transferred.0, "user-b");
    assert_ne!(transferred.1, "{}");
    assert_eq!(transferred.2, "expired");
    assert_eq!(transferred.3, 1_715_000_000.0_f64);
    assert!(transferred.4 > transferred.3);

    let new_owner_accounts = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(new_owner_accounts.status(), StatusCode::OK);
    let new_owner_payload = json_body(new_owner_accounts).await;
    let new_owner_row = new_owner_payload
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["uid"] == "uid-1")
        .expect("transferred uid is visible to new owner");
    assert_eq!(new_owner_row["created_at"], json!(1_715_000_000.0_f64));
    assert_eq!(new_owner_row["bound_at"], json!(transferred.4));
    assert_eq!(new_owner_row["added_at"], json!(transferred.4));
    assert_eq!(new_owner_row["addedAt"], json!(transferred.4));

    let old_owner_accounts = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_a_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old_owner_accounts.status(), StatusCode::OK);
    let old_owner_payload = json_body(old_owner_accounts).await;
    assert!(!old_owner_payload
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["uid"] == "uid-1"));

    tokio::time::sleep(std::time::Duration::from_millis(1_100)).await;
    let user_c_token = encode_access_token(&config.auth, "user-c", "user", false).unwrap();
    let c_qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_c_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(c_qr_generate_response.status(), StatusCode::OK);
    let c_session_id = json_body(c_qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    let c_poll = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{c_session_id}"))
                .header("authorization", format!("Bearer {user_c_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(c_poll.status(), StatusCode::OK);
    let c_payload = json_body(c_poll).await;
    assert_eq!(c_payload["status"], "success");

    let transferred_to_c: (String, f64) =
        sqlx::query_as("SELECT owner_username, bound_at FROM accounts WHERE uid = ?")
            .bind("uid-1")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(transferred_to_c.0, "user-c");
    assert!(transferred_to_c.1 > transferred.4);

    let c_accounts = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_c_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(c_accounts.status(), StatusCode::OK);
    let c_accounts_payload = json_body(c_accounts).await;
    let c_row = c_accounts_payload
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["uid"] == "uid-1")
        .expect("uid is visible to the latest owner");
    assert_eq!(c_row["added_at"], json!(transferred_to_c.1));
    assert_eq!(c_row["addedAt"], json!(transferred_to_c.1));

    let previous_owner_accounts = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(previous_owner_accounts.status(), StatusCode::OK);
    let previous_owner_payload = json_body(previous_owner_accounts).await;
    assert!(!previous_owner_payload
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["uid"] == "uid-1"));

    let previous_owner_refresh = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/uid-1/refresh")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(previous_owner_refresh.status(), StatusCode::NOT_FOUND);

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    clear_env();
}

#[tokio::test]
async fn qr_generate_is_blocked_when_license_service_is_unhealthy() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);

    let license_state = LicenseServerState::default();
    *license_state.healthy.lock().unwrap() = false;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-web", "user", false).unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(json_body(response).await["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("鉴权服务"));

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_import_claims_cloud_binding_before_local_insert() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 2;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-web", "user", false).unwrap();

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let payload = json_body(second).await;
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["account_uid"], "uid-qr");

    let claims = license_state.claims.lock().unwrap().clone();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0]["account_uid"], "uid-qr");

    let owner: String = sqlx::query_scalar("SELECT owner_username FROM accounts WHERE uid = ?")
        .bind("uid-qr")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(owner, "user-web");

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_import_keeps_local_db_empty_when_cloud_claim_conflicts() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 2;
    *license_state.claim_conflict.lock().unwrap() = true;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-web", "user", false).unwrap();

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    let payload = json_body(second).await;
    assert_eq!(payload["status"], "owner_conflict");
    assert_eq!(payload["can_apply_unbind"], true);
    assert_eq!(
        payload["message"],
        "该账号已绑定到其他用户，为防止该账号多端登录频繁请求引起不必要的风控，你可以提交解绑申请，管理员处理后再重新扫码添加。"
    );

    let claims = license_state.claims.lock().unwrap().clone();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0]["account_uid"], "uid-qr");

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE uid = ?")
        .bind("uid-qr")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn account_unbind_requests_are_limited_to_three_attempts_per_user_and_account() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let license_state = LicenseServerState::default();
    *license_state.reject_after.lock().unwrap() = Some(3);
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_b_token = encode_access_token(&config.auth, "user-b", "user", false).unwrap();

    for _ in 0..3 {
        let create_response = app
            .clone()
            .oneshot(
                Request::post("/api/accounts/unbind-requests")
                    .header("authorization", format!("Bearer {user_b_token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"account_uid": "uid-1"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(create_response.status(), StatusCode::OK);
    }

    let fourth_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/unbind-requests")
                .header("authorization", format!("Bearer {user_b_token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"account_uid": "uid-1"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fourth_response.status(), StatusCode::BAD_REQUEST);
    assert!(json_body(fourth_response).await["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("最多申请解绑 3 次"));

    let _ = license_shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn admin_can_list_and_approve_local_unbind_requests_before_qr_transfer_falls_back_remote() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_existing_account_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_b_token = encode_access_token(&config.auth, "user-b", "user", false).unwrap();
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();

    let request_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/unbind-requests")
                .header("authorization", format!("Bearer {user_b_token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"account_uid": "uid-1"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(request_response.status(), StatusCode::OK);

    let local_row: (i64, String, String, String, String) = sqlx::query_as(
        "SELECT id, requester_username, existing_owner_username, status, account_snapshot FROM account_unbind_requests WHERE account_uid = ? LIMIT 1",
    )
    .bind("uid-1")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(local_row.1, "user-b");
    assert_eq!(local_row.2, "user-a");
    assert_eq!(local_row.3, "pending");
    let snapshot: Value = serde_json::from_str(&local_row.4).unwrap();
    assert_eq!(snapshot["owner_username"], "user-a");
    assert_eq!(snapshot["status"], "active");

    let user_list_response = app
        .clone()
        .oneshot(
            Request::get("/api/admin/account-unbind-requests")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(user_list_response.status(), StatusCode::FORBIDDEN);

    let admin_list_response = app
        .clone()
        .oneshot(
            Request::get("/api/admin/account-unbind-requests?status=pending")
                .header("authorization", format!("Bearer {admin_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(admin_list_response.status(), StatusCode::OK);
    let admin_list_payload = json_body(admin_list_response).await;
    assert_eq!(admin_list_payload["count"], 1);
    assert_eq!(admin_list_payload["data"][0]["id"], local_row.0);
    assert_eq!(admin_list_payload["data"][0]["account_uid"], "uid-1");
    assert_eq!(
        admin_list_payload["data"][0]["requester_username"],
        "user-b"
    );

    let approve_response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/admin/account-unbind-requests/{}/approve",
                local_row.0
            ))
            .header("authorization", format!("Bearer {admin_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"note": "approved locally"}).to_string()))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(approve_response.status(), StatusCode::OK);
    let approve_payload = json_body(approve_response).await;
    assert_eq!(approve_payload["status"], "approved");
    assert_eq!(approve_payload["reviewed_by"], "admin");

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let first = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(first).await["status"], "scanned");

    let second = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {user_b_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(json_body(second).await["status"], "success");

    let transferred: (String, String) =
        sqlx::query_as("SELECT owner_username, status FROM accounts WHERE uid = ?")
            .bind("uid-1")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(transferred.0, "user-b");
    assert_eq!(transferred.1, "expired");

    let completed_status: String =
        sqlx::query_scalar("SELECT status FROM account_unbind_requests WHERE id = ?")
            .bind(local_row.0)
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(completed_status, "completed");

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    clear_env();
}

#[tokio::test]
async fn admin_reject_restores_account_snapshot_from_local_unbind_request() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let license_state = LicenseServerState::default();
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_b_token = encode_access_token(&config.auth, "user-b", "user", false).unwrap();
    let user_a_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();

    let request_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/unbind-requests")
                .header("authorization", format!("Bearer {user_b_token}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"account_uid": "uid-1"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(request_response.status(), StatusCode::OK);

    let request_id: i64 = sqlx::query_scalar(
        "SELECT id FROM account_unbind_requests WHERE account_uid = ? AND requester_username = ? LIMIT 1",
    )
    .bind("uid-1")
    .bind("user-b")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();

    let reject_response = app
        .clone()
        .oneshot(
            Request::post(format!(
                "/api/admin/account-unbind-requests/{request_id}/reject"
            ))
            .header("authorization", format!("Bearer {admin_token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"note": "owner verified"}).to_string()))
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(reject_response.status(), StatusCode::OK);
    let reject_payload = json_body(reject_response).await;
    assert_eq!(reject_payload["status"], "rejected");
    assert_eq!(reject_payload["reviewed_by"], "admin");

    let restored: (String, String, String, String, String) = sqlx::query_as(
        "SELECT owner_username, status, last_session_status, last_session_reason, cookies FROM accounts WHERE uid = ?",
    )
    .bind("uid-1")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(restored.0, "user-a");
    assert_eq!(restored.1, "active");
    assert_eq!(restored.2, "ok");
    assert_eq!(restored.3, "");
    assert_eq!(restored.4, r#"{"_m_h5_tk":"token-1","sid":"sid-1"}"#);

    let review_row: (String, String, f64) = sqlx::query_as(
        "SELECT status, review_note, reviewed_at FROM account_unbind_requests WHERE id = ?",
    )
    .bind(request_id)
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(review_row.0, "rejected");
    assert_eq!(review_row.1, "owner verified");
    assert!(review_row.2 > 0.0);

    let owner_accounts_response = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_a_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner_accounts_response.status(), StatusCode::OK);
    let owner_accounts_payload = json_body(owner_accounts_response).await;
    assert!(owner_accounts_payload
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["uid"] == "uid-1"));

    let _ = license_shutdown_tx.send(());
    clear_env();
}

#[tokio::test]
async fn released_from_me_account_can_be_listed_but_cannot_refresh() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let license_state = LicenseServerState::default();
    license_state.runtime_access.lock().unwrap().insert(
        "uid-1".to_string(),
        json!({
            "remote_binding_state": "released_from_me",
            "remote_binding_message": "账号已在云端释放，仅允许查看缓存或删除本地记录",
            "delete_only": true
        }),
    );
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let list_response = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload[0]["remote_binding_state"], "released_from_me");
    assert_eq!(list_payload[0]["delete_only"], true);
    assert!(list_payload[0]["remote_binding_message"]
        .as_str()
        .unwrap_or_default()
        .contains("只能进行删除操作"));

    let refresh_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/uid-1/refresh")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refresh_response.status(), StatusCode::FORBIDDEN);
    assert!(json_body(refresh_response).await["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("删除"));

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn released_from_me_account_can_be_deleted() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let license_state = LicenseServerState::default();
    license_state.runtime_access.lock().unwrap().insert(
        "uid-1".to_string(),
        json!({
            "remote_binding_state": "released_from_me",
            "remote_binding_message": "账号已在云端释放，仅允许查看缓存或删除本地记录",
            "delete_only": true
        }),
    );
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let delete_response = app
        .clone()
        .oneshot(
            Request::delete("/api/accounts/uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE uid = ?")
        .bind("uid-1")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    assert!(license_state.releases.lock().unwrap().is_empty());

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn owned_by_other_account_is_delete_only_and_local_delete_does_not_release_cloud_owner() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let license_state = LicenseServerState::default();
    license_state.runtime_access.lock().unwrap().insert(
        "uid-1".to_string(),
        json!({
            "remote_binding_state": "owned_by_other",
            "remote_binding_message": "该闲鱼账号当前归属于 other-user",
            "delete_only": true
        }),
    );
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);
    set_test_license_context_with_server_url(10, &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let list_response = app
        .clone()
        .oneshot(
            Request::get("/api/accounts")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload[0]["remote_binding_state"], "owned_by_other");
    assert_eq!(list_payload[0]["delete_only"], true);
    assert!(list_payload[0]["remote_binding_message"]
        .as_str()
        .unwrap_or_default()
        .contains("只能进行删除操作"));

    let refresh_response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/uid-1/refresh")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refresh_response.status(), StatusCode::FORBIDDEN);
    assert!(json_body(refresh_response).await["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("只能进行删除操作"));

    let delete_response = app
        .clone()
        .oneshot(
            Request::delete("/api/accounts/uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE uid = ?")
        .bind("uid-1")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    assert!(license_state.releases.lock().unwrap().is_empty());

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn delete_account_releases_cloud_binding_before_local_delete() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let license_state = LicenseServerState::default();
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::delete("/api/accounts/uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let releases = license_state.releases.lock().unwrap().clone();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0]["account_uid"], "uid-1");
    assert_eq!(releases[0]["reason"], "owner_deleted");

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE uid = ?")
        .bind("uid-1")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(remaining, 0);

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn delete_account_keeps_local_row_when_cloud_release_fails() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let license_state = LicenseServerState::default();
    *license_state.release_failure.lock().unwrap() = true;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-a")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::delete("/api/accounts/uid-1")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE uid = ?")
        .bind("uid-1")
        .fetch_one(&accounts_pool)
        .await
        .unwrap();
    assert_eq!(remaining, 1);

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn create_account_unbind_request_does_not_require_local_conflicting_row() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let license_state = LicenseServerState::default();
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state.clone()).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-b", "user", false).unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/unbind-requests")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"account_uid": "uid-cloud-only"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["status"], "pending");
    assert_eq!(payload["account_uid"], "uid-cloud-only");

    let created = license_state.created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["account_uid"], "uid-cloud-only");

    let local_row: (String, String, String) = sqlx::query_as(
        "SELECT requester_username, existing_owner_username, account_snapshot FROM account_unbind_requests WHERE account_uid = ? LIMIT 1",
    )
    .bind("uid-cloud-only")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(local_row.0, "user-b");
    assert_eq!(local_row.1, "cloud-owner");
    assert_eq!(local_row.2, "{}");

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn create_account_unbind_request_uses_cloud_owner_even_when_local_owner_matches_requester() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_sign_license_context(None);

    let license_state = LicenseServerState::default();
    *license_state.unbind_owner_override.lock().unwrap() = Some("cloud-owner".to_string());
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;
    sqlx::query("UPDATE accounts SET owner_username = ? WHERE uid = ?")
        .bind("user-b")
        .bind("uid-1")
        .execute(&accounts_pool)
        .await
        .unwrap();
    let app = build_app(state);
    let user_token = encode_access_token(&config.auth, "user-b", "user", false).unwrap();

    let response = app
        .clone()
        .oneshot(
            Request::post("/api/accounts/unbind-requests")
                .header("authorization", format!("Bearer {user_token}"))
                .header("x-license-auth-token", "auth-token")
                .header("content-type", "application/json")
                .body(Body::from(json!({"account_uid": "uid-1"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    assert_eq!(payload["status"], "pending");

    let local_row: (String, String) = sqlx::query_as(
        "SELECT requester_username, existing_owner_username FROM account_unbind_requests WHERE account_uid = ? ORDER BY id DESC LIMIT 1",
    )
    .bind("uid-1")
    .fetch_one(&accounts_pool)
    .await
    .unwrap();
    assert_eq!(local_row.0, "user-b");
    assert_eq!(local_row.1, "cloud-owner");

    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

#[tokio::test]
async fn qr_status_reports_import_error_when_account_persist_fails() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let passport = PassportState::default();
    let (passport_url, shutdown_tx) = spawn_passport(passport).await;
    env::set_var("XIANYU_QR_PASSPORT_HOST", &passport_url);
    let license_state = LicenseServerState::default();
    *license_state.account_limit.lock().unwrap() = 10;
    let (license_url, license_shutdown_tx) = spawn_license_server(license_state).await;
    env::set_var("XIANYU_LICENSE_SERVER_URL", &license_url);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    let app = build_app(state);

    let qr_generate_response = app
        .clone()
        .oneshot(
            Request::post("/api/qr/generate")
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .header("x-license-auth-token", "auth-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_generate_response.status(), StatusCode::OK);
    let session_id = json_body(qr_generate_response).await["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let qr_status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_status_response.status(), StatusCode::OK);
    let qr_status_payload = json_body(qr_status_response).await;
    assert_eq!(qr_status_payload["status"], "scanned");

    sqlx::query("DROP TABLE accounts")
        .execute(&accounts_pool)
        .await
        .unwrap();

    let qr_status_response = app
        .clone()
        .oneshot(
            Request::get(format!("/api/qr/status/{session_id}"))
                .header("authorization", format!("Bearer {TEST_BEARER}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(qr_status_response.status(), StatusCode::OK);
    let qr_status_payload = json_body(qr_status_response).await;
    assert_eq!(qr_status_payload["status"], "import_error");
    assert!(qr_status_payload["message"]
        .as_str()
        .unwrap_or_default()
        .contains("导入失败"));

    let _ = shutdown_tx.send(());
    let _ = license_shutdown_tx.send(());
    env::remove_var("XIANYU_QR_PASSPORT_HOST");
    env::remove_var("XIANYU_LICENSE_SERVER_URL");
    clear_env();
}

async fn spawn_passport(state: PassportState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/mini_login.htm", get(passport_mini_login))
        .route("/newlogin/qrcode/generate.do", get(passport_qr_generate))
        .route("/newlogin/qrcode/query.do", post(passport_qr_query))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn spawn_existing_account_passport(state: PassportState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/mini_login.htm", get(passport_existing_mini_login))
        .route("/newlogin/qrcode/generate.do", get(passport_qr_generate))
        .route("/newlogin/qrcode/query.do", post(passport_qr_query))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn spawn_license_server(state: LicenseServerState) -> (String, oneshot::Sender<()>) {
    let app = Router::new()
        .route("/health", get(license_health))
        .route("/api/me/licenses", get(license_list_my_licenses))
        .route(
            "/api/me/xianyu-account-bindings/resolve",
            post(license_resolve_xianyu_account_bindings),
        )
        .route(
            "/api/me/xianyu-account-bindings/runtime-access",
            get(license_runtime_access),
        )
        .route(
            "/api/me/xianyu-account-bindings/claim",
            post(license_claim_xianyu_account_binding),
        )
        .route(
            "/api/me/xianyu-account-bindings/release",
            post(license_release_xianyu_account_binding),
        )
        .route(
            "/api/me/account-unbind-requests",
            post(license_create_account_unbind_request),
        )
        .route(
            "/api/me/account-unbind-requests/approved",
            get(license_check_account_unbind_approved),
        )
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{}", addr), shutdown_tx)
}

async fn license_health(State(state): State<LicenseServerState>) -> impl IntoResponse {
    if *state.healthy.lock().unwrap() {
        (StatusCode::OK, axum::Json(json!({"status": "ok"})))
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({"detail": "license unhealthy"})),
        )
    }
}

async fn license_list_my_licenses(
    State(state): State<LicenseServerState>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let account_limit = *state.account_limit.lock().unwrap();
    let issued_at = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339();
    (
        StatusCode::OK,
        axum::Json(json!([
            {
                "id": 7,
                "user_id": 10,
                "sku": "trial",
                "license_key": "TEST-LICENSE",
                "signed_token": "signed-token",
                "issued_at": issued_at,
                "expires_at": expires_at,
                "revoked_at": null,
                "revoke_reason": null,
                "bound_device_id": null,
                "device_public_key_hex": null,
                "device_key_registered_at": null,
                "order_id": null,
                "account_limit": account_limit
            }
        ])),
    )
}

async fn license_create_account_unbind_request(
    State(state): State<LicenseServerState>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let body = request.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    let mut created = state.created.lock().unwrap();
    let reject_after = *state.reject_after.lock().unwrap();
    if reject_after.is_some_and(|max| created.len() >= max) {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(json!({"detail": "同一账号最多申请解绑 3 次"})),
        );
    }

    created.push(payload.clone());
    let existing_owner_override = state.unbind_owner_override.lock().unwrap().clone();
    let existing_owner = existing_owner_override.as_deref().unwrap_or_else(|| {
        payload["existing_owner_username"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("cloud-owner")
    });
    let account_nick = payload["account_nick"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("Cloud Account");
    (
        StatusCode::OK,
        axum::Json(json!({
            "id": 9001,
            "user_id": 10,
            "username": "user-b",
            "license_id": 7,
            "license_key": "TEST-LICENSE",
            "account_uid": payload["account_uid"],
            "account_nick": account_nick,
            "existing_owner_username": existing_owner,
            "reason": payload["reason"],
            "status": "pending",
            "attempt_count": created.len(),
            "max_attempts": 3
        })),
    )
}

async fn license_claim_xianyu_account_binding(
    State(state): State<LicenseServerState>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let body = request.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    state.claims.lock().unwrap().push(payload.clone());
    if *state.claim_conflict.lock().unwrap() {
        return (
            StatusCode::CONFLICT,
            axum::Json(json!({"detail": "xianyu account already owned by cloud-owner"})),
        );
    }
    (
        StatusCode::OK,
        axum::Json(json!({
            "account_uid": payload["account_uid"],
            "account_nick": payload["account_nick"],
            "status": "active",
            "owner_username": "user-web"
        })),
    )
}

async fn license_runtime_access(
    State(state): State<LicenseServerState>,
    Query(query): Query<HashMap<String, String>>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let account_uid = query.get("account_uid").cloned().unwrap_or_default();
    let payload = state
        .runtime_access
        .lock()
        .unwrap()
        .get(&account_uid)
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "remote_binding_state": "active",
                "remote_binding_message": "",
                "delete_only": false
            })
        });
    (StatusCode::OK, axum::Json(payload))
}

async fn license_resolve_xianyu_account_bindings(
    State(state): State<LicenseServerState>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let body = request.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
    let account_uids = payload
        .get("account_uids")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let runtime_access = state.runtime_access.lock().unwrap();
    let resolved = account_uids
        .iter()
        .filter_map(Value::as_str)
        .map(|account_uid| {
            let runtime = runtime_access
                .get(account_uid)
                .cloned()
                .unwrap_or_else(|| json!({}));
            let status = runtime
                .get("remote_binding_state")
                .and_then(Value::as_str)
                .unwrap_or("owned_by_me");
            let message = runtime
                .get("remote_binding_message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({
                "account_uid": account_uid,
                "status": status,
                "owner_username": if status == "owned_by_other" { json!("other-user") } else { Value::Null },
                "message": message,
            })
        })
        .collect::<Vec<_>>();
    (StatusCode::OK, axum::Json(Value::Array(resolved)))
}

async fn license_release_xianyu_account_binding(
    State(state): State<LicenseServerState>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let body = request.into_body().collect().await.unwrap().to_bytes();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    state.releases.lock().unwrap().push(payload.clone());
    if *state.release_failure.lock().unwrap() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({"detail": "release failed"})),
        );
    }
    (
        StatusCode::OK,
        axum::Json(json!({
            "account_uid": payload["account_uid"],
            "status": "released"
        })),
    )
}

async fn license_check_account_unbind_approved(
    State(state): State<LicenseServerState>,
    Query(query): Query<HashMap<String, String>>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == "Bearer auth-token");
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({"detail": "invalid license token"})),
        );
    }

    let approved = *state.approved.lock().unwrap();
    let account_uid = query.get("account_uid").cloned().unwrap_or_default();
    (
        StatusCode::OK,
        axum::Json(json!({
            "approved": approved,
            "request": if approved {
                json!({
                    "id": 9001,
                    "account_uid": account_uid,
                    "status": "approved"
                })
            } else {
                Value::Null
            }
        })),
    )
}

async fn passport_mini_login() -> impl IntoResponse {
    let html = r#"
    <html>
      <head>
        <meta name="eagleeye-trace" content="trace-123" />
        <script>
          window.viewData = {"loginFormData":{"appName":"xianyu","appEntrance":"web","_csrf_token":"csrf-123","umidToken":"umid-123","hsiz":"hsiz-123","bizParams":"","mainPage":"https://www.goofish.com/","isMobile":false,"lang":"zh_cn","returnUrl":"https://www.goofish.com/","fromSite":"77"}};
        </script>
      </head>
      <body>"defaultView":"sms"</body>
    </html>
    "#;
    let mut response = axum::response::Response::new(Body::from(html));
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("unb=uid-qr; Path=/"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("cookie2=cookie-two; Path=/"),
    );
    response
}

async fn passport_existing_mini_login() -> impl IntoResponse {
    let mut response = passport_mini_login().await.into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("unb=uid-1; Path=/"),
    );
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_static("cookie2=cookie-two; Path=/"),
    );
    response
}

async fn passport_qr_generate(Query(_query): Query<HashMap<String, String>>) -> impl IntoResponse {
    axum::Json(json!({
        "content": {
            "success": true,
            "data": {
                "t": "t-123",
                "ck": "ck-123",
                "codeContent": "https://passport.goofish.com/mock-qr-content"
            }
        }
    }))
}

async fn passport_qr_query(
    State(state): State<PassportState>,
    Query(_query): Query<HashMap<String, String>>,
    request: AxumRequest<Body>,
) -> impl IntoResponse {
    let _body = request.into_body().collect().await.unwrap().to_bytes();
    let mut count = state.query_count.lock().unwrap();
    *count += 1;
    let qr_status = if *count == 1 { "SCANED" } else { "CONFIRMED" };
    axum::Json(json!({
        "content": {
            "data": {
                "qrCodeStatus": qr_status
            }
        }
    }))
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid, nick, token, sid, cookies, browser_cookie_jar, status, created_at, updated_at,
            cookie_refreshed_at, last_session_check_at, last_session_ok_at, first_session_fail_at,
            last_session_status, last_session_reason, extra, security_score, security_score_updated_at,
            restriction_end, coin_balance, violation_count, fish_power, seller_30d_sold
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("uid-1")
    .bind("Alice")
    .bind("token-1")
    .bind("sid-1")
    .bind(r#"{"_m_h5_tk":"token-1","sid":"sid-1"}"#)
    .bind(r#"[{"name":"sid","value":"sid-1"}]"#)
    .bind("active")
    .bind(1_715_000_000.0_f64)
    .bind(1_715_000_100.0_f64)
    .bind(1_715_000_050.0_f64)
    .bind(1_715_000_060.0_f64)
    .bind(1_715_000_060.0_f64)
    .bind(0.0_f64)
    .bind("ok")
    .bind("")
    .bind(r#"{"avatar":"https://img.example/avatar.png","location":{"city":"Hangzhou","province":"Zhejiang"}}"#)
    .bind(88_i64)
    .bind(1_715_000_070.0_f64)
    .bind("2026-04-20")
    .bind(123_i64)
    .bind(2_i64)
    .bind(66_i64)
    .bind("888.00")
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_live_state(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO live_connection_state (
            uid, status, last_connected_at, last_disconnected_at, last_error, reconnect_attempts, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("uid-1")
    .bind("connected")
    .bind(1_715_000_180.0_f64)
    .bind(0.0_f64)
    .bind("")
    .bind(0_i64)
    .bind(1_715_000_180.0_f64)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        r#"
        INSERT INTO message_live_state (
            uid, session_id, session_type, latest_version, unread_count, peer_uid, peer_nick, peer_role,
            peer_logo, self_role, item_id, item_title, item_pic, summary, last_message_ts, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind("uid-1")
    .bind("session-1")
    .bind(1_i64)
    .bind(101_i64)
    .bind(3_i64)
    .bind("peer-1")
    .bind("Buyer A")
    .bind("buyer")
    .bind("https://img.example/peer.png")
    .bind("seller")
    .bind("item-1")
    .bind("Desk Lamp")
    .bind("https://img.example/item.png")
    .bind("你好，还在吗")
    .bind(1_715_000_200_000_i64)
    .bind(1_715_000_200.0_f64)
    .execute(pool)
    .await
    .unwrap();

    for (message_id, version, ts, text, is_self) in [
        ("m-1", 100_i64, 1_715_000_100_000_i64, "你好", 0_i64),
        ("m-2", 101_i64, 1_715_000_110_000_i64, "还在的", 1_i64),
    ] {
        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, version, ts, kind, title, text, image_url,
                image_urls, content_type, preview, sender_nick, sender_logo, is_self, cached_at
            )
            VALUES (?, ?, ?, ?, ?, ?, 'text', '', ?, '', '[]', 1, ?, ?, ?, ?, ?)
            "#,
        )
        .bind("uid-1")
        .bind("session-1")
        .bind(1_i64)
        .bind(message_id)
        .bind(version)
        .bind(ts)
        .bind(text)
        .bind(text)
        .bind(if is_self == 1 { "Alice" } else { "Buyer A" })
        .bind("https://img.example/sender.png")
        .bind(is_self)
        .bind(1_715_000_120.0_f64)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    env::set_var("APP_DATA_DIR", temp_dir.path());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("API_TOKEN", TEST_BEARER);
}

fn clear_env() {
    set_sign_license_context(None);
    env::remove_var("APP_DATA_DIR");
    env::remove_var("JWT_SECRET");
    env::remove_var("API_TOKEN");
    env::remove_var("XIANYU_QR_LOCAL_TTL_SECS");
}

fn set_test_license_context(account_limit: i64) {
    set_test_license_context_with_server_url(account_limit, "https://license.example.com");
}

fn set_test_license_context_with_server_url(account_limit: i64, server_url: &str) {
    set_sign_license_context(Some(SignLicenseContext {
        server_url: server_url.to_string(),
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7,
        username: "test-user".to_string(),
        sku: "trial".to_string(),
        account_limit,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-1".to_string(),
    }));
}
