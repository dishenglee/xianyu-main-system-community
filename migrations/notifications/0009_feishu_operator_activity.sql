CREATE TABLE IF NOT EXISTS feishu_operator_activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    owner_username TEXT NOT NULL DEFAULT '',
    sender_id TEXT NOT NULL,
    sender_name TEXT NOT NULL DEFAULT '',
    last_chat_id TEXT NOT NULL DEFAULT '',
    last_event_id TEXT NOT NULL DEFAULT '',
    last_active_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    UNIQUE(channel_id, owner_username, sender_id)
);

CREATE INDEX IF NOT EXISTS idx_feishu_operator_activity_owner_latest
ON feishu_operator_activity (channel_id, owner_username, last_active_at DESC, id DESC);
