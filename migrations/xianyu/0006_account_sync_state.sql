CREATE TABLE IF NOT EXISTS account_sync_state (
    uid TEXT NOT NULL,
    kind TEXT NOT NULL,
    last_synced_at REAL NOT NULL DEFAULT 0,
    last_ok INTEGER NOT NULL DEFAULT 0,
    last_reason TEXT NOT NULL DEFAULT '',
    total_count INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (uid, kind)
);

CREATE INDEX IF NOT EXISTS idx_account_sync_state_uid
ON account_sync_state(uid);
