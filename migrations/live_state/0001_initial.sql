CREATE TABLE IF NOT EXISTS live_connection_state (
    uid TEXT PRIMARY KEY,
    status TEXT DEFAULT '',
    last_connected_at REAL DEFAULT 0,
    last_disconnected_at REAL DEFAULT 0,
    last_error TEXT DEFAULT '',
    reconnect_attempts INTEGER DEFAULT 0,
    updated_at REAL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS message_live_state (
    uid TEXT NOT NULL,
    session_id TEXT NOT NULL,
    session_type INTEGER DEFAULT 1,
    latest_version INTEGER DEFAULT 0,
    latest_message_id TEXT DEFAULT '',
    unread_count INTEGER DEFAULT 0,
    peer_uid TEXT DEFAULT '',
    peer_nick TEXT DEFAULT '',
    peer_role TEXT DEFAULT '',
    peer_logo TEXT DEFAULT '',
    self_role TEXT DEFAULT '',
    item_id TEXT DEFAULT '',
    item_title TEXT DEFAULT '',
    item_pic TEXT DEFAULT '',
    summary TEXT DEFAULT '',
    last_message_ts INTEGER DEFAULT 0,
    last_sync_at REAL DEFAULT 0,
    last_ws_event_at REAL DEFAULT 0,
    last_http_compensate_at REAL DEFAULT 0,
    updated_at REAL DEFAULT 0,
    PRIMARY KEY (uid, session_id)
);

CREATE TABLE IF NOT EXISTS message_live_event (
    uid TEXT NOT NULL,
    session_id TEXT NOT NULL,
    session_type INTEGER DEFAULT 1,
    message_id TEXT DEFAULT '',
    version INTEGER DEFAULT 0,
    ts INTEGER DEFAULT 0,
    kind TEXT DEFAULT 'text',
    title TEXT DEFAULT '',
    text TEXT DEFAULT '',
    image_url TEXT DEFAULT '',
    preview TEXT DEFAULT '',
    sender_uid TEXT DEFAULT '',
    sender_nick TEXT DEFAULT '',
    sender_logo TEXT DEFAULT '',
    is_self INTEGER DEFAULT 0,
    peer_uid TEXT DEFAULT '',
    peer_nick TEXT DEFAULT '',
    peer_logo TEXT DEFAULT '',
    item_id TEXT DEFAULT '',
    item_title TEXT DEFAULT '',
    item_pic TEXT DEFAULT '',
    updated_at REAL DEFAULT 0,
    PRIMARY KEY (uid, session_id, version, message_id)
);

CREATE TABLE IF NOT EXISTS message_history_cache (
    uid TEXT NOT NULL,
    session_id TEXT NOT NULL,
    session_type INTEGER DEFAULT 1,
    message_id TEXT DEFAULT '',
    version INTEGER DEFAULT 0,
    ts INTEGER DEFAULT 0,
    kind TEXT DEFAULT 'text',
    title TEXT DEFAULT '',
    text TEXT DEFAULT '',
    image_url TEXT DEFAULT '',
    image_urls TEXT DEFAULT '',
    content_type INTEGER DEFAULT 0,
    preview TEXT DEFAULT '',
    sender_nick TEXT DEFAULT '',
    sender_logo TEXT DEFAULT '',
    is_self INTEGER DEFAULT 0,
    cached_at REAL DEFAULT 0,
    PRIMARY KEY (uid, session_id, version, message_id)
);

CREATE INDEX IF NOT EXISTS idx_history_cache_session
ON message_history_cache (uid, session_id, version ASC);

CREATE INDEX IF NOT EXISTS idx_live_state_uid_version
ON message_live_state (uid, latest_version DESC, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_live_event_uid_session_version
ON message_live_event (uid, session_id, version ASC, ts ASC);

CREATE INDEX IF NOT EXISTS idx_history_cache_cached_at
ON message_history_cache (cached_at);

CREATE INDEX IF NOT EXISTS idx_live_event_updated_at
ON message_live_event (updated_at);
