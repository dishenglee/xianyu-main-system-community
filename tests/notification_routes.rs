use std::{
    collections::VecDeque,
    env,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::Query,
    http::{Request as HttpRequest, StatusCode},
    response::Json,
    routing::get,
    Router,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;
use tower::util::ServiceExt;
use xianyu_rust_backend_full::notifications::feishu_native::{
    FeishuNativeCallFuture, FeishuNativeError, FeishuNativeRequest, FeishuNativeResponse,
    FeishuNativeRuntime, FeishuNativeTransport,
};
use xianyu_rust_backend_full::{
    app::build_app, auth::encode_access_token, config::AppConfig, state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const REDACTED_SECRET: &str = "********";
const UNSUPPORTED_NOTIFICATION_TYPES: &[&str] = &[
    "webhook",
    "Webhook",
    "generic_webhook",
    "email",
    "Email",
    "bark",
    "Bark",
    "telegram",
    "Telegram",
    "tg",
    "TG",
    "weixin_personal",
];

#[tokio::test]
async fn notification_channel_crud_persists_retained_types_and_rejects_unknown_types() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let pool = connect_notifications_pool(&config).await;

    let create_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Ops Bot",
                        "type": "feishu",
                        "config": {
                            "app_id": "cli_test"
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload = json_body(create_response).await;
    assert_eq!(create_payload["status"], "ok");
    assert_eq!(create_payload["channel"]["type"], "feishu");
    let channel_id = create_payload["channel"]["id"].as_i64().unwrap();

    let unsupported_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Removed Telegram Bot",
                        "type": "telegram",
                        "config": {},
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unsupported_response.status(), StatusCode::BAD_REQUEST);

    let list_response = app
        .clone()
        .oneshot(
            HttpRequest::get("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload.as_array().map(Vec::len), Some(1));

    let update_response = app
        .clone()
        .oneshot(
            HttpRequest::put(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "type": "ding_talk",
                        "enabled": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    let update_payload = json_body(update_response).await;
    assert_eq!(update_payload["channel"]["type"], "dingtalk");
    assert_eq!(update_payload["channel"]["enabled"], false);

    let stored: (String, i64) =
        sqlx::query_as("SELECT type, enabled FROM notification_channels WHERE id = ?")
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.0, "dingtalk");
    assert_eq!(stored.1, 0);

    clear_env();
}

#[tokio::test]
async fn message_notification_settings_route_defaults_enabled_and_persists_global_toggle() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let pool = connect_accounts_pool(&config).await;

    let get_response = app
        .clone()
        .oneshot(
            HttpRequest::get("/api/message-notifications/settings")
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload = json_body(get_response).await;
    assert_eq!(get_payload["platformMessagesEnabled"], json!(true));

    let put_response = app
        .clone()
        .oneshot(
            HttpRequest::put("/api/message-notifications/settings")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "platformMessagesEnabled": false
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);
    let put_payload = json_body(put_response).await;
    assert_eq!(
        put_payload["settings"]["platformMessagesEnabled"],
        json!(false)
    );

    let stored: i64 = sqlx::query_scalar(
        "SELECT platform_messages_enabled FROM ai_reply_settings WHERE cookie_id = '__global__'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, 0);

    clear_env();
}

#[tokio::test]
async fn notification_channel_crud_migrates_legacy_robot_url_config_key_on_write() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let pool = connect_notifications_pool(&config).await;

    let create_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Legacy DingTalk Robot",
                        "type": "dingtalk",
                        "config": {
                            "webhook_url": "https://oapi.dingtalk.example/robot/send",
                            "secret": "signing-secret"
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload = json_body(create_response).await;
    assert!(create_payload["channel"]["config"]["webhook_url"].is_null());
    assert_eq!(
        create_payload["channel"]["config"]["robot_url"],
        REDACTED_SECRET
    );
    let channel_id = create_payload["channel"]["id"].as_i64().unwrap();

    let stored: (Option<String>, Option<String>) = sqlx::query_as(
        "SELECT json_extract(config, '$.robot_url'), json_extract(config, '$.webhook_url') FROM notification_channels WHERE id = ?",
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored.0.as_deref(),
        Some("https://oapi.dingtalk.example/robot/send")
    );
    assert_eq!(stored.1, None);

    let update_response = app
        .clone()
        .oneshot(
            HttpRequest::put(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "config": {
                            "webhook_url": "https://oapi.dingtalk.example/robot/send-v2",
                            "secret": REDACTED_SECRET
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    let update_payload = json_body(update_response).await;
    assert!(update_payload["channel"]["config"]["webhook_url"].is_null());
    assert_eq!(
        update_payload["channel"]["config"]["robot_url"],
        REDACTED_SECRET
    );

    let stored: (Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT json_extract(config, '$.robot_url'), json_extract(config, '$.webhook_url'), json_extract(config, '$.secret') FROM notification_channels WHERE id = ?",
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored.0.as_deref(),
        Some("https://oapi.dingtalk.example/robot/send-v2")
    );
    assert_eq!(stored.1, None);
    assert_eq!(stored.2.as_deref(), Some("signing-secret"));

    clear_env();
}

#[tokio::test]
async fn notification_channel_responses_redact_sensitive_config_and_preserve_existing_secret_on_update(
) {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let pool = connect_notifications_pool(&config).await;
    seed_account(&config, "account-secret", "").await;

    let create_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Secret Feishu",
                        "type": "feishu",
                        "config": {
                            "app_id": "cli-secret",
                            "app_secret": "sec-original",
                            "verification_token": "verify-original",
                            "domain": "feishu"
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload = json_body(create_response).await;
    assert_eq!(
        create_payload["channel"]["config"]["app_secret"],
        REDACTED_SECRET
    );
    assert_eq!(
        create_payload["channel"]["config"]["verification_token"],
        REDACTED_SECRET
    );
    let channel_id = create_payload["channel"]["id"].as_i64().unwrap();

    let list_response = app
        .clone()
        .oneshot(
            HttpRequest::get("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload[0]["config"]["app_secret"], REDACTED_SECRET);

    let get_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_payload = json_body(get_response).await;
    assert_eq!(get_payload["config"]["app_secret"], REDACTED_SECRET);

    let update_response = app
        .clone()
        .oneshot(
            HttpRequest::put(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Secret Feishu Updated",
                        "type": "feishu",
                        "config": {
                            "app_id": "cli-secret",
                            "app_secret": REDACTED_SECRET,
                            "verification_token": REDACTED_SECRET,
                            "domain": "lark"
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);
    let update_payload = json_body(update_response).await;
    assert_eq!(
        update_payload["channel"]["config"]["app_secret"],
        REDACTED_SECRET
    );
    assert_eq!(update_payload["channel"]["config"]["domain"], "lark");

    let stored: (String, String, String) = sqlx::query_as(
        "SELECT json_extract(config, '$.app_secret'), json_extract(config, '$.verification_token'), json_extract(config, '$.domain') FROM notification_channels WHERE id = ?",
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored.0, "sec-original");
    assert_eq!(stored.1, "verify-original");
    assert_eq!(stored.2, "lark");

    let bind_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/message-notifications/account-secret")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "channel_id": channel_id,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bind_response.status(), StatusCode::OK);
    let bind_payload = json_body(bind_response).await;
    assert_eq!(
        bind_payload["binding"]["channel_config"]["app_secret"],
        REDACTED_SECRET
    );

    clear_env();
}

#[tokio::test]
async fn notification_channel_crud_rejects_unsupported_types_on_create_and_update() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let pool = connect_notifications_pool(&config).await;

    for unsupported_type in UNSUPPORTED_NOTIFICATION_TYPES {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::post("/api/notification-channels")
                    .header("authorization", "Bearer route-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "name": format!("Unsupported {unsupported_type}"),
                            "type": unsupported_type,
                            "config": {},
                            "enabled": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{unsupported_type} should not be creatable"
        );
    }

    let channel_id = create_channel(&app, "Retained Feishu", "feishu").await;
    for unsupported_type in UNSUPPORTED_NOTIFICATION_TYPES {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::put(format!("/api/notification-channels/{channel_id}"))
                    .header("authorization", "Bearer route-token")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "type": unsupported_type,
                            "enabled": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{unsupported_type} should not be assignable by update"
        );
    }

    let stored_type: String =
        sqlx::query_scalar("SELECT type FROM notification_channels WHERE id = ?")
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored_type, "feishu");

    let unsupported_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_channels WHERE type NOT IN ('wechat', 'dingtalk', 'feishu', 'lark', 'openclaw_weixin')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(unsupported_count, 0);

    clear_env();
}

#[tokio::test]
async fn notification_binding_round_trip_persists_runtime_metadata() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let pool = connect_notifications_pool(&config).await;
    seed_account(&config, "account-001", "").await;
    let channel_id = create_channel(&app, "Feishu Alerts", "feishu").await;

    let runtime_response = app
        .clone()
        .oneshot(
            HttpRequest::post(format!("/api/notification-channels/{channel_id}/runtime"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "runtime": {
                            "listener_status": "listening",
                            "receive_id": "chat-001"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    assert_eq!(runtime_payload["status"], "ok");
    assert_eq!(
        runtime_payload["runtime"]["runtime_json"]["listener_status"],
        "listening"
    );

    let binding_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/message-notifications/account-001")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "channel_id": channel_id,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(binding_response.status(), StatusCode::OK);
    let binding_payload = json_body(binding_response).await;
    assert_eq!(binding_payload["status"], "ok");
    assert_eq!(binding_payload["binding"]["channel_id"], channel_id);

    let account_bindings = app
        .clone()
        .oneshot(
            HttpRequest::get("/api/message-notifications/account-001")
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(account_bindings.status(), StatusCode::OK);
    let account_payload = json_body(account_bindings).await;
    assert_eq!(account_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(account_payload[0]["channel_type"], "feishu");

    let grouped_bindings = app
        .clone()
        .oneshot(
            HttpRequest::get("/api/message-notifications")
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(grouped_bindings.status(), StatusCode::OK);
    let grouped_payload = json_body(grouped_bindings).await;
    assert_eq!(
        grouped_payload["account-001"].as_array().map(Vec::len),
        Some(1)
    );

    let stored_runtime: (String, String) = sqlx::query_as(
        "SELECT status, runtime_json FROM notification_channel_runtime WHERE channel_id = ?",
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored_runtime.0, "idle");
    assert!(stored_runtime.1.contains("\"receive_id\":\"chat-001\""));

    let stored_binding: (String, i64) = sqlx::query_as(
        "SELECT uid, enabled FROM account_notification_bindings WHERE channel_id = ?",
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored_binding.0, "account-001");
    assert_eq!(stored_binding.1, 1);

    clear_env();
}

#[tokio::test]
async fn community_notification_channels_share_local_identity() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    seed_account(&config, "seller-a", "user-a").await;
    seed_account(&config, "seller-b", "user-b").await;
    let user_a = encode_access_token(&config.auth, "user-a", "user", false).unwrap();
    let user_b = encode_access_token(&config.auth, "user-b", "user", false).unwrap();

    let user_a_channel = create_channel_with_token(&app, &user_a, "User A Feishu", "feishu").await;
    let user_b_channel = create_channel_with_token(&app, &user_b, "User B Feishu", "feishu").await;

    let list_a = app
        .clone()
        .oneshot(
            HttpRequest::get("/api/notification-channels")
                .header("authorization", format!("Bearer {user_a}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_a.status(), StatusCode::OK);
    let list_a_payload = json_body(list_a).await;
    assert_eq!(list_a_payload.as_array().map(Vec::len), Some(2));
    assert!(list_a_payload
        .as_array()
        .unwrap()
        .iter()
        .any(|channel| channel["id"] == user_a_channel));
    assert!(list_a_payload
        .as_array()
        .unwrap()
        .iter()
        .all(|channel| channel["owner_username"] == "local-app"));

    let cross_bind = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/message-notifications/seller-b")
                .header("authorization", format!("Bearer {user_a}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "channel_id": user_a_channel,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_bind.status(), StatusCode::OK);

    let cross_channel = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/message-notifications/seller-a")
                .header("authorization", format!("Bearer {user_a}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "channel_id": user_b_channel,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cross_channel.status(), StatusCode::OK);

    let bind_a = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/message-notifications/seller-a")
                .header("authorization", format!("Bearer {user_a}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "channel_id": user_a_channel,
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bind_a.status(), StatusCode::OK);

    clear_env();
}

#[tokio::test]
async fn notification_channel_update_restarts_disabled_managed_channel_with_runtime_skip() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state.clone());
    let channel_id = create_channel(&app, "WeChat Route", "openclaw_weixin").await;

    let update_response = app
        .clone()
        .oneshot(
            HttpRequest::put(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "enabled": false,
                        "config": {
                            "base_url": "https://example.invalid",
                            "bot_token": "bot-token",
                            "ilink_user_id": "wx-user"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("skipped")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_reason")
            .and_then(Value::as_str),
        Some("channel disabled")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_source")
            .and_then(Value::as_str),
        Some("notify.manager")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_channel_type")
            .and_then(Value::as_str),
        Some("openclaw_weixin")
    );

    clear_env();
}

#[tokio::test]
async fn notification_channel_update_delegates_restart_to_unified_manager_for_unsupported_type() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state.clone());
    let channel_id = create_channel(&app, "WeChat Route", "wechat").await;

    let update_response = app
        .clone()
        .oneshot(
            HttpRequest::put(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "enabled": true,
                        "config": {
                            "delivery": "v2"
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_response.status(), StatusCode::OK);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("skipped")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_reason")
            .and_then(Value::as_str),
        Some("no worker implemented for channel type")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_source")
            .and_then(Value::as_str),
        Some("notify.manager")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_channel_type")
            .and_then(Value::as_str),
        Some("wechat")
    );

    clear_env();
}

#[tokio::test]
async fn deleting_feishu_channel_stops_native_channel_before_removing_row() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = QueuedFeishuTransport::new(vec![]);
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone())
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let app = build_app(state);
    let pool = connect_notifications_pool(&config).await;

    let create_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Delete Feishu",
                        "type": "feishu",
                        "config": {
                            "app_id": "cli-delete",
                            "app_secret": "sec-delete",
                            "domain": "feishu"
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload = json_body(create_response).await;
    let channel_id = create_payload["channel"]["id"].as_i64().unwrap();
    sqlx::query(
        r#"
        INSERT INTO feishu_user_bindings (
            channel_id,
            owner_username,
            sender_id,
            sender_name,
            chat_id,
            chat_type,
            source,
            last_message_id,
            last_event_id,
            last_seen_at,
            enabled,
            created_at,
            updated_at
        )
        VALUES (?, 'route-token', 'ou_delete', '删除测试', 'oc_delete', 'p2p', 'im_command', 'om_delete', 'evt_delete', 1, 1, 1, 1)
        "#,
    )
    .bind(channel_id)
    .execute(&pool)
    .await
    .unwrap();

    let delete_response = app
        .clone()
        .oneshot(
            HttpRequest::delete(format!("/api/notification-channels/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let methods = wait_for_feishu_methods(&transport, |methods| {
        methods.iter().any(|method| method == "channel.stop")
    })
    .await;
    assert_eq!(methods, vec!["channel.stop"]);
    assert_eq!(
        transport.call_params("channel.stop")[0]["channel_id"],
        channel_id
    );
    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM notification_channels WHERE id = ?")
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    let remaining_bindings: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM feishu_user_bindings WHERE channel_id = ?")
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining_bindings, 0);

    clear_env();
}

#[tokio::test]
async fn qr_poll_auto_provision_restarts_openclaw_worker_via_unified_manager_surface() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);
    let login_host = start_mock_qr_login_server().await;
    set_env("OPENCLAW_WEIXIN_LOGIN_HOST", &login_host);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state.clone());

    let poll_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/wechat/qr-poll")
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "qrcode": "qr-001",
                        "host": login_host,
                        "verify_code": "9527"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(poll_response.status(), StatusCode::OK);
    let poll_payload = json_body(poll_response).await;
    assert_eq!(poll_payload["status"], "confirmed");
    let channel_id = poll_payload["channel"]["id"].as_i64().unwrap();
    assert_eq!(poll_payload["channel"]["type"], "openclaw_weixin");
    assert_eq!(
        poll_payload["channel"]["config"]["bind_all_accounts"],
        Value::Bool(true)
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("spawned")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_source")
            .and_then(Value::as_str),
        Some("wechat.manager")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_channel_type")
            .and_then(Value::as_str),
        Some("openclaw_weixin")
    );

    clear_env();
}

#[tokio::test]
async fn feishu_qr_registration_pending_does_not_create_channel_and_confirmed_upserts_owner_channel(
) {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let owner_token = encode_access_token(&config.auth, "feishu-owner", "user", false).unwrap();
    let other_owner_token =
        encode_access_token(&config.auth, "feishu-other-owner", "user", false).unwrap();
    let transport = QueuedFeishuTransport::new(vec![
        json!({
            "device_code": "device-001",
            "qr_url": "https://accounts.feishu.cn/qr/device-001",
            "expires_at_ms": 1_797_000_000_000_i64,
            "poll_interval_ms": 2_000,
            "domain": "feishu"
        }),
        json!({
            "status": "pending",
            "device_code": "device-001",
            "domain": "feishu"
        }),
        json!({
            "device_code": "device-001",
            "domain": "lark",
            "credentials": {
                "client_id": "cli-feishu",
                "client_secret": "sec-initial",
                "domain": "accounts.larksuite.com",
                "operator_open_id": "ou_qr_operator"
            }
        }),
        json!({
            "scopes": [
                {"scope_name": "im:message.group_msg", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.group_at_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.p2p_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message:send_as_bot", "grant_status": 1, "scope_type": "tenant"}
            ]
        }),
        json!({
            "status": "sent",
            "message_id": "om_bootstrap_welcome"
        }),
        json!({
            "status": "confirmed",
            "device_code": "device-001",
            "domain": "lark",
            "credentials": {
                "app_id": "cli-feishu",
                "app_secret": "sec-updated",
                "domain": "accounts.larksuite.com",
                "operator_open_id": "ou_qr_operator"
            }
        }),
        json!({
            "scopes": [
                {"scope_name": "im:message.group_msg", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.group_at_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.p2p_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message:send_as_bot", "grant_status": 1, "scope_type": "tenant"}
            ]
        }),
    ]);
    let state = AppState::bootstrap(config.clone())
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let app = build_app(state);
    let pool = connect_notifications_pool(&config).await;

    let start_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-start")
                .header("authorization", format!("Bearer {owner_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::OK);
    let start_payload = json_body(start_response).await;
    assert_eq!(start_payload["device_code"], "device-001");
    assert_eq!(start_payload["domain"], "feishu");
    assert_eq!(start_payload["publish_required"], true);
    assert_eq!(
        start_payload["required_scopes"],
        json!([
            "im:message.group_msg",
            "im:message.group_at_msg:readonly",
            "im:message.p2p_msg:readonly",
            "im:message:send_as_bot"
        ])
    );

    let pending_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-poll")
                .header("authorization", format!("Bearer {other_owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "device_code": "device-001",
                        "domain": "feishu"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(pending_response.status(), StatusCode::OK);
    let pending_payload = json_body(pending_response).await;
    assert_eq!(pending_payload["status"], "pending");
    assert_eq!(pending_payload.get("channel"), None);
    assert_eq!(pending_payload["publish_required"], true);

    let pending_channel_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_channels WHERE type = 'feishu' AND owner_username = 'local-app'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending_channel_count, 0);

    let confirmed_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-poll")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "device_code": "device-001",
                        "domain": "accounts.larksuite.com"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(confirmed_response.status(), StatusCode::OK);
    let confirmed_payload = json_body(confirmed_response).await;
    assert_eq!(confirmed_payload["status"], "confirmed");
    assert_eq!(confirmed_payload["channel"]["type"], "feishu");
    assert_eq!(confirmed_payload["channel"]["enabled"], true);
    assert_eq!(confirmed_payload["channel"]["owner_username"], "local-app");
    assert_eq!(
        confirmed_payload["channel"]["config"]["app_id"],
        "cli-feishu"
    );
    assert_eq!(
        confirmed_payload["channel"]["config"]["app_secret"],
        REDACTED_SECRET
    );
    assert_eq!(confirmed_payload["channel"]["config"]["domain"], "lark");
    assert_eq!(
        confirmed_payload["channel"]["config"]["allowed_operator_ids"][0],
        "ou_qr_operator"
    );
    assert_eq!(
        confirmed_payload["channel"]["config"]["capabilities"],
        json!({
            "im_reply": true,
            "item_actions": true,
            "order_actions": true
        })
    );
    assert_eq!(
        confirmed_payload["channel"]["config"]["required_scopes"],
        json!([
            "im:message.group_msg",
            "im:message.group_at_msg:readonly",
            "im:message.p2p_msg:readonly",
            "im:message:send_as_bot"
        ])
    );
    assert_eq!(
        confirmed_payload["channel"]["config"]["scope_publish_required"],
        true
    );
    assert_eq!(
        confirmed_payload["channel"]["config"]["group_message_without_mention_expected"],
        true
    );
    assert_eq!(transport.call_params("register.poll")[1]["domain"], "lark");
    let channel_id = confirmed_payload["channel"]["id"].as_i64().unwrap();
    let binding = sqlx::query(
        r#"
        SELECT owner_username, sender_id, role, joined_via, enabled
        FROM feishu_user_bindings
        WHERE channel_id = ? AND sender_id = 'ou_qr_operator'
        LIMIT 1
        "#,
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(binding.get::<String, _>("owner_username"), "local-app");
    assert_eq!(binding.get::<String, _>("sender_id"), "ou_qr_operator");
    assert_eq!(binding.get::<String, _>("role"), "primary");
    assert_eq!(binding.get::<String, _>("joined_via"), "qr_owner");
    assert_eq!(binding.get::<i64, _>("enabled"), 1);

    let updated_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-poll")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "device_code": "device-001",
                        "domain": "lark"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(updated_response.status(), StatusCode::OK);
    let updated_payload = json_body(updated_response).await;
    assert_eq!(updated_payload["channel"]["id"], channel_id);
    assert_eq!(
        updated_payload["channel"]["config"]["app_secret"],
        REDACTED_SECRET
    );
    let stored_updated_secret: String = sqlx::query_scalar(
        "SELECT json_extract(config, '$.app_secret') FROM notification_channels WHERE id = ?",
    )
    .bind(channel_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored_updated_secret, "sec-updated");

    let confirmed_channel_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_channels WHERE type = 'feishu' AND owner_username = 'local-app'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(confirmed_channel_count, 1);
    let methods = wait_for_feishu_methods(&transport, |methods| {
        methods
            .iter()
            .filter(|method| method.as_str() == "channel.start")
            .count()
            >= 2
    })
    .await;
    assert_eq!(
        methods
            .iter()
            .filter(|method| method.starts_with("register."))
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "register.start",
            "register.poll",
            "register.poll",
            "register.poll"
        ]
    );
    assert_eq!(
        methods
            .iter()
            .filter(|method| method.starts_with("channel."))
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            "channel.stop",
            "channel.start",
            "channel.stop",
            "channel.start"
        ]
    );
    assert_eq!(
        methods
            .iter()
            .filter(|method| method.as_str() == "message.send")
            .cloned()
            .collect::<Vec<_>>(),
        vec!["message.send"]
    );
    assert_eq!(
        transport.call_params("message.send")[0]["receive_id"],
        "ou_qr_operator"
    );
    assert_eq!(
        transport.call_params("message.send")[0]["receive_id_type"],
        "open_id"
    );
    assert_eq!(
        transport.call_params("message.send")[0]["msg_type"],
        "interactive"
    );
    let card_text = transport.call_params("message.send")[0]["content"].to_string();
    assert!(card_text.contains("鱼管家飞书助手"));
    assert!(card_text.contains("绑定成功"));
    assert!(card_text.contains("每个新会话都会自动创建飞书群聊"));
    assert!(card_text.contains("直接在群里发送回复内容即可"));
    assert!(!card_text.contains("菜单"));
    assert!(!card_text.contains("最近消息"));
    assert!(!card_text.contains("商品列表"));
    assert!(!card_text.contains("订单列表"));
    assert!(!card_text.contains("协作者"));

    clear_env();
}

#[tokio::test]
async fn feishu_qr_registration_returns_scope_setup_qr_when_group_msg_missing() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let owner_token = encode_access_token(&config.auth, "feishu-owner", "user", false).unwrap();
    let transport = QueuedFeishuTransport::new(vec![
        json!({
            "device_code": "device-scope",
            "qr_url": "https://accounts.feishu.cn/qr/device-scope",
            "expires_at_ms": 1_797_000_000_000_i64,
            "poll_interval_ms": 2_000,
            "domain": "feishu"
        }),
        json!({
            "status": "confirmed",
            "device_code": "device-scope",
            "domain": "feishu",
            "credentials": {
                "app_id": "cli-feishu-missing",
                "app_secret": "sec-missing",
                "domain": "feishu",
                "operator_open_id": "ou_qr_operator"
            }
        }),
        json!({
            "scopes": [
                {"scope_name": "im:message.group_at_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.p2p_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message:send_as_bot", "grant_status": 1, "scope_type": "tenant"}
            ]
        }),
    ]);
    let state = AppState::bootstrap(config.clone())
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let app = build_app(state);

    let _ = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-start")
                .header("authorization", format!("Bearer {owner_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let poll_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-poll")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "device_code": "device-scope",
                        "domain": "feishu"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(poll_response.status(), StatusCode::OK);
    let payload = json_body(poll_response).await;
    assert_eq!(payload["status"], "scope_setup_required");
    assert_eq!(payload["publish_required"], false);
    assert_eq!(payload["missing_scopes"], json!(["im:message.group_msg"]));
    assert_eq!(payload.get("channel"), None);
    assert!(payload["qr_url"]
        .as_str()
        .unwrap_or_default()
        .contains("/page/launcher?clientID=cli-feishu-missing&addons="));
    assert!(payload["qr_image"]
        .as_str()
        .unwrap_or_default()
        .starts_with("data:image/svg+xml;base64,"));

    clear_env();
}

#[tokio::test]
async fn feishu_qr_registration_creates_second_channel_for_different_bot() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let owner_token = encode_access_token(&config.auth, "feishu-owner", "user", false).unwrap();
    let transport = QueuedFeishuTransport::new(vec![
        json!({
            "device_code": "device-002",
            "qr_url": "https://accounts.feishu.cn/qr/device-002",
            "expires_at_ms": 1_797_000_000_000_i64,
            "poll_interval_ms": 2_000,
            "domain": "feishu"
        }),
        json!({
            "device_code": "device-002",
            "domain": "feishu",
            "credentials": {
                "app_id": "cli-new",
                "app_secret": "sec-new",
                "domain": "feishu",
                "operator_open_id": "ou_new_operator"
            }
        }),
        json!({
            "scopes": [
                {"scope_name": "im:message.group_msg", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.group_at_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message.p2p_msg:readonly", "grant_status": 1, "scope_type": "tenant"},
                {"scope_name": "im:message:send_as_bot", "grant_status": 1, "scope_type": "tenant"}
            ]
        }),
        json!({
            "status": "sent",
            "message_id": "om_new_bootstrap"
        }),
    ]);
    let state = AppState::bootstrap(config.clone())
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(transport)));
    let app = build_app(state);
    let pool = connect_notifications_pool(&config).await;

    let existing_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Existing Feishu",
                        "type": "feishu",
                        "config": {
                            "app_id": "cli-existing",
                            "app_secret": "sec-existing",
                            "domain": "feishu"
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(existing_response.status(), StatusCode::OK);
    let existing_payload = json_body(existing_response).await;
    let existing_id = existing_payload["channel"]["id"].as_i64().unwrap();

    let start_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-start")
                .header("authorization", format!("Bearer {owner_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(start_response.status(), StatusCode::OK);

    let poll_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-poll")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "device_code": "device-002",
                        "domain": "feishu"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(poll_response.status(), StatusCode::OK);
    let poll_payload = json_body(poll_response).await;
    assert_eq!(poll_payload["status"], "confirmed");
    let new_id = poll_payload["channel"]["id"].as_i64().unwrap();
    assert_ne!(new_id, existing_id);
    assert_eq!(poll_payload["channel"]["config"]["app_id"], "cli-new");

    let existing_secret: String = sqlx::query_scalar(
        "SELECT json_extract(config, '$.app_secret') FROM notification_channels WHERE id = ?",
    )
    .bind(existing_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(existing_secret, "sec-existing");

    let channel_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notification_channels WHERE type = 'feishu' AND owner_username = 'local-app'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(channel_count, 2);

    clear_env();
}

#[tokio::test]
async fn feishu_qr_registration_native_error_is_bounded_and_redacted() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let owner_token = encode_access_token(&config.auth, "feishu-owner", "user", false).unwrap();
    let transport = RpcErrorFeishuTransport::new(
        "Feishu registration HTTP 500 (invalid_request_sec-super-sensitive)".repeat(12),
    );
    let state = AppState::bootstrap(config.clone())
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(transport)));
    state
        .feishu_qr_registrations()
        .remember("device-err", "local-app", "feishu")
        .await;
    let app = build_app(state);

    let response = app
        .oneshot(
            HttpRequest::post("/api/notification-channels/feishu/qr-poll")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "device_code": "device-err",
                        "domain": "feishu"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let payload = json_body(response).await;
    let detail = payload["detail"].as_str().unwrap();
    assert!(detail.contains("Feishu native transport API error"));
    assert!(!detail.contains("sec-super-sensitive"));
    assert!(detail.len() < 160, "{detail}");

    clear_env();
}

#[tokio::test]
async fn feishu_test_route_uses_scanned_operator_when_receive_id_is_missing() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = QueuedFeishuTransport::new(vec![json!({
        "status": "sent",
        "message_id": "om_test_send"
    })]);
    let config = AppConfig::from_env().unwrap();
    let owner_token = encode_access_token(&config.auth, "route-token", "user", false).unwrap();
    let state = AppState::bootstrap(config.clone())
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let app = build_app(state.clone());

    let create_response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": "Feishu Test Target",
                        "type": "feishu",
                        "config": {
                            "app_id": "cli-test-target",
                            "app_secret": "sec-test-target",
                            "allowed_operator_ids": ["ou_test_operator"]
                        },
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let create_payload = json_body(create_response).await;
    let channel_id = create_payload["channel"]["id"].as_i64().unwrap();

    let test_response = app
        .clone()
        .oneshot(
            HttpRequest::post(format!("/api/notification-channels/{channel_id}/test"))
                .header("authorization", format!("Bearer {owner_token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "message": "operator fallback" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(test_response.status(), StatusCode::OK);

    let calls = transport.call_params("message.send");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["receive_id"], "ou_test_operator");
    assert_eq!(calls[0]["receive_id_type"], "open_id");
    assert_eq!(calls[0]["msg_type"], "text");
    assert!(calls[0]["content"]["text"]
        .as_str()
        .unwrap()
        .contains("operator fallback"));

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "healthy");
    assert_eq!(runtime.last_error, "");
    assert_eq!(
        runtime
            .runtime_json
            .get("last_feishu_notify_message_id")
            .and_then(Value::as_str),
        Some("om_test_send")
    );

    clear_env();
}

#[tokio::test]
async fn feishu_member_management_routes_list_invite_and_remove_collaborator() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let owner_token = encode_access_token(&config.auth, "feishu-owner", "user", false).unwrap();
    let state = AppState::bootstrap(config.clone()).await.unwrap();
    let app = build_app(state.clone());
    let channel_id = create_channel_with_token(&app, &owner_token, "飞书协作", "feishu").await;

    state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            "local-app",
            "ou_primary",
            "主用户",
            "oc_primary",
            "p2p",
            "om_primary",
            "evt_primary",
            "primary",
            "qr_owner",
            None,
            None,
            true,
        )
        .await
        .unwrap();
    state
        .storage()
        .upsert_feishu_user_binding_with_role(
            channel_id,
            "local-app",
            "ou_collab",
            "客服小王",
            "oc_collab",
            "p2p",
            "om_collab",
            "evt_collab",
            "collaborator",
            "invite_code",
            Some("ou_primary"),
            Some("主用户"),
            true,
        )
        .await
        .unwrap();

    let list_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!(
                "/api/notification-channels/{channel_id}/feishu/members"
            ))
            .header("authorization", format!("Bearer {owner_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_payload = json_body(list_response).await;
    assert_eq!(list_payload["status"], "ok");
    assert_eq!(list_payload["members"].as_array().map(Vec::len), Some(2));
    assert!(list_payload["active_invite"].is_null());

    let invite_response = app
        .clone()
        .oneshot(
            HttpRequest::post(format!(
                "/api/notification-channels/{channel_id}/feishu/invites"
            ))
            .header("authorization", format!("Bearer {owner_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(invite_response.status(), StatusCode::OK);
    let invite_payload = json_body(invite_response).await;
    assert_eq!(invite_payload["status"], "ok");
    let invite_code = invite_payload["invite"]["code"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(!invite_code.is_empty());
    assert_eq!(
        invite_payload["invite"]["created_by_sender_id"],
        "ou_primary"
    );

    let list_with_invite = app
        .clone()
        .oneshot(
            HttpRequest::get(format!(
                "/api/notification-channels/{channel_id}/feishu/members"
            ))
            .header("authorization", format!("Bearer {owner_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list_with_invite.status(), StatusCode::OK);
    let list_with_invite_payload = json_body(list_with_invite).await;
    assert_eq!(
        list_with_invite_payload["active_invite"]["code"],
        Value::String(invite_code)
    );

    let delete_response = app
        .clone()
        .oneshot(
            HttpRequest::delete(format!(
                "/api/notification-channels/{channel_id}/feishu/members/ou_collab"
            ))
            .header("authorization", format!("Bearer {owner_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_response.status(), StatusCode::OK);

    let delete_primary_response = app
        .clone()
        .oneshot(
            HttpRequest::delete(format!(
                "/api/notification-channels/{channel_id}/feishu/members/ou_primary"
            ))
            .header("authorization", format!("Bearer {owner_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(delete_primary_response.status(), StatusCode::BAD_REQUEST);

    let final_list_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!(
                "/api/notification-channels/{channel_id}/feishu/members"
            ))
            .header("authorization", format!("Bearer {owner_token}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(final_list_response.status(), StatusCode::OK);
    let final_list_payload = json_body(final_list_response).await;
    assert_eq!(
        final_list_payload["members"].as_array().map(Vec::len),
        Some(1)
    );
    assert_eq!(final_list_payload["members"][0]["sender_id"], "ou_primary");

    clear_env();
}

#[tokio::test]
async fn notification_callback_updates_runtime_state() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let channel_id = create_channel(&app, "Wechat Hook", "wechat").await;

    let verify_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!(
                "/api/notification-hooks/wechat/{channel_id}?echostr=hello-rust"
            ))
            .header("authorization", "Bearer route-token")
            .body(Body::empty())
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(verify_response.status(), StatusCode::OK);
    let verify_body = verify_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    assert_eq!(std::str::from_utf8(&verify_body).unwrap(), "hello-rust");

    let callback_response = app
        .clone()
        .oneshot(
            HttpRequest::post(format!("/api/notification-hooks/wechat/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "event": "message",
                        "text": "hello payload-secret",
                        "token": "callback-token-secret"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback_response.status(), StatusCode::OK);
    let callback_payload = json_body(callback_response).await;
    assert_eq!(callback_payload["status"], "ok");
    assert_eq!(callback_payload["channel_id"], channel_id);

    let runtime_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!("/api/notification-channels/{channel_id}/runtime"))
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    assert_eq!(
        runtime_payload["runtime_json"]["last_callback_source"],
        "wechat"
    );
    assert_eq!(
        runtime_payload["runtime_json"]["last_callback_method"],
        "POST"
    );
    assert_eq!(
        runtime_payload["runtime_json"].get("last_callback_payload"),
        None
    );
    let runtime_text = runtime_payload["runtime_json"].to_string();
    assert!(!runtime_text.contains("payload-secret"));
    assert!(!runtime_text.contains("callback-token-secret"));

    clear_env();
}

#[tokio::test]
async fn notification_callback_accepts_feishu_route_for_lark_channel() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let channel_id = create_channel(&app, "Lark Hook", "lark").await;

    let callback_response = app
        .clone()
        .oneshot(
            HttpRequest::post(format!("/api/notification-hooks/feishu/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "event": "message",
                        "text": "hello from lark"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback_response.status(), StatusCode::OK);
    let callback_payload = json_body(callback_response).await;
    assert_eq!(callback_payload["status"], "ok");
    assert_eq!(callback_payload["channel_id"], channel_id);

    let runtime_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!("/api/notification-channels/{channel_id}/runtime"))
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    assert_eq!(
        runtime_payload["runtime_json"]["last_callback_source"],
        "lark"
    );
    assert_eq!(
        runtime_payload["runtime_json"]["last_callback_route_type"],
        "feishu"
    );

    clear_env();
}

#[tokio::test]
async fn notification_callback_accepts_lark_route_for_feishu_channel() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let app = build_app(AppState::bootstrap(config.clone()).await.unwrap());
    let channel_id = create_channel(&app, "Feishu Hook", "feishu").await;

    let callback_response = app
        .clone()
        .oneshot(
            HttpRequest::post(format!("/api/notification-hooks/lark/{channel_id}"))
                .header("authorization", "Bearer route-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "event": "message",
                        "text": "hello from feishu"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback_response.status(), StatusCode::OK);
    let callback_payload = json_body(callback_response).await;
    assert_eq!(callback_payload["status"], "ok");
    assert_eq!(callback_payload["channel_id"], channel_id);

    let runtime_response = app
        .clone()
        .oneshot(
            HttpRequest::get(format!("/api/notification-channels/{channel_id}/runtime"))
                .header("authorization", "Bearer route-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime_response.status(), StatusCode::OK);
    let runtime_payload = json_body(runtime_response).await;
    assert_eq!(
        runtime_payload["runtime_json"]["last_callback_source"],
        "feishu"
    );
    assert_eq!(
        runtime_payload["runtime_json"]["last_callback_route_type"],
        "lark"
    );

    clear_env();
}

async fn create_channel(app: &axum::Router, name: &str, channel_type: &str) -> i64 {
    create_channel_with_token(app, "route-token", name, channel_type).await
}

async fn create_channel_with_token(
    app: &axum::Router,
    token: &str,
    name: &str,
    channel_type: &str,
) -> i64 {
    let response = app
        .clone()
        .oneshot(
            HttpRequest::post("/api/notification-channels")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "name": name,
                        "type": channel_type,
                        "config": {},
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    payload["channel"]["id"].as_i64().unwrap()
}

async fn seed_account(config: &AppConfig, uid: &str, owner_username: &str) {
    let pool = SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO accounts (uid, owner_username, nick, status, token, cookies, created_at, updated_at)
        VALUES (?, ?, ?, 'active', '', '{}', unixepoch('subsec'), unixepoch('subsec'))
        ON CONFLICT(uid) DO UPDATE SET
            owner_username = excluded.owner_username,
            nick = excluded.nick,
            status = excluded.status,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(uid)
    .bind(owner_username)
    .bind(format!("账号 {uid}"))
    .execute(&pool)
    .await
    .unwrap();
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

async fn connect_notifications_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.notifications_db_path.display()
    ))
    .await
    .unwrap()
}

async fn connect_accounts_pool(config: &AppConfig) -> SqlitePool {
    SqlitePool::connect(&format!(
        "sqlite://{}",
        config.sqlite.accounts_db_path.display()
    ))
    .await
    .unwrap()
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "notification-test-secret");
    set_env("API_TOKEN", "route-token");
    clear_env_key("OPENCLAW_WEIXIN_LOGIN_HOST");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("API_TOKEN");
    clear_env_key("OPENCLAW_WEIXIN_LOGIN_HOST");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}

async fn start_mock_qr_login_server() -> String {
    async fn poll_handler(
        Query(query): Query<std::collections::HashMap<String, String>>,
    ) -> Json<Value> {
        assert_eq!(query.get("qrcode").map(String::as_str), Some("qr-001"));
        assert_eq!(query.get("verify_code").map(String::as_str), Some("9527"));
        Json(json!({
            "status": "confirmed",
            "bot_token": "wx-bot-token",
            "ilink_bot_id": "wx-bot-id",
            "ilink_user_id": "wx-user-001",
            "baseurl": "http://127.0.0.1:9",
            "redirect_host": "https://redirect.example.invalid"
        }))
    }

    let app = Router::new().route("/ilink/bot/get_qrcode_status", get(poll_handler));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

#[derive(Clone)]
struct QueuedFeishuTransport {
    responses: Arc<Mutex<VecDeque<Value>>>,
    calls: Arc<Mutex<Vec<FeishuNativeRequest>>>,
}

impl QueuedFeishuTransport {
    fn new(responses: Vec<Value>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.method.clone())
            .collect()
    }

    fn call_params(&self, method: &str) -> Vec<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.method == method)
            .map(|call| call.params.clone())
            .collect()
    }
}

async fn wait_for_feishu_methods(
    transport: &QueuedFeishuTransport,
    predicate: impl Fn(&[String]) -> bool,
) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let methods = transport.methods();
        if predicate(&methods) || Instant::now() >= deadline {
            return methods;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

impl FeishuNativeTransport for QueuedFeishuTransport {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        self.calls.lock().unwrap().push(request.clone());
        let response = match request.method.as_str() {
            "channel.start" => json!({
                "status": "started",
                "channel_id": request.params["channel_id"]
            }),
            "channel.stop" => json!({
                "status": "stopped",
                "channel_id": request.params["channel_id"]
            }),
            _ => self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| panic!("missing Feishu native response for {}", request.method)),
        };
        Box::pin(async move { Ok(FeishuNativeResponse::success(response)) })
    }
}

struct RpcErrorFeishuTransport {
    message: String,
}

impl RpcErrorFeishuTransport {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl FeishuNativeTransport for RpcErrorFeishuTransport {
    fn call(&self, _request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        let message = self.message.clone();
        Box::pin(async move { Err(FeishuNativeError::Api(message)) })
    }
}
