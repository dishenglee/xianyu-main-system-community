//! Parse raw WebSocket frames and ACCS sync payloads into `LiveMessageEvent`.

use std::io::Cursor;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{Map, Value};

use crate::routes::error::ResponseError;

use serde_json::json;

use crate::routes::accounts::messages::{
    extract_peer_uid_from_reminder_url, is_self_message, json_array, json_i64, json_object,
    json_string, message_uids_match, message_user_display, normalize_message_content,
    normalize_message_session_identity_from_session, peer_nick_needs_fallback,
    sanitized_peer_nick_candidate, sender_user_id, LiveMessageEvent,
};

pub(crate) fn parse_ws_json_frame(
    frame: tokio_tungstenite::tungstenite::Message,
) -> Result<Option<Value>, ResponseError> {
    match frame {
        tokio_tungstenite::tungstenite::Message::Text(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(ResponseError::internal),
        tokio_tungstenite::tungstenite::Message::Binary(binary) => {
            let text = String::from_utf8(binary.to_vec()).map_err(ResponseError::internal)?;
            serde_json::from_str(&text)
                .map(Some)
                .map_err(ResponseError::internal)
        }
        tokio_tungstenite::tungstenite::Message::Close(_) => Ok(None),
        _ => Ok(None),
    }
}

pub(super) fn extract_sync_push_data(payload: &Value) -> Vec<(String, i64)> {
    let body = json_object(payload.get("body"));
    let sync_push = json_object(body.get("syncPushPackage"));
    json_array(sync_push.get("data"))
        .into_iter()
        .filter_map(|item| {
            let obj = json_object(Some(&item));
            let encoded = json_string(obj.get("data"));
            if encoded.is_empty() {
                None
            } else {
                let pts = json_i64(obj.get("pts"));
                Some((encoded, pts))
            }
        })
        .collect()
}

pub(super) fn decode_wx_sync_data(raw: &str) -> Result<Value, ResponseError> {
    let mut encoded = raw.trim().to_string();
    let decoded = match BASE64_STANDARD.decode(encoded.as_bytes()) {
        Ok(decoded) => decoded,
        Err(_) => {
            while encoded.len() % 4 != 0 {
                encoded.push('=');
            }
            BASE64_STANDARD
                .decode(encoded.as_bytes())
                .map_err(ResponseError::internal)?
        }
    };

    if let Ok(text) = std::str::from_utf8(&decoded) {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            return Ok(value);
        }
    }

    let mut cursor = Cursor::new(decoded.as_slice());
    let value = rmpv::decode::read_value(&mut cursor).map_err(ResponseError::internal)?;
    Ok(rmpv_to_json(&value))
}

fn rmpv_to_json(value: &rmpv::Value) -> Value {
    match value {
        rmpv::Value::Nil => Value::Null,
        rmpv::Value::Boolean(flag) => Value::Bool(*flag),
        rmpv::Value::Integer(number) => number
            .as_i64()
            .map(Value::from)
            .or_else(|| number.as_u64().map(Value::from))
            .unwrap_or(Value::Null),
        rmpv::Value::F32(number) => serde_json::Number::from_f64(*number as f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rmpv::Value::F64(number) => serde_json::Number::from_f64(*number)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        rmpv::Value::String(text) => Value::String(
            text.as_str()
                .map(str::to_string)
                .unwrap_or_else(|| text.to_string()),
        ),
        rmpv::Value::Binary(binary) => Value::String(String::from_utf8_lossy(binary).to_string()),
        rmpv::Value::Array(items) => Value::Array(items.iter().map(rmpv_to_json).collect()),
        rmpv::Value::Map(entries) => {
            let mut object = Map::new();
            for (key, value) in entries {
                object.insert(rmpv_key_to_string(key), rmpv_to_json(value));
            }
            Value::Object(object)
        }
        rmpv::Value::Ext(_, binary) => Value::String(BASE64_STANDARD.encode(binary)),
    }
}

fn rmpv_key_to_string(value: &rmpv::Value) -> String {
    match value {
        rmpv::Value::String(text) => text
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| text.to_string()),
        rmpv::Value::Integer(number) => number
            .as_i64()
            .map(|value| value.to_string())
            .or_else(|| number.as_u64().map(|value| value.to_string()))
            .unwrap_or_default(),
        rmpv::Value::Boolean(flag) => flag.to_string(),
        _ => rmpv_to_json(value).to_string(),
    }
}

pub(crate) fn parse_live_sync_messages(
    current_uid: &str,
    message: &Value,
) -> Vec<LiveMessageEvent> {
    if let Value::Array(items) = message {
        let mut events = Vec::new();
        for item in items {
            events.extend(parse_live_sync_messages(current_uid, item));
        }
        return events;
    }

    let object = json_object(Some(message));
    if object.contains_key("chatType") {
        return extract_session_metadata_from_chat_type(current_uid, &object)
            .into_iter()
            .collect();
    }

    // 撤回（recall）的「数字 protobuf」格式 — 跟普通消息的 numeric 格式不同：
    //   {"1":"<PNM>","2":"<sid>@goofish","4":"<sender>@goofish","5":<ts>,
    //    "6":"<sender>@goofish","7":<original_ts>,
    //    "8":{"3":"200","4":"<sid>","5":{"originContentType":"1","textContent":"<原文>"}}}
    // 关键标记：根 "1" 是字符串（不是 object），且 "8.3" == "200"。
    // 我们必须在 numeric / string parser 之前拦截，否则前两个 parser 会因
    // 缺 sessionInfo/content 把帧丢掉，peer 那边永远看不到撤回。
    if json_object(object.get("1")).is_empty() && json_string(object.get("1")).ends_with(".PNM") {
        if let Some(event) = parse_recall_sync_message(current_uid, &object) {
            return vec![event];
        }
    }

    if !json_object(object.get("1")).is_empty() {
        return parse_numeric_live_sync_message(current_uid, message)
            .into_iter()
            .collect();
    }

    if !json_string(object.get("sessionId")).is_empty()
        || !json_object(object.get("content")).is_empty()
    {
        return parse_string_live_sync_message(current_uid, message)
            .into_iter()
            .collect();
    }

    // `userMessageModels` is added because some push frames wrap one or more
    // chat messages under `body.userMessageModels[].message.{content,sender,...}`
    // WITHOUT a `syncPushPackage`, so the worker hits the fallback path. The
    // old recursion list missed this key, dropping every such frame silently —
    // most visibly: image messages we sent ourselves never appeared in the
    // chat panel even though the upload + send succeeded.
    let mut events = Vec::new();
    for key in ["data", "body", "message", "payload", "userMessageModels"] {
        if let Some(value) = object.get(key) {
            match value {
                Value::Object(_) => events.extend(parse_live_sync_messages(current_uid, value)),
                Value::Array(items) => {
                    for item in items {
                        events.extend(parse_live_sync_messages(current_uid, item));
                    }
                }
                _ => {}
            }
        }
    }

    events
}

pub(crate) fn parse_live_sync_message(
    current_uid: &str,
    message: &Value,
) -> Option<LiveMessageEvent> {
    parse_live_sync_messages(current_uid, message)
        .into_iter()
        .next()
}

/// 撤回的「numeric protobuf」格式解析。
/// 详见 parse_live_sync_message 入口处的格式注释。`kind="recalled"` 的事件
/// 经过 event_processor.rs 时会被识别出来并 UPDATE 原 cache 行（按 PNM 找）
/// 而不是 INSERT 新一行——撤回的语义是「改变现有消息状态」。
fn parse_recall_sync_message(
    current_uid: &str,
    object: &Map<String, Value>,
) -> Option<LiveMessageEvent> {
    let upstream_message_id = json_string(object.get("1"));
    if !upstream_message_id.ends_with(".PNM") {
        return None;
    }
    let recall_block = json_object(object.get("8"));
    let code = json_string(recall_block.get("3"));
    if code != "200" {
        // Non-success recall — server-side rejected or unrelated frame.
        return None;
    }

    let session_id = trim_goofish_suffix(&json_string(object.get("2")));
    let sender_uid = trim_goofish_suffix(&json_string(object.get("4")))
        .or_else_empty(|| trim_goofish_suffix(&json_string(object.get("6"))));
    let recall_ts = normalize_timestamp_ms(json_i64(object.get("5")));
    let original_ts = normalize_timestamp_ms(json_i64(object.get("7")));
    let ts = if recall_ts > 0 {
        recall_ts
    } else {
        original_ts
    };

    // 原文本（"8.5.textContent"）— 服务端把它附在撤回事件里，方便客户端做
    // 「重新编辑」。我们传给 event_processor，后者会把原行 text 改成
    // "对方撤回了一条消息" / "你撤回了一条消息" + 同时把原文本另存到 preview
    // 给前端做重新编辑用。
    let _original_text = json_string(json_object(recall_block.get("5")).get("textContent"));

    let is_self = !sender_uid.is_empty() && message_uids_match(&sender_uid, current_uid);

    Some(LiveMessageEvent {
        session_id,
        session_type: 1,
        // message_id 留空——event_processor 会按 upstream_message_id 找原行
        // 来更新，不需要 UUID。
        message_id: String::new(),
        upstream_message_id,
        version: ts,
        ts,
        sender_uid,
        kind: "recalled".to_string(),
        text: if is_self {
            "你撤回了一条消息".to_string()
        } else {
            "对方撤回了一条消息".to_string()
        },
        preview: if is_self {
            "你撤回了一条消息".to_string()
        } else {
            "对方撤回了一条消息".to_string()
        },
        is_self,
        ..Default::default()
    })
}

trait OrElseEmpty {
    fn or_else_empty<F: FnOnce() -> String>(self, f: F) -> String;
}

impl OrElseEmpty for String {
    fn or_else_empty<F: FnOnce() -> String>(self, f: F) -> String {
        if self.is_empty() {
            f()
        } else {
            self
        }
    }
}

/// Extract session metadata (peer info, item info) from chatType pushes
/// like `sessionArouse` (contentType=8). These aren't messages but carry rich
/// `operation.sessionInfo.extensions` with ownerUserId, extUserId, itemId, etc.
fn extract_session_metadata_from_chat_type(
    current_uid: &str,
    object: &Map<String, Value>,
) -> Option<LiveMessageEvent> {
    let operation = json_object(object.get("operation"));
    let session_info = json_object(operation.get("sessionInfo"));
    let extensions = json_object(session_info.get("extensions"));
    let content = json_object(operation.get("content"));
    let session_arouse = json_object(content.get("sessionArouse"));
    let arouse_info = json_object(session_arouse.get("sessionArouseInfo"));
    let session_id = json_string(session_info.get("sessionId"));
    if session_id.is_empty() {
        let outer_sid = json_string(object.get("sessionId"));
        if outer_sid.is_empty() {
            return None;
        }
    }
    let sid = if !json_string(session_info.get("sessionId")).is_empty() {
        json_string(session_info.get("sessionId"))
    } else {
        json_string(object.get("sessionId"))
    };

    let session_type = json_i64(session_info.get("sessionType"))
        .max(json_i64(session_info.get("type")))
        .max(1);

    let owner_uid = json_string(extensions.get("ownerUserId"));
    let ext_uid = json_string(extensions.get("extUserId"));
    let item_seller_id = json_string(extensions.get("itemSellerId"));
    let item_id = json_string(extensions.get("itemId"));
    let item_title = json_string(extensions.get("itemTitle"));
    let item_pic = json_string(extensions.get("itemMainPic"));
    let activity_ts = normalize_timestamp_ms(
        json_i64(arouse_info.get("arouseTimeStamp")).max(json_i64(session_info.get("createTime"))),
    );

    let (peer_uid, peer_role, self_role) = if !owner_uid.is_empty() && !ext_uid.is_empty() {
        if message_uids_match(&owner_uid, current_uid) {
            (ext_uid, "seller".to_string(), "buyer".to_string())
        } else if message_uids_match(&ext_uid, current_uid) {
            (owner_uid.clone(), "buyer".to_string(), "seller".to_string())
        } else {
            (String::new(), String::new(), String::new())
        }
    } else {
        (String::new(), String::new(), String::new())
    };

    if peer_uid.is_empty() && item_id.is_empty() && item_title.is_empty() {
        return None;
    }

    let squad_key_seller = if !item_seller_id.is_empty() {
        format!("squadName_{item_seller_id}")
    } else if !owner_uid.is_empty() {
        format!("squadName_{owner_uid}")
    } else {
        String::new()
    };
    let squad_title = if !squad_key_seller.is_empty() {
        json_string(extensions.get(&squad_key_seller))
    } else {
        String::new()
    };
    let final_item_title = if !item_title.is_empty() {
        item_title
    } else {
        squad_title
    };

    Some(LiveMessageEvent {
        session_id: sid,
        session_type,
        message_id: String::new(),
        upstream_message_id: String::new(),
        version: activity_ts,
        ts: activity_ts,
        peer_uid,
        peer_nick: String::new(),
        peer_logo: String::new(),
        peer_role,
        self_role,
        item_id,
        item_title: final_item_title,
        item_pic,
        kind: "session_metadata".to_string(),
        title: String::new(),
        text: String::new(),
        image_url: String::new(),
        preview: String::new(),
        sender_uid: String::new(),
        sender_nick: String::new(),
        sender_logo: String::new(),
        is_self: false,
        unread_count: 0,
        accs_pts: 0,
        card_meta: String::new(),
        content_type: 0,
        reply_to_message_id: String::new(),
    })
}

fn extract_item_object(
    object: &Map<String, Value>,
    session_info: &Map<String, Value>,
) -> Map<String, Value> {
    for source in [
        object.get("item"),
        object.get("itemInfo"),
        session_info.get("itemInfo"),
    ] {
        let candidate = json_object(source);
        if !candidate.is_empty() {
            return candidate;
        }
    }
    Map::new()
}

fn extract_item_id_from_reminder_url(extension: &Map<String, Value>) -> String {
    let reminder_url = json_string(extension.get("reminderUrl"));
    if reminder_url.is_empty() {
        return String::new();
    }
    if let Some(captures) = ROOT_ITEM_ID_RE.captures(&reminder_url) {
        if let Some(matched) = captures.get(1) {
            return matched.as_str().to_string();
        }
    }
    if reminder_url.contains('%') {
        if let Ok(decoded) = urlencoding::decode(&reminder_url) {
            if let Some(captures) = ROOT_ITEM_ID_RE.captures(decoded.as_ref()) {
                if let Some(matched) = captures.get(1) {
                    return matched.as_str().to_string();
                }
            }
        }
    }
    String::new()
}

fn extract_peer_uid_from_extension_reminder_url(extension: &Map<String, Value>) -> String {
    extract_peer_uid_from_reminder_url(&json_string(extension.get("reminderUrl")))
}

fn merge_item_context_from_json_blob(
    raw_blob: &str,
    item_id: &mut String,
    item_title: &mut String,
    item_pic: &mut String,
) {
    let trimmed = raw_blob.trim();
    if trimmed.is_empty() {
        return;
    }
    let Ok(blob) = serde_json::from_str::<Value>(trimmed) else {
        return;
    };
    let mut stack = vec![blob];
    while let Some(current) = stack.pop() {
        match current {
            Value::Object(map) => {
                if item_id.is_empty() {
                    for key in ["itemId", "item_id", "id", "longItemId"] {
                        let candidate = json_string(map.get(key));
                        if !candidate.is_empty() {
                            *item_id = candidate;
                            break;
                        }
                    }
                }
                if item_title.is_empty() {
                    for key in ["itemTitle", "title"] {
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
                for (_k, value) in map {
                    stack.push(value);
                }
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

fn parse_string_live_sync_message(current_uid: &str, message: &Value) -> Option<LiveMessageEvent> {
    let object = json_object(Some(message));
    let session_info = json_object(object.get("sessionInfo"));
    let sender_info = json_object(object.get("senderInfo"));
    let sender = if sender_info.is_empty() {
        json_object(object.get("sender"))
    } else {
        sender_info
    };
    // `userMessageModels` items put metadata under `extension` (server-side
    // wrapper). Pull it out so sessionType / extJson lookups below have a
    // place to fall back to.
    let extension = json_object(object.get("extension"));
    let mut session = normalize_message_session_identity_from_session(&session_info, current_uid);
    if session.session_id.is_empty() {
        let cid = trim_goofish_suffix(&json_string(object.get("cid")));
        session.session_id = if cid.is_empty() {
            json_string(object.get("sessionId"))
        } else {
            cid
        };
    }
    if session.session_type <= 0 {
        session.session_type = json_i64(object.get("sessionType"))
            .max(json_i64(extension.get("sessionType")))
            .max(1);
    }

    let item = extract_item_object(&object, &session_info);
    if session.item_title.is_empty() {
        let title = json_string(item.get("title"));
        if !title.is_empty() {
            session.item_title = title;
        } else {
            let item_title = json_string(item.get("itemTitle"));
            if !item_title.is_empty() {
                session.item_title = item_title;
            } else {
                let from_session = json_string(session_info.get("itemTitle"));
                if !from_session.is_empty() {
                    session.item_title = from_session;
                }
            }
        }
    }
    if session.item_pic.is_empty() {
        for key in ["mainPic", "picUrl", "image", "coverUrl"] {
            let pic = json_string(item.get(key));
            if !pic.is_empty() {
                session.item_pic = pic;
                break;
            }
        }
    }
    if session.item_id.is_empty() {
        let from_reminder = extract_item_id_from_reminder_url(&extension);
        if !from_reminder.is_empty() {
            session.item_id = from_reminder;
        }
    }
    let peer_uid_from_reminder = extract_peer_uid_from_extension_reminder_url(&extension);
    if session.peer_uid.trim().is_empty() {
        if !peer_uid_from_reminder.is_empty()
            && !message_uids_match(&peer_uid_from_reminder, current_uid)
        {
            session.peer_uid = peer_uid_from_reminder.clone();
            if peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid) {
                let reminder_title = sanitized_peer_nick_candidate(
                    &json_string(extension.get("reminderTitle")),
                    &session.peer_uid,
                );
                if !reminder_title.is_empty() {
                    session.peer_nick = reminder_title;
                }
            }
        }
    }

    let content = normalize_message_content(object.get("content"));
    // Same field-name fallback as `is_self_message`: history records put the
    // sender's stripped uid in `senderInfo.userId`; userMessageModels wrappers
    // put `"<uid>@goofish"` under `sender.uid`. Without this fallback our own
    // outgoing echo would have empty sender_uid → optimistic dedup misses it
    // and the message appears twice in the chat panel.
    let sender_uid = sender_user_id(&sender);
    let sender_uid = if sender_uid.is_empty() {
        json_string(extension.get("senderUserId"))
    } else {
        sender_uid
    };
    let sender_nick = {
        let from_sender = message_user_display(&sender);
        if peer_nick_needs_fallback(&from_sender, &sender_uid) {
            let reminder_peer_uid = if !peer_uid_from_reminder.is_empty() {
                peer_uid_from_reminder.as_str()
            } else if !session.peer_uid.is_empty() {
                session.peer_uid.as_str()
            } else {
                sender_uid.as_str()
            };
            sanitized_peer_nick_candidate(
                &json_string(extension.get("reminderTitle")),
                reminder_peer_uid,
            )
        } else {
            from_sender
        }
    };
    let sender_logo = json_string(sender.get("logo"));
    let is_self = is_self_message(&sender, &session_info, current_uid);

    let mut message_id = json_string(object.get("messageUuid"));
    let mut reply_to_message_id = String::new();
    // extJson 一定要解一遍（不论 message_id 是否已经有了），因为 replyMessageId
    // 跟 messageId 在同一个 blob 里。两个位置（顶层 / extension 下）都尝试。
    {
        let raw_ext = json_string(object.get("extJson"));
        let raw_ext = if raw_ext.is_empty() {
            json_string(extension.get("extJson"))
        } else {
            raw_ext
        };
        if let Ok(ext_json) = serde_json::from_str::<Value>(&raw_ext) {
            if message_id.is_empty() {
                message_id = json_string(ext_json.get("messageId"));
            }
            reply_to_message_id = json_string(ext_json.get("replyMessageId"));
        }
        merge_item_context_from_json_blob(
            &raw_ext,
            &mut session.item_id,
            &mut session.item_title,
            &mut session.item_pic,
        );
    }
    if message_id.is_empty() {
        message_id = json_string(object.get("messageId"));
    }
    if session.item_id.is_empty() || session.item_pic.is_empty() {
        let (root_id, root_pic) = extract_item_context_from_root(message, &session.session_id);
        if session.item_id.is_empty() {
            session.item_id = root_id;
        }
        if session.item_pic.is_empty() {
            session.item_pic = root_pic;
        }
    }
    // PNM 格式的原始 messageId — 来源 object.messageId（不论 message_id 用了 UUID
    // 还是 PNM，PNM 一定在 object.messageId 里）。撤回 API 必须用 PNM。
    let upstream_message_id = json_string(object.get("messageId"));

    // Timestamp: history records carry `timeStamp` / `ts`; userMessageModels
    // wrappers use `createAt` (ms since epoch). Fall back through all three.
    let ts = normalize_timestamp_ms(
        json_i64(object.get("timeStamp"))
            .max(json_i64(object.get("ts")))
            .max(json_i64(object.get("createAt"))),
    );
    let version = json_i64(object.get("version")).max(ts);
    let preview = live_message_preview(
        &content.kind,
        &content.preview,
        &content.text,
        &content.title,
    );
    let sender_uid_out = sender_uid.clone();
    let peer_uid = if !is_self && !sender_uid.is_empty() {
        sender_uid
    } else {
        session.peer_uid.clone()
    };
    let peer_nick = if !is_self && !peer_nick_needs_fallback(&sender_nick, &peer_uid) {
        sender_nick.clone()
    } else {
        session.peer_nick.clone()
    };
    let peer_logo = if !is_self && !sender_logo.is_empty() {
        sender_logo.clone()
    } else {
        session.peer_logo.clone()
    };

    if session.session_id.is_empty() && message_id.is_empty() && preview.is_empty() {
        return None;
    }

    Some(LiveMessageEvent {
        session_id: session.session_id,
        session_type: session.session_type.max(1),
        message_id,
        upstream_message_id,
        version: version.max(ts).max(1),
        ts: ts.max(version).max(1),
        peer_uid,
        peer_nick,
        peer_logo,
        peer_role: session.peer_role,
        self_role: session.self_role,
        item_id: session.item_id,
        item_title: session.item_title,
        item_pic: session.item_pic,
        kind: if content.kind.is_empty() {
            "text".to_string()
        } else {
            content.kind
        },
        title: content.title,
        text: content.text,
        image_url: content.image_url,
        preview,
        sender_uid: sender_uid_out,
        sender_nick,
        sender_logo,
        is_self,
        unread_count: json_i64(object.get("unreadCount")),
        accs_pts: 0,
        card_meta: content.card_meta,
        content_type: content.content_type,
        reply_to_message_id,
    })
}

/// Python parity (`xianyu_live_client.py:30-31`):
/// `_ROOT_ITEM_ID_RE` matches `itemId=12345` / `itemId":"12345` / urlencoded
/// `itemId%22%3A%2212345`. The trailing digits (≥6) are the item id.
static ROOT_ITEM_ID_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(
        r#"(?:itemId=|item_id=|itemId(?:%22%3A|":)|item_id(?:%22%3A|":))(?:%22|")?(\d{6,})"#,
    )
    .expect("ROOT_ITEM_ID_RE compile")
});
/// Python parity: `_ROOT_ITEM_PIC_RE` — match `mainPic=`/`itemPic":"`/etc.
/// followed by an http(s):// URL.
static ROOT_ITEM_PIC_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
    regex::Regex::new(r#"(?:mainPic|itemPic|picUrl)(?:=|%22%3A|":)(?:%22|")?(https?://[^"'&\\]+)"#)
        .expect("ROOT_ITEM_PIC_RE compile")
});

/// Recursively walk a Value and yield every string fragment (and the
/// fragments inside any embedded JSON-blob string). Mirrors Python's
/// `_iter_root_text_fragments` (`xianyu_live_client.py:1030-1063`).
fn iter_root_text_fragments(value: &Value, fragments: &mut Vec<String>) {
    let mut stack: Vec<Value> = vec![value.clone()];
    while let Some(current) = stack.pop() {
        match current {
            Value::Object(map) => {
                for (_k, v) in map {
                    stack.push(v);
                }
            }
            Value::Array(items) => {
                stack.extend(items);
            }
            Value::String(text) => {
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() {
                    continue;
                }
                fragments.push(trimmed.clone());
                // If the string itself looks like a JSON blob, parse it and
                // recurse into the parsed structure too — many ACCS payloads
                // wrap inner items as JSON-encoded strings.
                let is_json_obj = trimmed.starts_with('{') && trimmed.ends_with('}');
                let is_json_arr = trimmed.starts_with('[') && trimmed.ends_with(']');
                if is_json_obj || is_json_arr {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&trimmed) {
                        stack.push(parsed);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Python parity (`xianyu_live_client.py:1001-1028`): when neither `bizTag`
/// nor any structured field carries `itemId`/`itemPic`, scan the entire
/// raw root payload for them. Some ACCS messages stash the item context
/// inside a urlencoded `extJson` blob or a deeply-nested string — the
/// regex sweeps those out as a last resort. Restricts matches to fragments
/// that mention `target_session` so we don't accidentally pull in the
/// itemId of a different conversation appearing in the same root.
fn extract_item_context_from_root(root: &Value, target_session: &str) -> (String, String) {
    let mut fragments = Vec::new();
    iter_root_text_fragments(root, &mut fragments);
    let mut item_id = String::new();
    let mut item_pic = String::new();
    for text in fragments.iter() {
        let mut haystacks: Vec<String> = vec![text.clone()];
        if text.contains('%') {
            // Try urldecoding once — Python uses `urllib.parse.unquote`.
            if let Ok(decoded) = urlencoding::decode(text) {
                let decoded = decoded.into_owned();
                if decoded != *text {
                    haystacks.push(decoded);
                }
            }
        }
        for haystack in haystacks {
            if !target_session.is_empty() && !haystack.contains(target_session) {
                continue;
            }
            if item_id.is_empty() {
                if let Some(cap) = ROOT_ITEM_ID_RE.captures(&haystack) {
                    if let Some(m) = cap.get(1) {
                        item_id = m.as_str().to_string();
                    }
                }
            }
            if item_pic.is_empty() {
                if let Some(cap) = ROOT_ITEM_PIC_RE.captures(&haystack) {
                    if let Some(m) = cap.get(1) {
                        item_pic = m.as_str().to_string();
                    }
                }
            }
            if !item_id.is_empty() && !item_pic.is_empty() {
                return (item_id, item_pic);
            }
        }
    }
    (item_id, item_pic)
}

fn parse_numeric_live_sync_message(current_uid: &str, message: &Value) -> Option<LiveMessageEvent> {
    let root = json_object(Some(message));
    let inner = json_object(root.get("1"));
    let session_id = trim_goofish_suffix(&json_string(inner.get("2")));
    let ts = normalize_timestamp_ms(json_i64(inner.get("5")));

    let sender_wrap = json_object(inner.get("1"));
    let mut sender_uid = trim_goofish_suffix(&json_string(sender_wrap.get("1")));
    let details = json_object(inner.get("10"));
    let reminder_content = json_string(details.get("reminderContent"));
    let detail_notice = json_string(details.get("detailNotice"));
    let receiver = trim_goofish_suffix(&json_string(details.get("receiver")));
    let peer_uid_from_reminder = extract_peer_uid_from_extension_reminder_url(&details);
    if sender_uid.is_empty() {
        sender_uid = trim_goofish_suffix(&json_string(details.get("senderUserId")));
    }
    let reminder_peer_uid = if !peer_uid_from_reminder.is_empty() {
        peer_uid_from_reminder.as_str()
    } else {
        sender_uid.as_str()
    };
    let sender_nick = first_non_empty(&[
        sanitized_peer_nick_candidate(&json_string(details.get("senderNick")), &sender_uid),
        sanitized_peer_nick_candidate(
            &json_string(details.get("reminderTitle")),
            reminder_peer_uid,
        ),
    ]);

    let mut text = if !reminder_content.is_empty() {
        reminder_content
    } else {
        detail_notice
    };
    let mut title = String::new();
    let mut image_url = String::new();
    let mut content_type = 0_i64;
    let mut card_meta = String::new();
    let content_data = json_object(inner.get("6"));
    if !content_data.is_empty() {
        let content_inner = json_object(content_data.get("3"));
        // Layout in numeric live sync frames (e.g. image push from another
        // tab/device, group-chat fan-out):
        //   "6": { "1": <outer contentType=101>, "3": { ← content_inner
        //     "2": <summary, e.g. "[图片]">,
        //     "4": <INNER contentType, e.g. 2 for image>,
        //     "5": <stringified JSON of inner content {contentType, image:...}>
        //   } }
        // Bug fix: the inner content JSON is at `content_inner.5`, NOT
        // `content_data.5` (that key doesn't exist at the outer level, so the
        // old code always read an empty string → contentType fell back to 0,
        // image_url stayed empty, and the message was persisted as
        // kind="text" text="[图片]" — exactly what the user reported.
        let text_from_content = json_string(content_inner.get("2"));
        if !text_from_content.is_empty() {
            text = text_from_content;
        }
        let content_json = json_string(content_inner.get("5"));
        if let Ok(content) = serde_json::from_str::<Value>(&content_json) {
            content_type = json_i64(content.get("contentType"));
            if content_type == 2 {
                let pics = json_array(json_object(content.get("image")).get("pics"));
                if let Some(first_pic) = pics.first() {
                    image_url = json_string(json_object(Some(first_pic)).get("url"));
                }
            }
            // Item card (7) / image card (8) / location card (30) — pull rich
            // metadata so the frontend can render the proper card layout
            // instead of falling back to a system pill. The shape mirrors
            // what `normalize_message_content` exposes for string parsers.
            if content_type == 7 || content_type == 8 {
                let item_card = json_object(content.get("itemCard"));
                let item = json_object(item_card.get("item"));
                let item_tip = json_string(item_card.get("itemTip"));
                let item_title = json_string(item.get("title"));
                let main_pic = json_string(item.get("mainPic"));
                let price = json_string(item.get("price"));
                card_meta = serde_json::to_string(&json!({
                    "kind": "item",
                    "itemId": json_string(item.get("itemId")),
                    "title": item_title,
                    "mainPic": main_pic,
                    "price": price,
                    "itemTip": item_tip,
                }))
                .unwrap_or_default();
                // Backfill summary fields so chat-list previews + image branch
                // of the bubble renderer get something useful.
                if !item_title.is_empty() {
                    text = item_title.clone();
                }
                if image_url.is_empty() && !main_pic.is_empty() {
                    image_url = main_pic;
                }
            } else if content_type == 30 {
                let location = json_object(content.get("locationCard"));
                let title = json_string(location.get("title"));
                let preview_url = json_string(location.get("url"));
                let action_url = json_string(
                    json_object(json_object(location.get("action")).get("page")).get("url"),
                );
                card_meta = serde_json::to_string(&json!({
                    "kind": "location",
                    "title": title,
                    "content": json_string(location.get("content")),
                    "latitude": json_string(location.get("latitude")),
                    "longitude": json_string(location.get("longitude")),
                    "previewUrl": preview_url,
                    "actionUrl": action_url,
                }))
                .unwrap_or_default();
                if !title.is_empty() {
                    text = title.clone();
                }
                if image_url.is_empty() && !preview_url.is_empty() {
                    image_url = preview_url;
                }
            } else if matches!(content_type, 25 | 26 | 28) {
                let dx_card = json_object(content.get("dxCard"));
                let item = json_object(dx_card.get("item"));
                let main = json_object(item.get("main"));
                let ex = json_object(main.get("exContent"));
                let head = json_object(ex.get("head"));
                title = first_non_empty(&[
                    json_string(head.get("title")),
                    json_string(ex.get("title")),
                ]);
                text = first_non_empty(&[
                    json_string(head.get("subtitle")),
                    json_string(ex.get("desc")),
                    json_string(ex.get("content")),
                    text.clone(),
                ]);
                let seller_avatar = json_string(head.get("avatarUrl"));
                let seller_nick = first_non_empty(&[
                    json_string(head.get("sellerName")),
                    json_string(head.get("name")),
                    json_string(head.get("title")),
                ]);
                let item1 = json_object(ex.get("item1"));
                let item_image = json_string(item1.get("image"));
                let item_title = json_string(item1.get("title"));
                let item_subtitle = first_non_empty(&[
                    json_string(item1.get("subTitle")),
                    json_string(item1.get("subtitle")),
                    json_string(item1.get("desc")),
                ]);
                let link_url = first_non_empty(&[
                    json_string(main.get("targetUrl")),
                    json_string(item.get("targetUrl")),
                    json_string(ex.get("targetUrl")),
                ]);
                card_meta = serde_json::to_string(&json!({
                    "sellerNick": seller_nick,
                    "sellerAvatar": seller_avatar,
                    "itemTitle": item_title,
                    "itemSubtitle": item_subtitle,
                    "itemPic": item_image,
                    "linkUrl": link_url,
                }))
                .unwrap_or_default();
            }
        }
        // Belt-and-suspenders: if content_inner.5 wasn't parseable, fall back
        // to the inner type tag at content_inner.4 so kind classification
        // (image/text/card) still picks the right branch.
        if content_type == 0 {
            content_type = json_i64(content_inner.get("4"));
        }
    }

    let mut message_id = String::new();
    if let Ok(ext_json) = serde_json::from_str::<Value>(&json_string(details.get("extJson"))) {
        message_id = json_string(ext_json.get("messageId"));
    }
    // PNM 来自 numeric frame inner[3]（消息体根字段，跟我们的 message_id UUID 区分）
    let upstream_message_id = json_string(inner.get("3"));
    let sender_logo = first_non_empty(&[
        json_string(details.get("senderLogo")),
        json_string(details.get("senderAvatar")),
    ]);
    let session_type = json_i64(details.get("sessionType")).max(1);
    let is_self = !sender_uid.is_empty() && message_uids_match(&sender_uid, current_uid);
    let kind = match content_type {
        2 => "image",
        7 | 8 | 30 => "card",
        6 | 11 | 25 | 26 | 28 => "card",
        14 => "tip",
        _ => "text",
    }
    .to_string();
    if kind == "image" && image_url.is_empty() && looks_like_url(&text) {
        image_url = text.clone();
        text.clear();
    }
    // For image messages, the "[图片]" placeholder text that lived in
    // content_inner.2 (or in reminderContent / detailNotice) is just the
    // server-side summary. Clear it now that we have the real image_url so
    // the renderer doesn't fall back to printing "[图片]" instead of the
    // image when text is non-empty.
    if kind == "image" && !image_url.is_empty() {
        text.clear();
    }

    // Python parity (`xianyu_live_client.py:919-938`): try `bizTag` JSON
    // (a string-encoded JSON blob inside details) for itemId/itemPic, then
    // fall back to a regex sweep across the entire root payload when bizTag
    // is empty or partial. Without this Rust returned `item_id="" item_pic=""`
    // for every numeric-key event, breaking UI that needs the商品 context.
    let mut item_id = String::new();
    let mut item_pic = String::new();
    let mut item_title = String::new();
    let biz_tag_str = json_string(details.get("bizTag"));
    if !biz_tag_str.is_empty() {
        if let Ok(biz) = serde_json::from_str::<Value>(&biz_tag_str) {
            if let Some(map) = biz.as_object() {
                let from_biz_id = json_string(map.get("itemId"));
                if !from_biz_id.is_empty() {
                    item_id = from_biz_id;
                }
                for key in ["itemTitle", "title"] {
                    if item_title.is_empty() {
                        let candidate = json_string(map.get(key));
                        if !candidate.is_empty() {
                            item_title = candidate;
                            break;
                        }
                    }
                }
                for key in ["itemPic", "itemImage", "itemMainPic"] {
                    if item_pic.is_empty() {
                        let candidate = json_string(map.get(key));
                        if !candidate.is_empty() {
                            item_pic = candidate;
                            break;
                        }
                    }
                }
            }
        }
    }
    if item_id.is_empty() || item_pic.is_empty() {
        let (root_id, root_pic) = extract_item_context_from_root(message, &session_id);
        if item_id.is_empty() {
            item_id = root_id;
        }
        if item_pic.is_empty() {
            item_pic = root_pic;
        }
    }

    let preview = live_message_preview(&kind, "", &text, &title);
    if session_id.is_empty() && message_id.is_empty() && preview.is_empty() {
        return None;
    }

    let sender_uid_out = sender_uid.clone();
    let self_role = if is_self && !peer_uid_from_reminder.is_empty() {
        "buyer".to_string()
    } else {
        String::new()
    };
    let peer_role = if is_self && !peer_uid_from_reminder.is_empty() {
        "seller".to_string()
    } else {
        String::new()
    };
    Some(LiveMessageEvent {
        session_id,
        session_type,
        message_id,
        upstream_message_id,
        version: ts.max(1),
        ts: ts.max(1),
        peer_uid: if is_self {
            if !peer_uid_from_reminder.is_empty() {
                peer_uid_from_reminder.clone()
            } else {
                receiver
            }
        } else {
            sender_uid
        },
        peer_nick: if is_self {
            if !peer_uid_from_reminder.is_empty() {
                sender_nick.clone()
            } else {
                String::new()
            }
        } else {
            sender_nick.clone()
        },
        peer_logo: if is_self {
            String::new()
        } else {
            sender_logo.clone()
        },
        peer_role,
        self_role,
        item_id,
        item_title,
        item_pic,
        kind,
        title,
        text,
        image_url,
        preview,
        sender_uid: sender_uid_out,
        sender_nick,
        sender_logo,
        is_self,
        // Numeric sync payloads use positional fields and we haven't
        // identified which slot holds `unreadCount` — default to 0 (peer 已读)
        // until we can confirm the mapping.
        unread_count: 0,
        accs_pts: 0,
        card_meta,
        content_type,
        // 数字 sync payload 是位置字段，没法稳定定位 extJson 槽位；先留空。
        // 接收侧后续走 mtop history 同步时会回填这个字段。
        reply_to_message_id: String::new(),
    })
}

pub(super) fn normalize_timestamp_ms(value: i64) -> i64 {
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

/// Detect YYYYMMDDHHMMSS date-format numbers (14 digits, year 2020-2099) and
/// convert them to epoch-ms. The upstream mtop session.sync API sometimes
/// returns `sortIndex` / `version` in this format instead of epoch-ms.
pub(crate) fn try_parse_date_format_timestamp(value: i64) -> Option<i64> {
    if value < 20200101000000 || value > 20991231235959 {
        return None;
    }
    let sec = (value % 100) as u32;
    let min = ((value / 100) % 100) as u32;
    let hour = ((value / 10000) % 100) as u32;
    let day = ((value / 1000000) % 100) as u32;
    let month = ((value / 100000000) % 100) as u32;
    let year = (value / 10000000000) as i32;
    if month < 1 || month > 12 || day < 1 || day > 31 || hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    // civil_from_days algorithm (Howard Hinnant)
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days_since_epoch = era as i64 * 146097 + doe as i64 - 719468;
    // Assume CST (UTC+8)
    let epoch_secs =
        days_since_epoch * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64 - 8 * 3600;
    Some(epoch_secs * 1000)
}

pub(super) fn trim_goofish_suffix(value: &str) -> String {
    value.trim().trim_end_matches("@goofish").to_string()
}

fn looks_like_url(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("http://") || trimmed.starts_with("https://")
}

fn first_non_empty(values: &[String]) -> String {
    values
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_default()
}

pub(super) fn live_message_preview(kind: &str, preview: &str, text: &str, title: &str) -> String {
    if !preview.trim().is_empty() {
        return preview.trim().to_string();
    }
    if !text.trim().is_empty() {
        return text.trim().to_string();
    }
    if !title.trim().is_empty() {
        return title.trim().to_string();
    }
    if kind == "image" {
        return "[图片]".to_string();
    }
    "[消息]".to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use serde_json::Value;

    use super::{parse_live_sync_message, parse_live_sync_messages};

    #[test]
    fn parse_live_sync_messages_collects_all_user_message_models() {
        let payload = json!({
            "body": {
                "userMessageModels": [
                    {
                        "message": {
                            "cid": "61826145707@goofish",
                            "sessionType": 1,
                            "messageId": "msg-1.PNM",
                            "messageUuid": "msg-1",
                            "createAt": 1779511307168_i64,
                            "content": {
                                "contentType": 1,
                                "text": { "text": "first" }
                            },
                            "sender": {
                                "uid": "1750000000001@goofish",
                                "nick": "Chen"
                            },
                            "extension": {
                                "sessionType": 1
                            }
                        }
                    },
                    {
                        "message": {
                            "cid": "61696947021@goofish",
                            "sessionType": 1,
                            "messageId": "msg-2.PNM",
                            "messageUuid": "msg-2",
                            "createAt": 1779511307169_i64,
                            "content": {
                                "contentType": 1,
                                "text": { "text": "second" }
                            },
                            "sender": {
                                "uid": "1750000000002@goofish",
                                "nick": "南唐安静的海参花"
                            },
                            "extension": {
                                "sessionType": 1
                            }
                        }
                    }
                ]
            }
        });

        let events = parse_live_sync_messages("90000000001", &payload);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].session_id, "61826145707");
        assert_eq!(events[0].text, "first");
        assert_eq!(events[1].session_id, "61696947021");
        assert_eq!(events[1].text, "second");

        let first = parse_live_sync_message("90000000001", &payload).expect("first event");
        assert_eq!(first.session_id, "61826145707");
        assert_eq!(first.text, "first");
    }

    #[test]
    fn parse_live_sync_message_extracts_item_id_from_reminder_url() {
        let payload = json!({
            "cid": "61826145707@goofish",
            "sessionType": 1,
            "messageId": "msg-reminder.PNM",
            "messageUuid": "msg-reminder",
            "createAt": 1779511307168_i64,
            "content": {
                "contentType": 1,
                "text": { "text": "露营车还在吗" }
            },
            "sender": {
                "uid": "65320912@goofish",
                "nick": "Chen"
            },
            "extension": {
                "sessionType": 1,
                "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=65320912&sid=61826145707"
            }
        });

        let event = parse_live_sync_message("90000000001", &payload).expect("parsed event");
        assert_eq!(event.session_id, "61826145707");
        assert_eq!(event.item_id, "1042662837101");
        assert_eq!(event.text, "露营车还在吗");
    }

    #[test]
    fn parse_string_live_sync_message_uses_extension_reminder_identity() {
        let payload = json!({
            "cid": "61826145707@goofish",
            "sessionType": 1,
            "messageId": "4122695106954.PNM",
            "messageUuid": "uuid-1",
            "createAt": 1779323926590_i64,
            "content": {
                "contentType": 1,
                "text": { "text": "180" }
            },
            "sender": {
                "uid": "65320912@goofish"
            },
            "extension": {
                "sessionType": 1,
                "reminderTitle": "Chen",
                "senderUserId": "65320912",
                "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=65320912&sid=61826145707"
            }
        });

        let event = parse_live_sync_message("90000000001", &payload).expect("parsed event");
        assert_eq!(event.session_id, "61826145707");
        assert_eq!(event.peer_uid, "65320912");
        assert_eq!(event.peer_nick, "Chen");
        assert_eq!(event.sender_uid, "65320912");
        assert_eq!(event.sender_nick, "Chen");
        assert_eq!(event.item_id, "1042662837101");
        assert_eq!(event.text, "180");
        assert!(!event.is_self);
    }

    #[test]
    fn parse_numeric_live_sync_message_extracts_reminder_identity() {
        let payload = json!({
            "1": {
                "1": {"1": "65320912@goofish"},
                "2": "61826145707@goofish",
                "3": "4122695106954.PNM",
                "5": 1779323926590_i64,
                "10": {
                    "detailNotice": "180",
                    "reminderTitle": "Chen",
                    "reminderContent": "180",
                    "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=65320912&sid=61826145707&messageId=c75f9532eba449ababc5d4798ab165c9&adv=no",
                    "senderUserId": "65320912"
                }
            }
        });

        let event = parse_live_sync_message("90000000001", &payload).expect("parsed event");
        assert_eq!(event.session_id, "61826145707");
        assert_eq!(event.upstream_message_id, "4122695106954.PNM");
        assert_eq!(event.peer_uid, "65320912");
        assert_eq!(event.peer_nick, "Chen");
        assert_eq!(event.item_id, "1042662837101");
        assert_eq!(event.text, "180");
        assert_eq!(event.preview, "180");
    }

    #[test]
    fn parse_numeric_live_sync_message_does_not_use_order_copy_as_peer_nick() {
        let payload = json!({
            "1": {
                "1": {"1": "2219662209062@goofish"},
                "2": "60093789939@goofish",
                "3": "4122695106955.PNM",
                "5": 1779323926591_i64,
                "10": {
                    "detailNotice": "未付款，买家关闭了订单",
                    "reminderTitle": "买家已拍下，待付款",
                    "reminderContent": "未付款，买家关闭了订单",
                    "reminderUrl": "fleamarket://message_chat?itemId=1042662837101&peerUserId=2219662209062&sid=60093789939",
                    "senderUserId": "2219662209062"
                }
            }
        });

        let event = parse_live_sync_message("90000000001", &payload).expect("parsed event");
        assert_eq!(event.session_id, "60093789939");
        assert_eq!(event.peer_uid, "2219662209062");
        assert_eq!(event.text, "未付款，买家关闭了订单");
        assert_eq!(event.sender_nick, "");
        assert_eq!(event.peer_nick, "");
    }

    #[test]
    fn parse_numeric_order_status_card_extracts_order_detail_link() {
        let payload = json!({
            "1": {
                "1": {"1": "2626821437@goofish"},
                "2": "61825916223@goofish",
                "3": "7312e62ea7224c1e918bd1444b2383fe.PNM",
                "5": 1779630108000_i64,
                "6": {
                    "1": 101,
                    "3": {
                        "2": "我已付款，等待你发货",
                        "4": 26,
                        "5": "{\"contentType\":26,\"dxCard\":{\"item\":{\"main\":{\"targetUrl\":\"fleamarket://order_detail?id=5116921095856268143&role=Seller\",\"exContent\":{\"title\":\"我已付款，等待你发货\",\"desc\":\"请包装好商品，并按我在闲鱼上提供的地址发货\"}}}}}"
                    }
                },
                "10": {
                    "detailNotice": "[卡片消息]",
                    "reminderTitle": "吉恩斯弗兰克",
                    "reminderContent": "[卡片消息]",
                    "reminderUrl": "fleamarket://message_chat?itemId=1053115290129&peerUserId=2626821437&sid=61825916223&adv=no",
                    "senderUserId": "2626821437",
                    "sessionType": "1"
                }
            }
        });

        let event = parse_live_sync_message("90000000004", &payload).expect("parsed event");
        assert_eq!(event.session_id, "61825916223");
        assert_eq!(event.content_type, 26);
        assert_eq!(event.kind, "card");
        assert_eq!(event.text, "请包装好商品，并按我在闲鱼上提供的地址发货");
        assert_eq!(event.title, "我已付款，等待你发货");
        assert_eq!(event.preview, "请包装好商品，并按我在闲鱼上提供的地址发货");
        let meta: Value = serde_json::from_str(&event.card_meta).expect("card meta json");
        assert_eq!(
            meta["linkUrl"],
            "fleamarket://order_detail?id=5116921095856268143&role=Seller"
        );
    }

    #[test]
    fn parse_numeric_outgoing_inquiry_uses_reminder_peer_identity() {
        let payload = json!({
            "1": {
                "1": {"1": "90000000001@goofish"},
                "2": "61915880115@goofish",
                "3": "206edc9e184b4a248168759fdd585058.PNM",
                "5": 1779286146044_i64,
                "10": {
                    "detailNotice": "是 350 升的吗",
                    "reminderTitle": "san丹530",
                    "reminderContent": "是 350 升的吗",
                    "reminderUrl": "fleamarket://message_chat?itemId=1049278015272&peerUserId=2609769497&sid=61915880115",
                    "senderUserId": "90000000001"
                }
            }
        });

        let event = parse_live_sync_message("90000000001", &payload).expect("parsed event");
        assert_eq!(event.session_id, "61915880115");
        assert_eq!(event.peer_uid, "2609769497");
        assert_eq!(event.peer_nick, "san丹530");
        assert_eq!(event.peer_role, "seller");
        assert_eq!(event.self_role, "buyer");
        assert_eq!(event.item_id, "1049278015272");
        assert_eq!(event.text, "是 350 升的吗");
        assert!(event.is_self);
    }

    #[test]
    fn parse_live_sync_message_extracts_item_title_and_pic_from_ext_json_blob() {
        let payload = json!({
            "cid": "61826145707@goofish",
            "sessionType": 1,
            "messageId": "msg-ext-item.PNM",
            "messageUuid": "msg-ext-item",
            "createAt": 1779511307168_i64,
            "content": {
                "contentType": 1,
                "text": { "text": "这个还在吗" }
            },
            "sender": {
                "uid": "65320912@goofish",
                "nick": "Chen"
            },
            "extension": {
                "sessionType": 1,
                "extJson": "{\"messageId\":\"msg-ext-item\",\"params\":{\"itemId\":\"1042662837101\",\"itemTitle\":\"露营车\",\"itemMainPic\":\"https://img.example/camper.png\"}}"
            }
        });

        let event = parse_live_sync_message("90000000001", &payload).expect("parsed event");
        assert_eq!(event.session_id, "61826145707");
        assert_eq!(event.item_id, "1042662837101");
        assert_eq!(event.item_title, "露营车");
        assert_eq!(event.item_pic, "https://img.example/camper.png");
    }
}
