-- 记录对方最近一次"已读"游标（peer 调用 relation.message.read 时，对方端会
-- 通过 live WS 推送 contentType=14/27 tip 或 session 元数据更新；这里我们用
-- 一个 peer_last_read_version 字段缓存：当我们看到对方读到的最新 msgId.PNM /
-- version >= 我们某条 outgoing 的 version，就把它判定为"已读"。
ALTER TABLE message_live_state ADD COLUMN peer_last_read_version INTEGER NOT NULL DEFAULT 0;
