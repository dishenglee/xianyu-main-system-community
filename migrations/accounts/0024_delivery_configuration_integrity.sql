-- reusable/card_pool_scope are compatibility columns added after SQLx
-- migrations on a brand-new database. Bootstrap fires this deferred migration
-- exactly once after those columns exist.
CREATE TABLE IF NOT EXISTS delivery_configuration_integrity_runs (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    requested_at INTEGER NOT NULL DEFAULT 0
);

CREATE TRIGGER IF NOT EXISTS migrate_delivery_configuration_integrity_insert
AFTER INSERT ON delivery_configuration_integrity_runs
BEGIN
    -- A first-release database must not contain reservations. Preserve all
    -- related data and require manual repair instead of guessing ownership.
    UPDATE delivery_rules
    SET needs_repair = 1, enabled = 0
    WHERE needs_repair = 0
      AND card_pool_scope = 'account'
      AND cookie_id <> '__global__'
      AND cookie_id NOT LIKE '__global__::user::%'
      AND EXISTS (
          SELECT 1 FROM cards
          WHERE cards.cookie_id = delivery_rules.cookie_id
            AND cards.card_pool_tag = delivery_rules.card_pool_tag
            AND cards.reusable = 0
            AND cards.reserved_run_id IS NOT NULL
      );

    -- Migrate only unambiguous account rules. One-time-only rules are valid;
    -- exactly one reusable row additionally becomes the fixed message.
    UPDATE delivery_rules
    SET fixed_message = CASE
            WHEN 1 = (
                SELECT COUNT(*) FROM cards
                WHERE cards.cookie_id = delivery_rules.cookie_id
                  AND cards.card_pool_tag = delivery_rules.card_pool_tag
                  AND cards.reusable = 1
                  AND TRIM(cards.card_content) <> ''
            ) THEN COALESCE(
                NULLIF(TRIM(fixed_message), ''),
                (
                    SELECT TRIM(cards.card_content)
                    FROM cards
                    WHERE cards.cookie_id = delivery_rules.cookie_id
                      AND cards.card_pool_tag = delivery_rules.card_pool_tag
                      AND cards.reusable = 1
                      AND TRIM(cards.card_content) <> ''
                    ORDER BY cards.created_at ASC, cards.id ASC
                    LIMIT 1
                ),
                ''
            )
            ELSE fixed_message
        END,
        one_time_enabled = CASE WHEN EXISTS (
            SELECT 1 FROM cards
            WHERE cards.cookie_id = delivery_rules.cookie_id
              AND cards.card_pool_tag = delivery_rules.card_pool_tag
              AND cards.reusable = 0
              AND TRIM(cards.card_content) <> ''
        ) THEN 1 ELSE 0 END
    WHERE needs_repair = 0
      AND card_pool_scope = 'account'
      AND cookie_id <> '__global__'
      AND cookie_id NOT LIKE '__global__::user::%'
      AND TRIM(item_id) <> ''
      AND NOT EXISTS (
          SELECT 1 FROM delivery_rules AS duplicate
          WHERE duplicate.cookie_id = delivery_rules.cookie_id
            AND TRIM(duplicate.item_id) = TRIM(delivery_rules.item_id)
            AND duplicate.id <> delivery_rules.id
      )
      AND 1 >= (
          SELECT COUNT(*) FROM cards
          WHERE cards.cookie_id = delivery_rules.cookie_id
            AND cards.card_pool_tag = delivery_rules.card_pool_tag
            AND cards.reusable = 1
            AND TRIM(cards.card_content) <> ''
      )
      AND (
          TRIM(fixed_message) <> ''
          OR EXISTS (
              SELECT 1 FROM cards
              WHERE cards.cookie_id = delivery_rules.cookie_id
                AND cards.card_pool_tag = delivery_rules.card_pool_tag
                AND TRIM(cards.card_content) <> ''
          )
      );

    -- Invalid and ambiguous rules are stopped without rewriting their content.
    UPDATE delivery_rules
    SET needs_repair = 1, enabled = 0
    WHERE needs_repair = 0
      AND (
           cookie_id = '__global__'
        OR cookie_id LIKE '__global__::user::%'
        OR card_pool_scope <> 'account'
        OR TRIM(item_id) = ''
        OR EXISTS (
            SELECT 1 FROM delivery_rules AS duplicate
            WHERE duplicate.cookie_id = delivery_rules.cookie_id
              AND TRIM(duplicate.item_id) = TRIM(delivery_rules.item_id)
              AND duplicate.id <> delivery_rules.id
        )
        OR 1 < (
            SELECT COUNT(*) FROM cards
            WHERE cards.cookie_id = delivery_rules.cookie_id
              AND cards.card_pool_tag = delivery_rules.card_pool_tag
              AND cards.reusable = 1
              AND TRIM(cards.card_content) <> ''
        )
        OR (
            TRIM(fixed_message) = ''
            AND NOT EXISTS (
                SELECT 1 FROM cards
                WHERE cards.cookie_id = delivery_rules.cookie_id
                  AND cards.card_pool_tag = delivery_rules.card_pool_tag
                  AND TRIM(cards.card_content) <> ''
            )
        )
      );

    -- Only valid account delivery pools are normalized. Used history always
    -- survives; available duplicates lose to used rows, otherwise the oldest
    -- available row wins.
    DELETE FROM cards
    WHERE reusable = 0
      AND used = 0
      AND reserved_run_id IS NULL
      AND TRIM(card_content) <> ''
      AND EXISTS (
          SELECT 1 FROM delivery_rules
          WHERE delivery_rules.cookie_id = cards.cookie_id
            AND delivery_rules.card_pool_tag = cards.card_pool_tag
            AND delivery_rules.card_pool_scope = 'account'
            AND delivery_rules.needs_repair = 0
            AND delivery_rules.cookie_id <> '__global__'
            AND delivery_rules.cookie_id NOT LIKE '__global__::user::%'
      )
      AND EXISTS (
          SELECT 1 FROM cards AS preserved
          WHERE preserved.id <> cards.id
            AND preserved.cookie_id = cards.cookie_id
            AND preserved.card_pool_tag = cards.card_pool_tag
            AND preserved.reusable = 0
            AND TRIM(preserved.card_content) = TRIM(cards.card_content)
            AND (
                preserved.used <> 0
                OR (
                    preserved.used = 0
                    AND preserved.reserved_run_id IS NULL
                    AND (preserved.created_at < cards.created_at
                         OR (preserved.created_at = cards.created_at AND preserved.id < cards.id))
                )
            )
      );
END;

CREATE TRIGGER IF NOT EXISTS enforce_delivery_configuration_unique_insert
BEFORE INSERT ON delivery_rules
WHEN NEW.needs_repair = 0
 AND NEW.card_pool_scope = 'account'
 AND TRIM(NEW.item_id) <> ''
 AND EXISTS (
     SELECT 1 FROM delivery_rules
     WHERE cookie_id = NEW.cookie_id
       AND TRIM(item_id) = TRIM(NEW.item_id)
       AND card_pool_scope = 'account'
 )
BEGIN
    SELECT RAISE(ABORT, 'duplicate delivery configuration');
END;

CREATE TRIGGER IF NOT EXISTS enforce_delivery_configuration_unique_update
BEFORE UPDATE OF cookie_id, item_id, needs_repair, card_pool_scope ON delivery_rules
WHEN NEW.needs_repair = 0
 AND NEW.card_pool_scope = 'account'
 AND TRIM(NEW.item_id) <> ''
 AND EXISTS (
     SELECT 1 FROM delivery_rules
     WHERE cookie_id = NEW.cookie_id
       AND TRIM(item_id) = TRIM(NEW.item_id)
       AND card_pool_scope = 'account'
       AND id <> OLD.id
 )
BEGIN
    SELECT RAISE(ABORT, 'duplicate delivery configuration');
END;

CREATE TRIGGER IF NOT EXISTS enforce_delivery_card_unique_insert
BEFORE INSERT ON cards
WHEN NEW.reusable = 0
 AND TRIM(NEW.card_content) <> ''
 AND EXISTS (
     SELECT 1 FROM delivery_rules
     WHERE delivery_rules.cookie_id = NEW.cookie_id
       AND delivery_rules.card_pool_tag = NEW.card_pool_tag
       AND delivery_rules.card_pool_scope = 'account'
       AND delivery_rules.needs_repair = 0
       AND delivery_rules.cookie_id <> '__global__'
       AND delivery_rules.cookie_id NOT LIKE '__global__::user::%'
 )
 AND EXISTS (
     SELECT 1 FROM cards
     WHERE cards.cookie_id = NEW.cookie_id
       AND cards.card_pool_tag = NEW.card_pool_tag
       AND cards.reusable = 0
       AND TRIM(cards.card_content) = TRIM(NEW.card_content)
 )
BEGIN
    SELECT RAISE(IGNORE);
END;
