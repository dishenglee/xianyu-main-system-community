use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::{app::build_app, config::AppConfig, state::AppState};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn community_product_login_license_and_cloud_binding_routes_are_absent() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    env::set_var("APP_DATA_DIR", temp_dir.path());
    env::remove_var("APP_BIND_ADDR");
    env::remove_var("JWT_SECRET");
    env::remove_var("APP_BEARER_TOKEN");

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config).await.unwrap());
    for path in [
        "/api/auth/login",
        "/api/licenses/activate",
        "/api/me/xianyu-account-bindings/claim",
        "/api/accounts/unbind-requests",
        "/api/admin/account-unbind-requests",
        "/api/admin/account-unbind-requests/1/approve",
        "/api/admin/account-unbind-requests/1/reject",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::post(path)
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(
            matches!(
                response.status(),
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{path}: {}",
            response.status()
        );
    }

    env::remove_var("APP_DATA_DIR");
}

#[tokio::test]
async fn local_app_without_credentials_can_access_all_local_accounts() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    env::set_var("APP_DATA_DIR", temp_dir.path());
    env::remove_var("APP_BIND_ADDR");
    env::remove_var("JWT_SECRET");
    env::remove_var("APP_BEARER_TOKEN");

    let config = AppConfig::from_env().unwrap();
    let initial_state = AppState::bootstrap(config.clone()).await.unwrap();
    seed_account(&config, "uid-a", "former-user-a").await;
    seed_account(&config, "uid-b", "former-user-b").await;
    seed_item(&config, "uid-b", "item-b").await;
    let notifications_pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.notifications_db_path.display()
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO notification_channels (name, type, owner_username, config, enabled, created_at, updated_at) VALUES ('历史渠道', 'feishu', 'former-user-a', '{}', 1, 1, 1)",
    )
    .execute(&notifications_pool)
    .await
    .unwrap();
    drop(notifications_pool);
    let xianyu_pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.xianyu_db_path.display()
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO comment_targets (owner_username, item_id, title, updated_at) VALUES ('former-user-a', 'old-comment-item', '历史评论目标', 1)",
    )
    .execute(&xianyu_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO automation_schedules (id, owner_username, name, action) VALUES ('old-schedule', 'former-user-a', '旧计划', 'refresh')",
    )
    .execute(&xianyu_pool)
    .await
    .unwrap();
    drop(xianyu_pool);
    drop(initial_state);

    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state);

    let accounts_pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap();
    let non_local_owner_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM accounts WHERE owner_username <> 'local-app'")
            .fetch_one(&accounts_pool)
            .await
            .unwrap();
    assert_eq!(non_local_owner_count, 0);

    let accounts = app
        .clone()
        .oneshot(Request::get("/api/accounts").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(accounts.status(), StatusCode::OK);
    let accounts = json_body(accounts).await;
    let mut uids = accounts
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|account| account["uid"].as_str())
        .collect::<Vec<_>>();
    uids.sort_unstable();
    assert_eq!(uids, vec!["uid-a", "uid-b"]);

    let market_items = app
        .clone()
        .oneshot(
            Request::post("/api/items")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-b",
                        "page": 1,
                        "page_size": 20,
                        "tab": "ON_SALE"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(market_items.status(), StatusCode::OK);
    let market_items = json_body(market_items).await;
    assert_eq!(market_items["data"]["itemList"][0]["itemId"], "item-b");

    let notification_channels = app
        .clone()
        .oneshot(
            Request::get("/api/notification-channels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(notification_channels.status(), StatusCode::OK);
    let notification_channels = json_body(notification_channels).await;
    assert_eq!(notification_channels[0]["name"], "历史渠道");
    assert_eq!(notification_channels[0]["owner_username"], "local-app");

    let xianyu_pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.xianyu_db_path.display()
    ))
    .await
    .unwrap();
    let comment_owner: String = sqlx::query_scalar(
        "SELECT owner_username FROM comment_targets WHERE item_id = 'old-comment-item'",
    )
    .fetch_one(&xianyu_pool)
    .await
    .unwrap();
    let schedule_owner: String = sqlx::query_scalar(
        "SELECT owner_username FROM automation_schedules WHERE id = 'old-schedule'",
    )
    .fetch_one(&xianyu_pool)
    .await
    .unwrap();
    assert_eq!(comment_owner, "");
    assert_eq!(schedule_owner, "");

    let deleted = app
        .oneshot(
            Request::delete("/api/accounts/uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(deleted.status(), StatusCode::OK);

    env::remove_var("APP_DATA_DIR");
}

async fn seed_account(config: &AppConfig, uid: &str, former_owner: &str) {
    let pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at) VALUES (?, ?, ?, 'active', '', '{}', unixepoch('subsec'), unixepoch('subsec'))",
    )
    .bind(uid)
    .bind(former_owner)
    .bind(format!("本地账号 {uid}"))
    .execute(&pool)
    .await
    .unwrap();
}

async fn seed_item(config: &AppConfig, uid: &str, item_id: &str) {
    let pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.xianyu_db_path.display()
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO account_items (uid, item_id, title, price, status, online, detail_json, created_at, updated_at) VALUES (?, ?, ?, '9.90', 'ON_SALE', 1, '{}', 0, 0)",
    )
    .bind(uid)
    .bind(item_id)
    .bind(format!("本地商品 {item_id}"))
    .execute(&pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}
