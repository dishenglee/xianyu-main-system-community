use serde_json::{json, Map, Value};
use tracing::warn;

use crate::{
    models::notifications::{
        normalize_notification_type, NotificationChannel, NotificationRuntime,
    },
    notifications::{
        channel_send,
        feishu_native::MessageSendRequest,
        openclaw_context_cache,
        template::{
            negotiate_template, DriverContentCapabilities, NegotiatedNotificationContent,
            NotificationContentKind, NotificationTemplate, RenderedNotificationContent,
        },
    },
    state::AppState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationDriverKind {
    DingtalkRobot,
    EnterpriseWechatRobot,
    OpenclawWeixin,
    Feishu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverCapabilities {
    pub returns_message_id: bool,
    pub requires_channel_id: bool,
    pub uses_context_token: bool,
    pub content: DriverContentCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverContentRequest {
    pub preferred: NotificationContentKind,
}

impl DriverContentRequest {
    pub fn text() -> Self {
        Self {
            preferred: NotificationContentKind::Text,
        }
    }

    pub fn html() -> Self {
        Self {
            preferred: NotificationContentKind::Html,
        }
    }

    pub fn markdown() -> Self {
        Self {
            preferred: NotificationContentKind::Markdown,
        }
    }

    pub fn media_preview() -> Self {
        Self {
            preferred: NotificationContentKind::MediaPreview,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverContentSelection {
    pub requested: NotificationContentKind,
    pub selected_kind: NotificationContentKind,
    pub body: String,
    pub fallback_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverResolution {
    pub normalized_type: String,
    pub kind: NotificationDriverKind,
    pub capabilities: DriverCapabilities,
}

#[derive(Debug, Clone)]
pub struct DriverSendRequest<'a> {
    pub channel_id: i64,
    pub channel_type: &'a str,
    pub config: &'a Map<String, Value>,
    pub text: &'a str,
    pub rendered: Option<&'a RenderedNotificationContent>,
}

#[derive(Debug, Clone)]
pub struct RuntimeSendRequest<'a> {
    pub channel_id: i64,
    pub channel_type: &'a str,
    pub config: &'a Map<String, Value>,
    pub text: &'a str,
    pub rendered: Option<&'a RenderedNotificationContent>,
    pub success_runtime_patch: &'a Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DriverSendOutcome {
    pub message_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    Healthy,
    Error,
}

impl RuntimeStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeUpdate {
    pub status: RuntimeStatus,
    pub last_error: String,
    pub last_attempt_at: f64,
    pub last_success_at: Option<f64>,
    pub runtime_json: Map<String, Value>,
}

pub fn resolve_driver(channel_type: &str) -> Result<DriverResolution, String> {
    const OPENCLAW_WEIXIN_CONTENT: DriverContentCapabilities = DriverContentCapabilities {
        preferred: NotificationContentKind::Markdown,
        supported: &[
            NotificationContentKind::Markdown,
            NotificationContentKind::Text,
        ],
        downgrade_order: &[
            NotificationContentKind::Markdown,
            NotificationContentKind::Text,
        ],
    };
    const FEISHU_CONTENT: DriverContentCapabilities = DriverContentCapabilities {
        preferred: NotificationContentKind::Text,
        supported: &[NotificationContentKind::Text],
        downgrade_order: &[NotificationContentKind::Text],
    };
    const ROBOT_TEXT_CONTENT: DriverContentCapabilities = DriverContentCapabilities {
        preferred: NotificationContentKind::Text,
        supported: &[NotificationContentKind::Text],
        downgrade_order: &[NotificationContentKind::Text],
    };

    let normalized = normalize_notification_type(channel_type)
        .ok_or_else(|| format!("unsupported notification channel type: {channel_type}"))?;

    let (kind, capabilities) = match normalized.as_str() {
        "dingtalk" => (
            NotificationDriverKind::DingtalkRobot,
            DriverCapabilities {
                returns_message_id: false,
                requires_channel_id: true,
                uses_context_token: false,
                content: ROBOT_TEXT_CONTENT,
            },
        ),
        "wechat" => (
            NotificationDriverKind::EnterpriseWechatRobot,
            DriverCapabilities {
                returns_message_id: false,
                requires_channel_id: true,
                uses_context_token: false,
                content: ROBOT_TEXT_CONTENT,
            },
        ),
        "openclaw_weixin" => (
            NotificationDriverKind::OpenclawWeixin,
            DriverCapabilities {
                returns_message_id: false,
                requires_channel_id: true,
                uses_context_token: true,
                content: OPENCLAW_WEIXIN_CONTENT,
            },
        ),
        "feishu" | "lark" => (
            NotificationDriverKind::Feishu,
            DriverCapabilities {
                returns_message_id: true,
                requires_channel_id: true,
                uses_context_token: false,
                content: FEISHU_CONTENT,
            },
        ),
        other => {
            return Err(format!("unsupported notification channel type: {other}"));
        }
    };

    Ok(DriverResolution {
        normalized_type: normalized,
        kind,
        capabilities,
    })
}

pub fn negotiate_driver_content(
    resolution: &DriverResolution,
    template: &NotificationTemplate,
    request: DriverContentRequest,
) -> DriverContentSelection {
    let NegotiatedNotificationContent {
        requested,
        selected,
        fallback_text,
    } = negotiate_template(template, resolution.capabilities.content, request.preferred);

    DriverContentSelection {
        requested,
        selected_kind: selected.kind,
        body: selected.body,
        fallback_text,
    }
}

pub fn render_driver_content(
    resolution: &DriverResolution,
    template: &NotificationTemplate,
    request: DriverContentRequest,
) -> RenderedNotificationContent {
    let selection = negotiate_driver_content(resolution, template, request);
    RenderedNotificationContent {
        requested_kind: selection.requested,
        selected_kind: selection.selected_kind,
        body: selection.body,
        fallback_text: selection.fallback_text,
    }
}

pub async fn send_via_driver(request: DriverSendRequest<'_>) -> Result<DriverSendOutcome, String> {
    send_via_driver_with_error_adapter(request, |err| err.to_string()).await
}

pub async fn send_via_driver_with_error_adapter<F>(
    request: DriverSendRequest<'_>,
    adapt_error: F,
) -> Result<DriverSendOutcome, String>
where
    F: FnOnce(&str) -> String,
{
    let resolution = resolve_driver(request.channel_type)?;
    match resolution.kind {
        NotificationDriverKind::DingtalkRobot => {
            channel_send::send_dingtalk_robot_text(channel_send::RenderedSendRequest {
                channel_id: request.channel_id,
                config: request.config,
                text: request.text,
                rendered: request.rendered,
            })
            .await
            .map_err(|err| adapt_error(&err))?;
            Ok(DriverSendOutcome::default())
        }
        NotificationDriverKind::EnterpriseWechatRobot => {
            channel_send::send_enterprise_wechat_robot_text(channel_send::RenderedSendRequest {
                channel_id: request.channel_id,
                config: request.config,
                text: request.text,
                rendered: request.rendered,
            })
            .await
            .map_err(|err| adapt_error(&err))?;
            Ok(DriverSendOutcome::default())
        }
        NotificationDriverKind::OpenclawWeixin => {
            channel_send::send_openclaw_text(channel_send::RenderedSendRequest {
                channel_id: request.channel_id,
                config: request.config,
                text: request.text,
                rendered: request.rendered,
            })
            .await
            .map_err(|err| adapt_error(&err))?;
            Ok(DriverSendOutcome::default())
        }
        NotificationDriverKind::Feishu => Err(adapt_error(
            "Feishu driver requires AppState native runtime",
        )),
    }
}

pub async fn send_channel_text(
    channel: &NotificationChannel,
    text: &str,
) -> Result<DriverSendOutcome, String> {
    send_via_driver(DriverSendRequest {
        channel_id: channel.id,
        channel_type: &channel.channel_type,
        config: &channel.config,
        text,
        rendered: None,
    })
    .await
}

pub async fn send_channel_text_with_runtime(
    state: &AppState,
    channel: &NotificationChannel,
    text: &str,
) -> Result<DriverSendOutcome, String> {
    send_text_with_runtime_parts(
        state,
        channel.id,
        &channel.channel_type,
        &channel.config,
        text,
        None,
        &Map::new(),
    )
    .await
}

pub async fn send_text_with_runtime_parts(
    state: &AppState,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    text: &str,
    rendered: Option<&RenderedNotificationContent>,
    success_runtime_patch: &Map<String, Value>,
) -> Result<DriverSendOutcome, String> {
    send_runtime_request_with_error_adapter(
        state,
        RuntimeSendRequest {
            channel_id,
            channel_type,
            config,
            text,
            rendered,
            success_runtime_patch,
        },
        |err| err.to_string(),
    )
    .await
}

pub async fn send_feishu_interactive_with_runtime_parts(
    state: &AppState,
    channel_id: i64,
    config: &Map<String, Value>,
    content: Value,
    success_runtime_patch: &Map<String, Value>,
) -> Result<DriverSendOutcome, String> {
    match send_feishu_payload_with_runtime(
        state,
        channel_id,
        config,
        "interactive",
        content,
        success_runtime_patch,
    )
    .await
    {
        Ok((outcome, runtime_patch)) => {
            patch_runtime_success(state, channel_id, &runtime_patch).await;
            Ok(outcome)
        }
        Err(err) => {
            patch_runtime_error(state, channel_id, &err, &Map::new()).await;
            Err(err)
        }
    }
}

pub async fn send_runtime_request(
    state: &AppState,
    request: RuntimeSendRequest<'_>,
) -> Result<DriverSendOutcome, String> {
    send_runtime_request_with_error_adapter(state, request, |err| err.to_string()).await
}

pub async fn send_text_with_runtime_parts_and_error_adapter<F>(
    state: &AppState,
    channel_id: i64,
    channel_type: &str,
    config: &Map<String, Value>,
    text: &str,
    rendered: Option<&RenderedNotificationContent>,
    success_runtime_patch: &Map<String, Value>,
    adapt_error: F,
) -> Result<DriverSendOutcome, String>
where
    F: FnOnce(&str) -> String,
{
    send_runtime_request_with_error_adapter(
        state,
        RuntimeSendRequest {
            channel_id,
            channel_type,
            config,
            text,
            rendered,
            success_runtime_patch,
        },
        adapt_error,
    )
    .await
}

pub async fn send_runtime_request_with_error_adapter<F>(
    state: &AppState,
    request: RuntimeSendRequest<'_>,
    adapt_error: F,
) -> Result<DriverSendOutcome, String>
where
    F: FnOnce(&str) -> String,
{
    let resolution = match resolve_driver(request.channel_type) {
        Ok(resolution) => resolution,
        Err(err) => {
            patch_runtime_error(state, request.channel_id, &err, &Map::new()).await;
            return Err(err);
        }
    };

    let result = match resolution.kind {
        NotificationDriverKind::DingtalkRobot | NotificationDriverKind::EnterpriseWechatRobot => {
            let driver_name = match resolution.kind {
                NotificationDriverKind::DingtalkRobot => "dingtalk",
                NotificationDriverKind::EnterpriseWechatRobot => "wechat",
                _ => unreachable!(),
            };
            send_via_driver_with_error_adapter(
                DriverSendRequest {
                    channel_id: request.channel_id,
                    channel_type: request.channel_type,
                    config: request.config,
                    text: request.text,
                    rendered: request.rendered,
                },
                adapt_error,
            )
            .await
            .map(|outcome| {
                let mut runtime_patch = request.success_runtime_patch.clone();
                runtime_patch.insert(
                    "last_robot_driver".to_string(),
                    Value::String(driver_name.to_string()),
                );
                (outcome, runtime_patch)
            })
        }
        NotificationDriverKind::OpenclawWeixin => send_via_driver_with_error_adapter(
            DriverSendRequest {
                channel_id: request.channel_id,
                channel_type: request.channel_type,
                config: request.config,
                text: request.text,
                rendered: request.rendered,
            },
            adapt_error,
        )
        .await
        .map(|outcome| (outcome, request.success_runtime_patch.clone())),
        NotificationDriverKind::Feishu => send_feishu_with_runtime(state, &request)
            .await
            .map_err(|err| adapt_error(&err)),
    };

    match result {
        Ok(outcome) => {
            patch_runtime_success(state, request.channel_id, &outcome.1).await;
            Ok(outcome.0)
        }
        Err(err) => {
            patch_runtime_error(state, request.channel_id, &err, &Map::new()).await;
            Err(err)
        }
    }
}

async fn send_feishu_with_runtime(
    state: &AppState,
    request: &RuntimeSendRequest<'_>,
) -> Result<(DriverSendOutcome, Map<String, Value>), String> {
    send_feishu_payload_with_runtime(
        state,
        request.channel_id,
        request.config,
        "text",
        json!({ "text": rendered_send_body(request.rendered, request.text) }),
        request.success_runtime_patch,
    )
    .await
}

async fn send_feishu_payload_with_runtime(
    state: &AppState,
    channel_id: i64,
    config: &Map<String, Value>,
    msg_type: &str,
    content: Value,
    success_runtime_patch: &Map<String, Value>,
) -> Result<(DriverSendOutcome, Map<String, Value>), String> {
    let target = feishu_send_target_for_send(state, channel_id, config).await?;
    let response = state
        .feishu_native()
        .message_send(MessageSendRequest {
            channel_id,
            receive_id: target.receive_id.clone(),
            receive_id_type: target.receive_id_type.clone(),
            msg_type: msg_type.to_string(),
            content,
            app_id: non_empty(&config_string(config, "app_id")),
            app_secret: non_empty(&config_string(config, "app_secret")),
            domain: non_empty(&config_string(config, "domain")),
            reply_to_message_id: None,
            reply_in_thread: None,
            fallback_to_plain_chat: Some(true),
            source_event_id: None,
        })
        .await
        .map_err(|err| err.to_string())?;

    let message_id = response.message_id;
    let mut runtime_patch = success_runtime_patch.clone();
    runtime_patch.insert(
        "last_feishu_notify_message_id".to_string(),
        Value::String(message_id.clone()),
    );
    runtime_patch.insert(
        "last_feishu_receive_id".to_string(),
        Value::String(target.receive_id),
    );
    runtime_patch.insert(
        "last_feishu_receive_id_type".to_string(),
        Value::String(target.receive_id_type),
    );
    runtime_patch.insert(
        "last_feishu_receive_target".to_string(),
        Value::String(target.source.to_string()),
    );
    runtime_patch.insert(
        "last_feishu_notify_fell_back".to_string(),
        Value::Bool(response.fell_back),
    );
    Ok((
        DriverSendOutcome {
            message_id: Some(message_id),
        },
        runtime_patch,
    ))
}

#[derive(Debug, Clone)]
struct FeishuSendTarget {
    receive_id: String,
    receive_id_type: String,
    source: &'static str,
}

async fn feishu_send_target_for_send(
    state: &AppState,
    channel_id: i64,
    config: &Map<String, Value>,
) -> Result<FeishuSendTarget, String> {
    let configured = config_string(config, "receive_id");
    let configured_type = {
        let value = config_string(config, "receive_id_type");
        if value.is_empty() {
            "chat_id".to_string()
        } else {
            value
        }
    };
    if !configured.is_empty() {
        return Ok(FeishuSendTarget {
            receive_id: configured,
            receive_id_type: configured_type,
            source: "config.receive_id",
        });
    }

    let runtime = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .map_err(|err| err.to_string())?;
    let runtime_chat_id = runtime
        .runtime_json
        .get("last_chat_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if !runtime_chat_id.is_empty() {
        return Ok(FeishuSendTarget {
            receive_id: runtime_chat_id,
            receive_id_type: "chat_id".to_string(),
            source: "runtime.last_chat_id",
        });
    }

    if let Some(operator_open_id) = first_feishu_operator_id(config) {
        return Ok(FeishuSendTarget {
            receive_id: operator_open_id,
            receive_id_type: "open_id".to_string(),
            source: "config.allowed_operator_ids",
        });
    }

    Err("missing receive_id for Feishu channel".to_string())
}

fn first_feishu_operator_id(config: &Map<String, Value>) -> Option<String> {
    for key in [
        "allowed_operator_ids",
        "allowed_sender_ids",
        "authorized_sender_ids",
        "operator_user_ids",
        "operator_ids",
        "operator_open_id",
    ] {
        if let Some(value) = config.get(key) {
            if let Some(first) = first_string_value(value) {
                return Some(first);
            }
        }
    }
    None
}

fn first_string_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => text
            .split([',', ';', '\n', '\r', '\t', ' '])
            .map(str::trim)
            .find(|part| !part.is_empty())
            .map(str::to_string),
        Value::Array(items) => items.iter().find_map(first_string_value),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn rendered_send_body<'a>(
    rendered: Option<&'a RenderedNotificationContent>,
    fallback_text: &'a str,
) -> &'a str {
    rendered
        .map(|value| value.body.as_str())
        .unwrap_or(fallback_text)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub async fn patch_runtime_success(
    state: &AppState,
    channel_id: i64,
    runtime_patch: &Map<String, Value>,
) {
    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .ok();
    let update = build_runtime_update(existing.as_ref(), RuntimeStatus::Healthy, "", runtime_patch);
    persist_runtime_update(state, channel_id, &update).await;
}

pub async fn patch_runtime_error(
    state: &AppState,
    channel_id: i64,
    err: &str,
    runtime_patch: &Map<String, Value>,
) {
    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .ok();
    let update = build_runtime_update(existing.as_ref(), RuntimeStatus::Error, err, runtime_patch);
    persist_runtime_update(state, channel_id, &update).await;
}

pub fn build_runtime_success_update(
    existing: Option<&NotificationRuntime>,
    runtime_patch: &Map<String, Value>,
) -> RuntimeUpdate {
    build_runtime_update(existing, RuntimeStatus::Healthy, "", runtime_patch)
}

pub fn build_runtime_error_update(
    existing: Option<&NotificationRuntime>,
    err: &str,
    runtime_patch: &Map<String, Value>,
) -> RuntimeUpdate {
    build_runtime_update(existing, RuntimeStatus::Error, err, runtime_patch)
}

fn build_runtime_update(
    existing: Option<&NotificationRuntime>,
    status: RuntimeStatus,
    err: &str,
    runtime_patch: &Map<String, Value>,
) -> RuntimeUpdate {
    let now = unix_now();
    let mut merged_runtime = existing
        .map(|runtime| runtime.runtime_json.clone())
        .unwrap_or_default();
    merged_runtime.extend(runtime_patch.clone());
    merged_runtime.insert("last_error".into(), Value::String(err.to_string()));

    let last_success_at = match status {
        RuntimeStatus::Healthy => Some(now),
        RuntimeStatus::Error => existing.map(|runtime| runtime.last_success_at.max(0.0)),
    };

    RuntimeUpdate {
        status,
        last_error: err.to_string(),
        last_attempt_at: now,
        last_success_at,
        runtime_json: merged_runtime,
    }
}

async fn persist_runtime_update(state: &AppState, channel_id: i64, update: &RuntimeUpdate) {
    if let Err(persist_err) = state
        .storage()
        .upsert_notification_runtime(
            channel_id,
            update.status.as_str(),
            &update.last_error,
            Some(update.last_attempt_at),
            update.last_success_at,
            &update.runtime_json,
        )
        .await
    {
        warn!(
            target: "notifications.driver",
            stream = "runtime",
            module = "notifications-driver",
            action = "persist_runtime_update",
            outcome = "warn",
            channel_id,
            status = ?update.status,
            error_code = "notification_runtime_persist_failed",
            retryable = true,
            detail = %persist_err,
            "failed to persist notification runtime update"
        );
    }
}

pub fn openclaw_context_token_for_send(
    channel_id: i64,
    config: &Map<String, Value>,
    to_user_id: &str,
) -> String {
    openclaw_context_cache::get(channel_id, to_user_id)
        .unwrap_or_else(|| config_string(config, "context_token"))
}

pub(crate) fn config_string(config: &Map<String, Value>, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
