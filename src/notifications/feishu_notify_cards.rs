use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Local, Utc};
use serde_json::{json, Value};

use crate::routes::accounts::LiveMessageEvent;

const MAX_GROUPS_ON_CARD: usize = 6;
const MAX_MESSAGES_PER_GROUP: usize = 6;

#[derive(Debug, Clone)]
pub(crate) struct FeishuImNotificationCardEvent {
    pub(crate) uid: String,
    pub(crate) account_name: String,
    pub(crate) reply_route_key: String,
    pub(crate) event: LiveMessageEvent,
}

#[derive(Debug, Clone)]
pub(crate) struct FeishuChatCardContext {
    pub(crate) uid: String,
    pub(crate) account_name: String,
    pub(crate) customer_name: String,
    pub(crate) session_id: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
    pub(crate) latest_route_key: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FeishuChatCardMessage {
    pub(crate) ts: i64,
    pub(crate) sender_label: String,
    pub(crate) body: String,
}

pub(crate) fn latest_card_event(
    events: &[FeishuImNotificationCardEvent],
) -> Option<&FeishuImNotificationCardEvent> {
    events.iter().max_by_key(|item| item.event.ts)
}

pub(crate) fn build_feishu_im_aggregate_card(events: &[FeishuImNotificationCardEvent]) -> Value {
    let total = events.len();
    let all_groups = conversation_groups(events);
    if all_groups.len() <= 1 {
        return build_chat_subcard_card(total, &all_groups);
    }
    build_inbox_card(total, &all_groups)
}

pub(crate) fn build_feishu_open_chat_card(
    context: FeishuChatCardContext,
    history: &[FeishuChatCardMessage],
) -> Value {
    let group = conversation_group_from_chat_context(context, history);
    let mut elements = vec![
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**{}**  <font color='grey'>{} · {}</font>",
                    escape_lark_md(&truncate(&group.customer_name, 24)),
                    escape_lark_md(&truncate(&group.account_name, 24)),
                    escape_lark_md(&truncate(&group.item_title, 32))
                ),
            }
        }),
        json!({
            "tag": "note",
            "elements": [{
                "tag": "plain_text",
                "content": "已切到这个客户会话，当前会话已自动设为回复对象。"
            }]
        }),
        json!({ "tag": "hr" }),
    ];

    if group.messages.is_empty() {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": "<font color='grey'>暂无本地历史消息缓存，直接输入消息或发送「回复 &lt;内容&gt;」都可以继续聊天。</font>",
            }
        }));
    } else {
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": chat_flow_markdown(&group),
            }
        }));
    }

    let chat_actions = chat_group_actions(&group);
    if !chat_actions.is_empty() {
        elements.push(json!({
            "tag": "action",
            "actions": chat_actions,
        }));
    }
    elements.push(json!({ "tag": "hr" }));
    elements.push(json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": product_subcard_markdown(&group),
        }
    }));

    let mut subcard_actions = Vec::new();
    if !group.uid.trim().is_empty() && !group.item_id.trim().is_empty() {
        subcard_actions.push(button(
            "商品详情",
            &format!("商品详情 {} {}", group.uid.trim(), group.item_id.trim()),
            false,
        ));
    }
    subcard_actions.push(button("订单详情", "订单列表", false));
    elements.push(json!({
        "tag": "action",
        "actions": subcard_actions,
    }));
    elements.push(json!({
        "tag": "note",
        "elements": [{
            "tag": "plain_text",
            "content": "当前会话已设为回复对象，直接输入消息或发送「回复 <内容>」都可以继续聊天。"
        }]
    }));

    card_shell("客户会话", "indigo", elements)
}

fn build_inbox_card(total: usize, all_groups: &[ConversationGroup]) -> Value {
    let visible_groups = all_groups
        .iter()
        .take(MAX_GROUPS_ON_CARD)
        .collect::<Vec<_>>();
    let hidden_group_count = all_groups.len().saturating_sub(MAX_GROUPS_ON_CARD);
    let account_count = unique_account_count(all_groups);
    let waiting_count = visible_groups.len();
    let mut elements = vec![
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "<text_tag color='red'>{waiting_count} 个待回复</text_tag>  <text_tag color='blue'>{account_count} 个账号</text_tag>  <font color='grey'>像 IM 收件箱一样先选会话，再进入聊天</font>"
                ),
            }
        }),
        json!({
            "tag": "note",
            "elements": [{
                "tag": "plain_text",
                "content": format!("本轮共收到 {total} 条消息，已归并成 {} 个会话。", all_groups.len())
            }]
        }),
    ];

    for (index, group) in visible_groups.iter().enumerate() {
        if index > 0 {
            elements.push(json!({ "tag": "hr" }));
        }
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": inbox_group_markdown(group),
            }
        }));
        let mut actions = vec![button(
            &format!("进入聊天 {}", index + 1),
            &format!("打开会话 {}", group.latest_route_key.trim()),
            true,
        )];
        if !group.uid.trim().is_empty() && !group.item_id.trim().is_empty() {
            actions.push(button(
                "商品",
                &format!("商品详情 {} {}", group.uid.trim(), group.item_id.trim()),
                false,
            ));
        }
        elements.push(json!({
            "tag": "action",
            "actions": actions,
        }));
    }

    if hidden_group_count > 0 {
        elements.push(json!({
            "tag": "note",
            "elements": [{
                "tag": "plain_text",
                "content": format!("还有 {} 个会话未展开，请发送「最近消息」查看更多。", hidden_group_count)
            }]
        }));
    }

    elements.push(json!({
        "tag": "note",
        "elements": [{
            "tag": "plain_text",
            "content": "点“进入聊天”后，会打开会话主卡，并把这个客户设为当前回复对象。"
        }]
    }));

    card_shell("闲鱼消息收件箱", "blue", elements)
}

fn build_chat_subcard_card(total: usize, all_groups: &[ConversationGroup]) -> Value {
    let group = all_groups
        .first()
        .cloned()
        .unwrap_or_else(ConversationGroup::default);
    let mut elements = vec![
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": format!(
                    "**{}**  <font color='grey'>{} · {}</font>",
                    escape_lark_md(&truncate(&group.customer_name, 24)),
                    escape_lark_md(&truncate(&group.account_name, 24)),
                    escape_lark_md(&truncate(&group.item_title, 32))
                ),
            }
        }),
        json!({
            "tag": "note",
            "elements": [{
                "tag": "plain_text",
                "content": format!("本轮共收到 {total} 条消息，当前展示这个客户的连续会话。")
            }]
        }),
        json!({ "tag": "hr" }),
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": chat_flow_markdown(&group),
            }
        }),
        json!({
            "tag": "action",
            "actions": chat_group_actions(&group),
        }),
        json!({ "tag": "hr" }),
        json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": product_subcard_markdown(&group),
            }
        }),
    ];

    let mut subcard_actions = Vec::new();
    if !group.uid.trim().is_empty() && !group.item_id.trim().is_empty() {
        subcard_actions.push(button(
            "商品详情",
            &format!("商品详情 {} {}", group.uid.trim(), group.item_id.trim()),
            false,
        ));
    }
    subcard_actions.push(button("订单详情", "订单列表", false));
    elements.push(json!({
        "tag": "action",
        "actions": subcard_actions,
    }));
    elements.push(json!({
        "tag": "note",
        "elements": [{
            "tag": "plain_text",
            "content": "先点“回复客户”，再发送「回复 <内容>」即可继续聊天。商品和订单入口放在聊天下面，不打断聊天主体。"
        }]
    }));

    card_shell("客户会话", "indigo", elements)
}

fn card_shell(title: &str, template: &str, elements: Vec<Value>) -> Value {
    json!({
        "config": {
            "wide_screen_mode": true,
            "enable_forward": true,
        },
        "header": {
            "template": template,
            "title": {
                "tag": "plain_text",
                "content": title,
            },
        },
        "elements": elements,
    })
}

fn unique_account_count(groups: &[ConversationGroup]) -> usize {
    groups
        .iter()
        .map(|group| group.uid.trim())
        .filter(|uid| !uid.is_empty())
        .collect::<BTreeSet<_>>()
        .len()
}

fn inbox_group_markdown(group: &ConversationGroup) -> String {
    format!(
        "**{}**  <font color='grey'>{}</font>  <text_tag color='red'>待回复</text_tag>\n{} · {}\n<font color='red'>{}</font>",
        escape_lark_md(&truncate(&group.customer_name, 24)),
        format_timestamp_ms(group.latest_ts),
        escape_lark_md(&truncate(&group.item_title, 30)),
        escape_lark_md(&truncate(&group.account_name, 20)),
        escape_lark_md(&truncate(&latest_message_body(group), 80))
    )
}

#[derive(Debug, Clone)]
struct ConversationMessage {
    ts: i64,
    sender_label: String,
    body: String,
}

#[derive(Debug, Clone, Default)]
struct ConversationGroup {
    uid: String,
    account_name: String,
    customer_name: String,
    customer_key: String,
    item_id: String,
    item_title: String,
    item_key: String,
    latest_ts: i64,
    latest_route_key: String,
    total_messages: usize,
    session_ids: BTreeSet<String>,
    messages: Vec<ConversationMessage>,
}

fn conversation_group_from_chat_context(
    context: FeishuChatCardContext,
    history: &[FeishuChatCardMessage],
) -> ConversationGroup {
    let mut session_ids = BTreeSet::new();
    if !context.session_id.trim().is_empty() {
        session_ids.insert(context.session_id.trim().to_string());
    }
    let messages = history
        .iter()
        .map(|message| ConversationMessage {
            ts: message.ts,
            sender_label: if message.sender_label.trim().is_empty() {
                "客户".to_string()
            } else {
                message.sender_label.trim().to_string()
            },
            body: display_or_default(&message.body, "[空消息]"),
        })
        .collect::<Vec<_>>();
    ConversationGroup {
        uid: context.uid.clone(),
        account_name: display_or_default(&context.account_name, &context.uid),
        customer_name: display_or_default(&context.customer_name, "买家"),
        customer_key: display_or_default(&context.customer_name, "customer"),
        item_id: context.item_id.clone(),
        item_title: display_or_default(&context.item_title, "未知商品"),
        item_key: display_or_default(&context.item_id, &context.item_title),
        latest_ts: messages.last().map(|message| message.ts).unwrap_or(0),
        latest_route_key: context.latest_route_key.clone(),
        total_messages: messages.len(),
        session_ids,
        messages,
    }
}

fn conversation_groups(events: &[FeishuImNotificationCardEvent]) -> Vec<ConversationGroup> {
    let mut grouped = BTreeMap::<String, ConversationGroup>::new();
    for item in events {
        let customer_key = display_or_default(
            &identity_or_fallback(&item.event.peer_uid, &item.event.peer_nick),
            "customer",
        );
        let item_key = display_or_default(
            &identity_or_fallback(&item.event.item_id, &item.event.item_title),
            "item",
        );
        let group = grouped
            .entry(format!(
                "{}\t{}\t{}",
                item.uid.trim(),
                customer_key,
                item_key
            ))
            .or_insert_with(|| ConversationGroup {
                uid: item.uid.clone(),
                account_name: display_or_default(&item.account_name, &item.uid),
                customer_name: customer_name(&item.event),
                customer_key: customer_key.clone(),
                item_id: item.event.item_id.trim().to_string(),
                item_title: item_name(&item.event),
                item_key: item_key.clone(),
                ..ConversationGroup::default()
            });
        group.total_messages += 1;
        if !item.event.session_id.trim().is_empty() {
            group
                .session_ids
                .insert(item.event.session_id.trim().to_string());
        }
        group.messages.push(ConversationMessage {
            ts: item.event.ts,
            sender_label: conversation_sender_label(&item.event),
            body: message_body(&item.event),
        });
        if item.event.ts >= group.latest_ts {
            group.latest_ts = item.event.ts;
            group.latest_route_key = item.reply_route_key.trim().to_string();
            if !item.event.item_id.trim().is_empty() {
                group.item_id = item.event.item_id.trim().to_string();
            }
            if !item.event.item_title.trim().is_empty() {
                group.item_title = item.event.item_title.trim().to_string();
            }
        }
    }
    let mut groups = grouped.into_values().collect::<Vec<_>>();
    for group in &mut groups {
        group.messages.sort_by_key(|message| message.ts);
    }
    groups.sort_by(|left, right| {
        right
            .latest_ts
            .cmp(&left.latest_ts)
            .then_with(|| left.account_name.cmp(&right.account_name))
            .then_with(|| left.customer_key.cmp(&right.customer_key))
            .then_with(|| left.item_key.cmp(&right.item_key))
    });
    groups
}

fn chat_flow_markdown(group: &ConversationGroup) -> String {
    let hidden_count = group.messages.len().saturating_sub(MAX_MESSAGES_PER_GROUP);
    let mut lines = group
        .messages
        .iter()
        .rev()
        .take(MAX_MESSAGES_PER_GROUP)
        .collect::<Vec<_>>();
    lines.reverse();
    let mut out = Vec::new();
    for message in lines {
        let color = if message.sender_label == "我" {
            "grey"
        } else {
            "blue"
        };
        out.push(format!(
            "<font color='{color}'>{} {}</font>\n{}",
            escape_lark_md(&message.sender_label),
            format_timestamp_ms(message.ts),
            escape_lark_md(&truncate(&message.body, 160))
        ));
    }
    if hidden_count > 0 {
        out.push(format!(
            "<font color='grey'>... 还有 {} 条更早消息</font>",
            hidden_count
        ));
    }
    out.join("\n\n")
}

fn product_subcard_markdown(group: &ConversationGroup) -> String {
    format!(
        "**商品副卡**\n商品：{}\n账号：{}\n状态：<text_tag color='green'>在售</text_tag>\n会话：`{}`",
        item_display_markdown(group),
        escape_lark_md(&truncate(&group.account_name, 24)),
        escape_lark_md(&display_or_default(first_session_id(group), "未知"))
    )
}

fn chat_group_actions(group: &ConversationGroup) -> Vec<Value> {
    let mut actions = Vec::new();
    if !group.latest_route_key.trim().is_empty() {
        actions.push(button(
            "回复客户",
            &format!("选择回复 {}", group.latest_route_key.trim()),
            true,
        ));
    }
    actions
}

fn button(label: &str, command: &str, primary: bool) -> Value {
    json!({
        "tag": "button",
        "type": if primary { "primary" } else { "default" },
        "text": {
            "tag": "plain_text",
            "content": label,
        },
        "value": {
            "command": command,
        },
    })
}

fn identity_or_fallback(primary: &str, fallback: &str) -> String {
    let primary = primary.trim();
    if !primary.is_empty() {
        primary.to_string()
    } else {
        fallback.trim().to_string()
    }
}

fn customer_name(event: &LiveMessageEvent) -> String {
    if !event.peer_nick.trim().is_empty() {
        event.peer_nick.trim().to_string()
    } else if !event.peer_uid.trim().is_empty() {
        event.peer_uid.trim().to_string()
    } else {
        "买家".to_string()
    }
}

fn conversation_sender_label(event: &LiveMessageEvent) -> String {
    if event.is_self {
        "我".to_string()
    } else {
        "客户".to_string()
    }
}

fn item_name(event: &LiveMessageEvent) -> String {
    if !event.item_title.trim().is_empty() {
        event.item_title.trim().to_string()
    } else if !event.item_id.trim().is_empty() {
        event.item_id.trim().to_string()
    } else {
        "未知商品".to_string()
    }
}

fn message_body(event: &LiveMessageEvent) -> String {
    if !event.text.trim().is_empty() {
        event.text.trim().to_string()
    } else if !event.title.trim().is_empty() {
        event.title.trim().to_string()
    } else if !event.preview.trim().is_empty() {
        event.preview.trim().to_string()
    } else {
        format!("[{}]", event.kind)
    }
}

fn item_display_markdown(group: &ConversationGroup) -> String {
    let title = escape_lark_md(&truncate(&group.item_title, 48));
    if !group.item_id.trim().is_empty() && group.item_title.trim() != group.item_id.trim() {
        format!("{title} (`{}`)", escape_lark_md(group.item_id.trim()))
    } else {
        title
    }
}

fn latest_message_body(group: &ConversationGroup) -> String {
    group
        .messages
        .last()
        .map(|message| message.body.clone())
        .unwrap_or_else(|| "收到新消息".to_string())
}

fn first_session_id(group: &ConversationGroup) -> &str {
    group
        .session_ids
        .iter()
        .next()
        .map(String::as_str)
        .unwrap_or("")
}

fn display_or_default(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn escape_lark_md(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('`', "\\`")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn format_timestamp_ms(ts: i64) -> String {
    if ts <= 0 {
        return "未知时间".to_string();
    }
    DateTime::<Utc>::from_timestamp_millis(ts)
        .map(|value| {
            value
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| ts.to_string())
}

fn truncate(value: &str, max_chars: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        format!(
            "{}...",
            normalized
                .chars()
                .take(max_chars.saturating_sub(3))
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_card_marks_same_session_messages_as_customer_continuation() {
        let events = vec![
            card_event(
                "seller-a",
                "一号卖家",
                "session-a",
                "buyer-a",
                "item-a",
                "第一句",
                1,
            ),
            card_event(
                "seller-a",
                "一号卖家",
                "session-a",
                "buyer-a",
                "item-a",
                "追问一句",
                2,
            ),
        ];

        let card = build_feishu_im_aggregate_card(&events);
        let rendered = card.to_string();

        assert!(rendered.contains("客户会话"), "{rendered}");
        assert!(rendered.contains("第一句"), "{rendered}");
        assert!(rendered.contains("追问一句"), "{rendered}");
        assert!(rendered.contains("回复客户"), "{rendered}");
        assert!(rendered.contains("商品副卡"), "{rendered}");
        assert!(rendered.contains("订单详情"), "{rendered}");
        assert!(!rendered.contains("回复 2"), "{rendered}");
        assert!(!rendered.contains("进入聊天 1"), "{rendered}");
    }

    #[test]
    fn aggregate_card_marks_same_item_multi_session_messages() {
        let events = vec![
            card_event(
                "seller-a",
                "一号卖家",
                "session-a",
                "buyer-a",
                "item-a",
                "这件还在吗",
                1,
            ),
            card_event(
                "seller-a",
                "一号卖家",
                "session-b",
                "buyer-b",
                "item-a",
                "能便宜吗",
                2,
            ),
        ];

        let card = build_feishu_im_aggregate_card(&events);
        let rendered = card.to_string();

        assert!(rendered.contains("闲鱼消息收件箱"), "{rendered}");
        assert!(rendered.contains("这件还在吗"), "{rendered}");
        assert!(rendered.contains("能便宜吗"), "{rendered}");
        assert!(rendered.contains("进入聊天 1"), "{rendered}");
        assert!(rendered.contains("进入聊天 2"), "{rendered}");
    }

    #[test]
    fn aggregate_card_marks_multi_account_messages() {
        let events = vec![
            card_event(
                "seller-a",
                "一号卖家",
                "session-a",
                "buyer-a",
                "item-a",
                "A 账号消息",
                1,
            ),
            card_event(
                "seller-b",
                "二号卖家",
                "session-b",
                "buyer-b",
                "item-b",
                "B 账号消息",
                2,
            ),
        ];

        let card = build_feishu_im_aggregate_card(&events);
        let rendered = card.to_string();

        assert!(rendered.contains("闲鱼消息收件箱"), "{rendered}");
        assert!(rendered.contains("一号卖家"), "{rendered}");
        assert!(rendered.contains("二号卖家"), "{rendered}");
        assert!(rendered.contains("进入聊天 1"), "{rendered}");
    }

    #[test]
    fn aggregate_card_avoids_explicit_table_widths() {
        let events = vec![card_event(
            "seller-a",
            "一号卖家",
            "session-a",
            "buyer-a",
            "item-a",
            "这件还在吗",
            1,
        )];

        let card = build_feishu_im_aggregate_card(&events);
        let rendered = card.to_string();

        assert!(!rendered.contains("\"width\""), "{rendered}");
        assert!(!rendered.contains("\"tag\":\"table\""), "{rendered}");
    }

    fn card_event(
        uid: &str,
        account_name: &str,
        session_id: &str,
        peer_uid: &str,
        item_id: &str,
        text: &str,
        ts: i64,
    ) -> FeishuImNotificationCardEvent {
        FeishuImNotificationCardEvent {
            uid: uid.to_string(),
            account_name: account_name.to_string(),
            reply_route_key: format!("route-{ts}"),
            event: LiveMessageEvent {
                session_id: session_id.to_string(),
                peer_uid: peer_uid.to_string(),
                peer_nick: peer_uid.to_string(),
                item_id: item_id.to_string(),
                item_title: format!("商品 {item_id}"),
                text: text.to_string(),
                ts,
                ..Default::default()
            },
        }
    }
}
