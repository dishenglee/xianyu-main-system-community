CREATE TABLE IF NOT EXISTS feishu_session_group_chats (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    owner_username TEXT NOT NULL DEFAULT '',
    uid TEXT NOT NULL,
    session_id TEXT NOT NULL,
    peer_uid TEXT NOT NULL DEFAULT '',
    customer_name TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    item_title TEXT NOT NULL DEFAULT '',
    item_pic TEXT NOT NULL DEFAULT '',
    item_price TEXT NOT NULL DEFAULT '',
    feishu_chat_id TEXT NOT NULL DEFAULT '',
    feishu_chat_name TEXT NOT NULL DEFAULT '',
    feishu_operator_sender_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active',
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    last_message_ts REAL NOT NULL DEFAULT 0,
    closed_at REAL NOT NULL DEFAULT 0,
    UNIQUE(channel_id, session_id)
);

CREATE INDEX IF NOT EXISTS idx_feishu_session_group_chats_owner_session
ON feishu_session_group_chats (owner_username, uid, session_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_feishu_session_group_chats_channel_chat
ON feishu_session_group_chats (channel_id, feishu_chat_id)
WHERE feishu_chat_id <> '';
