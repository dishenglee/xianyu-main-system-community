#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeishuImCommand {
    Help,
    Menu,
    Bind,
    Unbind,
    Collaborators,
    GenerateInvite,
    JoinInvite,
    LeaveCollaboration,
    ItemList,
    ItemDetail,
    OrderList,
    OrderDetail,
    RecentMessages,
    RecentReply,
    OpenChatRoute,
    SelectReplyRoute,
    Ack,
}

pub(crate) fn parse_feishu_im_command(text: &str) -> FeishuImCommand {
    if open_chat_route_id(text).is_some() {
        return FeishuImCommand::OpenChatRoute;
    }
    if selected_reply_route_id(text).is_some() {
        return FeishuImCommand::SelectReplyRoute;
    }
    if recent_reply_body(text).is_some() {
        return FeishuImCommand::RecentReply;
    }
    if invite_join_code(text).is_some() {
        return FeishuImCommand::JoinInvite;
    }
    let normalized = text
        .trim()
        .trim_start_matches("@鱼管家")
        .trim()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "/帮助" | "帮助" | "/help" | "help" => FeishuImCommand::Help,
        "/菜单" | "菜单" | "/menu" | "menu" => FeishuImCommand::Menu,
        "/绑定" | "绑定" | "/bind" | "bind" => FeishuImCommand::Bind,
        "/解绑" | "解绑" | "/unbind" | "unbind" => FeishuImCommand::Unbind,
        "/协作者" | "协作者" | "/members" | "members" => FeishuImCommand::Collaborators,
        "/生成邀请码" | "生成邀请码" | "/invite" | "invite" => {
            FeishuImCommand::GenerateInvite
        }
        "/退出协作" | "退出协作" | "/leave" | "leave" => {
            FeishuImCommand::LeaveCollaboration
        }
        "/最近消息" | "最近消息" | "/recent" | "recent" => FeishuImCommand::RecentMessages,
        "/商品列表" | "商品列表" | "/items" | "items" => FeishuImCommand::ItemList,
        "/订单列表" | "订单列表" | "/orders" | "orders" => FeishuImCommand::OrderList,
        _ if starts_with_command(&normalized, &["/商品详情", "商品详情", "/item", "item"]) => {
            FeishuImCommand::ItemDetail
        }
        _ if starts_with_command(&normalized, &["/订单详情", "订单详情", "/order", "order"]) => {
            FeishuImCommand::OrderDetail
        }
        _ => FeishuImCommand::Ack,
    }
}

pub(crate) fn unbound_feishu_reply(command: FeishuImCommand) -> String {
    match command {
        FeishuImCommand::Help => {
            "鱼管家飞书助手\n\n当前飞书用户尚未加入 owner，只能查看帮助和加入提示。加入完成后才能使用闲鱼 IM、商品和订单能力。"
                .to_string()
        }
        FeishuImCommand::Bind => {
            "当前飞书接入已经改成扫码自动绑定或邀请码加入。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::Unbind => "当前飞书用户尚未绑定，无需解绑。".to_string(),
        FeishuImCommand::Collaborators => {
            "当前飞书用户尚未加入 owner，不能查看协作者。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::GenerateInvite => {
            "当前飞书用户尚未加入 owner，不能生成邀请码。若你是协作者，请先发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::JoinInvite => "正在校验邀请码。".to_string(),
        FeishuImCommand::LeaveCollaboration => {
            "当前飞书用户尚未加入 owner，无需退出协作。".to_string()
        }
        FeishuImCommand::Menu => {
            "当前飞书用户尚未加入 owner，不能查看菜单。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::ItemList
        | FeishuImCommand::ItemDetail
        | FeishuImCommand::OrderList
        | FeishuImCommand::OrderDetail => {
            "当前飞书用户尚未加入 owner，不能查看商品或订单。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::RecentReply => {
            "当前飞书用户尚未加入 owner，不能回复最近闲鱼会话。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::OpenChatRoute => {
            "当前飞书用户尚未加入 owner，不能进入闲鱼会话。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::SelectReplyRoute => {
            "当前飞书用户尚未加入 owner，不能选择闲鱼回复目标。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::RecentMessages => {
            "当前飞书用户尚未加入 owner，不能查看最近闲鱼消息。若你是协作者，请发送「加入 <邀请码>」。".to_string()
        }
        FeishuImCommand::Ack => {
            "收到。当前飞书用户尚未加入 owner，当前不会触发闲鱼 IM、商品或订单操作。发送 /帮助 查看加入方式。"
                .to_string()
        }
    }
}

pub(crate) fn bound_feishu_reply(command: FeishuImCommand, owner_username: &str) -> String {
    let owner = owner_username.trim();
    match command {
        FeishuImCommand::Help => format!(
            "鱼管家飞书助手\n\n当前飞书用户已加入 owner `{owner}`。\n\n当前支持：\n- 在飞书引用鱼管家推送的闲鱼 IM 通知并输入回复内容，可回复对应闲鱼会话。\n- 发送「回复 <内容>」可回复当前飞书会话最近一条鱼管家推送的闲鱼 IM。\n- 发送「商品列表」查看当前 owner 的商品缓存。\n- 发送「订单列表」查看当前 owner 的订单缓存。\n- 发送「协作者」查看当前飞书协作成员。\n- 主用户可发送「生成邀请码」邀请新的飞书协作者。\n- 发送「下架商品 <uid> <item_id>」生成确认码，确认后下架商品。\n- 发送「无需快递发货 <uid> <tid>」生成确认码，确认后将订单标记为无需快递发货。\n- 发送「确认 <nonce>」执行 5 分钟内由本人发起的待确认动作。"
        ),
        FeishuImCommand::Menu => business_menu_reply(),
        FeishuImCommand::Bind => {
            format!("当前飞书用户已绑定到 owner `{owner}`，无需重复绑定。")
        }
        FeishuImCommand::Unbind => "已解绑当前飞书用户。再次发送「绑定」可重新绑定。".to_string(),
        FeishuImCommand::Collaborators => "协作者列表正在读取。".to_string(),
        FeishuImCommand::GenerateInvite => "正在生成协作者邀请码。".to_string(),
        FeishuImCommand::JoinInvite => format!("当前飞书用户已加入 owner `{owner}`，无需重复加入。"),
        FeishuImCommand::LeaveCollaboration => "正在退出协作。".to_string(),
        FeishuImCommand::ItemList => "商品列表正在读取。".to_string(),
        FeishuImCommand::ItemDetail => "请使用「商品详情 <uid> <item_id>」查看商品详情。".to_string(),
        FeishuImCommand::OrderList => "订单列表正在读取。".to_string(),
        FeishuImCommand::OrderDetail => "请使用「订单详情 <uid> <tid>」查看订单详情。".to_string(),
        FeishuImCommand::RecentMessages => "最近消息正在读取。".to_string(),
        FeishuImCommand::RecentReply => {
            "请使用「回复 <内容>」回复当前飞书会话最近一条闲鱼 IM。".to_string()
        }
        FeishuImCommand::OpenChatRoute => {
            "请点击收件箱卡片里的“进入聊天”，打开会话主卡后继续回复。".to_string()
        }
        FeishuImCommand::SelectReplyRoute => {
            "请点击聚合通知卡片里的回复按钮，先选中要回复的闲鱼会话。".to_string()
        }
        FeishuImCommand::Ack => format!(
            "收到。当前飞书用户已绑定到 owner `{owner}`。如需回复闲鱼 IM，请在飞书中引用鱼管家推送的客户消息后输入回复内容；发送 /帮助 查看当前能力。"
        ),
    }
}

pub(crate) fn bind_success_reply(owner_username: &str) -> String {
    format!(
        "已加入 owner `{}`。\n\n现在可以在飞书中引用鱼管家推送的闲鱼 IM 通知并输入回复内容，也可以发送「商品列表」或「订单列表」查询缓存数据。",
        owner_username.trim()
    )
}

pub(crate) fn bind_rejected_reply(reason: &str) -> String {
    format!(
        "无法加入：{}。请先在 App 内扫码创建飞书主用户，或让主用户发送邀请码。",
        reason.trim()
    )
}

pub(crate) fn recent_reply_body(text: &str) -> Option<String> {
    let normalized = text.trim().trim_start_matches("@鱼管家").trim();
    for prefix in ["回复 最近", "/回复 最近", "回复", "/回复", "/r", "r"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let body = rest.trim();
            if !body.is_empty() {
                return Some(body.to_string());
            }
        }
    }
    None
}

pub(crate) fn open_chat_route_id(text: &str) -> Option<String> {
    let normalized = text.trim().trim_start_matches("@鱼管家").trim();
    for prefix in ["打开会话", "/打开会话", "open_chat", "/open_chat"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let route_id = rest.trim();
            if !route_id.is_empty() {
                return Some(route_id.to_string());
            }
        }
    }
    None
}

pub(crate) fn selected_reply_route_id(text: &str) -> Option<String> {
    let normalized = text.trim().trim_start_matches("@鱼管家").trim();
    for prefix in ["选择回复", "/选择回复", "select_reply", "/select_reply"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let route_id = rest.trim();
            if !route_id.is_empty() {
                return Some(route_id.to_string());
            }
        }
    }
    None
}

pub(crate) fn invite_join_code(text: &str) -> Option<String> {
    let normalized = text.trim().trim_start_matches("@鱼管家").trim();
    for prefix in ["加入", "/加入", "join", "/join"] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let code = rest.trim();
            if !code.is_empty() {
                return Some(code.to_string());
            }
        }
    }
    None
}

fn starts_with_command(normalized: &str, commands: &[&str]) -> bool {
    commands.iter().any(|command| {
        normalized == *command
            || normalized
                .strip_prefix(command)
                .map(|rest| rest.starts_with(' '))
                .unwrap_or(false)
    })
}

fn business_menu_reply() -> String {
    [
        "鱼管家飞书菜单",
        "",
        "IM：",
        "- 最近消息",
        "- 回复 <内容>",
        "",
        "协作：",
        "- 协作者",
        "- 生成邀请码（主用户）",
        "- 加入 <邀请码>",
        "- 退出协作",
        "",
        "商品：",
        "- 商品列表",
        "- 商品详情 <uid> <item_id>",
        "- 下架商品 <uid> <item_id>",
        "- 降价 <uid> <item_id> <amount>",
        "",
        "订单：",
        "- 订单列表",
        "- 订单详情 <uid> <tid>",
        "- 订单改价 <uid> <tid> <price>",
        "- 无需快递发货 <uid> <tid>",
        "- 发货 <uid> <tid> <company_code> <mail_no> [company_name]",
        "",
        "写操作会先返回确认码，需发送「确认 <nonce>」后执行。",
    ]
    .join("\n")
}
