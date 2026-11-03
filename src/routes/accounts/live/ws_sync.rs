//! WebSocket-based sync operations: sync recent messages, fetch history.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::state::LiveResponseWaiterKind;
use crate::{routes::error::ResponseError, state::AppState};

use crate::routes::accounts::wx_live::{
    fetch_wx_live_access_token, generate_live_mid, wx_live_ws_url,
};
use crate::routes::accounts::{WX_LIVE_ACCS_APP_KEY, WX_LIVE_ORIGIN, WX_LIVE_USER_AGENT};

const WX_LIVE_MAX_CURSOR: i64 = 9_007_199_254_740_991;
use crate::routes::accounts::messages::escape_json_string;

use super::event_parser::{
    decode_wx_sync_data, extract_sync_push_data, parse_live_sync_messages, parse_ws_json_frame,
};
use super::event_processor::process_live_message_event;

#[allow(dead_code)] // P-4 fix: caller removed; kept as dead code reference
pub(crate) async fn sync_recent_message_events_via_ws(
    state: &AppState,
    uid: &str,
    max_wait: Duration,
) -> Result<usize, ResponseError> {
    let (access_token, device_id) = fetch_wx_live_access_token(state, uid).await?;

    let mut request = wx_live_ws_url()
        .into_client_request()
        .map_err(ResponseError::internal)?;
    request.headers_mut().insert(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(WX_LIVE_USER_AGENT),
    );
    request.headers_mut().insert(
        "Origin",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(WX_LIVE_ORIGIN),
    );

    let (mut websocket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(ResponseError::internal)?;

    let register_payload = json!({
        "lwp": "/reg",
        "headers": {
            "cache-header": "app-key token ua wv",
            "app-key": WX_LIVE_ACCS_APP_KEY,
            "token": access_token,
            "ua": WX_LIVE_USER_AGENT,
            "dt": "j",
            "wv": "im:3,au:3,sy:6",
            "sync": "0,0;0;0;",
            "did": device_id,
            "mid": generate_live_mid(),
        },
    });
    websocket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            register_payload.to_string(),
        ))
        .await
        .map_err(ResponseError::internal)?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64;
    let ack_diff_payload = json!({
        "lwp": "/r/SyncStatus/ackDiff",
        "headers": { "mid": generate_live_mid() },
        "body": [{
            "pipeline": "sync",
            "tooLong2Tag": "PNM,1",
            "channel": "sync",
            "topic": "sync",
            "highPts": 0,
            "pts": current_time * 1000,
            "seq": 0,
            "timestamp": current_time,
        }],
    });
    websocket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            ack_diff_payload.to_string(),
        ))
        .await
        .map_err(ResponseError::internal)?;

    let deadline = tokio::time::Instant::now() + max_wait;
    let mut persisted = 0_usize;
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next_frame = match tokio::time::timeout(remaining, websocket.next()).await {
            Ok(frame) => frame,
            Err(_) => break,
        };
        let Some(frame) = next_frame else {
            break;
        };
        let frame = frame.map_err(ResponseError::internal)?;
        let Some(payload) = parse_ws_json_frame(frame)? else {
            continue;
        };

        let headers = payload
            .get("headers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let is_sync_push = !extract_sync_push_data(&payload).is_empty();
        if is_sync_push {
            let ack = json!({
                "code": 200,
                "headers": {
                    "mid": headers.get("mid").and_then(Value::as_str).unwrap_or_default(),
                    "sid": headers.get("sid").and_then(Value::as_str).unwrap_or_default(),
                }
            });
            websocket
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    ack.to_string(),
                ))
                .await
                .map_err(ResponseError::internal)?;
            for (encoded, pts) in extract_sync_push_data(&payload) {
                let decoded = decode_wx_sync_data(&encoded)?;
                for mut event in parse_live_sync_messages(uid, &decoded) {
                    if pts > 0 {
                        event.accs_pts = pts;
                    }
                    if process_live_message_event(state, uid, event)
                        .await?
                        .is_some()
                    {
                        persisted += 1;
                    }
                }
            }
        }
    }

    let _ = websocket.close(None).await;
    Ok(persisted)
}

#[allow(dead_code)] // P-4 fix: caller removed; only fetch_message_history_via_active_socket is used
pub(crate) async fn fetch_message_history_via_ws(
    state: &AppState,
    uid: &str,
    session_id: &str,
    count: i64,
    cursor: i64,
) -> Result<Value, ResponseError> {
    let (access_token, device_id) = fetch_wx_live_access_token(state, uid).await?;

    let mut request = wx_live_ws_url()
        .into_client_request()
        .map_err(ResponseError::internal)?;
    request.headers_mut().insert(
        "User-Agent",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(WX_LIVE_USER_AGENT),
    );
    request.headers_mut().insert(
        "Origin",
        tokio_tungstenite::tungstenite::http::HeaderValue::from_static(WX_LIVE_ORIGIN),
    );

    let (mut websocket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(ResponseError::internal)?;

    let register_payload = json!({
        "lwp": "/reg",
        "headers": {
            "cache-header": "app-key token ua wv",
            "app-key": WX_LIVE_ACCS_APP_KEY,
            "token": access_token,
            "ua": WX_LIVE_USER_AGENT,
            "dt": "j",
            "wv": "im:3,au:3,sy:6",
            "sync": "0,0;0;0;",
            "did": device_id,
            "mid": generate_live_mid(),
        },
    });
    websocket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            register_payload.to_string(),
        ))
        .await
        .map_err(ResponseError::internal)?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64;
    let ack_diff_payload = json!({
        "lwp": "/r/SyncStatus/ackDiff",
        "headers": { "mid": generate_live_mid() },
        "body": [{
            "pipeline": "sync",
            "tooLong2Tag": "PNM,1",
            "channel": "sync",
            "topic": "sync",
            "highPts": 0,
            "pts": current_time * 1000,
            "seq": 0,
            "timestamp": current_time,
        }],
    });
    websocket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            ack_diff_payload.to_string(),
        ))
        .await
        .map_err(ResponseError::internal)?;

    let list_mid = generate_live_mid();
    let list_payload = json!({
        "lwp": "/r/MessageManager/listUserMessages",
        "headers": { "mid": list_mid },
        "body": [
            format!("{session_id}@goofish"),
            false,
            if cursor > 0 { cursor } else { WX_LIVE_MAX_CURSOR },
            count.clamp(1, 100),
            false
        ],
    });
    websocket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            list_payload.to_string(),
        ))
        .await
        .map_err(ResponseError::internal)?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let next_frame = tokio::time::timeout(remaining, websocket.next())
            .await
            .map_err(|_| ResponseError::internal_message("wx live list_messages timeout"))?;
        let Some(frame) = next_frame else {
            break;
        };
        let frame = frame.map_err(ResponseError::internal)?;
        match frame {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                let payload: Value =
                    serde_json::from_str(&text).map_err(ResponseError::internal)?;
                if payload
                    .get("headers")
                    .and_then(Value::as_object)
                    .and_then(|headers| headers.get("mid"))
                    .and_then(Value::as_str)
                    == Some(list_mid.as_str())
                {
                    let body = payload
                        .get("body")
                        .cloned()
                        .or_else(|| payload.get("data").cloned())
                        .unwrap_or_else(|| json!({}));
                    return Ok(json!({
                        "data": body,
                        "ret": ["SUCCESS::WS拉取成功"],
                    }));
                }
            }
            tokio_tungstenite::tungstenite::Message::Binary(binary) => {
                if let Ok(text) = String::from_utf8(binary.to_vec()) {
                    if let Ok(payload) = serde_json::from_str::<Value>(&text) {
                        if payload
                            .get("headers")
                            .and_then(Value::as_object)
                            .and_then(|headers| headers.get("mid"))
                            .and_then(Value::as_str)
                            == Some(list_mid.as_str())
                        {
                            let body = payload
                                .get("body")
                                .cloned()
                                .or_else(|| payload.get("data").cloned())
                                .unwrap_or_else(|| json!({}));
                            return Ok(json!({
                                "data": body,
                                "ret": ["SUCCESS::WS拉取成功"],
                            }));
                        }
                    }
                }
            }
            tokio_tungstenite::tungstenite::Message::Close(_) => break,
            _ => {}
        }
    }

    Err(ResponseError::internal_message(
        "wx live list_messages did not return body",
    ))
}

/// Python parity: `xianyu_wx_live_client.py:881-913 list_messages` —
/// fetches message history via the **active** WebSocket (not a fresh
/// short-lived one) by registering an oneshot waiter on `(uid, mid)` and
/// pushing `/r/MessageManager/listUserMessages` through `live_senders()`.
///
/// Why this exists alongside `fetch_message_history_via_ws`:
///   * The old impl opened a brand-new TLS+WS+/reg+/ackDiff for every
///     history fetch (~1.5-2s extra latency + an extra `mtop.wx.login.token`
///     call that isn't needed). Each fresh socket also competes with the
///     primary worker socket for the same device_id sticky route on the
///     ACCS server, eventually getting flagged as "multi-endpoint drift".
///   * Reusing the active socket eliminates both costs: the worker has
///     already done /reg+/ackDiff, server only sees one session per uid,
///     and the response comes back in <500ms typically.
///
/// Caller must check `state.live_senders().get(uid)` ahead of time;
/// returns 503 if no active socket. 8s timeout matches Python.
pub(crate) async fn fetch_message_history_via_active_socket(
    state: &AppState,
    uid: &str,
    session_id: &str,
    count: i64,
    cursor: i64,
) -> Result<Value, ResponseError> {
    let sender = state.live_senders().get(uid).await.ok_or_else(|| {
        ResponseError::service_unavailable(
            "live socket for this uid is not connected; falling back is caller's choice",
        )
    })?;

    let mid = generate_live_mid();
    let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register_with_kind(uid, &mid, tx, LiveResponseWaiterKind::ListMessages)
        .await
        .map_err(|_| ResponseError::service_unavailable("消息历史请求标识已被占用，请重试"))?;

    // body[0] = session cid (with @goofish suffix), body[1] = false (??),
    // body[2] = cursor (MAX_SAFE_INTEGER == 9007199254740991 means "from
    // newest"), body[3] = page size, body[4] = false (?). Matches Python
    // `xianyu_wx_live_client.py:893-906` byte-for-byte.
    let cursor_value = if cursor > 0 {
        cursor
    } else {
        WX_LIVE_MAX_CURSOR
    };
    let count_clamped = count.clamp(1, 100);
    let cid = format!("{session_id}@goofish");
    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp": "/r/MessageManager/listUserMessages", "headers": {{"mid": "{mid}"}}, "body": ["#,
            r#""{cid}", false, {cursor}, {count}, false"#,
            r#"]}}"#,
        ),
        mid = escape_json_string(&mid),
        cid = escape_json_string(&cid),
        cursor = cursor_value,
        count = count_clamped,
    );

    if sender.send(ws_payload_str).is_err() {
        // Cleanup orphan waiter — frame dispatcher will never wake it now.
        let _ = state.live_response_waiters().take(uid, &mid).await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending list_messages",
        ));
    }

    // Python uses 8s timeout (`timeout: float = 8.0`).
    let resp = match tokio::time::timeout(Duration::from_secs(8), rx).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(_)) => {
            let _ = state.live_response_waiters().take(uid, &mid).await;
            return Err(ResponseError::internal_message(
                "live socket dropped before list_messages response",
            ));
        }
        Err(_) => {
            let _ = state.live_response_waiters().take(uid, &mid).await;
            return Err(ResponseError::internal_message(
                "wx live list_messages timeout",
            ));
        }
    };

    // Wrapper preserves the same envelope shape as
    // `fetch_message_history_via_ws` so downstream callers don't care
    // which transport produced the response.
    let body = resp
        .get("body")
        .cloned()
        .or_else(|| resp.get("data").cloned())
        .unwrap_or_else(|| json!({}));
    Ok(json!({
        "data": body,
        "ret": ["SUCCESS::WS拉取成功"],
    }))
}
