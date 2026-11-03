use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use std::time::Duration;

use crate::{
    routes::accounts::{
        generate_live_mid,
        runtime_access::{ensure_account_can_call_upstream, load_account_owner_username},
    },
    state::AppState,
};

use super::live_sender_unavailable_detail;
use super::protocol::{
    escape_json_string_inline, generate_uuid4_like_local, unix_timestamp_millis,
};

/// Push a `/r/MessageSend/sendByReceiverScope` payload to `uid`'s active
/// live socket. Mirrors Python `xianyu_wx_live_client.py:705-760 send_text`
/// (and `:762-825 send_image` for `custom_type=2`) byte-for-byte. Replaces
/// the previous `send_live_payload` which opened a new WS + did /reg every
/// HTTP send — that was the #1 cause of "secondary device" demotion on the
/// goofish side, silencing live push to the primary worker.
pub(super) async fn send_live_via_active_socket(
    state: &AppState,
    uid: &str,
    session_id: &str,
    to_uid: &str,
    custom_type: i64,
    encoded_inner: &str,
    text_send: bool,
) -> Result<Value, String> {
    if session_id.trim().is_empty() {
        return Err("missing sessionId".to_string());
    }
    if to_uid.trim().is_empty() {
        return Err("missing toUid".to_string());
    }
    let owner_username = load_account_owner_username(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    ensure_account_can_call_upstream(state, owner_username.as_deref(), uid)
        .await
        .map_err(|error| error.to_string())?;

    let sender = match state
        .live_senders()
        .wait_alive(uid, Duration::from_secs(5))
        .await
    {
        Some(sender) => sender,
        None => return Err(live_sender_unavailable_detail(state, uid).await),
    };

    // Mirror the hand-built JSON formatting from `messages::pc_send_message`
    // — server is byte-strict on this RPC's framing.
    let message_uuid = generate_uuid4_like_local();
    let ws_mid = generate_live_mid();
    let data_cid = format!("{session_id}@goofish");
    let recv_to = format!("{to_uid}@goofish");
    let recv_self = format!("{uid}@goofish");
    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp": "/r/MessageSend/sendByReceiverScope", "headers": {{"mid": "{mid}"}}, "body": ["#,
            r#"{{"uuid": "{uuid}", "cid": "{cid}", "conversationType": 1, "#,
            r#""content": {{"contentType": 101, "custom": {{"type": {ct}, "data": "{data}"}}}}, "#,
            r#""redPointPolicy": 0, "extension": {{"extJson": "{{}}"}}, "#,
            r#""ctx": {{"appVersion": "1.0", "platform": "wx"}}, "#,
            r#""mtags": {{}}, "msgReadStatusSetting": 1}}, "#,
            r#"{{"actualReceivers": ["{r1}", "{r2}"]}}]}}"#,
        ),
        mid = escape_json_string_inline(&ws_mid),
        uuid = escape_json_string_inline(&message_uuid),
        cid = escape_json_string_inline(&data_cid),
        ct = custom_type,
        data = escape_json_string_inline(encoded_inner),
        r1 = escape_json_string_inline(&recv_to),
        r2 = escape_json_string_inline(&recv_self),
    );

    if sender.send(ws_payload_str).is_err() {
        // Race: tx alive when we read it but rx dropped before send.
        state
            .live_senders()
            .unregister_if_matches(uid, &sender)
            .await;
        return Err("live socket dropped while sending".to_string());
    }

    // Python parity: fire-and-forget, no waiting for ack. Optimistic SUCCESS.
    Ok(json!({
        "ret": [if text_send { "SUCCESS::实时发送成功" } else { "SUCCESS::图片发送成功" }],
        "data": {
            "messageId": message_uuid,
            "ts": unix_timestamp_millis(),
        }
    }))
}

pub(super) async fn send_live_text_message(
    state: &AppState,
    uid: &str,
    session_id: &str,
    to_uid: &str,
    content: &str,
) -> Result<Value, String> {
    if content.trim().is_empty() {
        return Err("missing content".to_string());
    }
    // Hand-built tight-separator inner JSON (Python parity:
    // `json.dumps(..., separators=(",", ":"), ensure_ascii=False)`).
    // Avoids `serde_json` escaping non-ASCII → \uXXXX which would not
    // match Python's bytes for Chinese content.
    let inner = format!(
        r#"{{"contentType":1,"text":{{"text":"{}"}}}}"#,
        escape_json_string_inline(content)
    );
    let encoded = BASE64_STANDARD.encode(inner.as_bytes());
    send_live_via_active_socket(state, uid, session_id, to_uid, 1, &encoded, true).await
}

pub(super) async fn send_live_image_message(
    state: &AppState,
    uid: &str,
    session_id: &str,
    to_uid: &str,
    image_url: &str,
    width: i64,
    height: i64,
) -> Result<Value, String> {
    if image_url.trim().is_empty() {
        return Err("missing imageUrl".to_string());
    }
    let w = if width > 0 { width } else { 400 };
    let h = if height > 0 { height } else { 400 };
    // Same hand-built tight inner JSON (image_url is ASCII; safe with
    // serde_json, but use format! for consistency with text path).
    let inner = format!(
        r#"{{"contentType":2,"image":{{"pics":[{{"type":0,"url":"{}","width":{},"height":{}}}]}}}}"#,
        escape_json_string_inline(image_url),
        w,
        h,
    );
    let encoded = BASE64_STANDARD.encode(inner.as_bytes());
    send_live_via_active_socket(state, uid, session_id, to_uid, 2, &encoded, false).await
}

// P-4/P-5 fix: `send_live_payload` and `register_live_socket` removed —
// they opened a new short-lived WS + ran /reg + /ackDiff per HTTP send,
// causing goofish to demote the primary live worker to "secondary device"
// and silence push delivery. All sends now go through the worker's active
// socket via `send_live_via_active_socket` (Python parity).
