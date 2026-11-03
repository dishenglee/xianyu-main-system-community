-- 闲鱼商品 list 接口下发的 actions[] 数组（actionType + actionName + actionUrl 预签名 token）。
-- 闲鱼客户端是 server-driven UI：list 拿到 actions 决定每个商品有哪些可执行操作，
-- 点按钮时把 actionUrl 透传给后续调用。这里把 actions JSON 持久化到本地 cache，
-- 前端可以据此动态渲染 hover 按钮、不可用 action 自动隐藏。
ALTER TABLE account_items ADD COLUMN actions_json TEXT NOT NULL DEFAULT '[]';
