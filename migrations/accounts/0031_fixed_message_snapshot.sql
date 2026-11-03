ALTER TABLE delivery_runs
ADD COLUMN fixed_message_snapshot TEXT NOT NULL DEFAULT '';

ALTER TABLE delivery_runs
ADD COLUMN fixed_message_snapshot_captured INTEGER NOT NULL DEFAULT 0;
