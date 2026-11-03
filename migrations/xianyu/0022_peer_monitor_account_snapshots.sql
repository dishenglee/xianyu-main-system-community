-- A peer monitor target is a seller account.  The item that led us to the
-- account is retained as discovery context only; it is not a monitored item.
ALTER TABLE peer_monitor_targets ADD COLUMN source_item_title TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_targets ADD COLUMN source_item_price TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_targets ADD COLUMN source_item_image_url TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_targets ADD COLUMN profile_json TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_targets ADD COLUMN profile_observed_at REAL;
ALTER TABLE peer_monitor_targets ADD COLUMN last_success_at REAL;
ALTER TABLE peer_monitor_targets ADD COLUMN refresh_started_at REAL;

-- This table used to contain only the product that introduced the seller.
-- Keep existing rows for backwards compatibility, but record complete account
-- snapshots from now on and distinguish active items from historical ones.
ALTER TABLE peer_monitor_item_snapshots ADD COLUMN status TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_item_snapshots ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1;
ALTER TABLE peer_monitor_item_snapshots ADD COLUMN first_observed_at REAL NOT NULL DEFAULT 0;
ALTER TABLE peer_monitor_item_snapshots ADD COLUMN last_observed_at REAL NOT NULL DEFAULT 0;

UPDATE peer_monitor_item_snapshots
SET first_observed_at = observed_at,
    last_observed_at = observed_at
WHERE first_observed_at = 0 OR last_observed_at = 0;

ALTER TABLE peer_monitor_events ADD COLUMN entity_type TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_events ADD COLUMN entity_id TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_events ADD COLUMN previous_json TEXT NOT NULL DEFAULT '';
ALTER TABLE peer_monitor_events ADD COLUMN current_json TEXT NOT NULL DEFAULT '';

ALTER TABLE peer_monitor_runs ADD COLUMN trigger TEXT NOT NULL DEFAULT 'manual';
ALTER TABLE peer_monitor_runs ADD COLUMN started_at REAL;
ALTER TABLE peer_monitor_runs ADD COLUMN finished_at REAL;
ALTER TABLE peer_monitor_runs ADD COLUMN profile_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE peer_monitor_runs ADD COLUMN item_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE peer_monitor_runs ADD COLUMN timeline_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE peer_monitor_runs ADD COLUMN rate_count INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS peer_monitor_profile_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL REFERENCES peer_monitor_targets(id) ON DELETE CASCADE,
    profile_json TEXT NOT NULL,
    observed_at REAL NOT NULL DEFAULT (unixepoch('subsec'))
);

CREATE TABLE IF NOT EXISTS peer_monitor_timeline_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL REFERENCES peer_monitor_targets(id) ON DELETE CASCADE,
    event_key TEXT NOT NULL,
    item_id TEXT NOT NULL DEFAULT '',
    trade_id TEXT NOT NULL DEFAULT '',
    event_type TEXT NOT NULL DEFAULT '',
    event_date TEXT NOT NULL DEFAULT '',
    title TEXT NOT NULL DEFAULT '',
    price TEXT NOT NULL DEFAULT '',
    image_url TEXT NOT NULL DEFAULT '',
    first_observed_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    last_observed_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    UNIQUE(target_id, event_key)
);

CREATE TABLE IF NOT EXISTS peer_monitor_rate_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    target_id INTEGER NOT NULL REFERENCES peer_monitor_targets(id) ON DELETE CASCADE,
    rate_id TEXT NOT NULL,
    trade_id TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    item_title TEXT NOT NULL DEFAULT '',
    item_price TEXT NOT NULL DEFAULT '',
    item_image_url TEXT NOT NULL DEFAULT '',
    rater_nick TEXT NOT NULL DEFAULT '',
    feedback TEXT NOT NULL DEFAULT '',
    created_at_text TEXT NOT NULL DEFAULT '',
    rate_json TEXT NOT NULL DEFAULT '',
    first_observed_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    last_observed_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    UNIQUE(target_id, rate_id)
);

CREATE INDEX IF NOT EXISTS idx_peer_monitor_targets_due
ON peer_monitor_targets(enabled, last_refreshed_at, refresh_started_at);
CREATE INDEX IF NOT EXISTS idx_peer_monitor_items_active
ON peer_monitor_item_snapshots(target_id, is_active, last_observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_peer_monitor_timeline_target
ON peer_monitor_timeline_snapshots(target_id, last_observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_peer_monitor_rates_target
ON peer_monitor_rate_snapshots(target_id, last_observed_at DESC);
CREATE INDEX IF NOT EXISTS idx_peer_monitor_profile_target
ON peer_monitor_profile_snapshots(target_id, observed_at DESC);
