UPDATE notification_channels
SET config = json_remove(
    json_set(
        config,
        '$.robot_url',
        json_extract(config, '$.webhook_url')
    ),
    '$.webhook_url'
)
WHERE lower(trim(type)) IN ('wechat', 'dingtalk')
  AND json_valid(config)
  AND json_type(config, '$.webhook_url') IS NOT NULL
  AND (
      json_type(config, '$.robot_url') IS NULL
      OR json_type(config, '$.robot_url') = 'null'
      OR trim(COALESCE(CAST(json_extract(config, '$.robot_url') AS TEXT), '')) = ''
  );

UPDATE notification_channels
SET config = json_remove(config, '$.webhook_url')
WHERE json_valid(config)
  AND json_type(config, '$.webhook_url') IS NOT NULL;
