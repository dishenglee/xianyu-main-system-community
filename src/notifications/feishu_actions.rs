use serde_json::{json, Map, Value};

use crate::{
    notifications::reply_router::account_uid_belongs_to_owner,
    routes::market::{
        items::{run_item_downshelf_action, run_item_downshelf_upstream_action},
        orders::{
            run_order_adjust_price_local_action, run_order_adjust_price_upstream_action,
            run_order_consign_dummy_action, run_order_consign_dummy_upstream_action,
            run_order_consign_local_action, run_order_consign_upstream_action,
        },
        pricing::{run_item_reduce_price_action, run_item_reduce_price_local_action},
    },
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeishuBusinessActionReply {
    pub(crate) status: &'static str,
    pub(crate) reply: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedAction {
    Confirm {
        nonce: String,
    },
    ItemDownshelf {
        uid: String,
        item_id: String,
    },
    ItemReducePrice {
        uid: String,
        item_id: String,
        amount: String,
    },
    OrderDummyConsign {
        uid: String,
        tid: String,
    },
    OrderAdjustPrice {
        uid: String,
        tid: String,
        price: String,
    },
    OrderConsign {
        uid: String,
        tid: String,
        company_code: String,
        mail_no: String,
        company_name: String,
    },
}

pub(crate) async fn handle_feishu_business_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    source_message_id: &str,
    text: &str,
) -> Result<Option<FeishuBusinessActionReply>, String> {
    let Some(action) = parse_action(text) else {
        return Ok(None);
    };

    let reply = match action {
        ParsedAction::Confirm { nonce } => {
            confirm_pending_action(
                state,
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                &nonce,
            )
            .await?
        }
        ParsedAction::ItemDownshelf { uid, item_id } => {
            create_pending_item_action(
                state,
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                source_message_id,
                "item_downshelf",
                &uid,
                &item_id,
            )
            .await?
        }
        ParsedAction::ItemReducePrice {
            uid,
            item_id,
            amount,
        } => {
            create_pending_item_reduce_price_action(
                state,
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                source_message_id,
                &uid,
                &item_id,
                &amount,
            )
            .await?
        }
        ParsedAction::OrderDummyConsign { uid, tid } => {
            create_pending_order_action(
                state,
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                source_message_id,
                "order_dummy_consign",
                &uid,
                &tid,
            )
            .await?
        }
        ParsedAction::OrderAdjustPrice { uid, tid, price } => {
            create_pending_order_adjust_price_action(
                state,
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                source_message_id,
                &uid,
                &tid,
                &price,
            )
            .await?
        }
        ParsedAction::OrderConsign {
            uid,
            tid,
            company_code,
            mail_no,
            company_name,
        } => {
            create_pending_order_consign_action(
                state,
                channel_id,
                owner_username,
                sender_id,
                chat_id,
                source_message_id,
                &uid,
                &tid,
                &company_code,
                &mail_no,
                &company_name,
            )
            .await?
        }
    };

    Ok(Some(reply))
}

fn parse_action(text: &str) -> Option<ParsedAction> {
    let normalized = text.trim().trim_start_matches("@鱼管家").trim();
    let parts = normalized.split_whitespace().collect::<Vec<_>>();
    match parts.as_slice() {
        ["确认", nonce] | ["/确认", nonce] | ["confirm", nonce] | ["/confirm", nonce] => {
            Some(ParsedAction::Confirm {
                nonce: nonce.trim().to_ascii_uppercase(),
            })
        }
        ["下架商品", uid, item_id] | ["/下架商品", uid, item_id] => {
            Some(ParsedAction::ItemDownshelf {
                uid: uid.trim().to_string(),
                item_id: item_id.trim().to_string(),
            })
        }
        ["降价", uid, item_id, amount] | ["/降价", uid, item_id, amount] => {
            Some(ParsedAction::ItemReducePrice {
                uid: uid.trim().to_string(),
                item_id: item_id.trim().to_string(),
                amount: amount.trim().to_string(),
            })
        }
        ["无需快递发货", uid, tid] | ["/无需快递发货", uid, tid] => {
            Some(ParsedAction::OrderDummyConsign {
                uid: uid.trim().to_string(),
                tid: tid.trim().to_string(),
            })
        }
        ["订单改价", uid, tid, price]
        | ["/订单改价", uid, tid, price]
        | ["改价", uid, tid, price]
        | ["/改价", uid, tid, price] => Some(ParsedAction::OrderAdjustPrice {
            uid: uid.trim().to_string(),
            tid: tid.trim().to_string(),
            price: price.trim().to_string(),
        }),
        ["发货", uid, tid, company_code, mail_no] | ["/发货", uid, tid, company_code, mail_no] => {
            Some(ParsedAction::OrderConsign {
                uid: uid.trim().to_string(),
                tid: tid.trim().to_string(),
                company_code: company_code.trim().to_string(),
                mail_no: mail_no.trim().to_string(),
                company_name: company_code.trim().to_string(),
            })
        }
        ["发货", uid, tid, company_code, mail_no, company_name @ ..]
        | ["/发货", uid, tid, company_code, mail_no, company_name @ ..] => {
            Some(ParsedAction::OrderConsign {
                uid: uid.trim().to_string(),
                tid: tid.trim().to_string(),
                company_code: company_code.trim().to_string(),
                mail_no: mail_no.trim().to_string(),
                company_name: company_name.join(" ").trim().to_string(),
            })
        }
        _ => None,
    }
}

async fn create_pending_item_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    source_message_id: &str,
    action_type: &str,
    uid: &str,
    item_id: &str,
) -> Result<FeishuBusinessActionReply, String> {
    if !account_uid_belongs_to_owner(state, uid, owner_username).await? {
        return Ok(rejected_reply("闲鱼账号不属于当前绑定 owner。"));
    }
    let Some(title) = item_title(state, uid, item_id).await? else {
        return Ok(rejected_reply("商品不存在或不属于该账号。"));
    };

    let nonce = generate_nonce();
    state
        .storage()
        .create_feishu_pending_action(
            channel_id,
            owner_username,
            sender_id,
            chat_id,
            &nonce,
            action_type,
            uid,
            item_id,
            &Map::new(),
            source_message_id,
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(FeishuBusinessActionReply {
        status: "business_action_pending",
        reply: format!(
            "将执行：下架商品\n账号：{uid}\n商品：{} ({item_id})\n\n请在 5 分钟内发送：确认 {nonce}",
            display_or_default(&title, "未命名商品")
        ),
    })
}

async fn create_pending_item_reduce_price_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    source_message_id: &str,
    uid: &str,
    item_id: &str,
    amount: &str,
) -> Result<FeishuBusinessActionReply, String> {
    if !account_uid_belongs_to_owner(state, uid, owner_username).await? {
        return Ok(rejected_reply("闲鱼账号不属于当前绑定 owner。"));
    }
    let Some(title) = item_title(state, uid, item_id).await? else {
        return Ok(rejected_reply("商品不存在或不属于该账号。"));
    };
    if parse_positive_amount(amount).is_none() {
        return Ok(rejected_reply("降价金额无效。"));
    }

    let nonce = generate_nonce();
    let args = Map::from_iter([("amount".to_string(), json!(amount.trim()))]);
    state
        .storage()
        .create_feishu_pending_action(
            channel_id,
            owner_username,
            sender_id,
            chat_id,
            &nonce,
            "item_reduce_price",
            uid,
            item_id,
            &args,
            source_message_id,
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(FeishuBusinessActionReply {
        status: "business_action_pending",
        reply: format!(
            "将执行：商品降价\n账号：{uid}\n商品：{} ({item_id})\n降价金额：{}\n\n请在 5 分钟内发送：确认 {nonce}",
            display_or_default(&title, "未命名商品"),
            amount.trim()
        ),
    })
}

async fn create_pending_order_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    source_message_id: &str,
    action_type: &str,
    uid: &str,
    tid: &str,
) -> Result<FeishuBusinessActionReply, String> {
    if !account_uid_belongs_to_owner(state, uid, owner_username).await? {
        return Ok(rejected_reply("闲鱼账号不属于当前绑定 owner。"));
    }
    let Some(title) = order_title(state, uid, tid).await? else {
        return Ok(rejected_reply("订单不存在或不属于该账号。"));
    };

    let nonce = generate_nonce();
    state
        .storage()
        .create_feishu_pending_action(
            channel_id,
            owner_username,
            sender_id,
            chat_id,
            &nonce,
            action_type,
            uid,
            tid,
            &Map::new(),
            source_message_id,
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(FeishuBusinessActionReply {
        status: "business_action_pending",
        reply: format!(
            "将执行：无需快递发货\n账号：{uid}\n订单：{} ({tid})\n\n请在 5 分钟内发送：确认 {nonce}",
            display_or_default(&title, "未命名订单")
        ),
    })
}

async fn create_pending_order_adjust_price_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    source_message_id: &str,
    uid: &str,
    tid: &str,
    price: &str,
) -> Result<FeishuBusinessActionReply, String> {
    if !account_uid_belongs_to_owner(state, uid, owner_username).await? {
        return Ok(rejected_reply("闲鱼账号不属于当前绑定 owner。"));
    }
    let Some(title) = order_title(state, uid, tid).await? else {
        return Ok(rejected_reply("订单不存在或不属于该账号。"));
    };
    if parse_positive_amount(price).is_none() {
        return Ok(rejected_reply("订单改价金额无效。"));
    }

    let nonce = generate_nonce();
    let args = Map::from_iter([("price".to_string(), json!(price.trim()))]);
    state
        .storage()
        .create_feishu_pending_action(
            channel_id,
            owner_username,
            sender_id,
            chat_id,
            &nonce,
            "order_adjust_price",
            uid,
            tid,
            &args,
            source_message_id,
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(FeishuBusinessActionReply {
        status: "business_action_pending",
        reply: format!(
            "将执行：订单改价\n账号：{uid}\n订单：{} ({tid})\n新金额：{}\n\n请在 5 分钟内发送：确认 {nonce}",
            display_or_default(&title, "未命名订单"),
            price.trim()
        ),
    })
}

async fn create_pending_order_consign_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    source_message_id: &str,
    uid: &str,
    tid: &str,
    company_code: &str,
    mail_no: &str,
    company_name: &str,
) -> Result<FeishuBusinessActionReply, String> {
    if !account_uid_belongs_to_owner(state, uid, owner_username).await? {
        return Ok(rejected_reply("闲鱼账号不属于当前绑定 owner。"));
    }
    let Some(title) = order_title(state, uid, tid).await? else {
        return Ok(rejected_reply("订单不存在或不属于该账号。"));
    };
    if company_code.trim().is_empty() || mail_no.trim().is_empty() {
        return Ok(rejected_reply("发货需要快递公司编码和运单号。"));
    }

    let nonce = generate_nonce();
    let args = Map::from_iter([
        ("company_code".to_string(), json!(company_code.trim())),
        ("company_name".to_string(), json!(company_name.trim())),
        ("mail_no".to_string(), json!(mail_no.trim())),
    ]);
    state
        .storage()
        .create_feishu_pending_action(
            channel_id,
            owner_username,
            sender_id,
            chat_id,
            &nonce,
            "order_consign",
            uid,
            tid,
            &args,
            source_message_id,
        )
        .await
        .map_err(|error| error.to_string())?;

    Ok(FeishuBusinessActionReply {
        status: "business_action_pending",
        reply: format!(
            "将执行：普通快递发货\n账号：{uid}\n订单：{} ({tid})\n快递：{} {}\n运单号：{}\n\n请在 5 分钟内发送：确认 {nonce}",
            display_or_default(&title, "未命名订单"),
            company_code.trim(),
            company_name.trim(),
            mail_no.trim()
        ),
    })
}

async fn confirm_pending_action(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    sender_id: &str,
    chat_id: &str,
    nonce: &str,
) -> Result<FeishuBusinessActionReply, String> {
    let Some(action) = state
        .storage()
        .consume_feishu_pending_action(channel_id, owner_username, sender_id, chat_id, nonce)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(rejected_reply(
            "确认码无效、已过期、已使用，或不属于当前飞书用户。",
        ));
    };

    if action.owner_username.trim() != owner_username.trim()
        || !account_uid_belongs_to_owner(state, &action.uid, owner_username).await?
    {
        return Ok(rejected_reply("待确认动作不属于当前绑定 owner。"));
    }

    let allow_local_fallback = state
        .storage()
        .get_notification_channel(action.channel_id)
        .await
        .map_err(|error| error.to_string())?
        .map(|channel| config_bool(&channel.config, "allow_local_business_action_fallback"))
        .unwrap_or(false);

    let result = match action.action_type.as_str() {
        "item_downshelf" if allow_local_fallback => {
            run_item_downshelf_action(state, &action.uid, &action.target_id).await
        }
        "item_downshelf" => {
            run_item_downshelf_upstream_action(state, &action.uid, &action.target_id).await
        }
        "item_reduce_price" if allow_local_fallback => {
            let amount = action_arg(&action.args, "amount")?;
            run_item_reduce_price_local_action(
                state,
                &action.uid,
                &action.target_id,
                &Value::String(amount),
            )
            .await
        }
        "item_reduce_price" => {
            let amount = action_arg(&action.args, "amount")?;
            run_item_reduce_price_action(
                state,
                &action.uid,
                &action.target_id,
                &Value::String(amount),
            )
            .await
        }
        "order_dummy_consign" if allow_local_fallback => {
            run_order_consign_dummy_action(state, &action.uid, &action.target_id).await
        }
        "order_dummy_consign" => {
            run_order_consign_dummy_upstream_action(state, &action.uid, &action.target_id).await
        }
        "order_adjust_price" if allow_local_fallback => {
            let price = action_arg(&action.args, "price")?;
            run_order_adjust_price_local_action(state, &action.uid, &action.target_id, &price).await
        }
        "order_adjust_price" => {
            let price = action_arg(&action.args, "price")?;
            run_order_adjust_price_upstream_action(state, &action.uid, &action.target_id, &price)
                .await
        }
        "order_consign" if allow_local_fallback => {
            let company_code = action_arg(&action.args, "company_code")?;
            let company_name = action_arg(&action.args, "company_name")?;
            let mail_no = action_arg(&action.args, "mail_no")?;
            run_order_consign_local_action(
                state,
                &action.uid,
                &action.target_id,
                &company_code,
                &company_name,
                &mail_no,
            )
            .await
        }
        "order_consign" => {
            let company_code = action_arg(&action.args, "company_code")?;
            let company_name = action_arg(&action.args, "company_name")?;
            let mail_no = action_arg(&action.args, "mail_no")?;
            run_order_consign_upstream_action(
                state,
                &action.uid,
                &action.target_id,
                &company_code,
                &company_name,
                &mail_no,
            )
            .await
        }
        other => Err(format!("未知动作类型：{other}")),
    };

    match result {
        Ok(payload) if action_success(&payload) => Ok(FeishuBusinessActionReply {
            status: "business_action_executed",
            reply: format!("已执行：{}", action_label(&action.action_type)),
        }),
        Ok(payload) => Ok(FeishuBusinessActionReply {
            status: "business_action_rejected",
            reply: format!(
                "执行失败：{}",
                payload
                    .get("ret")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "上游返回失败".to_string())
            ),
        }),
        Err(error) => Ok(FeishuBusinessActionReply {
            status: "business_action_rejected",
            reply: format!("执行失败：{}", error.trim()),
        }),
    }
}

async fn item_title(state: &AppState, uid: &str, item_id: &str) -> Result<Option<String>, String> {
    let title = sqlx::query_scalar::<_, String>(
        "SELECT title FROM account_items WHERE uid = ? AND item_id = ? LIMIT 1",
    )
    .bind(uid.trim())
    .bind(item_id.trim())
    .fetch_optional(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    Ok(title)
}

async fn order_title(state: &AppState, uid: &str, tid: &str) -> Result<Option<String>, String> {
    let title = sqlx::query_scalar::<_, String>(
        "SELECT title FROM account_orders WHERE uid = ? AND tid = ? LIMIT 1",
    )
    .bind(uid.trim())
    .bind(tid.trim())
    .fetch_optional(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    Ok(title)
}

fn rejected_reply(reason: &str) -> FeishuBusinessActionReply {
    FeishuBusinessActionReply {
        status: "business_action_rejected",
        reply: format!("无法执行：{}", reason.trim()),
    }
}

fn action_success(payload: &Value) -> bool {
    payload
        .get("ret")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .any(|value| value.starts_with("SUCCESS::"))
        })
        .unwrap_or(false)
        || payload
            .get("data")
            .and_then(|data| data.get("success"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn action_label(action_type: &str) -> &'static str {
    match action_type {
        "item_downshelf" => "下架商品",
        "item_reduce_price" => "商品降价",
        "order_dummy_consign" => "无需快递发货",
        "order_adjust_price" => "订单改价",
        "order_consign" => "普通快递发货",
        _ => "业务动作",
    }
}

fn generate_nonce() -> String {
    format!("{:06X}", rand::random::<u32>() & 0xFF_FFFF)
}

fn config_bool(config: &Map<String, Value>, key: &str) -> bool {
    match config.get(key) {
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_i64() == Some(1),
        Some(Value::String(value)) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        _ => false,
    }
}

fn display_or_default(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn action_arg(args: &Map<String, Value>, key: &str) -> Result<String, String> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if value.is_empty() {
        Err(format!("待确认动作缺少参数：{key}"))
    } else {
        Ok(value)
    }
}

fn parse_positive_amount(value: &str) -> Option<f64> {
    let parsed = value.trim().parse::<f64>().ok()?;
    (parsed > 0.0).then_some(parsed)
}
