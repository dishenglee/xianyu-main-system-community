CREATE TABLE IF NOT EXISTS service_worker_state (
    worker_name TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT '',
    tick_count INTEGER NOT NULL DEFAULT 0,
    last_tick_at TEXT NOT NULL DEFAULT '',
    last_error TEXT NOT NULL DEFAULT '',
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_service_worker_state_status_updated
ON service_worker_state (status, updated_at DESC);
