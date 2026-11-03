CREATE TABLE IF NOT EXISTS delivery_discovery_alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    alert_key TEXT NOT NULL UNIQUE,
    alert_type TEXT NOT NULL,
    uid TEXT NOT NULL,
    rule_id INTEGER,
    order_id TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    order_status TEXT NOT NULL DEFAULT '',
    order_status_text TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open',
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    occurrence_count INTEGER NOT NULL DEFAULT 1,
    first_seen_at INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    resolved_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_delivery_discovery_alerts_uid_status_updated
ON delivery_discovery_alerts (uid, status, updated_at DESC, id DESC);
