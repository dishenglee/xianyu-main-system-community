//! PC 消息路由 + 上游 hydrate/persist/normalize (split from accounts/mod.rs in B4.4).
//!
//! 包含 /api/messages/* PC 端的 session/history/info 路由，以及对应的
//! 上游 hydrate、数据库 persist、响应 normalize 工具函数和会话加载帮助。
//!
//! Sub-modules:
//!   sessions    — session CRUD handlers
//!   history     — history fetching, hydration, prefetch
//!   persistence — SQLite read/write helpers
//!   live_ops    — live ACCS WS send/create/image

use serde_json::{json, Map, Value};

use crate::state::AppState;

use super::shared::{
    default_page_size_30, default_session_type, default_session_types, parse_json_array, ApiResult,
};
use super::ResponseError;

mod history;
mod live_ops;
mod live_ops_cards;
mod normalize;
mod persistence;
mod read_recall;
mod sessions;

// ---------------------------------------------------------------------------
// Public re-exports consumed by accounts/mod.rs and other sibling modules
// ---------------------------------------------------------------------------

pub(crate) use history::hydrate_message_session_info_from_upstream;
pub use history::prefetch_account_message_history;
pub(crate) use history::{
    hydrate_message_history_from_upstream, hydrate_message_sessions_from_upstream,
    hydrate_message_sessions_from_wx_session_list,
};
pub(crate) use live_ops::escape_json_string;
// Persistence helpers — re-exported so sub-modules can use super:: imports
pub(super) use history::pc_message_history;
pub(super) use persistence::{history_row_to_value, load_sessions_for_uid};
pub(crate) use persistence::{
    load_single_session_info, load_trusted_single_session_info, persist_message_history_page,
    persist_message_sessions,
};
pub(super) use sessions::{
    pc_mark_session_read, pc_message_headinfo, pc_session_info, pc_sessions, pc_sessions_batch,
};

// Handler re-exports needed by the parent router
pub(super) use live_ops::{live_create_chat, live_send_image, pc_send_message};
pub(crate) use live_ops::{
    live_send_image as live_send_image_internal, pc_send_message as pc_send_message_internal,
};
pub(crate) use live_ops::{LiveSendImagePayload, PcSendPayload};
pub(super) use live_ops_cards::{
    live_send_item, live_send_location, nearby_pois, search_items_for_send, search_pois,
};
pub(super) use read_recall::live_mark_session_read;
pub(crate) use read_recall::live_recall_message;
pub(crate) use read_recall::LiveRecallMessagePayload;

// Normalize re-exports — identity/content normalization used by history + sessions
pub(super) use normalize::{
    extract_item_id_from_value, masked_uid_matches, merge_cached_session_identity,
    message_location_payload, normalize_message_history, normalize_message_item_id,
    normalize_message_sessions, resolve_session_participants, resolve_trade_roles,
};
pub(crate) use normalize::{
    extract_peer_uid_from_reminder_url, is_platform_notification_peer, is_self_message,
    message_uids_match, message_user_display, normalize_message_content,
    normalize_message_session_identity_from_session, sender_user_id,
};

// ---------------------------------------------------------------------------
// Shared data types — used across all sub-modules + sibling modules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(crate) struct MessageSessionIdentity {
    pub(crate) session_id: String,
    pub(crate) session_type: i64,
    pub(crate) version: i64,
    pub(crate) unread: i64,
    pub(crate) latest_message_id: String,
    pub(crate) peer_uid: String,
    pub(crate) peer_nick: String,
    pub(crate) peer_logo: String,
    pub(crate) peer_role: String,
    pub(crate) self_role: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
    pub(crate) item_pic: String,
    pub(crate) summary: String,
    pub(crate) ts: i64,
    pub(crate) sort_index: i64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MessageHistoryEntry {
    pub(crate) message_id: String,
    /// 闲鱼平台原始 messageId（PNM 格式，如 "4111237573609.PNM"），
    /// 跟 `message_id`（UUID）不同 — UUID 用作我们这边的主键 / dedup key，
    /// PNM 是撤回接口识别的消息格式。
    /// 来源：userMessageModels[].message.messageId。
    pub(crate) upstream_message_id: String,
    pub(crate) version: i64,
    pub(crate) ts: i64,
    pub(crate) kind: String,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) image_url: String,
    pub(crate) image_urls: Vec<String>,
    pub(crate) content_type: i64,
    pub(crate) preview: String,
    pub(crate) sender_nick: String,
    pub(crate) sender_logo: String,
    pub(crate) is_self: bool,
    // Peer-read indicator. Comes from the upstream `message.unreadCount`
    // field on each userMessageModel. Semantics confirmed by packet capture:
    //  - is_self=true (我发的) + unread_count=0 → peer 已读
    //  - is_self=true + unread_count>0 → peer 未读
    //  - is_self=false → "我"未读消息数 (already covered by session-level
    //    unread count; this per-message value is only useful for outgoing).
    pub(crate) unread_count: i64,
    pub(crate) recalled_at: i64,
    pub(crate) recalled_original_text: String,
    pub(crate) recalled_original_image_url: String,
    // JSON blob of seller+item meta for dxCard messages (25/26/28). Empty
    // otherwise. See NormalizedMessageContent::card_meta for the shape.
    pub(crate) card_meta: String,
    // 引用回复（chat quote-reply）— `reply_to_message_id` 是被引用消息的 PNM
    // ("<seq>.PNM")，从 message.extension.extJson.replyMessageId 解出来；
    // `reply_to_text` 是发送时显示的引用文本（渲染过的 preview/text，可能含
    // [图片] 前缀），存一份避免接收端要回扫历史才能渲染气泡。
    pub(crate) reply_to_message_id: String,
    pub(crate) reply_to_text: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct MessageHistoryPage {
    pub(crate) session: MessageSessionIdentity,
    pub(crate) has_more: bool,
    pub(crate) next_cursor: i64,
    pub(crate) latest_cursor: i64,
    pub(crate) messages: Vec<MessageHistoryEntry>,
}

pub(crate) const MAX_SESSION_LIST_LIMIT: i64 = 500;

pub(crate) async fn ensure_message_session_belongs_to_uid(
    state: &AppState,
    uid: &str,
    session_id: &str,
) -> Result<Value, ResponseError> {
    ensure_message_session_belongs_to_uid_with_type(state, uid, session_id, 1).await
}

pub(crate) async fn ensure_message_session_belongs_to_uid_with_type(
    state: &AppState,
    uid: &str,
    session_id: &str,
    session_type: i64,
) -> Result<Value, ResponseError> {
    persistence::load_trusted_single_session_info(
        state.storage().live_state(),
        uid,
        session_id,
        session_type,
    )
    .await
    .map_err(ResponseError::internal)?
    .ok_or_else(|| ResponseError::not_found("会话不存在"))
}

pub(crate) async fn ensure_message_item_belongs_to_uid(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<(), ResponseError> {
    let found = sqlx::query("SELECT 1 FROM account_items WHERE uid = ? AND item_id = ? LIMIT 1")
        .bind(uid.trim())
        .bind(item_id.trim())
        .fetch_optional(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?
        .is_some();
    if found {
        Ok(())
    } else {
        Err(ResponseError::not_found("商品不存在"))
    }
}

#[derive(Debug, Clone, Default)]
pub struct LiveMessageEvent {
    pub session_id: String,
    pub session_type: i64,
    pub message_id: String,
    /// PNM 格式 messageId（撤回 API 要这个），跟 message_id（UUID）平行存在。
    pub upstream_message_id: String,
    pub version: i64,
    pub ts: i64,
    pub peer_uid: String,
    pub peer_nick: String,
    pub peer_logo: String,
    pub peer_role: String,
    pub self_role: String,
    pub item_id: String,
    pub item_title: String,
    pub item_pic: String,
    pub kind: String,
    pub title: String,
    pub text: String,
    pub image_url: String,
    pub preview: String,
    pub sender_uid: String,
    pub sender_nick: String,
    pub sender_logo: String,
    pub is_self: bool,
    // Peer-read indicator from upstream `message.unreadCount` (0 = peer read,
    // >0 = peer hasn't read). For incoming messages this is from our own POV
    // and the frontend mostly ignores it (session-level unread already shows
    // the badge).
    pub unread_count: i64,
    pub accs_pts: i64,
    /// 卡片元数据 JSON 字符串（contentType 7/8 = itemCard, 30 = locationCard）。
    /// 空 = 非卡片消息。直接透传给前端做富卡片渲染。
    pub card_meta: String,
    /// 上游真实 contentType（1=text, 2=image, 5=sticker, 7/8=item card, 30=location）。
    /// event_processor 直接传给 message_history_cache.content_type，前端按它分发渲染。
    pub content_type: i64,
    // 引用回复 PNM — 来自 message.extension.extJson.replyMessageId。WS 推送的
    // 帧上同样能拿（event_parser 解开 extJson 后回填）。frontend 收到事件后
    // 会把 reply_to_message_id 塞回 message_history_cache 行，让接收侧气泡
    // 渲染 quote 块。空 = 这条不是回复。
    pub reply_to_message_id: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LiveSessionCursor {
    pub(crate) latest_version: i64,
    pub(crate) latest_message_id: String,
    pub(crate) unread_count: i64,
}

#[derive(Debug, Clone, Default)]
pub(super) struct NormalizedMessageContent {
    pub(super) kind: String,
    pub(super) title: String,
    pub(super) text: String,
    pub(super) image_url: String,
    pub(super) image_urls: Vec<String>,
    pub(super) content_type: i64,
    pub(super) preview: String,
    // JSON blob with rich-card metadata for content_type 25/26/28 (交易/订单/
    // 物流 dxCard). Keys: sellerNick, sellerAvatar, itemTitle, itemSubtitle,
    // itemPic, linkUrl. Empty string means "no card meta on this message".
    pub(super) card_meta: String,
}

// ---------------------------------------------------------------------------
// WX mini-program constants (shared by h5 request helper)
// ---------------------------------------------------------------------------

const WX_MINI_UTDID: &str = "28b8IGorElQCAQAAAACuy6pp";
const WX_MINI_APP_VERSION: &str = "0.3.79";
pub(super) const WX_MESSAGE_SESSION_TYPES: &str = "[1,3,6]";

// ---------------------------------------------------------------------------
// Shared helpers — used by multiple sub-modules
// ---------------------------------------------------------------------------

pub(super) fn str_to_json_number(s: &str) -> Value {
    s.trim()
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(s.to_string()))
}

fn inject_wx_common_fields(mut data: Value) -> Value {
    if let Some(obj) = data.as_object_mut() {
        obj.entry("utdid")
            .or_insert_with(|| Value::String(WX_MINI_UTDID.to_string()));
        obj.entry("platform")
            .or_insert_with(|| Value::String("ios".to_string()));
        obj.entry("miniAppVersion")
            .or_insert_with(|| Value::String(WX_MINI_APP_VERSION.to_string()));
    }
    data
}

pub(super) async fn successful_message_h5_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    let data = inject_wx_common_fields(data);
    super::successful_upstream_h5_request(state, uid, api, version, data, extra_params).await
}

pub(super) async fn successful_message_protocol_h5_request(
    state: &AppState,
    uid: &str,
    opcode: u32,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    let protocol = super::resolve_account_protocol(state, opcode).await?;
    successful_message_h5_request(
        state,
        uid,
        protocol.api(),
        protocol.version(),
        data,
        extra_params,
    )
    .await
}

// ---------------------------------------------------------------------------
// JSON utility helpers (pub(super) — used broadly across sub-modules)
// ---------------------------------------------------------------------------

pub(super) fn coerce_mtop_data(response: &Value) -> Map<String, Value> {
    match response.get("data") {
        Some(Value::Object(object)) => object.clone(),
        Some(Value::String(raw)) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    }
}

pub(crate) fn json_object(value: Option<&Value>) -> Map<String, Value> {
    value
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn json_array(value: Option<&Value>) -> Vec<Value> {
    value.and_then(Value::as_array).cloned().unwrap_or_default()
}

pub(crate) fn json_string(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(raw)) => raw.trim().to_string(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn json_i64(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
        Some(Value::String(raw)) => raw.trim().parse::<i64>().unwrap_or(0),
        Some(Value::Bool(flag)) => i64::from(*flag),
        _ => 0,
    }
}

pub(super) fn json_bool(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_i64().unwrap_or(0) != 0,
        Some(Value::String(raw)) => matches!(raw.trim(), "true" | "1" | "TRUE" | "True"),
        _ => false,
    }
}

pub(crate) fn is_legacy_platform_reminder_summary(summary: &str) -> bool {
    let summary = summary.trim();
    if summary.is_empty() {
        return false;
    }

    [
        "快给ta一个评价",
        "期待你的评价",
        "你已被禁言",
        "投缘优惠",
        "闲鱼小红花",
    ]
    .iter()
    .any(|pattern| summary.contains(pattern))
}

pub(crate) fn should_suppress_chat_unread(session_type: i64, summary: &str) -> bool {
    session_type == 1 && is_legacy_platform_reminder_summary(summary)
}

// ---------------------------------------------------------------------------
// Session identity helpers (used by both sessions + history sub-modules)
// ---------------------------------------------------------------------------

pub(crate) fn message_session_identity_to_value(session: &MessageSessionIdentity) -> Value {
    json!({
        "sessionId": session.session_id,
        "sessionType": session.session_type,
        "version": session.version,
        "unread": session.unread,
        "peerUid": session.peer_uid,
        "peerNick": session.peer_nick,
        "peerLogo": session.peer_logo,
        "peerRole": session.peer_role,
        "selfRole": session.self_role,
        "itemId": session.item_id,
        "itemTitle": session.item_title,
        "itemPic": session.item_pic,
        "summary": session.summary,
        "ts": session.ts,
        "sortIndex": session.sort_index,
    })
}

pub(crate) fn message_session_identity_from_value(value: &Value) -> MessageSessionIdentity {
    MessageSessionIdentity {
        session_id: json_string(value.get("sessionId")),
        session_type: json_i64(value.get("sessionType")).max(1),
        version: json_i64(value.get("version")),
        unread: json_i64(value.get("unread")),
        latest_message_id: json_string(value.get("latestMessageId")),
        peer_uid: json_string(value.get("peerUid")),
        peer_nick: json_string(value.get("peerNick")),
        peer_logo: json_string(value.get("peerLogo")),
        peer_role: json_string(value.get("peerRole")),
        self_role: json_string(value.get("selfRole")),
        item_id: json_string(value.get("itemId")),
        item_title: json_string(value.get("itemTitle")),
        item_pic: json_string(value.get("itemPic")),
        summary: json_string(value.get("summary")),
        ts: json_i64(value.get("ts")),
        sort_index: json_i64(value.get("sortIndex")),
    }
}

pub(super) fn info_has_identity(session: &MessageSessionIdentity) -> bool {
    if session.session_type == 1 {
        let has_peer = !peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid)
            || !session.peer_uid.trim().is_empty();
        let has_item = !normalize_message_item_id(&session.item_id).is_empty()
            || !session.item_title.trim().is_empty()
            || !session.item_pic.trim().is_empty();
        return has_peer && has_item;
    }

    !peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid)
        || !session.peer_uid.trim().is_empty()
}

pub(crate) fn peer_nick_needs_fallback(peer_nick: &str, peer_uid: &str) -> bool {
    let nick = peer_nick.trim();
    if nick.is_empty() {
        return true;
    }
    if peer_nick_is_system_copy(nick) {
        return true;
    }
    if matches!(nick, "你有一条新消息" | "您有一条新消息") {
        return true;
    }
    if matches!(nick, "未知" | "未知会话" | "未知账号" | "未知客户") {
        return true;
    }
    if nick.starts_with("用户_") || nick.starts_with("tbNick_") {
        return true;
    }
    if !peer_uid.trim().is_empty() && nick == peer_uid.trim() {
        return true;
    }
    nick.chars().all(|ch| ch.is_ascii_digit())
}

fn peer_nick_is_system_copy(nick: &str) -> bool {
    if nick.contains("闲鱼小红花") {
        return true;
    }

    const ORDER_COPY_PATTERNS: [&str; 6] = [
        "待付款",
        "等待你发货",
        "待发货",
        "记得及时发货",
        "去发货",
        "退款成功",
    ];

    ORDER_COPY_PATTERNS
        .iter()
        .any(|pattern| nick.contains(pattern))
}

pub(crate) fn sanitized_peer_nick_candidate(peer_nick: &str, peer_uid: &str) -> String {
    let candidate = peer_nick.trim();
    if peer_nick_needs_fallback(candidate, peer_uid) {
        String::new()
    } else {
        candidate.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::peer_nick_needs_fallback;

    #[test]
    fn placeholder_copy_requires_peer_nick_fallback() {
        assert!(peer_nick_needs_fallback("你有一条新消息", "peer-1"));
        assert!(peer_nick_needs_fallback("您有一条新消息", "peer-1"));
        assert!(peer_nick_needs_fallback("买家已拍下，待付款", "peer-1"));
        assert!(peer_nick_needs_fallback(
            "卖家人不错？送Ta闲鱼小红花",
            "peer-1"
        ));
        assert!(!peer_nick_needs_fallback("真实买家昵称", "peer-1"));
    }
}

pub(super) fn message_session_identity_to_value_with_history(page: &MessageHistoryPage) -> Value {
    let mut object = message_session_identity_to_value(&page.session)
        .as_object()
        .cloned()
        .unwrap_or_default();
    object.insert("hasMore".to_string(), Value::Bool(page.has_more));
    object.insert("nextCursor".to_string(), Value::from(page.next_cursor));
    object.insert("latestCursor".to_string(), Value::from(page.latest_cursor));
    object.insert(
        "messages".to_string(),
        Value::Array(
            page.messages
                .iter()
                .map(message_history_entry_to_value)
                .collect(),
        ),
    );
    Value::Object(object)
}

// Alias for clarity — used by history sub-module
pub(super) use message_session_identity_to_value_with_history as message_history_page_to_value;

pub(super) fn message_history_entry_to_value(message: &MessageHistoryEntry) -> Value {
    let card_meta = if message.card_meta.is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(&message.card_meta).unwrap_or(Value::Null)
    };
    json!({
        "sessionType": 0,
        "messageId": message.message_id,
        // PNM 格式的上游 messageId — 撤回必须传它（参考 read_recall.rs）
        "upstreamMessageId": message.upstream_message_id,
        "version": message.version,
        "ts": message.ts,
        "kind": message.kind,
        "title": message.title,
        "text": message.text,
        "imageUrl": message.image_url,
        "imageUrls": message.image_urls,
        "contentType": message.content_type,
        "preview": message.preview,
        "senderNick": message.sender_nick,
        "senderLogo": message.sender_logo,
        "isSelf": message.is_self,
        "unreadCount": message.unread_count,
        "cardMeta": card_meta,
        // 引用回复两件套：replyToMessageId = 被引用的 PNM；replyToText = 当时
        // 在引用条里渲染的文本。前端在气泡上方画一个浅色 quote 块。
        "replyToMessageId": message.reply_to_message_id,
        "replyToText": message.reply_to_text,
    })
}

// ---------------------------------------------------------------------------
// Session enrich + merge helpers (used by sessions + history)
// ---------------------------------------------------------------------------

pub(super) async fn enrich_session_values_with_upstream(
    state: &AppState,
    uid: &str,
    mut sessions: Vec<Value>,
) -> Vec<Value> {
    for session in &mut sessions {
        if !session_needs_identity_fallback(session) {
            continue;
        }

        let session_id = json_string(session.get("sessionId"));
        if session_id.is_empty() {
            continue;
        }
        let session_type = session
            .get("sessionType")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        let Ok(info) =
            hydrate_message_session_info_from_upstream(state, uid, &session_id, session_type).await
        else {
            continue;
        };
        merge_session_value_with_identity(session, &info);
    }

    sort_session_values_by_identity(&mut sessions);
    sessions
}

fn session_needs_identity_fallback(session: &Value) -> bool {
    let session_type = session
        .get("sessionType")
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let peer_uid = json_string(session.get("peerUid"));
    let peer_nick = json_string(session.get("peerNick"));
    if peer_nick_needs_fallback(&peer_nick, &peer_uid) {
        return true;
    }
    session_type == 1
        && (json_string(session.get("itemId")).is_empty()
            || json_string(session.get("itemTitle")).is_empty()
            || json_string(session.get("itemPic")).is_empty())
}

fn merge_session_value_with_identity(session: &mut Value, fallback: &MessageSessionIdentity) {
    let Some(object) = session.as_object_mut() else {
        return;
    };
    let fallback_value = message_session_identity_to_value(fallback);
    let fallback_object = fallback_value.as_object().cloned().unwrap_or_default();

    merge_session_text_field(object, &fallback_object, "peerUid", false);
    merge_session_text_field(object, &fallback_object, "peerNick", true);
    merge_session_text_field(object, &fallback_object, "peerLogo", false);
    merge_session_text_field(object, &fallback_object, "peerRole", false);
    merge_session_text_field(object, &fallback_object, "selfRole", false);
    merge_session_text_field(object, &fallback_object, "itemId", false);
    merge_session_text_field(object, &fallback_object, "itemTitle", false);
    merge_session_text_field(object, &fallback_object, "itemPic", false);
    merge_session_text_field(object, &fallback_object, "summary", false);

    let current_ts = object.get("ts").and_then(Value::as_i64).unwrap_or(0);
    let fallback_ts = fallback_object
        .get("ts")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if fallback_ts > current_ts {
        object.insert("ts".to_string(), Value::from(fallback_ts));
    }

    let current_version = object.get("version").and_then(Value::as_i64).unwrap_or(0);
    let fallback_version = fallback_object
        .get("version")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if fallback_version > current_version {
        object.insert("version".to_string(), Value::from(fallback_version));
    }

    let current_sort = object.get("sortIndex").and_then(Value::as_i64).unwrap_or(0);
    let fallback_sort = fallback_object
        .get("sortIndex")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if normalized_session_sort_value_for_merge(&Value::Object(object.clone()))
        < normalized_session_sort_value_for_merge(&Value::Object(fallback_object.clone()))
        || fallback_sort > current_sort
    {
        object.insert(
            "sortIndex".to_string(),
            Value::from(fallback_sort.max(fallback_ts)),
        );
    }
}

fn merge_session_text_field(
    object: &mut Map<String, Value>,
    fallback: &Map<String, Value>,
    key: &str,
    placeholder_sensitive: bool,
) {
    let current = json_string(object.get(key));
    let incoming = json_string(fallback.get(key));
    if incoming.is_empty() {
        return;
    }
    let should_replace = if placeholder_sensitive {
        let peer_uid = json_string(object.get("peerUid"));
        peer_nick_needs_fallback(&current, &peer_uid)
    } else {
        current.is_empty()
    };
    if should_replace {
        object.insert(key.to_string(), Value::String(incoming));
    }
}

fn sort_session_values_by_identity(sessions: &mut [Value]) {
    sessions.sort_by(|left, right| {
        session_type_priority_for_sort(left)
            .cmp(&session_type_priority_for_sort(right))
            .then_with(|| {
                normalized_session_sort_value_for_merge(right)
                    .cmp(&normalized_session_sort_value_for_merge(left))
            })
            .then_with(|| {
                json_string(left.get("sessionId")).cmp(&json_string(right.get("sessionId")))
            })
    });
}

fn session_type_priority_for_sort(session: &Value) -> i64 {
    match session
        .get("sessionType")
        .and_then(Value::as_i64)
        .unwrap_or(1)
    {
        3 | 6 => 0,
        _ => 1,
    }
}

fn normalized_session_sort_value_for_merge(session: &Value) -> i64 {
    let primary = session
        .get("sortIndex")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if primary > 0 {
        let primary_text = primary.to_string();
        if primary_text.len() == 14 && primary_text.starts_with("20") {
            let ts = session.get("ts").and_then(Value::as_i64).unwrap_or(0);
            if ts > 0 {
                return ts;
            }
        }
        return primary;
    }

    session
        .get("ts")
        .and_then(Value::as_i64)
        .or_else(|| session.get("version").and_then(Value::as_i64))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Session type + message normalization helpers (pub(super))
// ---------------------------------------------------------------------------

pub(super) fn parse_message_session_types(raw: &str) -> Vec<i64> {
    serde_json::from_str::<Vec<i64>>(raw).unwrap_or_default()
}

pub(super) fn filter_session_values_by_types(
    sessions: Vec<Value>,
    allowed_types: &[i64],
) -> Vec<Value> {
    if allowed_types.is_empty() {
        return sessions;
    }
    sessions
        .into_iter()
        .filter(|session| {
            allowed_types.contains(
                &session
                    .get("sessionType")
                    .and_then(Value::as_i64)
                    .unwrap_or(1),
            )
        })
        .collect()
}
