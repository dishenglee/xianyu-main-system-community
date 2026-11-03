//! Live ACCS socket operations — send text, create chat, send image.
//! These handlers require an active live WebSocket for `uid` (503 otherwise).

use axum::extract::{Extension, State};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Map, Value};
use tracing::{info, warn};

use crate::models::auth::RequestIdentity;
use crate::routes::{
    accounts::runtime_access::{ensure_account_can_call_upstream, load_account_owner_username},
    live::wait_for_live_sender,
    operations::{append_operation_log, bump_message_daily_stat},
    tenant::{ensure_account_uid_accessible, owner_username_filter},
};
use crate::state::AppState;
use crate::telemetry::IncidentInput;

use super::{
    ensure_message_item_belongs_to_uid, ensure_message_session_belongs_to_uid, json_string,
    load_single_session_info, persist_message_history_page, MessageHistoryEntry,
    MessageHistoryPage, MessageSessionIdentity, ResponseError,
};

// ---------------------------------------------------------------------------
// POST /api/messages/pc/send
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct PcSendPayload {
    pub uid: String,
    pub session_id: String,
    pub to_uid: String,
    pub content: String,
    // 引用回复（quote-reply）：reply_to_message_id 必须是 PNM 格式
    // （"<seq>.PNM"），跟撤回 API 一样从被引用消息的 `upstreamMessageId` 拿。
    // reply_to_text 是发送时显示给用户看的引用文本（preview 或裁剪后的原文），
    // 我们这边一起 stash 到 message_history_cache.reply_to_text，避免接收端
    // 收到推送后还要回扫历史才能渲染气泡。
    #[serde(default)]
    pub reply_to_message_id: String,
    #[serde(default)]
    pub reply_to_text: String,
}

async fn report_message_incident(
    state: &AppState,
    action: &str,
    route: &str,
    error_code: &str,
    error_message: String,
    http_status: Option<u16>,
    retryable: bool,
    context_json: Value,
) {
    let _ = state
        .incident_reporter()
        .report_from_state(
            state,
            IncidentInput {
                error_level: if retryable {
                    "warn".to_string()
                } else {
                    "error".to_string()
                },
                module: "message".to_string(),
                action: action.to_string(),
                route: route.to_string(),
                error_code: error_code.to_string(),
                error_message,
                http_status,
                upstream_ret: String::new(),
                retryable,
                context_json,
                environment: None,
            },
        )
        .await;
}

pub(super) fn extract_send_ack_message_id(resp: &Value) -> Option<String> {
    let pnm = resp
        .get("body")
        .and_then(|b| b.get("messageId"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if pnm.is_empty() {
        None
    } else {
        Some(pnm.to_string())
    }
}

pub async fn pc_send_message(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<PcSendPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let uid = payload.uid.trim();
    let session_id = payload.session_id.trim();
    let to_uid = payload.to_uid.trim();
    let content = payload.content.trim();
    let reply_to_message_id = payload.reply_to_message_id.trim().to_string();
    let reply_to_text = payload.reply_to_text.trim().to_string();
    if uid.is_empty() || session_id.is_empty() || to_uid.is_empty() || content.is_empty() {
        return Err(ResponseError::bad_request(
            "uid / session_id / to_uid / content required",
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid).await?;
    let account_owner = load_account_owner_username(&state, uid).await?;
    ensure_account_can_call_upstream(&state, account_owner.as_deref(), uid).await?;
    ensure_message_session_belongs_to_uid(&state, uid, session_id).await?;

    // `get_alive` skips stale entries (worker disconnected but the
    // `SenderGuard`'s async cleanup hasn't fired yet) so we don't return a
    // sender whose receiver was already dropped — that would lead to a
    // ws.send `Broken pipe` error in the worker loop, which is harmless but
    // pollutes logs and forces the caller to retry. By rejecting up front
    // we get a clean 503 the user can retry on, with no spurious worker
    // shutdown (the worker's natural reconnect cycle handles this).
    let sender = wait_for_live_sender(&state, uid).await?;

    // Mirror Python: inner `{contentType:1, text:{text:...}}` base64-encoded and
    // wrapped inside a contentType=101 "custom" envelope.
    let inner = json!({ "contentType": 1, "text": { "text": content } });
    let encoded =
        BASE64_STANDARD.encode(serde_json::to_string(&inner).unwrap_or_default().as_bytes());
    // MID + UUID 要 match Python 原版格式，否则服务端会 silent drop：
    //   mid  = "{rand_3digit}{ms_ts} 0"   (带空格 + 尾缀 0)
    //   uuid = 标准 UUID4 带连字符
    let message_uuid = generate_uuid4_like();
    let ws_mid = generate_accs_mid();

    // Register a response waiter BEFORE pushing the WS frame. 服务端把 send
    // ACK 通过同一个 mid 回送（{"code":200,"body":{messageId:"<PNM>", uuid:"<our uuid>"}}），
    // 这条 ACK 里有撤回需要的 PNM messageId。worker.rs 的帧分发器会按 mid 唤醒
    // 我们这边的 oneshot。注册必须在 send 之前，否则可能出现 ACK 先到、waiter
    // 还没注册的竞态（虽然 ACCS 通常有几十毫秒延迟，但稳妥起见）。
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register_send(uid, &ws_mid, &message_uuid, ack_tx)
        .await
        .map_err(|_| ResponseError::service_unavailable("消息发送请求标识已被占用，请重试"))?;

    // serde_json 的 `json!` 宏底层是 BTreeMap，会按 key 字母序输出，把
    // `lwp/headers/body` 变成 `body/headers/lwp`。Python 用 dict 插入序
    // 输出 `lwp` 在最前，服务端 ACK 200；Rust 乱序被 silent drop。
    // 手拼 JSON 字符串强制保序匹配 Python `send_text`。
    let data_cid = format!("{session_id}@goofish");
    let recv_to = format!("{to_uid}@goofish");
    let recv_self = format!("{uid}@goofish");
    let esc_mid = escape_json_string(&ws_mid);
    let esc_uuid = escape_json_string(&message_uuid);
    let esc_cid = escape_json_string(&data_cid);
    let esc_data = escape_json_string(&encoded);
    let esc_r1 = escape_json_string(&recv_to);
    let esc_r2 = escape_json_string(&recv_self);
    // extJson 必须是字符串化 JSON（"{}" 或 "{\"replyMessageId\":\"<PNM>\"}"），
    // 闲鱼 WS 校验严格——直接塞对象会被 silent drop。引用时只往 extJson 里塞
    // replyMessageId，跟抓包（/tmp/ws-capture/ws_frames.jsonl）里的发送帧一致。
    let ext_json_inner = if reply_to_message_id.is_empty() {
        "{}".to_string()
    } else {
        format!(
            r#"{{\"replyMessageId\":\"{}\"}}"#,
            escape_json_string(&reply_to_message_id),
        )
    };
    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp":"/r/MessageSend/sendByReceiverScope","headers":{{"mid":"{mid}"}},"body":["#,
            r#"{{"uuid":"{uuid}","cid":"{cid}","conversationType":1,"#,
            r#""content":{{"contentType":101,"custom":{{"type":1,"data":"{data}"}}}},"#,
            r#""redPointPolicy":0,"extension":{{"extJson":"{ext}"}},"#,
            r#""ctx":{{"appVersion":"1.0","platform":"wx"}},"#,
            r#""mtags":{{}},"msgReadStatusSetting":1}},"#,
            r#"{{"actualReceivers":["{r1}","{r2}"]}}]}}"#,
        ),
        mid = esc_mid,
        uuid = esc_uuid,
        cid = esc_cid,
        data = esc_data,
        ext = ext_json_inner,
        r1 = esc_r1,
        r2 = esc_r2,
    );
    info!(
        target: "messages.live_ops",
        stream = "runtime",
        module = "messages-live",
        action = "send_text_frame_ready",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        detail = %format!(
            "to={} bytes={} preview={}",
            to_uid,
            ws_payload_str.len(),
            &ws_payload_str[..ws_payload_str.len().min(300)]
        ),
        "prepared text message websocket payload"
    );
    if sender.send(ws_payload_str).is_err() {
        // Race: tx was alive when we read it but rx got dropped before our
        // send. Clean up the stale entry so subsequent calls don't repeat
        // the dance. Also drop the waiter we just registered — nobody will
        // wake it now.
        let _ = state.live_response_waiters().take(uid, &ws_mid).await;
        state
            .live_senders()
            .unregister_if_matches(uid, &sender)
            .await;
        report_message_incident(
            &state,
            "manual_send_text",
            crate::sealed_protocol::sealed_str!("/api/messages/pc/send"),
            "live_socket_dropped",
            "live socket dropped while sending".to_string(),
            Some(503),
            true,
            json!({
                "uid": uid,
                "sessionId": session_id,
                "toUid": to_uid,
                "wsMid": ws_mid,
                "messageId": message_uuid,
                "replyToMessageId": reply_to_message_id,
            }),
        )
        .await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending",
        ));
    }

    let ts_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    // 等服务端 send-ACK（带 PNM messageId）。实际抓包显示 ACK 通常在 100-300ms
    // 内到，超过 2s 几乎肯定丢了。timeout 控制在 2s 是为了「发完立刻撤回」这种
    // 操作能拿到 PNM —— 没 PNM 撤回 API 必返 message-not-exist。即使超时，下面
    // 的 persist 还是会落 optimistic 行（PNM 留空），让消息可见，只是撤回不行。
    let upstream_message_id = match tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx)
        .await
    {
        Ok(Ok(resp)) => {
            let Some(pnm) = extract_send_ack_message_id(&resp) else {
                warn!(
                    target: "messages.live_ops",
                    stream = "runtime",
                    module = "messages-live",
                    action = "send_text_ack_missing_message_id",
                    outcome = "warn",
                    account_uid = %uid,
                    session_id = %session_id,
                    error_code = "ack_missing_message_id",
                    retryable = true,
                    detail = %serde_json::to_string(&resp).unwrap_or_default(),
                    "text message ACK missing PNM messageId"
                );
                report_message_incident(
                    &state,
                    "manual_send_text",
                    crate::sealed_protocol::sealed_str!("/api/messages/pc/send"),
                    "ack_missing_message_id",
                    "send ACK missing upstream message id".to_string(),
                    Some(504),
                    true,
                    json!({
                        "uid": uid,
                        "sessionId": session_id,
                        "toUid": to_uid,
                        "wsMid": ws_mid,
                        "messageId": message_uuid,
                        "replyToMessageId": reply_to_message_id,
                        "ack": resp,
                    }),
                )
                .await;
                return Err(ResponseError::service_unavailable(
                    "消息发送未收到闲鱼确认，请重试",
                ));
            };
            info!(
                target: "messages.live_ops",
                stream = "runtime",
                module = "messages-live",
                action = "send_text_ack_ok",
                outcome = "success",
                account_uid = %uid,
                session_id = %session_id,
                detail = %pnm,
                "text message ACK received"
            );
            pnm
        }
        Ok(Err(_)) => {
            warn!(target: "messages.live_ops", stream = "runtime", module = "messages-live", action = "send_text_ack_waiter_dropped", outcome = "warn", account_uid = %uid, session_id = %session_id, error_code = "ack_waiter_dropped", retryable = true, "send ACK waiter dropped");
            report_message_incident(
                &state,
                "manual_send_text",
                crate::sealed_protocol::sealed_str!("/api/messages/pc/send"),
                "ack_waiter_dropped",
                "send ACK waiter dropped".to_string(),
                Some(504),
                true,
                json!({
                    "uid": uid,
                    "sessionId": session_id,
                    "toUid": to_uid,
                    "wsMid": ws_mid,
                    "messageId": message_uuid,
                    "replyToMessageId": reply_to_message_id,
                }),
            )
            .await;
            return Err(ResponseError::service_unavailable(
                "消息发送未收到闲鱼确认，请重试",
            ));
        }
        Err(_) => {
            // Drop the orphan waiter slot so the map doesn't leak.
            let _ = state.live_response_waiters().take(uid, &ws_mid).await;
            warn!(target: "messages.live_ops", stream = "runtime", module = "messages-live", action = "send_text_ack_timeout", outcome = "warn", account_uid = %uid, session_id = %session_id, error_code = "ack_timeout", retryable = true, "send ACK timeout after 2s");
            report_message_incident(
                &state,
                "manual_send_text",
                crate::sealed_protocol::sealed_str!("/api/messages/pc/send"),
                "ack_timeout",
                "send ACK timeout after 2s".to_string(),
                Some(504),
                true,
                json!({
                    "uid": uid,
                    "sessionId": session_id,
                    "toUid": to_uid,
                    "wsMid": ws_mid,
                    "messageId": message_uuid,
                    "replyToMessageId": reply_to_message_id,
                }),
            )
            .await;
            return Err(ResponseError::service_unavailable(
                "消息发送未收到闲鱼确认，请重试",
            ));
        }
    };

    // Persist the sent message locally so it shows up immediately in
    // cached reads. The ACCS WebSocket does NOT echo self-sent messages
    // back to the sender, so without this persist the message would be
    // invisible until the user triggers an HTTP history refresh.
    let mut session_identity =
        load_single_session_info(state.storage().live_state(), uid, session_id, 1)
            .await
            .ok()
            .flatten()
            .and_then(|val| {
                let obj = val.as_object()?;
                Some(MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version: ts_millis,
                    peer_uid: json_string(obj.get("peerUid")),
                    peer_nick: json_string(obj.get("peerNick")),
                    peer_logo: json_string(obj.get("peerLogo")),
                    peer_role: json_string(obj.get("peerRole")),
                    self_role: json_string(obj.get("selfRole")),
                    item_id: json_string(obj.get("itemId")),
                    item_title: json_string(obj.get("itemTitle")),
                    item_pic: json_string(obj.get("itemPic")),
                    summary: content.to_string(),
                    ts: ts_millis,
                    sort_index: ts_millis,
                    ..Default::default()
                })
            })
            .unwrap_or_else(|| MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: ts_millis,
                peer_uid: to_uid.to_string(),
                summary: content.to_string(),
                ts: ts_millis,
                sort_index: ts_millis,
                ..Default::default()
            });
    session_identity.latest_message_id = message_uuid.clone();

    let page = MessageHistoryPage {
        session: session_identity,
        has_more: false,
        next_cursor: 0,
        latest_cursor: ts_millis,
        messages: vec![MessageHistoryEntry {
            message_id: message_uuid.clone(),
            upstream_message_id: upstream_message_id.clone(),
            version: ts_millis,
            ts: ts_millis,
            kind: "text".to_string(),
            text: content.to_string(),
            is_self: true,
            sender_nick: String::new(),
            // 引用回复 stash —— 落本地 cache 后，UI 刷新就能直接渲染 quote 块。
            reply_to_message_id: reply_to_message_id.clone(),
            reply_to_text: reply_to_text.clone(),
            ..Default::default()
        }],
    };
    if let Err(e) =
        persist_message_history_page(state.storage().live_state(), uid, &page, true).await
    {
        tracing::warn!(uid, session_id, error = %e, "failed to persist self-sent message");
    }

    bump_message_daily_stat(&state, uid, "manual_reply_count", 1).await;
    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "message",
            action: "manual_send_text",
            operator: uid,
            trigger_source: "manual",
            uid,
            account_nick: "",
            target_type: "session",
            target_id: session_id,
            target_title: to_uid,
            status: "success",
            error_code: "",
            message: "手动发送文本消息成功",
            params: json!({
                "to_uid": to_uid,
                "content": content,
                "reply_to_message_id": reply_to_message_id,
            }),
            before: json!({}),
            after: json!({
                "message_id": message_uuid,
                "upstream_message_id": upstream_message_id,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(axum::Json(json!({
        "ok": true,
        "uid": uid,
        "sessionId": session_id,
        "toUid": to_uid,
        "messageId": message_uuid,
        "upstreamMessageId": upstream_message_id,
        "ts": ts_millis,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/messages/chat/create
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct LiveCreateChatPayload {
    pub uid: String,
    pub to_uid: String,
    #[serde(default)]
    pub item_id: Option<String>,
}

const LIVE_RPC_TIMEOUT_SECS: u64 = 20;

pub async fn live_create_chat(
    axum::extract::State(state): axum::extract::State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<LiveCreateChatPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let uid = payload.uid.trim().to_string();
    let to_uid = payload.to_uid.trim().to_string();
    let item_id = payload.item_id.unwrap_or_default().trim().to_string();
    if uid.is_empty() || to_uid.is_empty() {
        return Err(ResponseError::bad_request("uid / to_uid required"));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    let account_owner = load_account_owner_username(&state, &uid).await?;
    ensure_account_can_call_upstream(&state, account_owner.as_deref(), &uid).await?;
    if !item_id.is_empty() {
        ensure_message_item_belongs_to_uid(&state, &uid, &item_id).await?;
    }

    // `get_alive` skips senders whose channel is already closed (race
    // window after worker WS drop, before SenderGuard clean-up runs).
    let sender = wait_for_live_sender(&state, &uid).await?;

    let mid = generate_accs_mid();
    let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register(&uid, &mid, tx)
        .await
        .map_err(|_| ResponseError::service_unavailable("会话创建请求标识已被占用，请重试"))?;

    let pair_first = format!("{to_uid}@goofish");
    let pair_second = format!("{uid}@goofish");
    let extension_str = if item_id.is_empty() {
        "{}".to_string()
    } else {
        format!(r#"{{"itemId":"{}"}}"#, escape_json_string(&item_id))
    };
    // Hand-built JSON: keep `lwp/headers/body` insertion order + Python-style
    // spaces. Server is byte-strict on this RPC's framing (same as /reg).
    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp": "/r/SingleChatConversation/create", "headers": {{"mid": "{mid}"}}, "body": [{{"#,
            r#""pairFirst": "{pair_first}", "pairSecond": "{pair_second}", "#,
            r#""bizType": "1", "extension": {ext}, "#,
            r#""ctx": {{"appVersion": "1.0", "platform": "wx"}}"#,
            r#"}}]}}"#,
        ),
        mid = escape_json_string(&mid),
        pair_first = escape_json_string(&pair_first),
        pair_second = escape_json_string(&pair_second),
        ext = extension_str,
    );

    if sender.send(ws_payload_str).is_err() {
        // Cleanup orphan waiter — frame dispatcher will never wake it now.
        let _ = state.live_response_waiters().take(&uid, &mid).await;
        // Race: the channel was alive when we read it but rx dropped before
        // our send. Clear the stale entry too.
        state
            .live_senders()
            .unregister_if_matches(&uid, &sender)
            .await;
        report_message_incident(
            &state,
            "manual_create_chat",
            crate::sealed_protocol::sealed_str!("/api/messages/chat/create"),
            "live_socket_dropped",
            "live socket dropped while sending create_chat".to_string(),
            Some(503),
            true,
            json!({
                "uid": uid,
                "toUid": to_uid,
                "itemId": item_id,
                "wsMid": mid,
            }),
        )
        .await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending create_chat",
        ));
    }

    // Python parity: 20s timeout. If it expires we still return ok=true with
    // mid so caller can retry / poll, but cid stays empty.
    let cid = match tokio::time::timeout(std::time::Duration::from_secs(LIVE_RPC_TIMEOUT_SECS), rx)
        .await
    {
        Ok(Ok(resp)) => extract_create_chat_cid(&resp),
        Ok(Err(_)) => {
            // Sender dropped without ever sending — usually means the worker
            // tore down the WS while we were waiting.
            let _ = state.live_response_waiters().take(&uid, &mid).await;
            report_message_incident(
                &state,
                "manual_create_chat",
                crate::sealed_protocol::sealed_str!("/api/messages/chat/create"),
                "ack_waiter_dropped",
                "create_chat ACK waiter dropped".to_string(),
                Some(504),
                true,
                json!({
                    "uid": uid,
                    "toUid": to_uid,
                    "itemId": item_id,
                    "wsMid": mid,
                }),
            )
            .await;
            String::new()
        }
        Err(_) => {
            // Timeout — drop the orphan waiter so we don't leak the slot.
            let _ = state.live_response_waiters().take(&uid, &mid).await;
            warn!(
                target: "messages.live_ops",
                stream = "runtime",
                module = "messages-live",
                action = "create_chat_ack_timeout",
                outcome = "warn",
                account_uid = %uid,
                error_code = "ack_timeout",
                retryable = true,
                "create_chat ACK timeout without CID"
            );
            report_message_incident(
                &state,
                "manual_create_chat",
                crate::sealed_protocol::sealed_str!("/api/messages/chat/create"),
                "ack_timeout",
                format!("create_chat ACK timeout after {}s", LIVE_RPC_TIMEOUT_SECS),
                Some(504),
                true,
                json!({
                    "uid": uid,
                    "toUid": to_uid,
                    "itemId": item_id,
                    "wsMid": mid,
                }),
            )
            .await;
            String::new()
        }
    };

    Ok(axum::Json(json!({
        "ok": true,
        "uid": uid,
        "toUid": to_uid,
        "itemId": item_id,
        "mid": mid,
        "cid": cid,
    })))
}

/// Python parity (`xianyu_live_client.py:1402-1437 _extract_cid` +
/// `_find_cid_in_map`): server may return CID at any of:
///   body.cid                                       (rare)
///   body.singleChatConversation.cid                (most common)
///   body.conversation.cid                          (some flows)
///   body[0].cid / body[0].singleChatConversation.cid / body[0].conversation.cid
/// Strip the `@goofish` suffix if present.
fn extract_create_chat_cid(resp: &Value) -> String {
    let body = resp.get("body");
    if let Some(Value::Object(map)) = body {
        let cid = find_cid_in_map(map);
        if !cid.is_empty() {
            return cid;
        }
    }
    if let Some(Value::Array(items)) = body {
        for item in items {
            if let Some(map) = item.as_object() {
                let cid = find_cid_in_map(map);
                if !cid.is_empty() {
                    return cid;
                }
            }
        }
    }
    String::new()
}

fn find_cid_in_map(m: &Map<String, Value>) -> String {
    let direct = json_string(m.get("cid"));
    if !direct.is_empty() {
        return direct.split('@').next().unwrap_or(&direct).to_string();
    }
    for key in ["singleChatConversation", "conversation"] {
        if let Some(nested) = m.get(key).and_then(Value::as_object) {
            let nested_cid = json_string(nested.get("cid"));
            if !nested_cid.is_empty() {
                return nested_cid
                    .split('@')
                    .next()
                    .unwrap_or(&nested_cid)
                    .to_string();
            }
        }
    }
    String::new()
}

// ---------------------------------------------------------------------------
// POST /api/messages/send/image
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct LiveSendImagePayload {
    pub uid: String,
    pub session_id: String,
    pub to_uid: String,
    pub image_url: String,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
}

pub async fn live_send_image(
    axum::extract::State(state): axum::extract::State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<LiveSendImagePayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let uid = payload.uid.trim().to_string();
    let session_id = payload.session_id.trim().to_string();
    let to_uid = payload.to_uid.trim().to_string();
    let image_url = payload.image_url.trim().to_string();
    if uid.is_empty() || session_id.is_empty() || to_uid.is_empty() || image_url.is_empty() {
        return Err(ResponseError::bad_request(
            "uid / session_id / to_uid / image_url required",
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    let account_owner = load_account_owner_username(&state, &uid).await?;
    ensure_account_can_call_upstream(&state, account_owner.as_deref(), &uid).await?;
    ensure_message_session_belongs_to_uid(&state, &uid, &session_id).await?;
    let width = if payload.width > 0 {
        payload.width
    } else {
        400
    };
    let height = if payload.height > 0 {
        payload.height
    } else {
        400
    };

    // `get_alive` skips senders whose channel is already closed.
    let sender = wait_for_live_sender(&state, &uid).await?;

    let message_uuid = generate_uuid4_like();
    let ws_mid = generate_accs_mid();

    // 注册 ACK waiter —— 跟 pc_send_message 一样，等服务端把 PNM messageId
    // 通过同一 mid 回送，没 PNM 撤回 API 必返 message-not-exist。原版 fire-and-forget
    // 改成等 ACK（最多 2s），就能拿到撤回必需的 upstream_message_id。
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register_send(&uid, &ws_mid, &message_uuid, ack_tx)
        .await
        .map_err(|_| ResponseError::service_unavailable("图片发送请求标识已被占用，请重试"))?;

    // Build the inner image envelope (contentType=2) and base64-encode it.
    // Python's `json.dumps(separators=(",", ":"))` produces a TIGHT (no
    // trailing space) JSON because the *encoded* form matters less than the
    // outer envelope, but to keep wire-compat with Python we mirror its
    // tight separators exactly. The OUTER envelope still uses Python-style
    // `, ` and `: ` spaces (server is byte-strict on the outer wrapper).
    let inner_image = json!({
        "contentType": 2,
        "image": {
            "pics": [{
                "type": 0,
                "url": image_url,
                "width": width,
                "height": height,
            }]
        },
    });
    let encoded = BASE64_STANDARD.encode(
        serde_json::to_string(&inner_image)
            .unwrap_or_default()
            .as_bytes(),
    );

    let data_cid = format!("{session_id}@goofish");
    let recv_to = format!("{to_uid}@goofish");
    let recv_self = format!("{uid}@goofish");
    let esc_mid = escape_json_string(&ws_mid);
    let esc_uuid = escape_json_string(&message_uuid);
    let esc_cid = escape_json_string(&data_cid);
    let esc_data = escape_json_string(&encoded);
    let esc_r1 = escape_json_string(&recv_to);
    let esc_r2 = escape_json_string(&recv_self);
    // Identical structure to `pc_send_message` text path, but `custom.type=2`
    // (image) instead of `1` (text). Field order + spacing mirrors Python
    // `send_image` (`xianyu_wx_live_client.py:789-815`).
    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp": "/r/MessageSend/sendByReceiverScope", "headers": {{"mid": "{mid}"}}, "body": ["#,
            r#"{{"uuid": "{uuid}", "cid": "{cid}", "conversationType": 1, "#,
            r#""content": {{"contentType": 101, "custom": {{"type": 2, "data": "{data}"}}}}, "#,
            r#""redPointPolicy": 0, "extension": {{"extJson": "{{}}"}}, "#,
            r#""ctx": {{"appVersion": "1.0", "platform": "wx"}}, "#,
            r#""mtags": {{}}, "msgReadStatusSetting": 1}}, "#,
            r#"{{"actualReceivers": ["{r1}", "{r2}"]}}]}}"#,
        ),
        mid = esc_mid,
        uuid = esc_uuid,
        cid = esc_cid,
        data = esc_data,
        r1 = esc_r1,
        r2 = esc_r2,
    );
    info!(
        target: "messages.live_ops",
        stream = "runtime",
        module = "messages-live",
        action = "send_image_frame_ready",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        detail = %format!("to={} image_url={} bytes={}", to_uid, image_url, ws_payload_str.len()),
        "prepared image message websocket payload"
    );
    if sender.send(ws_payload_str).is_err() {
        // Race: tx alive → rx dropped before send. Clean up so subsequent
        // calls fast-fail at `get_alive` instead of repeating the dance.
        let _ = state.live_response_waiters().take(&uid, &ws_mid).await;
        state
            .live_senders()
            .unregister_if_matches(&uid, &sender)
            .await;
        report_message_incident(
            &state,
            "manual_send_image",
            crate::sealed_protocol::sealed_str!("/api/messages/send/image"),
            "live_socket_dropped",
            "live socket dropped while sending image".to_string(),
            Some(503),
            true,
            json!({
                "uid": uid,
                "sessionId": session_id,
                "toUid": to_uid,
                "wsMid": ws_mid,
                "messageId": message_uuid,
                "imageUrl": image_url,
                "width": width,
                "height": height,
            }),
        )
        .await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending image",
        ));
    }

    // 等 ACK 拿 PNM —— 2s 超时。拿到的 PNM 立刻返给前端，前端把 optimistic
    // 行 promote 成 (real messageId, PNM)，这样发完立刻就能撤回。
    let upstream_message_id =
        match tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx).await {
            Ok(Ok(resp)) => {
                let Some(pnm) = extract_send_ack_message_id(&resp) else {
                    warn!(
                        target: "messages.live_ops",
                        stream = "runtime",
                        module = "messages-live",
                        action = "send_image_ack_missing_message_id",
                        outcome = "warn",
                        account_uid = %uid,
                        session_id = %session_id,
                        error_code = "ack_missing_message_id",
                        retryable = true,
                        detail = %serde_json::to_string(&resp).unwrap_or_default(),
                        "image message ACK missing PNM messageId"
                    );
                    report_message_incident(
                        &state,
                        "manual_send_image",
                        crate::sealed_protocol::sealed_str!("/api/messages/send/image"),
                        "ack_missing_message_id",
                        "send image ACK missing upstream message id".to_string(),
                        Some(504),
                        true,
                        json!({
                            "uid": uid,
                            "sessionId": session_id,
                            "toUid": to_uid,
                            "wsMid": ws_mid,
                            "messageId": message_uuid,
                            "imageUrl": image_url,
                            "ack": resp,
                        }),
                    )
                    .await;
                    return Err(ResponseError::service_unavailable(
                        "图片发送未收到闲鱼确认，请重试",
                    ));
                };
                pnm
            }
            Ok(Err(_)) => {
                report_message_incident(
                    &state,
                    "manual_send_image",
                    crate::sealed_protocol::sealed_str!("/api/messages/send/image"),
                    "ack_waiter_dropped",
                    "send image ACK waiter dropped".to_string(),
                    Some(504),
                    true,
                    json!({
                        "uid": uid,
                        "sessionId": session_id,
                        "toUid": to_uid,
                        "wsMid": ws_mid,
                        "messageId": message_uuid,
                        "imageUrl": image_url,
                    }),
                )
                .await;
                return Err(ResponseError::service_unavailable(
                    "图片发送未收到闲鱼确认，请重试",
                ));
            }
            Err(_) => {
                let _ = state.live_response_waiters().take(&uid, &ws_mid).await;
                report_message_incident(
                    &state,
                    "manual_send_image",
                    crate::sealed_protocol::sealed_str!("/api/messages/send/image"),
                    "ack_timeout",
                    "send image ACK timeout after 2s".to_string(),
                    Some(504),
                    true,
                    json!({
                        "uid": uid,
                        "sessionId": session_id,
                        "toUid": to_uid,
                        "wsMid": ws_mid,
                        "messageId": message_uuid,
                        "imageUrl": image_url,
                    }),
                )
                .await;
                return Err(ResponseError::service_unavailable(
                    "图片发送未收到闲鱼确认，请重试",
                ));
            }
        };
    info!(
        target: "messages.live_ops",
        stream = "runtime",
        module = "messages-live",
        action = "send_image_ack_ok",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        detail = %upstream_message_id,
        "image message ACK received"
    );

    bump_message_daily_stat(&state, &uid, "manual_reply_count", 1).await;
    bump_message_daily_stat(&state, &uid, "image_reply_count", 1).await;
    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "message",
            action: "manual_send_image",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "session",
            target_id: &session_id,
            target_title: &to_uid,
            status: "success",
            error_code: "",
            message: "手动发送图片消息成功",
            params: json!({
                "to_uid": to_uid,
                "image_url": image_url,
                "width": width,
                "height": height,
            }),
            before: json!({}),
            after: json!({
                "message_id": message_uuid,
                "upstream_message_id": upstream_message_id,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(axum::Json(json!({
        "ok": true,
        "uid": uid,
        "sessionId": session_id,
        "toUid": to_uid,
        "messageId": message_uuid,
        "upstreamMessageId": upstream_message_id,
        "ts": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    })))
}

// ---------------------------------------------------------------------------
// JSON + ACCS utilities used only by live ops
// ---------------------------------------------------------------------------

// JSON 字符串转义——手拼 payload 时保证特殊字符（"/\/\\/控制字符）被正确
// 编码。会话 id、uuid、mid、base64 data 里不会含特殊字符，这里主要防御性。
pub fn escape_json_string(s: &str) -> String {
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

// 生成 UUID4 (`xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`)。必须每次都唯一，
// 否则闲鱼服务端把重复 uuid 当成重放攻击 silent drop。之前的 xorshift 种子
// 对第 9-16 字节不够分散（同一进程内连续发送尾部 8 字节相同）。
// 使用跨平台 UUID4，避免 Windows 没有 /dev/urandom 时走弱随机 fallback。
pub fn generate_uuid4_like() -> String {
    uuid::Uuid::new_v4().to_string()
}

// 生成 ACCS 消息 mid：`f"{rand}{ms_ts} 0"` — Python 原版格式。带一个
// 空格 + 结尾的 "0" 是 ACCS 的历史约定，服务端会校验。
pub fn generate_accs_mid() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static N: AtomicU32 = AtomicU32::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    // Python 用 `int(1000*random())` 得 0..999 前缀。这里用纳秒尾部 3 位充当。
    let nanos3 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() % 1000) as u32)
        .unwrap_or(0);
    let seq = N.fetch_add(1, Ordering::Relaxed) % 1000;
    let prefix = (nanos3 ^ seq) % 1000;
    format!("{prefix}{ms} 0")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_send_ack_message_id_rejects_missing_pnm() {
        assert_eq!(
            extract_send_ack_message_id(&json!({"code": 200, "body": {}})),
            None
        );
        assert_eq!(
            extract_send_ack_message_id(&json!({"code": 200, "body": {"messageId": "  "}})),
            None
        );
    }

    #[test]
    fn extract_send_ack_message_id_accepts_non_empty_pnm() {
        assert_eq!(
            extract_send_ack_message_id(&json!({"code": 200, "body": {"messageId": "123.PNM"}})),
            Some("123.PNM".to_string())
        );
    }

    #[test]
    fn generate_uuid4_like_is_cross_platform_uuid_v4() {
        let first = uuid::Uuid::parse_str(&generate_uuid4_like()).expect("valid uuid");
        let second = uuid::Uuid::parse_str(&generate_uuid4_like()).expect("valid uuid");
        assert_eq!(first.get_version_num(), 4);
        assert_eq!(second.get_version_num(), 4);
        assert_ne!(first, second);
    }
}
