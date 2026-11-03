CREATE TABLE IF NOT EXISTS data_compass_daily_metrics (
    uid TEXT NOT NULL,
    stat_date TEXT NOT NULL,
    pay_amount REAL NOT NULL DEFAULT 0,
    pay_order_count INTEGER NOT NULL DEFAULT 0,
    refund_amount REAL NOT NULL DEFAULT 0,
    refund_order_count INTEGER NOT NULL DEFAULT 0,
    source_synced_at REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (uid, stat_date)
);

CREATE INDEX IF NOT EXISTS idx_data_compass_daily_metrics_date_uid
ON data_compass_daily_metrics(stat_date DESC, uid);

CREATE TABLE IF NOT EXISTS data_compass_sync_state (
    uid TEXT PRIMARY KEY,
    last_attempt_at REAL NOT NULL DEFAULT 0,
    last_success_at REAL NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT ''
);
