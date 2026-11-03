CREATE TABLE IF NOT EXISTS automation_schedules (
    id TEXT PRIMARY KEY,
    owner_username TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL,
    action TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    time_of_day TEXT NOT NULL DEFAULT '09:00',
    timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
    account_scope TEXT NOT NULL DEFAULT 'all',
    account_uids_json TEXT NOT NULL DEFAULT '[]',
    item_scope TEXT NOT NULL DEFAULT 'all',
    item_ids_json TEXT NOT NULL DEFAULT '[]',
    config_json TEXT NOT NULL DEFAULT '{}',
    last_run_at REAL NOT NULL DEFAULT 0,
    last_status TEXT NOT NULL DEFAULT '',
    last_message TEXT NOT NULL DEFAULT '',
    next_run_at REAL NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_automation_schedules_owner_updated ON automation_schedules(owner_username, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_automation_schedules_due ON automation_schedules(enabled, next_run_at);
