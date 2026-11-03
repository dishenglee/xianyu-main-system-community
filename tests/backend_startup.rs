use std::{net::SocketAddr, time::Duration};

use axum::{response::IntoResponse, routing::get, Json, Router};
use serde_json::json;
use tokio::net::TcpListener;
use xianyu_rust_backend_full::{config::AppConfig, run_server};

async fn slow_health() -> impl IntoResponse {
    tokio::time::sleep(Duration::from_secs(2)).await;
    "ok"
}

async fn slow_update_policy() -> impl IntoResponse {
    tokio::time::sleep(Duration::from_secs(2)).await;
    Json(json!({
        "minimumVersion": "0.1.0",
        "latestVersion": env!("CARGO_PKG_VERSION"),
        "required": false,
    }))
}

async fn spawn_slow_control_plane() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/health", get(slow_health))
        .route("/api/client/update-policy", get(slow_update_policy));

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    format!("http://{addr}")
}

fn temp_config(data_dir: &std::path::Path, control_plane_base: &str) -> AppConfig {
    let mut config = AppConfig::default();
    config.bind_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    config.sqlite.data_dir = data_dir.to_path_buf();
    config.sqlite.accounts_db_path = data_dir.join("accounts.db");
    config.sqlite.live_state_db_path = data_dir.join("live_state.db");
    config.sqlite.notifications_db_path = data_dir.join("notifications.db");
    config.sqlite.xianyu_db_path = data_dir.join("xianyu.db");
    config.xianyu.sign_service_url = control_plane_base.to_string();
    config.xianyu.license_server_url = control_plane_base.to_string();
    config
}

#[tokio::test]
async fn run_server_publishes_listener_before_startup_dependency_checks_finish() {
    let control_plane_base = spawn_slow_control_plane().await;
    let temp = tempfile::tempdir().unwrap();
    let config = temp_config(temp.path(), &control_plane_base);

    unsafe {
        std::env::set_var(
            "XIANYU_UPDATE_POLICY_URL",
            format!("{control_plane_base}/api/client/update-policy"),
        );
    }

    let server = tokio::time::timeout(Duration::from_secs(1), run_server(config))
        .await
        .expect("listener should be ready before slow startup dependency probes finish")
        .expect("server should start");

    let mut server = server;
    server.shutdown().await.unwrap();
    server.wait_for_shutdown().await;

    unsafe {
        std::env::remove_var("XIANYU_UPDATE_POLICY_URL");
    }
}
