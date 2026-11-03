use serde_json::{Map, Value};
use sqlx::Row;

use crate::{
    models::notifications::{FeishuSessionGroupChat, NotificationChannel},
    state::AppState,
};

use super::{
    feishu_events::{FeishuEventReply, FeishuHandledEvent, FeishuImEvent},
    feishu_group_cards::{
        build_feishu_group_main_card, FeishuGroupCardContext, FeishuGroupCardMessage,
    },
    feishu_group_router::mark_group_closed,
    reply_router::send_reply_via_route,
};

enum GroupCommand {
    PlainReply,
    CardReply,
    Item,
    Order,
    Refresh,
    Close,
}

pub(crate) async fn handle_feishu_group_event(
    state: &AppState,
    channel: &NotificationChannel,
    event: &FeishuImEvent,
    runtime_patch: &mut Map<String, Value>,
) -> Result<Option<FeishuHandledEvent>, String> {
    let Some(mapping) = state
        .storage()
        .get_feishu_session_group_chat_by_chat_id(channel.id, &event.chat_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok(None);
    };

    if !group_sender_is_allowed(state, channel.id, &mapping, &event.sender_id).await? {
        return Ok(Some(text_outcome(
            runtime_patch,
            "group_operator_rejected",
            "当前群聊用户尚未绑定该 owner，请先私聊鱼管家机器人完成绑定后再操作当前会话。",
        )));
    }

    record_operator_activity(state, channel.id, &mapping.owner_username, event).await?;
    if !mapping.owner_username.trim().is_empty() {
        runtime_patch.insert(
            "last_feishu_bound_owner".to_string(),
            Value::String(mapping.owner_username.clone()),
        );
    }

    let handled = match parse_group_command(event) {
        GroupCommand::PlainReply | GroupCommand::CardReply => {
            handle_group_reply(state, &mapping, event, runtime_patch).await?
        }
        GroupCommand::Item => handle_group_item(state, &mapping, runtime_patch).await?,
        GroupCommand::Order => handle_group_order(state, &mapping, runtime_patch).await?,
        GroupCommand::Refresh => handle_group_refresh(state, &mapping, runtime_patch).await?,
        GroupCommand::Close => handle_group_close(state, channel, &mapping, runtime_patch).await?,
    };

    Ok(Some(handled))
}

fn parse_group_command(event: &FeishuImEvent) -> GroupCommand {
    let trimmed = event.text.trim();
    if trimmed.eq_ignore_ascii_case("group.reply.send")
        || (trimmed.is_empty() && card_reply_text(event).is_some())
    {
        GroupCommand::CardReply
    } else if trimmed.starts_with("#商品") {
        GroupCommand::Item
    } else if trimmed.starts_with("#订单") {
        GroupCommand::Order
    } else if trimmed.starts_with("#刷新") {
        GroupCommand::Refresh
    } else if trimmed.starts_with("#结束") {
        GroupCommand::Close
    } else {
        GroupCommand::PlainReply
    }
}

async fn handle_group_reply(
    state: &AppState,
    mapping: &FeishuSessionGroupChat,
    event: &FeishuImEvent,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuHandledEvent, String> {
    let reply_text = if event.text.trim().eq_ignore_ascii_case("group.reply.send") {
        card_reply_text(event).unwrap_or_default()
    } else {
        event.text.trim().to_string()
    };
    if reply_text.trim().is_empty() {
        return Ok(text_outcome(
            runtime_patch,
            "group_reply_empty",
            "请输入要回复给闲鱼客户的内容。",
        ));
    }

    let delivery = send_reply_via_route(
        state,
        &mapping.uid,
        &mapping.session_id,
        &mapping.peer_uid,
        &mapping.customer_name,
        &mapping.item_id,
        &mapping.item_title,
        &reply_text,
    )
    .await?;

    runtime_patch.insert(
        "last_xianyu_reply_session_id".to_string(),
        Value::String(delivery.session_id.clone()),
    );
    runtime_patch.insert(
        "last_xianyu_reply_customer_name".to_string(),
        Value::String(delivery.customer_name.clone()),
    );

    Ok(text_outcome(
        runtime_patch,
        "xianyu_replied",
        &format!(
            "已回复闲鱼会话 `{}`（{}）。",
            delivery.session_id,
            display_or_default(&delivery.customer_name, "客户")
        ),
    ))
}

async fn handle_group_item(
    state: &AppState,
    mapping: &FeishuSessionGroupChat,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuHandledEvent, String> {
    let row = sqlx::query(
        r#"
        SELECT item_id, title, price, status, online
        FROM account_items
        WHERE uid = ? AND item_id = ?
        LIMIT 1
        "#,
    )
    .bind(mapping.uid.trim())
    .bind(mapping.item_id.trim())
    .fetch_optional(state.storage().xianyu())
    .await
    .map_err(|err| err.to_string())?;

    let reply = if let Some(row) = row {
        let online = row.get::<i64, _>("online") != 0;
        format!(
            "商品详情\n账号：{}\n商品ID：{}\n标题：{}\n价格：{}\n状态：{}{}",
            mapping.uid,
            row.get::<String, _>("item_id"),
            display_or_default(&row.get::<String, _>("title"), "未命名商品"),
            display_or_default(&row.get::<String, _>("price"), "价格未知"),
            display_or_default(&row.get::<String, _>("status"), "状态未知"),
            if online {
                ""
            } else {
                "\n上下架：已下架"
            }
        )
    } else {
        "当前会话商品尚未同步到本地缓存。".to_string()
    };

    Ok(text_outcome(runtime_patch, "group_item_replied", &reply))
}

async fn handle_group_order(
    state: &AppState,
    mapping: &FeishuSessionGroupChat,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuHandledEvent, String> {
    let rows = sqlx::query(
        r#"
        SELECT tid, title, price, status_text, buyer_nick
        FROM account_orders
        WHERE uid = ?
          AND (
                item_id = ?
                OR buyer_nick = ?
              )
        ORDER BY updated_at DESC, created_at DESC, tid DESC
        LIMIT 3
        "#,
    )
    .bind(mapping.uid.trim())
    .bind(mapping.item_id.trim())
    .bind(mapping.customer_name.trim())
    .fetch_all(state.storage().xianyu())
    .await
    .map_err(|err| err.to_string())?;

    let reply = if rows.is_empty() {
        "当前会话暂未匹配到相关订单。".to_string()
    } else {
        let mut lines = vec!["相关订单".to_string()];
        for row in rows {
            lines.push(format!(
                "- {} | {} | {} | {} | 买家：{}",
                row.get::<String, _>("tid"),
                display_or_default(&row.get::<String, _>("title"), "未命名订单"),
                display_or_default(&row.get::<String, _>("price"), "金额未知"),
                display_or_default(&row.get::<String, _>("status_text"), "状态未知"),
                display_or_default(&row.get::<String, _>("buyer_nick"), "未知"),
            ));
        }
        lines.join("\n")
    };

    Ok(text_outcome(runtime_patch, "group_order_replied", &reply))
}

async fn handle_group_refresh(
    state: &AppState,
    mapping: &FeishuSessionGroupChat,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuHandledEvent, String> {
    let account_name = load_account_name_for_uid(state, &mapping.uid).await?;
    let history = load_group_history(state, mapping).await?;
    let card = build_feishu_group_main_card(group_context(mapping, &account_name), &history);
    Ok(interactive_outcome(runtime_patch, "group_refreshed", card))
}

async fn handle_group_close(
    state: &AppState,
    channel: &NotificationChannel,
    mapping: &FeishuSessionGroupChat,
    runtime_patch: &mut Map<String, Value>,
) -> Result<FeishuHandledEvent, String> {
    mark_group_closed(state, channel, mapping).await?;
    Ok(no_reply_outcome(runtime_patch, "group_closed"))
}

async fn group_sender_is_allowed(
    state: &AppState,
    channel_id: i64,
    mapping: &FeishuSessionGroupChat,
    sender_id: &str,
) -> Result<bool, String> {
    let sender_id = sender_id.trim();
    if sender_id.is_empty() {
        return Ok(false);
    }
    let Some(binding) = state
        .storage()
        .get_feishu_user_binding(channel_id, sender_id)
        .await
        .map_err(|err| err.to_string())?
    else {
        return Ok(false);
    };

    let binding_owner = binding.owner_username.trim();
    let mapping_owner = mapping.owner_username.trim();
    if !mapping_owner.is_empty() {
        Ok(binding.enabled && binding_owner == mapping_owner)
    } else {
        Ok(binding.enabled)
    }
}

async fn record_operator_activity(
    state: &AppState,
    channel_id: i64,
    owner_username: &str,
    event: &FeishuImEvent,
) -> Result<(), String> {
    state
        .storage()
        .upsert_feishu_operator_activity(
            channel_id,
            owner_username,
            &event.sender_id,
            &event.sender_name,
            &event.chat_id,
            &event.event_id,
        )
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn card_reply_text(event: &FeishuImEvent) -> Option<String> {
    event
        .action_form_values
        .get("reply_text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn load_account_name_for_uid(state: &AppState, uid: &str) -> Result<String, String> {
    sqlx::query_scalar::<_, String>("SELECT nick FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid.trim())
        .fetch_optional(state.storage().accounts())
        .await
        .map(|value| value.unwrap_or_else(|| uid.trim().to_string()))
        .map_err(|err| err.to_string())
}

async fn load_group_history(
    state: &AppState,
    mapping: &FeishuSessionGroupChat,
) -> Result<Vec<FeishuGroupCardMessage>, String> {
    let rows = sqlx::query(
        r#"
        SELECT ts, title, text, preview, sender_nick, is_self, kind
        FROM message_history_cache
        WHERE uid = ? AND session_id = ?
        ORDER BY version DESC, ts DESC
        LIMIT 12
        "#,
    )
    .bind(mapping.uid.trim())
    .bind(mapping.session_id.trim())
    .fetch_all(state.storage().live_state())
    .await
    .map_err(|err| err.to_string())?;

    let mut history = rows
        .into_iter()
        .map(|row| {
            let is_self = row.try_get::<i64, _>("is_self").unwrap_or(0) != 0;
            let body = first_non_empty(&[
                row.try_get::<String, _>("text").unwrap_or_default(),
                row.try_get::<String, _>("title").unwrap_or_default(),
                row.try_get::<String, _>("preview").unwrap_or_default(),
            ])
            .unwrap_or_else(|| {
                let kind = row.try_get::<String, _>("kind").unwrap_or_default();
                if kind.trim().is_empty() {
                    "[新消息]".to_string()
                } else {
                    format!("[{}]", kind.trim())
                }
            });
            FeishuGroupCardMessage {
                ts: row.try_get::<i64, _>("ts").unwrap_or_default(),
                sender_label: if is_self {
                    "我".to_string()
                } else {
                    display_or_default(
                        &row.try_get::<String, _>("sender_nick").unwrap_or_default(),
                        &mapping.customer_name,
                    )
                },
                body,
                is_self,
            }
        })
        .collect::<Vec<_>>();
    history.reverse();
    Ok(history)
}

fn group_context(mapping: &FeishuSessionGroupChat, account_name: &str) -> FeishuGroupCardContext {
    FeishuGroupCardContext {
        uid: mapping.uid.clone(),
        account_name: account_name.to_string(),
        customer_name: mapping.customer_name.clone(),
        session_id: mapping.session_id.clone(),
        item_id: mapping.item_id.clone(),
        item_title: mapping.item_title.clone(),
        item_pic: mapping.item_pic.clone(),
        item_price: mapping.item_price.clone(),
        feishu_chat_name: mapping.feishu_chat_name.clone(),
        status: mapping.status.clone(),
    }
}

fn first_non_empty(values: &[String]) -> Option<String> {
    values
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn text_outcome(
    runtime_patch: &mut Map<String, Value>,
    status: &str,
    text: &str,
) -> FeishuHandledEvent {
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String(status.to_string()),
    );
    FeishuHandledEvent {
        status: status.to_string(),
        reply: Some(FeishuEventReply::text(text.to_string())),
    }
}

fn interactive_outcome(
    runtime_patch: &mut Map<String, Value>,
    status: &str,
    content: Value,
) -> FeishuHandledEvent {
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String(status.to_string()),
    );
    FeishuHandledEvent {
        status: status.to_string(),
        reply: Some(FeishuEventReply::interactive(content)),
    }
}

fn no_reply_outcome(runtime_patch: &mut Map<String, Value>, status: &str) -> FeishuHandledEvent {
    runtime_patch.insert(
        "last_feishu_event_status".to_string(),
        Value::String(status.to_string()),
    );
    FeishuHandledEvent {
        status: status.to_string(),
        reply: None,
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
