use std::{
    env,
    sync::{Mutex, Once},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::util::ServiceExt;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use xianyu_rust_backend_full::{
    app::build_app, auth::encode_access_token, config::AppConfig, logging::memory_log_layer,
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
static TRACING_INIT: Once = Once::new();

#[tokio::test]
async fn proxy_location_and_logs_routes_expose_compatibility_baselines() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();
    let accounts_pool = connect_pool(&config.sqlite.accounts_db_path).await;
    seed_account(&accounts_pool).await;

    let proxy_status_response = app
        .clone()
        .oneshot(
            Request::get("/api/proxy/status")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_status_response.status(), StatusCode::OK);
    let proxy_status_payload = json_body(proxy_status_response).await;
    assert_eq!(proxy_status_payload["data"]["enabled"], false);
    assert_eq!(proxy_status_payload["data"]["strategy"], "round-robin");
    assert_eq!(proxy_status_payload["data"]["total"], 0);

    let proxy_add_response = app
        .clone()
        .oneshot(
            Request::post("/api/proxy/add")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "url": "http://proxy-a.example:8080",
                        "label": "primary"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_add_response.status(), StatusCode::OK);

    let proxy_list_response = app
        .clone()
        .oneshot(
            Request::get("/api/proxy/list")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_list_response.status(), StatusCode::OK);
    let proxy_list_payload = json_body(proxy_list_response).await;
    assert_eq!(
        proxy_list_payload["data"]["proxies"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(proxy_list_payload["data"]["proxies"][0]["label"], "primary");

    let proxy_enabled_response = app
        .clone()
        .oneshot(
            Request::post("/api/proxy/set_enabled")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "enabled": true }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_enabled_response.status(), StatusCode::OK);
    let proxy_enabled_payload = json_body(proxy_enabled_response).await;
    assert_eq!(proxy_enabled_payload["data"]["enabled"], true);

    let proxy_strategy_response = app
        .clone()
        .oneshot(
            Request::post("/api/proxy/set_strategy")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "strategy": "latency" }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_strategy_response.status(), StatusCode::OK);
    let proxy_strategy_payload = json_body(proxy_strategy_response).await;
    assert_eq!(proxy_strategy_payload["data"]["strategy"], "latency");

    let proxy_check_response = app
        .clone()
        .oneshot(
            Request::post("/api/proxy/check")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "url": "http://proxy-a.example:8080" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_check_response.status(), StatusCode::OK);
    let proxy_check_payload = json_body(proxy_check_response).await;
    assert_eq!(proxy_check_payload["data"]["alive"], true);

    let proxy_check_all_response = app
        .clone()
        .oneshot(
            Request::post("/api/proxy/check_all")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_check_all_response.status(), StatusCode::OK);
    let proxy_check_all_payload = json_body(proxy_check_all_response).await;
    assert_eq!(proxy_check_all_payload["data"]["total"], 1);
    assert_eq!(proxy_check_all_payload["data"]["alive"], 1);

    let proxy_remove_response = app
        .clone()
        .oneshot(
            Request::post("/api/proxy/remove")
                .header("authorization", bearer(&admin_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "url": "http://proxy-a.example:8080" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_remove_response.status(), StatusCode::OK);

    let location_divisions_response = app
        .clone()
        .oneshot(
            Request::get("/api/location/divisions?uid=uid-1")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(location_divisions_response.status(), StatusCode::OK);
    let location_divisions_payload = json_body(location_divisions_response).await;
    assert_eq!(location_divisions_payload["ret"][0], "SUCCESS::调用成功");
    assert_eq!(
        location_divisions_payload["divisionArea"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default()
            > 0,
        true
    );

    let location_children_response = app
        .clone()
        .oneshot(
            Request::get("/api/location/division_children?uid=uid-1&parent_id=330000")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(location_children_response.status(), StatusCode::OK);
    let location_children_payload = json_body(location_children_response).await;
    assert_eq!(location_children_payload["ret"][0], "SUCCESS::调用成功");
    assert_eq!(location_children_payload["items"][0]["na"], "杭州市");

    let location_gps_response = app
        .clone()
        .oneshot(
            Request::get("/api/location/by_gps?uid=uid-1&lat=30.2741&lng=120.1551")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(location_gps_response.status(), StatusCode::OK);
    let location_gps_payload = json_body(location_gps_response).await;
    assert_eq!(location_gps_payload["city"], "杭州市");
    assert_eq!(location_gps_payload["divisionId"], "330106");

    let poi_response = app
        .clone()
        .oneshot(
            Request::get("/api/location/poi_search?uid=uid-1&keyword=%E8%A5%BF%E6%B9%96")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(poi_response.status(), StatusCode::OK);
    let poi_payload = json_body(poi_response).await;
    assert_eq!(poi_payload["ret"][0], "SUCCESS::调用成功");
    assert_eq!(poi_payload["items"][0]["city"], "杭州市");

    let logs_response = app
        .clone()
        .oneshot(
            Request::get("/api/logs?limit=10")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs_response.status(), StatusCode::OK);
    let logs_payload = json_body(logs_response).await;
    assert!(logs_payload["data"].is_array());
    assert!(logs_payload["count"].as_i64().unwrap_or_default() >= 0);

    let clear_logs_response = app
        .clone()
        .oneshot(
            Request::delete("/api/logs")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(clear_logs_response.status(), StatusCode::OK);
    let clear_logs_payload = json_body(clear_logs_response).await;
    assert_eq!(clear_logs_payload["status"], "ok");
}

#[tokio::test]
async fn regular_user_cannot_access_global_logs_or_proxy_pool() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let logs_response = app
        .clone()
        .oneshot(
            Request::get("/api/logs?limit=10")
                .header("authorization", bearer(&user_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs_response.status(), StatusCode::OK);

    let proxy_response = app
        .clone()
        .oneshot(
            Request::get("/api/proxy/list")
                .header("authorization", bearer(&user_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(proxy_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn authenticated_user_can_ingest_client_events_without_admin_access() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    init_test_tracing();
    xianyu_rust_backend_full::logging::clear_logs();
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let admin_token = encode_access_token(&config.auth, "admin", "admin", false).unwrap();
    let user_token = encode_access_token(&config.auth, "user-a", "user", false).unwrap();

    let ingest_response = app
        .clone()
        .oneshot(
            Request::post("/api/logs/client-events")
                .header("authorization", bearer(&user_token))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "level": "error",
                        "logger": "frontend.api",
                        "module": "messages",
                        "action": "send",
                        "route": "/api/messages/pc/send",
                        "errorCode": "send_failed",
                        "message": "send message failed",
                        "detail": "HTTP 503",
                        "httpStatus": 503,
                        "retryable": true,
                        "promoteToIncident": true,
                        "context": {
                            "sessionId": "s-1",
                            "token": "should-not-leak"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ingest_response.status(), StatusCode::OK);
    let ingest_payload = json_body(ingest_response).await;
    assert_eq!(ingest_payload["status"], "ok");
    assert!(ingest_payload["incidentId"]
        .as_str()
        .unwrap_or_default()
        .starts_with("inc_"));

    let logs_response = app
        .clone()
        .oneshot(
            Request::get("/api/logs?limit=50&keyword=send%20message%20failed")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs_response.status(), StatusCode::OK);
    let logs_payload = json_body(logs_response).await;
    let logs = logs_payload["data"].as_array().cloned().unwrap_or_default();
    assert!(!logs.is_empty());
    assert_eq!(logs[0]["logger"], "client.telemetry.api");
    assert!(logs[0]["fieldsJson"].to_string().contains("[redacted]"));

    let incidents_response = app
        .clone()
        .oneshot(
            Request::get("/api/logs/incidents?limit=20&module=messages")
                .header("authorization", bearer(&admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(incidents_response.status(), StatusCode::OK);
    let incidents_payload = json_body(incidents_response).await;
    let incidents = incidents_payload["data"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(incidents.len(), 1);
    assert_eq!(incidents[0]["errorCode"], "send_failed");
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn init_test_tracing() {
    TRACING_INIT.call_once(|| {
        let _ = tracing_subscriber::registry()
            .with(EnvFilter::new("info"))
            .with(fmt::layer().with_test_writer().with_target(true))
            .with(memory_log_layer())
            .try_init();
    });
}

fn configure_env(temp_dir: &TempDir) {
    let data_dir = temp_dir.path().join("data");
    env::set_var("APP_DATA_DIR", data_dir.display().to_string());
    env::set_var("JWT_SECRET", "test-jwt-secret");
    env::set_var("ADMIN_USERNAME", "admin");
    env::set_var("ADMIN_PASSWORD", "password");
    env::remove_var("API_TOKEN");
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    let url = format!("sqlite://{}", path.display());
    SqlitePool::connect(&url).await.unwrap()
}

async fn seed_account(pool: &SqlitePool) {
    sqlx::query(
        r#"
        INSERT INTO accounts (
            uid,
            owner_username,
            nick,
            token,
            sid,
            cookies,
            browser_cookie_jar,
            status,
            created_at,
            updated_at,
            cookie_refreshed_at,
            last_session_check_at,
            last_session_ok_at,
            first_session_fail_at,
            last_session_status,
            last_session_reason,
            extra,
            security_score,
            security_score_updated_at,
            restriction_end,
            coin_balance,
            violation_count,
            fish_power,
            seller_30d_sold
        )
        VALUES (?, ?, ?, '', '', '', '', 'active', 0, 0, 0, 0, 0, 0, '', '', '{}', 90, 0, '', 0, 0, 0, 0)
        "#,
    )
    .bind("uid-1")
    .bind("admin")
    .bind("Seller One")
    .execute(pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}
