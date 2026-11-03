CREATE TABLE IF NOT EXISTS feishu_im_reply_routes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    owner_username TEXT NOT NULL,
    feishu_message_id TEXT NOT NULL,
    chat_id TEXT NOT NULL DEFAULT '',
    uid TEXT NOT NULL,
    session_id TEXT NOT NULL,
    peer_uid TEXT NOT NULL DEFAULT '',
    customer_name TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    item_title TEXT NOT NULL DEFAULT '',
    xianyu_message_id TEXT NOT NULL DEFAULT '',
    xianyu_upstream_message_id TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT 0,
    expires_at REAL NOT NULL DEFAULT 0,
    UNIQUE(channel_id, feishu_message_id)
);

CREATE INDEX IF NOT EXISTS idx_feishu_im_reply_routes_chat_recent
ON feishu_im_reply_routes (channel_id, chat_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_feishu_im_reply_routes_owner_session
ON feishu_im_reply_routes (owner_username, uid, session_id);
