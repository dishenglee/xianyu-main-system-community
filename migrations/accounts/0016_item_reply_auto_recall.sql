ALTER TABLE item_replies
ADD COLUMN auto_recall INTEGER NOT NULL DEFAULT 0;

UPDATE item_replies
SET auto_recall = 1
WHERE cookie_id IN (
    SELECT uid
    FROM accounts
    WHERE json_extract(COALESCE(extra, '{}'), '$.autoRecallItemReplies') = 1
);
