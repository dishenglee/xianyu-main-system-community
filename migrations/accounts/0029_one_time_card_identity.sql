ALTER TABLE cards
ADD COLUMN one_time_identity_active INTEGER NOT NULL DEFAULT 1;

CREATE TABLE IF NOT EXISTS one_time_card_duplicate_quarantine (
    card_id INTEGER PRIMARY KEY,
    cookie_id TEXT NOT NULL,
    card_pool_tag TEXT NOT NULL,
    normalized_content TEXT NOT NULL,
    canonical_card_id INTEGER NOT NULL,
    quarantined_at INTEGER NOT NULL
);

INSERT INTO one_time_card_duplicate_quarantine (
    card_id,
    cookie_id,
    card_pool_tag,
    normalized_content,
    canonical_card_id,
    quarantined_at
)
SELECT
    c.id,
    c.cookie_id,
    c.card_pool_tag,
    TRIM(c.card_content),
    (
        SELECT MIN(c2.id)
        FROM cards c2
        WHERE c2.reusable = 0
          AND c2.cookie_id = c.cookie_id
          AND c2.card_pool_tag = c.card_pool_tag
          AND TRIM(c2.card_content) = TRIM(c.card_content)
    ),
    CAST(unixepoch('subsec') * 1000 AS INTEGER)
FROM cards c
WHERE c.reusable = 0
  AND TRIM(c.card_content) <> ''
  AND c.id <> (
      SELECT MIN(c2.id)
      FROM cards c2
      WHERE c2.reusable = 0
        AND c2.cookie_id = c.cookie_id
        AND c2.card_pool_tag = c.card_pool_tag
        AND TRIM(c2.card_content) = TRIM(c.card_content)
  );

UPDATE cards
SET one_time_identity_active = 0
WHERE id IN (SELECT card_id FROM one_time_card_duplicate_quarantine);

CREATE UNIQUE INDEX IF NOT EXISTS ux_cards_one_time_owner_pool_content
ON cards(cookie_id, card_pool_tag, TRIM(card_content))
WHERE reusable = 0
  AND one_time_identity_active = 1
  AND TRIM(card_content) <> '';
