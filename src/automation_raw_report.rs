use serde_json::{json, Value};

const MAX_TEXT_LEN: usize = 12_000;
const MAX_RAW_PAYLOAD_JSON_LEN: usize = 180_000;

#[derive(Debug, Clone)]
pub struct AutomationRawReport<'a> {
    pub schedule_id: &'a str,
    pub schedule_name: &'a str,
    pub action: &'a str,
    pub trigger_source: &'a str,
    pub owner_username: &'a str,
    pub uid: &'a str,
    pub account_nick: &'a str,
    pub target_type: &'a str,
    pub target_id: &'a str,
    pub target_title: &'a str,
    pub job_id: &'a str,
    pub job_item_id: Option<i64>,
    pub status: &'a str,
    pub error_code: &'a str,
    pub message: &'a str,
    pub params: Value,
    pub response: Value,
}

pub fn emit_automation_raw_report(report: AutomationRawReport<'_>) {
    let item_id = if report.target_type == "item" {
        report.target_id
    } else {
        ""
    };
    let payload = sanitize_raw_value(json!({
        "schedule": {
            "id": report.schedule_id,
            "name": report.schedule_name,
            "action": report.action,
            "triggerSource": report.trigger_source,
            "ownerUsername": report.owner_username,
        },
        "account": {
            "uid": report.uid,
            "nick": report.account_nick,
        },
        "target": {
            "type": report.target_type,
            "id": report.target_id,
            "title": report.target_title,
        },
        "job": {
            "id": report.job_id,
            "itemId": report.job_item_id,
        },
        "request": {
            "params": report.params,
        },
        "response": report.response,
    }));
    let raw_payload_json = serialize_limited_json(&payload);
    let outcome = normalize_status(report.status);

    tracing::info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "other-automation",
        action = report.action,
        outcome = outcome,
        error_code = report.error_code,
        username = report.owner_username,
        account_uid = report.uid,
        item_id = item_id,
        job_id = report.job_id,
        schedule_id = report.schedule_id,
        schedule_name = report.schedule_name,
        trigger_source = report.trigger_source,
        target_type = report.target_type,
        target_id = report.target_id,
        target_title = report.target_title,
        job_item_id = report.job_item_id.unwrap_or_default(),
        raw_payload_json = raw_payload_json.as_str(),
        "other automation raw execution report: {}", report.message
    );
}

pub fn sanitize_raw_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let next = if is_sensitive_key(&lower) {
                        Value::String("[redacted]".to_string())
                    } else {
                        sanitize_raw_value(value)
                    };
                    (key, next)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_raw_value).collect()),
        Value::String(text) => Value::String(sanitize_text(&text)),
        other => other,
    }
}

fn normalize_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "success" | "ok" => "success",
        "running" => "running",
        "queued" => "queued",
        "skipped" => "skipped",
        _ => "failed",
    }
}

fn serialize_limited_json(value: &Value) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_RAW_PAYLOAD_JSON_LEN {
        text.truncate(MAX_RAW_PAYLOAD_JSON_LEN);
        text.push_str("...");
    }
    text
}

fn sanitize_text(input: &str) -> String {
    let mut text = input.trim().to_string();
    if text.is_empty() {
        return text;
    }
    for marker in [
        "Bearer ",
        "bearer ",
        "Cookie: ",
        "cookie: ",
        "authorization: ",
        "Authorization: ",
        "cookie=",
        "Cookie=",
        "token=",
        "access_token=",
        "_m_h5_tk=",
        "_m_h5_tk_enc=",
        "cookie2=",
        "sgcookie=",
        "sid=",
    ] {
        redact_marker_values(&mut text, marker);
    }
    if text.len() > MAX_TEXT_LEN {
        text.truncate(MAX_TEXT_LEN);
        text.push_str("...");
    }
    text
}

fn redact_marker_values(text: &mut String, marker: &str) {
    let mut search_from = 0;
    while let Some(relative_index) = text[search_from..].find(marker) {
        let marker_start = search_from + relative_index;
        let value_start = marker_start + marker.len();
        let tail = &text[value_start..];
        let value_end = tail
            .find(|ch: char| {
                matches!(ch, '&' | ';' | ',' | '"' | '\'' | '<' | '>') || ch.is_whitespace()
            })
            .map(|offset| value_start + offset)
            .unwrap_or(text.len());
        if value_end > value_start {
            text.replace_range(value_start..value_end, "[redacted]");
            search_from = value_start + "[redacted]".len();
        } else {
            search_from = value_start;
        }
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let compact = key.replace(['_', '-'], "");
    [
        "cookie",
        "token",
        "authorization",
        "password",
        "secret",
        "signedtoken",
        "hmacsecret",
        "mh5tk",
        "cookie2",
        "sgcookie",
        "sid",
    ]
    .iter()
    .any(|needle| compact.contains(needle))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::sync::Once;
    use tracing_subscriber::prelude::*;

    use super::{emit_automation_raw_report, sanitize_raw_value, AutomationRawReport};

    fn init_logging() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::registry()
                .with(crate::logging::memory_log_layer())
                .try_init();
        });
    }

    #[test]
    fn sanitize_raw_value_preserves_business_payload_but_redacts_credentials() {
        let cleaned = sanitize_raw_value(json!({
            "request": {
                "api": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.edit"),
                "data": {
                    "itemId": "ITEM-1",
                    "price": "19.90",
                    "title": "原始商品标题",
                    "token": "secret-token",
                    "actionUrl": "https://h5.example.test/path?token=abc123&itemId=ITEM-1",
                    "cookieValue": "sid=raw-cookie; _m_h5_tk=tk123"
                },
                "headers": {
                    "authorization": "Bearer raw-auth",
                    "cookie": "sid=raw-cookie",
                    "x-trace": "trace-1"
                }
            },
            "response": {
                "ret": ["FAIL_SYS_TOKEN_EXOIRED::令牌过期"],
                "data": {
                    "message": "Cookie: raw-cookie failed",
                    "authText": "Authorization: Bearer raw-auth failed",
                    "itemId": "ITEM-1"
                }
            }
        }));

        assert_eq!(cleaned["request"]["data"]["itemId"], "ITEM-1");
        assert_eq!(cleaned["request"]["data"]["title"], "原始商品标题");
        assert_eq!(cleaned["request"]["data"]["token"], "[redacted]");
        assert_eq!(cleaned["request"]["data"]["cookieValue"], "[redacted]");
        assert_eq!(cleaned["request"]["headers"]["authorization"], "[redacted]");
        assert_eq!(cleaned["request"]["headers"]["cookie"], "[redacted]");
        assert_eq!(cleaned["request"]["headers"]["x-trace"], "trace-1");

        let serialized = serde_json::to_string(&cleaned).unwrap();
        assert!(serialized.contains("FAIL_SYS_TOKEN_EXOIRED"));
        assert!(!serialized.contains("raw-auth"));
        assert!(!serialized.contains("raw-cookie"));
        assert!(!serialized.contains("abc123"));
    }

    #[test]
    fn emit_automation_raw_report_records_uploadable_usage_log() {
        init_logging();
        let before_sequence = crate::logging::latest_sequence();

        emit_automation_raw_report(AutomationRawReport {
            schedule_id: "auto-1",
            schedule_name: "定时闲鱼币签到",
            action: "coin_signin",
            trigger_source: "schedule",
            owner_username: "owner-a",
            uid: "uid-1",
            account_nick: "测试账号",
            target_type: "account",
            target_id: "uid-1",
            target_title: "测试账号",
            job_id: "auto-1",
            job_item_id: None,
            status: "failed",
            error_code: "FAIL_SYS_TOKEN_EXOIRED",
            message: "令牌过期",
            params: json!({ "itemId": "ITEM-1", "token": "secret-token" }),
            response: json!({ "ret": ["FAIL_SYS_TOKEN_EXOIRED::令牌过期"], "cookie": "raw-cookie" }),
        });

        let logs = crate::logging::query_logs_after_sequence(
            before_sequence,
            None,
            None,
            Some("client.telemetry.action"),
            10,
        );
        let entry = logs
            .iter()
            .find(|entry| {
                entry.module.as_deref() == Some("other-automation")
                    && entry.job_id.as_deref() == Some("auto-1")
            })
            .expect("automation raw report log should be recorded");

        assert_eq!(entry.stream.as_deref(), Some("usage"));
        assert_eq!(entry.action.as_deref(), Some("coin_signin"));
        assert_eq!(entry.outcome.as_deref(), Some("failed"));
        assert_eq!(entry.account_uid.as_deref(), Some("uid-1"));
        assert_eq!(entry.job_id.as_deref(), Some("auto-1"));

        let raw = entry
            .fields_json
            .get("raw_payload_json")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        assert!(raw.contains("FAIL_SYS_TOKEN_EXOIRED"));
        assert!(raw.contains("ITEM-1"));
        assert!(!raw.contains("secret-token"));
        assert!(!raw.contains("raw-cookie"));
    }
}
