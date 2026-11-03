//! Session identity normalization and UID/item utilities.
//! Message content normalization lives in `normalize_content.rs`.

use serde_json::{json, Map, Value};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::xianyu_client::LocationContext;

use super::{
    coerce_mtop_data, json_array, json_bool, json_i64, json_object, json_string,
    load_single_session_info, peer_nick_needs_fallback, sanitized_peer_nick_candidate,
    MessageHistoryEntry, MessageHistoryPage, MessageSessionIdentity, NormalizedMessageContent,
};

pub use normalize_content::normalize_message_content;

mod normalize_content;

pub(crate) fn extract_peer_uid_from_reminder_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(peer_uid) = extract_peer_uid_from_reminder_url_once(trimmed) {
        return peer_uid;
    }
    if trimmed.contains('%') {
        if let Ok(decoded) = urlencoding::decode(trimmed) {
            if let Some(peer_uid) = extract_peer_uid_from_reminder_url_once(decoded.as_ref()) {
                return peer_uid;
            }
        }
    }
    String::new()
}

fn extract_peer_uid_from_reminder_url_once(raw: &str) -> Option<String> {
    ROOT_EMBEDDED_PEER_UID_RE
        .captures(raw)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().to_string())
}

// ---------------------------------------------------------------------------
// Session identity normalization
// ---------------------------------------------------------------------------

pub fn normalize_message_session_identity_from_session(
    session: &Map<String, Value>,
    current_uid: &str,
) -> MessageSessionIdentity {
    let (self_user, peer_user) = resolve_session_participants(session, current_uid);
    let (self_role, peer_role) = resolve_trade_roles(session, current_uid, &self_user, &peer_user);
    let item_info = json_object(session.get("itemInfo"));
    let attribute_map = json_object(session.get("attributeMap"));
    let peer_uid = json_string(peer_user.get("userId"));

    // Real mtop `session.sync` puts both `squadName_{seller_uid}` and
    // `squadName_{buyer_uid}` into `attributeMap`:
    //   - squadName_{seller} = the real item title for this conversation
    //   - squadName_{buyer}  = the buyer's own squad-list entry (a different
    //     item the buyer cares about) — looks plausible but is the wrong title
    // The seller is `itemInfo.sellerInfo.userId` (or the `ownerInfo.userId`
    // fallback for the older owner/user shape), and is **never** the peer when
    // the current account is the seller. Keying on `peer_uid` was therefore
    // wrong on ~52% of sessionType=1 rows where the user is the seller.
    let seller_uid = {
        let from_item = json_string(json_object(item_info.get("sellerInfo")).get("userId"));
        if !from_item.is_empty() {
            from_item
        } else {
            json_string(json_object(session.get("ownerInfo")).get("userId"))
        }
    };
    let mut item_title = if !seller_uid.is_empty() {
        json_string(attribute_map.get(&format!("squadName_{seller_uid}")))
    } else {
        String::new()
    };
    // Older fixtures and a few legacy responses still only carry
    // `squadName_{peer_uid}` — fall back to that, then to the bare
    // `squadName` key, before giving up.
    if item_title.is_empty() && !peer_uid.is_empty() {
        item_title = json_string(attribute_map.get(&format!("squadName_{peer_uid}")));
    }
    if item_title.is_empty() {
        item_title = json_string(attribute_map.get("squadName"));
    }
    if item_title.is_empty() {
        item_title = single_squad_name_title(&attribute_map);
    }
    if item_title.is_empty() {
        item_title = first_message_session_detail_text(
            &attribute_map,
            &["itemTitle", "title", "semanticsDesc"],
        );
    }

    let peer_nick = sanitized_peer_nick_candidate(&message_user_display(&peer_user), &peer_uid);
    // 闲鱼 platform notification-center / workbench virtual sessions
    // ("工作台通知", "通知中心o重要", "通知中心o全部", "通知中心o处置",
    // "通知中心o订单", ...) are returned with whatever sessionType the
    // upstream query filter asked for: `[1,3,6]` returns them as type=1,
    // `[53]` returns them as type=53. The phone UI collapses them into a
    // single pinned "工作台通知" entry — we mirror that by force-classifying
    // them to sessionType=53 here so they never leak into the chat list.
    // Detection is by peer_nick prefix because the platform nicks are stable
    // identifiers; peer_uids (synthetic `2222xxxxxxxxxx`) overlap with other
    // platform channels so they're a less reliable key.
    let raw_session_type = json_i64(session.get("sessionType")).max(1);
    let session_type = if is_platform_notification_peer(&peer_nick) {
        53
    } else {
        raw_session_type
    };
    MessageSessionIdentity {
        session_id: json_string(session.get("sessionId")),
        session_type,
        version: 0,
        unread: 0,
        latest_message_id: String::new(),
        peer_uid,
        peer_nick,
        peer_logo: json_string(peer_user.get("logo")),
        peer_role,
        self_role,
        item_id: extract_item_id_from_value(&Value::Object(item_info.clone()))
            .or_else(|| extract_item_id_from_value(&Value::Object(attribute_map.clone())))
            .unwrap_or_default(),
        item_title,
        item_pic: json_string(item_info.get("mainPic")),
        summary: String::new(),
        ts: 0,
        sort_index: 0,
    }
}

fn single_squad_name_title(object: &Map<String, Value>) -> String {
    let mut values = object
        .iter()
        .filter(|(key, _)| key.starts_with("squadName_"))
        .filter_map(|(_, value)| {
            let text = json_string(Some(value));
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        })
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    if values.len() == 1 {
        values.pop().unwrap_or_default()
    } else {
        String::new()
    }
}

fn first_message_session_detail_text(object: &Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        let candidate = json_string(object.get(*key));
        if !candidate.is_empty() {
            return candidate;
        }
    }
    String::new()
}

/// Detect 闲鱼 platform notification-center / workbench virtual sessions by
/// their stable upstream peer nick. The upstream returns these with whichever
/// sessionType the `pc.session.sync` filter asked for (1 or 53), so we can't
/// trust the response's sessionType alone — without this fixup the same
/// underlying session leaks into the chat list as a duplicate-looking type=1
/// row whenever the user triggers a force refresh.
pub(crate) fn is_platform_notification_peer(peer_nick: &str) -> bool {
    let trimmed = peer_nick.trim();
    trimmed == "工作台通知" || trimmed.starts_with("通知中心")
}

pub fn resolve_session_participants(
    session: &Map<String, Value>,
    current_uid: &str,
) -> (Map<String, Value>, Map<String, Value>) {
    let users = json_array(session.get("users"))
        .into_iter()
        .filter_map(|user| user.as_object().cloned())
        .collect::<Vec<_>>();
    if !users.is_empty() {
        if !current_uid.trim().is_empty() {
            if let Some(index) = users
                .iter()
                .position(|user| message_uids_match(&json_string(user.get("userId")), current_uid))
            {
                let self_user = users.get(index).cloned().unwrap_or_default();
                let peer_user = users
                    .iter()
                    .enumerate()
                    .find(|(peer_index, user)| {
                        *peer_index != index && !json_string(user.get("userId")).is_empty()
                    })
                    .map(|(_, user)| user.clone())
                    .unwrap_or_default();
                return (self_user, peer_user);
            }
        }
        let self_user = users.first().cloned().unwrap_or_default();
        let self_uid = json_string(self_user.get("userId"));
        let peer_user = users
            .iter()
            .find(|user| json_string(user.get("userId")) != self_uid)
            .cloned()
            .unwrap_or_default();
        return (self_user, peer_user);
    }

    let owner = json_object(session.get("ownerInfo"));
    let user = json_object(session.get("userInfo"));
    let owner_uid = json_string(owner.get("userId"));
    let user_uid = json_string(user.get("userId"));
    if !current_uid.trim().is_empty() {
        if message_uids_match(&owner_uid, current_uid)
            && !message_uids_match(&user_uid, current_uid)
        {
            return (owner, user);
        }
        if message_uids_match(&user_uid, current_uid)
            && !message_uids_match(&owner_uid, current_uid)
        {
            return (user, owner);
        }
    }
    if json_i64(session.get("sessionType")) == 1 {
        return (user, owner);
    }
    (owner, user)
}

pub async fn merge_cached_session_identity(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    session_type: i64,
    session: &mut MessageSessionIdentity,
) -> Result<(), sqlx::Error> {
    use super::peer_nick_needs_fallback;
    let Some(cached) = load_single_session_info(pool, uid, session_id, session_type).await? else {
        return Ok(());
    };
    if session.peer_uid.trim().is_empty() {
        session.peer_uid = json_string(cached.get("peerUid"));
    }
    let cached_nick = json_string(cached.get("peerNick"));
    if peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid)
        && !peer_nick_needs_fallback(&cached_nick, &session.peer_uid)
    {
        session.peer_nick = cached_nick;
    } else if session.peer_nick.trim().is_empty() {
        session.peer_nick = cached_nick;
    }
    if session.peer_logo.trim().is_empty() {
        session.peer_logo = json_string(cached.get("peerLogo"));
    }
    if session.peer_role.trim().is_empty() {
        session.peer_role = json_string(cached.get("peerRole"));
    }
    if session.self_role.trim().is_empty() {
        session.self_role = json_string(cached.get("selfRole"));
    }
    if session.item_id.trim().is_empty() {
        session.item_id = json_string(cached.get("itemId"));
    }
    if session.item_title.trim().is_empty() {
        session.item_title = json_string(cached.get("itemTitle"));
    }
    if session.item_pic.trim().is_empty() {
        session.item_pic = json_string(cached.get("itemPic"));
    }
    if session.summary.trim().is_empty() {
        session.summary = json_string(cached.get("summary"));
    }
    if session.ts <= 0 {
        session.ts = json_i64(cached.get("ts"));
    }
    if session.sort_index <= 0 {
        session.sort_index = json_i64(cached.get("sortIndex"));
    }
    if session.version <= 0 {
        session.version = json_i64(cached.get("version"));
    }
    Ok(())
}

pub fn resolve_trade_roles(
    session: &Map<String, Value>,
    current_uid: &str,
    self_user: &Map<String, Value>,
    peer_user: &Map<String, Value>,
) -> (String, String) {
    if json_i64(session.get("sessionType")) != 1 {
        return (String::new(), String::new());
    }
    let item_info = json_object(session.get("itemInfo"));
    let seller_uid = json_string(json_object(item_info.get("sellerInfo")).get("userId"));
    let self_uid = json_string(self_user.get("userId"));
    let peer_uid = json_string(peer_user.get("userId"));

    if !seller_uid.is_empty() {
        if message_uids_match(&self_uid, &seller_uid)
            || message_uids_match(current_uid, &seller_uid)
        {
            return ("seller".to_string(), "buyer".to_string());
        }
        if message_uids_match(&peer_uid, &seller_uid) {
            return ("buyer".to_string(), "seller".to_string());
        }
    }

    let owner = json_object(session.get("ownerInfo"));
    let user = json_object(session.get("userInfo"));
    let owner_uid = json_string(owner.get("userId"));
    let user_uid = json_string(user.get("userId"));

    if message_uids_match(&self_uid, &owner_uid) || message_uids_match(current_uid, &owner_uid) {
        return ("seller".to_string(), "buyer".to_string());
    }
    if message_uids_match(&self_uid, &user_uid) || message_uids_match(current_uid, &user_uid) {
        return ("buyer".to_string(), "seller".to_string());
    }
    if message_uids_match(&peer_uid, &owner_uid) {
        return ("buyer".to_string(), "seller".to_string());
    }
    if message_uids_match(&peer_uid, &user_uid) {
        return ("seller".to_string(), "buyer".to_string());
    }
    (String::new(), String::new())
}

// ---------------------------------------------------------------------------
// History normalization helpers (used by history.rs)
// ---------------------------------------------------------------------------

fn coerce_session_timestamp(value: i64) -> i64 {
    use super::super::live::try_parse_date_format_timestamp;
    if value <= 0 {
        return 0;
    }
    if let Some(ms) = try_parse_date_format_timestamp(value) {
        return ms;
    }
    if value < 10_000_000_000 {
        return value * 1000;
    }
    value
}

fn normalize_message_session_entry(
    raw: &Value,
    current_uid: &str,
) -> Option<MessageSessionIdentity> {
    let session = json_object(raw.get("session"));
    if session.is_empty() {
        return None;
    }
    let mut normalized = normalize_message_session_identity_from_session(&session, current_uid);
    let summary = json_object(json_object(raw.get("message")).get("summary"));
    normalized.summary = json_string(summary.get("summary"));
    normalized.unread = json_i64(summary.get("unread"));
    normalized.ts = coerce_session_timestamp(json_i64(summary.get("ts")));
    normalized.version = coerce_session_timestamp(
        json_i64(summary.get("version")).max(json_i64(summary.get("sortIndex"))),
    );
    if normalized.version <= 0 {
        normalized.version = normalized.ts;
    }
    normalized.sort_index = coerce_session_timestamp(json_i64(summary.get("sortIndex")));
    if normalized.sort_index <= 0 {
        normalized.sort_index = normalized.ts.max(normalized.version);
    }
    if normalized.ts <= 0 {
        normalized.ts = normalized.sort_index.max(normalized.version);
    }
    Some(normalized)
}

pub fn normalize_message_sessions(
    response: &Value,
    current_uid: &str,
) -> Vec<MessageSessionIdentity> {
    let data = coerce_mtop_data(response);
    json_array(data.get("sessions"))
        .into_iter()
        .filter_map(|raw| normalize_message_session_entry(&raw, current_uid))
        .collect()
}

fn convert_ws_message_model(model: &Value) -> Option<Value> {
    let model = json_object(Some(model));
    let msg = json_object(model.get("message"));
    if msg.is_empty() {
        return None;
    }
    let ext = json_object(msg.get("extension"));
    let content_wrap = json_object(msg.get("content"));
    let custom = json_object(content_wrap.get("custom"));
    let sender_raw = json_object(msg.get("sender"));

    let sender_uid_raw = json_string(sender_raw.get("uid"));
    let sender_uid_from_sender = sender_uid_raw
        .split('@')
        .next()
        .unwrap_or_default()
        .to_string();
    let sender_uid = if sender_uid_from_sender.is_empty() {
        json_string(ext.get("senderUserId"))
    } else {
        sender_uid_from_sender
    };
    let cid = json_string(msg.get("cid"));
    let session_id = cid.split('@').next().unwrap_or_default().to_string();
    let ts = json_i64(msg.get("createAt"));

    let mut decoded_content = custom
        .get("data")
        .and_then(Value::as_str)
        .and_then(|raw| {
            BASE64_STANDARD
                .decode(raw)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        })
        .unwrap_or(Value::Null);
    if !decoded_content.is_object() {
        let summary = json_string(custom.get("summary"));
        if !summary.is_empty() {
            decoded_content = serde_json::json!({
                "contentType": 1,
                "text": { "text": summary },
            });
        }
    }

    let ext_json_str = json_string(ext.get("extJson"));
    // extJson 是字符串化的 JSON（"{\"messageId\":\"...\",\"replyMessageId\":\"...\"}"）。
    // 一次性解开，下面同时拎 messageId（前端主键 UUID）+ replyMessageId
    // （被引用的 PNM）。replyContent 字段闲鱼 WS 不下发，所以引用气泡的文本
    // 要靠 frontend 客户端在历史里按 PNM 反查。
    let ext_parsed = serde_json::from_str::<Value>(&ext_json_str).unwrap_or(Value::Null);
    let ext_message_id = ext_parsed
        .get("messageId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    let reply_to_message_id = ext_parsed
        .get("replyMessageId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_default();
    let reminder_url = json_string(ext.get("reminderUrl"));
    let peer_uid_from_reminder = extract_peer_uid_from_reminder_url(&reminder_url);
    let reminder_nick_uid = if !peer_uid_from_reminder.is_empty() {
        peer_uid_from_reminder.as_str()
    } else {
        sender_uid.as_str()
    };
    let sender_nick =
        sanitized_peer_nick_candidate(&json_string(ext.get("reminderTitle")), reminder_nick_uid);
    let session_users = if !peer_uid_from_reminder.is_empty()
        && !message_uids_match(&peer_uid_from_reminder, &sender_uid)
    {
        json!([
            {"userId": sender_uid, "fishNick": ""},
            {"userId": peer_uid_from_reminder, "fishNick": sender_nick}
        ])
    } else {
        Value::Array(Vec::new())
    };
    let mut item_id =
        extract_item_id_from_value(&json!({ "reminderUrl": reminder_url })).unwrap_or_default();
    let mut item_title = String::new();
    let mut item_pic = String::new();
    merge_ws_item_context_from_value(&ext_parsed, &mut item_id, &mut item_title, &mut item_pic);

    // `message.unreadCount` is the peer-read indicator: for outgoing
    // messages, 0 = peer 已读, >0 = peer 未读. Always pull it through so
    // the frontend can display read/unread chips on outgoing bubbles.
    let unread_count = json_i64(msg.get("unreadCount"));

    // PNM (服务端原始 messageId) — 撤回 API 必须用它，跟我们作为主键的 UUID
    // (messageUuid / extJson.messageId) 不同。两者都暴露给下游。
    let upstream_message_id = json_string(msg.get("messageId"));
    let recall_feature = json_object(model.get("recallFeature"));
    Some(serde_json::json!({
        "messageUuid": if !ext_message_id.is_empty() {
            ext_message_id
        } else {
            upstream_message_id.clone()
        },
        "upstreamMessageId": upstream_message_id,
        "version": ts,
        "timeStamp": ts,
        "content": decoded_content,
        "unreadCount": unread_count,
        "senderInfo": {
            "userId": sender_uid,
            "nick": sender_nick,
            "fishNick": sender_nick,
        },
        "sessionInfo": {
            "sessionId": session_id,
            "sessionType": json_i64(ext.get("sessionType")).max(1),
            "users": session_users,
            "attributeMap": {
                "reminderTitle": sender_nick,
                "reminderUrl": json_string(ext.get("reminderUrl")),
                "senderUserId": sender_uid,
                "itemTitle": item_title,
                "peerUserId": peer_uid_from_reminder,
            },
            "itemInfo": {
                "itemId": item_id,
                "mainPic": item_pic,
            },
        },
        "extJson": ext_json_str,
        "msgStatus": json_i64(model.get("msgStatus")),
        "recallFeature": Value::Object(recall_feature),
        // 引用回复 PNM —— normalize_message_history 会从这里取出来注入 MessageHistoryEntry。
        "replyToMessageId": reply_to_message_id,
    }))
}

fn merge_ws_item_context_from_value(
    value: &Value,
    item_id: &mut String,
    item_title: &mut String,
    item_pic: &mut String,
) {
    let mut stack = vec![value.clone()];
    while let Some(current) = stack.pop() {
        match current {
            Value::Object(map) => {
                if item_id.is_empty() {
                    if let Some(candidate) = extract_item_id_from_value(&Value::Object(map.clone()))
                    {
                        *item_id = candidate;
                    }
                }
                if item_title.is_empty() {
                    for key in ["itemTitle", "title", "semanticsDesc"] {
                        let candidate = json_string(map.get(key));
                        if !candidate.is_empty() {
                            *item_title = candidate;
                            break;
                        }
                    }
                }
                if item_pic.is_empty() {
                    for key in ["itemPic", "itemImage", "itemMainPic", "mainPic", "picUrl"] {
                        let candidate = json_string(map.get(key));
                        if !candidate.is_empty() {
                            *item_pic = candidate;
                            break;
                        }
                    }
                }
                stack.extend(map.into_values());
            }
            Value::Array(items) => stack.extend(items),
            Value::String(text) => {
                let nested = text.trim();
                if (nested.starts_with('{') && nested.ends_with('}'))
                    || (nested.starts_with('[') && nested.ends_with(']'))
                {
                    if let Ok(parsed) = serde_json::from_str::<Value>(nested) {
                        stack.push(parsed);
                    }
                }
            }
            _ => {}
        }
        if !item_id.is_empty() && !item_title.is_empty() && !item_pic.is_empty() {
            return;
        }
    }
}

/// Pull `replyMessageId` out of the stringified `extJson` blob that mtop
/// history (`messages[]`) carries directly on each entry. The WS-derived
/// `userMessageModels[]` path already pre-parses it in
/// `convert_ws_message_model`, but `messages[].extJson` is still stringified
/// JSON so we need a second decoder here.
fn ext_parsed_for_history(raw: Option<&Value>) -> Option<String> {
    let raw_str = match raw {
        Some(Value::String(s)) => s.as_str(),
        _ => return None,
    };
    serde_json::from_str::<Value>(raw_str)
        .ok()
        .and_then(|value| {
            value
                .get("replyMessageId")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn recall_feature_info(raw: &Map<String, Value>, is_self: bool) -> Option<(i64, String, String)> {
    let recall_feature = json_object(raw.get("recallFeature"));
    let code = json_string(recall_feature.get("code"));
    let recalled = code == "200" || json_i64(raw.get("msgStatus")) == 2;
    if !recalled {
        return None;
    }

    let extension = json_object(recall_feature.get("extension"));
    let original_text = json_string(extension.get("textContent"));
    let recalled_at = json_i64(recall_feature.get("recalledAt"))
        .max(json_i64(recall_feature.get("recallAt")))
        .max(json_i64(raw.get("recalledAt")))
        .max(json_i64(raw.get("timeStamp")))
        .max(json_i64(raw.get("version")));
    let recall_text = if is_self {
        "你撤回了一条消息"
    } else {
        "对方撤回了一条消息"
    }
    .to_string();

    Some((recalled_at, recall_text, original_text))
}

fn derive_chat_unread_from_messages(messages: &[MessageHistoryEntry]) -> i64 {
    let mut unread = 0_i64;
    for message in messages.iter().rev() {
        if message.is_self {
            break;
        }
        if message.unread_count > 0 {
            unread += 1;
            continue;
        }
        break;
    }
    unread
}

pub fn normalize_message_history(response: &Value, current_uid: &str) -> MessageHistoryPage {
    let data = coerce_mtop_data(response);
    let mut raw_messages = json_array(data.get("messages"));
    if raw_messages.is_empty() {
        raw_messages = json_array(data.get("userMessageModels"))
            .into_iter()
            .filter_map(|model| convert_ws_message_model(&model))
            .collect();
    }
    raw_messages.sort_by(|left, right| {
        let left_version = json_i64(json_object(Some(left)).get("version"));
        let right_version = json_i64(json_object(Some(right)).get("version"));
        let left_ts = json_i64(json_object(Some(left)).get("timeStamp"));
        let right_ts = json_i64(json_object(Some(right)).get("timeStamp"));
        (left_version, left_ts).cmp(&(right_version, right_ts))
    });

    let session_info = raw_messages
        .first()
        .map(|message| json_object(json_object(Some(message)).get("sessionInfo")))
        .unwrap_or_default();
    let mut session = normalize_message_session_identity_from_session(&session_info, current_uid);
    if session.session_id.is_empty() {
        session.session_id = json_string(data.get("sessionId"));
    }
    if session.session_type <= 0 {
        session.session_type = json_i64(data.get("type")).max(1);
    }

    let mut messages = Vec::with_capacity(raw_messages.len());
    let mut versions = Vec::with_capacity(raw_messages.len());
    for raw in &raw_messages {
        let raw_object = json_object(Some(raw));
        let version = json_i64(raw_object.get("version"));
        if version > 0 {
            versions.push(version);
        }
        let session_info = json_object(raw_object.get("sessionInfo"));
        let sender = json_object(raw_object.get("senderInfo"));

        if (session.peer_uid.trim().is_empty()
            || peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid))
            && !is_self_message(&sender, &session_info, current_uid)
        {
            let sender_uid = json_string(sender.get("userId"));
            if !sender_uid.is_empty() && session.peer_uid.trim().is_empty() {
                session.peer_uid = sender_uid;
            }
            let sender_nick = message_user_display(&sender);
            if !peer_nick_needs_fallback(&sender_nick, &json_string(sender.get("userId")))
                && peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid)
            {
                session.peer_nick = sender_nick;
            }
            let sender_logo = json_string(sender.get("logo"));
            if !sender_logo.is_empty() && session.peer_logo.trim().is_empty() {
                session.peer_logo = sender_logo;
            }
        }

        let mut content = normalize_message_content(raw_object.get("content"));
        let is_self = is_self_message(&sender, &session_info, current_uid);
        let recall_info = recall_feature_info(&raw_object, is_self);
        let mut recalled_at = 0_i64;
        let mut recalled_original_text = String::new();
        let mut recalled_original_image_url = String::new();
        if let Some((at, recall_text, original_text)) = recall_info {
            recalled_at = at.max(1);
            recalled_original_text = original_text;
            if !content.image_url.trim().is_empty() {
                recalled_original_image_url = content.image_url.clone();
            }
            content.kind = "recalled".to_string();
            content.title = String::new();
            content.text = recall_text.clone();
            content.image_url = String::new();
            content.image_urls.clear();
            content.content_type = 27;
            content.preview = recall_text;
            content.card_meta = String::new();
        }
        let message_id = {
            let primary = json_string(raw_object.get("messageUuid"));
            if !primary.is_empty() {
                primary
            } else if let Ok(ext_json) =
                serde_json::from_str::<Value>(&json_string(raw_object.get("extJson")))
            {
                json_string(ext_json.get("messageId"))
            } else {
                String::new()
            }
        };
        // PNM 来自 convert_ws_message_model 注入的 upstreamMessageId 字段。
        // 这是撤回 API 唯一认的格式，必须跟 message_id（UUID）分开存。
        let upstream_message_id = json_string(raw_object.get("upstreamMessageId"));
        // 引用回复 PNM：来源是 convert_ws_message_model 解出 extJson.replyMessageId
        // 后回填的字段。mtop history (messages[].extJson) 走的同一字段路径，所以
        // 这里两条路径都覆盖。reply_to_text 上游不下发，留给前端引用按钮在发送
        // 时填、接收侧 fallback 到客户端历史回扫。
        let reply_to_message_id = {
            let direct = json_string(raw_object.get("replyToMessageId"));
            if !direct.is_empty() {
                direct
            } else if let Some(value) = ext_parsed_for_history(raw_object.get("extJson")) {
                value
            } else {
                String::new()
            }
        };
        messages.push(MessageHistoryEntry {
            message_id,
            upstream_message_id,
            version,
            ts: json_i64(raw_object.get("timeStamp")),
            kind: content.kind,
            title: content.title,
            text: content.text,
            image_url: content.image_url,
            image_urls: content.image_urls,
            content_type: content.content_type,
            preview: content.preview,
            sender_nick: sanitized_peer_nick_candidate(
                &message_user_display(&sender),
                &json_string(sender.get("userId")),
            ),
            sender_logo: json_string(sender.get("logo")),
            is_self,
            unread_count: json_i64(raw_object.get("unreadCount")),
            recalled_at,
            recalled_original_text,
            recalled_original_image_url,
            card_meta: content.card_meta,
            reply_to_message_id,
            reply_to_text: String::new(),
        });
    }

    if let Some(last_message) = messages.last() {
        session.summary = if !last_message.preview.trim().is_empty() {
            last_message.preview.clone()
        } else if !last_message.text.trim().is_empty() {
            last_message.text.clone()
        } else {
            last_message.title.clone()
        };
        session.latest_message_id = last_message.message_id.clone();
        session.version = last_message.version;
        session.ts = last_message.ts;
        session.sort_index = last_message.ts.max(last_message.version);
    }
    if session.session_type == 1 {
        session.unread = derive_chat_unread_from_messages(&messages);
    }

    MessageHistoryPage {
        session,
        has_more: json_bool(data.get("hasMore")),
        next_cursor: versions.iter().copied().min().unwrap_or(0),
        latest_cursor: versions.iter().copied().max().unwrap_or(0),
        messages,
    }
}

// ---------------------------------------------------------------------------
// Sender / message identity helpers
// ---------------------------------------------------------------------------

/// Extract the sender's uid handling BOTH possible field names:
///   - History records → `senderInfo.userId` (already stripped of `@goofish`)
///   - userMessageModels WS wrappers → `sender.uid` (carries `"<uid>@goofish"`)
/// Without this, our own outgoing image/text echoes were detected as
/// `is_self=false`, breaking optimistic dedup and avatar placement.
pub(crate) fn sender_user_id(sender: &Map<String, Value>) -> String {
    let primary = json_string(sender.get("userId"));
    if !primary.is_empty() {
        return primary;
    }
    let raw = json_string(sender.get("uid"));
    raw.split('@').next().unwrap_or("").to_string()
}

pub fn is_self_message(
    sender: &Map<String, Value>,
    session_info: &Map<String, Value>,
    current_uid: &str,
) -> bool {
    let sender_id = sender_user_id(sender);
    let (self_user, peer_user) = resolve_session_participants(session_info, current_uid);
    let self_id = json_string(self_user.get("userId"));
    let peer_id = json_string(peer_user.get("userId"));

    if message_uids_match(&sender_id, &self_id) || message_uids_match(&sender_id, current_uid) {
        return true;
    }
    if message_uids_match(&sender_id, &peer_id) {
        return false;
    }

    let sender_name = message_user_display(sender);
    if sender_name == message_user_display(&self_user) && !sender_name.is_empty() {
        return true;
    }
    if sender_name == message_user_display(&peer_user) && !sender_name.is_empty() {
        return false;
    }

    let sender_logo = json_string(sender.get("logo"));
    if !sender_logo.is_empty() && sender_logo == json_string(self_user.get("logo")) {
        return true;
    }
    false
}

pub fn message_user_display(user: &Map<String, Value>) -> String {
    let fish_nick = json_string(user.get("fishNick"));
    if !fish_nick.is_empty() {
        return decode_message_html_entities(&fish_nick);
    }
    let nick = json_string(user.get("nick"));
    if !nick.is_empty() {
        return decode_message_html_entities(&nick);
    }
    decode_message_html_entities(&json_string(user.get("userId")))
}

fn decode_message_html_entities(raw: &str) -> String {
    let mut current = raw.trim().to_string();
    for _ in 0..3 {
        let decoded = current
            .replace("&amp;", "&")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'");
        if decoded == current {
            break;
        }
        current = decoded;
    }
    current
}

// ---------------------------------------------------------------------------
// Item ID + UID utilities
// ---------------------------------------------------------------------------

pub fn extract_item_id_from_value(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => {
            for key in ["itemId", "item_id", "id", "longItemId"] {
                let candidate = normalize_message_item_id(&json_string(object.get(key)));
                if !candidate.is_empty() {
                    return Some(candidate);
                }
            }
            for nested in object.values() {
                if let Some(candidate) = extract_item_id_from_value(nested) {
                    return Some(candidate);
                }
            }
            None
        }
        Value::Array(array) => array.iter().find_map(extract_item_id_from_value),
        Value::String(raw) => {
            let candidate = normalize_message_item_id(raw);
            if !candidate.is_empty()
                && (candidate.chars().all(|ch| ch.is_ascii_digit())
                    || candidate.starts_with("item-"))
            {
                Some(candidate)
            } else if let Some(captures) = ROOT_EMBEDDED_ITEM_ID_RE.captures(raw) {
                captures.get(1).map(|m| m.as_str().to_string())
            } else {
                None
            }
        }
        _ => None,
    }
}

static ROOT_EMBEDDED_ITEM_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:itemId=|item_id[\"':= ]+|itemId(?:%22%3A|":)|item_id(?:%22%3A|":))(?:%22|")?(\d{6,})"#)
        .expect("ROOT_EMBEDDED_ITEM_ID_RE compile")
});

static ROOT_EMBEDDED_PEER_UID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?:peerUserId=|peer_uid[\"':= ]+|peerUserId(?:%22%3A|":)|peer_uid(?:%22%3A|":))(?:%22|")?(\d{5,})"#)
        .expect("ROOT_EMBEDDED_PEER_UID_RE compile")
});

pub fn normalize_message_item_id(raw: &str) -> String {
    let value = raw.trim();
    if value.is_empty() || value == "未知" || value.starts_with("auto_") {
        return String::new();
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) && value.len() < 6 {
        return String::new();
    }
    value.to_string()
}

pub fn message_uids_match(left_uid: &str, right_uid: &str) -> bool {
    let left = left_uid.trim();
    let right = right_uid.trim();
    if left.is_empty() || right.is_empty() {
        return false;
    }
    if left == right {
        return true;
    }
    masked_uid_matches(left, right) || masked_uid_matches(right, left)
}

pub fn masked_uid_matches(masked_uid: &str, exact_uid: &str) -> bool {
    if !masked_uid.contains('*') {
        return false;
    }
    let prefix = masked_uid.split('*').next().unwrap_or_default();
    let suffix = masked_uid.rsplit('*').next().unwrap_or_default();
    (!prefix.is_empty() || !suffix.is_empty())
        && exact_uid.starts_with(prefix)
        && exact_uid.ends_with(suffix)
        && exact_uid.len() >= prefix.len() + suffix.len()
}

pub fn message_location_payload(location: &LocationContext) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert(
        "fishAdCode".to_string(),
        Value::String(location.fish_ad_code.clone()),
    );
    payload.insert("gps".to_string(), Value::String(location.gps.clone()));
    payload.insert(
        "latitude".to_string(),
        Value::String(location.latitude.clone()),
    );
    payload.insert(
        "longitude".to_string(),
        Value::String(location.longitude.clone()),
    );
    if !location.city.trim().is_empty() {
        payload.insert("city".to_string(), Value::String(location.city.clone()));
    }
    if !location.province.trim().is_empty() {
        payload.insert(
            "province".to_string(),
            Value::String(location.province.clone()),
        );
    }
    if !location.area.trim().is_empty() {
        payload.insert("area".to_string(), Value::String(location.area.clone()));
    }
    payload
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use serde_json::{json, Map, Value};

    use super::{
        extract_item_id_from_value, extract_peer_uid_from_reminder_url, message_user_display,
        normalize_message_history, BASE64_STANDARD,
    };

    fn user_with_nick(nick: &str) -> Map<String, Value> {
        let mut user = Map::new();
        user.insert("nick".to_string(), Value::String(nick.to_string()));
        user
    }

    #[test]
    fn message_user_display_decodes_double_encoded_entities() {
        assert_eq!(
            message_user_display(&user_with_nick("影子&amp;amp;i")),
            "影子&i"
        );
        assert_eq!(
            message_user_display(&user_with_nick("Tom &amp; Jerry")),
            "Tom & Jerry"
        );
    }

    #[test]
    fn extract_item_id_from_embedded_reminder_url_string() {
        let payload = json!({
            "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=65320912&sid=61826145707"
        });
        assert_eq!(
            extract_item_id_from_value(&payload),
            Some("1042662837101".to_string())
        );
    }

    #[test]
    fn extract_peer_uid_from_embedded_reminder_url_string() {
        assert_eq!(
            extract_peer_uid_from_reminder_url(
                "fleamarket://message_chat?itemId=1049278015272&peerUserId=2609769497&sid=61915880115"
            ),
            "2609769497"
        );
    }

    #[test]
    fn normalize_session_uses_mini_program_reminder_url_for_item_id() {
        let payload = json!({
            "session": {
                "sessionId": "61826145707",
                "sessionType": 1,
                "users": [
                    {"userId": "90000000001", "fishNick": "测试用户甲"},
                    {"userId": "65320912", "fishNick": "Chen"}
                ],
                "attributeMap": {
                    "reminderTitle": "Chen",
                    "reminderContent": "180",
                    "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=65320912&sid=61826145707",
                    "senderUserId": "65320912",
                    "squadName_90000000001": "露营车"
                },
                "itemInfo": {}
            },
            "message": {
                "summary": {
                    "summary": "180",
                    "ts": 1779323926590_i64,
                    "version": 1779323926590_i64,
                    "sortIndex": 1779323926590_i64
                }
            }
        });

        let sessions = super::normalize_message_sessions(
            &json!({"data": {"sessions": [payload]}}),
            "90000000001",
        );
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "61826145707");
        assert_eq!(sessions[0].peer_uid, "65320912");
        assert_eq!(sessions[0].peer_nick, "Chen");
        assert_eq!(sessions[0].item_id, "1042662837101");
        assert_eq!(sessions[0].item_title, "露营车");
        assert_eq!(sessions[0].summary, "180");
    }

    #[test]
    fn normalize_ws_user_message_model_carries_extension_item_context() {
        let ext_json = json!({
            "messageId": "uuid-1",
            "itemTitle": "折叠露营车",
            "itemPic": "https://img.example.com/camp-cart.jpg"
        })
        .to_string();
        let decoded_content = BASE64_STANDARD.encode(
            serde_json::to_vec(&json!({
                "contentType": 1,
                "text": { "text": "180" }
            }))
            .unwrap(),
        );
        let payload = json!({
            "data": {
                "userMessageModels": [
                    {
                        "message": {
                            "cid": "61826145707@goofish",
                            "messageId": "4122695106954.PNM",
                            "createAt": 1779323926590_i64,
                            "unreadCount": 1,
                            "sender": {
                                "uid": "65320912@goofish"
                            },
                            "content": {
                                "custom": {
                                    "data": decoded_content,
                                    "summary": "180"
                                }
                            },
                            "extension": {
                                "sessionType": 1,
                                "reminderTitle": "Chen",
                                "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=65320912&sid=61826145707&messageId=4122695106954.PNM",
                                "senderUserId": "65320912",
                                "extJson": ext_json
                            }
                        }
                    }
                ]
            }
        });

        let page = normalize_message_history(&payload, "90000000001");

        assert_eq!(page.session.session_id, "61826145707");
        assert_eq!(page.session.peer_uid, "65320912");
        assert_eq!(page.session.peer_nick, "Chen");
        assert_eq!(page.session.item_id, "1042662837101");
        assert_eq!(page.session.item_title, "折叠露营车");
        assert_eq!(
            page.session.item_pic,
            "https://img.example.com/camp-cart.jpg"
        );
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].message_id, "uuid-1");
        assert_eq!(page.messages[0].upstream_message_id, "4122695106954.PNM");
        assert_eq!(page.messages[0].text, "180");
    }

    #[test]
    fn normalize_ws_outgoing_inquiry_uses_reminder_peer_identity() {
        let decoded_content = BASE64_STANDARD.encode(
            serde_json::to_vec(&json!({
                "contentType": 1,
                "text": { "text": "是 350 升的吗" }
            }))
            .unwrap(),
        );
        let payload = json!({
            "data": {
                "userMessageModels": [
                    {
                        "message": {
                            "cid": "61915880115@goofish",
                            "messageId": "206edc9e184b4a248168759fdd585058.PNM",
                            "createAt": 1779286146044_i64,
                            "sender": {
                                "uid": "90000000001@goofish"
                            },
                            "content": {
                                "custom": {
                                    "data": decoded_content,
                                    "summary": "是 350 升的吗"
                                }
                            },
                            "extension": {
                                "sessionType": 1,
                                "reminderTitle": "san丹530",
                                "senderUserId": "90000000001",
                                "reminderUrl": "fleamarket://message_chat?itemId=1049278015272&peerUserId=2609769497&sid=61915880115"
                            }
                        }
                    }
                ]
            }
        });

        let page = normalize_message_history(&payload, "90000000001");

        assert_eq!(page.session.session_id, "61915880115");
        assert_eq!(page.session.peer_uid, "2609769497");
        assert_eq!(page.session.peer_nick, "san丹530");
        assert_eq!(page.session.item_id, "1049278015272");
        assert_eq!(page.messages.len(), 1);
        assert!(page.messages[0].is_self);
        assert_eq!(page.messages[0].text, "是 350 升的吗");
    }

    #[test]
    fn normalize_ws_user_message_model_does_not_use_order_copy_as_peer_nick() {
        let decoded_content = BASE64_STANDARD.encode(
            serde_json::to_vec(&json!({
                "contentType": 1,
                "text": { "text": "未付款，买家关闭了订单" }
            }))
            .unwrap(),
        );
        let payload = json!({
            "data": {
                "userMessageModels": [
                    {
                        "message": {
                            "cid": "60093789939@goofish",
                            "messageId": "4122695106955.PNM",
                            "createAt": 1779323926591_i64,
                            "sender": {
                                "uid": "2219662209062@goofish"
                            },
                            "content": {
                                "custom": {
                                    "data": decoded_content,
                                    "summary": "未付款，买家关闭了订单"
                                }
                            },
                            "extension": {
                                "sessionType": 1,
                                "reminderTitle": "买家已拍下，待付款",
                                "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=2219662209062&sid=60093789939",
                                "senderUserId": "2219662209062"
                            }
                        }
                    }
                ]
            }
        });

        let page = normalize_message_history(&payload, "90000000001");

        assert_eq!(page.session.session_id, "60093789939");
        assert_eq!(page.session.peer_uid, "2219662209062");
        assert_eq!(page.session.peer_nick, "");
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].text, "未付款，买家关闭了订单");
        assert_eq!(page.messages[0].sender_nick, "");
    }

    #[test]
    fn normalize_ws_recalled_history_model_uses_recall_feature_not_message_type_zero() {
        let payload = json!({
            "data": {
                "userMessageModels": [
                    {
                        "message": {
                            "cid": "61522179993@goofish",
                            "messageId": "4104912015963.PNM",
                            "createAt": 1778853768253_i64,
                            "sender": {
                                "uid": "2217257941119@goofish"
                            },
                            "content": {
                                "contentType": 0
                            },
                            "extension": {
                                "sessionType": 1,
                                "reminderTitle": "蓝染老师的文学社",
                                "senderUserId": "2217257941119",
                                "reminderUrl": "fleamarket://message_chat?itemId=1051897161756&peerUserId=2217257941119&sid=61522179993"
                            }
                        },
                        "msgStatus": 2,
                        "recallFeature": {
                            "code": "200",
                            "extension": {
                                "originContentType": "1",
                                "textContent": "撤回前原文"
                            },
                            "operatorType": 0,
                            "operatorUid": "2217257941119",
                            "showRecallStatusSetting": 1
                        }
                    }
                ]
            }
        });

        let page = normalize_message_history(&payload, "90000000001");

        assert_eq!(page.session.summary, "对方撤回了一条消息");
        assert_eq!(page.messages.len(), 1);
        let message = &page.messages[0];
        assert_eq!(message.kind, "recalled");
        assert_eq!(message.text, "对方撤回了一条消息");
        assert_eq!(message.preview, "对方撤回了一条消息");
        assert_eq!(message.content_type, 27);
        assert_eq!(message.recalled_at, 1778853768253_i64);
        assert_eq!(message.recalled_original_text, "撤回前原文");
    }

    #[test]
    fn normalize_message_history_derives_chat_unread_from_trailing_incoming_messages() {
        let payload = json!({
            "data": {
                "sessionId": "sess-unread",
                "type": 1,
                "hasMore": false,
                "messages": [
                    {
                        "messageUuid": "msg-1",
                        "version": 1713330001000_i64,
                        "timeStamp": 1713330001000_i64,
                        "unreadCount": 0,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "我先回你" }
                        },
                        "senderInfo": {
                            "userId": "uid-self",
                            "nick": "我",
                            "fishNick": "我"
                        },
                        "sessionInfo": {
                            "sessionId": "sess-unread",
                            "sessionType": 1,
                            "users": [
                                { "userId": "uid-self", "nick": "我", "fishNick": "我" },
                                { "userId": "buyer-1", "nick": "买家", "fishNick": "买家" }
                            ],
                            "attributeMap": {
                                "squadName_buyer-1": "键盘"
                            },
                            "itemInfo": {}
                        }
                    },
                    {
                        "messageUuid": "msg-2",
                        "version": 1713330002000_i64,
                        "timeStamp": 1713330002000_i64,
                        "unreadCount": 1,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "第一条未读" }
                        },
                        "senderInfo": {
                            "userId": "buyer-1",
                            "nick": "买家",
                            "fishNick": "买家"
                        },
                        "sessionInfo": {
                            "sessionId": "sess-unread",
                            "sessionType": 1,
                            "users": [
                                { "userId": "uid-self", "nick": "我", "fishNick": "我" },
                                { "userId": "buyer-1", "nick": "买家", "fishNick": "买家" }
                            ],
                            "attributeMap": {
                                "squadName_buyer-1": "键盘"
                            },
                            "itemInfo": {}
                        }
                    },
                    {
                        "messageUuid": "msg-3",
                        "version": 1713330003000_i64,
                        "timeStamp": 1713330003000_i64,
                        "unreadCount": 1,
                        "content": {
                            "contentType": 1,
                            "text": { "text": "第二条未读" }
                        },
                        "senderInfo": {
                            "userId": "buyer-1",
                            "nick": "买家",
                            "fishNick": "买家"
                        },
                        "sessionInfo": {
                            "sessionId": "sess-unread",
                            "sessionType": 1,
                            "users": [
                                { "userId": "uid-self", "nick": "我", "fishNick": "我" },
                                { "userId": "buyer-1", "nick": "买家", "fishNick": "买家" }
                            ],
                            "attributeMap": {
                                "squadName_buyer-1": "键盘"
                            },
                            "itemInfo": {}
                        }
                    }
                ]
            }
        });

        let page = normalize_message_history(&payload, "uid-self");

        assert_eq!(page.session.unread, 2);
    }
}
