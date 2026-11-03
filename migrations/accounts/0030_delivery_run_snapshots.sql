ALTER TABLE delivery_runs
ADD COLUMN pool_owner_key TEXT NOT NULL DEFAULT '';

ALTER TABLE delivery_runs
ADD COLUMN pool_scope_snapshot TEXT NOT NULL DEFAULT '';

ALTER TABLE delivery_runs
ADD COLUMN one_time_pool_tags_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE delivery_runs
ADD COLUMN fixed_pool_tags_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE delivery_runs
ADD COLUMN purchase_quantity_source TEXT NOT NULL DEFAULT '';

ALTER TABLE delivery_runs
ADD COLUMN quantity_confirmed_at INTEGER NOT NULL DEFAULT 0;

ALTER TABLE delivery_runs
ADD COLUMN cancel_observed_at INTEGER NOT NULL DEFAULT 0;
