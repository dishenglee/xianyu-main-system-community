CREATE TABLE IF NOT EXISTS comment_tasks (
    task_id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'draft',
    source TEXT NOT NULL DEFAULT 'comment_targets',
    target_total INTEGER NOT NULL DEFAULT 0,
    account_total INTEGER NOT NULL DEFAULT 0,
    account_uids_json TEXT NOT NULL DEFAULT '[]',
    comment_templates_json TEXT NOT NULL DEFAULT '[]',
    publish_interval_min_seconds REAL NOT NULL DEFAULT 0,
    publish_interval_max_seconds REAL NOT NULL DEFAULT 0,
    delete_interval_min_seconds REAL NOT NULL DEFAULT 0,
    delete_interval_max_seconds REAL NOT NULL DEFAULT 0,
    delete_after_seconds REAL NOT NULL DEFAULT 3600,
    enable_delete_comments INTEGER NOT NULL DEFAULT 1,
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    started_at REAL NOT NULL DEFAULT 0,
    paused_at REAL NOT NULL DEFAULT 0,
    finished_at REAL NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    last_transition_at REAL NOT NULL DEFAULT 0,
    last_log_at REAL NOT NULL DEFAULT 0,
    completed_targets INTEGER NOT NULL DEFAULT 0,
    successful_targets INTEGER NOT NULL DEFAULT 0,
    failed_targets INTEGER NOT NULL DEFAULT 0,
    skipped_targets INTEGER NOT NULL DEFAULT 0,
    pending_targets INTEGER NOT NULL DEFAULT 0,
    current_target_id TEXT NOT NULL DEFAULT '',
    current_account_uid TEXT NOT NULL DEFAULT '',
    last_message TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_comment_tasks_status_updated
ON comment_tasks(status, updated_at DESC);

CREATE TABLE IF NOT EXISTS comment_task_targets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    title TEXT NOT NULL DEFAULT '',
    price TEXT NOT NULL DEFAULT '',
    seller_id TEXT NOT NULL DEFAULT '',
    seller_nick TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    UNIQUE(task_id, item_id)
);

CREATE INDEX IF NOT EXISTS idx_comment_task_targets_task_position
ON comment_task_targets(task_id, position ASC);

CREATE TABLE IF NOT EXISTS comment_task_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    level TEXT NOT NULL DEFAULT 'info',
    event TEXT NOT NULL DEFAULT 'info',
    message TEXT NOT NULL DEFAULT '',
    context_json TEXT NOT NULL DEFAULT '{}',
    account_uid TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_comment_task_logs_task_created
ON comment_task_logs(task_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS comment_task_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    account_uid TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    target_status TEXT NOT NULL DEFAULT 'pending',
    success INTEGER NOT NULL DEFAULT 0,
    comment_content TEXT NOT NULL DEFAULT '',
    comment_content_hash TEXT NOT NULL DEFAULT '',
    source_comment_json TEXT NOT NULL DEFAULT '{}',
    published_comment_id TEXT NOT NULL DEFAULT '',
    published_at REAL NOT NULL DEFAULT 0,
    failure_reason_code TEXT NOT NULL DEFAULT '',
    failure_reason_message TEXT NOT NULL DEFAULT '',
    failure_scope TEXT NOT NULL DEFAULT '',
    request_payload_json TEXT NOT NULL DEFAULT '{}',
    response_payload_json TEXT NOT NULL DEFAULT '{}',
    delete_job_payload_json TEXT NOT NULL DEFAULT '{}',
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_comment_task_attempts_task_created
ON comment_task_attempts(task_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS comment_delete_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL,
    attempt_id INTEGER NOT NULL DEFAULT 0,
    account_uid TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    comment_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    payload_json TEXT NOT NULL DEFAULT '{}',
    scheduled_at REAL NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    started_at REAL NOT NULL DEFAULT 0,
    finished_at REAL NOT NULL DEFAULT 0,
    cancelled_at REAL NOT NULL DEFAULT 0,
    last_error_at REAL NOT NULL DEFAULT 0,
    last_error_code TEXT NOT NULL DEFAULT '',
    last_error_message TEXT NOT NULL DEFAULT '',
    claimed_by TEXT NOT NULL DEFAULT '',
    claim_token TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_comment_delete_jobs_task_status
ON comment_delete_jobs(task_id, status, scheduled_at ASC, id ASC);
