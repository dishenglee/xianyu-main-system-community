CREATE TABLE IF NOT EXISTS accounts (
    uid TEXT PRIMARY KEY,
    nick TEXT DEFAULT '',
    token TEXT DEFAULT '',
    sid TEXT DEFAULT '',
    x_umt TEXT DEFAULT '',
    cookies TEXT DEFAULT '{}',
    browser_cookie_jar TEXT DEFAULT '[]',
    app_info TEXT DEFAULT '700502@fleamarket_android_7.25.50',
    status TEXT DEFAULT 'active',
    created_at REAL DEFAULT 0,
    updated_at REAL DEFAULT 0,
    cookie_refreshed_at REAL DEFAULT 0,
    last_session_check_at REAL DEFAULT 0,
    last_session_ok_at REAL DEFAULT 0,
    first_session_fail_at REAL DEFAULT 0,
    last_session_status TEXT DEFAULT '',
    last_session_reason TEXT DEFAULT '',
    last_session_evidence TEXT DEFAULT '{}',
    extra TEXT DEFAULT '{}',
    security_score INTEGER DEFAULT -1,
    security_score_updated_at REAL DEFAULT 0,
    restriction_end TEXT DEFAULT '',
    coin_balance INTEGER DEFAULT 0,
    violation_count INTEGER DEFAULT 0,
    fish_power INTEGER DEFAULT 0,
    seller_30d_sold TEXT DEFAULT ''
);

CREATE TABLE IF NOT EXISTS items (
    item_id TEXT PRIMARY KEY,
    uid TEXT NOT NULL,
    title TEXT DEFAULT '',
    price TEXT DEFAULT '',
    original_price TEXT DEFAULT '',
    pic_url TEXT DEFAULT '',
    status INTEGER DEFAULT 0,
    online INTEGER DEFAULT 1,
    view_count INTEGER DEFAULT 0,
    want_count INTEGER DEFAULT 0,
    collect_num INTEGER DEFAULT 0,
    exposure_num TEXT DEFAULT '0',
    city TEXT DEFAULT '',
    category_id TEXT DEFAULT '',
    gmt_created TEXT DEFAULT '',
    first_modified_diff TEXT DEFAULT '',
    synced_at REAL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_items_uid ON items(uid);

CREATE TABLE IF NOT EXISTS orders (
    tid TEXT PRIMARY KEY,
    uid TEXT NOT NULL,
    title TEXT DEFAULT '',
    price TEXT DEFAULT '',
    pic_url TEXT DEFAULT '',
    status TEXT DEFAULT '',
    status_text TEXT DEFAULT '',
    buyer_nick TEXT DEFAULT '',
    gmt_create TEXT DEFAULT '',
    synced_at REAL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_orders_uid ON orders(uid);

CREATE TABLE IF NOT EXISTS keywords (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    keyword TEXT DEFAULT '',
    reply TEXT DEFAULT '',
    type TEXT DEFAULT 'text',
    item_id TEXT DEFAULT '',
    enabled INTEGER DEFAULT 1,
    created_at REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS item_replies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    reply TEXT DEFAULT '',
    type TEXT DEFAULT 'text',
    enabled INTEGER DEFAULT 1,
    created_at REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS cards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    card_key TEXT DEFAULT '',
    card_content TEXT DEFAULT '',
    card_pool_tag TEXT DEFAULT '',
    used INTEGER DEFAULT 0,
    used_at REAL DEFAULT 0,
    used_order_id TEXT DEFAULT '',
    created_at REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS delivery_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    keyword TEXT DEFAULT '',
    item_id TEXT DEFAULT '',
    card_pool_tag TEXT DEFAULT '',
    enabled INTEGER DEFAULT 1,
    created_at REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS ai_conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat_id TEXT NOT NULL,
    cookie_id TEXT NOT NULL,
    user_id TEXT DEFAULT '',
    item_id TEXT DEFAULT '',
    role TEXT DEFAULT 'user',
    content TEXT DEFAULT '',
    intent TEXT DEFAULT 'default',
    created_at REAL DEFAULT 0
);

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
    updated_at REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS biz_orders (
    order_id TEXT NOT NULL,
    cookie_id TEXT NOT NULL,
    item_id TEXT DEFAULT '',
    buyer_id TEXT DEFAULT '',
    amount TEXT DEFAULT '',
    status TEXT DEFAULT 'processing',
    last_shipped_at REAL DEFAULT 0,
    last_msg_at REAL DEFAULT 0,
    created_at REAL DEFAULT 0,
    updated_at REAL DEFAULT 0,
    PRIMARY KEY (cookie_id, order_id)
);

CREATE INDEX IF NOT EXISTS idx_keywords_cookie_created
ON keywords (cookie_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_keywords_cookie_item_enabled
ON keywords (cookie_id, item_id, enabled);

CREATE INDEX IF NOT EXISTS idx_item_replies_cookie_item_enabled
ON item_replies (cookie_id, item_id, enabled);

CREATE INDEX IF NOT EXISTS idx_item_replies_cookie_created
ON item_replies (cookie_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cards_cookie_used_created
ON cards (cookie_id, used, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cards_cookie_order
ON cards (cookie_id, used_order_id);

CREATE INDEX IF NOT EXISTS idx_cards_cookie_tag_used
ON cards (cookie_id, card_pool_tag, used, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_delivery_rules_cookie_enabled
ON delivery_rules (cookie_id, enabled);

CREATE INDEX IF NOT EXISTS idx_delivery_rules_cookie_keyword_item
ON delivery_rules (cookie_id, keyword, item_id);

CREATE INDEX IF NOT EXISTS idx_ai_conversations_chat_created
ON ai_conversations (chat_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_ai_conversations_cookie_item_created
ON ai_conversations (cookie_id, item_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_ai_reply_settings_updated_at
ON ai_reply_settings (updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_biz_orders_cookie_status_updated
ON biz_orders (cookie_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_biz_orders_item_buyer
ON biz_orders (cookie_id, item_id, buyer_id);

CREATE TABLE IF NOT EXISTS service_runtime_state (
    service_name TEXT PRIMARY KEY,
    version TEXT NOT NULL DEFAULT '',
    started_at TEXT NOT NULL DEFAULT '',
    startup_count INTEGER NOT NULL DEFAULT 0,
    last_ready_at TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS auth_login_audit (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL,
    legacy INTEGER NOT NULL DEFAULT 0,
    token_kind TEXT NOT NULL DEFAULT 'jwt',
    issued_at REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_auth_login_audit_username_issued_at
ON auth_login_audit (username, issued_at DESC);
