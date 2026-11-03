CREATE TABLE IF NOT EXISTS delivery_rule_card_pools (
    rule_id INTEGER NOT NULL,
    pool_tag TEXT NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (rule_id, pool_tag),
    FOREIGN KEY (rule_id) REFERENCES delivery_rules(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_delivery_rule_card_pools_tag
ON delivery_rule_card_pools(pool_tag, rule_id);

INSERT OR IGNORE INTO delivery_rule_card_pools (rule_id, pool_tag, position, created_at)
SELECT id, TRIM(card_pool_tag), 0, CAST(created_at AS INTEGER)
FROM delivery_rules
WHERE TRIM(COALESCE(card_pool_tag, '')) <> '';
