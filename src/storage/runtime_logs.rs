use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::{logging::LogEntry, telemetry::IncidentReport};

use super::{parse_json_map, unix_seconds_now, StorageError, StorageState};

const MEMORY_CURSOR_KEY: &str = "runtime_logs.memory_cursor";
const RUNTIME_UPLOAD_CURSOR_KEY: &str = "runtime_logs.upload_cursor.runtime";
const USAGE_UPLOAD_CURSOR_KEY: &str = "runtime_logs.upload_cursor.usage";
const RUNTIME_DIAGNOSTIC_POLICY_KEY: &str = "runtime_logs.diagnostic_policy";
const DEFAULT_RETENTION_DAYS: f64 = 7.0;
const DEFAULT_RETENTION_MAX_BYTES: i64 = 50 * 1024 * 1024;
const ANDROID_RETENTION_MAX_BYTES: i64 = 8 * 1024 * 1024;
const RETENTION_PRUNE_BATCH_SIZE: i64 = 10_000;
const CONTROL_PLANE_MAX_UPLOAD_ATTEMPTS: i64 = 3;
const DEFAULT_DIAGNOSTIC_LEVEL: &str = "DEBUG";
const DEFAULT_DIAGNOSTIC_SAMPLE_RATE: f64 = 1.0;

#[derive(Debug, Clone, Default)]
pub struct RuntimeLogFilter {
    pub stream: String,
    pub level: String,
    pub keyword: String,
    pub logger: String,
    pub module: String,
    pub trace_id: String,
    pub request_id: String,
    pub incident_id: String,
    pub username: String,
    pub device_id: String,
    pub account_uid: String,
    pub session_id: String,
    pub item_id: String,
    pub job_id: String,
    pub action: String,
    pub outcome: String,
    pub error_code: String,
    pub since: f64,
    pub after_sequence: u64,
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistedLogStream {
    Runtime,
    Usage,
}

impl PersistedLogStream {
    fn from_filter(raw: &str) -> Self {
        if raw.trim().eq_ignore_ascii_case("usage") {
            Self::Usage
        } else {
            Self::Runtime
        }
    }

    fn from_entry(entry: &LogEntry) -> Self {
        match entry.stream.as_deref() {
            Some(stream) if stream.eq_ignore_ascii_case("usage") => Self::Usage,
            _ => Self::Runtime,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Usage => "usage",
        }
    }

    fn table_name(self) -> &'static str {
        match self {
            Self::Runtime => "runtime_logs",
            Self::Usage => "usage_events",
        }
    }

    fn upload_cursor_key(self) -> &'static str {
        match self {
            Self::Runtime => RUNTIME_UPLOAD_CURSOR_KEY,
            Self::Usage => USAGE_UPLOAD_CURSOR_KEY,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeDiagnosticPolicyRecord {
    enabled: bool,
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
    starts_at: Option<f64>,
    ends_at: Option<f64>,
    include_payloads: bool,
    sample_rate: f64,
}

#[derive(Debug, Clone)]
struct RuntimeDiagnosticPolicyDecision {
    matched: bool,
    level: Option<String>,
    include_payloads: bool,
    sample_rate: f64,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeDiagnosticMatchContext<'a> {
    module: Option<&'a str>,
    username: Option<&'a str>,
    device_id: Option<&'a str>,
    account_uid: Option<&'a str>,
    session_id: Option<&'a str>,
    occurred_at: f64,
    sampling_key: Option<&'a str>,
}

impl Default for RuntimeDiagnosticPolicyRecord {
    fn default() -> Self {
        Self {
            enabled: false,
            level: DEFAULT_DIAGNOSTIC_LEVEL.to_string(),
            modules: Vec::new(),
            usernames: Vec::new(),
            device_ids: Vec::new(),
            account_uids: Vec::new(),
            session_ids: Vec::new(),
            starts_at: None,
            ends_at: None,
            include_payloads: false,
            sample_rate: DEFAULT_DIAGNOSTIC_SAMPLE_RATE,
        }
    }
}

impl StorageState {
    pub async fn load_runtime_diagnostic_policy(&self) -> Result<Value, StorageError> {
        let policy = self.read_runtime_diagnostic_policy_record().await?;
        Ok(policy_to_value(&policy))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn save_runtime_diagnostic_policy(
        &self,
        enabled: bool,
        level: &str,
        modules: &[String],
        usernames: &[String],
        device_ids: &[String],
        account_uids: &[String],
        session_ids: &[String],
        starts_at: Option<f64>,
        ends_at: Option<f64>,
        include_payloads: bool,
        sample_rate: f64,
    ) -> Result<Value, StorageError> {
        let policy = RuntimeDiagnosticPolicyRecord {
            enabled,
            level: normalize_diagnostic_level(level),
            modules: normalize_policy_list(modules),
            usernames: normalize_policy_list(usernames),
            device_ids: normalize_policy_list(device_ids),
            account_uids: normalize_policy_list(account_uids),
            session_ids: normalize_policy_list(session_ids),
            starts_at: normalize_optional_timestamp(starts_at),
            ends_at: normalize_optional_timestamp(ends_at),
            include_payloads,
            sample_rate: normalize_sample_rate(sample_rate),
        };
        self.write_runtime_diagnostic_policy_record(&policy).await?;
        Ok(policy_to_value(&policy))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn runtime_diagnostic_policy_decision(
        &self,
        module: Option<&str>,
        username: Option<&str>,
        device_id: Option<&str>,
        account_uid: Option<&str>,
        session_id: Option<&str>,
        occurred_at: Option<f64>,
        sampling_key: Option<&str>,
    ) -> Result<Value, StorageError> {
        let policy = self.read_runtime_diagnostic_policy_record().await?;
        let decision = evaluate_runtime_diagnostic_policy(
            &policy,
            RuntimeDiagnosticMatchContext {
                module,
                username,
                device_id,
                account_uid,
                session_id,
                occurred_at: normalize_optional_timestamp(occurred_at)
                    .unwrap_or_else(unix_seconds_now),
                sampling_key,
            },
        );
        Ok(policy_decision_to_value(&decision))
    }

    pub async fn sync_runtime_log_buffer(&self) -> Result<usize, StorageError> {
        let after_sequence = self.read_log_sync_sequence(MEMORY_CURSOR_KEY).await?;
        let entries =
            crate::logging::query_logs_after_sequence(after_sequence, None, None, None, 5000);
        if entries.is_empty() {
            return Ok(0);
        }

        let last_sequence = entries
            .last()
            .map(|entry| entry.sequence)
            .unwrap_or(after_sequence);
        let mut runtime_entries = Vec::new();
        let mut usage_entries = Vec::new();
        for entry in entries {
            match PersistedLogStream::from_entry(&entry) {
                PersistedLogStream::Runtime => runtime_entries.push(entry),
                PersistedLogStream::Usage => usage_entries.push(entry),
            }
        }

        self.insert_runtime_log_entries(PersistedLogStream::Runtime, &runtime_entries)
            .await?;
        self.insert_runtime_log_entries(PersistedLogStream::Usage, &usage_entries)
            .await?;
        self.write_log_sync_sequence(MEMORY_CURSOR_KEY, last_sequence)
            .await?;

        Ok(runtime_entries.len() + usage_entries.len())
    }

    pub async fn list_runtime_logs(
        &self,
        filter: &RuntimeLogFilter,
    ) -> Result<Vec<LogEntry>, StorageError> {
        let stream = PersistedLogStream::from_filter(&filter.stream);
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT sequence, occurred_at, level, logger, message, fields_json, stream, trace_id, \
             request_id, incident_id, username, device_id, account_uid, session_id, item_id, job_id, \
             module, action, outcome, error_code, retryable, duration_ms \
             FROM {} WHERE 1=1",
            stream.table_name()
        ));

        if !filter.level.trim().is_empty() {
            builder
                .push(" AND level = ")
                .push_bind(filter.level.trim().to_ascii_uppercase());
        }
        if !filter.logger.trim().is_empty() {
            builder
                .push(" AND logger = ")
                .push_bind(filter.logger.trim());
        }
        if !filter.module.trim().is_empty() {
            builder
                .push(" AND module = ")
                .push_bind(filter.module.trim());
        }
        if !filter.trace_id.trim().is_empty() {
            builder
                .push(" AND trace_id = ")
                .push_bind(filter.trace_id.trim());
        }
        if !filter.request_id.trim().is_empty() {
            builder
                .push(" AND request_id = ")
                .push_bind(filter.request_id.trim());
        }
        if !filter.incident_id.trim().is_empty() {
            builder
                .push(" AND incident_id = ")
                .push_bind(filter.incident_id.trim());
        }
        if !filter.username.trim().is_empty() {
            builder
                .push(" AND username = ")
                .push_bind(filter.username.trim());
        }
        if !filter.device_id.trim().is_empty() {
            builder
                .push(" AND device_id = ")
                .push_bind(filter.device_id.trim());
        }
        if !filter.account_uid.trim().is_empty() {
            builder
                .push(" AND account_uid = ")
                .push_bind(filter.account_uid.trim());
        }
        if !filter.session_id.trim().is_empty() {
            builder
                .push(" AND session_id = ")
                .push_bind(filter.session_id.trim());
        }
        if !filter.item_id.trim().is_empty() {
            builder
                .push(" AND item_id = ")
                .push_bind(filter.item_id.trim());
        }
        if !filter.job_id.trim().is_empty() {
            builder
                .push(" AND job_id = ")
                .push_bind(filter.job_id.trim());
        }
        if !filter.action.trim().is_empty() {
            builder
                .push(" AND action = ")
                .push_bind(filter.action.trim());
        }
        if !filter.outcome.trim().is_empty() {
            builder
                .push(" AND outcome = ")
                .push_bind(filter.outcome.trim());
        }
        if !filter.error_code.trim().is_empty() {
            builder
                .push(" AND error_code = ")
                .push_bind(filter.error_code.trim());
        }
        if filter.since > 0.0 {
            builder.push(" AND occurred_at >= ").push_bind(filter.since);
        }
        if filter.after_sequence > 0 {
            builder
                .push(" AND sequence > ")
                .push_bind(filter.after_sequence as i64);
        }
        if !filter.keyword.trim().is_empty() {
            let keyword = format!("%{}%", filter.keyword.trim());
            builder
                .push(" AND (message LIKE ")
                .push_bind(keyword.clone())
                .push(" OR logger LIKE ")
                .push_bind(keyword.clone())
                .push(" OR module LIKE ")
                .push_bind(keyword.clone())
                .push(" OR action LIKE ")
                .push_bind(keyword.clone())
                .push(" OR error_code LIKE ")
                .push_bind(keyword.clone())
                .push(" OR fields_json LIKE ")
                .push_bind(keyword)
                .push(")");
        }

        builder
            .push(" ORDER BY sequence DESC LIMIT ")
            .push_bind(filter.limit.clamp(1, 2000) as i64);

        let rows = builder.build().fetch_all(&self.xianyu).await?;
        let mut entries = rows
            .into_iter()
            .map(runtime_log_from_row)
            .collect::<Vec<_>>();
        entries.reverse();
        Ok(entries)
    }

    pub async fn latest_runtime_log_sequence(
        &self,
        stream: Option<&str>,
    ) -> Result<u64, StorageError> {
        let stream = PersistedLogStream::from_filter(stream.unwrap_or("runtime"));
        let row = sqlx::query_scalar::<_, Option<i64>>(&format!(
            "SELECT MAX(sequence) FROM {}",
            stream.table_name()
        ))
        .fetch_one(&self.xianyu)
        .await?;
        Ok(row.unwrap_or_default().max(0) as u64)
    }

    pub async fn clear_runtime_log_storage(&self) -> Result<(), StorageError> {
        sqlx::query("DELETE FROM runtime_logs")
            .execute(&self.xianyu)
            .await?;
        sqlx::query("DELETE FROM usage_events")
            .execute(&self.xianyu)
            .await?;
        self.write_log_sync_sequence(MEMORY_CURSOR_KEY, crate::logging::latest_sequence())
            .await?;
        self.write_log_sync_sequence(RUNTIME_UPLOAD_CURSOR_KEY, 0)
            .await?;
        self.write_log_sync_sequence(USAGE_UPLOAD_CURSOR_KEY, 0)
            .await?;
        Ok(())
    }

    pub async fn next_control_plane_log_batch(
        &self,
        stream: &str,
        limit: usize,
    ) -> Result<Vec<LogEntry>, StorageError> {
        let stream = PersistedLogStream::from_filter(stream);
        let after_sequence = self
            .read_log_sync_sequence(stream.upload_cursor_key())
            .await?;
        let rows = sqlx::query(&format!(
            "SELECT sequence, occurred_at, level, logger, message, fields_json, stream, trace_id, \
             request_id, incident_id, username, device_id, account_uid, session_id, item_id, job_id, \
             module, action, outcome, error_code, retryable, duration_ms \
             FROM {} WHERE sequence > ? AND upload_status IN ('pending', 'retry') ORDER BY sequence ASC LIMIT ?",
            stream.table_name()
        ))
        .bind(after_sequence as i64)
        .bind(limit.clamp(1, 500) as i64)
        .fetch_all(&self.xianyu)
        .await?;
        Ok(rows.into_iter().map(runtime_log_from_row).collect())
    }

    pub async fn mark_control_plane_logs_uploaded(
        &self,
        stream: &str,
        last_sequence: u64,
    ) -> Result<(), StorageError> {
        let stream = PersistedLogStream::from_filter(stream);
        let now = unix_seconds_now();
        sqlx::query(&format!(
            "UPDATE {} SET upload_status = 'uploaded', upload_attempt_count = upload_attempt_count + 1, \
             last_upload_error = '', last_upload_attempt_at = ?, uploaded_at = ? \
             WHERE sequence <= ? AND upload_status != 'uploaded'",
            stream.table_name()
        ))
        .bind(now)
        .bind(now)
        .bind(last_sequence as i64)
        .execute(&self.xianyu)
        .await?;
        self.write_log_sync_sequence(stream.upload_cursor_key(), last_sequence)
            .await
    }

    pub async fn mark_control_plane_logs_failed(
        &self,
        stream: &str,
        last_sequence: u64,
        error: &str,
    ) -> Result<(), StorageError> {
        let stream = PersistedLogStream::from_filter(stream);
        let now = unix_seconds_now();
        sqlx::query(&format!(
            "UPDATE {} SET upload_status = 'retry', upload_attempt_count = upload_attempt_count + 1, \
             last_upload_error = ?, last_upload_attempt_at = ? \
             WHERE sequence <= ? AND upload_status IN ('pending', 'retry')",
            stream.table_name()
        ))
        .bind(truncate_text(error, 1024))
        .bind(now)
        .bind(last_sequence as i64)
        .execute(&self.xianyu)
        .await?;

        sqlx::query(&format!(
            "UPDATE {} SET upload_status = 'suppressed' \
             WHERE sequence <= ? AND upload_status = 'retry' AND upload_attempt_count >= ?",
            stream.table_name()
        ))
        .bind(last_sequence as i64)
        .bind(CONTROL_PLANE_MAX_UPLOAD_ATTEMPTS)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn enqueue_incident_upload(
        &self,
        report: &IncidentReport,
    ) -> Result<(), StorageError> {
        let payload_json = serde_json::to_string(report).map_err(StorageError::from)?;
        sqlx::query(
            r#"
            INSERT INTO incident_upload_queue (
                incident_id, payload_json, error_level, module, action, error_code,
                occurred_at, retryable, status, attempt_count, last_error, last_attempt_at,
                next_retry_at, queued_at, uploaded_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'queued', 0, '', 0, 0, unixepoch('subsec'), 0, unixepoch('subsec'))
            ON CONFLICT(incident_id) DO UPDATE SET
                payload_json = excluded.payload_json,
                error_level = excluded.error_level,
                module = excluded.module,
                action = excluded.action,
                error_code = excluded.error_code,
                occurred_at = excluded.occurred_at,
                retryable = excluded.retryable,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&report.incident_id)
        .bind(payload_json)
        .bind(&report.error_level)
        .bind(&report.module)
        .bind(&report.action)
        .bind(&report.error_code)
        .bind(report.occurred_at)
        .bind(if report.retryable { 1_i64 } else { 0_i64 })
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn mark_incident_upload_started(
        &self,
        incident_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE incident_upload_queue SET status = 'uploading', updated_at = unixepoch('subsec') WHERE incident_id = ?",
        )
        .bind(incident_id)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn mark_incident_upload_result(
        &self,
        incident_id: &str,
        uploaded: bool,
        retryable: bool,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        let now = unix_seconds_now();
        let status = if uploaded {
            "uploaded"
        } else if retryable {
            "retry"
        } else {
            "failed"
        };
        let next_retry_at = if uploaded || !retryable {
            0.0
        } else {
            now + 30.0
        };
        sqlx::query(
            r#"
            UPDATE incident_upload_queue
            SET status = ?,
                attempt_count = attempt_count + 1,
                last_error = ?,
                last_attempt_at = ?,
                next_retry_at = ?,
                uploaded_at = CASE WHEN ? THEN ? ELSE uploaded_at END,
                updated_at = ?
            WHERE incident_id = ?
            "#,
        )
        .bind(status)
        .bind(truncate_text(error.unwrap_or_default(), 1024))
        .bind(now)
        .bind(next_retry_at)
        .bind(uploaded)
        .bind(now)
        .bind(now)
        .bind(incident_id)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    pub async fn next_incident_upload_batch(
        &self,
        limit: usize,
    ) -> Result<Vec<IncidentReport>, StorageError> {
        let now = unix_seconds_now();
        let rows = sqlx::query(
            r#"
            SELECT payload_json
            FROM incident_upload_queue
            WHERE status IN ('queued', 'retry') AND (next_retry_at <= 0 OR next_retry_at <= ?)
            ORDER BY occurred_at ASC, queued_at ASC
            LIMIT ?
            "#,
        )
        .bind(now)
        .bind(limit.clamp(1, 100) as i64)
        .fetch_all(&self.xianyu)
        .await?;

        rows.into_iter()
            .map(|row| {
                let payload_json: String = row.get("payload_json");
                serde_json::from_str::<IncidentReport>(&payload_json).map_err(StorageError::from)
            })
            .collect()
    }

    pub async fn enforce_runtime_log_retention(&self) -> Result<(), StorageError> {
        let cutoff = unix_seconds_now() - retention_window_days() * 24.0 * 3600.0;
        for stream in [PersistedLogStream::Runtime, PersistedLogStream::Usage] {
            sqlx::query(&format!(
                "DELETE FROM {} WHERE upload_status IN ('uploaded', 'suppressed') AND occurred_at < ?",
                stream.table_name()
            ))
            .bind(cutoff)
            .execute(&self.xianyu)
            .await?;

            let max_bytes = retention_max_bytes();
            if self.table_total_bytes(stream).await? <= max_bytes {
                continue;
            }

            let mut total_removed = 0_u64;
            loop {
                let removed = self
                    .prune_runtime_log_batch(stream, "upload_status IN ('uploaded', 'suppressed')")
                    .await?;
                total_removed += removed;
                if removed == 0 || self.table_total_bytes(stream).await? <= max_bytes {
                    break;
                }
            }

            while self.table_total_bytes(stream).await? > max_bytes {
                let removed = self.prune_runtime_log_batch(stream, "1=1").await?;
                if removed == 0 {
                    break;
                }
                total_removed += removed;
            }

            if total_removed > 0 {
                tracing::warn!(
                    target: "runtime.logs.retention",
                    stream = stream.as_str(),
                    removed = total_removed,
                    "runtime log retention pruned oldest local rows"
                );
            }
        }

        Ok(())
    }

    async fn insert_runtime_log_entries(
        &self,
        stream: PersistedLogStream,
        entries: &[LogEntry],
    ) -> Result<(), StorageError> {
        let diagnostic_policy = if matches!(stream, PersistedLogStream::Runtime) {
            Some(self.read_runtime_diagnostic_policy_record().await?)
        } else {
            None
        };
        for entry in entries {
            let effective_level = diagnostic_policy
                .as_ref()
                .map(|policy| diagnostic_policy_for_entry(policy, entry))
                .filter(|decision| decision.matched);
            let stored_level = effective_level
                .as_ref()
                .and_then(|decision| decision.level.as_deref())
                .map(str::to_string)
                .unwrap_or_else(|| normalize_diagnostic_level(&entry.level));
            let fields_json =
                serde_json::to_string(&entry.fields_json).unwrap_or_else(|_| "{}".to_string());
            let upload_status =
                initial_upload_status_with_policy(entry, stream, effective_level.as_ref());
            sqlx::query(&format!(
                "INSERT OR IGNORE INTO {} (
                    sequence, occurred_at, level, logger, message, fields_json, stream,
                    trace_id, request_id, incident_id, username, device_id, account_uid,
                    session_id, item_id, job_id, module, action, outcome, error_code,
                    retryable, duration_ms, size_bytes, upload_status, upload_attempt_count,
                    last_upload_error, last_upload_attempt_at, uploaded_at, stored_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, '', 0, 0, unixepoch('subsec'))",
                stream.table_name()
            ))
            .bind(entry.sequence as i64)
            .bind(entry.timestamp)
            .bind(stored_level)
            .bind(entry.logger.trim())
            .bind(entry.message.trim())
            .bind(&fields_json)
            .bind(normalized_stream(entry, stream))
            .bind(entry.trace_id.as_deref().unwrap_or_default())
            .bind(entry.request_id.as_deref().unwrap_or_default())
            .bind(entry.incident_id.as_deref().unwrap_or_default())
            .bind(entry.username.as_deref().unwrap_or_default())
            .bind(entry.device_id.as_deref().unwrap_or_default())
            .bind(entry.account_uid.as_deref().unwrap_or_default())
            .bind(entry.session_id.as_deref().unwrap_or_default())
            .bind(entry.item_id.as_deref().unwrap_or_default())
            .bind(entry.job_id.as_deref().unwrap_or_default())
            .bind(entry.module.as_deref().unwrap_or_default())
            .bind(entry.action.as_deref().unwrap_or_default())
            .bind(entry.outcome.as_deref().unwrap_or_default())
            .bind(entry.error_code.as_deref().unwrap_or_default())
            .bind(entry.retryable.map(|value| if value { 1_i64 } else { 0_i64 }))
            .bind(entry.duration_ms.map(|value| value as i64))
            .bind(estimate_log_entry_size(entry, &fields_json))
            .bind(upload_status)
            .execute(&self.xianyu)
            .await?;
        }
        Ok(())
    }

    async fn read_log_sync_sequence(&self, state_key: &str) -> Result<u64, StorageError> {
        let value = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT sequence_value FROM log_sync_state WHERE state_key = ?",
        )
        .bind(state_key)
        .fetch_optional(&self.xianyu)
        .await?
        .flatten()
        .unwrap_or_default();
        Ok(value.max(0) as u64)
    }

    async fn write_log_sync_sequence(
        &self,
        state_key: &str,
        sequence_value: u64,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO log_sync_state (state_key, sequence_value, text_value, updated_at)
            VALUES (?, ?, '', unixepoch('subsec'))
            ON CONFLICT(state_key) DO UPDATE SET
                sequence_value = excluded.sequence_value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(state_key)
        .bind(sequence_value as i64)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    async fn read_log_sync_text_value(
        &self,
        state_key: &str,
    ) -> Result<Option<String>, StorageError> {
        let value = sqlx::query_scalar::<_, Option<String>>(
            "SELECT text_value FROM log_sync_state WHERE state_key = ?",
        )
        .bind(state_key)
        .fetch_optional(&self.xianyu)
        .await?
        .flatten()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
        Ok(value)
    }

    async fn write_log_sync_text_value(
        &self,
        state_key: &str,
        text_value: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            INSERT INTO log_sync_state (state_key, sequence_value, text_value, updated_at)
            VALUES (?, 0, ?, unixepoch('subsec'))
            ON CONFLICT(state_key) DO UPDATE SET
                text_value = excluded.text_value,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(state_key)
        .bind(text_value)
        .execute(&self.xianyu)
        .await?;
        Ok(())
    }

    async fn read_runtime_diagnostic_policy_record(
        &self,
    ) -> Result<RuntimeDiagnosticPolicyRecord, StorageError> {
        let Some(raw) = self
            .read_log_sync_text_value(RUNTIME_DIAGNOSTIC_POLICY_KEY)
            .await?
        else {
            return Ok(RuntimeDiagnosticPolicyRecord::default());
        };
        match serde_json::from_str::<RuntimeDiagnosticPolicyRecord>(&raw) {
            Ok(policy) => Ok(normalize_runtime_diagnostic_policy(policy)),
            Err(error) => {
                tracing::warn!(
                    target: "runtime.logs.policy",
                    error = %error,
                    "invalid runtime diagnostic policy payload; falling back to default"
                );
                Ok(RuntimeDiagnosticPolicyRecord::default())
            }
        }
    }

    async fn write_runtime_diagnostic_policy_record(
        &self,
        policy: &RuntimeDiagnosticPolicyRecord,
    ) -> Result<(), StorageError> {
        let payload =
            serde_json::to_string(policy).unwrap_or_else(|_| "{\"enabled\":false}".to_string());
        self.write_log_sync_text_value(RUNTIME_DIAGNOSTIC_POLICY_KEY, &payload)
            .await
    }

    async fn table_total_bytes(&self, stream: PersistedLogStream) -> Result<i64, StorageError> {
        let total = sqlx::query_scalar::<_, Option<i64>>(&format!(
            "SELECT SUM(size_bytes) FROM {}",
            stream.table_name()
        ))
        .fetch_one(&self.xianyu)
        .await?
        .unwrap_or_default();
        Ok(total)
    }

    async fn prune_runtime_log_batch(
        &self,
        stream: PersistedLogStream,
        predicate: &str,
    ) -> Result<u64, StorageError> {
        let result = sqlx::query(&format!(
            "DELETE FROM {} WHERE sequence IN (
                SELECT sequence FROM {} WHERE {}
                ORDER BY occurred_at ASC, sequence ASC LIMIT ?
            )",
            stream.table_name(),
            stream.table_name(),
            predicate
        ))
        .bind(RETENTION_PRUNE_BATCH_SIZE)
        .execute(&self.xianyu)
        .await?;
        Ok(result.rows_affected())
    }
}

fn runtime_log_from_row(row: sqlx::sqlite::SqliteRow) -> LogEntry {
    let fields_json = Value::Object(parse_json_map(row.get::<String, _>("fields_json").as_str()));
    LogEntry {
        sequence: row.get::<i64, _>("sequence").max(0) as u64,
        timestamp: row.get("occurred_at"),
        level: row.get("level"),
        logger: row.get("logger"),
        message: row.get("message"),
        fields_json,
        stream: non_empty_string(row.get("stream")),
        trace_id: non_empty_string(row.get("trace_id")),
        request_id: non_empty_string(row.get("request_id")),
        incident_id: non_empty_string(row.get("incident_id")),
        username: non_empty_string(row.get("username")),
        device_id: non_empty_string(row.get("device_id")),
        account_uid: non_empty_string(row.get("account_uid")),
        session_id: non_empty_string(row.get("session_id")),
        item_id: non_empty_string(row.get("item_id")),
        job_id: non_empty_string(row.get("job_id")),
        module: non_empty_string(row.get("module")),
        action: non_empty_string(row.get("action")),
        outcome: non_empty_string(row.get("outcome")),
        error_code: non_empty_string(row.get("error_code")),
        retryable: row
            .get::<Option<i64>, _>("retryable")
            .map(|value| value != 0),
        duration_ms: row
            .get::<Option<i64>, _>("duration_ms")
            .map(|value| value.max(0) as u64),
    }
}

fn normalized_stream(entry: &LogEntry, fallback: PersistedLogStream) -> String {
    entry
        .stream
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_else(|| fallback.as_str().to_string())
}

fn initial_upload_status_with_policy(
    entry: &LogEntry,
    stream: PersistedLogStream,
    policy_decision: Option<&RuntimeDiagnosticPolicyDecision>,
) -> &'static str {
    if policy_decision.is_some_and(|decision| decision.matched) {
        return "pending";
    }
    if should_upload_entry(entry.level.as_str(), stream) {
        "pending"
    } else {
        "suppressed"
    }
}

fn should_upload_entry(level: &str, stream: PersistedLogStream) -> bool {
    let level = normalize_diagnostic_level(level);
    match stream {
        PersistedLogStream::Runtime => matches!(level.as_str(), "WARN" | "ERROR"),
        PersistedLogStream::Usage => level == "INFO",
    }
}

fn diagnostic_policy_for_entry(
    policy: &RuntimeDiagnosticPolicyRecord,
    entry: &LogEntry,
) -> RuntimeDiagnosticPolicyDecision {
    evaluate_runtime_diagnostic_policy(
        policy,
        RuntimeDiagnosticMatchContext {
            module: entry.module.as_deref(),
            username: entry.username.as_deref(),
            device_id: entry.device_id.as_deref(),
            account_uid: entry.account_uid.as_deref(),
            session_id: entry.session_id.as_deref(),
            occurred_at: entry.timestamp,
            sampling_key: runtime_diagnostic_sampling_key(entry).as_deref(),
        },
    )
}

fn evaluate_runtime_diagnostic_policy(
    policy: &RuntimeDiagnosticPolicyRecord,
    context: RuntimeDiagnosticMatchContext<'_>,
) -> RuntimeDiagnosticPolicyDecision {
    let matched = policy.enabled
        && time_window_matches(policy.starts_at, policy.ends_at, context.occurred_at)
        && list_match(&policy.modules, context.module)
        && list_match(&policy.usernames, context.username)
        && list_match(&policy.device_ids, context.device_id)
        && list_match(&policy.account_uids, context.account_uid)
        && list_match(&policy.session_ids, context.session_id)
        && sample_rate_matches(policy.sample_rate, context.sampling_key);

    RuntimeDiagnosticPolicyDecision {
        matched,
        level: matched.then(|| policy.level.clone()),
        include_payloads: matched && policy.include_payloads,
        sample_rate: policy.sample_rate,
    }
}

fn policy_to_value(policy: &RuntimeDiagnosticPolicyRecord) -> Value {
    json!({
        "enabled": policy.enabled,
        "level": policy.level,
        "modules": policy.modules,
        "usernames": policy.usernames,
        "deviceIds": policy.device_ids,
        "accountUids": policy.account_uids,
        "sessionIds": policy.session_ids,
        "startsAt": policy.starts_at,
        "endsAt": policy.ends_at,
        "includePayloads": policy.include_payloads,
        "sampleRate": policy.sample_rate,
    })
}

fn policy_decision_to_value(decision: &RuntimeDiagnosticPolicyDecision) -> Value {
    json!({
        "matched": decision.matched,
        "level": decision.level,
        "includePayloads": decision.include_payloads,
        "sampleRate": decision.sample_rate,
    })
}

fn normalize_runtime_diagnostic_policy(
    policy: RuntimeDiagnosticPolicyRecord,
) -> RuntimeDiagnosticPolicyRecord {
    RuntimeDiagnosticPolicyRecord {
        enabled: policy.enabled,
        level: normalize_diagnostic_level(&policy.level),
        modules: normalize_policy_list(&policy.modules),
        usernames: normalize_policy_list(&policy.usernames),
        device_ids: normalize_policy_list(&policy.device_ids),
        account_uids: normalize_policy_list(&policy.account_uids),
        session_ids: normalize_policy_list(&policy.session_ids),
        starts_at: normalize_optional_timestamp(policy.starts_at),
        ends_at: normalize_optional_timestamp(policy.ends_at),
        include_payloads: policy.include_payloads,
        sample_rate: normalize_sample_rate(policy.sample_rate),
    }
}

fn normalize_policy_list(values: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        normalized.push(trimmed.to_string());
    }
    normalized
}

fn normalize_optional_timestamp(value: Option<f64>) -> Option<f64> {
    value.filter(|timestamp| timestamp.is_finite() && *timestamp > 0.0)
}

fn normalize_sample_rate(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_DIAGNOSTIC_SAMPLE_RATE;
    }
    value.clamp(0.0, 1.0)
}

fn normalize_diagnostic_level(level: &str) -> String {
    match level.trim().to_ascii_uppercase().as_str() {
        "TRACE" => "TRACE".to_string(),
        "DEBUG" => "DEBUG".to_string(),
        "INFO" => "INFO".to_string(),
        "WARN" | "WARNING" => "WARN".to_string(),
        "ERROR" => "ERROR".to_string(),
        _ => DEFAULT_DIAGNOSTIC_LEVEL.to_string(),
    }
}

fn list_match(filters: &[String], candidate: Option<&str>) -> bool {
    if filters.is_empty() {
        return true;
    }
    let Some(candidate) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    filters
        .iter()
        .any(|filter| string_matches_filter(candidate, filter))
}

fn string_matches_filter(candidate: &str, filter: &str) -> bool {
    let candidate = candidate.trim().to_ascii_lowercase();
    let filter = filter.trim().to_ascii_lowercase();
    if let Some(prefix) = filter.strip_suffix('*') {
        candidate.starts_with(prefix)
    } else {
        candidate == filter
    }
}

fn time_window_matches(starts_at: Option<f64>, ends_at: Option<f64>, occurred_at: f64) -> bool {
    starts_at.map_or(true, |starts_at| occurred_at >= starts_at)
        && ends_at.map_or(true, |ends_at| occurred_at <= ends_at)
}

fn sample_rate_matches(sample_rate: f64, sampling_key: Option<&str>) -> bool {
    if sample_rate >= 1.0 {
        return true;
    }
    if sample_rate <= 0.0 {
        return false;
    }
    let normalized_key = sampling_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("__runtime_diagnostic_default__");
    stable_sampling_bucket(normalized_key) < sample_rate
}

fn stable_sampling_bucket(key: &str) -> f64 {
    const FNV_OFFSET: u32 = 0x811c9dc5;
    const FNV_PRIME: u32 = 0x01000193;

    let mut hash = FNV_OFFSET;
    for byte in key.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    f64::from(hash) / 4_294_967_296.0
}

fn runtime_diagnostic_sampling_key(entry: &LogEntry) -> Option<String> {
    [
        entry.trace_id.as_deref(),
        entry.request_id.as_deref(),
        entry.session_id.as_deref(),
        entry.username.as_deref(),
        entry.device_id.as_deref(),
        entry.account_uid.as_deref(),
        entry.module.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|value| !value.is_empty())
    .map(str::to_string)
}

fn estimate_log_entry_size(entry: &LogEntry, fields_json: &str) -> i64 {
    let total = entry.logger.len()
        + entry.message.len()
        + fields_json.len()
        + entry.stream.as_deref().unwrap_or_default().len()
        + entry.trace_id.as_deref().unwrap_or_default().len()
        + entry.request_id.as_deref().unwrap_or_default().len()
        + entry.incident_id.as_deref().unwrap_or_default().len()
        + entry.username.as_deref().unwrap_or_default().len()
        + entry.device_id.as_deref().unwrap_or_default().len()
        + entry.account_uid.as_deref().unwrap_or_default().len()
        + entry.session_id.as_deref().unwrap_or_default().len()
        + entry.item_id.as_deref().unwrap_or_default().len()
        + entry.job_id.as_deref().unwrap_or_default().len()
        + entry.module.as_deref().unwrap_or_default().len()
        + entry.action.as_deref().unwrap_or_default().len()
        + entry.outcome.as_deref().unwrap_or_default().len()
        + entry.error_code.as_deref().unwrap_or_default().len();
    total as i64
}

fn retention_window_days() -> f64 {
    std::env::var("XIANYU_RUNTIME_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| *value > 0.0)
        .unwrap_or(DEFAULT_RETENTION_DAYS)
}

fn retention_max_bytes() -> i64 {
    std::env::var("XIANYU_RUNTIME_LOG_RETENTION_MAX_BYTES")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(|| {
            if cfg!(target_os = "android") {
                ANDROID_RETENTION_MAX_BYTES
            } else {
                DEFAULT_RETENTION_MAX_BYTES
            }
        })
}

fn non_empty_string(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn truncate_text(value: &str, limit: usize) -> String {
    let mut text = value.trim().to_string();
    if text.len() > limit {
        text.truncate(limit);
    }
    text
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        StorageError::InvalidDatabaseUrl {
            path: std::path::PathBuf::from("<runtime-log-json>"),
            message: value.to_string(),
        }
    }
}
