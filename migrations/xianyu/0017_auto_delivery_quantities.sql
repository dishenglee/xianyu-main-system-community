ALTER TABLE account_items
ADD COLUMN quantity INTEGER;

ALTER TABLE account_items
ADD COLUMN quantity_source TEXT;

ALTER TABLE account_items
ADD COLUMN quantity_synced_at INTEGER;

ALTER TABLE account_orders
ADD COLUMN purchase_quantity INTEGER;

ALTER TABLE account_orders
ADD COLUMN purchase_quantity_source TEXT;
