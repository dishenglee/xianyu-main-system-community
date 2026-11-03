-- 反撤回价值：对方撤回的消息我们要照样保存原内容，pill 下面把原内容塞进去
-- 给用户看。pill 文本（"对方撤回了一条消息" / "你撤回了一条消息"）我们之前
-- 已经写到 text 列里了，但原内容会被覆盖丢失——所以单开一列存原文。
--
-- 1) recalled_at: 撤回发生的时间戳（ms）。0 = 没被撤回。
-- 2) recalled_original_text: 撤回前的原文本（带 [图片]/[链接] 前缀的渲染文本，
--    跟 text 列保持同种格式）。空字符串 = 没原文（早期消息、纯图片等）。
ALTER TABLE message_history_cache ADD COLUMN recalled_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE message_history_cache ADD COLUMN recalled_original_text TEXT NOT NULL DEFAULT '';
ALTER TABLE message_history_cache ADD COLUMN recalled_original_image_url TEXT NOT NULL DEFAULT '';
