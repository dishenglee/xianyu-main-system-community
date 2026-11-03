-- 账号违规事件流水。
--
-- 闲鱼上游在 items 列表里会通过 cardData.bizMessage / banner.title / auditStatus
-- 或显式的 itemStatus=REJECTED|BANNED 等字段标记一件商品当前的违规状态。
-- items_sync 拿到 raw card 后跑一遍关键字 + 状态码匹配，命中则插入一行。
--
-- 每个 (uid, item_id, violation_kind) 三元组只允许一行 —— 同一商品的同一种
-- 违规反复同步不重复推送；只有当上游切换到一个新的 violation_kind（例如先
-- "审核未通过"后改判"违规商品下架"）时才会再插一行并再推一次通知。
--
-- notified_at = 0 表示等待 WeChat 推送 worker 接手；推送成功后写入毫秒时间戳。

CREATE TABLE IF NOT EXISTS account_violations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uid TEXT NOT NULL,
    item_id TEXT NOT NULL DEFAULT '',
    item_title TEXT NOT NULL DEFAULT '',
    -- 违规分类：'banned'(违规下架/封禁/处罚) | 'audit_failed'(审核未通过) |
    -- 'complaint'(投诉处理) | 'other'(命中关键字但分不到上述三类)
    violation_kind TEXT NOT NULL DEFAULT '',
    -- 上游原始文案 —— 最大限度保留 bizMessage / banner.title 等可读字段，
    -- 方便前端展示和运营回溯。
    reason_text TEXT NOT NULL DEFAULT '',
    -- 触发判定的上游字段（itemStatus / bizCode / banner / auditStatus / 等）
    -- 的 JSON 快照，便于以后调整 detect_violation 的阈值时复盘。
    raw_metadata TEXT NOT NULL DEFAULT '{}',
    detected_at INTEGER NOT NULL DEFAULT 0,
    notified_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE(uid, item_id, violation_kind)
);

CREATE INDEX IF NOT EXISTS idx_account_violations_uid_at
ON account_violations (uid, detected_at DESC);

CREATE INDEX IF NOT EXISTS idx_account_violations_pending
ON account_violations (notified_at) WHERE notified_at = 0;
