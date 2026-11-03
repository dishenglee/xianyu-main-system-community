CREATE TABLE IF NOT EXISTS feishu_pending_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    owner_username TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    chat_id TEXT NOT NULL DEFAULT '',
    nonce TEXT NOT NULL,
    action_type TEXT NOT NULL,
    uid TEXT NOT NULL,
    target_id TEXT NOT NULL,
    args_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'pending',
    source_message_id TEXT NOT NULL DEFAULT '',
    created_at REAL NOT NULL DEFAULT 0,
    expires_at REAL NOT NULL DEFAULT 0,
    confirmed_at REAL NOT NULL DEFAULT 0,
    UNIQUE(channel_id, nonce)
);

CREATE INDEX IF NOT EXISTS idx_feishu_pending_actions_sender_status
ON feishu_pending_actions (channel_id, sender_id, status, expires_at DESC);

CREATE INDEX IF NOT EXISTS idx_feishu_pending_actions_owner_target
ON feishu_pending_actions (owner_username, action_type, uid, target_id);
