use axum::{
    extract::{Extension, Path, Query, State},
    routing::get,
    Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{error, info, warn};

use crate::{
    logging,
    models::auth::RequestIdentity,
    routes::{
        error::{ApiResult, ResponseError},
        tenant::{ensure_admin_or_legacy, is_admin_or_legacy},
    },
    state::AppState,
    storage::RuntimeLogFilter,
    telemetry::{IncidentInput, IncidentReport},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/logs"),
            get(get_logs).delete(clear_logs),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/logs/diagnostic-policy"),
            get(get_runtime_diagnostic_policy).put(update_runtime_diagnostic_policy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/logs/diagnostics/policy"),
            get(get_runtime_diagnostic_policy_client),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/logs/incidents"),
            get(list_incidents).post(create_incident),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/logs/incidents/:incident_id"),
            get(get_incident),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/logs/client-events"),
            axum::routing::post(create_client_event),
        )
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    #[serde(default)]
    stream: String,
    #[serde(default)]
    level: String,
    #[serde(default)]
    keyword: String,
    #[serde(default)]
    logger: String,
    #[serde(default)]
    module: String,
    #[serde(default)]
    trace_id: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    incident_id: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    device_id: String,
    #[serde(default)]
    account_uid: String,
    #[serde(default)]
    session_id: String,
    #[serde(default)]
    item_id: String,
    #[serde(default)]
    job_id: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    outcome: String,
    #[serde(default)]
    error_code: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    since: f64,
    #[serde(default)]
    after_sequence: u64,
}

#[derive(Debug, Deserialize)]
struct IncidentsQuery {
    #[serde(default)]
    level: String,
    #[serde(default)]
    module: String,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    incident_id: String,
    #[serde(default = "default_limit")]
    limit: usize,
    #[serde(default)]
    since: f64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientEventPayload {
    #[serde(default)]
    level: String,
    #[serde(default)]
    stream: String,
    #[serde(default)]
    logger: String,
    #[serde(default)]
    module: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    route: String,
    #[serde(default)]
    trace_id: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    error_code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    detail: String,
    #[serde(default)]
    http_status: Option<u16>,
    #[serde(default)]
    retryable: bool,
    #[serde(default)]
    promote_to_incident: bool,
    #[serde(default = "default_context_json")]
    context: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDiagnosticPolicyPayload {
    #[serde(default)]
    enabled: bool,
    #[serde(default = "default_runtime_diagnostic_level")]
    level: String,
    #[serde(default)]
    modules: Vec<String>,
    #[serde(default)]
    usernames: Vec<String>,
    #[serde(default)]
    device_ids: Vec<String>,
    #[serde(default)]
    account_uids: Vec<String>,
    #[serde(default)]
    session_ids: Vec<String>,
    #[serde(default)]
    starts_at: Option<f64>,
    #[serde(default)]
    ends_at: Option<f64>,
    #[serde(default)]
    include_payloads: bool,
    #[serde(default = "default_runtime_diagnostic_sample_rate")]
    sample_rate: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClientEventLevel {
    Info,
    Warn,
    Error,
}

async fn get_logs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<LogsQuery>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    state
        .storage()
        .sync_runtime_log_buffer()
        .await
        .map_err(ResponseError::internal)?;
    let entries = state
        .storage()
        .list_runtime_logs(&RuntimeLogFilter {
            stream: query.stream.clone(),
            level: query.level.clone(),
            keyword: query.keyword.clone(),
            logger: query.logger.clone(),
            module: query.module.clone(),
            trace_id: query.trace_id.clone(),
            request_id: query.request_id.clone(),
            incident_id: query.incident_id.clone(),
            username: query.username.clone(),
            device_id: query.device_id.clone(),
            account_uid: query.account_uid.clone(),
            session_id: query.session_id.clone(),
            item_id: query.item_id.clone(),
            job_id: query.job_id.clone(),
            action: query.action.clone(),
            outcome: query.outcome.clone(),
            error_code: query.error_code.clone(),
            since: query.since.max(0.0),
            after_sequence: query.after_sequence,
            limit: query.limit.clamp(1, 2000),
        })
        .await
        .map_err(ResponseError::internal)?;
    let latest_sequence = entries.last().map(|entry| entry.sequence).unwrap_or(
        state
            .storage()
            .latest_runtime_log_sequence(
                (!query.stream.trim().is_empty()).then_some(query.stream.as_str()),
            )
            .await
            .map_err(ResponseError::internal)?,
    );

    Ok(ResponseJson(json!({
        "data": entries,
        "count": entries.len(),
        "latestSequence": latest_sequence,
        "nextAfterSequence": latest_sequence,
    })))
}

async fn clear_logs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    state
        .storage()
        .clear_runtime_log_storage()
        .await
        .map_err(ResponseError::internal)?;
    logging::clear_logs();
    Ok(ResponseJson(json!({
        "status": "ok",
        "message": "日志已清除",
    })))
}

async fn get_runtime_diagnostic_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let policy = state
        .storage()
        .load_runtime_diagnostic_policy()
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "data": policy,
    })))
}

async fn get_runtime_diagnostic_policy_client(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let policy = state
        .storage()
        .load_runtime_diagnostic_policy()
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "data": scoped_runtime_diagnostic_policy(&state, &identity, policy),
    })))
}

async fn update_runtime_diagnostic_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<RuntimeDiagnosticPolicyPayload>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let level = validate_runtime_diagnostic_level(&payload.level)?;
    let sample_rate = validate_runtime_diagnostic_sample_rate(payload.sample_rate)?;
    let starts_at = validate_runtime_diagnostic_timestamp(payload.starts_at, "startsAt")?;
    let ends_at = validate_runtime_diagnostic_timestamp(payload.ends_at, "endsAt")?;
    if let (Some(starts_at), Some(ends_at)) = (starts_at, ends_at) {
        if ends_at < starts_at {
            return Err(ResponseError::bad_request("endsAt 不能早于 startsAt"));
        }
    }
    let policy = state
        .storage()
        .save_runtime_diagnostic_policy(
            payload.enabled,
            level,
            &payload.modules,
            &payload.usernames,
            &payload.device_ids,
            &payload.account_uids,
            &payload.session_ids,
            starts_at,
            ends_at,
            payload.include_payloads,
            sample_rate,
        )
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({
        "data": policy,
    })))
}

async fn create_client_event(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<ClientEventPayload>,
) -> ApiResult {
    let level = normalize_client_event_level(&payload.level);
    let stream = normalize_client_event_stream(&payload.stream, level);
    let logger = client_event_target(&payload.logger);
    let module = normalize_client_text(&payload.module, "frontend");
    let action = normalize_client_text(&payload.action, "client_event");
    let route = normalize_client_text(&payload.route, "");
    let trace_id = normalize_client_correlation(&payload.trace_id, &identity.correlation.trace_id);
    let request_id = normalize_client_optional_correlation(
        &payload.request_id,
        identity.correlation.request_id.as_deref(),
    );
    let error_code = normalize_client_text(&payload.error_code, "client_event");
    let message = build_client_event_message(&payload);
    let detail = normalize_client_text(&payload.detail, "");
    let context = sanitize_client_value(payload.context);
    let context_json = serde_json::to_string(&context).unwrap_or_else(|_| "{}".to_string());

    emit_client_event_log(
        level,
        logger,
        stream,
        &module,
        &action,
        &route,
        &trace_id,
        request_id.as_deref(),
        identity.username.trim(),
        &error_code,
        &message,
        &detail,
        payload.http_status,
        payload.retryable,
        &context_json,
    );
    state
        .storage()
        .sync_runtime_log_buffer()
        .await
        .map_err(ResponseError::internal)?;

    let incident_id = if payload.promote_to_incident || level == ClientEventLevel::Error {
        let report = state
            .incident_reporter()
            .ingest_external(
                &state,
                IncidentInput {
                    error_level: level.as_str().to_string(),
                    module,
                    action,
                    route,
                    error_code,
                    error_message: if detail.is_empty() {
                        message.clone()
                    } else {
                        format!("{message}: {detail}")
                    },
                    http_status: payload.http_status,
                    upstream_ret: String::new(),
                    retryable: payload.retryable,
                    context_json: json!({
                        "stream": stream,
                        "traceId": trace_id,
                        "requestId": request_id,
                        "username": identity.username.trim(),
                        "logger": payload.logger.trim(),
                        "context": context,
                    }),
                    environment: None,
                },
            )
            .await;
        Some(report.incident_id)
    } else {
        None
    };

    Ok(ResponseJson(json!({
        "status": "ok",
        "incidentId": incident_id,
    })))
}

async fn create_incident(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<IncidentInput>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let report = state
        .incident_reporter()
        .ingest_external(&state, payload)
        .await;
    Ok(incident_response(report))
}

async fn list_incidents(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<IncidentsQuery>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let incidents = state.incident_reporter().list(
        query.limit.clamp(1, 500),
        query.since.max(0.0),
        (!query.level.trim().is_empty()).then_some(query.level.as_str()),
        (!query.module.trim().is_empty()).then_some(query.module.as_str()),
        (!query.error_code.trim().is_empty()).then_some(query.error_code.as_str()),
        (!query.incident_id.trim().is_empty()).then_some(query.incident_id.as_str()),
    );
    Ok(ResponseJson(json!({
        "data": incidents,
        "count": incidents.len(),
    })))
}

async fn get_incident(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(incident_id): Path<String>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    match state.incident_reporter().get(incident_id.trim()) {
        Some(report) => Ok(incident_response(report)),
        None => Ok(ResponseJson(json!({
            "data": Value::Null,
            "found": false,
        }))),
    }
}

fn default_limit() -> usize {
    200
}

fn incident_response(report: IncidentReport) -> ResponseJson<Value> {
    ResponseJson(json!({
        "data": report,
        "found": true,
    }))
}

fn default_context_json() -> Value {
    json!({})
}

fn default_runtime_diagnostic_level() -> String {
    "DEBUG".to_string()
}

fn default_runtime_diagnostic_sample_rate() -> f64 {
    1.0
}

fn default_runtime_diagnostic_policy_value() -> Value {
    json!({
        "enabled": false,
        "level": default_runtime_diagnostic_level(),
        "modules": [],
        "usernames": [],
        "deviceIds": [],
        "accountUids": [],
        "sessionIds": [],
        "startsAt": Value::Null,
        "endsAt": Value::Null,
        "includePayloads": false,
        "sampleRate": default_runtime_diagnostic_sample_rate(),
    })
}

fn scoped_runtime_diagnostic_policy(
    state: &AppState,
    identity: &RequestIdentity,
    policy: Value,
) -> Value {
    if is_admin_or_legacy(state, identity) {
        return policy;
    }
    let mut scoped = match policy {
        Value::Object(map) => Value::Object(map),
        _ => default_runtime_diagnostic_policy_value(),
    };
    let Some(object) = scoped.as_object_mut() else {
        return default_runtime_diagnostic_policy_value();
    };

    let usernames = object
        .get("usernames")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_str().map(str::trim).map(ToOwned::to_owned))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    let username_matches = usernames.is_empty()
        || usernames
            .iter()
            .any(|value| value.eq_ignore_ascii_case(identity.username.trim()));
    if !username_matches {
        return default_runtime_diagnostic_policy_value();
    }

    object.insert("usernames".to_string(), Value::Array(Vec::new()));
    object.insert("deviceIds".to_string(), Value::Array(Vec::new()));
    scoped
}

impl ClientEventLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

fn normalize_client_event_level(raw: &str) -> ClientEventLevel {
    match raw.trim().to_ascii_lowercase().as_str() {
        "info" => ClientEventLevel::Info,
        "warn" | "warning" => ClientEventLevel::Warn,
        _ => ClientEventLevel::Error,
    }
}

fn normalize_client_event_stream(raw: &str, level: ClientEventLevel) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "usage" => "usage",
        "audit" => "audit",
        "incident" => "incident",
        "runtime" => "runtime",
        _ if matches!(level, ClientEventLevel::Info) => "usage",
        _ => "runtime",
    }
}

fn normalize_client_correlation(raw: &str, fallback: &str) -> String {
    let normalized = raw.trim();
    if normalized.is_empty() {
        fallback.trim().to_string()
    } else {
        normalized.chars().take(128).collect()
    }
}

fn normalize_client_optional_correlation(raw: &str, fallback: Option<&str>) -> Option<String> {
    let normalized = raw.trim();
    if !normalized.is_empty() {
        return Some(normalized.chars().take(128).collect());
    }
    fallback
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect())
}

fn client_event_target(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "api" | "frontend.api" => "client.telemetry.api",
        "action" | "frontend.action" => "client.telemetry.action",
        "runtime" | "frontend.runtime" => "client.telemetry.runtime",
        "console" | "frontend.console" => "client.telemetry.console",
        _ => "client.telemetry.other",
    }
}

fn build_client_event_message(payload: &ClientEventPayload) -> String {
    let message = normalize_client_text(&payload.message, "");
    if !message.is_empty() {
        return message;
    }
    normalize_client_text(&payload.detail, "客户端事件")
}

fn normalize_client_text(raw: &str, fallback: &str) -> String {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        value = fallback.to_string();
    }
    if value.len() > 512 {
        value.truncate(512);
        value.push_str("...");
    }
    redact_sensitive_fragments(&value)
}

fn sanitize_client_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let next = if is_sensitive_client_key(&lower) {
                        Value::String("[redacted]".to_string())
                    } else {
                        sanitize_client_value(value)
                    };
                    (key, next)
                })
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(sanitize_client_value).collect())
        }
        Value::String(text) => Value::String(normalize_client_text(&text, "")),
        other => other,
    }
}

fn is_sensitive_client_key(key: &str) -> bool {
    [
        "token",
        "authorization",
        "cookie",
        "password",
        "secret",
        "content",
        "text",
        "itemtitle",
        "payload",
        "rawcontent",
        "bodyhtml",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn redact_sensitive_fragments(input: &str) -> String {
    let mut text = input.trim().to_string();
    if text.is_empty() {
        return text;
    }
    for marker in [
        "Bearer ",
        "bearer ",
        "authorization: ",
        "cookie=",
        "Cookie: ",
        "token=",
    ] {
        if let Some(index) = text.find(marker) {
            let prefix_end = index + marker.len();
            let tail = &text[prefix_end..];
            let replacement_end = tail
                .find(|ch: char| ch.is_whitespace() || ch == ';' || ch == ',' || ch == '"')
                .map(|offset| prefix_end + offset)
                .unwrap_or(text.len());
            text.replace_range(prefix_end..replacement_end, "[redacted]");
        }
    }
    text
}

fn validate_runtime_diagnostic_level(raw: &str) -> Result<&'static str, ResponseError> {
    match raw.trim().to_ascii_uppercase().as_str() {
        "TRACE" => Ok("TRACE"),
        "DEBUG" | "" => Ok("DEBUG"),
        "INFO" => Ok("INFO"),
        "WARN" | "WARNING" => Ok("WARN"),
        "ERROR" => Ok("ERROR"),
        _ => Err(ResponseError::bad_request(
            "level 仅支持 TRACE/DEBUG/INFO/WARN/ERROR",
        )),
    }
}

fn validate_runtime_diagnostic_sample_rate(raw: f64) -> Result<f64, ResponseError> {
    if !raw.is_finite() || !(0.0..=1.0).contains(&raw) {
        return Err(ResponseError::bad_request(
            "sampleRate 必须是 0 到 1 之间的数字",
        ));
    }
    Ok(raw)
}

fn validate_runtime_diagnostic_timestamp(
    raw: Option<f64>,
    field: &str,
) -> Result<Option<f64>, ResponseError> {
    match raw {
        Some(value) if !value.is_finite() || value <= 0.0 => Err(ResponseError::bad_request(
            format!("{field} 必须是正数时间戳"),
        )),
        other => Ok(other),
    }
}

fn emit_client_event_log(
    level: ClientEventLevel,
    logger: &'static str,
    stream: &str,
    module: &str,
    action: &str,
    route: &str,
    trace_id: &str,
    request_id: Option<&str>,
    username: &str,
    error_code: &str,
    message: &str,
    detail: &str,
    http_status: Option<u16>,
    retryable: bool,
    context_json: &str,
) {
    macro_rules! emit_with_target {
        ($target:literal) => {
            match level {
                ClientEventLevel::Info => info!(
                    target: $target,
                    stream = stream,
                    module = module,
                    action = action,
                    route = route,
                    trace_id = trace_id,
                    request_id = request_id.unwrap_or_default(),
                    username = username,
                    error_code = error_code,
                    http_status = http_status.unwrap_or_default(),
                    retryable = retryable,
                    detail = detail,
                    context_json = context_json,
                    "{message}"
                ),
                ClientEventLevel::Warn => warn!(
                    target: $target,
                    stream = stream,
                    module = module,
                    action = action,
                    route = route,
                    trace_id = trace_id,
                    request_id = request_id.unwrap_or_default(),
                    username = username,
                    error_code = error_code,
                    http_status = http_status.unwrap_or_default(),
                    retryable = retryable,
                    detail = detail,
                    context_json = context_json,
                    "{message}"
                ),
                ClientEventLevel::Error => error!(
                    target: $target,
                    stream = stream,
                    module = module,
                    action = action,
                    route = route,
                    trace_id = trace_id,
                    request_id = request_id.unwrap_or_default(),
                    username = username,
                    error_code = error_code,
                    http_status = http_status.unwrap_or_default(),
                    retryable = retryable,
                    detail = detail,
                    context_json = context_json,
                    "{message}"
                ),
            }
        };
    }

    match logger {
        "client.telemetry.api" => emit_with_target!("client.telemetry.api"),
        "client.telemetry.action" => emit_with_target!("client.telemetry.action"),
        "client.telemetry.console" => emit_with_target!("client.telemetry.console"),
        "client.telemetry.runtime" => emit_with_target!("client.telemetry.runtime"),
        _ => emit_with_target!("client.telemetry.other"),
    }
}

#[cfg(test)]
mod tests {
    use super::{client_event_target, sanitize_client_value, ClientEventPayload};
    use serde_json::json;

    #[test]
    fn sanitize_client_value_redacts_sensitive_keys_and_fragments() {
        let value = json!({
            "token": "abc",
            "nested": {
                "authorization": "Bearer secret-token",
                "safe": "ok"
            },
            "message": "Bearer very-secret"
        });

        let cleaned = sanitize_client_value(value);
        assert_eq!(cleaned["token"], "[redacted]");
        assert_eq!(cleaned["nested"]["authorization"], "[redacted]");
        assert_eq!(cleaned["nested"]["safe"], "ok");
        assert_eq!(cleaned["message"], "Bearer [redacted]");
    }

    #[test]
    fn client_event_target_maps_known_frontend_categories() {
        assert_eq!(client_event_target("frontend.api"), "client.telemetry.api");
        assert_eq!(client_event_target("console"), "client.telemetry.console");
        assert_eq!(client_event_target("unknown"), "client.telemetry.other");
    }

    #[test]
    fn client_event_payload_defaults_empty_context() {
        let payload: ClientEventPayload = serde_json::from_value(json!({
            "message": "boom"
        }))
        .unwrap();

        assert_eq!(payload.context, json!({}));
    }
}
