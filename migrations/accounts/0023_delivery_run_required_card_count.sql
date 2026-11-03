ALTER TABLE delivery_runs
ADD COLUMN required_card_count INTEGER NOT NULL DEFAULT 0;
