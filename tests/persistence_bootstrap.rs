use std::{env, path::Path, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{query_scalar, SqlitePool};
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn bootstrap_creates_sqlite_files_and_runs_foundational_migrations() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().expect("config loads");
    let state = AppState::bootstrap(config.clone())
        .await
        .expect("state bootstraps");
    let runtime = state.runtime().inspection(config.service_name.clone());

    assert_eq!(runtime.state_backend, "sqlite");
    assert_eq!(runtime.startup_count, 1);
    assert_eq!(runtime.databases.len(), 4);

    for database in &runtime.databases {
        assert!(Path::new(&database.path).exists(), "{database:?}");
        assert!(database.migrations >= 1, "{database:?}");
    }

    let accounts_url = format!("sqlite://{}", config.sqlite.accounts_db_path.display());
    let accounts = SqlitePool::connect(&accounts_url).await.unwrap();
    let has_accounts_table = query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'accounts'",
    )
    .fetch_one(&accounts)
    .await
    .unwrap();
    let has_runtime_table = query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'service_runtime_state'",
    )
    .fetch_one(&accounts)
    .await
    .unwrap();
    assert_eq!(has_accounts_table, 1);
    assert_eq!(has_runtime_table, 1);

    let xianyu_url = format!("sqlite://{}", config.sqlite.xianyu_db_path.display());
    let xianyu = SqlitePool::connect(&xianyu_url).await.unwrap();
    let has_comment_targets = query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'comment_targets'",
    )
    .fetch_one(&xianyu)
    .await
    .unwrap();
    assert_eq!(has_comment_targets, 1);

    clear_env();
}

#[tokio::test]
async fn bootstrap_increments_startup_count_across_restarts() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().expect("config loads");
    let first = AppState::bootstrap(config.clone())
        .await
        .expect("first bootstrap");
    assert_eq!(
        first
            .runtime()
            .inspection(config.service_name.clone())
            .startup_count,
        1
    );
    drop(first);

    let second = AppState::bootstrap(config.clone())
        .await
        .expect("second bootstrap");
    assert_eq!(
        second
            .runtime()
            .inspection(config.service_name.clone())
            .startup_count,
        2
    );

    clear_env();
}

#[tokio::test]
async fn product_login_route_is_removed_and_does_not_audit_logins() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let config = AppConfig::from_env().expect("config loads");
    let app = build_app(
        AppState::bootstrap(config.clone())
            .await
            .expect("state bootstraps"),
    );
    let response = app
        .oneshot(
            Request::post("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "username": "compat-admin",
                        "password": "compat-pass"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let accounts_url = format!("sqlite://{}", config.sqlite.accounts_db_path.display());
    let accounts = SqlitePool::connect(&accounts_url).await.unwrap();
    let login_count =
        query_scalar::<_, i64>("SELECT COUNT(*) FROM auth_login_audit WHERE username = ?")
            .bind("local-app")
            .fetch_one(&accounts)
            .await
            .unwrap();
    assert_eq!(login_count, 0);

    drop(accounts);
    let restarted = AppState::bootstrap(config.clone())
        .await
        .expect("restart bootstrap");
    assert_eq!(
        restarted
            .runtime()
            .inspection(config.service_name.clone())
            .startup_count,
        2
    );

    let accounts = SqlitePool::connect(&accounts_url).await.unwrap();
    let persisted_logins =
        query_scalar::<_, i64>("SELECT COUNT(*) FROM auth_login_audit WHERE username = ?")
            .bind("local-app")
            .fetch_one(&accounts)
            .await
            .unwrap();
    assert_eq!(persisted_logins, 0);

    clear_env();
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "persistence-secret");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
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
