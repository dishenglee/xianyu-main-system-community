CREATE TABLE IF NOT EXISTS peer_monitor_targets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_username TEXT NOT NULL,
    seller_id TEXT NOT NULL,
    seller_name TEXT NOT NULL DEFAULT '',
    seller_avatar TEXT NOT NULL DEFAULT '',
    source_item_id TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    baseline_created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    last_refreshed_at REAL,
    last_error TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    UNIQUE(owner_username, seller_id)
);

CREATE TABLE IF NOT EXISTS peer_monitor_item_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL REFERENCES peer_monitor_targets(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    price TEXT NOT NULL DEFAULT '',
    image_url TEXT NOT NULL DEFAULT '',
    observed_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    UNIQUE(target_id, item_id)
);

CREATE TABLE IF NOT EXISTS peer_monitor_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL REFERENCES peer_monitor_targets(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE TABLE IF NOT EXISTS peer_monitor_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL REFERENCES peer_monitor_targets(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE INDEX IF NOT EXISTS idx_peer_monitor_targets_owner ON peer_monitor_targets(owner_username, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_peer_monitor_events_target ON peer_monitor_events(target_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_peer_monitor_runs_target ON peer_monitor_runs(target_id, created_at DESC);
