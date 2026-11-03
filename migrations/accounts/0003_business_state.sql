CREATE TABLE IF NOT EXISTS ai_reply_settings (
    cookie_id TEXT PRIMARY KEY,
    ai_enabled INTEGER DEFAULT 0,
    auto_reply_enabled INTEGER DEFAULT 0,
    auto_ship_enabled INTEGER DEFAULT 0,
    api_type TEXT DEFAULT 'openai',
    api_key TEXT DEFAULT '',
    base_url TEXT DEFAULT '',
    model_name TEXT DEFAULT '',
    max_bargain_rounds INTEGER DEFAULT 0,
    max_discount_percent REAL DEFAULT 0,
    max_discount_amount REAL DEFAULT 0,
    custom_prompts TEXT DEFAULT '{}',
    updated_at INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_ai_reply_settings_updated_at
ON ai_reply_settings (updated_at DESC);

CREATE TABLE IF NOT EXISTS biz_orders (
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

CREATE INDEX IF NOT EXISTS idx_biz_orders_cookie_status_updated
ON biz_orders (cookie_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_biz_orders_item_buyer
ON biz_orders (cookie_id, item_id, buyer_id);
