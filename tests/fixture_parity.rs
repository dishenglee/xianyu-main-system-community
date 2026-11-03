use std::{env, sync::Mutex, time::Duration};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{
    app::build_app, config::AppConfig, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_and_live_contract_match_m002_fixtures() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    set_env("ADMIN_USERNAME", "compat-admin");
    set_env("ADMIN_PASSWORD", "compat-pass");

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let supervisor = state.start_runtime_supervisor();
    tokio::time::sleep(Duration::from_millis(2200)).await;

    let app = build_app(state.clone());
    let runtime_response = app
        .clone()
        .oneshot(
            Request::get("/runtime")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_body = runtime_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let runtime_payload: Value = serde_json::from_slice(&runtime_body).unwrap();

    let runtime_fixture = read_fixture("fixtures/m002/runtime-inspection.json");
    assert_eq!(
        runtime_payload["state_backend"],
        runtime_fixture["state_backend"]
    );
    assert_eq!(
        runtime_payload["auth_configured"],
        runtime_fixture["auth_configured"]
    );
    assert_eq!(runtime_payload["ready"], runtime_fixture["ready"]);

    let database_names: Vec<String> = runtime_payload["databases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(ToString::to_string))
        .collect();
    let manager_names: Vec<String> = runtime_payload["managers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["name"].as_str().map(ToString::to_string))
        .collect();
    let fixture_database_names: Vec<String> = runtime_fixture["database_names"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry.as_str().map(ToString::to_string))
        .collect();
    let fixture_manager_names: Vec<String> = runtime_fixture["manager_names"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry.as_str().map(ToString::to_string))
        .collect();
    assert_eq!(database_names, fixture_database_names);
    for expected_manager in fixture_manager_names {
        assert!(
            manager_names.iter().any(|name| name == &expected_manager),
            "missing expected runtime manager {expected_manager}"
        );
    }

    let (base_url, server) = spawn_server(app).await;
    let (mut socket, _) = connect_async(format!("{base_url}/api/live/ws"))
        .await
        .unwrap();

    let connected = socket.next().await.unwrap().unwrap().into_text().unwrap();
    let connected_payload: Value = serde_json::from_str(&connected).unwrap();
    assert_eq!(
        connected_payload,
        read_fixture("fixtures/m002/live-connected.json")
    );

    socket
        .send(Message::Text("{\"cmd\":\"ping\"}".into()))
        .await
        .unwrap();
    let pong = socket.next().await.unwrap().unwrap().into_text().unwrap();
    let pong_payload: Value = serde_json::from_str(&pong).unwrap();
    assert_eq!(
        pong_payload["type"],
        read_fixture("fixtures/m002/live-pong.json")["type"]
    );

    socket.close(None).await.unwrap();
    server.abort();
    supervisor.shutdown_and_wait().await;
    clear_env();
}

async fn spawn_server(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("ws://{}", addr), server)
}

fn read_fixture(path: &str) -> Value {
    let absolute = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), path);
    serde_json::from_str(&std::fs::read_to_string(absolute).unwrap()).unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "fixture-parity-secret");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
    clear_env_key("XIANYU_LOCAL_ADMIN_LOGIN_ENABLED");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
    clear_env_key("XIANYU_LOCAL_ADMIN_LOGIN_ENABLED");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
