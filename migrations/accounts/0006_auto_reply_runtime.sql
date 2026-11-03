CREATE TABLE IF NOT EXISTS ai_conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    item_id TEXT NOT NULL DEFAULT '',
    user_id TEXT NOT NULL DEFAULT '',
    role TEXT NOT NULL DEFAULT 'user',
    intent TEXT NOT NULL DEFAULT 'default',
    content TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_ai_conversations_chat_created
ON ai_conversations (cookie_id, chat_id, created_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS default_reply_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    item_id TEXT NOT NULL DEFAULT '',
    reply_type TEXT NOT NULL DEFAULT 'default',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_default_reply_records_unique
ON default_reply_records (cookie_id, chat_id, item_id, reply_type);

CREATE TABLE IF NOT EXISTS automation_message_dedup (
    message_key TEXT PRIMARY KEY,
    cookie_id TEXT NOT NULL,
    session_id TEXT NOT NULL DEFAULT '',
    peer_uid TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_automation_message_dedup_created
ON automation_message_dedup (created_at DESC);
