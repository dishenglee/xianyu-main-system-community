ALTER TABLE accounts ADD COLUMN owner_username TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_accounts_owner_status_updated
ON accounts (owner_username, status, updated_at DESC);
