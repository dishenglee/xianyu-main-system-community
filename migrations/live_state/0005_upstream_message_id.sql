-- 闲鱼撤回接口只接受 PNM 格式的
-- 服务端 messageId（形如 "4111237573609.PNM"），不接受我们用作客户端主键的
-- UUID（形如 "020427614f4c..."，来自 extJson.messageId）。
--
-- 为了支持撤回，把 PNM 单独存一份。message_id（UUID）继续作为主键和 dedup key。
ALTER TABLE message_history_cache ADD COLUMN upstream_message_id TEXT NOT NULL DEFAULT '';
