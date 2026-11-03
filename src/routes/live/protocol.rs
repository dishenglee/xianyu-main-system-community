use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;

use crate::{routes::accounts::generate_live_mid, state::AppState};

use super::AUTOSYNC_MAP_TTL;
use super::AUTOSYNC_ORDERS_DEBOUNCE;

/// JSON string escaper for hand-built WS payloads. We hand-build because
/// `serde_json::json!()` reorders keys alphabetically (BTreeMap), which the
/// ACCS server treats as malformed and silently drops, then RSTs the socket
/// after a few seconds of unanswered fingerprint pings.
pub(super) fn escape_json_string_inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

pub(super) fn unix_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis()
}

/// Detect trade-related live events that should trigger an order re-sync.
///
/// Xianyu uses `sessionType == 3` for 交易/订单通知 channels. We also sniff the
/// event `type` field for keywords as a belt-and-suspenders check, since some
/// payloads arrive without the session type populated.
pub(super) fn event_looks_like_trade(event: &Value) -> bool {
    if event.get("sessionType").and_then(Value::as_i64) == Some(3) {
        return true;
    }
    if let Some(session_type) = event.get("session_type").and_then(Value::as_i64) {
        if session_type == 3 {
            return true;
        }
    }
    let content_type = event
        .get("contentType")
        .or_else(|| event.get("content_type"))
        .and_then(Value::as_i64);
    if matches!(content_type, Some(25 | 26 | 28 | 32)) && event_has_order_signal(event) {
        return true;
    }
    let ty = event.get("type").and_then(Value::as_str).unwrap_or("");
    if ty.contains("order") || ty.contains("trade") || ty.contains("deal") {
        return true;
    }
    false
}

fn event_has_order_signal(event: &Value) -> bool {
    let mut haystack = String::new();
    for key in ["title", "text", "rawPreview", "preview"] {
        if let Some(value) = event.get(key).and_then(Value::as_str) {
            haystack.push_str(value);
            haystack.push('\n');
        }
    }
    if let Some(card_meta) = event.get("cardMeta").or_else(|| event.get("card_meta")) {
        collect_json_strings(card_meta, &mut haystack);
    }

    const ORDER_PATTERNS: [&str; 10] = [
        "order_detail",
        "我已拍下，待付款",
        "已拍下，待付款",
        "买家已拍下，待付款",
        "待付款",
        "我已付款，等待你发货",
        "等待你发货",
        "待发货",
        "记得及时发货",
        "去发货",
    ];
    ORDER_PATTERNS
        .iter()
        .any(|pattern| haystack.contains(pattern))
}

fn collect_json_strings(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            out.push_str(text);
            out.push('\n');
        }
        Value::Array(items) => {
            for item in items {
                collect_json_strings(item, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_json_strings(value, out);
            }
        }
        _ => {}
    }
}

pub(super) fn autosync_last_fired() -> &'static Mutex<HashMap<String, Instant>> {
    static STORE: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Fire-and-forget order resync for `uid`, debounced so a flurry of trade
/// notifications can't hammer the upstream API.
pub(super) fn maybe_autosync_orders(state: AppState, uid: String) {
    {
        let mut map = autosync_last_fired().lock().unwrap();
        let now = Instant::now();
        // Opportunistic TTL prune: drop entries that haven't been touched for
        // AUTOSYNC_MAP_TTL (10x debounce window). Bounds memory under churn
        // (accounts come and go) without needing a dedicated sweeper task.
        map.retain(|_, ts| now.duration_since(*ts) < AUTOSYNC_MAP_TTL);
        if let Some(prev) = map.get(&uid) {
            if now.duration_since(*prev) < AUTOSYNC_ORDERS_DEBOUNCE {
                return;
            }
        }
        map.insert(uid.clone(), now);
    }
    tokio::spawn(async move {
        match crate::routes::market::sync_orders_to_db(&state, &uid, true).await {
            Ok(outcome) => {
                tracing::info!(
                    uid = %uid,
                    ok = outcome.ok,
                    total = outcome.total,
                    "autosync orders triggered by trade notification"
                );
                // Broadcast over the live channel so the frontend orders page
                // can refresh without the user hitting the manual refresh
                // button. The frontend listens via GlobalMessageNotify and
                // dispatches a window event for any mounted orders view to
                // pick up.
                if outcome.ok {
                    state.live_events().publish(serde_json::json!({
                        "type": "orders.synced",
                        "uid": uid,
                        "total": outcome.total,
                    }));
                }
            }
            Err(err) => {
                tracing::warn!(uid = %uid, error = %err, "autosync orders failed");
            }
        }
    });
}

pub(super) async fn acknowledge_sync_push(
    websocket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    payload: &Value,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    let headers = payload
        .get("headers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // Python parity (`xianyu_wx_live_client.py:740-755 _send_ack`):
    //   ack = {"code": 200, "headers": {"mid": "...", "sid": "..."}}
    //   await websocket.send(json.dumps(ack, ensure_ascii=False))
    // i.e. ONLY mid+sid, never app-key/ua/dt. An earlier Rust version
    // included app-key/ua based on a comment claim that "without them ACK
    // is silently dropped" — but Python's parity test (which uses just
    // mid+sid) successfully receives push events while Rust with
    // app-key/ua included doesn't. Match Python byte-for-byte.
    let mid = headers
        .get("mid")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sid = headers.get("sid").and_then(Value::as_str).unwrap_or("");
    let ack = format!(
        r#"{{"code": 200, "headers": {{"mid": "{}", "sid": "{}"}}}}"#,
        escape_json_string_inline(mid),
        escape_json_string_inline(sid),
    );
    websocket.send(TungsteniteMessage::Text(ack)).await
}

pub(super) fn is_sync_push_payload(payload: &Value) -> bool {
    payload
        .get("body")
        .and_then(Value::as_object)
        .and_then(|body| body.get("syncPushPackage"))
        .and_then(Value::as_object)
        .and_then(|sync| sync.get("data"))
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
}

/// Python parity (`xianyu_wx_live_client.py:614-622 _handle_packet` +
/// `:1388-1399 _is_sync_package`): ONLY ack frames whose
/// `body.syncPushPackage.data` is a non-empty list. Python doesn't ack
/// fingerprint-only frames (`syncExtensionModel`/`syncExtraType` only)
/// nor empty `syncPushPackage` shells. An earlier Rust version ACKed
/// these too, but that turned every fingerprint heartbeat into an
/// unsolicited ack on the server side, which appears to flag the client
/// as "not following protocol" and stops live push delivery.
pub(super) fn is_server_push_frame(payload: &Value) -> bool {
    is_sync_push_payload(payload)
}

pub(super) fn has_sync_push_package(payload: &Value) -> bool {
    payload
        .get("body")
        .and_then(Value::as_object)
        .and_then(|body| body.get("syncPushPackage"))
        .is_some()
}

pub(super) fn extract_correlation_group_id(payload: &Value) -> Option<String> {
    for key in ["body", "headers", "data"] {
        match payload.get(key) {
            Some(Value::Object(obj)) => {
                if let Some(cgi) = obj.get("correlationGroupId").and_then(Value::as_str) {
                    let cgi = cgi.trim();
                    if !cgi.is_empty() {
                        return Some(cgi.to_string());
                    }
                }
            }
            Some(Value::Array(arr)) => {
                for item in arr {
                    if let Some(cgi) = item.get("correlationGroupId").and_then(Value::as_str) {
                        let cgi = cgi.trim();
                        if !cgi.is_empty() {
                            return Some(cgi.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    payload
        .get("correlationGroupId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Pull `maxPts` / `maxHighPts` out of a `syncPushPackage` so we can commit
/// the cursor back to the server. Python POC line 752-755.
pub(super) fn extract_max_pts(payload: &Value) -> (i64, i64) {
    let Some(sync_pkg) = payload
        .get("body")
        .and_then(Value::as_object)
        .and_then(|body| body.get("syncPushPackage"))
        .and_then(Value::as_object)
    else {
        return (0, 0);
    };
    let max_pts = sync_pkg.get("maxPts").and_then(Value::as_i64).unwrap_or(0);
    let max_high_pts = sync_pkg
        .get("maxHighPts")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    (max_pts, max_high_pts)
}

/// Send `/r/SyncStatus/ackDiff` carrying the latest cursor during registration.
/// Runtime sync pushes are acknowledged with the lightweight mid/sid ACK only;
/// sending extra ackDiff frames after every push or heartbeat makes the real
/// ACCS gateway reset otherwise healthy sockets.
pub(super) async fn send_cursor_ack(
    websocket: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    pts: i64,
    high_pts: i64,
) -> Result<(), tokio_tungstenite::tungstenite::Error> {
    let current_time = unix_timestamp_millis() as i64;
    let payload = format!(
        concat!(
            r#"{{"lwp": "/r/SyncStatus/ackDiff", "headers": {{"mid": "{mid}"}}, "body": [{{"#,
            r#""pipeline": "sync", "tooLong2Tag": "PNM,1", "channel": "sync", "topic": "sync", "#,
            r#""highPts": {high_pts}, "pts": {pts}, "seq": 0, "timestamp": {ts}}}]}}"#,
        ),
        mid = generate_live_mid(),
        high_pts = high_pts,
        pts = pts,
        ts = current_time,
    );
    websocket.send(TungsteniteMessage::Text(payload)).await
}

pub(super) fn normalized_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn command_result_payload(
    event_type: &str,
    request_id: &str,
    session_id: &str,
    result: Result<serde_json::Value, String>,
) -> serde_json::Value {
    match result {
        Ok(response) => serde_json::json!({
            "type": event_type,
            "ok": true,
            "requestId": request_id,
            "sessionId": session_id,
            "response": response,
        }),
        Err(reason) => serde_json::json!({
            "type": event_type,
            "ok": false,
            "requestId": request_id,
            "sessionId": session_id,
            "reason": reason,
            "response": {
                "detail": reason,
            }
        }),
    }
}

pub(super) fn generate_uuid4_like_local() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::event_looks_like_trade;

    #[test]
    fn detects_trade_events_from_session_type() {
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 3,
        })));
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "session_type": 3,
        })));
    }

    #[test]
    fn detects_trade_events_from_order_status_card_text() {
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 26,
            "title": "我已拍下，待付款",
            "text": "请双方沟通及时确认价格",
        })));
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 26,
            "title": "我已付款，等待你发货",
            "text": "请包装好商品，并按我在闲鱼上提供的地址发货",
        })));
    }

    #[test]
    fn detects_trade_events_from_order_detail_link() {
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 26,
            "title": "记得及时发货",
            "cardMeta": {
                "linkUrl": "fleamarket://order_detail?id=5116921095856268143&role=Seller"
            },
        })));
    }

    #[test]
    fn detects_trade_events_from_dynamic_order_status_cards() {
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 32,
            "title": "我已付款，等待你发货",
            "text": "请包装好商品，并按我在闲鱼上提供的地址发货",
        })));
        assert!(event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 32,
            "title": "记得及时发货",
            "text": "如已发货，请点击「去发货」输入快递单号",
        })));
    }

    #[test]
    fn small_knife_cards_do_not_trigger_order_sync() {
        assert!(!event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 26,
            "title": "我刚刚刀成功啦, 获得小刀优惠¥6.12",
            "text": "买家小刀优惠将在24小时后失效",
            "rawPreview": "我刚刚刀成功啦, 获得小刀优惠¥6.12",
            "cardMeta": {
                "linkUrl": ""
            },
        })));
    }

    #[test]
    fn generic_notification_cards_do_not_trigger_order_sync() {
        assert!(!event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 28,
            "text": "您今日完成发布任务奖励的闲鱼币尚未领取",
            "senderNick": "闲鱼精选",
        })));
    }

    #[test]
    fn ordinary_chat_messages_do_not_trigger_trade_sync() {
        assert!(!event_looks_like_trade(&json!({
            "type": "message.received",
            "sessionType": 1,
            "contentType": 1,
            "text": "你好",
        })));
    }
}
