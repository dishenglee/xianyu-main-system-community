ALTER TABLE delivery_runs
ADD COLUMN waiting_notice_claimed_at INTEGER NOT NULL DEFAULT 0;

ALTER TABLE delivery_runs
ADD COLUMN delivery_send_claimed_at INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_delivery_runs_send_claims
ON delivery_runs (
    status,
    waiting_notice_sent_at,
    waiting_notice_claimed_at,
    message_sent_at,
    delivery_send_claimed_at
);
