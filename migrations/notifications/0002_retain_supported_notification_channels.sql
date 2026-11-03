UPDATE notification_channels
SET type = 'dingtalk'
WHERE lower(trim(type)) = 'ding_talk';

UPDATE notification_channels
SET type = 'openclaw_weixin'
WHERE lower(trim(type)) IN (
    'openclaw',
    'openclaw-weixin',
    'openclaw_wx',
    'openclaw-wx',
    'weixin_personal'
);

DELETE FROM notification_channel_runtime
WHERE channel_id IN (
    SELECT id
    FROM notification_channels
    WHERE lower(trim(type)) NOT IN (
        'wechat',
        'dingtalk',
        'feishu',
        'lark',
        'openclaw_weixin'
    )
);

DELETE FROM account_notification_bindings
WHERE channel_id IN (
    SELECT id
    FROM notification_channels
    WHERE lower(trim(type)) NOT IN (
        'wechat',
        'dingtalk',
        'feishu',
        'lark',
        'openclaw_weixin'
    )
);

DELETE FROM notification_channels
WHERE lower(trim(type)) NOT IN (
    'wechat',
    'dingtalk',
    'feishu',
    'lark',
    'openclaw_weixin'
);
