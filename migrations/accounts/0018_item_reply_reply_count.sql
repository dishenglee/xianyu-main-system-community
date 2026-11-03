ALTER TABLE item_replies
ADD COLUMN reply_count INTEGER NOT NULL DEFAULT 1;

UPDATE item_replies
SET reply_count = 1
WHERE reply_count < 1;

CREATE TABLE IF NOT EXISTS item_reply_task_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    cookie_id TEXT NOT NULL,
    chat_id TEXT NOT NULL,
    task_key TEXT NOT NULL,
    run_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_item_reply_task_runs_unique
ON item_reply_task_runs (cookie_id, chat_id, task_key);
