use std::{
    env,
    sync::{Arc, Mutex},
};

use serde_json::{json, Map, Value};
use tempfile::TempDir;
use xianyu_rust_backend_full::{
    config::AppConfig,
    notifications::{
        feishu_native::{
            FeishuNativeCallFuture, FeishuNativeError, FeishuNativeRequest, FeishuNativeResponse,
            FeishuNativeRuntime, FeishuNativeTransport,
        },
        manager::{restart_worker_for_channel, start_managed_channel_workers},
    },
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[tokio::test]
async fn restart_worker_for_unsupported_channel_is_safe_noop_with_runtime_skip_reason() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_channel(&state, "WeChat Placeholder", "wechat", Map::new(), true).await;

    restart_worker_for_channel(state.clone(), channel_id).await;

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
async fn restart_worker_for_feishu_channel_calls_native_start_and_records_started_runtime() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let channel_id = create_channel(
        &state,
        "Feishu Worker",
        "feishu",
        Map::from_iter([
            ("app_id".to_string(), Value::String("cli-a".to_string())),
            ("app_secret".to_string(), Value::String("sec-a".to_string())),
            ("domain".to_string(), Value::String("feishu".to_string())),
            (
                "bot_open_id".to_string(),
                Value::String("ou_bot".to_string()),
            ),
        ]),
        true,
    )
    .await;

    restart_worker_for_channel(state.clone(), channel_id).await;

    let calls = transport.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| call.method.as_str())
            .collect::<Vec<_>>(),
        vec!["channel.stop", "channel.start"]
    );
    assert_eq!(calls[1].params["channel_id"], channel_id);
    assert_eq!(calls[1].params["app_id"], "cli-a");
    assert_eq!(calls[1].params["bot_open_id"], "ou_bot");

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
        Some("feishu.manager")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("transport_channel_status")
            .and_then(Value::as_str),
        Some("started")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("transport_connected")
            .and_then(Value::as_bool),
        Some(true)
    );

    clear_env();
}

#[tokio::test]
async fn restart_worker_for_feishu_channel_redacts_native_errors_in_runtime() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            FailingTransport,
        )));
    let channel_id = create_channel(
        &state,
        "Broken Feishu Worker",
        "feishu",
        Map::from_iter([
            ("app_id".to_string(), Value::String("cli-a".to_string())),
            (
                "app_secret".to_string(),
                Value::String("sec-super-sensitive".to_string()),
            ),
            ("domain".to_string(), Value::String("feishu".to_string())),
        ]),
        true,
    )
    .await;

    restart_worker_for_channel(state.clone(), channel_id).await;

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "error");
    assert_eq!(runtime.last_error, "Feishu native transport API error");
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_reason")
            .and_then(Value::as_str),
        Some("Feishu native transport API error")
    );
    let runtime_text = serde_json::to_string(&runtime.runtime_json).unwrap();
    assert!(!runtime_text.contains("sec-super-sensitive"));
    assert!(!runtime_text.contains("sdk-token"));

    clear_env();
}

#[tokio::test]
async fn startup_scan_spawns_supported_openclaw_workers_and_preserves_existing_runtime_restart_fields(
) {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let openclaw_id = create_channel(
        &state,
        "Startup WeChat Worker",
        "openclaw_weixin",
        Map::from_iter([
            (
                "base_url".to_string(),
                Value::String("https://example.invalid".to_string()),
            ),
            (
                "bot_token".to_string(),
                Value::String("startup-bot-token".to_string()),
            ),
            (
                "ilink_user_id".to_string(),
                Value::String("wx-user".to_string()),
            ),
        ]),
        true,
    )
    .await;
    let disabled_openclaw_id = create_channel(
        &state,
        "Disabled Startup WeChat Worker",
        "openclaw_weixin",
        Map::from_iter([
            (
                "base_url".to_string(),
                Value::String("https://example.invalid".to_string()),
            ),
            (
                "bot_token".to_string(),
                Value::String("disabled-startup-bot-token".to_string()),
            ),
            (
                "ilink_user_id".to_string(),
                Value::String("wx-user-disabled".to_string()),
            ),
        ]),
        false,
    )
    .await;
    let unsupported_id = create_channel(
        &state,
        "Startup WeChat Placeholder",
        "wechat",
        Map::new(),
        true,
    )
    .await;

    state
        .storage()
        .patch_notification_runtime(
            openclaw_id,
            &Map::from_iter([(
                "worker_restart_status".to_string(),
                Value::String("preexisting-openclaw".to_string()),
            )]),
        )
        .await
        .unwrap();
    state
        .storage()
        .patch_notification_runtime(
            disabled_openclaw_id,
            &Map::from_iter([(
                "worker_restart_status".to_string(),
                Value::String("preexisting-disabled".to_string()),
            )]),
        )
        .await
        .unwrap();
    state
        .storage()
        .patch_notification_runtime(
            unsupported_id,
            &Map::from_iter([(
                "worker_restart_status".to_string(),
                Value::String("preexisting-unsupported".to_string()),
            )]),
        )
        .await
        .unwrap();

    start_managed_channel_workers(state.clone()).await;

    let openclaw_runtime = state
        .storage()
        .get_notification_runtime(openclaw_id)
        .await
        .unwrap();
    assert!(openclaw_runtime.last_attempt_at >= 0.0);
    assert_eq!(
        openclaw_runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("preexisting-openclaw")
    );

    let disabled_runtime = state
        .storage()
        .get_notification_runtime(disabled_openclaw_id)
        .await
        .unwrap();
    assert!(disabled_runtime.last_attempt_at >= 0.0);
    assert_eq!(
        disabled_runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("preexisting-disabled")
    );

    let unsupported_runtime = state
        .storage()
        .get_notification_runtime(unsupported_id)
        .await
        .unwrap();
    assert!(unsupported_runtime.last_attempt_at >= 0.0);
    assert_eq!(
        unsupported_runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("preexisting-unsupported")
    );

    clear_env();
}

#[tokio::test]
async fn startup_scan_starts_enabled_feishu_native_channels() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let transport = RecordingTransport::default();
    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config)
        .await
        .unwrap()
        .with_feishu_native(FeishuNativeRuntime::with_transport(Arc::new(
            transport.clone(),
        )));
    let enabled_id = create_channel(
        &state,
        "Startup Feishu Worker",
        "feishu",
        Map::from_iter([
            (
                "app_id".to_string(),
                Value::String("cli-startup".to_string()),
            ),
            (
                "app_secret".to_string(),
                Value::String("sec-startup".to_string()),
            ),
            ("domain".to_string(), Value::String("feishu".to_string())),
            (
                "bot_open_id".to_string(),
                Value::String("ou_startup_bot".to_string()),
            ),
        ]),
        true,
    )
    .await;
    let disabled_id = create_channel(
        &state,
        "Disabled Startup Feishu Worker",
        "feishu",
        Map::from_iter([
            (
                "app_id".to_string(),
                Value::String("cli-disabled".to_string()),
            ),
            (
                "app_secret".to_string(),
                Value::String("sec-disabled".to_string()),
            ),
        ]),
        false,
    )
    .await;

    start_managed_channel_workers(state.clone()).await;

    let calls = transport.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "channel.start");
    assert_eq!(calls[0].params["channel_id"], enabled_id);
    assert_eq!(calls[0].params["app_id"], "cli-startup");
    assert_eq!(calls[0].params["domain"], "feishu");
    assert_eq!(calls[0].params["bot_open_id"], "ou_startup_bot");

    let enabled_runtime = state
        .storage()
        .get_notification_runtime(enabled_id)
        .await
        .unwrap();
    assert_eq!(enabled_runtime.status, "healthy");
    assert_eq!(
        enabled_runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("spawned")
    );
    assert_eq!(
        enabled_runtime
            .runtime_json
            .get("worker_restart_source")
            .and_then(Value::as_str),
        Some("feishu.manager")
    );
    assert_eq!(
        enabled_runtime
            .runtime_json
            .get("transport_channel_status")
            .and_then(Value::as_str),
        Some("started")
    );
    assert_eq!(
        enabled_runtime
            .runtime_json
            .get("transport_connected")
            .and_then(Value::as_bool),
        Some(true)
    );

    let disabled_runtime = state
        .storage()
        .get_notification_runtime(disabled_id)
        .await
        .unwrap();
    assert_eq!(disabled_runtime.status, "");
    assert!(disabled_runtime.runtime_json.is_empty());

    clear_env();
}

#[tokio::test]
async fn restart_worker_for_disabled_openclaw_channel_records_runtime_skip() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_channel(
        &state,
        "Disabled WeChat Worker",
        "openclaw_weixin",
        Map::from_iter([
            (
                "base_url".to_string(),
                Value::String("https://example.invalid".to_string()),
            ),
            (
                "bot_token".to_string(),
                Value::String("bot-token".to_string()),
            ),
            (
                "ilink_user_id".to_string(),
                Value::String("wx-user".to_string()),
            ),
        ]),
        false,
    )
    .await;

    restart_worker_for_channel(state.clone(), channel_id).await;

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
async fn restart_worker_for_misconfigured_wechat_channel_records_error_reason() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let temp_dir = TempDir::new().unwrap();
    configure_env(&temp_dir);

    let config = AppConfig::from_env().unwrap();
    let state = AppState::bootstrap(config).await.unwrap();
    let channel_id = create_channel(
        &state,
        "Broken WeChat Worker",
        "openclaw_weixin",
        Map::from_iter([(
            "base_url".to_string(),
            Value::String("https://example.invalid".to_string()),
        )]),
        true,
    )
    .await;

    restart_worker_for_channel(state.clone(), channel_id).await;

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .unwrap();
    assert_eq!(runtime.status, "error");
    assert_eq!(runtime.last_error, "missing base_url or bot_token");
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_status")
            .and_then(Value::as_str),
        Some("error")
    );
    assert_eq!(
        runtime
            .runtime_json
            .get("worker_restart_source")
            .and_then(Value::as_str),
        Some("wechat.manager")
    );

    clear_env();
}

async fn create_channel(
    state: &AppState,
    name: &str,
    channel_type: &str,
    config: Map<String, Value>,
    enabled: bool,
) -> i64 {
    state
        .storage()
        .create_notification_channel(name, channel_type, &config, enabled)
        .await
        .unwrap()
        .id
}

#[derive(Clone, Default)]
struct RecordingTransport {
    calls: Arc<Mutex<Vec<FeishuNativeRequest>>>,
}

impl RecordingTransport {
    fn calls(&self) -> Vec<FeishuNativeRequest> {
        self.calls.lock().unwrap().clone()
    }
}

impl FeishuNativeTransport for RecordingTransport {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        self.calls.lock().unwrap().push(request.clone());
        Box::pin(async move {
            let result = match request.method.as_str() {
                "channel.start" => json!({
                    "status": "started",
                    "channel_id": request.params["channel_id"]
                }),
                "channel.stop" => json!({
                    "status": "stopped",
                    "channel_id": request.params["channel_id"]
                }),
                other => panic!("unexpected method {other}"),
            };
            Ok(FeishuNativeResponse::success(result))
        })
    }
}

#[derive(Clone)]
struct FailingTransport;

impl FeishuNativeTransport for FailingTransport {
    fn call(&self, _request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        Box::pin(async move {
            Err(FeishuNativeError::Api(
                "sdk failed app_secret=sec-super-sensitive token=sdk-token".to_string(),
            ))
        })
    }
}

fn configure_env(temp_dir: &TempDir) {
    set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
    set_env("JWT_SECRET", "notifications-worker-restart-secret");
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
