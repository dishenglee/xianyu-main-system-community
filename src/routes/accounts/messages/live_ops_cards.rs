//! Live ACCS rich-card sends — location (contentType=30) and item (contentType=7).
//!
//! Mirrors `live_ops::pc_send_message` byte-for-byte for the outer envelope:
//! hand-crafted JSON string with strict `lwp/headers/body` key order, Python-
//! style spacing, and base64-encoded inner content. The inner payload uses
//! `serde_json::json!` because it's an unconstrained nested object — server
//! only cares about the outer wire framing.
//!
//! Both endpoints register a `live_response_waiters` oneshot BEFORE pushing the
//! WS frame so the server-generated PNM messageId from the ACK can be stashed
//! in the optimistic `message_history_cache` row. See `pc_send_message` for the
//! 2s timeout rationale.
//!
//! Persistence: rows are written with `kind="card"` + `card_meta` JSON blob so
//! the frontend can render the rich-card UI from cache without needing the
//! upstream history sync.

use axum::extract::{Extension, State};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use tracing::info;

use crate::models::auth::RequestIdentity;
use crate::protocol_catalog::opcodes;
use crate::routes::{
    accounts::runtime_access::{ensure_account_can_call_upstream, load_account_owner_username},
    live::wait_for_live_sender,
    operations::{append_operation_log, bump_message_daily_stat},
    tenant::{ensure_account_uid_accessible, owner_username_filter},
};
use crate::state::AppState;
use crate::telemetry::IncidentInput;

use super::live_ops::{
    escape_json_string, extract_send_ack_message_id, generate_accs_mid, generate_uuid4_like,
};
use super::{
    coerce_mtop_data, ensure_message_item_belongs_to_uid, ensure_message_session_belongs_to_uid,
    json_array, json_bool, json_i64, json_string, load_single_session_info,
    persist_message_history_page, successful_message_protocol_h5_request, MessageHistoryEntry,
    MessageHistoryPage, MessageSessionIdentity, ResponseError,
};

// ---------------------------------------------------------------------------
// Shared envelope builder
// ---------------------------------------------------------------------------

/// Build the outer `/r/MessageSend/sendByReceiverScope` envelope used by every
/// custom-content send path (text/image/location/item). Identical structure to
/// `pc_send_message`'s text path; only `custom.type` and `encoded` differ.
///
/// `custom_type`:
///   1 — text
///   2 — image
///   3 — location/dxCard (verified from WS capture)
/// `encoded` is base64 of the inner content JSON.
/// `ext_json_inner` is the already-stringified extJson body (`"{}"` or
/// `r#"{\"replyMessageId\":\"<PNM>\"}"#`). Caller is responsible for escape.
fn build_send_envelope(
    ws_mid: &str,
    message_uuid: &str,
    session_id: &str,
    to_uid: &str,
    self_uid: &str,
    custom_type: u32,
    encoded: &str,
    ext_json_inner: &str,
) -> String {
    let data_cid = format!("{session_id}@goofish");
    let recv_to = format!("{to_uid}@goofish");
    let recv_self = format!("{self_uid}@goofish");
    let esc_mid = escape_json_string(ws_mid);
    let esc_uuid = escape_json_string(message_uuid);
    let esc_cid = escape_json_string(&data_cid);
    let esc_data = escape_json_string(encoded);
    let esc_r1 = escape_json_string(&recv_to);
    let esc_r2 = escape_json_string(&recv_self);
    format!(
        concat!(
            r#"{{"lwp":"/r/MessageSend/sendByReceiverScope","headers":{{"mid":"{mid}"}},"body":["#,
            r#"{{"uuid":"{uuid}","cid":"{cid}","conversationType":1,"#,
            r#""content":{{"contentType":101,"custom":{{"type":{ctype},"data":"{data}"}}}},"#,
            r#""redPointPolicy":0,"extension":{{"extJson":"{ext}"}},"#,
            r#""ctx":{{"appVersion":"1.0","platform":"wx"}},"#,
            r#""mtags":{{}},"msgReadStatusSetting":1}},"#,
            r#"{{"actualReceivers":["{r1}","{r2}"]}}]}}"#,
        ),
        mid = esc_mid,
        uuid = esc_uuid,
        cid = esc_cid,
        ctype = custom_type,
        data = esc_data,
        ext = ext_json_inner,
        r1 = esc_r1,
        r2 = esc_r2,
    )
}

/// Build the `extJson` string for either an empty (`"{}"`) or a quote-reply
/// (`r#"{\"replyMessageId\":\"<PNM>\"}"#`) envelope. Mirrors `pc_send_message`.
fn build_ext_json_inner(reply_to_message_id: &str) -> String {
    if reply_to_message_id.is_empty() {
        "{}".to_string()
    } else {
        format!(
            r#"{{\"replyMessageId\":\"{}\"}}"#,
            escape_json_string(reply_to_message_id),
        )
    }
}

/// Hydrate session identity from existing cache (preserves peerNick/itemTitle/
/// itemPic etc.) and bump version/summary to the current send. Used by both
/// location + item paths after a successful send so optimistic rows show up
/// in the chat list with full identity data.
async fn build_session_identity_from_cache(
    state: &AppState,
    uid: &str,
    session_id: &str,
    to_uid: &str,
    summary: String,
    ts_millis: i64,
) -> MessageSessionIdentity {
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
                summary: summary.clone(),
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
            summary,
            ts: ts_millis,
            sort_index: ts_millis,
            ..Default::default()
        })
}

/// Wait for the send-ACK and extract the server-generated PNM messageId. Same
/// 2s budget + cleanup-on-timeout semantics as `pc_send_message`.
async fn await_send_ack(
    state: &AppState,
    action: &str,
    route: &str,
    send_kind: &str,
    uid: &str,
    session_id: &str,
    ws_mid: &str,
    ack_rx: tokio::sync::oneshot::Receiver<Value>,
) -> Result<String, ResponseError> {
    match tokio::time::timeout(std::time::Duration::from_secs(2), ack_rx).await {
        Ok(Ok(resp)) => match extract_send_ack_message_id(&resp) {
            Some(pnm) => Ok(pnm),
            None => {
                tracing::warn!(
                    uid,
                    session_id,
                    ack = %serde_json::to_string(&resp).unwrap_or_default(),
                    "send ACK missing PNM messageId"
                );
                let _ = state
                    .incident_reporter()
                    .report_from_state(
                        state,
                        IncidentInput {
                            error_level: "warn".to_string(),
                            module: "message".to_string(),
                            action: action.to_string(),
                            route: route.to_string(),
                            error_code: "ack_missing_message_id".to_string(),
                            error_message: "send ACK missing upstream message id".to_string(),
                            http_status: Some(504),
                            upstream_ret: String::new(),
                            retryable: true,
                            context_json: json!({
                                "sendKind": send_kind,
                                "uid": uid,
                                "sessionId": session_id,
                                "wsMid": ws_mid,
                                "ack": resp,
                            }),
                            environment: None,
                        },
                    )
                    .await;
                Err(ResponseError::service_unavailable(
                    "卡片消息发送未收到闲鱼确认，请重试",
                ))
            }
        },
        Ok(Err(_)) => {
            tracing::warn!(uid, session_id, "send ACK waiter dropped");
            let _ = state
                .incident_reporter()
                .report_from_state(
                    state,
                    IncidentInput {
                        error_level: "warn".to_string(),
                        module: "message".to_string(),
                        action: action.to_string(),
                        route: route.to_string(),
                        error_code: "ack_waiter_dropped".to_string(),
                        error_message: "send ACK waiter dropped".to_string(),
                        http_status: Some(504),
                        upstream_ret: String::new(),
                        retryable: true,
                        context_json: json!({
                            "sendKind": send_kind,
                            "uid": uid,
                            "sessionId": session_id,
                            "wsMid": ws_mid,
                        }),
                        environment: None,
                    },
                )
                .await;
            Err(ResponseError::service_unavailable(
                "卡片消息发送未收到闲鱼确认，请重试",
            ))
        }
        Err(_) => {
            let _ = state.live_response_waiters().take(uid, ws_mid).await;
            tracing::warn!(uid, session_id, "send ACK timeout 2s");
            let _ = state
                .incident_reporter()
                .report_from_state(
                    state,
                    IncidentInput {
                        error_level: "warn".to_string(),
                        module: "message".to_string(),
                        action: action.to_string(),
                        route: route.to_string(),
                        error_code: "ack_timeout".to_string(),
                        error_message: "send ACK timeout after 2s".to_string(),
                        http_status: Some(504),
                        upstream_ret: String::new(),
                        retryable: true,
                        context_json: json!({
                            "sendKind": send_kind,
                            "uid": uid,
                            "sessionId": session_id,
                            "wsMid": ws_mid,
                        }),
                        environment: None,
                    },
                )
                .await;
            Err(ResponseError::service_unavailable(
                "卡片消息发送未收到闲鱼确认，请重试",
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/messages/send/location  (contentType=30 locationCard)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct LiveSendLocationPayload {
    pub uid: String,
    pub session_id: String,
    pub to_uid: String,
    pub latitude: String,  // "28.193512"
    pub longitude: String, // "113.010909"
    pub title: String,     // 主标题，例如 "韵达快递(长沙大学店)"
    pub content: String,   // 副标题/详细地址
    // 引用回复：跟 pc_send_message 一致——reply_to_message_id 是被引用消息的
    // PNM。空 → 普通发送，extJson="{}"。
    #[serde(default)]
    pub reply_to_message_id: String,
    #[serde(default)]
    pub reply_to_text: String,
}

/// Static preview thumbnail used by PC web for every location card — reused
/// verbatim so cards render identically regardless of which client sent them.
const LOCATION_PREVIEW_THUMB: &str =
    "https://img.alicdn.com/imgextra/i4/O1CN01t5Yxwi1vA3LNVBAWU_!!6000000006131-2-tps-300-300.png";

pub async fn live_send_location(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<LiveSendLocationPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let uid = payload.uid.trim().to_string();
    let session_id = payload.session_id.trim().to_string();
    let to_uid = payload.to_uid.trim().to_string();
    let latitude = payload.latitude.trim().to_string();
    let longitude = payload.longitude.trim().to_string();
    let title = payload.title.trim().to_string();
    let content = payload.content.trim().to_string();
    let reply_to_message_id = payload.reply_to_message_id.trim().to_string();
    let reply_to_text = payload.reply_to_text.trim().to_string();

    if uid.is_empty()
        || session_id.is_empty()
        || to_uid.is_empty()
        || latitude.is_empty()
        || longitude.is_empty()
        || title.is_empty()
    {
        return Err(ResponseError::bad_request(
            "uid / session_id / to_uid / latitude / longitude / title required",
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    let account_owner = load_account_owner_username(&state, &uid).await?;
    ensure_account_can_call_upstream(&state, account_owner.as_deref(), &uid).await?;
    ensure_message_session_belongs_to_uid(&state, &uid, &session_id).await?;

    // Same `get_alive` semantics as text/image: skip stale entries so the
    // worker doesn't pollute logs with a `Broken pipe`.
    let sender = wait_for_live_sender(&state, &uid).await?;

    // Action URL — Taobao H5 map page used by PC web when tapping the card.
    // URL-encode `title` and `content` (UTF-8 → percent encoding) to match
    // upstream capture; otherwise the preview thumbnail loads but the H5 sheet
    // shows blank labels.
    let action_url = format!(
        "https://market.m.taobao.com/app/idleFish-F2e/fish-base/pages/map-for-im/index.html?role=consumer&latitude={lat}&longitude={lng}&mainTitle={t}&subTitle={c}",
        lat = latitude,
        lng = longitude,
        t = urlencoding::encode(&title),
        c = urlencoding::encode(&content),
    );

    // Inner content envelope (contentType=30). Field order doesn't matter
    // here — only the outer wrapper is byte-strict. Use `serde_json::json!`
    // for readability and automatic UTF-8 / escape correctness.
    // `action.page` mirrors the PC-web capture exactly — the H5 map sheet ignores
    // the extra style flags but they appear in every real send, so keep them so
    // we don't get flagged as a non-stock client.
    let inner = json!({
        "contentType": 30,
        "locationCard": {
            "action": {
                "actionType": 4,
                "page": {
                    "actionStyle": 0,
                    "actionType": 0,
                    "iosActionStyle": 0,
                    "showGuideAlways": false,
                    "url": action_url,
                },
            },
            "content": content,
            "height": 0,
            "latitude": latitude,
            "longitude": longitude,
            "showSender": false,
            "title": title,
            "url": LOCATION_PREVIEW_THUMB,
            "width": 0,
        },
    });
    let encoded =
        BASE64_STANDARD.encode(serde_json::to_string(&inner).unwrap_or_default().as_bytes());

    let message_uuid = generate_uuid4_like();
    let ws_mid = generate_accs_mid();

    // Register ACK waiter BEFORE send — same race-window concern as text path.
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register_send(&uid, &ws_mid, &message_uuid, ack_tx)
        .await
        .map_err(|_| ResponseError::service_unavailable("位置发送请求标识已被占用，请重试"))?;

    let ext_json_inner = build_ext_json_inner(&reply_to_message_id);
    // Location uses custom.type=3 (dxCard family — same byte that 25/26/28
    // upstream cards arrive on). Verified against the captured PC-web send.
    let ws_payload_str = build_send_envelope(
        &ws_mid,
        &message_uuid,
        &session_id,
        &to_uid,
        &uid,
        3,
        &encoded,
        &ext_json_inner,
    );
    info!(
        target: "messages.live_ops",
        stream = "runtime",
        module = "messages-live",
        action = "send_location_frame_ready",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        detail = %format!("to={} title={} bytes={}", to_uid, title, ws_payload_str.len()),
        "prepared location message websocket payload"
    );
    if sender.send(ws_payload_str).is_err() {
        let _ = state.live_response_waiters().take(&uid, &ws_mid).await;
        state
            .live_senders()
            .unregister_if_matches(&uid, &sender)
            .await;
        let _ = state
            .incident_reporter()
            .report_from_state(
                &state,
                IncidentInput {
                    error_level: "warn".to_string(),
                    module: "message".to_string(),
                    action: "manual_send_location".to_string(),
                    route: crate::sealed_protocol::sealed_str!("/api/messages/send/location")
                        .to_string(),
                    error_code: "live_socket_dropped".to_string(),
                    error_message: "live socket dropped while sending location".to_string(),
                    http_status: Some(503),
                    upstream_ret: String::new(),
                    retryable: true,
                    context_json: json!({
                        "sendKind": "location_card",
                        "uid": uid,
                        "sessionId": session_id,
                        "toUid": to_uid,
                        "wsMid": ws_mid,
                        "messageId": message_uuid,
                        "replyToMessageId": reply_to_message_id,
                        "latitude": latitude,
                        "longitude": longitude,
                    }),
                    environment: None,
                },
            )
            .await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending location",
        ));
    }

    let ts_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let upstream_message_id = await_send_ack(
        &state,
        "manual_send_location",
        crate::sealed_protocol::sealed_str!("/api/messages/send/location"),
        "location_card",
        &uid,
        &session_id,
        &ws_mid,
        ack_rx,
    )
    .await?;

    let preview = format!("[位置] {title}");

    // Stash the rendered location card JSON in `card_meta` so the frontend can
    // re-render the card UI from cache without re-fetching anything. Mirrors
    // the shape of the inner `locationCard` payload, plus a `kind` discriminator
    // so the renderer can dispatch on `cardMeta.kind === "location"`.
    let card_meta_value = json!({
        "kind": "location",
        "title": title,
        "content": content,
        "latitude": latitude,
        "longitude": longitude,
        "previewUrl": LOCATION_PREVIEW_THUMB,
        "actionUrl": action_url,
    });
    let card_meta = serde_json::to_string(&card_meta_value).unwrap_or_default();

    let mut session_identity = build_session_identity_from_cache(
        &state,
        &uid,
        &session_id,
        &to_uid,
        preview.clone(),
        ts_millis,
    )
    .await;
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
            kind: "card".to_string(),
            // text = 主标题 — keeps the chat-list summary readable when the
            // session row falls back to `message.text` (some cache paths).
            text: title.clone(),
            // image_url = static preview thumb so the optimistic row's
            // thumbnail matches what the recipient sees.
            image_url: LOCATION_PREVIEW_THUMB.to_string(),
            content_type: 30,
            preview: preview.clone(),
            is_self: true,
            sender_nick: String::new(),
            card_meta,
            reply_to_message_id: reply_to_message_id.clone(),
            reply_to_text: reply_to_text.clone(),
            ..Default::default()
        }],
    };
    if let Err(e) =
        persist_message_history_page(state.storage().live_state(), &uid, &page, true).await
    {
        tracing::warn!(uid, session_id, error = %e, "failed to persist self-sent location");
    }

    bump_message_daily_stat(&state, &uid, "manual_reply_count", 1).await;
    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "message",
            action: "manual_send_location",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "session",
            target_id: &session_id,
            target_title: &to_uid,
            status: "success",
            error_code: "",
            message: "手动发送位置消息成功",
            params: json!({
                "to_uid": to_uid,
                "title": title,
                "content": content,
                "latitude": latitude,
                "longitude": longitude,
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
// POST /api/messages/send/item  (contentType=7 itemCard)
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct LiveSendItemPayload {
    pub uid: String,
    pub session_id: String,
    pub to_uid: String,
    pub item_id: String,
    pub title: String,
    pub main_pic: String,
    pub price: String, // "¥50.00" — with prefix
    #[serde(default)]
    pub item_tip: String, // 默认 "我想要"
    // 引用回复 — 同 location/text。
    #[serde(default)]
    pub reply_to_message_id: String,
    #[serde(default)]
    pub reply_to_text: String,
}

pub async fn live_send_item(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<LiveSendItemPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let uid = payload.uid.trim().to_string();
    let session_id = payload.session_id.trim().to_string();
    let to_uid = payload.to_uid.trim().to_string();
    let item_id = payload.item_id.trim().to_string();
    let title = payload.title.trim().to_string();
    let main_pic = payload.main_pic.trim().to_string();
    let price = payload.price.trim().to_string();
    let item_tip_input = payload.item_tip.trim().to_string();
    let item_tip = if item_tip_input.is_empty() {
        "我想要".to_string()
    } else {
        item_tip_input
    };
    let reply_to_message_id = payload.reply_to_message_id.trim().to_string();
    let reply_to_text = payload.reply_to_text.trim().to_string();

    if uid.is_empty()
        || session_id.is_empty()
        || to_uid.is_empty()
        || item_id.is_empty()
        || title.is_empty()
        || main_pic.is_empty()
        || price.is_empty()
    {
        return Err(ResponseError::bad_request(
            "uid / session_id / to_uid / item_id / title / main_pic / price required",
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    let account_owner = load_account_owner_username(&state, &uid).await?;
    ensure_account_can_call_upstream(&state, account_owner.as_deref(), &uid).await?;
    ensure_message_session_belongs_to_uid(&state, &uid, &session_id).await?;
    ensure_message_item_belongs_to_uid(&state, &uid, &item_id).await?;

    let sender = wait_for_live_sender(&state, &uid).await?;

    // Inner content envelope (contentType=7). Same comment as location: only
    // the outer wrapper is byte-strict, so `json!` is fine here.
    let inner = json!({
        "contentType": 7,
        "itemCard": {
            "item": {
                "itemId": item_id,
                "mainPic": main_pic,
                "price": price,
                "title": title,
            },
            "itemTip": item_tip,
        },
    });
    let encoded =
        BASE64_STANDARD.encode(serde_json::to_string(&inner).unwrap_or_default().as_bytes());

    let message_uuid = generate_uuid4_like();
    let ws_mid = generate_accs_mid();

    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register_send(&uid, &ws_mid, &message_uuid, ack_tx)
        .await
        .map_err(|_| ResponseError::service_unavailable("商品卡片发送请求标识已被占用，请重试"))?;

    let ext_json_inner = build_ext_json_inner(&reply_to_message_id);
    // Item card uses custom.type=3 same as location (dxCard family).
    let ws_payload_str = build_send_envelope(
        &ws_mid,
        &message_uuid,
        &session_id,
        &to_uid,
        &uid,
        3,
        &encoded,
        &ext_json_inner,
    );
    info!(
        target: "messages.live_ops",
        stream = "runtime",
        module = "messages-live",
        action = "send_item_frame_ready",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        detail = %format!("to={} item_id={} bytes={}", to_uid, item_id, ws_payload_str.len()),
        "prepared item-card websocket payload"
    );
    if sender.send(ws_payload_str).is_err() {
        let _ = state.live_response_waiters().take(&uid, &ws_mid).await;
        state
            .live_senders()
            .unregister_if_matches(&uid, &sender)
            .await;
        let _ = state
            .incident_reporter()
            .report_from_state(
                &state,
                IncidentInput {
                    error_level: "warn".to_string(),
                    module: "message".to_string(),
                    action: "manual_send_item_card".to_string(),
                    route: crate::sealed_protocol::sealed_str!("/api/messages/send/item-card")
                        .to_string(),
                    error_code: "live_socket_dropped".to_string(),
                    error_message: "live socket dropped while sending item".to_string(),
                    http_status: Some(503),
                    upstream_ret: String::new(),
                    retryable: true,
                    context_json: json!({
                        "sendKind": "item_card",
                        "uid": uid,
                        "sessionId": session_id,
                        "toUid": to_uid,
                        "wsMid": ws_mid,
                        "messageId": message_uuid,
                        "itemId": item_id,
                        "replyToMessageId": reply_to_message_id,
                    }),
                    environment: None,
                },
            )
            .await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending item",
        ));
    }

    let ts_millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let upstream_message_id = await_send_ack(
        &state,
        "manual_send_item_card",
        crate::sealed_protocol::sealed_str!("/api/messages/send/item-card"),
        "item_card",
        &uid,
        &session_id,
        &ws_mid,
        ack_rx,
    )
    .await?;

    let preview = format!("[商品] {title}");

    let card_meta_value = json!({
        "kind": "item",
        "itemId": item_id,
        "title": title,
        "mainPic": main_pic,
        "price": price,
        "itemTip": item_tip,
    });
    let card_meta = serde_json::to_string(&card_meta_value).unwrap_or_default();

    let mut session_identity = build_session_identity_from_cache(
        &state,
        &uid,
        &session_id,
        &to_uid,
        preview.clone(),
        ts_millis,
    )
    .await;
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
            kind: "card".to_string(),
            text: title.clone(),
            image_url: main_pic.clone(),
            content_type: 7,
            preview: preview.clone(),
            is_self: true,
            sender_nick: String::new(),
            card_meta,
            reply_to_message_id: reply_to_message_id.clone(),
            reply_to_text: reply_to_text.clone(),
            ..Default::default()
        }],
    };
    if let Err(e) =
        persist_message_history_page(state.storage().live_state(), &uid, &page, true).await
    {
        tracing::warn!(uid, session_id, error = %e, "failed to persist self-sent item card");
    }

    bump_message_daily_stat(&state, &uid, "manual_reply_count", 1).await;
    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "message",
            action: "manual_send_item",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "session",
            target_id: &session_id,
            target_title: &to_uid,
            status: "success",
            error_code: "",
            message: "手动发送商品卡片成功",
            params: json!({
                "to_uid": to_uid,
                "item_id": item_id,
                "title": title,
                "price": price,
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
// POST /api/messages/items/search
//
// Mirror PC web's item-card picker fetch — call
// `mtop.taobao.idlemessage.pc.tool.item.search` to list either the user's
// OWN items (`searchItemRole=own`) or the PEER's items in the conversation
// (`searchItemRole=other`). The peer-side fetch is what makes "TA的商品"
// work — there's no `GET /api/items` flavor for that.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct ItemSearchPayload {
    pub uid: String,
    pub session_id: String,
    /// "own" (default) — current account's listings.
    /// "other" — the peer's listings in this conversation.
    #[serde(default)]
    pub role: String,
    /// Optional title filter (server-side `queryWord`).
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_search_page")]
    pub page: i64,
    #[serde(default = "default_search_page_size")]
    pub page_size: i64,
}

fn default_search_page() -> i64 {
    1
}
fn default_search_page_size() -> i64 {
    20
}

/// 抽商品价格 — 抓包验证的字段优先：
///   • `reservePrice` ("149.00", 纯数字字符串) — 真实 MTOP 返回的字段
///   • `price` / `priceStr` — 兜底（其它内部接口的命名）
/// 拿到纯数字就加 ¥ 前缀；已经有「¥」就原样返回。
fn extract_item_price(item: &Value) -> String {
    let candidates = [
        json_string(item.get("reservePrice")),
        json_string(item.get("price")),
        json_string(item.get("priceStr")),
    ];
    for raw in candidates.iter() {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        if s.starts_with('¥') || s.starts_with('￥') {
            return s.to_string();
        }
        return format!("¥{s}");
    }
    String::new()
}

/// 抽商品主图 — `absoluteMajorPicture` 是抓包确认的真实字段，
/// `mainPic` / `picUrl` 是兜底。
fn extract_item_pic(item: &Value) -> String {
    let candidates = [
        json_string(item.get("absoluteMajorPicture")),
        json_string(item.get("mainPic")),
        json_string(item.get("picUrl")),
    ];
    candidates
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

pub async fn search_items_for_send(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    axum::Json(payload): axum::Json<ItemSearchPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let uid = payload.uid.trim().to_string();
    let session_id = payload.session_id.trim().to_string();
    let raw_role = payload.role.trim();
    // Default own; only allow the two roles the upstream recognises.
    let role = match raw_role {
        "" | "own" => "own",
        "other" => "other",
        _ => {
            return Err(ResponseError::bad_request("role must be 'own' or 'other'"));
        }
    };
    let query = payload.query.trim().to_string();
    // Clamp page/page_size to sane bounds so a hostile client can't ask for
    // 10K rows in one shot.
    let page = payload.page.max(1);
    let page_size = payload.page_size.clamp(1, 50);

    if uid.is_empty() || session_id.is_empty() {
        return Err(ResponseError::bad_request("uid and session_id required"));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    ensure_message_session_belongs_to_uid(&state, &uid, &session_id).await?;

    let data = json!({
        "sessionId": session_id,
        "pageSize": page_size,
        "pageNumber": page,
        "searchItemRole": role,
        "queryWord": query,
    });

    let protocol = super::super::resolve_account_protocol(&state, opcodes::MESSAGE_ITEM_SEARCH)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let upstream = successful_message_protocol_h5_request(
        &state,
        &uid,
        opcodes::MESSAGE_ITEM_SEARCH,
        data,
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    state.record_mtop_response(protocol.api(), &upstream);

    // 抓包验证的响应结构（idlemessage.pc.tool.item.search v1.0）：
    //   data.items: [{ itemId, title, absoluteMajorPicture, reservePrice, desc, targetUrl }]
    //   data.nextPage: bool（false = 没下一页）
    //   data.totalCount: i64
    // 旧实现猜的字段（list / hasMore / mainPic / price）全错——这里按抓包对齐。
    let mtop_data = coerce_mtop_data(&upstream);
    let list = json_array(mtop_data.get("items"));
    let has_more = json_bool(mtop_data.get("nextPage"));
    let total_count = json_i64(mtop_data.get("totalCount"));

    let items: Vec<Value> = list
        .iter()
        .filter_map(|item| {
            let item_id = json_string(item.get("itemId"));
            if item_id.is_empty() {
                return None;
            }
            Some(json!({
                "itemId": item_id,
                "title": json_string(item.get("title")),
                "mainPic": extract_item_pic(item),
                "price": extract_item_price(item),
            }))
        })
        .collect();

    Ok(axum::Json(json!({
        "items": items,
        "hasMore": has_more,
        "pageNumber": page,
        "totalCount": total_count,
        "role": role,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/messages/location/nearby — 高德附近 POI 代理
//
// 前端 JSONP 直连高德在 Tauri/CSP 环境里跑不通，所以把请求搬到后端。
// 高德 web key (PC 闲鱼自家的，公开)：`c9b68d4ce9a2a97f22a4a439404488ca`。
// 返回 [{name, address, latitude, longitude, distance}] —— 前端列表直接渲染。
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct NearbyPoiPayload {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default = "default_radius")]
    pub radius: i64,
    #[serde(default)]
    pub keyword: String,
    #[serde(default = "default_poi_page_size")]
    pub page_size: i64,
}
fn default_radius() -> i64 {
    500
}
fn default_poi_page_size() -> i64 {
    30
}

const AMAP_KEY: &str = "c9b68d4ce9a2a97f22a4a439404488ca";

pub async fn nearby_pois(
    axum::Json(payload): axum::Json<NearbyPoiPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let lat = payload.latitude;
    let lng = payload.longitude;
    if !lat.is_finite() || !lng.is_finite() || lat.abs() > 90.0 || lng.abs() > 180.0 {
        return Err(ResponseError::bad_request("invalid lat/lng"));
    }
    let radius = payload.radius.clamp(50, 5000);
    let offset = payload.page_size.clamp(1, 50);
    // 高德 location 参数是 "lng,lat" 顺序，跟 GPS 习惯反着 —— 别写错。
    let url = format!(
        "https://restapi.amap.com/v3/place/around?platform=JS&s=rsv3&logversion=2.0\
         &key={key}&sdkversion=2.3.5.6\
         &appname={appname}\
         &csid={csid}\
         &page=1&offset={offset}\
         &language=zh_cn\
         &location={lng},{lat}\
         &radius={radius}\
         &children=&type_=NEARBY&antiCrab=true\
         &keywords={kw}",
        key = AMAP_KEY,
        appname = crate::sealed_protocol::sealed_str!("https%3A%2F%2Fwww.goofish.com%2Fim"),
        csid = uuid_for_amap(),
        offset = offset,
        lng = lng,
        lat = lat,
        radius = radius,
        kw = urlencoding::encode(&payload.keyword),
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ResponseError::internal(e.to_string()))?;
    let resp = client
        .get(&url)
        .header("Referer", crate::sealed_protocol::sealed_str!("https://www.goofish.com/im"))
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36")
        .send()
        .await
        .map_err(|e| ResponseError::service_unavailable(format!("amap fetch failed: {e}")))?;
    let body = resp
        .text()
        .await
        .map_err(|e| ResponseError::service_unavailable(format!("amap read failed: {e}")))?;
    // 没传 callback 时高德直接返 JSON。但保险起见——如果上游帮我们包成 JSONP，
    // 把 `<cb>(...)` 的壳剥掉。
    let json_str = if let Some(start) = body.find('{') {
        let end = body.rfind('}').unwrap_or(body.len() - 1);
        &body[start..=end]
    } else {
        &body[..]
    };
    let parsed: Value = serde_json::from_str(json_str).map_err(|e| {
        ResponseError::service_unavailable(format!(
            "amap parse failed: {e}; body[:200]={}",
            &body[..body.len().min(200)]
        ))
    })?;

    if parsed.get("status").and_then(Value::as_str) != Some("1") {
        return Err(ResponseError::service_unavailable(format!(
            "amap status non-ok: {}",
            serde_json::to_string(&parsed)
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>(),
        )));
    }

    let pois_raw = json_array(parsed.get("pois"));
    let pois: Vec<Value> = pois_raw
        .iter()
        .filter_map(|p| {
            let name = json_string(p.get("name"));
            if name.is_empty() {
                return None;
            }
            // location 是 "lng,lat" 字符串
            let loc = json_string(p.get("location"));
            let mut parts = loc.split(',');
            let p_lng = parts.next().and_then(|s| s.trim().parse::<f64>().ok())?;
            let p_lat = parts.next().and_then(|s| s.trim().parse::<f64>().ok())?;
            let address = json_string(p.get("address"));
            let distance = json_string(p.get("distance"));
            // 行政区字段 —— pname / cityname / adname / adcode 是高德标准命名。
            // adcode 跟闲鱼/淘宝的 divisionId / fishAdCode 是同一国标 6 位行政区
            // 划代码（如 430104 = 长沙岳麓区），可以直接当 divisionId 用，省得
            // 再调一个 by_gps 接口反查。
            Some(json!({
                "name": name,
                "address": address,
                "latitude": format!("{:.6}", p_lat),
                "longitude": format!("{:.6}", p_lng),
                "distance": distance,
                "province": json_string(p.get("pname")),
                "city": json_string(p.get("cityname")),
                "district": json_string(p.get("adname")),
                "adcode": json_string(p.get("adcode")),
            }))
        })
        .collect();

    Ok(axum::Json(json!({
        "count": pois.len(),
        "pois": pois,
    })))
}

fn uuid_for_amap() -> String {
    // 高德 csid 接受任意 hex 串，长度无所谓。复用 generate_uuid4_like 的熵。
    generate_uuid4_like().to_uppercase()
}

// ---------------------------------------------------------------------------
// POST /api/messages/location/search — 高德输入提示（搜索关键词时调）
//
// 跟 nearby 一个套路 —— 前端调不通高德，代理到后端。返回的 tips 比
// place/around 的 pois 字段少（高德 inputtips 只给 name/address/location/id）。
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct InputTipsPayload {
    pub keyword: String,
    /// 城市限定（可选）—— 用 adcode / 城市名 / 拼音。空 = 全国
    #[serde(default)]
    pub city: String,
    /// citylimit=true 只在 city 内搜，false 跨城。默认 false。
    #[serde(default)]
    pub citylimit: bool,
    /// 中心点 lng,lat — 给个位置可以让高德按距离排序
    #[serde(default)]
    pub origin_latitude: Option<f64>,
    #[serde(default)]
    pub origin_longitude: Option<f64>,
}

pub async fn search_pois(
    axum::Json(payload): axum::Json<InputTipsPayload>,
) -> Result<axum::Json<Value>, ResponseError> {
    let keyword = payload.keyword.trim();
    if keyword.is_empty() {
        return Ok(axum::Json(json!({ "tips": [] })));
    }
    let location_param = match (payload.origin_longitude, payload.origin_latitude) {
        (Some(lng), Some(lat)) if lng.is_finite() && lat.is_finite() => {
            format!("&location={lng},{lat}")
        }
        _ => String::new(),
    };
    let url = format!(
        "https://restapi.amap.com/v3/assistant/inputtips?platform=JS&s=rsv3&logversion=2.0\
         &key={key}&sdkversion=2.3.5.6\
         &appname={appname}\
         &csid={csid}\
         &city={city}\
         &type=&citylimit={citylimit}\
         &datatype=all\
         &language=zh_cn\
         &keywords={kw}\
         {location}",
        key = AMAP_KEY,
        appname = crate::sealed_protocol::sealed_str!("https%3A%2F%2Fwww.goofish.com%2Fim"),
        csid = uuid_for_amap(),
        city = urlencoding::encode(&payload.city),
        citylimit = if payload.citylimit { "true" } else { "false" },
        kw = urlencoding::encode(keyword),
        location = location_param,
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| ResponseError::internal(e.to_string()))?;
    let resp = client
        .get(&url)
        .header(
            "Referer",
            crate::sealed_protocol::sealed_str!("https://www.goofish.com/im"),
        )
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/148.0.0.0 Safari/537.36",
        )
        .send()
        .await
        .map_err(|e| ResponseError::service_unavailable(format!("amap tips fetch failed: {e}")))?;
    let body = resp
        .text()
        .await
        .map_err(|e| ResponseError::service_unavailable(format!("amap tips read failed: {e}")))?;
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|e| ResponseError::service_unavailable(format!("amap tips parse failed: {e}")))?;
    if parsed.get("status").and_then(Value::as_str) != Some("1") {
        return Err(ResponseError::service_unavailable(format!(
            "amap tips non-ok: {}",
            serde_json::to_string(&parsed)
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>(),
        )));
    }

    let tips_raw = json_array(parsed.get("tips"));
    // inputtips 偶尔返没 location 的纯文字建议（典型是高德的「行政区」联想），
    // 这种没坐标的过滤掉——发送时必须有 lat/lng。
    let tips: Vec<Value> = tips_raw
        .iter()
        .filter_map(|t| {
            let name = json_string(t.get("name"));
            let loc = json_string(t.get("location"));
            if name.is_empty() || loc.is_empty() {
                return None;
            }
            let mut parts = loc.split(',');
            let lng = parts.next().and_then(|s| s.trim().parse::<f64>().ok())?;
            let lat = parts.next().and_then(|s| s.trim().parse::<f64>().ok())?;
            let address = json_string(t.get("address"));
            let district = json_string(t.get("district"));
            // inputtips 的 district 字段格式："长沙市岳麓区" —— 不是分开的
            // city / district。adcode 一般有；province 字段没有，需要从
            // district 字符串里推或者前端不显示也行（adcode 6位足以唯一定位）。
            Some(json!({
                "name": name,
                "address": if !address.is_empty() { address.clone() } else { district.clone() },
                "latitude": format!("{:.6}", lat),
                "longitude": format!("{:.6}", lng),
                // 把整个 district 字段透传，让前端在 adcode 不够时降级显示
                "district": district,
                "adcode": json_string(t.get("adcode")),
            }))
        })
        .collect();

    Ok(axum::Json(json!({
        "count": tips.len(),
        "tips": tips,
    })))
}
