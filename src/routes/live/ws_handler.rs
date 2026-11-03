use std::collections::HashSet;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::{
    models::auth::{AuthUser, RequestIdentity},
    routes::{
        accounts::messages::load_trusted_single_session_info,
        tenant::{accessible_account_uids, auth_user_has_unscoped_legacy_access},
    },
    state::AppState,
    storage::StorageState,
};

use super::{
    messaging::{send_live_image_message, send_live_text_message},
    protocol::{command_result_payload, normalized_string, unix_timestamp_millis},
};

#[derive(Debug, Deserialize)]
pub(super) struct LiveWsQuery {
    pub(super) token: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct LiveClientCommand {
    pub(super) cmd: String,
    pub(super) request_id: Option<String>,
    pub(super) uid: Option<String>,
    pub(super) uids: Option<Vec<String>>,
    pub(super) session_id: Option<String>,
    pub(super) session_type: Option<i64>,
    pub(super) peer_uid: Option<String>,
    pub(super) peer_nick: Option<String>,
    pub(super) item_id: Option<String>,
    pub(super) item_title: Option<String>,
    pub(super) to_uid: Option<String>,
    pub(super) content: Option<String>,
    pub(super) image_url: Option<String>,
    pub(super) width: Option<i64>,
    pub(super) height: Option<i64>,
}

pub(super) async fn live_ws(
    State(state): State<AppState>,
    Query(_query): Query<LiveWsQuery>,
    websocket: WebSocketUpgrade,
) -> Response {
    let identity = RequestIdentity::new("local-app".to_string(), "admin".to_string(), true, true);

    websocket.on_upgrade(move |socket| handle_socket(socket, identity, state))
}

async fn handle_socket(socket: WebSocket, identity: RequestIdentity, state: AppState) {
    let user = AuthUser::from(identity.clone());
    let current_uid = user.username.clone();
    let storage = state.storage().clone();
    let allowed_uids = websocket_allowed_uids(&state, &identity, &user).await;
    let mut live_rx = state.live_events().subscribe();
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<Value>();

    let writer = tokio::spawn(async move {
        while let Some(payload) = outgoing_rx.recv().await {
            if ws_sender
                .send(Message::Text(payload.to_string()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    if outgoing_tx
        .send(json!({
            "type": "connection.connected",
            "user": user,
            "channel": "browser",
        }))
        .is_err()
    {
        writer.abort();
        return;
    }

    let mut focused_sessions = HashSet::new();
    let mut subscribed_uids = HashSet::new();

    loop {
        tokio::select! {
            incoming = ws_receiver.next() => {
                let Some(message_result) = incoming else {
                    break;
                };
                let Ok(message) = message_result else {
                    break;
                };
                if !handle_client_message(
                    &state,
                    &storage,
                    allowed_uids.as_ref(),
                    &current_uid,
                    &outgoing_tx,
                    &mut focused_sessions,
                    &mut subscribed_uids,
                    message,
                )
                .await {
                    break;
                }
            }
            event = live_rx.recv() => {
                match event {
                    Ok(event) => {
                        let uid = event.get("uid").and_then(Value::as_str).unwrap_or_default();
                        if !uid_allowed(allowed_uids.as_ref(), uid) {
                            continue;
                        }
                        if !subscribed_uids.is_empty() && !subscribed_uids.contains(uid) {
                            continue;
                        }
                        if outgoing_tx.send(event).is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    writer.abort();
}

async fn handle_client_message(
    state: &AppState,
    storage: &StorageState,
    allowed_uids: Option<&HashSet<String>>,
    current_uid: &str,
    outgoing_tx: &mpsc::UnboundedSender<Value>,
    focused_sessions: &mut HashSet<String>,
    subscribed_uids: &mut HashSet<String>,
    message: Message,
) -> bool {
    match message {
        Message::Text(text) => {
            let Ok(command) = serde_json::from_str::<LiveClientCommand>(&text) else {
                return true;
            };

            match command.cmd.trim().to_ascii_lowercase().as_str() {
                "ping" => {
                    let _ = outgoing_tx.send(json!({
                        "type": "pong",
                        "ts": unix_timestamp_millis(),
                    }));
                }
                "subscribe" => {
                    *subscribed_uids = command
                        .uids
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|uid| normalized_string(Some(uid)))
                        .filter(|uid| uid_allowed(allowed_uids, uid))
                        .collect();

                    if let Ok(states) = storage.list_live_connection_states().await {
                        for state in states {
                            if !uid_allowed(allowed_uids, &state.uid) {
                                continue;
                            }
                            if !subscribed_uids.is_empty()
                                && !subscribed_uids.contains(state.uid.as_str())
                            {
                                continue;
                            }
                            let _ = outgoing_tx.send(json!({
                                "type": format!("connection.{}", state.status),
                                "uid": state.uid,
                                "reason": state.last_error,
                                "channel": "wx",
                                "snapshot": true,
                                "updatedAt": (state.updated_at * 1000.0) as u64,
                            }));
                        }
                    }
                }
                "focus" => {
                    let uid =
                        normalized_string(command.uid).unwrap_or_else(|| current_uid.to_string());
                    if !uid_allowed(allowed_uids, &uid) {
                        return true;
                    }
                    let Some(session_id) = normalized_string(command.session_id) else {
                        return true;
                    };
                    let session_type = command.session_type.unwrap_or(1);

                    match load_trusted_single_session_info(
                        storage.live_state(),
                        &uid,
                        &session_id,
                        session_type,
                    )
                    .await
                    {
                        Ok(Some(_)) => {
                            focused_sessions.insert(format!("{uid}:{session_id}"));
                            let _ = outgoing_tx.send(json!({
                                "type": "session.focused",
                                "uid": uid,
                                "sessionId": session_id,
                                "sessionType": session_type,
                            }));
                        }
                        _ => {
                            let _ = outgoing_tx.send(json!({
                                "type": "session.focus.rejected",
                                "uid": uid,
                                "sessionId": session_id,
                                "sessionType": session_type,
                                "error": "会话不存在",
                            }));
                        }
                    }
                }
                "blur" => {
                    let uid =
                        normalized_string(command.uid).unwrap_or_else(|| current_uid.to_string());
                    if !uid_allowed(allowed_uids, &uid) {
                        return true;
                    }
                    if let Some(session_id) = normalized_string(command.session_id) {
                        focused_sessions.remove(format!("{uid}:{session_id}").as_str());
                        let _ = outgoing_tx.send(json!({
                            "type": "session.blurred",
                            "uid": uid,
                            "sessionId": session_id,
                        }));
                    } else {
                        focused_sessions.clear();
                        let _ = outgoing_tx.send(json!({
                            "type": "session.blurred",
                            "uid": uid,
                        }));
                    }
                }
                "send" => {
                    let request_id = normalized_string(command.request_id).unwrap_or_default();
                    let uid =
                        normalized_string(command.uid).unwrap_or_else(|| current_uid.to_string());
                    let session_id = normalized_string(command.session_id).unwrap_or_default();
                    if !uid_allowed(allowed_uids, &uid) {
                        let payload = command_result_payload(
                            "send.result",
                            &request_id,
                            &session_id,
                            Err("账号不存在".to_string()),
                        );
                        let _ = outgoing_tx.send(payload);
                        return true;
                    }
                    let to_uid = normalized_string(command.to_uid).unwrap_or_default();
                    let content = normalized_string(command.content).unwrap_or_default();
                    let response =
                        match validate_ws_session_target(storage, &uid, &session_id, 1, &to_uid)
                            .await
                        {
                            Ok(()) => {
                                send_live_text_message(state, &uid, &session_id, &to_uid, &content)
                                    .await
                            }
                            Err(err) => Err(err),
                        };
                    let payload =
                        command_result_payload("send.result", &request_id, &session_id, response);
                    let _ = outgoing_tx.send(payload);
                }
                "send_image" => {
                    let request_id = normalized_string(command.request_id).unwrap_or_default();
                    let uid =
                        normalized_string(command.uid).unwrap_or_else(|| current_uid.to_string());
                    let session_id = normalized_string(command.session_id).unwrap_or_default();
                    if !uid_allowed(allowed_uids, &uid) {
                        let payload = command_result_payload(
                            "send_image.result",
                            &request_id,
                            &session_id,
                            Err("账号不存在".to_string()),
                        );
                        let _ = outgoing_tx.send(payload);
                        return true;
                    }
                    let to_uid = normalized_string(command.to_uid).unwrap_or_default();
                    let image_url = normalized_string(command.image_url).unwrap_or_default();
                    let width = command.width.unwrap_or(0).max(0);
                    let height = command.height.unwrap_or(0).max(0);
                    let response =
                        match validate_ws_session_target(storage, &uid, &session_id, 1, &to_uid)
                            .await
                        {
                            Ok(()) => {
                                send_live_image_message(
                                    state,
                                    &uid,
                                    &session_id,
                                    &to_uid,
                                    &image_url,
                                    width,
                                    height,
                                )
                                .await
                            }
                            Err(err) => Err(err),
                        };
                    let payload = command_result_payload(
                        "send_image.result",
                        &request_id,
                        &session_id,
                        response,
                    );
                    let _ = outgoing_tx.send(payload);
                }
                _ => {}
            }
            true
        }
        Message::Close(_) => false,
        _ => true,
    }
}

async fn websocket_allowed_uids(
    state: &AppState,
    identity: &RequestIdentity,
    user: &AuthUser,
) -> Option<HashSet<String>> {
    if identity.bootstrap {
        return None;
    }
    if auth_user_has_unscoped_legacy_access(user) {
        return None;
    }
    accessible_account_uids(state.storage().accounts(), Some(user.username.trim()))
        .await
        .ok()
}

fn uid_allowed(allowed_uids: Option<&HashSet<String>>, uid: &str) -> bool {
    allowed_uids
        .map(|allowed| allowed.contains(uid))
        .unwrap_or(true)
}

async fn validate_ws_session_target(
    storage: &StorageState,
    uid: &str,
    session_id: &str,
    session_type: i64,
    to_uid: &str,
) -> Result<(), String> {
    let Some(session) =
        load_trusted_single_session_info(storage.live_state(), uid, session_id, session_type)
            .await
            .map_err(|err| err.to_string())?
    else {
        return Err("会话不存在".to_string());
    };
    let cached_peer = session
        .get("peerUid")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if !cached_peer.is_empty() && cached_peer != to_uid.trim() {
        return Err("会话接收方不匹配".to_string());
    }
    Ok(())
}
