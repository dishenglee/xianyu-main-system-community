ALTER TABLE delivery_runs ADD COLUMN lease_owner TEXT NOT NULL DEFAULT '';
ALTER TABLE delivery_runs ADD COLUMN lease_expires_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE delivery_runs ADD COLUMN waiting_notice_claim_owner TEXT NOT NULL DEFAULT '';
ALTER TABLE delivery_runs ADD COLUMN waiting_notice_claim_expires_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE delivery_runs ADD COLUMN delivery_send_claim_owner TEXT NOT NULL DEFAULT '';
ALTER TABLE delivery_runs ADD COLUMN delivery_send_claim_expires_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE delivery_runs ADD COLUMN delivery_send_started_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE delivery_runs ADD COLUMN message_acknowledged_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE delivery_runs ADD COLUMN message_ack_id TEXT NOT NULL DEFAULT '';
ALTER TABLE delivery_runs ADD COLUMN message_ack_uuid TEXT NOT NULL DEFAULT '';

CREATE INDEX idx_delivery_runs_due_lease
ON delivery_runs(uid, status, next_retry_at, lease_expires_at, updated_at);
