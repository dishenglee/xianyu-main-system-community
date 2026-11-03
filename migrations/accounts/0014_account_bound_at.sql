ALTER TABLE accounts ADD COLUMN bound_at REAL NOT NULL DEFAULT 0;

UPDATE accounts
SET bound_at = CASE
    WHEN COALESCE(owner_username, '') <> '' AND updated_at > 0 THEN updated_at
    WHEN created_at > 0 THEN created_at
    ELSE strftime('%s', 'now')
END
WHERE bound_at <= 0;

CREATE INDEX IF NOT EXISTS idx_accounts_owner_bound
ON accounts (owner_username, bound_at DESC, uid ASC);
