CREATE TABLE IF NOT EXISTS feishu_user_bindings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    owner_username TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    sender_name TEXT NOT NULL DEFAULT '',
    chat_id TEXT NOT NULL DEFAULT '',
    chat_type TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'im_command',
    last_message_id TEXT NOT NULL DEFAULT '',
    last_event_id TEXT NOT NULL DEFAULT '',
    last_seen_at REAL NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    UNIQUE(channel_id, sender_id)
);

CREATE INDEX IF NOT EXISTS idx_feishu_user_bindings_owner_updated
ON feishu_user_bindings (owner_username, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_feishu_user_bindings_sender
ON feishu_user_bindings (sender_id, channel_id);
