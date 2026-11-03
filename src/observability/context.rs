use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};

use super::{EventStream, StructuredEventEnvelope};

pub const TRACE_ID_HEADER: &str = "x-trace-id";
pub const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestCorrelation {
    pub trace_id: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

impl RequestCorrelation {
    pub fn detached() -> Self {
        Self {
            trace_id: generate_correlation_id("trace"),
            request_id: None,
        }
    }

    pub fn from_http_headers(headers: &HeaderMap) -> Self {
        let trace_id = normalized_header_value(headers, TRACE_ID_HEADER)
            .unwrap_or_else(|| generate_correlation_id("trace"));
        let request_id = normalized_header_value(headers, REQUEST_ID_HEADER)
            .or_else(|| Some(generate_correlation_id("request")));

        Self {
            trace_id,
            request_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestContext {
    pub stream: EventStream,
    pub correlation: RequestCorrelation,
    pub incident_id: Option<String>,
    pub username: Option<String>,
    pub device_id: Option<String>,
    pub account_uid: Option<String>,
    pub session_id: Option<String>,
    pub item_id: Option<String>,
    pub job_id: Option<String>,
    pub module: Option<String>,
    pub action: Option<String>,
    pub outcome: Option<String>,
    pub error_code: Option<String>,
    pub retryable: Option<bool>,
    pub duration_ms: Option<u64>,
}

impl RequestContext {
    pub fn new(stream: EventStream, correlation: RequestCorrelation) -> Self {
        Self {
            stream,
            correlation,
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
        }
    }

    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        let username = username.into();
        if !username.trim().is_empty() {
            self.username = Some(username);
        }
        self
    }

    pub fn envelope(&self) -> StructuredEventEnvelope {
        StructuredEventEnvelope {
            stream: self.stream,
            trace_id: self.correlation.trace_id.clone(),
            request_id: self.correlation.request_id.clone(),
            incident_id: self.incident_id.clone(),
            username: self.username.clone(),
            device_id: self.device_id.clone(),
            account_uid: self.account_uid.clone(),
            session_id: self.session_id.clone(),
            item_id: self.item_id.clone(),
            job_id: self.job_id.clone(),
            module: self.module.clone(),
            action: self.action.clone(),
            outcome: self.outcome.clone(),
            error_code: self.error_code.clone(),
            retryable: self.retryable,
            duration_ms: self.duration_ms,
        }
    }
}

fn normalized_header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            let mut value = value.to_string();
            if value.len() > 128 {
                value.truncate(128);
            }
            value
        })
}

fn generate_correlation_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(1);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);

    format!("{prefix}-{timestamp:x}-{sequence:x}")
}
