use serde_json::{Map, Value};

use crate::models::notifications::{NotificationBinding, NotificationChannel};

pub const REDACTED_CONFIG_VALUE: &str = "********";

const SENSITIVE_CONFIG_KEYS: &[&str] = &[
    "access_token",
    "app_secret",
    "bot_token",
    "client_secret",
    "context_token",
    "encoding_aes_key",
    "encrypt_key",
    "password",
    "robot_url",
    "secret",
    "token",
    "verification_token",
];

const SENSITIVE_ERROR_MARKERS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "authorization",
    "cookie",
    "set-cookie",
    "app_secret",
    "client_secret",
    "key",
    "private",
    "sec-",
    "sensitive",
];

pub fn redact_channel(mut channel: NotificationChannel) -> NotificationChannel {
    channel.config = redact_config(&channel.config);
    channel
}

pub fn redact_binding(mut binding: NotificationBinding) -> NotificationBinding {
    binding.channel_config = redact_config(&binding.channel_config);
    binding
}

pub fn redact_config(config: &Map<String, Value>) -> Map<String, Value> {
    config
        .iter()
        .map(|(key, value)| {
            let next = if is_sensitive_config_key(key) && has_non_empty_value(value) {
                Value::String(REDACTED_CONFIG_VALUE.to_string())
            } else {
                value.clone()
            };
            (key.clone(), next)
        })
        .collect()
}

pub fn preserve_redacted_config_values(
    existing: &Map<String, Value>,
    incoming: Map<String, Value>,
) -> Map<String, Value> {
    incoming
        .into_iter()
        .map(|(key, value)| {
            if is_sensitive_config_key(&key) && is_redacted_config_value(&value) {
                return (
                    key.clone(),
                    existing.get(&key).cloned().unwrap_or(Value::Null),
                );
            }
            (key, value)
        })
        .filter(|(_, value)| !value.is_null())
        .collect()
}

pub fn safe_worker_error_message(message: &str) -> String {
    let text = message.trim();
    if is_safe_missing_config_message(text) {
        return text.to_string();
    }
    if is_safe_error_message(text) {
        return text.to_string();
    }
    for prefix in [
        "Feishu native transport unavailable",
        "Feishu native transport protocol error",
        "Feishu native transport API error",
        "Feishu native transport JSON error",
    ] {
        if text.starts_with(prefix) {
            return prefix.to_string();
        }
    }
    "notification worker error".to_string()
}

fn is_redacted_config_value(value: &Value) -> bool {
    value.as_str().map(str::trim) == Some(REDACTED_CONFIG_VALUE)
}

fn is_sensitive_config_key(key: &str) -> bool {
    let normalized = key.trim().to_ascii_lowercase();
    SENSITIVE_CONFIG_KEYS.contains(&normalized.as_str())
        || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
}

fn has_non_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        _ => true,
    }
}

fn is_safe_error_message(message: &str) -> bool {
    message.len() <= 120
        && !contains_sensitive_marker(message)
        && message.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, ' ' | '_' | '-' | '.' | ':' | '/' | '(' | ')')
        })
}

fn is_safe_missing_config_message(message: &str) -> bool {
    message.len() <= 120
        && message.starts_with("missing ")
        && message.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, ' ' | '_' | '-' | '.' | ':' | '/' | '(' | ')')
        })
}

fn contains_sensitive_marker(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    SENSITIVE_ERROR_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}
