CREATE TABLE IF NOT EXISTS runtime_logs (
    sequence INTEGER PRIMARY KEY,
    occurred_at REAL NOT NULL,
    level TEXT NOT NULL,
    logger TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    fields_json TEXT NOT NULL DEFAULT '{}',
    stream TEXT NOT NULL DEFAULT 'runtime',
    trace_id TEXT NOT NULL DEFAULT '',
    request_id TEXT NOT NULL DEFAULT '',
    incident_id TEXT NOT NULL DEFAULT '',
    username TEXT NOT NULL DEFAULT '',
    device_id TEXT NOT NULL DEFAULT '',
    account_uid TEXT NOT NULL DEFAULT '',
    session_id TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    job_id TEXT NOT NULL DEFAULT '',
    module TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    error_code TEXT NOT NULL DEFAULT '',
    retryable INTEGER,
    duration_ms INTEGER,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    upload_status TEXT NOT NULL DEFAULT 'pending',
    upload_attempt_count INTEGER NOT NULL DEFAULT 0,
    last_upload_error TEXT NOT NULL DEFAULT '',
    last_upload_attempt_at REAL NOT NULL DEFAULT 0,
    uploaded_at REAL NOT NULL DEFAULT 0,
    stored_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_runtime_logs_occurred_at ON runtime_logs(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_level_sequence ON runtime_logs(level, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_logger_sequence ON runtime_logs(logger, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_module_sequence ON runtime_logs(module, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_trace_id ON runtime_logs(trace_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_request_id ON runtime_logs(request_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_incident_id ON runtime_logs(incident_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_username ON runtime_logs(username);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_device_id ON runtime_logs(device_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_account_uid ON runtime_logs(account_uid);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_session_id ON runtime_logs(session_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_item_id ON runtime_logs(item_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_job_id ON runtime_logs(job_id);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_action_sequence ON runtime_logs(action, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_error_code ON runtime_logs(error_code);
CREATE INDEX IF NOT EXISTS idx_runtime_logs_upload_status_sequence
    ON runtime_logs(upload_status, sequence ASC);

CREATE TABLE IF NOT EXISTS usage_events (
    sequence INTEGER PRIMARY KEY,
    occurred_at REAL NOT NULL,
    level TEXT NOT NULL,
    logger TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    fields_json TEXT NOT NULL DEFAULT '{}',
    stream TEXT NOT NULL DEFAULT 'usage',
    trace_id TEXT NOT NULL DEFAULT '',
    request_id TEXT NOT NULL DEFAULT '',
    incident_id TEXT NOT NULL DEFAULT '',
    username TEXT NOT NULL DEFAULT '',
    device_id TEXT NOT NULL DEFAULT '',
    account_uid TEXT NOT NULL DEFAULT '',
    session_id TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    job_id TEXT NOT NULL DEFAULT '',
    module TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    error_code TEXT NOT NULL DEFAULT '',
    retryable INTEGER,
    duration_ms INTEGER,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    upload_status TEXT NOT NULL DEFAULT 'pending',
    upload_attempt_count INTEGER NOT NULL DEFAULT 0,
    last_upload_error TEXT NOT NULL DEFAULT '',
    last_upload_attempt_at REAL NOT NULL DEFAULT 0,
    uploaded_at REAL NOT NULL DEFAULT 0,
    stored_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_usage_events_occurred_at ON usage_events(occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_level_sequence ON usage_events(level, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_logger_sequence ON usage_events(logger, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_module_sequence ON usage_events(module, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_trace_id ON usage_events(trace_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_request_id ON usage_events(request_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_incident_id ON usage_events(incident_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_username ON usage_events(username);
CREATE INDEX IF NOT EXISTS idx_usage_events_device_id ON usage_events(device_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_account_uid ON usage_events(account_uid);
CREATE INDEX IF NOT EXISTS idx_usage_events_session_id ON usage_events(session_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_item_id ON usage_events(item_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_job_id ON usage_events(job_id);
CREATE INDEX IF NOT EXISTS idx_usage_events_action_sequence ON usage_events(action, sequence DESC);
CREATE INDEX IF NOT EXISTS idx_usage_events_error_code ON usage_events(error_code);
CREATE INDEX IF NOT EXISTS idx_usage_events_upload_status_sequence
    ON usage_events(upload_status, sequence ASC);

CREATE TABLE IF NOT EXISTS incident_upload_queue (
    incident_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    error_level TEXT NOT NULL DEFAULT 'error',
    module TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL DEFAULT '',
    error_code TEXT NOT NULL DEFAULT '',
    occurred_at REAL NOT NULL DEFAULT 0,
    retryable INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'queued',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    last_attempt_at REAL NOT NULL DEFAULT 0,
    next_retry_at REAL NOT NULL DEFAULT 0,
    queued_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    uploaded_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_incident_upload_queue_status_occurred_at
    ON incident_upload_queue(status, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_incident_upload_queue_module_occurred_at
    ON incident_upload_queue(module, occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_incident_upload_queue_error_code_occurred_at
    ON incident_upload_queue(error_code, occurred_at DESC);

CREATE TABLE IF NOT EXISTS log_sync_state (
    state_key TEXT PRIMARY KEY,
    sequence_value INTEGER NOT NULL DEFAULT 0,
    text_value TEXT NOT NULL DEFAULT '',
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);
