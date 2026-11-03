CREATE TABLE IF NOT EXISTS offline_operation_log_outbox (
    event_id TEXT PRIMARY KEY,
    module TEXT NOT NULL,
    action TEXT NOT NULL,
    operator TEXT NOT NULL DEFAULT '',
    trigger_source TEXT NOT NULL DEFAULT 'manual',
    uid TEXT NOT NULL DEFAULT '',
    account_nick TEXT NOT NULL DEFAULT '',
    target_type TEXT NOT NULL,
    target_id TEXT NOT NULL DEFAULT '',
    target_title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'success',
    message TEXT NOT NULL DEFAULT '',
    params_json TEXT NOT NULL DEFAULT '{}',
    before_json TEXT NOT NULL DEFAULT '{}',
    after_json TEXT NOT NULL DEFAULT '{}',
    extra_json TEXT NOT NULL DEFAULT '{}',
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    flushed_at REAL
);

CREATE INDEX IF NOT EXISTS idx_offline_operation_log_outbox_pending
    ON offline_operation_log_outbox(flushed_at, created_at);
