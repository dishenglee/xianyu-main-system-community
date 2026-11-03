ALTER TABLE feishu_user_bindings ADD COLUMN role TEXT NOT NULL DEFAULT 'collaborator';
ALTER TABLE feishu_user_bindings ADD COLUMN joined_via TEXT NOT NULL DEFAULT 'legacy_upgrade';
ALTER TABLE feishu_user_bindings ADD COLUMN invited_by_sender_id TEXT NOT NULL DEFAULT '';
ALTER TABLE feishu_user_bindings ADD COLUMN invited_by_sender_name TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS feishu_collaborator_invites (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id INTEGER NOT NULL,
    owner_username TEXT NOT NULL,
    code TEXT NOT NULL,
    created_by_sender_id TEXT NOT NULL DEFAULT '',
    created_by_sender_name TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'active',
    expires_at REAL NOT NULL DEFAULT 0,
    used_by_sender_id TEXT NOT NULL DEFAULT '',
    used_at REAL NOT NULL DEFAULT 0,
    created_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    updated_at REAL NOT NULL DEFAULT (unixepoch('subsec')),
    UNIQUE(channel_id, code)
);

CREATE INDEX IF NOT EXISTS idx_feishu_collaborator_invites_active
ON feishu_collaborator_invites (channel_id, status, expires_at DESC, id DESC);
