CREATE TABLE IF NOT EXISTS account_items (
    item_id TEXT NOT NULL,
    uid TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    short_title TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    price TEXT NOT NULL DEFAULT '',
    orig_price TEXT NOT NULL DEFAULT '',
    pic_url TEXT NOT NULL DEFAULT '',
    image_urls_json TEXT NOT NULL DEFAULT '[]',
    status TEXT NOT NULL DEFAULT 'ON_SALE',
    online INTEGER NOT NULL DEFAULT 1,
    want_count INTEGER NOT NULL DEFAULT 0,
    view_count INTEGER NOT NULL DEFAULT 0,
    exposure_num INTEGER NOT NULL DEFAULT 0,
    coin_open INTEGER NOT NULL DEFAULT 0,
    coin_ratio TEXT NOT NULL DEFAULT '',
    city TEXT NOT NULL DEFAULT '',
    province TEXT NOT NULL DEFAULT '',
    area TEXT NOT NULL DEFAULT '',
    division_id TEXT NOT NULL DEFAULT '',
    latitude TEXT NOT NULL DEFAULT '',
    longitude TEXT NOT NULL DEFAULT '',
    fish_ad_code TEXT NOT NULL DEFAULT '',
    item_context_json TEXT NOT NULL DEFAULT '{}',
    detail_json TEXT NOT NULL DEFAULT '{}',
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (uid, item_id)
);

CREATE INDEX IF NOT EXISTS idx_account_items_uid_status_updated
ON account_items(uid, status, updated_at DESC);

CREATE TABLE IF NOT EXISTS account_orders (
    tid TEXT NOT NULL,
    uid TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT '',
    price TEXT NOT NULL DEFAULT '',
    pic_url TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT '',
    status_text TEXT NOT NULL DEFAULT '',
    buyer_id TEXT NOT NULL DEFAULT '',
    buyer_nick TEXT NOT NULL DEFAULT '',
    buyer_avatar TEXT NOT NULL DEFAULT '',
    gmt_create TEXT NOT NULL DEFAULT '',
    express_company_code TEXT NOT NULL DEFAULT '',
    express_company_name TEXT NOT NULL DEFAULT '',
    mail_no TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (uid, tid)
);

CREATE INDEX IF NOT EXISTS idx_account_orders_uid_updated
ON account_orders(uid, updated_at DESC);

CREATE TABLE IF NOT EXISTS item_comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uid TEXT NOT NULL,
    item_id TEXT NOT NULL,
    comment_id TEXT NOT NULL DEFAULT '',
    author_id TEXT NOT NULL DEFAULT '',
    author_nick TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    published_at REAL NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_item_comments_uid_item_created
ON item_comments(uid, item_id, created_at DESC, id DESC);
