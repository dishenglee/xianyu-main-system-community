CREATE TABLE IF NOT EXISTS item_comment_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uid TEXT NOT NULL DEFAULT '',
    item_id TEXT NOT NULL DEFAULT '',
    source_api TEXT NOT NULL DEFAULT '',
    source_type TEXT NOT NULL DEFAULT '',
    source_comment_key TEXT NOT NULL DEFAULT '',
    comment_id TEXT NOT NULL DEFAULT '',
    author_id TEXT NOT NULL DEFAULT '',
    author_nick TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    content_hash TEXT NOT NULL DEFAULT '',
    published_at TEXT NOT NULL DEFAULT '',
    category TEXT NOT NULL DEFAULT '其他',
    quality_score REAL NOT NULL DEFAULT 0,
    quality_label TEXT NOT NULL DEFAULT 'bad',
    score_breakdown_json TEXT NOT NULL DEFAULT '{}',
    raw_json TEXT NOT NULL DEFAULT '{}',
    snapshot_time REAL NOT NULL DEFAULT 0,
    accepted_for_txt INTEGER NOT NULL DEFAULT 0,
    txt_exported_at REAL NOT NULL DEFAULT 0,
    txt_export_path TEXT NOT NULL DEFAULT '',
    txt_imported_at REAL NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0,
    UNIQUE(uid, item_id, source_type, source_comment_key)
);

CREATE INDEX IF NOT EXISTS idx_comment_snapshots_item
ON item_comment_snapshots(item_id, snapshot_time DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_comment_snapshots_quality
ON item_comment_snapshots(quality_label, quality_score DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_comment_snapshots_source
ON item_comment_snapshots(source_type, source_api, snapshot_time DESC);

CREATE INDEX IF NOT EXISTS idx_comment_snapshots_category
ON item_comment_snapshots(category, quality_label, id DESC);

CREATE INDEX IF NOT EXISTS idx_comment_snapshots_hash
ON item_comment_snapshots(uid, content_hash, id DESC);
