CREATE TABLE IF NOT EXISTS feishu_event_receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    event_id TEXT NOT NULL,
    message_id TEXT NOT NULL DEFAULT '',
    chat_id TEXT NOT NULL DEFAULT '',
    sender_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'processing',
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    UNIQUE(channel_id, event_id)
);

CREATE INDEX IF NOT EXISTS idx_feishu_event_receipts_channel_updated
ON feishu_event_receipts (channel_id, updated_at DESC, id DESC);
