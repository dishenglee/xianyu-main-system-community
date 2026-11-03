use std::{env, str::FromStr, sync::Mutex};

use serde_json::{json, Map, Value};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

use xianyu_rust_backend_full::{
    config::AppConfig,
    models::notifications::{normalize_notification_type, ALLOWED_NOTIFICATION_TYPES},
    notifications::{
        driver::{
            patch_runtime_error, patch_runtime_success, resolve_driver, DriverContentRequest,
            NotificationDriverKind,
        },
        template::{NotificationContentKind, NotificationTemplate},
    },
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
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
const RETAINED_NOTIFICATION_TYPES: &[&str] =
    &["wechat", "dingtalk", "feishu", "lark", "openclaw_weixin"];

#[test]
fn notification_type_registry_keeps_only_retained_product_channels() {
    assert_eq!(ALLOWED_NOTIFICATION_TYPES, RETAINED_NOTIFICATION_TYPES);

    for (raw, normalized) in [
        ("wechat", "wechat"),
        ("ding_talk", "dingtalk"),
        ("dingtalk", "dingtalk"),
        ("feishu", "feishu"),
        ("lark", "lark"),
        ("openclaw-wx", "openclaw_weixin"),
        ("openclaw_weixin", "openclaw_weixin"),
    ] {
        assert_eq!(
            normalize_notification_type(raw).as_deref(),
            Some(normalized)
        );
    }

    for unsupported in UNSUPPORTED_NOTIFICATION_TYPES {
        assert_eq!(
            normalize_notification_type(unsupported),
            None,
            "{unsupported} should not be accepted as a notification type"
        );
    }
}

#[test]
fn driver_registry_resolves_supported_runtime_drivers_and_rejects_unknown_types() {
    let openclaw = resolve_driver("openclaw-wx").expect("openclaw alias should resolve");
    assert_eq!(openclaw.normalized_type, "openclaw_weixin");
    assert_eq!(openclaw.kind, NotificationDriverKind::OpenclawWeixin);
    assert!(!openclaw.capabilities.returns_message_id);
    assert!(openclaw.capabilities.requires_channel_id);
    assert!(openclaw.capabilities.uses_context_token);
    assert_eq!(
        openclaw.capabilities.content.preferred,
        NotificationContentKind::Markdown
    );
    assert!(!openclaw
        .capabilities
        .content
        .supports(NotificationContentKind::Html));

    let feishu = resolve_driver("feishu").expect("feishu native driver should resolve");
    assert_eq!(feishu.normalized_type, "feishu");
    assert_eq!(feishu.kind, NotificationDriverKind::Feishu);
    assert!(feishu.capabilities.returns_message_id);
    assert!(feishu.capabilities.requires_channel_id);
    assert!(!feishu.capabilities.uses_context_token);
    assert_eq!(
        feishu.capabilities.content.preferred,
        NotificationContentKind::Text
    );

    let lark = resolve_driver("lark").expect("lark native driver should resolve");
    assert_eq!(lark.normalized_type, "lark");
    assert_eq!(lark.kind, NotificationDriverKind::Feishu);

    let dingtalk = resolve_driver("dingtalk").expect("dingtalk robot driver should resolve");
    assert_eq!(dingtalk.normalized_type, "dingtalk");
    assert_eq!(dingtalk.kind, NotificationDriverKind::DingtalkRobot);
    assert!(!dingtalk.capabilities.returns_message_id);
    assert!(dingtalk.capabilities.requires_channel_id);
    assert!(!dingtalk.capabilities.uses_context_token);
    assert_eq!(
        dingtalk.capabilities.content.preferred,
        NotificationContentKind::Text
    );

    let wechat = resolve_driver("wechat").expect("enterprise wechat robot driver should resolve");
    assert_eq!(wechat.normalized_type, "wechat");
    assert_eq!(wechat.kind, NotificationDriverKind::EnterpriseWechatRobot);
    assert!(!wechat.capabilities.returns_message_id);
    assert!(wechat.capabilities.requires_channel_id);
    assert!(!wechat.capabilities.uses_context_token);
    assert_eq!(
        wechat.capabilities.content.preferred,
        NotificationContentKind::Text
    );

    for unsupported in UNSUPPORTED_NOTIFICATION_TYPES {
        let err = resolve_driver(unsupported).expect_err("unsupported type should be rejected");
        assert!(
            err.contains("unsupported notification channel type"),
            "{unsupported} returned unexpected error: {err}"
        );
    }

    let err = resolve_driver("unknown-driver").expect_err("unsupported type should be rejected");
    assert!(err.contains("unsupported notification channel type: unknown-driver"));
}

#[test]
fn driver_content_negotiation_picks_strongest_supported_variant_and_preserves_fallback_text() {
    let template = NotificationTemplate::new("plain fallback")
        .with_text("plain body")
        .with_html("<p>html body</p>")
        .with_markdown("**markdown body**")
        .with_media_preview("media://preview-card");

    let openclaw = resolve_driver("openclaw_weixin").expect("openclaw driver");
    let openclaw_html = xianyu_rust_backend_full::notifications::driver::negotiate_driver_content(
        &openclaw,
        &template,
        DriverContentRequest::html(),
    );
    assert_eq!(openclaw_html.requested, NotificationContentKind::Html);
    assert_eq!(
        openclaw_html.selected_kind,
        NotificationContentKind::Markdown
    );
    assert_eq!(openclaw_html.body, "**markdown body**");
    assert_eq!(openclaw_html.fallback_text, "plain fallback");
}

#[test]
fn runtime_helpers_merge_success_and_error_without_dropping_existing_keys() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().expect("temp dir");
    configure_env(&temp_dir);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let state = rt
        .block_on(AppState::bootstrap(AppConfig::from_env().expect("config")))
        .expect("state");

    let channel_id = rt.block_on(async {
        let config = Map::from_iter([
            (
                "base_url".to_string(),
                Value::String("https://wx.test".to_string()),
            ),
            (
                "bot_token".to_string(),
                Value::String("token-001".to_string()),
            ),
            (
                "ilink_user_id".to_string(),
                Value::String("wx-user".to_string()),
            ),
        ]);
        let channel = state
            .storage()
            .create_notification_channel("WeChat Runtime", "openclaw_weixin", &config, true)
            .await
            .expect("create channel");

        let mut initial_patch = Map::new();
        initial_patch.insert(
            "listener_status".to_string(),
            Value::String("listening".to_string()),
        );
        initial_patch.insert("context_tokens".to_string(), json!({"wx-user": "ctx-001"}));
        state
            .storage()
            .upsert_notification_runtime(
                channel.id,
                "idle",
                "",
                Some(10.0),
                Some(9.0),
                &initial_patch,
            )
            .await
            .expect("seed runtime");
        channel.id
    });

    rt.block_on(async {
        let mut success_patch = Map::new();
        success_patch.insert(
            "last_driver".to_string(),
            Value::String("openclaw_weixin".to_string()),
        );
        patch_runtime_success(&state, channel_id, &success_patch).await;

        let after_success = state
            .storage()
            .get_notification_runtime(channel_id)
            .await
            .expect("runtime after success");
        assert_eq!(after_success.status, "healthy");
        assert_eq!(after_success.last_error, "");
        assert!(after_success.last_attempt_at > 0.0);
        assert!(after_success.last_success_at > 0.0);
        assert_eq!(
            after_success
                .runtime_json
                .get("listener_status")
                .and_then(Value::as_str),
            Some("listening")
        );
        assert_eq!(
            after_success.runtime_json.get("context_tokens"),
            Some(&json!({"wx-user": "ctx-001"}))
        );
        assert_eq!(
            after_success
                .runtime_json
                .get("last_driver")
                .and_then(Value::as_str),
            Some("openclaw_weixin")
        );

        let mut error_patch = Map::new();
        error_patch.insert(
            "listener_status".to_string(),
            Value::String("error".to_string()),
        );
        patch_runtime_error(&state, channel_id, "driver failed", &error_patch).await;

        let after_error = state
            .storage()
            .get_notification_runtime(channel_id)
            .await
            .expect("runtime after error");
        assert_eq!(after_error.status, "error");
        assert_eq!(after_error.last_error, "driver failed");
        assert!(after_error.last_attempt_at >= after_success.last_attempt_at);
        assert_eq!(after_error.last_success_at, after_success.last_success_at);
        assert_eq!(
            after_error
                .runtime_json
                .get("listener_status")
                .and_then(Value::as_str),
            Some("error")
        );
        assert_eq!(
            after_error.runtime_json.get("context_tokens"),
            Some(&json!({"wx-user": "ctx-001"}))
        );
    });

    clear_env();
}

#[test]
fn active_notification_queries_exclude_unsupported_channel_rows() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().expect("temp dir");
    configure_env(&temp_dir);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let state = rt
        .block_on(AppState::bootstrap(AppConfig::from_env().expect("config")))
        .expect("state");

    rt.block_on(async {
        let unsupported_id = sqlx::query(
            r#"
            INSERT INTO notification_channels (name, type, owner_username, config, enabled, created_at, updated_at)
            VALUES ('Unsupported Channel', '__unsupported__', '', '{"bind_all_accounts":true}', 1, 1, 1)
            "#,
        )
        .execute(state.storage().notifications())
        .await
        .expect("insert unsupported channel")
        .last_insert_rowid();
        let retained_id = sqlx::query(
            r#"
            INSERT INTO notification_channels (name, type, owner_username, config, enabled, created_at, updated_at)
            VALUES ('Feishu Active', 'feishu', '', '{"bind_all_accounts":true}', 1, 1, 2)
            "#,
        )
        .execute(state.storage().notifications())
        .await
        .expect("insert retained channel")
        .last_insert_rowid();

        for channel_id in [unsupported_id, retained_id] {
            sqlx::query(
                r#"
                INSERT INTO account_notification_bindings (uid, channel_id, enabled, created_at, updated_at)
                VALUES ('seller-active-list', ?, 1, 1, 1)
                "#,
            )
            .bind(channel_id)
            .execute(state.storage().notifications())
            .await
            .expect("insert binding");
        }

        assert!(state
            .storage()
            .get_notification_channel(unsupported_id)
            .await
            .expect("get unsupported channel")
            .is_none());

        let all_channels = state
            .storage()
            .list_notification_channels()
            .await
            .expect("list all channels");
        assert_eq!(all_channels.len(), 1);
        assert_eq!(all_channels[0].id, retained_id);
        assert_eq!(all_channels[0].channel_type, "feishu");

        let bind_all = state
            .storage()
            .list_bind_all_notification_channels()
            .await
            .expect("list bind all");
        assert_eq!(
            bind_all
                .iter()
                .map(|channel| channel.channel_type.as_str())
                .collect::<Vec<_>>(),
            vec!["feishu"]
        );

        let bindings = state
            .storage()
            .list_notification_bindings(Some("seller-active-list"))
            .await
            .expect("list bindings");
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].channel_id, retained_id);
        assert_eq!(bindings[0].channel_type, "feishu");
    });

    clear_env();
}

#[test]
fn notification_migration_physically_removes_obsolete_channel_rows() {
    let _guard = ENV_LOCK.lock().unwrap();
    let temp_dir = TempDir::new().expect("temp dir");
    configure_env(&temp_dir);

    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let config = AppConfig::from_env().expect("config");
    rt.block_on(seed_legacy_notifications_database(&config));
    let state = rt
        .block_on(AppState::bootstrap(config))
        .expect("state after migrations");

    rt.block_on(async {
        let old_channels: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM notification_channels WHERE lower(type) IN ('telegram', 'webhook', 'email', 'bark')",
        )
        .fetch_one(state.storage().notifications())
        .await
        .expect("old channel count");
        assert_eq!(old_channels, 0);

        let retained_channels: Vec<String> = sqlx::query_scalar(
            "SELECT type FROM notification_channels ORDER BY id ASC",
        )
        .fetch_all(state.storage().notifications())
        .await
        .expect("retained channel types");
        assert_eq!(
            retained_channels,
            vec!["feishu", "dingtalk", "openclaw_weixin"]
        );

        let dingtalk_config: String =
            sqlx::query_scalar("SELECT config FROM notification_channels WHERE type = 'dingtalk'")
                .fetch_one(state.storage().notifications())
                .await
                .expect("dingtalk config");
        let dingtalk_config: Value =
            serde_json::from_str(&dingtalk_config).expect("dingtalk config json");
        assert_eq!(
            dingtalk_config.get("robot_url").and_then(Value::as_str),
            Some("https://oapi.dingtalk.example/robot/send")
        );
        assert!(dingtalk_config.get("webhook_url").is_none());

        let orphan_runtime: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notification_channel_runtime")
                .fetch_one(state.storage().notifications())
                .await
                .expect("runtime count");
        assert_eq!(orphan_runtime, 3);

        let orphan_bindings: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM account_notification_bindings")
                .fetch_one(state.storage().notifications())
                .await
                .expect("binding count");
        assert_eq!(orphan_bindings, 3);
    });

    clear_env();
}

async fn seed_legacy_notifications_database(config: &AppConfig) {
    let options = SqliteConnectOptions::from_str(&format!(
        "sqlite://{}",
        config.sqlite.notifications_db_path.display()
    ))
    .expect("sqlite options")
    .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("connect notifications db");
    for statement in include_str!("../migrations/notifications/0001_initial.sql").split(';') {
        let statement = statement.trim();
        if !statement.is_empty() {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .expect("apply 0001 statement");
        }
    }
    for channel_type in [
        "telegram",
        "webhook",
        "email",
        "bark",
        "feishu",
        "ding_talk",
        "weixin_personal",
    ] {
        let config_json = if channel_type == "ding_talk" {
            json!({
                "robot_url": "",
                "webhook_url": "https://oapi.dingtalk.example/robot/send"
            })
            .to_string()
        } else {
            "{}".to_string()
        };
        let channel_id = sqlx::query(
            r#"
            INSERT INTO notification_channels (name, type, owner_username, config, enabled, created_at, updated_at)
            VALUES (?, ?, '', ?, 1, 1, 1)
            "#,
        )
        .bind(format!("{channel_type} channel"))
        .bind(channel_type)
        .bind(config_json)
        .execute(&pool)
        .await
        .expect("insert legacy channel")
        .last_insert_rowid();
        sqlx::query(
            r#"
            INSERT INTO notification_channel_runtime (channel_id, status, runtime_json, updated_at)
            VALUES (?, 'legacy', '{}', 1)
            "#,
        )
        .bind(channel_id)
        .execute(&pool)
        .await
        .expect("insert legacy runtime");
        sqlx::query(
            r#"
            INSERT INTO account_notification_bindings (uid, channel_id, enabled, created_at, updated_at)
            VALUES ('seller-migration', ?, 1, 1, 1)
            "#,
        )
        .bind(channel_id)
        .execute(&pool)
        .await
        .expect("insert legacy binding");
    }
    pool.close().await;
}

fn configure_env(temp_dir: &TempDir) {
    set_env(
        "APP_DATA_DIR",
        temp_dir.path().to_str().expect("temp dir path"),
    );
    set_env("JWT_SECRET", "notifications-driver-runtime-secret");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn clear_env() {
    clear_env_key("APP_DATA_DIR");
    clear_env_key("JWT_SECRET");
    clear_env_key("ADMIN_USERNAME");
    clear_env_key("ADMIN_PASSWORD");
    clear_env_key("API_TOKEN");
    clear_env_key("JWT_EXPIRE_HOURS");
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
