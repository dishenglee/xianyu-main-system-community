ALTER TABLE account_unbind_requests
ADD COLUMN account_snapshot TEXT NOT NULL DEFAULT '{}';
