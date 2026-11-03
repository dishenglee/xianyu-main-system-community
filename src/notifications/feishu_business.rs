use sqlx::Row;

use crate::{notifications::reply_router::account_uid_belongs_to_owner, state::AppState};

use super::im_commands::FeishuImCommand;

pub(crate) async fn render_feishu_business_reply(
    state: &AppState,
    owner_username: &str,
    command: FeishuImCommand,
    text: &str,
) -> Result<Option<String>, String> {
    match command {
        FeishuImCommand::Menu => Ok(Some(render_menu())),
        FeishuImCommand::ItemList => render_item_list(state, owner_username).await.map(Some),
        FeishuImCommand::ItemDetail => render_item_detail(state, owner_username, text)
            .await
            .map(Some),
        FeishuImCommand::OrderList => render_order_list(state, owner_username).await.map(Some),
        FeishuImCommand::OrderDetail => render_order_detail(state, owner_username, text)
            .await
            .map(Some),
        _ => Ok(None),
    }
}

fn render_menu() -> String {
    [
        "鱼管家飞书菜单",
        "",
        "IM：",
        "- 回复 <内容>",
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

async fn render_item_list(state: &AppState, owner_username: &str) -> Result<String, String> {
    let accounts = owner_accounts(state, owner_username).await?;
    if accounts.is_empty() {
        return Ok("当前 owner 没有可用闲鱼账号。".to_string());
    }

    let mut lines = vec!["商品列表".to_string()];
    let mut count = 0usize;
    for account in accounts {
        let rows = sqlx::query(
            r#"
            SELECT item_id, title, price, status, online, updated_at
            FROM account_items
            WHERE uid = ?
            ORDER BY updated_at DESC, created_at DESC, item_id DESC
            LIMIT 5
            "#,
        )
        .bind(&account.uid)
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(|err| err.to_string())?;
        if rows.is_empty() {
            continue;
        }
        lines.push(format!(
            "账号：{} ({})",
            display_name(&account.nick, &account.uid),
            account.uid
        ));
        for row in rows {
            count += 1;
            let item_id = row.get::<String, _>("item_id");
            let title = row.get::<String, _>("title");
            let price = row.get::<String, _>("price");
            let status = row.get::<String, _>("status");
            let online = row.get::<i64, _>("online") != 0;
            lines.push(format!(
                "- {} | {} | {} | {}{}\n  详情：商品详情 {} {}\n  操作：下架商品 {} {} / 降价 {} {} <amount>",
                item_id,
                display_name(&title, "未命名商品"),
                display_name(&price, "价格未知"),
                display_name(&status, "状态未知"),
                if online { "" } else { " | 已下架" },
                account.uid,
                item_id,
                account.uid,
                item_id,
                account.uid,
                item_id
            ));
        }
    }

    if count == 0 {
        Ok("当前 owner 暂无商品缓存，请先在 App 内同步商品。".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

async fn render_order_list(state: &AppState, owner_username: &str) -> Result<String, String> {
    let accounts = owner_accounts(state, owner_username).await?;
    if accounts.is_empty() {
        return Ok("当前 owner 没有可用闲鱼账号。".to_string());
    }

    let mut lines = vec!["订单列表".to_string()];
    let mut count = 0usize;
    for account in accounts {
        let rows = sqlx::query(
            r#"
            SELECT tid, title, price, status_text, buyer_nick, updated_at
            FROM account_orders
            WHERE uid = ?
            ORDER BY updated_at DESC, created_at DESC, tid DESC
            LIMIT 5
            "#,
        )
        .bind(&account.uid)
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(|err| err.to_string())?;
        if rows.is_empty() {
            continue;
        }
        lines.push(format!(
            "账号：{} ({})",
            display_name(&account.nick, &account.uid),
            account.uid
        ));
        for row in rows {
            count += 1;
            let tid = row.get::<String, _>("tid");
            let title = row.get::<String, _>("title");
            let price = row.get::<String, _>("price");
            let status_text = row.get::<String, _>("status_text");
            let buyer_nick = row.get::<String, _>("buyer_nick");
            lines.push(format!(
                "- {} | {} | {} | {} | 买家：{}\n  详情：订单详情 {} {}\n  操作：订单改价 {} {} <price> / 无需快递发货 {} {} / 发货 {} {} <company_code> <mail_no>",
                tid,
                display_name(&title, "未命名订单"),
                display_name(&price, "金额未知"),
                display_name(&status_text, "状态未知"),
                display_name(&buyer_nick, "未知"),
                account.uid,
                tid,
                account.uid,
                tid,
                account.uid,
                tid,
                account.uid,
                tid
            ));
        }
    }

    if count == 0 {
        Ok("当前 owner 暂无订单缓存，请先在 App 内同步订单。".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

async fn render_item_detail(
    state: &AppState,
    owner_username: &str,
    text: &str,
) -> Result<String, String> {
    let Some((uid, item_id)) = parse_two_args(text, &["商品详情", "/商品详情", "item", "/item"])
    else {
        return Ok("请使用：商品详情 <uid> <item_id>".to_string());
    };
    if !account_uid_belongs_to_owner(state, &uid, owner_username).await? {
        return Ok("无法查看：闲鱼账号不属于当前绑定 owner。".to_string());
    }
    let row = sqlx::query(
        r#"
        SELECT item_id, title, price, status, online, updated_at
        FROM account_items
        WHERE uid = ? AND item_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid.trim())
    .bind(item_id.trim())
    .fetch_optional(state.storage().xianyu())
    .await
    .map_err(|err| err.to_string())?;
    let Some(row) = row else {
        return Ok("商品不存在或不属于该账号。".to_string());
    };
    let online = row.get::<i64, _>("online") != 0;
    Ok(format!(
        "商品详情\n账号：{}\n商品ID：{}\n标题：{}\n价格：{}\n状态：{}{}",
        uid,
        row.get::<String, _>("item_id"),
        display_name(&row.get::<String, _>("title"), "未命名商品"),
        display_name(&row.get::<String, _>("price"), "价格未知"),
        display_name(&row.get::<String, _>("status"), "状态未知"),
        if online {
            ""
        } else {
            "\n上下架：已下架"
        }
    ))
}

async fn render_order_detail(
    state: &AppState,
    owner_username: &str,
    text: &str,
) -> Result<String, String> {
    let Some((uid, tid)) = parse_two_args(text, &["订单详情", "/订单详情", "order", "/order"])
    else {
        return Ok("请使用：订单详情 <uid> <tid>".to_string());
    };
    if !account_uid_belongs_to_owner(state, &uid, owner_username).await? {
        return Ok("无法查看：闲鱼账号不属于当前绑定 owner。".to_string());
    }
    let row = sqlx::query(
        r#"
        SELECT tid, title, price, status, status_text, buyer_nick, mail_no, updated_at
        FROM account_orders
        WHERE uid = ? AND tid = ?
        LIMIT 1
        "#,
    )
    .bind(uid.trim())
    .bind(tid.trim())
    .fetch_optional(state.storage().xianyu())
    .await
    .map_err(|err| err.to_string())?;
    let Some(row) = row else {
        return Ok("订单不存在或不属于该账号。".to_string());
    };
    Ok(format!(
        "订单详情\n账号：{}\n订单ID：{}\n标题：{}\n金额：{}\n状态：{} ({})\n买家：{}\n运单号：{}",
        uid,
        row.get::<String, _>("tid"),
        display_name(&row.get::<String, _>("title"), "未命名订单"),
        display_name(&row.get::<String, _>("price"), "金额未知"),
        display_name(&row.get::<String, _>("status_text"), "状态未知"),
        display_name(&row.get::<String, _>("status"), "状态未知"),
        display_name(&row.get::<String, _>("buyer_nick"), "未知"),
        display_name(&row.get::<String, _>("mail_no"), "无")
    ))
}

#[derive(Debug)]
struct OwnerAccount {
    uid: String,
    nick: String,
}

async fn owner_accounts(
    state: &AppState,
    owner_username: &str,
) -> Result<Vec<OwnerAccount>, String> {
    let rows = sqlx::query(
        r#"
        SELECT uid, nick
        FROM accounts
        WHERE owner_username = ? AND status != 'disabled'
        ORDER BY updated_at DESC, uid ASC
        LIMIT 20
        "#,
    )
    .bind(owner_username.trim())
    .fetch_all(state.storage().accounts())
    .await
    .map_err(|err| err.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| OwnerAccount {
            uid: row.get::<String, _>("uid"),
            nick: row.get::<String, _>("nick"),
        })
        .collect())
}

fn display_name(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_two_args(text: &str, commands: &[&str]) -> Option<(String, String)> {
    let normalized = text.trim().trim_start_matches("@鱼管家").trim();
    for command in commands {
        let Some(rest) = normalized.strip_prefix(command) else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let parts = rest.split_whitespace().collect::<Vec<_>>();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}
