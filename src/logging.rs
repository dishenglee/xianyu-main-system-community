use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{layer::Context, registry::LookupSpan, Layer};

const LOG_BUFFER_CAPACITY: usize = 2000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub sequence: u64,
    pub timestamp: f64,
    pub level: String,
    pub logger: String,
    pub message: String,
    pub fields_json: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MemoryLogLayer;

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Map<String, Value>,
}

pub fn memory_log_layer() -> MemoryLogLayer {
    MemoryLogLayer
}

pub fn default_env_filter() -> EnvFilter {
    let mut env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    for directive in [
        "client.telemetry.api=info",
        "client.telemetry.action=info",
        "client.telemetry.console=info",
        "client.telemetry.runtime=info",
        "client.telemetry.other=info",
    ] {
        if let Ok(parsed) = directive.parse() {
            env_filter = env_filter.add_directive(parsed);
        }
    }
    env_filter
}

pub(crate) fn env_flag_enabled(key: &str) -> bool {
    env_flag_value_enabled(std::env::var(key).ok().as_deref())
}

pub(crate) fn env_flag_value_enabled(value: Option<&str>) -> bool {
    matches!(
        value.map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on" | "debug")
    )
}

pub fn query_logs(
    level: Option<&str>,
    keyword: Option<&str>,
    logger_name: Option<&str>,
    limit: usize,
    since: f64,
) -> Vec<LogEntry> {
    let normalized_level = level.unwrap_or_default().trim().to_uppercase();
    let normalized_keyword = keyword.unwrap_or_default().trim().to_lowercase();
    let normalized_logger = logger_name.unwrap_or_default().trim().to_lowercase();
    let buffer = log_buffer().lock().unwrap();
    let mut results = Vec::new();

    for entry in buffer.iter().rev() {
        if since > 0.0 && entry.timestamp <= since {
            break;
        }
        if !matches_log_filters(
            entry,
            &normalized_level,
            &normalized_keyword,
            &normalized_logger,
        ) {
            continue;
        }
        results.push(entry.clone());
        if results.len() >= limit {
            break;
        }
    }

    results.reverse();
    results
}

pub fn clear_logs() {
    log_buffer().lock().unwrap().clear();
}

pub fn recent_logs(limit: usize) -> Vec<LogEntry> {
    query_logs(None, None, None, limit.max(1), 0.0)
}

pub fn latest_sequence() -> u64 {
    log_buffer()
        .lock()
        .unwrap()
        .back()
        .map(|entry| entry.sequence)
        .unwrap_or(0)
}

pub fn query_logs_after_sequence(
    after_sequence: u64,
    level: Option<&str>,
    keyword: Option<&str>,
    logger_name: Option<&str>,
    limit: usize,
) -> Vec<LogEntry> {
    let normalized_level = level.unwrap_or_default().trim().to_uppercase();
    let normalized_keyword = keyword.unwrap_or_default().trim().to_lowercase();
    let normalized_logger = logger_name.unwrap_or_default().trim().to_lowercase();
    let buffer = log_buffer().lock().unwrap();
    let mut results = Vec::new();

    for entry in buffer.iter() {
        if entry.sequence <= after_sequence {
            continue;
        }
        if !matches_log_filters(
            entry,
            &normalized_level,
            &normalized_keyword,
            &normalized_logger,
        ) {
            continue;
        }
        results.push(entry.clone());
        if results.len() >= limit.max(1) {
            break;
        }
    }

    results
}

#[cfg(test)]
pub fn push_log_for_test(level: &str, logger: &str, message: &str) {
    push_log(LogEntry {
        sequence: next_sequence(),
        timestamp: now_seconds(),
        level: level.to_string(),
        logger: logger.to_string(),
        message: message.to_string(),
        fields_json: Value::Object(Map::new()),
        stream: None,
        trace_id: None,
        request_id: None,
        incident_id: None,
        username: None,
        device_id: None,
        account_uid: None,
        session_id: None,
        item_id: None,
        job_id: None,
        module: None,
        action: None,
        outcome: None,
        error_code: None,
        retryable: None,
        duration_ms: None,
    });
}

impl<S> Layer<S> for MemoryLogLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let fields = visitor.fields;

        let message = visitor
            .message
            .or_else(|| {
                (!fields.is_empty()).then(|| {
                    fields
                        .iter()
                        .map(|(key, value)| format!("{key}={}", render_field_value(value)))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
            })
            .unwrap_or_else(|| metadata.name().to_string());

        push_log(LogEntry {
            sequence: next_sequence(),
            timestamp: now_seconds(),
            level: metadata.level().as_str().to_string(),
            logger: metadata.target().to_string(),
            message,
            stream: extract_string_field(&fields, "stream"),
            trace_id: extract_string_field(&fields, "trace_id"),
            request_id: extract_string_field(&fields, "request_id"),
            incident_id: extract_string_field(&fields, "incident_id"),
            username: extract_string_field(&fields, "username"),
            device_id: extract_string_field(&fields, "device_id"),
            account_uid: extract_string_field(&fields, "account_uid"),
            session_id: extract_string_field(&fields, "session_id"),
            item_id: extract_string_field(&fields, "item_id"),
            job_id: extract_string_field(&fields, "job_id"),
            module: extract_string_field(&fields, "module"),
            action: extract_string_field(&fields, "action"),
            outcome: extract_string_field(&fields, "outcome"),
            error_code: extract_string_field(&fields, "error_code"),
            retryable: extract_bool_field(&fields, "retryable"),
            duration_ms: extract_u64_field(&fields, "duration_ms"),
            fields_json: Value::Object(fields),
        });
    }
}

impl tracing::field::Visit for EventVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(rendered.trim_matches('"').to_string());
        } else {
            self.store_field(
                field.name(),
                Value::String(rendered.trim_matches('"').to_string()),
            );
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.store_field(field.name(), Value::String(value.to_string()));
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.store_field(field.name(), Value::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.store_field(field.name(), Value::from(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.store_field(field.name(), Value::from(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.store_field(field.name(), Value::from(value));
    }
}

impl EventVisitor {
    fn store_field(&mut self, name: &str, value: Value) {
        self.fields.insert(name.to_string(), value);
    }
}

fn extract_string_field(fields: &Map<String, Value>, name: &str) -> Option<String> {
    fields.get(name).and_then(|value| match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    })
}

fn extract_bool_field(fields: &Map<String, Value>, name: &str) -> Option<bool> {
    fields.get(name).and_then(|value| match value {
        Value::Bool(value) => Some(*value),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    })
}

fn extract_u64_field(fields: &Map<String, Value>, name: &str) -> Option<u64> {
    fields.get(name).and_then(|value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    })
}

fn push_log(entry: LogEntry) {
    let mut buffer = log_buffer().lock().unwrap();
    if buffer.len() == LOG_BUFFER_CAPACITY {
        buffer.pop_front();
    }
    buffer.push_back(entry);
}

fn next_sequence() -> u64 {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    SEQUENCE.fetch_add(1, Ordering::Relaxed)
}

fn log_buffer() -> &'static Arc<Mutex<VecDeque<LogEntry>>> {
    static BUFFER: OnceLock<Arc<Mutex<VecDeque<LogEntry>>>> = OnceLock::new();
    BUFFER.get_or_init(|| Arc::new(Mutex::new(VecDeque::with_capacity(LOG_BUFFER_CAPACITY))))
}

pub(crate) fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn render_field_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn matches_log_filters(
    entry: &LogEntry,
    normalized_level: &str,
    normalized_keyword: &str,
    normalized_logger: &str,
) -> bool {
    if !normalized_level.is_empty() && entry.level != normalized_level {
        return false;
    }
    if !normalized_keyword.is_empty() {
        let fields_text = serde_json::to_string(&entry.fields_json).unwrap_or_default();
        if !entry.message.to_lowercase().contains(normalized_keyword)
            && !fields_text.to_lowercase().contains(normalized_keyword)
        {
            return false;
        }
    }
    if !normalized_logger.is_empty() && !entry.logger.to_lowercase().contains(normalized_logger) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn log_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn env_flag_value_enabled_defaults_to_false() {
        assert!(!env_flag_value_enabled(None));
    }

    #[test]
    fn env_flag_value_enabled_accepts_common_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on", "debug"] {
            assert!(
                env_flag_value_enabled(Some(value)),
                "{value} should enable flag"
            );
        }
    }

    #[test]
    fn env_flag_value_enabled_rejects_falsey_and_unknown_values() {
        for value in ["", "0", "false", "off", "no", "maybe"] {
            assert!(
                !env_flag_value_enabled(Some(value)),
                "{value} should not enable flag"
            );
        }
    }

    #[test]
    fn query_logs_matches_keyword_in_fields_json() {
        let _guard = log_test_lock().lock().unwrap();
        clear_logs();
        push_log(LogEntry {
            sequence: next_sequence(),
            timestamp: now_seconds(),
            level: "WARN".to_string(),
            logger: "incident".to_string(),
            message: "heartbeat failed".to_string(),
            fields_json: serde_json::json!({
                "errorCode": "heartbeat_failed",
                "attempts": 3,
            }),
            stream: None,
            trace_id: None,
            request_id: None,
            incident_id: Some("inc_1".to_string()),
            username: None,
            device_id: None,
            account_uid: None,
            session_id: None,
            item_id: None,
            job_id: None,
            module: None,
            action: None,
            outcome: None,
            error_code: None,
            retryable: None,
            duration_ms: None,
        });

        let entries = query_logs(None, Some("heartbeat_failed"), None, 10, 0.0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].incident_id.as_deref(), Some("inc_1"));
    }

    #[test]
    fn query_logs_after_sequence_returns_incremental_slice() {
        let _guard = log_test_lock().lock().unwrap();
        clear_logs();
        push_log(LogEntry {
            sequence: next_sequence(),
            timestamp: now_seconds(),
            level: "INFO".to_string(),
            logger: "boot".to_string(),
            message: "boot ok".to_string(),
            fields_json: serde_json::json!({}),
            stream: None,
            trace_id: None,
            request_id: None,
            incident_id: None,
            username: None,
            device_id: None,
            account_uid: None,
            session_id: None,
            item_id: None,
            job_id: None,
            module: None,
            action: None,
            outcome: None,
            error_code: None,
            retryable: None,
            duration_ms: None,
        });
        push_log(LogEntry {
            sequence: next_sequence(),
            timestamp: now_seconds(),
            level: "WARN".to_string(),
            logger: "live".to_string(),
            message: "sender missing".to_string(),
            fields_json: serde_json::json!({}),
            stream: None,
            trace_id: None,
            request_id: None,
            incident_id: Some("inc_2".to_string()),
            username: None,
            device_id: None,
            account_uid: None,
            session_id: None,
            item_id: None,
            job_id: None,
            module: None,
            action: None,
            outcome: None,
            error_code: None,
            retryable: None,
            duration_ms: None,
        });

        let first_seq = query_logs(None, None, None, 10, 0.0)[0].sequence;
        let entries = query_logs_after_sequence(first_seq, None, None, None, 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].message, "sender missing");
    }

    #[test]
    fn query_logs_matches_logger_case_insensitively() {
        let _guard = log_test_lock().lock().unwrap();
        clear_logs();
        push_log(LogEntry {
            sequence: next_sequence(),
            timestamp: now_seconds(),
            level: "INFO".to_string(),
            logger: "Live.Worker".to_string(),
            message: "connected".to_string(),
            fields_json: serde_json::json!({}),
            stream: None,
            trace_id: None,
            request_id: None,
            incident_id: None,
            username: None,
            device_id: None,
            account_uid: None,
            session_id: None,
            item_id: None,
            job_id: None,
            module: None,
            action: None,
            outcome: None,
            error_code: None,
            retryable: None,
            duration_ms: None,
        });

        let entries = query_logs(None, None, Some("live"), 10, 0.0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].logger, "Live.Worker");
    }
}
