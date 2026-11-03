CREATE TABLE IF NOT EXISTS comment_targets (
    item_id TEXT PRIMARY KEY,
    title TEXT DEFAULT '',
    price TEXT DEFAULT '',
    pic_url TEXT DEFAULT '',
    item_url TEXT DEFAULT '',
    seller_id TEXT DEFAULT '',
    seller_nick TEXT DEFAULT '',
    source TEXT DEFAULT 'search',
    status TEXT DEFAULT 'pending',
    selected_at REAL DEFAULT 0,
    created_at REAL DEFAULT 0,
    updated_at REAL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_comment_targets_status
ON comment_targets(status);

CREATE INDEX IF NOT EXISTS idx_comment_targets_selected_at
ON comment_targets(selected_at DESC);

CREATE INDEX IF NOT EXISTS idx_comment_targets_status_selected_at
ON comment_targets(status, selected_at DESC);

CREATE TABLE IF NOT EXISTS comment_library (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL DEFAULT '',
    content_hash TEXT NOT NULL UNIQUE DEFAULT '',
    source TEXT NOT NULL DEFAULT 'manual',
    source_type TEXT NOT NULL DEFAULT '',
    source_label TEXT NOT NULL DEFAULT '',
    source_ref TEXT NOT NULL DEFAULT '',
    source_item_id TEXT NOT NULL DEFAULT '',
    enabled INTEGER NOT NULL DEFAULT 1,
    usage_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    last_used_at REAL NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_comment_library_hash
ON comment_library(content_hash);

CREATE INDEX IF NOT EXISTS idx_comment_library_enabled_updated
ON comment_library(enabled, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_comment_library_source
ON comment_library(source, source_type, updated_at DESC);
