#![allow(dead_code)]

use chrono::{DateTime, Local, Utc};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub(crate) struct FeishuGroupCardContext {
    pub(crate) uid: String,
    pub(crate) account_name: String,
    pub(crate) customer_name: String,
    pub(crate) session_id: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
    pub(crate) item_pic: String,
    pub(crate) item_price: String,
    pub(crate) feishu_chat_name: String,
    pub(crate) status: String,
}

#[derive(Debug, Clone)]
pub(crate) struct FeishuGroupCardMessage {
    pub(crate) ts: i64,
    pub(crate) sender_label: String,
    pub(crate) body: String,
    pub(crate) is_self: bool,
}

pub(crate) fn build_feishu_group_main_card(
    context: FeishuGroupCardContext,
    _history: &[FeishuGroupCardMessage],
) -> Value {
    card_shell(
        &group_title(&context),
        status_template(&context.status),
        vec![summary_block(
            &context,
            status_display_label(&context.status),
        )],
    )
}

pub(crate) fn build_feishu_group_incremental_message(
    context: &FeishuGroupCardContext,
    _message: &FeishuGroupCardMessage,
) -> Value {
    card_shell(
        &group_title(context),
        status_template(&context.status),
        vec![summary_block(
            context,
            status_display_label(&context.status),
        )],
    )
}

pub(crate) fn build_feishu_group_status_card(
    context: &FeishuGroupCardContext,
    status_label: &str,
) -> Value {
    let label = display_or_default(status_label, status_display_label(&context.status));
    card_shell(
        &group_title(context),
        "purple",
        vec![summary_block(context, &label)],
    )
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
                "content": truncate(title, 60),
            },
        },
        "elements": elements,
    })
}

fn summary_block(context: &FeishuGroupCardContext, status_label: &str) -> Value {
    json!({
        "tag": "div",
        "text": {
            "tag": "lark_md",
            "content": group_summary_markdown(context, status_label),
        }
    })
}

fn group_summary_markdown(context: &FeishuGroupCardContext, status_label: &str) -> String {
    let status_color = if context.status.eq_ignore_ascii_case("closed")
        || context.status.eq_ignore_ascii_case("won")
    {
        "green"
    } else {
        "blue"
    };
    format!(
        "**{}**  <font color='grey'>{} · {}</font>\n<text_tag color='{}'>{}</text_tag>  <font color='grey'>会话 {}</font>\n商品：{}{}\n账号：{}",
        escape_lark_md(&truncate(&display_or_default(&context.customer_name, "客户"), 24)),
        escape_lark_md(&truncate(&display_or_default(&context.account_name, &context.uid), 24)),
        escape_lark_md(&truncate(&display_or_default(&context.item_title, "未知商品"), 30)),
        status_color,
        escape_lark_md(status_label),
        escape_lark_md(&display_or_default(&context.session_id, "未知")),
        escape_lark_md(&truncate(&display_or_default(&context.item_title, "未知商品"), 40)),
        price_suffix(&context.item_price),
        escape_lark_md(&truncate(&display_or_default(&context.account_name, &context.uid), 24)),
    )
}

fn sender_label(message: &FeishuGroupCardMessage) -> String {
    if !message.sender_label.trim().is_empty() {
        message.sender_label.trim().to_string()
    } else if message.is_self {
        "我".to_string()
    } else {
        "客户".to_string()
    }
}

fn group_title(context: &FeishuGroupCardContext) -> String {
    display_or_default(&context.feishu_chat_name, "飞书会话群")
}

fn status_template(status: &str) -> &str {
    if status.eq_ignore_ascii_case("closed") || status.eq_ignore_ascii_case("won") {
        "green"
    } else {
        "indigo"
    }
}

fn status_display_label(status: &str) -> &str {
    if status.eq_ignore_ascii_case("closed") {
        "已结束"
    } else if status.eq_ignore_ascii_case("won") {
        "已成交"
    } else if status.eq_ignore_ascii_case("failed") {
        "创建失败"
    } else {
        "进行中"
    }
}

fn price_suffix(price: &str) -> String {
    let trimmed = price.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("  <font color='green'>¥{}</font>", escape_lark_md(trimmed))
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

fn format_timestamp(ts: i64) -> String {
    if ts <= 0 {
        return "未知时间".to_string();
    }
    let millis = if ts >= 1_000_000_000_000 {
        ts
    } else {
        ts.saturating_mul(1000)
    };
    DateTime::<Utc>::from_timestamp_millis(millis)
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
