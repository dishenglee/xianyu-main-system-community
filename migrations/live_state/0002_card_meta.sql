-- 扩展 message_history_cache 存闲鱼 dxCard (content_type 25/26/28)
-- 解析出来的卖家/商品元数据，给通知消息卡片富渲染用。JSON 字符串，
-- 字段: { sellerNick, sellerAvatar, itemTitle, itemSubtitle, itemPic, linkUrl }
ALTER TABLE message_history_cache ADD COLUMN card_meta TEXT NOT NULL DEFAULT '';
