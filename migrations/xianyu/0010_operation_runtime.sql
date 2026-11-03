CREATE TABLE IF NOT EXISTS operation_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL DEFAULT '',
    module TEXT NOT NULL,
    action TEXT NOT NULL,
    operator TEXT NOT NULL DEFAULT '',
    trigger_source TEXT NOT NULL DEFAULT 'manual',
    uid TEXT NOT NULL DEFAULT '',
    account_nick TEXT NOT NULL DEFAULT '',
    target_type TEXT NOT NULL DEFAULT '',
    target_id TEXT NOT NULL DEFAULT '',
    target_title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'success',
    error_code TEXT NOT NULL DEFAULT '',
    message TEXT NOT NULL DEFAULT '',
    params_json TEXT NOT NULL DEFAULT '{}',
    before_json TEXT NOT NULL DEFAULT '{}',
    after_json TEXT NOT NULL DEFAULT '{}',
    extra_json TEXT NOT NULL DEFAULT '{}',
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_operation_logs_created_at ON operation_logs(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operation_logs_module_action ON operation_logs(module, action);
CREATE INDEX IF NOT EXISTS idx_operation_logs_uid_created_at ON operation_logs(uid, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operation_logs_job_id ON operation_logs(job_id);
CREATE INDEX IF NOT EXISTS idx_operation_logs_status ON operation_logs(status);

CREATE TABLE IF NOT EXISTS operation_jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    module TEXT NOT NULL,
    action TEXT NOT NULL,
    operator TEXT NOT NULL DEFAULT '',
    trigger_source TEXT NOT NULL DEFAULT 'manual',
    status TEXT NOT NULL DEFAULT 'queued',
    params_json TEXT NOT NULL DEFAULT '{}',
    summary_json TEXT NOT NULL DEFAULT '{}',
    total_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    fail_count INTEGER NOT NULL DEFAULT 0,
    skipped_count INTEGER NOT NULL DEFAULT 0,
    started_at REAL NOT NULL DEFAULT 0,
    finished_at REAL NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    last_error TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_operation_jobs_status_created_at ON operation_jobs(status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_operation_jobs_module_action ON operation_jobs(module, action);

CREATE TABLE IF NOT EXISTS operation_job_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    seq INTEGER NOT NULL DEFAULT 0,
    uid TEXT NOT NULL DEFAULT '',
    account_nick TEXT NOT NULL DEFAULT '',
    target_type TEXT NOT NULL DEFAULT '',
    target_id TEXT NOT NULL DEFAULT '',
    target_title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'queued',
    params_json TEXT NOT NULL DEFAULT '{}',
    result_json TEXT NOT NULL DEFAULT '{}',
    message TEXT NOT NULL DEFAULT '',
    error_code TEXT NOT NULL DEFAULT '',
    started_at REAL NOT NULL DEFAULT 0,
    finished_at REAL NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_operation_job_items_job_id_seq ON operation_job_items(job_id, seq);
CREATE INDEX IF NOT EXISTS idx_operation_job_items_job_id_status ON operation_job_items(job_id, status);
CREATE INDEX IF NOT EXISTS idx_operation_job_items_uid_created_at ON operation_job_items(uid, created_at DESC);

CREATE TABLE IF NOT EXISTS message_daily_stats (
    stat_date TEXT NOT NULL,
    uid TEXT NOT NULL,
    account_nick TEXT NOT NULL DEFAULT '',
    notification_message_count INTEGER NOT NULL DEFAULT 0,
    interaction_message_count INTEGER NOT NULL DEFAULT 0,
    received_message_count INTEGER NOT NULL DEFAULT 0,
    default_reply_count INTEGER NOT NULL DEFAULT 0,
    auto_reply_count INTEGER NOT NULL DEFAULT 0,
    ai_reply_count INTEGER NOT NULL DEFAULT 0,
    manual_reply_count INTEGER NOT NULL DEFAULT 0,
    image_reply_count INTEGER NOT NULL DEFAULT 0,
    auto_delivery_count INTEGER NOT NULL DEFAULT 0,
    reply_failure_count INTEGER NOT NULL DEFAULT 0,
    recall_count INTEGER NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    PRIMARY KEY (stat_date, uid)
);

CREATE INDEX IF NOT EXISTS idx_message_daily_stats_date_uid ON message_daily_stats(stat_date DESC, uid);
