ALTER TABLE account_orders ADD COLUMN item_id TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_account_orders_uid_buyer_item_status
ON account_orders(uid, buyer_id, item_id, updated_at DESC);
