CREATE TABLE IF NOT EXISTS cache_sync_runtime_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    status TEXT NOT NULL DEFAULT 'idle',
    cached_uids_json TEXT NOT NULL DEFAULT '[]',
    total_entries INTEGER NOT NULL DEFAULT 0,
    last_sync_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS comment_delete_runtime_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    running_job_ids_json TEXT NOT NULL DEFAULT '[]',
    running_count INTEGER NOT NULL DEFAULT 0,
    pending_count INTEGER NOT NULL DEFAULT 0,
    retryable_count INTEGER NOT NULL DEFAULT 0,
    persisted_running_count INTEGER NOT NULL DEFAULT 0,
    runtime_ready INTEGER NOT NULL DEFAULT 0,
    last_poll_at REAL NOT NULL DEFAULT 0,
    last_runtime_error TEXT NOT NULL DEFAULT '',
    last_recovered_count INTEGER NOT NULL DEFAULT 0,
    claim_batch_size INTEGER NOT NULL DEFAULT 0,
    action_capacity INTEGER NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);
