ALTER TABLE accounts ADD COLUMN total_earned REAL NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN fish_shop_enabled INTEGER NOT NULL DEFAULT 0;
ALTER TABLE accounts ADD COLUMN fish_shop_level TEXT NOT NULL DEFAULT '';

UPDATE accounts
SET
    total_earned = COALESCE(
        CAST(json_extract(extra, '$.summary_cache.totalEarned') AS REAL),
        total_earned
    ),
    fish_shop_enabled = CASE
        WHEN json_extract(extra, '$.summary_cache.fishShopEnabled') IN (1, '1', 'true', 'TRUE')
            THEN 1
        ELSE fish_shop_enabled
    END,
    fish_shop_level = COALESCE(
        NULLIF(json_extract(extra, '$.summary_cache.fishShopLevel'), ''),
        fish_shop_level
    )
WHERE json_valid(extra);
