use rand::seq::IndexedRandom;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    models::business::{AiSettings, ItemReply, Keyword, ReplyMode},
    routes::{
        accounts::{LiveMessageEvent, LiveSendImagePayload, PcSendPayload, ResponseError},
        market::successful_upstream_h5_request,
    },
    state::AppState,
    storage::{is_global_scope_key, unix_millis_now, GLOBAL_AI_SETTINGS_KEY},
    xianyu_client::http::http_client,
};

fn json_log_summary(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().take(16).cloned().collect::<Vec<_>>();
            keys.sort();
            json!({
                "kind": "object",
                "keyCount": map.len(),
                "keys": keys,
                "choicesCount": map.get("choices").and_then(Value::as_array).map(|items| items.len()).unwrap_or(0),
            })
        }
        Value::Array(items) => json!({ "kind": "array", "len": items.len() }),
        Value::String(text) => json!({ "kind": "string", "len": text.chars().count() }),
        Value::Null => json!({ "kind": "null" }),
        Value::Bool(_) => json!({ "kind": "bool" }),
        Value::Number(_) => json!({ "kind": "number" }),
    }
}

async fn bump_message_stat(state: &AppState, uid: &str, field: &str, delta: i64) {
    let account_nick =
        sqlx::query_scalar::<_, String>("SELECT nick FROM accounts WHERE uid = ? LIMIT 1")
            .bind(uid)
            .fetch_optional(state.storage().accounts())
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| uid.to_string());
    let stat_date = sqlx::query_scalar::<_, String>("SELECT date('now','localtime')")
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap_or_else(|_| "1970-01-01".to_string());
    let _ = state
        .storage()
        .bump_message_daily_stat(&stat_date, uid, &account_nick, field, delta)
        .await;
}

const AI_REPLY_MAX_HISTORY: i64 = 10;

const PAID_TRIGGER_PATTERNS: &[&str] = &[
    "[我已付款，等待你发货]",
    "[已付款，待发货]",
    "我已付款，等待你发货",
    "已付款，待发货",
    "[记得及时发货]",
    "记得及时发货",
    "等待你发货",
    "待发货",
    "去发货",
];

const SALE_TRIGGER_PATTERNS: &[&str] = &["卖出宝贝", "宝贝已卖出", "售出宝贝"];

const AUTO_REPLY_DEDUP_TTL_MS: i64 = 60 * 60 * 1000;
const AUTO_RECALL_DELAY_SECS: u64 = 119;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationAction {
    None,
    RepliedText(String),
    RepliedImage(String),
    RepliedBatch { count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplyAction {
    None,
    Sequence(Vec<ReplyAction>),
    Text {
        content: String,
        stat_field: &'static str,
        auto_recall: bool,
    },
    Image {
        url: String,
        stat_field: &'static str,
        auto_recall: bool,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SentMessageRefs {
    message_id: String,
    upstream_message_id: String,
}

pub(crate) async fn process_incoming_message(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
) -> Result<AutomationAction, ResponseError> {
    if event.is_self {
        return Ok(AutomationAction::None);
    }

    wake_auto_delivery_for_event(uid, event).await;
    spawn_auto_receive_red_flower(state, uid, event);

    if event.session_type != 1 {
        return Ok(AutomationAction::None);
    }

    bump_message_stat(state, uid, "received_message_count", 1).await;
    if crate::routes::accounts::messages::is_platform_notification_peer(&event.peer_nick) {
        bump_message_stat(state, uid, "notification_message_count", 1).await;
    } else {
        bump_message_stat(state, uid, "interaction_message_count", 1).await;
    }

    let settings = state
        .storage()
        .get_ai_settings(uid)
        .await
        .map_err(ResponseError::internal)?
        .unwrap_or_default();

    if settings.auto_reply_enabled {
        if should_skip_duplicate_message(state, uid, event)
            .await
            .map_err(ResponseError::internal)?
        {
            return Ok(AutomationAction::None);
        } else {
            let action = resolve_reply_action(state, uid, event, &settings).await?;
            if let Some(outcome) = execute_reply_action(state, uid, event, action).await? {
                return Ok(outcome);
            }
        }
    }

    Ok(AutomationAction::None)
}

fn spawn_auto_receive_red_flower(state: &AppState, uid: &str, event: &LiveMessageEvent) {
    let Some(order_id) = red_flower_event_order_id(event) else {
        return;
    };
    let state_clone = state.clone();
    let uid_owned = uid.to_string();
    tokio::spawn(async move {
        match crate::routes::market::orders::auto_receive_red_flower(
            &state_clone,
            &uid_owned,
            &order_id,
            "live_message_auto",
        )
        .await
        {
            Ok(issued) => {
                tracing::info!(uid=%uid_owned, order_id=%order_id, issued, "auto receive red flower completed");
            }
            Err(error) => {
                tracing::warn!(uid=%uid_owned, order_id=%order_id, error=%error, "auto receive red flower failed");
            }
        }
    });
}

fn red_flower_event_order_id(event: &LiveMessageEvent) -> Option<String> {
    if !looks_like_red_flower_event(event) {
        return None;
    }
    extract_order_id_from_card_meta(&event.card_meta)
}

fn looks_like_red_flower_event(event: &LiveMessageEvent) -> bool {
    [
        event.peer_nick.as_str(),
        event.title.as_str(),
        event.text.as_str(),
        event.preview.as_str(),
    ]
    .iter()
    .any(|value| value.contains("小红花"))
}

fn extract_order_id_from_card_meta(card_meta: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(card_meta).ok()?;
    extract_order_id_from_value(&parsed)
}

fn extract_order_id_from_value(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["orderId", "orderNo", "bizOrderId", "id"] {
                let candidate = map
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim();
                if candidate.chars().all(|ch| ch.is_ascii_digit()) && candidate.len() >= 6 {
                    return Some(candidate.to_string());
                }
            }
            for key in ["linkUrl", "targetUrl", "url"] {
                if let Some(candidate) = map.get(key).and_then(Value::as_str) {
                    if let Some(order_id) = extract_order_id_from_url(candidate) {
                        return Some(order_id);
                    }
                }
            }
            map.values().find_map(extract_order_id_from_value)
        }
        Value::Array(items) => items.iter().find_map(extract_order_id_from_value),
        Value::String(text) => extract_order_id_from_url(text),
        _ => None,
    }
}

fn extract_order_id_from_url(raw: &str) -> Option<String> {
    let marker = "order_detail?id=";
    let start = raw.find(marker)? + marker.len();
    let tail = &raw[start..];
    let digits = tail
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if digits.len() >= 6 {
        Some(digits)
    } else {
        None
    }
}

async fn resolve_reply_action(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    settings: &AiSettings,
) -> Result<ReplyAction, ResponseError> {
    let item_replies = state
        .storage()
        .list_item_replies(uid)
        .await
        .map_err(ResponseError::internal)?;
    let item_mode = if event.item_id.trim().is_empty() {
        None
    } else {
        state
            .storage()
            .get_item_reply_mode(uid, &event.item_id)
            .await
            .map_err(ResponseError::internal)?
            .map(|mode| mode.reply_mode.normalize_item())
    };

    resolve_reply_action_for_mode(
        state,
        uid,
        event,
        settings,
        item_mode.unwrap_or(ReplyMode::FollowGlobal),
        &item_replies,
    )
    .await
}

async fn resolve_reply_action_for_mode(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    settings: &AiSettings,
    mode: ReplyMode,
    item_replies: &[ItemReply],
) -> Result<ReplyAction, ResponseError> {
    let effective_mode = match mode {
        ReplyMode::FollowGlobal => settings.reply_mode.normalize_account(),
        other => other,
    };

    match effective_mode {
        ReplyMode::ItemReply => {
            let matched_item_replies = match_item_replies(item_replies.to_vec(), &event.item_id);
            if let Some(action) = resolve_item_reply_task_action(
                state,
                uid,
                event,
                matched_item_replies,
                "auto_reply_count",
            )
            .await?
            {
                return Ok(action);
            }
            Ok(ReplyAction::None)
        }
        ReplyMode::Keyword => {
            if let Some(keyword) = match_keyword_reply(
                state
                    .storage()
                    .list_keywords_with_global(uid)
                    .await
                    .map_err(ResponseError::internal)?,
                &event.item_id,
                &event.text,
            ) {
                return Ok(reply_action_from_parts(
                    keyword.kind.as_str(),
                    keyword.reply.as_str(),
                    "auto_reply_count",
                    false,
                ));
            }
            Ok(ReplyAction::None)
        }
        ReplyMode::Ai => {
            if let Some(text) = generate_ai_reply(state, uid, event, settings).await? {
                return Ok(ReplyAction::Text {
                    content: text,
                    stat_field: "ai_reply_count",
                    auto_recall: false,
                });
            }
            Ok(ReplyAction::None)
        }
        ReplyMode::Default => {
            let default_replies = match_default_replies(item_replies.to_vec(), &event.item_id);
            if let Some(action) = resolve_item_reply_task_action(
                state,
                uid,
                event,
                default_replies,
                "default_reply_count",
            )
            .await?
            {
                return Ok(action);
            }
            Ok(ReplyAction::None)
        }
        ReplyMode::Off | ReplyMode::FollowGlobal => Ok(ReplyAction::None),
    }
}

async fn execute_reply_action(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    action: ReplyAction,
) -> Result<Option<AutomationAction>, ResponseError> {
    match action {
        ReplyAction::Sequence(actions) => {
            let mut sent = 0_usize;
            let mut last_outcome = None;
            for action in actions {
                if let Some(outcome) =
                    execute_single_reply_action(state, uid, event, action).await?
                {
                    sent += 1;
                    last_outcome = Some(outcome);
                }
            }
            Ok(match (sent, last_outcome) {
                (0, _) => None,
                (1, outcome) => outcome,
                (count, _) => Some(AutomationAction::RepliedBatch { count }),
            })
        }
        other => execute_single_reply_action(state, uid, event, other).await,
    }
}

async fn execute_single_reply_action(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    action: ReplyAction,
) -> Result<Option<AutomationAction>, ResponseError> {
    match action {
        ReplyAction::None => Ok(None),
        ReplyAction::Sequence(_) => Ok(None),
        ReplyAction::Text {
            content,
            stat_field,
            auto_recall,
        } => {
            let sent = send_text_reply(state, uid, event, &content).await?;
            if auto_recall {
                maybe_auto_recall_reply(state, uid, event, &sent).await;
            }
            bump_message_stat(state, uid, stat_field, 1).await;
            Ok(Some(AutomationAction::RepliedText(content)))
        }
        ReplyAction::Image {
            url,
            stat_field,
            auto_recall,
        } => {
            let sent = send_image_reply(state, uid, event, &url).await?;
            if auto_recall {
                maybe_auto_recall_reply(state, uid, event, &sent).await;
            }
            bump_message_stat(state, uid, "image_reply_count", 1).await;
            bump_message_stat(state, uid, stat_field, 1).await;
            Ok(Some(AutomationAction::RepliedImage(url)))
        }
    }
}

fn reply_actions_from_item_replies(
    replies: Vec<ItemReply>,
    uid: &str,
    event: &LiveMessageEvent,
    stat_field: &'static str,
) -> ReplyAction {
    let mut text_actions = Vec::new();
    let mut image_actions = Vec::new();

    for action in replies.into_iter().filter_map(|reply| {
        let rendered = render_template(reply.reply.as_str(), uid, event);
        match reply_action_from_parts(
            reply.kind.as_str(),
            &rendered,
            stat_field,
            reply.auto_recall,
        ) {
            ReplyAction::None => None,
            action => Some(action),
        }
    }) {
        match action {
            ReplyAction::Text { .. } => text_actions.push(action),
            ReplyAction::Image { .. } => image_actions.push(action),
            ReplyAction::None | ReplyAction::Sequence(_) => {}
        }
    }

    let mut selected = Vec::new();
    if let Some(action) = text_actions.choose(&mut rand::rng()).cloned() {
        selected.push(action);
    }
    if let Some(action) = image_actions.choose(&mut rand::rng()).cloned() {
        selected.push(action);
    }

    match selected.len() {
        0 => ReplyAction::None,
        1 => selected.pop().unwrap_or(ReplyAction::None),
        _ => ReplyAction::Sequence(selected),
    }
}

fn reply_action_from_parts(
    kind: &str,
    payload: &str,
    stat_field: &'static str,
    auto_recall: bool,
) -> ReplyAction {
    let body = payload.trim();
    if body.is_empty() {
        return ReplyAction::None;
    }
    if kind.eq_ignore_ascii_case("image") {
        return ReplyAction::Image {
            url: body.to_string(),
            stat_field,
            auto_recall,
        };
    }
    ReplyAction::Text {
        content: body.to_string(),
        stat_field,
        auto_recall,
    }
}

fn match_item_replies(mut replies: Vec<ItemReply>, item_id: &str) -> Vec<ItemReply> {
    replies.retain(|reply| reply.enabled && !reply.reply.trim().is_empty());
    replies.retain(|reply| {
        !is_global_scope_key(reply.cookie_id.as_str())
            && !reply.item_id.trim().is_empty()
            && reply.item_id.trim() == item_id.trim()
    });
    replies.sort_by_key(|reply| (reply.created_at, reply.id));
    replies
}

#[cfg(test)]
fn match_default_reply(replies: Vec<ItemReply>, item_id: &str) -> Option<ItemReply> {
    match_default_replies(replies, item_id).into_iter().next()
}

fn match_default_replies(mut replies: Vec<ItemReply>, _item_id: &str) -> Vec<ItemReply> {
    replies.retain(|reply| reply.enabled && !reply.reply.trim().is_empty());
    replies.retain(|reply| {
        !is_global_scope_key(reply.cookie_id.as_str()) && reply.item_id.trim().is_empty()
    });
    replies.sort_by_key(|reply| (reply.created_at, reply.id));
    replies
}

async fn resolve_item_reply_task_action(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    replies: Vec<ItemReply>,
    stat_field: &'static str,
) -> Result<Option<ReplyAction>, ResponseError> {
    let Some(first_reply) = replies.first().cloned() else {
        return Ok(None);
    };

    let action = reply_actions_from_item_replies(replies, uid, event, stat_field);
    if matches!(action, ReplyAction::None) {
        return Ok(None);
    }

    let allowed = state
        .storage()
        .try_consume_item_reply_task_run(
            uid,
            &event.session_id,
            &first_reply.task_key(),
            first_reply.normalized_reply_count(),
        )
        .await
        .map_err(ResponseError::internal)?;
    if !allowed {
        return Ok(None);
    }

    Ok(Some(action))
}

fn match_keyword_reply(mut rules: Vec<Keyword>, item_id: &str, message: &str) -> Option<Keyword> {
    let normalized = message.trim().to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    rules.retain(|rule| {
        rule.cookie_id != GLOBAL_AI_SETTINGS_KEY
            && rule.enabled
            && !rule.keyword.trim().is_empty()
            && !rule.reply.trim().is_empty()
    });
    rules.sort_by_key(|rule| (rule.item_id.trim().is_empty(), -rule.created_at));
    rules.into_iter().find(|rule| {
        let item_match = rule.item_id.trim().is_empty() || rule.item_id.trim() == item_id.trim();
        item_match && normalized.contains(&rule.keyword.trim().to_lowercase())
    })
}

async fn generate_ai_reply(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    settings: &AiSettings,
) -> Result<Option<String>, ResponseError> {
    if settings.api_key.trim().is_empty() || settings.base_url.trim().is_empty() {
        return Ok(None);
    }

    let intent = detect_intent(&event.text);
    state
        .storage()
        .append_ai_conversation(
            uid,
            &event.session_id,
            &event.item_id,
            &event.peer_uid,
            "user",
            intent,
            &event.text,
        )
        .await
        .map_err(ResponseError::internal)?;

    if intent == "price" && settings.max_bargain_rounds > 0 {
        let bargain_rounds = state
            .storage()
            .count_ai_messages_by_intent(uid, &event.session_id, "user", "price")
            .await
            .map_err(ResponseError::internal)?;
        if bargain_rounds > settings.max_bargain_rounds {
            return Ok(None);
        }
    }

    let history = state
        .storage()
        .list_recent_ai_conversations(uid, &event.session_id, AI_REPLY_MAX_HISTORY)
        .await
        .map_err(ResponseError::internal)?;
    let item_context = load_item_context(state, uid, &event.item_id, &event.item_title)
        .await
        .map_err(ResponseError::internal)?;
    let bargain_rounds = if intent == "price" {
        state
            .storage()
            .count_ai_messages_by_intent(uid, &event.session_id, "user", "price")
            .await
            .map_err(ResponseError::internal)?
    } else {
        0
    };
    let Some(messages) = build_ai_messages(
        event,
        settings,
        intent,
        bargain_rounds,
        history,
        &item_context,
    ) else {
        return Ok(None);
    };

    let payload = json!({
        "model": settings.model_name,
        "messages": messages,
    });

    let client = http_client().map_err(ResponseError::internal_message)?;
    let endpoint = normalize_chat_completion_url(&settings.base_url);
    let response = client
        .post(endpoint)
        .bearer_auth(settings.api_key.trim())
        .json(&payload)
        .send()
        .await
        .map_err(ResponseError::internal)?;
    let status = response.status();
    let raw: Value = response.json().await.map_err(ResponseError::internal)?;
    if !status.is_success() {
        tracing::warn!(
            uid,
            status = %status,
            body_summary = %json_log_summary(&raw),
            "ai reply request failed"
        );
        return Ok(None);
    }

    let text = filter_forbidden_ai_words(&extract_ai_response_text(&raw), settings);
    if text.trim().is_empty() {
        return Ok(None);
    }

    state
        .storage()
        .append_ai_conversation(
            uid,
            &event.session_id,
            &event.item_id,
            &event.peer_uid,
            "assistant",
            intent,
            text.as_str(),
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(Some(text))
}

#[derive(Debug, Default, Clone)]
struct ItemAiContext {
    title: String,
    price: String,
    desc: String,
}

async fn load_item_context(
    state: &AppState,
    uid: &str,
    item_id: &str,
    fallback_title: &str,
) -> Result<ItemAiContext, sqlx::Error> {
    if item_id.trim().is_empty() {
        return Ok(ItemAiContext {
            title: fallback_title.trim().to_string(),
            ..ItemAiContext::default()
        });
    }

    let row = sqlx::query(
        r#"
        SELECT title, price, description, detail_json
        FROM account_items
        WHERE uid = ? AND item_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(item_id)
    .fetch_optional(state.storage().xianyu())
    .await?;

    let Some(row) = row else {
        return Ok(ItemAiContext {
            title: fallback_title.trim().to_string(),
            ..ItemAiContext::default()
        });
    };

    let title = first_non_empty(&[
        row.get::<String, _>("title"),
        fallback_title.trim().to_string(),
    ]);
    let price = row.get::<String, _>("price");
    let desc = first_non_empty(&[
        row.get::<String, _>("description"),
        extract_item_desc_from_detail_json(row.get::<String, _>("detail_json").as_str()),
    ]);
    let mut item = ItemAiContext { title, price, desc };
    if item.desc.trim().is_empty() {
        if let Ok(fetched) = fetch_item_context_from_upstream(state, uid, item_id).await {
            if !fetched.title.trim().is_empty() {
                item.title = fetched.title;
            }
            if !fetched.price.trim().is_empty() {
                item.price = fetched.price;
            }
            if !fetched.desc.trim().is_empty() {
                item.desc = fetched.desc;
            }
        }
    }
    Ok(item)
}

fn build_ai_messages(
    event: &LiveMessageEvent,
    settings: &AiSettings,
    intent: &str,
    bargain_rounds: i64,
    history: Vec<crate::models::business::AiConversation>,
    item: &ItemAiContext,
) -> Option<Vec<Value>> {
    let base_system_prompt = match settings.custom_prompts.get(intent).and_then(Value::as_str) {
        Some(prompt) if !prompt.trim().is_empty() => prompt.trim().to_string(),
        _ => settings
            .custom_prompts
            .get("system")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|prompt| !prompt.is_empty())
            .unwrap_or(default_ai_prompt(intent))
            .to_string(),
    };
    let forbidden_words = ai_forbidden_words(settings);
    let system_prompt = if forbidden_words.is_empty() {
        base_system_prompt
    } else {
        format!(
            "{base}\n\n禁止在回复中使用以下违禁词：{words}。如果买家消息中包含这些词，也不要复述。",
            base = base_system_prompt,
            words = forbidden_words.join("、"),
        )
    };

    let item_title = first_non_empty(&[
        item.title.clone(),
        event.item_title.trim().to_string(),
        "未知商品".to_string(),
    ]);
    let item_price = if item.price.trim().is_empty() {
        "未知".to_string()
    } else {
        item.price.trim().to_string()
    };
    let item_desc = if item.desc.trim().is_empty() {
        "暂无商品描述".to_string()
    } else {
        item.desc.trim().to_string()
    };

    let context_str = history
        .iter()
        .rev()
        .take(AI_REPLY_MAX_HISTORY as usize)
        .rev()
        .map(|row| format!("{}: {}", row.role, row.content.trim()))
        .collect::<Vec<_>>()
        .join("\n");

    let user_prompt = format!(
        "商品信息：\n商品标题: {title}\n商品价格: {price}\n商品描述: {desc}\n\n对话历史：\n{context}\n\n议价设置：\n- 当前议价次数：{bargain_rounds}\n- 最大议价轮数：{max_rounds}\n- 最大优惠百分比：{max_percent}%\n- 最大优惠金额：{max_amount}元\n\n当前买家昵称：{buyer}\n用户消息：{message}\n\n请根据以上信息生成回复：",
        title = item_title,
        price = item_price,
        desc = item_desc,
        context = if context_str.trim().is_empty() {
            "暂无"
        } else {
            context_str.as_str()
        },
        bargain_rounds = bargain_rounds,
        max_rounds = settings.max_bargain_rounds,
        max_percent = settings.max_discount_percent,
        max_amount = settings.max_discount_amount,
        buyer = event.sender_nick.trim(),
        message = event.text.trim(),
    );

    Some(vec![
        json!({
            "role": "system",
            "content": system_prompt,
        }),
        json!({
            "role": "user",
            "content": user_prompt,
        }),
    ])
}

fn ai_forbidden_words(settings: &AiSettings) -> Vec<String> {
    let Some(raw) = settings
        .custom_prompts
        .get("forbiddenWords")
        .and_then(Value::as_str)
    else {
        return Vec::new();
    };
    raw.split(|ch: char| {
        ch == '\n' || ch == ',' || ch == '，' || ch == '、' || ch == ';' || ch == '；'
    })
    .map(str::trim)
    .filter(|word| !word.is_empty())
    .map(ToOwned::to_owned)
    .collect()
}

fn filter_forbidden_ai_words(text: &str, settings: &AiSettings) -> String {
    let forbidden_words = ai_forbidden_words(settings);
    if forbidden_words.is_empty() {
        return text.to_string();
    }
    let normalized_text = text.to_lowercase();
    if forbidden_words
        .iter()
        .any(|word| normalized_text.contains(&word.to_lowercase()))
    {
        tracing::warn!("ai reply blocked by forbidden words");
        return String::new();
    }
    text.to_string()
}

fn default_ai_prompt(intent: &str) -> &'static str {
    match intent {
        "price" => {
            "你是闲鱼卖家的自动客服，当前是询价或议价场景。先结合商品信息、当前议价次数、最大优惠比例和最大优惠金额来判断能否优惠。能优惠时给出明确但克制的口径；不能继续优惠时要礼貌说明；不要一次性把底价说死，不要承诺包邮、赠品或额外服务，除非商品信息里已经明确。"
        }
        "tech" => {
            "你是闲鱼卖家的自动客服，当前是商品功能、使用方式、成色或兼容性咨询场景。只基于现有商品信息和对话内容回答，不确定就明确说需要买家下单前再确认，不要编造参数或功能。"
        }
        "default" => {
            "你是闲鱼卖家的自动客服，当前是普通咨询场景。请直接回答买家的问题，语气自然、简洁、礼貌，优先推动成交，但不要过度营销，不要编造未提供的信息。"
        }
        _ => {
            "你是闲鱼卖家的自动客服。回复要简洁、自然、礼貌，优先直接解决问题。不要编造商品不存在的信息，不要承诺无法确认的售后或赠品，不要输出 Markdown，不要暴露你是程序。"
        }
    }
}

fn extract_item_desc_from_detail_json(raw: &str) -> String {
    let Ok(detail) = serde_json::from_str::<Value>(raw) else {
        return String::new();
    };
    first_non_empty(&[
        detail
            .get("desc")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        detail
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        detail
            .get("itemDesc")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    ])
}

async fn fetch_item_context_from_upstream(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<ItemAiContext, String> {
    let upstream: Value = successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
        "1.0",
        json!({ "itemId": item_id }),
        Vec::new(),
    )
    .await?;
    let item_do = upstream
        .get("data")
        .and_then(|data| data.get(crate::sealed_protocol::sealed_str!("itemDO")))
        .cloned()
        .unwrap_or(Value::Null);
    Ok(ItemAiContext {
        title: item_do
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        price: item_do
            .get("soldPrice")
            .or_else(|| item_do.get("price"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        desc: item_do
            .get("desc")
            .or_else(|| item_do.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
    })
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find_map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_default()
}

fn normalize_chat_completion_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        return trimmed.to_string();
    }
    format!("{trimmed}/chat/completions")
}

fn extract_ai_response_text(raw: &Value) -> String {
    sanitize_outbound_reply_text(
        raw.get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
            .as_str(),
    )
}

fn detect_intent(text: &str) -> &'static str {
    let normalized = text.trim().to_lowercase();
    if normalized.contains('便')
        || normalized.contains("便宜")
        || normalized.contains("最低")
        || normalized.contains("少点")
        || normalized.contains("包邮")
        || normalized.contains("多少钱")
    {
        "price"
    } else if normalized.contains("怎么")
        || normalized.contains("能用")
        || normalized.contains("教程")
        || normalized.contains("功能")
        || normalized.contains("支持")
    {
        "tech"
    } else {
        "default"
    }
}

fn render_template(template: &str, uid: &str, event: &LiveMessageEvent) -> String {
    sanitize_outbound_reply_text(
        template
            .replace("{send_user_name}", event.sender_nick.trim())
            .replace("{send_user_id}", event.peer_uid.trim())
            .replace("{send_message}", event.text.trim())
            .replace("{item_id}", event.item_id.trim())
            .replace("{cookie_id}", uid.trim())
            .as_str(),
    )
}

fn sanitize_outbound_reply_text(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let without_test_prefix = strip_internal_reply_prefix(trimmed).unwrap_or(trimmed);
    without_test_prefix.trim().to_string()
}

fn strip_internal_reply_prefix(raw: &str) -> Option<&str> {
    let colon_index = raw.find(':')?;
    let (prefix, rest_with_colon) = raw.split_at(colon_index);
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return None;
    }
    let is_internal_prefix = prefix.starts_with("AI_")
        || prefix.starts_with("KW_")
        || prefix.starts_with("AUTOTEST_")
        || prefix.starts_with("TEST_");
    if !is_internal_prefix {
        return None;
    }
    Some(rest_with_colon.trim_start_matches(':').trim())
}

async fn should_skip_duplicate_message(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
) -> Result<bool, crate::storage::StorageError> {
    let message_key = if !event.upstream_message_id.trim().is_empty() {
        format!("{uid}:pnm:{}", event.upstream_message_id.trim())
    } else if !event.message_id.trim().is_empty() {
        format!("{uid}:msg:{}", event.message_id.trim())
    } else {
        format!(
            "{uid}:fallback:{}:{}:{}",
            event.session_id.trim(),
            event.peer_uid.trim(),
            event.ts
        )
    };
    let inserted = state
        .storage()
        .record_automation_message(
            &message_key,
            uid,
            &event.session_id,
            &event.peer_uid,
            &event.item_id,
            "reply",
        )
        .await?;
    state
        .storage()
        .delete_old_automation_messages(unix_millis_now() - AUTO_REPLY_DEDUP_TTL_MS)
        .await?;
    Ok(!inserted)
}

fn is_auto_delivery_trigger(text: &str) -> bool {
    let raw = text.trim();
    !raw.is_empty()
        && PAID_TRIGGER_PATTERNS
            .iter()
            .any(|pattern| raw.contains(pattern))
}

fn is_sale_notification_trigger(text: &str) -> bool {
    let raw = text.trim();
    !raw.is_empty()
        && SALE_TRIGGER_PATTERNS
            .iter()
            .any(|pattern| raw.contains(pattern))
}

fn auto_delivery_wake_reason(event: &LiveMessageEvent) -> Option<&'static str> {
    let content = format!("{}\n{}\n{}", event.title, event.text, event.preview);
    if is_auto_delivery_trigger(&content) {
        return Some("paid_message");
    }
    if is_sale_notification_trigger(&content) {
        return Some("sale_notification");
    }
    None
}

async fn wake_auto_delivery_for_event(uid: &str, event: &LiveMessageEvent) {
    if let Some(reason) = auto_delivery_wake_reason(event) {
        super::delivery::wake_account(uid, reason).await;
    }
}

fn sent_message_refs_from_response(payload: &Value) -> SentMessageRefs {
    SentMessageRefs {
        message_id: payload
            .get("messageId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
        upstream_message_id: payload
            .get("upstreamMessageId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

async fn maybe_auto_recall_reply(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    sent: &SentMessageRefs,
) {
    if sent.message_id.trim().is_empty() {
        tracing::warn!(
            uid,
            session_id = %event.session_id,
            "skip auto recall because sent message id is empty"
        );
        return;
    }

    let state = state.clone();
    let uid = uid.to_string();
    let session_id = event.session_id.clone();
    let message_id = sent.message_id.clone();
    let upstream_message_id = sent.upstream_message_id.clone();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(AUTO_RECALL_DELAY_SECS)).await;

        let payload = crate::routes::accounts::messages::LiveRecallMessagePayload {
            uid: uid.clone(),
            session_id: session_id.clone(),
            message_id: message_id.clone(),
            upstream_message_id: upstream_message_id.clone(),
        };

        if let Err(error) = crate::routes::accounts::messages::live_recall_message(
            axum::extract::State(state.clone()),
            system_identity(&state),
            axum::Json(payload),
        )
        .await
        {
            tracing::warn!(
                uid,
                session_id = %session_id,
                message_id = %message_id,
                upstream_message_id = %upstream_message_id,
                error = %error,
                "auto recall failed after delayed default/item reply recall"
            );
        }
    });
}

async fn send_text_reply(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    text: &str,
) -> Result<SentMessageRefs, ResponseError> {
    let payload = PcSendPayload {
        uid: uid.to_string(),
        session_id: event.session_id.clone(),
        to_uid: event.peer_uid.clone(),
        content: text.to_string(),
        reply_to_message_id: String::new(),
        reply_to_text: String::new(),
    };
    let response = crate::routes::accounts::messages::pc_send_message_internal(
        axum::extract::State(state.clone()),
        system_identity(state),
        axum::extract::Json(payload),
    )
    .await?;
    Ok(sent_message_refs_from_response(&response.0))
}

async fn send_image_reply(
    state: &AppState,
    uid: &str,
    event: &LiveMessageEvent,
    image_url: &str,
) -> Result<SentMessageRefs, ResponseError> {
    let payload = LiveSendImagePayload {
        uid: uid.to_string(),
        session_id: event.session_id.clone(),
        to_uid: event.peer_uid.clone(),
        image_url: image_url.to_string(),
        width: 400,
        height: 400,
    };
    let response = crate::routes::accounts::messages::live_send_image_internal(
        axum::extract::State(state.clone()),
        system_identity(state),
        axum::extract::Json(payload),
    )
    .await?;
    Ok(sent_message_refs_from_response(&response.0))
}

fn system_identity(state: &AppState) -> axum::Extension<RequestIdentity> {
    axum::Extension(RequestIdentity::new(
        state.config().auth.username.clone(),
        "admin".to_string(),
        true,
        false,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::business::{ItemReply, Keyword};

    static DELIVERY_WAKE_TEST_LOCK: once_cell::sync::Lazy<tokio::sync::Mutex<()>> =
        once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(()));

    fn item_reply(item_id: &str, reply: &str) -> ItemReply {
        ItemReply {
            item_id: item_id.to_string(),
            reply: reply.to_string(),
            enabled: true,
            kind: "text".to_string(),
            ..ItemReply::default()
        }
    }

    fn scoped_item_reply(cookie_id: &str, item_id: &str, reply: &str, kind: &str) -> ItemReply {
        ItemReply {
            cookie_id: cookie_id.to_string(),
            item_id: item_id.to_string(),
            reply: reply.to_string(),
            enabled: true,
            kind: kind.to_string(),
            ..ItemReply::default()
        }
    }

    fn scoped_item_reply_at(
        cookie_id: &str,
        item_id: &str,
        reply: &str,
        kind: &str,
        created_at: i64,
    ) -> ItemReply {
        ItemReply {
            created_at,
            ..scoped_item_reply(cookie_id, item_id, reply, kind)
        }
    }

    fn keyword(item_id: &str, keyword: &str, reply: &str) -> Keyword {
        Keyword {
            item_id: item_id.to_string(),
            keyword: keyword.to_string(),
            reply: reply.to_string(),
            enabled: true,
            kind: "text".to_string(),
            ..Keyword::default()
        }
    }

    fn scoped_keyword(cookie_id: &str, item_id: &str, word: &str, reply: &str) -> Keyword {
        Keyword {
            cookie_id: cookie_id.to_string(),
            ..keyword(item_id, word, reply)
        }
    }

    fn reply_event(item_id: &str) -> LiveMessageEvent {
        LiveMessageEvent {
            session_id: "session-1".to_string(),
            session_type: 1,
            message_id: "msg-1".to_string(),
            upstream_message_id: "msg-1.PNM".to_string(),
            version: 1,
            ts: 1,
            peer_uid: "peer-1".to_string(),
            peer_nick: "买家".to_string(),
            peer_logo: String::new(),
            peer_role: String::new(),
            self_role: String::new(),
            item_id: item_id.to_string(),
            item_title: "测试商品".to_string(),
            item_pic: String::new(),
            kind: "text".to_string(),
            title: String::new(),
            text: "你好".to_string(),
            image_url: String::new(),
            preview: "你好".to_string(),
            sender_uid: "peer-1".to_string(),
            sender_nick: "买家".to_string(),
            sender_logo: String::new(),
            is_self: false,
            unread_count: 0,
            accs_pts: 0,
            card_meta: String::new(),
            content_type: 1,
            reply_to_message_id: String::new(),
        }
    }

    #[test]
    fn item_replies_return_all_entries_for_same_item_oldest_first() {
        let hits = match_item_replies(
            vec![
                scoped_item_reply_at("seller-001", "item-2", "第二条", "text", 20),
                scoped_item_reply_at("seller-001", "item-2", "第一条", "text", 10),
                scoped_item_reply_at(GLOBAL_AI_SETTINGS_KEY, "item-2", "旧全局条目", "text", 5),
                item_reply("item-1", "hello"),
            ],
            "item-2",
        );
        let replies = hits
            .iter()
            .map(|reply| reply.reply.as_str())
            .collect::<Vec<_>>();
        assert_eq!(replies, vec!["第一条", "第二条"]);
    }

    #[test]
    fn keyword_reply_prefers_item_bound_rule() {
        let hit = match_keyword_reply(
            vec![
                keyword("", "多少钱", "通用"),
                keyword("item-9", "多少钱", "专用"),
            ],
            "item-9",
            "这款多少钱",
        )
        .expect("should match");
        assert_eq!(hit.reply, "专用");
    }

    #[test]
    fn keyword_reply_ignores_global_template_pool_rules_at_runtime() {
        let hit = match_keyword_reply(
            vec![
                scoped_keyword(GLOBAL_AI_SETTINGS_KEY, "", "激活码", "全局关键词"),
                scoped_keyword("seller-001", "", "激活码", "账号关键词"),
            ],
            "item-9",
            "我要激活码",
        )
        .expect("should match");
        assert_eq!(hit.reply, "账号关键词");

        assert!(match_keyword_reply(
            vec![scoped_keyword(
                GLOBAL_AI_SETTINGS_KEY,
                "",
                "激活码",
                "全局关键词"
            )],
            "item-9",
            "我要激活码",
        )
        .is_none());
    }

    #[test]
    fn default_reply_falls_back_to_empty_item_id() {
        let hit = match_default_reply(
            vec![item_reply("", "默认"), item_reply("item-1", "专用")],
            "item-x",
        )
        .expect("should match");
        assert_eq!(hit.reply, "默认");
    }

    #[test]
    fn default_reply_ignores_global_template_pool_rules() {
        let hit = match_default_reply(
            vec![
                scoped_item_reply(GLOBAL_AI_SETTINGS_KEY, "", "全局默认", "text"),
                scoped_item_reply("seller-001", "", "账号默认", "text"),
            ],
            "item-x",
        )
        .expect("should match");
        assert_eq!(hit.reply, "账号默认");

        assert!(match_default_reply(
            vec![scoped_item_reply(
                GLOBAL_AI_SETTINGS_KEY,
                "",
                "全局默认",
                "text"
            )],
            "item-x",
        )
        .is_none());
    }

    #[test]
    fn default_replies_return_all_account_entries_oldest_first() {
        let hits = match_default_replies(
            vec![
                scoped_item_reply_at(
                    "seller-001",
                    "",
                    "https://example.com/reply.png",
                    "image",
                    20,
                ),
                scoped_item_reply_at("seller-001", "", "可以的", "text", 10),
                scoped_item_reply_at(GLOBAL_AI_SETTINGS_KEY, "", "旧全局默认", "text", 5),
                scoped_item_reply_at("seller-001", "item-other", "其他商品专用", "text", 30),
            ],
            "item-x",
        );

        let replies = hits
            .iter()
            .map(|reply| reply.reply.as_str())
            .collect::<Vec<_>>();
        assert_eq!(replies, vec!["可以的", "https://example.com/reply.png"]);
    }

    #[test]
    fn reply_action_returns_image_for_image_kind() {
        let hit = reply_action_from_parts(
            "image",
            "https://example.com/reply.png",
            "image_reply_count",
            false,
        );
        assert_eq!(
            hit,
            ReplyAction::Image {
                url: "https://example.com/reply.png".to_string(),
                stat_field: "image_reply_count",
                auto_recall: false,
            }
        );
    }

    #[test]
    fn item_reply_actions_randomly_pick_one_text_and_one_image() {
        let action = reply_actions_from_item_replies(
            vec![
                ItemReply {
                    auto_recall: true,
                    reply_count: 2,
                    ..scoped_item_reply_at("seller-001", "item-2", "第一条", "text", 10)
                },
                ItemReply {
                    auto_recall: true,
                    reply_count: 2,
                    ..scoped_item_reply_at(
                        "seller-001",
                        "item-2",
                        "https://example.com/reply.png",
                        "image",
                        20,
                    )
                },
            ],
            "seller-001",
            &reply_event("item-2"),
            "default_reply_count",
        );

        match action {
            ReplyAction::Sequence(actions) => {
                assert_eq!(actions.len(), 2);
                assert!(matches!(
                    actions[0],
                    ReplyAction::Text {
                        ref content,
                        stat_field: "default_reply_count",
                        auto_recall: true,
                    } if content == "第一条"
                ));
                assert!(matches!(
                    actions[1],
                    ReplyAction::Image {
                        ref url,
                        stat_field: "default_reply_count",
                        auto_recall: true,
                    } if url == "https://example.com/reply.png"
                ));
            }
            other => panic!("expected a mixed reply sequence, got {other:?}"),
        }
    }

    #[test]
    fn auto_recall_delay_matches_two_minute_window() {
        assert_eq!(AUTO_RECALL_DELAY_SECS, 119);
    }

    #[test]
    fn detect_intent_marks_price_related_messages() {
        assert_eq!(detect_intent("这个还能便宜点吗"), "price");
        assert_eq!(detect_intent("怎么使用"), "tech");
        assert_eq!(detect_intent("你好"), "default");
    }

    #[test]
    fn paid_trigger_patterns_cover_common_system_text() {
        assert!(is_auto_delivery_trigger("[我已付款，等待你发货]"));
        assert!(is_auto_delivery_trigger("已付款，待发货"));
        assert!(is_auto_delivery_trigger("记得及时发货"));
        assert!(is_auto_delivery_trigger(
            "如已发货，请点击「去发货」输入快递单号"
        ));
        assert!(!is_auto_delivery_trigger("我已拍下，待付款"));
        assert!(!is_auto_delivery_trigger("你好，在吗"));
    }

    #[test]
    fn paid_trigger_checks_order_card_title_and_preview() {
        let event = LiveMessageEvent {
            session_id: "61825916223".to_string(),
            session_type: 1,
            message_id: "msg-1".to_string(),
            upstream_message_id: "msg-1.PNM".to_string(),
            version: 1,
            ts: 1,
            peer_uid: "2626821437".to_string(),
            peer_nick: "吉恩斯弗兰克".to_string(),
            peer_logo: String::new(),
            peer_role: String::new(),
            self_role: String::new(),
            item_id: "1053115290129".to_string(),
            item_title: "windsurf200刀制作方法".to_string(),
            item_pic: String::new(),
            kind: "card".to_string(),
            title: "我已付款，等待你发货".to_string(),
            text: "请包装好商品，并按我在闲鱼上提供的地址发货".to_string(),
            image_url: String::new(),
            preview: "我已付款，等待你发货".to_string(),
            sender_uid: "2626821437".to_string(),
            sender_nick: "吉恩斯弗兰克".to_string(),
            sender_logo: String::new(),
            is_self: false,
            unread_count: 0,
            accs_pts: 0,
            card_meta: String::new(),
            content_type: 26,
            reply_to_message_id: String::new(),
        };

        assert!(auto_delivery_wake_reason(&event).is_some());
    }

    #[tokio::test]
    async fn paid_event_only_emits_delivery_wake_signal() {
        let _guard = DELIVERY_WAKE_TEST_LOCK.lock().await;
        let _ = super::super::delivery::take_test_account_wakes();
        let event = LiveMessageEvent {
            title: "我已付款，等待你发货".to_string(),
            ..Default::default()
        };

        wake_auto_delivery_for_event("seller-1", &event).await;

        assert_eq!(
            super::super::delivery::take_test_account_wakes(),
            vec![("seller-1".to_string(), "paid_message".to_string())]
        );
    }

    #[test]
    fn sale_notification_is_an_immediate_delivery_sync_signal() {
        let event = LiveMessageEvent {
            session_type: 3,
            peer_nick: "通知消息".to_string(),
            title: "卖出宝贝奖励你150闲鱼币".to_string(),
            text: "你有闲鱼币待领取，今日有效".to_string(),
            ..Default::default()
        };

        assert_eq!(auto_delivery_wake_reason(&event), Some("sale_notification"));
    }

    #[tokio::test]
    async fn delivery_wake_does_not_depend_on_legacy_auto_ship_flag() {
        let _guard = DELIVERY_WAKE_TEST_LOCK.lock().await;
        let _ = super::super::delivery::take_test_account_wakes();
        let event = LiveMessageEvent {
            title: "我已付款，等待你发货".to_string(),
            ..Default::default()
        };

        wake_auto_delivery_for_event("seller-configured", &event).await;

        assert_eq!(
            super::super::delivery::take_test_account_wakes(),
            vec![("seller-configured".to_string(), "paid_message".to_string())]
        );
    }

    #[test]
    fn sanitize_outbound_reply_text_removes_internal_test_prefixes() {
        assert_eq!(
            sanitize_outbound_reply_text("AI_OK_20260517: 功能正常，插卡即用，300元不议价。"),
            "功能正常，插卡即用，300元不议价。"
        );
        assert_eq!(
            sanitize_outbound_reply_text("KW_OK_20260517: 这是关键词自动回复"),
            "这是关键词自动回复"
        );
        assert_eq!(
            sanitize_outbound_reply_text("测试通过，不带前缀"),
            "测试通过，不带前缀"
        );
    }

    #[test]
    fn red_flower_card_meta_extracts_order_detail_id() {
        let event = LiveMessageEvent {
            peer_nick: "闲鱼小红花".to_string(),
            title: "你收到一朵小红花".to_string(),
            card_meta: json!({
                "linkUrl": "fleamarket://order_detail?id=5126452633611015400&role=Seller"
            })
            .to_string(),
            ..Default::default()
        };

        assert_eq!(
            red_flower_event_order_id(&event),
            Some("5126452633611015400".to_string())
        );
    }

    #[test]
    fn non_red_flower_event_does_not_trigger_auto_receive() {
        let event = LiveMessageEvent {
            peer_nick: "通知消息".to_string(),
            title: "卖出宝贝奖励你150闲鱼币".to_string(),
            card_meta: json!({
                "linkUrl": "fleamarket://order_detail?id=5126452633611015400&role=Seller"
            })
            .to_string(),
            ..Default::default()
        };

        assert_eq!(red_flower_event_order_id(&event), None);
    }
}
