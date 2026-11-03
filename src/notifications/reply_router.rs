use serde_json::{json, Value};
use sqlx::Row;
use tracing::debug;

use crate::{
    routes::{live::live_sender_unavailable_detail, tenant::account_uid_accessible},
    state::AppState,
};

pub(crate) async fn send_reply_to_session_for_owner(
    state: &AppState,
    owner_username: &str,
    session_id: &str,
    body: &str,
) -> Result<ReplyDeliveryResult, String> {
    let route =
        resolve_reply_route_from_session_for_owner(state, owner_username, session_id).await?;
    send_text_via_live_socket(state, &route.uid, session_id, &route.peer_uid, body).await?;
    Ok(ReplyDeliveryResult {
        session_id: session_id.to_string(),
        customer_name: route.customer_name,
        uid: route.uid,
        peer_uid: route.peer_uid,
        item_id: route.item_id,
        item_title: route.item_title,
    })
}

pub(crate) async fn send_reply_via_route(
    state: &AppState,
    uid: &str,
    session_id: &str,
    peer_uid: &str,
    customer_name: &str,
    item_id: &str,
    item_title: &str,
    body: &str,
) -> Result<ReplyDeliveryResult, String> {
    send_text_via_live_socket(state, uid, session_id, peer_uid, body).await?;
    Ok(ReplyDeliveryResult {
        session_id: session_id.to_string(),
        customer_name: customer_name.trim().to_string(),
        uid: uid.trim().to_string(),
        peer_uid: peer_uid.trim().to_string(),
        item_id: item_id.trim().to_string(),
        item_title: item_title.trim().to_string(),
    })
}

#[derive(Debug, Clone)]
pub(crate) struct ReplyDeliveryResult {
    pub(crate) session_id: String,
    pub(crate) customer_name: String,
    pub(crate) uid: String,
    pub(crate) peer_uid: String,
    pub(crate) item_id: String,
    pub(crate) item_title: String,
}

#[derive(Debug, Clone)]
struct ReplyRoute {
    uid: String,
    peer_uid: String,
    customer_name: String,
    item_id: String,
    item_title: String,
}

async fn resolve_reply_route_from_session_for_owner(
    state: &AppState,
    owner_username: &str,
    session_id: &str,
) -> Result<ReplyRoute, String> {
    let owner = owner_username.trim();
    let candidates = sqlx::query(
        r#"
        SELECT uid, peer_uid, peer_nick, item_id, item_title
        FROM message_live_state
        WHERE session_id = ?
          AND (
                latest_version > 0
                OR last_message_ts > 0
                OR TRIM(COALESCE(latest_message_id, '')) <> ''
                OR EXISTS (
                    SELECT 1
                    FROM message_history_cache h
                    WHERE h.uid = message_live_state.uid
                      AND h.session_id = message_live_state.session_id
                    LIMIT 1
                )
          )
        ORDER BY updated_at DESC
        LIMIT 20
        "#,
    )
    .bind(session_id)
    .fetch_all(state.storage().live_state())
    .await
    .map_err(|err| err.to_string())?;

    let mut selected = None;
    for row in candidates {
        let uid: String = row.try_get("uid").map_err(|err| err.to_string())?;
        if account_uid_belongs_to_owner(state, &uid, owner).await? {
            selected = Some((uid, row));
            break;
        }
    }

    let Some((uid, row)) = selected else {
        return Err(format!("未找到当前用户可回复的会话 `{session_id}`"));
    };
    let peer_uid: String = row.try_get("peer_uid").map_err(|err| err.to_string())?;
    let customer_name: String = row.try_get("peer_nick").unwrap_or_default();
    let item_id: String = row.try_get("item_id").unwrap_or_default();
    let item_title: String = row.try_get("item_title").unwrap_or_default();
    if uid.trim().is_empty() || peer_uid.trim().is_empty() {
        return Err(format!(
            "会话 `{session_id}` 的路由信息不完整，缺少 uid 或 peer_uid"
        ));
    }
    Ok(ReplyRoute {
        uid,
        peer_uid,
        customer_name,
        item_id,
        item_title,
    })
}

pub(crate) async fn account_uid_belongs_to_owner(
    state: &AppState,
    uid: &str,
    owner_username: &str,
) -> Result<bool, String> {
    account_uid_accessible(
        state.storage().accounts(),
        Some(owner_username.trim()),
        uid.trim(),
    )
    .await
    .map_err(|err| err.to_string())
}

async fn send_text_via_live_socket(
    state: &AppState,
    uid: &str,
    session_id: &str,
    to_uid: &str,
    content: &str,
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use std::time::Duration;

    let sender = match state
        .live_senders()
        .wait_alive(uid, Duration::from_secs(5))
        .await
    {
        Some(sender) => sender,
        None => return Err(live_sender_unavailable_detail(state, uid).await),
    };

    let inner = json!({ "contentType": 1, "text": { "text": content } });
    let encoded =
        BASE64_STANDARD.encode(serde_json::to_string(&inner).unwrap_or_default().as_bytes());

    let message_uuid = generate_uuid4_like();
    let ws_mid = generate_accs_mid();

    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register_send(uid, &ws_mid, &message_uuid, ack_tx)
        .await
        .map_err(|_| "消息发送请求标识已被占用，请重试".to_string())?;

    let data_cid = format!("{session_id}@goofish");
    let recv_to = format!("{to_uid}@goofish");
    let recv_self = format!("{uid}@goofish");

    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp":"/r/MessageSend/sendByReceiverScope","headers":{{"mid":"{mid}"}},"body":["#,
            r#"{{"uuid":"{uuid}","cid":"{cid}","conversationType":1,"#,
            r#""content":{{"contentType":101,"custom":{{"type":1,"data":"{data}"}}}},"#,
            r#""redPointPolicy":0,"extension":{{"extJson":"{{}}"}},"#,
            r#""ctx":{{"appVersion":"1.0","platform":"wx"}},"#,
            r#""mtags":{{}},"msgReadStatusSetting":1}},"#,
            r#"{{"actualReceivers":["{r1}","{r2}"]}}]}}"#,
        ),
        mid = escape_json_string(&ws_mid),
        uuid = escape_json_string(&message_uuid),
        cid = escape_json_string(&data_cid),
        data = escape_json_string(&encoded),
        r1 = escape_json_string(&recv_to),
        r2 = escape_json_string(&recv_self),
    );

    debug!(
        target: "notifications.reply_router",
        stream = "runtime",
        module = "notifications-reply-router",
        action = "send_start",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        to_uid = %to_uid,
        text_preview = %truncate_for_log(content),
        "sending reply via live socket"
    );

    if sender.send(ws_payload_str).is_err() {
        let _ = state.live_response_waiters().take(uid, &ws_mid).await;
        return Err("live socket dropped while sending".to_string());
    }
    let ack = match tokio::time::timeout(Duration::from_secs(2), ack_rx).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(_)) => return Err("live socket ACK waiter dropped".to_string()),
        Err(_) => {
            let _ = state.live_response_waiters().take(uid, &ws_mid).await;
            return Err("live socket ACK timeout".to_string());
        }
    };

    let code = ack
        .get("code")
        .and_then(Value::as_i64)
        .or_else(|| {
            ack.get("code")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<i64>().ok())
        })
        .unwrap_or_default();
    if code != 200 {
        return Err(format!(
            "live socket ACK failed: {}",
            serde_json::to_string(&ack).unwrap_or_default()
        ));
    }

    let pnm = ack
        .get("body")
        .and_then(|body| body.get("messageId"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if pnm.is_empty() {
        return Err(format!(
            "live socket ACK missing messageId: {}",
            serde_json::to_string(&ack).unwrap_or_default()
        ));
    }

    debug!(
        target: "notifications.reply_router",
        stream = "runtime",
        module = "notifications-reply-router",
        action = "send_ack_ok",
        outcome = "success",
        account_uid = %uid,
        session_id = %session_id,
        to_uid = %to_uid,
        upstream_message_id = %pnm,
        "reply live socket ACK succeeded"
    );
    Ok(())
}

fn truncate_for_log(text: &str) -> String {
    let trimmed = text.trim();
    let mut out = trimmed.chars().take(80).collect::<String>();
    if trimmed.chars().count() > 80 {
        out.push_str("...");
    }
    out
}

fn escape_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    for ch in input.chars() {
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

fn generate_uuid4_like() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn generate_accs_mid() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static N: AtomicU32 = AtomicU32::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let nanos3 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_nanos() % 1000) as u32)
        .unwrap_or(0);
    let seq = N.fetch_add(1, Ordering::Relaxed) % 1000;
    let prefix = (nanos3 ^ seq) % 1000;
    format!("{prefix}{ms} 0")
}
