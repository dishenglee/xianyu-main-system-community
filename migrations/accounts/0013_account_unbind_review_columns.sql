ALTER TABLE account_unbind_requests ADD COLUMN reviewed_by TEXT NOT NULL DEFAULT '';
ALTER TABLE account_unbind_requests ADD COLUMN reviewed_at REAL NOT NULL DEFAULT 0;
ALTER TABLE account_unbind_requests ADD COLUMN review_note TEXT NOT NULL DEFAULT '';
