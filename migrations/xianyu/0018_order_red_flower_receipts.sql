CREATE TABLE IF NOT EXISTS order_red_flower_receipts (
    uid TEXT NOT NULL,
    order_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'success',
    upstream_ret TEXT NOT NULL DEFAULT '',
    received_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (uid, order_id)
);
