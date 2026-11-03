ALTER TABLE live_connection_state ADD COLUMN last_pts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE live_connection_state ADD COLUMN last_high_pts INTEGER NOT NULL DEFAULT 0;
