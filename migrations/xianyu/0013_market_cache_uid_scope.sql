DROP INDEX IF EXISTS idx_account_items_uid_status_updated;
DROP INDEX IF EXISTS idx_account_orders_uid_updated;
DROP INDEX IF EXISTS idx_account_orders_uid_buyer_item_status;

ALTER TABLE account_items RENAME TO account_items_old_0013;

CREATE TABLE account_items (
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
    actions_json TEXT NOT NULL DEFAULT '[]',
    coin_opened_at REAL NOT NULL DEFAULT 0,
    PRIMARY KEY (uid, item_id)
);

INSERT OR REPLACE INTO account_items (
    item_id, uid, title, short_title, description, price, orig_price, pic_url,
    image_urls_json, status, online, want_count, view_count, exposure_num,
    coin_open, coin_ratio, city, province, area, division_id, latitude, longitude,
    fish_ad_code, item_context_json, detail_json, created_at, updated_at,
    actions_json, coin_opened_at
)
SELECT
    item_id, uid, title, short_title, description, price, orig_price, pic_url,
    image_urls_json, status, online, want_count, view_count, exposure_num,
    coin_open, coin_ratio, city, province, area, division_id, latitude, longitude,
    fish_ad_code, item_context_json, detail_json, created_at, updated_at,
    actions_json, coin_opened_at
FROM account_items_old_0013
WHERE TRIM(uid) <> '' AND TRIM(item_id) <> '';

DROP TABLE account_items_old_0013;

CREATE INDEX IF NOT EXISTS idx_account_items_uid_status_updated
ON account_items(uid, status, updated_at DESC);

ALTER TABLE account_orders RENAME TO account_orders_old_0013;

CREATE TABLE account_orders (
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
    item_id TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (uid, tid)
);

INSERT OR REPLACE INTO account_orders (
    tid, uid, title, price, pic_url, status, status_text, buyer_id, buyer_nick,
    buyer_avatar, gmt_create, express_company_code, express_company_name, mail_no,
    created_at, updated_at, item_id
)
SELECT
    tid, uid, title, price, pic_url, status, status_text, buyer_id, buyer_nick,
    buyer_avatar, gmt_create, express_company_code, express_company_name, mail_no,
    created_at, updated_at, item_id
FROM account_orders_old_0013
WHERE TRIM(uid) <> '' AND TRIM(tid) <> '';

DROP TABLE account_orders_old_0013;

CREATE INDEX IF NOT EXISTS idx_account_orders_uid_updated
ON account_orders(uid, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_account_orders_uid_buyer_item_status
ON account_orders(uid, buyer_id, item_id, updated_at DESC);
