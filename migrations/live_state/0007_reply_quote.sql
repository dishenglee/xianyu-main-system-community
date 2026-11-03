-- 引用回复（chat quote-reply）持久化：闲鱼 WS 协议在 message.extension.extJson
-- 里塞 {"replyMessageId":"<PNM>"} 表示这条消息引用了另一条；上游不携带被引用消息
-- 的原文，所以前端要么 client-side join 历史里的同 PNM 消息拿回原文，要么发送
-- 方在发送时把当时显示给用户的引用文本也存一份。
--
-- 1) reply_to_message_id: 被引用消息的 PNM（"<seq>.PNM"）。空 = 不是回复。
-- 2) reply_to_text:        发送时显示的引用文本（渲染过的 preview/text，可能
--    带 [图片] 前缀）。让接收端直接渲染、不必回扫历史。
ALTER TABLE message_history_cache ADD COLUMN reply_to_message_id TEXT NOT NULL DEFAULT '';
ALTER TABLE message_history_cache ADD COLUMN reply_to_text TEXT NOT NULL DEFAULT '';
