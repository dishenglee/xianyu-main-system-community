DELETE FROM delivery_rule_card_pools
WHERE rule_id IN (
    SELECT id FROM delivery_rules WHERE one_time_enabled = 0
);
