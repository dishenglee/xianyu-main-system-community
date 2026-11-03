CREATE INDEX IF NOT EXISTS idx_runtime_logs_upload_status_occurred_sequence
    ON runtime_logs(upload_status, occurred_at ASC, sequence ASC);

CREATE INDEX IF NOT EXISTS idx_usage_events_upload_status_occurred_sequence
    ON usage_events(upload_status, occurred_at ASC, sequence ASC);

CREATE INDEX IF NOT EXISTS idx_incident_upload_queue_retry_due
    ON incident_upload_queue(status, next_retry_at, occurred_at ASC, queued_at ASC);
