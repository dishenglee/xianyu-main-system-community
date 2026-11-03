DROP TABLE IF EXISTS comment_targets_owner_scope_migration;
ALTER TABLE comment_targets RENAME TO comment_targets_owner_scope_migration;

CREATE TABLE comment_targets (
    owner_username TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL,
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
    updated_at REAL DEFAULT 0,
    PRIMARY KEY (owner_username, item_id)
);

INSERT INTO comment_targets (
    owner_username, item_id, title, price, pic_url, item_url, seller_id, seller_nick,
    source, status, selected_at, created_at, updated_at
)
SELECT
    '', item_id, title, price, pic_url, item_url, seller_id, seller_nick,
    source, status, selected_at, created_at, updated_at
FROM comment_targets_owner_scope_migration;

DROP TABLE comment_targets_owner_scope_migration;

CREATE INDEX IF NOT EXISTS idx_comment_targets_owner_status
ON comment_targets(owner_username, status);

CREATE INDEX IF NOT EXISTS idx_comment_targets_owner_selected_at
ON comment_targets(owner_username, selected_at DESC);

CREATE INDEX IF NOT EXISTS idx_comment_targets_owner_status_selected_at
ON comment_targets(owner_username, status, selected_at DESC);

DROP TABLE IF EXISTS comment_library_owner_scope_migration;
ALTER TABLE comment_library RENAME TO comment_library_owner_scope_migration;

CREATE TABLE comment_library (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_username TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    content_hash TEXT NOT NULL DEFAULT '',
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
    updated_at REAL NOT NULL DEFAULT 0,
    UNIQUE(owner_username, content_hash)
);

INSERT INTO comment_library (
    id, owner_username, content, content_hash, source, source_type, source_label,
    source_ref, source_item_id, enabled, usage_count, success_count, last_used_at,
    created_at, updated_at
)
SELECT
    id, '', content, content_hash, source, source_type, source_label,
    source_ref, source_item_id, enabled, usage_count, success_count, last_used_at,
    created_at, updated_at
FROM comment_library_owner_scope_migration;

DROP TABLE comment_library_owner_scope_migration;

CREATE UNIQUE INDEX IF NOT EXISTS idx_comment_library_owner_hash
ON comment_library(owner_username, content_hash);

CREATE INDEX IF NOT EXISTS idx_comment_library_owner_enabled_updated
ON comment_library(owner_username, enabled, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_comment_library_owner_source
ON comment_library(owner_username, source, source_type, updated_at DESC);
