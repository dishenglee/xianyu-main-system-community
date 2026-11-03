DROP INDEX IF EXISTS idx_biz_orders_cookie_status_updated;
DROP INDEX IF EXISTS idx_biz_orders_item_buyer;

ALTER TABLE biz_orders RENAME TO biz_orders_old_0011;

CREATE TABLE biz_orders (
    order_id TEXT NOT NULL,
    cookie_id TEXT NOT NULL,
    item_id TEXT DEFAULT '',
    buyer_id TEXT DEFAULT '',
    amount TEXT DEFAULT '',
    status TEXT DEFAULT 'processing',
    last_shipped_at INTEGER DEFAULT 0,
    last_msg_at INTEGER DEFAULT 0,
    created_at INTEGER DEFAULT 0,
    updated_at INTEGER DEFAULT 0,
    PRIMARY KEY (cookie_id, order_id)
);

INSERT OR REPLACE INTO biz_orders (
    order_id,
    cookie_id,
    item_id,
    buyer_id,
    amount,
    status,
    last_shipped_at,
    last_msg_at,
    created_at,
    updated_at
)
SELECT
    order_id,
    cookie_id,
    item_id,
    buyer_id,
    amount,
    status,
    last_shipped_at,
    last_msg_at,
    created_at,
    updated_at
FROM biz_orders_old_0011
WHERE TRIM(cookie_id) <> '' AND TRIM(order_id) <> '';

DROP TABLE biz_orders_old_0011;

CREATE INDEX IF NOT EXISTS idx_biz_orders_cookie_status_updated
ON biz_orders (cookie_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_biz_orders_item_buyer
ON biz_orders (cookie_id, item_id, buyer_id);
