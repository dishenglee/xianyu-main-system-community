use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    control_plane_tls::verify_response_pin,
    license_runtime::{get_sign_license_context, ProofMode},
    logging,
    state::AppState,
};

const INCIDENT_BUFFER_CAPACITY: usize = 512;
const RECENT_LOG_TAIL_LIMIT: usize = 40;

static INCIDENT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static CONTROL_PLANE_UPLOAD_BLOCK_UNTIL: AtomicU64 = AtomicU64::new(0);
const DESKTOP_CONTROL_PLANE_LOG_BATCH_LIMIT: usize = 100;
const ANDROID_CONTROL_PLANE_LOG_BATCH_LIMIT: usize = 20;
const CONTROL_PLANE_AUTH_FAILURE_COOLDOWN_SECS: u64 = 600;
fn device_proof_header_key_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-key-id")
}
fn device_proof_header_public_key() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-public-key")
}
fn device_proof_header_signature() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-signature")
}
fn device_proof_header_timestamp() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-ts")
}
fn device_proof_header_nonce() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-nonce")
}
fn legacy_proof_header_license_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-id")
}
fn legacy_proof_header_device_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-device-id")
}
fn legacy_proof_header_ts() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-proof-ts")
}
fn legacy_proof_header_nonce() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-proof-nonce")
}
fn legacy_proof_header_sig() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-proof-signature")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub build_channel: String,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub os_version: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub hostname_hash: String,
    #[serde(default)]
    pub network_type: String,
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default)]
    pub proxy_type: String,
    #[serde(default)]
    pub public_ip_hash: String,
    #[serde(default)]
    pub isp_region_hint: String,
    #[serde(default)]
    pub control_plane_rtt_ms: u64,
    #[serde(default)]
    pub configured_account_count: i64,
    #[serde(default)]
    pub active_account_count: i64,
    #[serde(default)]
    pub online_live_account_count: i64,
    #[serde(default)]
    pub alive_live_sender_count: i64,
    #[serde(default)]
    pub total_live_sender_count: i64,
    #[serde(default)]
    pub closed_live_sender_count: i64,
    #[serde(default)]
    pub db_connected_missing_sender_count: i64,
    #[serde(default)]
    pub sender_missing_db_connected_count: i64,
    #[serde(default)]
    pub memory_usage_mb: u64,
    #[serde(default)]
    pub cpu_load: f64,
    #[serde(default)]
    pub disk_free_mb: u64,
    #[serde(default)]
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IncidentArtifactManifest {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub recent_log_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IncidentInput {
    #[serde(default = "default_error_level")]
    pub error_level: String,
    pub module: String,
    pub action: String,
    #[serde(default)]
    pub route: String,
    pub error_code: String,
    pub error_message: String,
    #[serde(default)]
    pub http_status: Option<u16>,
    #[serde(default)]
    pub upstream_ret: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default = "default_context_json")]
    pub context_json: Value,
    #[serde(default)]
    pub environment: Option<EnvironmentSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncidentReport {
    pub incident_id: String,
    pub occurred_at: f64,
    pub error_level: String,
    pub module: String,
    pub action: String,
    pub route: String,
    pub error_code: String,
    pub error_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub upstream_ret: String,
    pub retryable: bool,
    pub context_json: Value,
    pub environment: EnvironmentSnapshot,
    pub recent_logs: Vec<crate::logging::LogEntry>,
    pub artifact: IncidentArtifactManifest,
}

#[derive(Debug, Clone, Default)]
pub struct IncidentReporter {
    inner: Arc<Mutex<VecDeque<IncidentReport>>>,
}

#[derive(Debug, Clone)]
pub struct LogSyncWorker {
    state: AppState,
}

impl IncidentReporter {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn report_from_state(
        &self,
        state: &AppState,
        mut input: IncidentInput,
    ) -> IncidentReport {
        let environment = sanitize_environment(
            input
                .environment
                .take()
                .unwrap_or(state.collect_environment_snapshot().await),
        );
        let recent_logs = logging::recent_logs(RECENT_LOG_TAIL_LIMIT);
        let report = build_incident_report(input, environment, recent_logs);
        self.push(report.clone());
        tracing::error!(
            target: "incident",
            incident_id = %report.incident_id,
            module = %report.module,
            action = %report.action,
            error_code = %report.error_code,
            "incident recorded"
        );
        if let Err(error) = state.storage().enqueue_incident_upload(&report).await {
            tracing::warn!(
                incident_id = %report.incident_id,
                error = %error,
                "incident queue persistence failed"
            );
        }
        spawn_control_plane_upload(state.clone(), report.clone());
        report
    }

    pub async fn ingest_external(&self, state: &AppState, input: IncidentInput) -> IncidentReport {
        self.report_from_state(state, input).await
    }

    pub fn list(
        &self,
        limit: usize,
        since: f64,
        level: Option<&str>,
        module: Option<&str>,
        error_code: Option<&str>,
        incident_id: Option<&str>,
    ) -> Vec<IncidentReport> {
        let normalized_level = level.unwrap_or_default().trim().to_ascii_lowercase();
        let normalized_module = module.unwrap_or_default().trim().to_ascii_lowercase();
        let normalized_error_code = error_code.unwrap_or_default().trim().to_ascii_lowercase();
        let normalized_incident_id = incident_id.unwrap_or_default().trim().to_ascii_lowercase();
        let buffer = self.inner.lock().unwrap();
        let mut reports = Vec::new();

        for report in buffer.iter().rev() {
            if since > 0.0 && report.occurred_at <= since {
                break;
            }
            if !normalized_level.is_empty()
                && report.error_level.to_ascii_lowercase() != normalized_level
            {
                continue;
            }
            if !normalized_module.is_empty()
                && !report
                    .module
                    .to_ascii_lowercase()
                    .contains(&normalized_module)
            {
                continue;
            }
            if !normalized_error_code.is_empty()
                && !report
                    .error_code
                    .to_ascii_lowercase()
                    .contains(&normalized_error_code)
            {
                continue;
            }
            if !normalized_incident_id.is_empty()
                && !report
                    .incident_id
                    .to_ascii_lowercase()
                    .contains(&normalized_incident_id)
            {
                continue;
            }
            reports.push(report.clone());
            if reports.len() >= limit {
                break;
            }
        }

        reports.reverse();
        reports
    }

    pub fn get(&self, incident_id: &str) -> Option<IncidentReport> {
        self.inner
            .lock()
            .unwrap()
            .iter()
            .find(|report| report.incident_id == incident_id)
            .cloned()
    }

    fn push(&self, report: IncidentReport) {
        let mut buffer = self.inner.lock().unwrap();
        if buffer.len() == INCIDENT_BUFFER_CAPACITY {
            buffer.pop_front();
        }
        buffer.push_back(report);
    }
}

impl LogSyncWorker {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn sync_once(&self) -> Result<usize, String> {
        self.state
            .storage()
            .sync_runtime_log_buffer()
            .await
            .map_err(|error| format!("sync runtime log buffer: {error}"))?;

        let mut uploaded_count = 0;
        let mut first_error: Option<String> = None;

        if let Some(context) = get_sign_license_context() {
            for stream in ["runtime", "usage"] {
                match sync_control_plane_stream(&self.state, &context, stream).await {
                    Ok(count) => uploaded_count += count,
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }

            if let Err(error) = sync_incident_upload_queue(&self.state).await {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        if let Err(error) = self.state.storage().enforce_runtime_log_retention().await {
            if first_error.is_none() {
                first_error = Some(format!("enforce runtime log retention: {error}"));
            }
        }

        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(uploaded_count)
        }
    }
}

fn spawn_control_plane_upload(state: AppState, report: IncidentReport) {
    tokio::spawn(async move {
        match upload_queued_incident(&state, &report).await {
            Ok(true) | Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    incident_id = %report.incident_id,
                    error = %error,
                    "incident control-plane upload failed"
                );
            }
        }
    });
}

async fn sync_control_plane_stream(
    state: &AppState,
    context: &crate::license_runtime::SignLicenseContext,
    stream: &str,
) -> Result<usize, String> {
    if control_plane_upload_blocked() {
        return Ok(0);
    }
    let entries = state
        .storage()
        .next_control_plane_log_batch(stream, control_plane_log_batch_limit())
        .await
        .map_err(|error| format!("load {stream} control-plane log batch: {error}"))?;
    if entries.is_empty() {
        return Ok(0);
    }

    let last_sequence = entries
        .last()
        .map(|entry| entry.sequence)
        .unwrap_or_default();
    match upload_logs_to_control_plane(context, &entries).await {
        Ok(()) => {
            state
                .storage()
                .mark_control_plane_logs_uploaded(stream, last_sequence)
                .await
                .map_err(|error| format!("mark {stream} control-plane logs uploaded: {error}"))?;
            Ok(entries.len())
        }
        Err(error) => {
            remember_control_plane_upload_failure(&error);
            state
                .storage()
                .mark_control_plane_logs_failed(stream, last_sequence, &error)
                .await
                .map_err(|storage_error| {
                    format!(
                        "mark {stream} control-plane logs failed after upload error ({error}): {storage_error}"
                    )
                })?;
            Err(error)
        }
    }
}

async fn sync_incident_upload_queue(state: &AppState) -> Result<usize, String> {
    let reports = state
        .storage()
        .next_incident_upload_batch(control_plane_log_batch_limit())
        .await
        .map_err(|error| format!("load incident upload queue: {error}"))?;
    let mut uploaded_count = 0;

    for report in reports {
        match upload_queued_incident(state, &report).await {
            Ok(true) => uploaded_count += 1,
            Ok(false) => break,
            Err(error) => {
                tracing::warn!(
                    incident_id = %report.incident_id,
                    error = %error,
                    "incident retry upload failed"
                );
            }
        }
    }

    Ok(uploaded_count)
}

async fn upload_queued_incident(state: &AppState, report: &IncidentReport) -> Result<bool, String> {
    let Some(context) = get_sign_license_context() else {
        return Ok(false);
    };
    if control_plane_upload_blocked() {
        return Ok(false);
    }

    state
        .storage()
        .mark_incident_upload_started(&report.incident_id)
        .await
        .map_err(|error| format!("mark incident upload started: {error}"))?;

    match upload_report_to_control_plane(report.clone(), context, control_plane_pins()).await {
        Ok(()) => {
            state
                .storage()
                .mark_incident_upload_result(&report.incident_id, true, report.retryable, None)
                .await
                .map_err(|error| format!("mark incident upload success: {error}"))?;
            Ok(true)
        }
        Err(error) => {
            remember_control_plane_upload_failure(&error);
            let should_retry = report.retryable && !is_control_plane_auth_rejection(&error);
            state
                .storage()
                .mark_incident_upload_result(&report.incident_id, false, should_retry, Some(&error))
                .await
                .map_err(|storage_error| {
                    format!(
                        "mark incident upload failure after upload error ({error}): {storage_error}"
                    )
                })?;
            Err(error)
        }
    }
}

fn control_plane_upload_blocked() -> bool {
    let until = CONTROL_PLANE_UPLOAD_BLOCK_UNTIL.load(Ordering::Relaxed);
    until > crate::control_plane_auth::unix_now_secs() as u64
}

fn remember_control_plane_upload_failure(error: &str) {
    if !is_control_plane_auth_rejection(error) {
        return;
    }
    let until = crate::control_plane_auth::unix_now_secs() as u64
        + CONTROL_PLANE_AUTH_FAILURE_COOLDOWN_SECS;
    CONTROL_PLANE_UPLOAD_BLOCK_UNTIL.store(until, Ordering::Relaxed);
}

fn is_control_plane_auth_rejection(error: &str) -> bool {
    let normalized = error.to_ascii_lowercase();
    normalized.contains("401")
        && (normalized.contains("hmac mismatch")
            || normalized.contains("proof-invalid")
            || normalized.contains("proof invalid")
            || normalized.contains("unauthorized"))
}

fn control_plane_pins() -> Vec<String> {
    std::env::var("XIANYU_CONTROL_PLANE_CERT_PINS")
        .ok()
        .map(|value| crate::control_plane_tls::parse_pin_set(&value))
        .unwrap_or_default()
}

fn control_plane_log_batch_limit() -> usize {
    if cfg!(target_os = "android") {
        ANDROID_CONTROL_PLANE_LOG_BATCH_LIMIT
    } else {
        DESKTOP_CONTROL_PLANE_LOG_BATCH_LIMIT
    }
}

fn apply_control_plane_proof_headers(
    builder: reqwest::RequestBuilder,
    proof_mode: &ProofMode,
    context: &crate::license_runtime::SignLicenseContext,
) -> reqwest::RequestBuilder {
    let builder = crate::license_runtime::apply_client_version_headers(builder);
    match proof_mode {
        ProofMode::Device(proof) => builder
            .header(device_proof_header_key_id(), proof.license_id.to_string())
            .header(device_proof_header_public_key(), &proof.public_key_hex)
            .header(device_proof_header_signature(), &proof.signature_hex)
            .header(device_proof_header_timestamp(), proof.timestamp.to_string())
            .header(device_proof_header_nonce(), &proof.nonce),
        ProofMode::LegacyLicenseHmac {
            timestamp,
            nonce,
            signature_hex,
        } => builder
            .header(
                legacy_proof_header_license_id(),
                context.license_id.to_string(),
            )
            .header(legacy_proof_header_device_id(), &context.device_id)
            .header(legacy_proof_header_ts(), timestamp.to_string())
            .header(legacy_proof_header_nonce(), nonce)
            .header(legacy_proof_header_sig(), signature_hex),
    }
}

async fn upload_report_to_control_plane(
    report: IncidentReport,
    context: crate::license_runtime::SignLicenseContext,
    pins: Vec<String>,
) -> Result<(), String> {
    let client = Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .tls_info(true)
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|error| format!("build client: {error}"))?;

    let payload = build_control_plane_payload(&report, &context)?;
    let report_value =
        serde_json::to_value(&payload).map_err(|error| format!("serialize report: {error}"))?;
    let timestamp = crate::control_plane_auth::unix_now_secs() as u64;
    let request_body =
        build_control_plane_upload_body(&context, "report", report_value, timestamp)?;
    let body_bytes = serde_json::to_vec(&request_body)
        .map_err(|error| format!("serialize report body: {error}"))?;
    let app_data_dir = std::env::var("APP_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map_err(|_| "APP_DATA_DIR missing for device-proof incident upload".to_string())?;
    let proof_mode = crate::license_runtime::select_runtime_proof_mode(
        &client,
        &context.server_url,
        &context,
        &app_data_dir,
        "POST",
        crate::sealed_protocol::sealed_str!("/api/nodes/incidents"),
        &body_bytes,
    )
    .await?;

    let response = apply_control_plane_proof_headers(
        client
            .post(format!(
                "{}{}",
                context.server_url.trim_end_matches('/'),
                crate::sealed_protocol::sealed_str!("/api/nodes/incidents")
            ))
            .json(&request_body),
        &proof_mode,
        &context,
    )
    .send()
    .await
    .map_err(|error| format!("send request: {error}"))?;

    verify_response_pin(
        &response,
        &pins,
        "license_service",
        &format!(
            "{}{}",
            context.server_url.trim_end_matches('/'),
            crate::sealed_protocol::sealed_str!("/api/nodes/incidents")
        ),
    )?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("server returned {status}: {body}"));
    }

    Ok(())
}

async fn upload_logs_to_control_plane(
    context: &crate::license_runtime::SignLicenseContext,
    entries: &[crate::logging::LogEntry],
) -> Result<(), String> {
    let client = Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .tls_info(true)
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| format!("build client: {error}"))?;

    let logs = entries
        .iter()
        .map(|entry| {
            json!({
                "log_id": control_plane_log_id(context, entry),
                "license_id": context.license_id,
                "node_id": node_id_for_device(&context.device_id),
                "device_id": context.device_id,
                "stream": entry.stream.as_deref().unwrap_or("runtime"),
                "level": entry.level,
                "logger": entry.logger,
                "message": entry.message,
                "fields": entry.fields_json,
                "trace_id": entry.trace_id,
                "request_id": entry.request_id,
                "incident_id": entry.incident_id,
                "username": entry.username,
                "event_device_id": entry.device_id,
                "account_uid": entry.account_uid,
                "session_id": entry.session_id,
                "item_id": entry.item_id,
                "job_id": entry.job_id,
                "module": entry.module,
                "action": entry.action,
                "outcome": entry.outcome,
                "error_code": entry.error_code,
                "retryable": entry.retryable,
                "duration_ms": entry.duration_ms,
                "client_timestamp": chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
                    (entry.timestamp * 1000.0) as i64,
                )
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339(),
            })
        })
        .collect::<Vec<_>>();
    let timestamp = crate::control_plane_auth::unix_now_secs() as u64;
    let request_body =
        build_control_plane_upload_body(context, "logs", Value::Array(logs), timestamp)?;
    let body_bytes =
        serde_json::to_vec(&request_body).map_err(|error| format!("serialize logs: {error}"))?;
    let app_data_dir = std::env::var("APP_DATA_DIR")
        .map(std::path::PathBuf::from)
        .map_err(|_| "APP_DATA_DIR missing for device-proof log upload".to_string())?;
    let proof_mode = crate::license_runtime::select_runtime_proof_mode(
        &client,
        &context.server_url,
        context,
        &app_data_dir,
        "POST",
        crate::sealed_protocol::sealed_str!("/api/nodes/logs"),
        &body_bytes,
    )
    .await?;

    let response = apply_control_plane_proof_headers(
        client
            .post(format!(
                "{}{}",
                context.server_url.trim_end_matches('/'),
                crate::sealed_protocol::sealed_str!("/api/nodes/logs")
            ))
            .json(&request_body),
        &proof_mode,
        context,
    )
    .send()
    .await
    .map_err(|error| format!("send request: {error}"))?;

    let pins = std::env::var("XIANYU_CONTROL_PLANE_CERT_PINS")
        .ok()
        .map(|value| crate::control_plane_tls::parse_pin_set(&value))
        .unwrap_or_default();
    verify_response_pin(
        &response,
        &pins,
        "license_service",
        &format!(
            "{}{}",
            context.server_url.trim_end_matches('/'),
            crate::sealed_protocol::sealed_str!("/api/nodes/logs")
        ),
    )?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("server returned {status}: {body}"));
    }

    Ok(())
}

fn control_plane_log_id(
    context: &crate::license_runtime::SignLicenseContext,
    entry: &crate::logging::LogEntry,
) -> String {
    let timestamp_millis = (entry.timestamp * 1000.0).round() as i64;
    let fields_json =
        serde_json::to_string(&entry.fields_json).unwrap_or_else(|_| "{}".to_string());
    let fingerprint = format!(
        "license_id={}\ndevice_id={}\ntimestamp_ms={}\nsequence={}\nlevel={}\nlogger={}\nmessage={}\nincident_id={}\nfields={}",
        context.license_id,
        context.device_id.trim(),
        timestamp_millis,
        entry.sequence,
        entry.level.trim(),
        entry.logger.trim(),
        entry.message.trim(),
        entry.incident_id.as_deref().unwrap_or("").trim(),
        fields_json
    );

    format!("clog-{}", sha256_hex(&fingerprint))
}

fn build_control_plane_upload_body(
    context: &crate::license_runtime::SignLicenseContext,
    payload_key: &'static str,
    payload_value: Value,
    timestamp: u64,
) -> Result<Value, String> {
    let payload_json = serde_json::to_string(&payload_value)
        .map_err(|error| format!("serialize {payload_key} json: {error}"))?;
    let mac = crate::control_plane_auth::sign_message_hex(
        &context.hmac_secret_hex,
        &format!(
            "{}.{}.{}.{}",
            context.license_id, context.device_id, timestamp, payload_json
        ),
    )?;

    let mut body = serde_json::Map::new();
    body.insert("license_id".to_string(), json!(context.license_id));
    body.insert("device_id".to_string(), json!(context.device_id));
    body.insert("ts".to_string(), json!(timestamp));
    body.insert("mac".to_string(), json!(mac));
    body.insert(payload_key.to_string(), payload_value);
    Ok(Value::Object(body))
}

fn build_control_plane_payload(
    report: &IncidentReport,
    context: &crate::license_runtime::SignLicenseContext,
) -> Result<Value, String> {
    let occurred_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(
        (report.occurred_at * 1000.0) as i64,
    )
    .unwrap_or_else(chrono::Utc::now)
    .to_rfc3339();
    let size_bytes = serde_json::to_vec(&report.recent_logs)
        .map(|bytes| bytes.len() as i64)
        .unwrap_or(0);
    Ok(json!({
        "incident_id": report.incident_id,
        "license_id": context.license_id,
        "node_id": node_id_for_device(&context.device_id),
        "device_id": context.device_id,
        "module": report.module,
        "action": report.action,
        "route": report.route,
        "error_level": report.error_level,
        "error_code": report.error_code,
        "error_message": report.error_message,
        "http_status": report.http_status.map(i64::from),
        "upstream_ret": report.upstream_ret,
        "retryable": report.retryable,
        "app_version": report.environment.app_version,
        "build_channel": report.environment.build_channel,
        "os": report.environment.os,
        "os_version": report.environment.os_version,
        "arch": report.environment.arch,
        "hostname_hash": report.environment.hostname_hash,
        "network_type": report.environment.network_type,
        "proxy_enabled": report.environment.proxy_enabled,
        "proxy_type": report.environment.proxy_type,
        "public_ip_hash": report.environment.public_ip_hash,
        "isp_region_hint": report.environment.isp_region_hint,
        "configured_account_count": report.environment.configured_account_count,
        "active_account_count": report.environment.active_account_count,
        "online_live_account_count": report.environment.online_live_account_count,
        "alive_live_sender_count": report.environment.alive_live_sender_count,
        "total_live_sender_count": report.environment.total_live_sender_count,
        "context": {
            "context": report.context_json,
            "recentLogs": report.recent_logs,
            "artifact": report.artifact,
        },
        "occurred_at": occurred_at,
        "artifacts": [{
            "artifact_kind": "recent_logs",
            "storage_path": format!(
                "local-worker/{}/incidents/{}/recent_logs.json",
                node_id_for_device(&context.device_id),
                report.incident_id
            ),
            "content_type": "application/json",
            "size_bytes": size_bytes,
        }],
    }))
}

fn build_incident_report(
    input: IncidentInput,
    environment: EnvironmentSnapshot,
    recent_logs: Vec<crate::logging::LogEntry>,
) -> IncidentReport {
    let occurred_at = logging::now_seconds();
    let incident_id = format!(
        "inc_{:013}_{}",
        (occurred_at * 1000.0) as u64,
        INCIDENT_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let error_level = normalize_error_level(&input.error_level);
    IncidentReport {
        incident_id,
        occurred_at,
        error_level,
        module: input.module.trim().to_string(),
        action: input.action.trim().to_string(),
        route: input.route.trim().to_string(),
        error_code: input.error_code.trim().to_string(),
        error_message: sanitize_text(&input.error_message),
        http_status: input.http_status,
        upstream_ret: sanitize_text(&input.upstream_ret),
        retryable: input.retryable,
        context_json: sanitize_json(input.context_json),
        environment,
        artifact: IncidentArtifactManifest {
            source: "local-worker".to_string(),
            recent_log_count: recent_logs.len(),
        },
        recent_logs,
    }
}

fn normalize_error_level(level: &str) -> String {
    let normalized = level.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return default_error_level();
    }
    normalized
}

fn default_error_level() -> String {
    "error".to_string()
}

fn default_context_json() -> Value {
    json!({})
}

fn sanitize_environment(mut snapshot: EnvironmentSnapshot) -> EnvironmentSnapshot {
    snapshot.hostname_hash = sanitize_hashish(&snapshot.hostname_hash);
    snapshot.public_ip_hash = sanitize_hashish(&snapshot.public_ip_hash);
    snapshot.proxy_type = sanitize_text(&snapshot.proxy_type);
    snapshot.isp_region_hint = sanitize_text(&snapshot.isp_region_hint);
    snapshot.network_type = sanitize_text(&snapshot.network_type);
    snapshot
}

fn sanitize_hashish(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.len() == 64 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return trimmed.to_ascii_lowercase();
    }
    sha256_hex(trimmed)
}

fn sanitize_json(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let cleaned = map
                .into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let next = if is_sensitive_key(&lower) {
                        Value::String("[redacted]".to_string())
                    } else {
                        sanitize_json(value)
                    };
                    (key, next)
                })
                .collect();
            Value::Object(cleaned)
        }
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_json).collect()),
        Value::String(text) => Value::String(sanitize_text(&text)),
        other => other,
    }
}

fn sanitize_text(input: &str) -> String {
    let mut text = input.trim().to_string();
    if text.is_empty() {
        return text;
    }
    for marker in [
        "Bearer ",
        "bearer ",
        "cookie=",
        "Cookie: ",
        "authorization: ",
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
    if text.len() > 2048 {
        text.truncate(2048);
        text.push_str("…");
    }
    text
}

fn is_sensitive_key(key: &str) -> bool {
    [
        "cookie",
        "token",
        "authorization",
        "password",
        "secret",
        "signed_token",
        "hmac_secret",
        "body_html",
        "raw_content",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn sha256_hex(input: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(input.as_bytes());
    hex::encode(digest.finalize())
}

fn node_id_for_device(device_id: &str) -> String {
    let digest = Sha256::digest(format!("local-worker:{device_id}").as_bytes());
    format!("node-{}", hex::encode(digest)[..24].to_string())
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn hmac_hex(secret_hex: &str, message: &[u8]) -> Result<String, String> {
    use hmac::{Hmac, Mac};

    type HmacSha256 = Hmac<Sha256>;

    let secret = hex::decode(secret_hex).map_err(|error| format!("hex decode: {error}"))?;
    let mut mac =
        HmacSha256::new_from_slice(&secret).map_err(|error| format!("hmac init: {error}"))?;
    mac.update(message);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_sign_context() -> crate::license_runtime::SignLicenseContext {
        crate::license_runtime::SignLicenseContext {
            server_url: "https://license.example.test".to_string(),
            auth_token: "auth-token".to_string(),
            signed_token: "signed-token".to_string(),
            license_id: 42,
            username: "alice".to_string(),
            sku: "trial".to_string(),
            account_limit: 2,
            hmac_secret_hex: "11".repeat(32),
            device_id: "device-001".to_string(),
        }
    }

    #[test]
    fn sanitize_json_redacts_sensitive_keys() {
        let value = json!({
            "authToken": "abc",
            "cookieJar": "raw-cookie",
            "nested": {
                "password": "secret",
                "safe": "ok"
            }
        });

        let cleaned = sanitize_json(value);
        assert_eq!(cleaned["authToken"], "[redacted]");
        assert_eq!(cleaned["cookieJar"], "[redacted]");
        assert_eq!(cleaned["nested"]["password"], "[redacted]");
        assert_eq!(cleaned["nested"]["safe"], "ok");
    }

    #[test]
    fn incident_report_list_supports_incident_id_filter() {
        let reporter = IncidentReporter::new();
        reporter.push(IncidentReport {
            incident_id: "inc_alpha".to_string(),
            occurred_at: 1.0,
            error_level: "error".to_string(),
            module: "live".to_string(),
            action: "send".to_string(),
            route: crate::sealed_protocol::sealed_str!("/api/live").to_string(),
            error_code: "send_failed".to_string(),
            error_message: "failed".to_string(),
            http_status: Some(503),
            upstream_ret: String::new(),
            retryable: true,
            context_json: json!({}),
            environment: EnvironmentSnapshot::default(),
            recent_logs: Vec::new(),
            artifact: IncidentArtifactManifest {
                source: "local-worker".to_string(),
                recent_log_count: 0,
            },
        });
        reporter.push(IncidentReport {
            incident_id: "inc_beta".to_string(),
            occurred_at: 2.0,
            error_level: "warn".to_string(),
            module: "market".to_string(),
            action: "sync".to_string(),
            route: crate::sealed_protocol::sealed_str!("/api/market").to_string(),
            error_code: "timeout".to_string(),
            error_message: "timeout".to_string(),
            http_status: None,
            upstream_ret: String::new(),
            retryable: false,
            context_json: json!({}),
            environment: EnvironmentSnapshot::default(),
            recent_logs: Vec::new(),
            artifact: IncidentArtifactManifest {
                source: "local-worker".to_string(),
                recent_log_count: 0,
            },
        });

        let reports = reporter.list(10, 0.0, None, None, None, Some("beta"));
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].incident_id, "inc_beta");
    }

    #[test]
    fn logging_query_filters_client_telemetry_and_runtime_signals() {
        crate::logging::clear_logs();
        crate::logging::push_log_for_test("INFO", "client.telemetry.api", "mutating request ok");
        crate::logging::push_log_for_test("INFO", "request", "generic request ok");
        crate::logging::push_log_for_test("WARN", "live.worker", "ws dropped");

        let telemetry_entries = crate::logging::query_logs_after_sequence(
            0,
            None,
            None,
            Some("client.telemetry"),
            DESKTOP_CONTROL_PLANE_LOG_BATCH_LIMIT,
        );
        let telemetry_messages = telemetry_entries
            .iter()
            .map(|entry| entry.message.as_str())
            .collect::<Vec<_>>();
        let runtime_entries = crate::logging::query_logs_after_sequence(
            0,
            Some("WARN"),
            None,
            None,
            DESKTOP_CONTROL_PLANE_LOG_BATCH_LIMIT,
        );
        let runtime_messages = runtime_entries
            .iter()
            .map(|entry| entry.message.as_str())
            .collect::<Vec<_>>();
        let all_entries = crate::logging::query_logs_after_sequence(
            0,
            None,
            None,
            None,
            DESKTOP_CONTROL_PLANE_LOG_BATCH_LIMIT,
        );
        let all_messages = all_entries
            .iter()
            .map(|entry| entry.message.as_str())
            .collect::<Vec<_>>();

        assert!(telemetry_messages.contains(&"mutating request ok"));
        assert!(!telemetry_messages.contains(&"generic request ok"));
        assert!(runtime_messages.contains(&"ws dropped"));
        assert!(!runtime_messages.contains(&"mutating request ok"));
        assert!(all_messages.contains(&"generic request ok"));
    }

    #[test]
    fn control_plane_log_id_is_stable_per_entry_but_not_reused_sequence() {
        let context = test_sign_context();
        let baseline = crate::logging::LogEntry {
            sequence: 751,
            timestamp: 1_781_595_070.973,
            level: "INFO".to_string(),
            logger: "client.telemetry.runtime".to_string(),
            message: "Synthetic runtime probe after upload body fix".to_string(),
            fields_json: json!({
                "logger": "frontend.runtime",
                "kind": "synthetic"
            }),
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
        };

        let same = control_plane_log_id(&context, &baseline);
        let repeated = control_plane_log_id(&context, &baseline);
        assert_eq!(same, repeated);

        let reused_sequence_after_restart = crate::logging::LogEntry {
            timestamp: baseline.timestamp + 120.0,
            ..baseline.clone()
        };
        assert_ne!(
            same,
            control_plane_log_id(&context, &reused_sequence_after_restart)
        );
    }

    #[test]
    fn build_control_plane_upload_body_wraps_and_signs_payload() {
        let context = test_sign_context();
        let payload = json!({
            "incident_id": "incident-001",
            "message": "heartbeat failed",
        });

        let body =
            build_control_plane_upload_body(&context, "report", payload.clone(), 123).unwrap();

        let expected_mac = crate::control_plane_auth::sign_message_hex(
            &context.hmac_secret_hex,
            &format!(
                "{}.{}.{}.{}",
                context.license_id,
                context.device_id,
                123,
                serde_json::to_string(&payload).unwrap()
            ),
        )
        .unwrap();

        assert_eq!(body["license_id"], 42);
        assert_eq!(body["device_id"], "device-001");
        assert_eq!(body["ts"], 123);
        assert_eq!(body["mac"], expected_mac);
        assert_eq!(body["report"], payload);
    }

    #[test]
    fn build_control_plane_upload_body_preserves_log_arrays() {
        let context = test_sign_context();
        let logs = json!([
            {
                "log_id": "log-001",
                "license_id": 42,
                "device_id": "device-001",
                "message": "frontend action ok"
            }
        ]);

        let body = build_control_plane_upload_body(&context, "logs", logs.clone(), 456).unwrap();

        assert_eq!(body["logs"], logs);
        assert_eq!(body["ts"], 456);
        assert!(body["mac"].as_str().is_some_and(|value| !value.is_empty()));
    }

    #[test]
    fn control_plane_auth_rejections_are_not_retryable() {
        assert!(super::is_control_plane_auth_rejection(
            r#"server returned 401 Unauthorized: {"detail":"unauthorized: hmac mismatch"}"#
        ));
        assert!(super::is_control_plane_auth_rejection(
            "server returned 401 Unauthorized: proof-invalid"
        ));
        assert!(!super::is_control_plane_auth_rejection(
            "server returned 502 Bad Gateway"
        ));
    }
}
