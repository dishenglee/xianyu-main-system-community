ALTER TABLE ai_reply_settings
ADD COLUMN reply_mode TEXT NOT NULL DEFAULT 'off';

UPDATE ai_reply_settings
SET reply_mode = CASE
    WHEN COALESCE(ai_enabled, 0) = 1 THEN 'ai'
    WHEN COALESCE(auto_reply_enabled, 0) = 1 THEN 'default'
    ELSE 'off'
END
WHERE TRIM(COALESCE(reply_mode, '')) = '' OR reply_mode = 'off';

CREATE TABLE IF NOT EXISTS item_reply_modes (
    cookie_id TEXT NOT NULL,
    item_id TEXT NOT NULL,
    reply_mode TEXT NOT NULL DEFAULT 'follow_global',
    updated_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (cookie_id, item_id)
);

CREATE INDEX IF NOT EXISTS idx_item_reply_modes_cookie_updated
ON item_reply_modes (cookie_id, updated_at DESC);

INSERT OR IGNORE INTO item_reply_modes (
    cookie_id,
    item_id,
    reply_mode,
    updated_at
)
SELECT
    cookie_id,
    item_id,
    'item_reply',
    COALESCE(created_at, 0)
FROM item_replies
WHERE TRIM(COALESCE(item_id, '')) <> ''
  AND cookie_id <> '__global__'
  AND cookie_id NOT LIKE '__global__::user::%';
