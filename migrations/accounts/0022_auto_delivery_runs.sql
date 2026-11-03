ALTER TABLE delivery_rules
ADD COLUMN fixed_message TEXT NOT NULL DEFAULT '';

ALTER TABLE delivery_rules
ADD COLUMN one_time_enabled INTEGER NOT NULL DEFAULT 1;

ALTER TABLE delivery_rules
ADD COLUMN needs_repair INTEGER NOT NULL DEFAULT 0;

ALTER TABLE cards
ADD COLUMN reserved_run_id INTEGER;

ALTER TABLE cards
ADD COLUMN reserved_order_id TEXT NOT NULL DEFAULT '';

ALTER TABLE cards
ADD COLUMN reserved_at INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS delivery_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uid TEXT NOT NULL,
    rule_id INTEGER NOT NULL,
    order_id TEXT NOT NULL,
    buyer_id TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL,
    purchase_quantity INTEGER,
    status TEXT NOT NULL DEFAULT 'pending',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    waiting_notice_sent_at INTEGER NOT NULL DEFAULT 0,
    admin_notified_at INTEGER NOT NULL DEFAULT 0,
    consigned_at INTEGER NOT NULL DEFAULT 0,
    message_sent_at INTEGER NOT NULL DEFAULT 0,
    completed_at INTEGER NOT NULL DEFAULT 0,
    next_retry_at INTEGER NOT NULL DEFAULT 0,
    last_error_code TEXT NOT NULL DEFAULT '',
    last_error_message TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE (uid, order_id)
);

CREATE TABLE IF NOT EXISTS delivery_run_cards (
    run_id INTEGER NOT NULL,
    card_id INTEGER NOT NULL,
    position INTEGER NOT NULL,
    content_snapshot TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'reserved',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, position),
    UNIQUE (card_id)
);

CREATE TABLE IF NOT EXISTS delivery_alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL UNIQUE,
    uid TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    system_notified_at INTEGER NOT NULL DEFAULT 0,
    external_notified_at INTEGER NOT NULL DEFAULT 0,
    resolved_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_delivery_runs_uid_status_retry
ON delivery_runs (uid, status, next_retry_at, updated_at);

CREATE INDEX IF NOT EXISTS idx_delivery_alerts_uid_status_updated
ON delivery_alerts (uid, status, updated_at DESC);
