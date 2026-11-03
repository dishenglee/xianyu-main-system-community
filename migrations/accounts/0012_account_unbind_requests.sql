CREATE TABLE IF NOT EXISTS account_unbind_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_uid TEXT NOT NULL,
    requester_username TEXT NOT NULL,
    existing_owner_username TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'pending',
    reason TEXT NOT NULL DEFAULT '',
    requested_at REAL NOT NULL DEFAULT 0,
    updated_at REAL NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS ux_account_unbind_requests_pending
ON account_unbind_requests (account_uid, requester_username)
WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_account_unbind_requests_status
ON account_unbind_requests (status, requested_at DESC);
