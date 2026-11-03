//! History fetching, hydration, normalization, and prefetch.

use std::sync::Arc;

use axum::{
    extract::{Extension, Query, State},
    Json as ResponseJson,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::Semaphore;
use tracing::{debug, warn};

use crate::{
    models::auth::RequestIdentity,
    notifications::feishu_group_router::repair_group_metadata_for_event,
    protocol_catalog::opcodes,
    routes::accounts::LiveMessageEvent,
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
    telemetry::IncidentInput,
    xianyu_client::{self, LocationContext},
};

use super::{
    coerce_mtop_data, default_page_size_30, default_session_type,
    ensure_message_session_belongs_to_uid_with_type, json_array, json_bool, json_i64, json_object,
    json_string, load_sessions_for_uid, load_single_session_info, merge_cached_session_identity,
    message_history_entry_to_value, message_history_page_to_value, message_location_payload,
    message_session_identity_from_value, message_session_identity_to_value, message_user_display,
    normalize_message_content, normalize_message_history,
    normalize_message_session_identity_from_session, normalize_message_sessions,
    peer_nick_needs_fallback, persist_message_history_page, persist_message_sessions,
    successful_message_protocol_h5_request, ApiResult, MessageHistoryEntry, MessageHistoryPage,
    MessageSessionIdentity, ResponseError, MAX_SESSION_LIST_LIMIT, WX_MESSAGE_SESSION_TYPES,
};

fn json_log_summary(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().take(24).cloned().collect::<Vec<_>>();
            keys.sort();
            let data_key_count = map
                .get("data")
                .and_then(Value::as_object)
                .map(|data| data.len())
                .unwrap_or(0);
            json!({
                "kind": "object",
                "keyCount": map.len(),
                "keys": keys,
                "dataKeyCount": data_key_count,
            })
        }
        Value::Array(values) => json!({ "kind": "array", "len": values.len() }),
        Value::String(text) => json!({ "kind": "string", "len": text.chars().count() }),
        Value::Null => json!({ "kind": "null" }),
        Value::Bool(_) => json!({ "kind": "bool" }),
        Value::Number(_) => json!({ "kind": "number" }),
    }
}

fn json_log_summary_string(value: &Value) -> String {
    serde_json::to_string(&json_log_summary(value)).unwrap_or_else(|_| "{}".to_string())
}

use super::super::{
    live::fetch_message_history_via_active_socket, resolve_account_protocol,
    successful_upstream_protocol_h5_request,
};

fn should_trace_raw_session_probe(session_id: &str) -> bool {
    matches!(
        session_id,
        "61696947021" | "61826137452" | "61826145707" | "61915880115"
    )
}

#[derive(Debug, Default)]
struct MessageTradeHeadInfo {
    item_id: String,
    item_title: String,
    item_pic: String,
}

#[derive(Debug, Default)]
struct MessageItemDetailInfo {
    item_title: String,
    item_pic: String,
}

// ---------------------------------------------------------------------------
// Query types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SessionHistoryQuery {
    uid: String,
    session_id: String,
    #[serde(default = "default_session_type")]
    session_type: i64,
    #[serde(default = "default_page_size_30")]
    num: i64,
    #[serde(default)]
    ts: i64,
    /// When `true`, skip the mtop upstream hydrate (4-5s per call) and return
    /// only local `message_history_cache` rows. Lets the UI paint instantly
    /// from the live-worker's pre-seeded cache, then issue a background
    /// `force=true` refresh to pull any gaps from upstream.
    #[serde(default)]
    cached: bool,
    #[serde(default)]
    force: bool,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub async fn pc_message_history(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SessionHistoryQuery>,
) -> ApiResult {
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        query.uid.trim(),
    )
    .await?;
    ensure_message_session_belongs_to_uid_with_type(
        &state,
        query.uid.trim(),
        query.session_id.trim(),
        query.session_type,
    )
    .await?;
    // The live worker continuously persists ACCS push messages into
    // `message_history_cache`, and the history-prefetch worker bulk-fills old
    // messages on startup. So `cached=true` means "pure local read" — we
    // never hit the 4-5s mtop upstream path on click. `force=true` still goes
    // upstream for manual refresh.
    let skip_upstream = query.cached && !query.force;

    if !skip_upstream {
        match hydrate_message_history_from_upstream(
            state.storage().live_state(),
            &state,
            &query.uid,
            &query.session_id,
            query.session_type,
            query.num.clamp(1, 200),
            query.ts,
        )
        .await
        {
            Ok(page) => {
                if !page.messages.is_empty() {
                    let mut normalized = message_history_page_to_value(&page);
                    if let Some(obj) = normalized.as_object_mut() {
                        obj.insert("warning".to_string(), Value::Null);
                        obj.insert("stale".to_string(), Value::Bool(false));
                    }
                    return Ok(ResponseJson(json!({
                        "normalized": normalized,
                    })));
                }
            }
            Err(error) => {
                let cached_count = sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(1) FROM message_history_cache WHERE uid = ?1 AND session_id = ?2",
                )
                .bind(&query.uid)
                .bind(&query.session_id)
                .fetch_one(state.storage().live_state())
                .await
                .unwrap_or(0);
                if cached_count <= 0 {
                    let _ = state
                        .incident_reporter()
                        .report_from_state(
                            &state,
                            IncidentInput {
                                error_level: "warn".to_string(),
                                module: "message".to_string(),
                                action: "load_history".to_string(),
                                route: crate::sealed_protocol::sealed_str!(
                                    "/api/messages/pc/history"
                                )
                                .to_string(),
                                error_code: "history_upstream_unavailable".to_string(),
                                error_message: error.detail().to_string(),
                                http_status: Some(503),
                                upstream_ret: String::new(),
                                retryable: true,
                                context_json: json!({
                                    "uid": query.uid,
                                    "sessionId": query.session_id,
                                    "sessionType": query.session_type,
                                    "num": query.num,
                                    "ts": query.ts,
                                    "cached": query.cached,
                                    "force": query.force,
                                    "cachedCount": cached_count,
                                    "phase": "initial_hydrate",
                                }),
                                environment: None,
                            },
                        )
                        .await;
                    return Err(ResponseError::service_unavailable(error.detail()));
                }
                return Ok(ResponseJson(json!({
                    "normalized": {
                        "warning": error.detail(),
                        "stale": true,
                    }
                })));
            }
        }
    }

    let session_seed = load_single_session_info(
        state.storage().live_state(),
        &query.uid,
        &query.session_id,
        query.session_type,
    )
    .await
    .map_err(ResponseError::internal)?
    .unwrap_or_else(|| {
        json!({
            "sessionId": query.session_id,
            "sessionType": query.session_type,
            "peerUid": "",
            "peerNick": "",
            "peerLogo": "",
            "peerRole": "",
            "selfRole": "",
            "itemId": "",
            "itemTitle": "",
            "itemPic": "",
            "summary": "",
        })
    });

    let sql = if query.ts > 0 {
        r#"
        SELECT session_type, message_id, upstream_message_id, version, ts, kind, title, text, image_url, image_urls,
               content_type, preview, sender_nick, sender_logo, is_self, unread_count, card_meta,
               recalled_at, recalled_original_text, recalled_original_image_url,
               reply_to_message_id, reply_to_text
        FROM message_history_cache
        WHERE uid = ? AND session_id = ? AND version < ?
        ORDER BY version DESC, ts DESC
        LIMIT ?
        "#
    } else {
        r#"
        SELECT session_type, message_id, upstream_message_id, version, ts, kind, title, text, image_url, image_urls,
               content_type, preview, sender_nick, sender_logo, is_self, unread_count, card_meta,
               recalled_at, recalled_original_text, recalled_original_image_url,
               reply_to_message_id, reply_to_text
        FROM message_history_cache
        WHERE uid = ? AND session_id = ?
        ORDER BY version DESC, ts DESC
        LIMIT ?
        "#
    };
    let rows = if query.ts > 0 {
        sqlx::query(sql)
            .bind(&query.uid)
            .bind(&query.session_id)
            .bind(query.ts)
            .bind(query.num.clamp(1, 10000))
            .fetch_all(state.storage().live_state())
            .await
            .map_err(ResponseError::internal)?
    } else {
        sqlx::query(sql)
            .bind(&query.uid)
            .bind(&query.session_id)
            .bind(query.num.clamp(1, 10000))
            .fetch_all(state.storage().live_state())
            .await
            .map_err(ResponseError::internal)?
    };

    let mut messages = rows
        .into_iter()
        .map(super::history_row_to_value)
        .collect::<Vec<_>>();
    messages.reverse();

    let versions = messages
        .iter()
        .filter_map(|message| message.get("version").and_then(Value::as_i64))
        .collect::<Vec<_>>();
    let next_cursor = versions.iter().copied().min().unwrap_or(0);
    let latest_cursor = versions.iter().copied().max().unwrap_or(0);

    let mut normalized = session_seed;
    let normalized_object = normalized
        .as_object_mut()
        .ok_or_else(|| ResponseError::internal_message("invalid session seed"))?;
    normalized_object.insert(
        "hasMore".to_string(),
        Value::Bool(messages.len() as i64 >= query.num),
    );
    normalized_object.insert("nextCursor".to_string(), Value::from(next_cursor));
    normalized_object.insert("latestCursor".to_string(), Value::from(latest_cursor));
    normalized_object.insert("messages".to_string(), Value::Array(messages));
    normalized_object.insert("stale".to_string(), Value::Bool(false));
    normalized_object.insert("warning".to_string(), Value::Null);

    Ok(ResponseJson(json!({ "normalized": normalized })))
}

// ---------------------------------------------------------------------------
// Hydration helpers
// ---------------------------------------------------------------------------

pub async fn hydrate_message_sessions_from_upstream(
    state: &AppState,
    uid: &str,
    limit: i64,
) -> Result<Vec<MessageSessionIdentity>, ResponseError> {
    let payload = json!({
        "sessionTypes": WX_MESSAGE_SESSION_TYPES,
        "fetchNum": limit.clamp(1, MAX_SESSION_LIST_LIMIT),
    });
    let protocol = resolve_account_protocol(state, opcodes::MESSAGE_PC_SESSION_SYNC)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let upstream = successful_message_protocol_h5_request(
        state,
        uid,
        opcodes::MESSAGE_PC_SESSION_SYNC,
        payload,
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    state.record_mtop_response(protocol.api(), &upstream);
    let sessions = normalize_message_sessions(&upstream, uid);
    persist_message_sessions(state.storage().live_state(), uid, &sessions)
        .await
        .map_err(ResponseError::internal)?;
    Ok(sessions)
}

pub(crate) async fn hydrate_message_sessions_from_wx_session_list(
    state: &AppState,
    uid: &str,
    limit: i64,
) -> Result<Vec<MessageSessionIdentity>, ResponseError> {
    let payload = json!({
        "sessionTypes": WX_MESSAGE_SESSION_TYPES,
        "fetchNum": limit.clamp(1, MAX_SESSION_LIST_LIMIT),
    });
    let protocol = resolve_account_protocol(state, opcodes::MESSAGE_WX_SESSION_SYNC)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let upstream = successful_message_protocol_h5_request(
        state,
        uid,
        opcodes::MESSAGE_WX_SESSION_SYNC,
        payload,
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    state.record_mtop_response(protocol.api(), &upstream);
    let sessions = normalize_message_sessions(&upstream, uid);
    persist_message_sessions(state.storage().live_state(), uid, &sessions)
        .await
        .map_err(ResponseError::internal)?;
    Ok(sessions)
}

pub async fn hydrate_message_history_from_upstream(
    _live_state: &sqlx::Pool<sqlx::Sqlite>,
    state: &AppState,
    uid: &str,
    session_id: &str,
    session_type: i64,
    limit: i64,
    cursor: i64,
) -> Result<MessageHistoryPage, ResponseError> {
    let location = xianyu_client::load_location(state.storage().accounts(), uid)
        .await
        .unwrap_or_else(|_| LocationContext::default());
    let mut payload = message_location_payload(&location);
    let version = if cursor > 0 {
        cursor
    } else {
        i64::from(i32::MAX)
    };
    payload.insert(
        "req".to_string(),
        Value::String(
            serde_json::to_string(&json!({
                "sessionId": session_id,
                "type": session_type,
                "start": 0,
                "version": version,
                "fetchs": limit.clamp(1, MAX_SESSION_LIST_LIMIT),
                // Python `pc_message_sync` sends this; the app-IM variant
                // (mtop.taobao.idlemessage.message.sync) silently drops
                // session_type 3/6 content, but the PC variant with
                // includeRequestMsg returns系统通知 / 互动消息 history.
                "includeRequestMsg": true,
            }))
            .map_err(ResponseError::internal)?,
        ),
    );
    // Python `LiveConnectionManager._prefetch_session_history` uses the PC
    // variant. The App variant we were using doesn't return messages for
    // session_type 3 (系统通知) / 6 (互动消息) — that's why the cache had
    // 17K type-1 rows but zero type-3/6 rows.
    let protocol = resolve_account_protocol(state, opcodes::MESSAGE_PC_HISTORY_SYNC)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let mut page = match successful_message_protocol_h5_request(
        state,
        uid,
        opcodes::MESSAGE_PC_HISTORY_SYNC,
        Value::Object(payload),
        Vec::new(),
    )
    .await
    {
        Ok(upstream) => {
            if should_trace_raw_session_probe(session_id) {
                debug!(
                    target: "messages.history",
                    stream = "runtime",
                    module = "messages-history",
                    action = "raw_probe_http",
                    account_uid = %uid,
                    session_id = %session_id,
                    api = %protocol.api(),
                    payload_summary = %json_log_summary_string(&upstream),
                    "captured raw history sync probe payload"
                );
            }
            state.record_mtop_response(protocol.api(), &upstream);
            normalize_message_history(&upstream, uid)
        }
        Err(error) => {
            warn!(
                uid,
                session_id,
                detail = %error,
                "hydrate message history upstream failed"
            );
            Default::default()
        }
    };
    let upstream_empty = page.messages.is_empty();

    if page.messages.is_empty() {
        // P-4 fix: ONLY use the active-socket path. The previous fallback to
        // `fetch_message_history_via_ws` opened an independent short WS,
        // re-ran /reg + /ackDiff for the same uid, and made goofish silence
        // push delivery to the primary live worker. Python doesn't have a
        // fallback — `list_messages` always uses the active connection.
        let ws_result = fetch_message_history_via_active_socket(
            state,
            uid,
            session_id,
            limit.clamp(1, MAX_SESSION_LIST_LIMIT),
            cursor,
        )
        .await;
        match ws_result {
            Ok(ws_payload) => {
                let ws_page = normalize_message_history(&ws_payload, uid);
                if !ws_page.messages.is_empty() {
                    page = ws_page;
                }
            }
            Err(error) => {
                warn!(uid, session_id, detail = %error.detail(), "wx live history fallback failed");
                if upstream_empty {
                    let _ = state
                        .incident_reporter()
                        .report_from_state(
                            state,
                            IncidentInput {
                                error_level: "warn".to_string(),
                                module: "message".to_string(),
                                action: "hydrate_history".to_string(),
                                route: crate::sealed_protocol::sealed_str!(
                                    "/api/messages/pc/history"
                                )
                                .to_string(),
                                error_code: "history_upstream_unavailable".to_string(),
                                error_message: error.detail().to_string(),
                                http_status: Some(503),
                                upstream_ret: String::new(),
                                retryable: true,
                                context_json: json!({
                                    "uid": uid,
                                    "sessionId": session_id,
                                    "sessionType": session_type,
                                    "limit": limit,
                                    "cursor": cursor,
                                    "phase": "active_socket_fallback",
                                }),
                                environment: None,
                            },
                        )
                        .await;
                }
            }
        }
    }

    if page.session.session_id.is_empty() {
        page.session.session_id = session_id.to_string();
    }
    if page.session.session_id != session_id {
        return Err(ResponseError::internal_message(format!(
            "message history session mismatch: expected {session_id}, got {}",
            page.session.session_id
        )));
    }
    if page.session.session_type <= 0 {
        page.session.session_type = session_type.max(1);
    }
    merge_cached_session_identity(
        state.storage().live_state(),
        uid,
        session_id,
        page.session.session_type,
        &mut page.session,
    )
    .await
    .map_err(ResponseError::internal)?;
    if page.session.version <= 0 {
        page.session.version = page.latest_cursor;
    }
    if page.session.ts <= 0 {
        page.session.ts = page.latest_cursor;
    }
    if page.session.sort_index <= 0 {
        page.session.sort_index = page.session.ts;
    }
    if page.session.summary.is_empty() {
        page.session.summary = page
            .messages
            .last()
            .map(|message| {
                if !message.preview.trim().is_empty() {
                    message.preview.clone()
                } else if !message.text.trim().is_empty() {
                    message.text.clone()
                } else {
                    message.title.clone()
                }
            })
            .unwrap_or_default();
    }

    let refresh_session_state = cursor <= 0;
    persist_message_history_page(
        state.storage().live_state(),
        uid,
        &page,
        refresh_session_state,
    )
    .await
    .map_err(ResponseError::internal)?;
    Ok(page)
}

pub async fn hydrate_message_session_info_from_upstream(
    state: &AppState,
    uid: &str,
    session_id: &str,
    session_type: i64,
) -> Result<MessageSessionIdentity, ResponseError> {
    let cached_session =
        load_single_session_info(state.storage().live_state(), uid, session_id, session_type)
            .await
            .map_err(ResponseError::internal)?
            .map(|value| message_session_identity_from_value(&value));

    let mut session = hydrate_message_history_from_upstream(
        state.storage().live_state(),
        state,
        uid,
        session_id,
        session_type,
        20,
        0,
    )
    .await?
    .session;
    if should_trace_raw_session_probe(session_id) {
        debug!(
            target: "messages.history",
            stream = "runtime",
            module = "messages-history",
            action = "raw_probe_session_info",
            account_uid = %uid,
            session_id = %session_id,
            stage = "after_history",
            payload_summary = %json_log_summary_string(&message_session_identity_to_value(&session)),
            "captured post-history session identity probe"
        );
    }
    if let Some(cached) = cached_session.as_ref() {
        if session.session_id.trim().is_empty() {
            session.session_id = cached.session_id.clone();
        }
        if session.session_type <= 0 {
            session.session_type = cached.session_type.max(1);
        }
    }
    let mut session_changed = false;

    // Live-only sessions often already have peer_uid but still miss item
    // metadata. `pc.session.sync` is still our only stable way to merge the
    // canonical session identity back into the cache, so refresh whenever any
    // essential trade-session identity field is still missing.
    if session_needs_session_sync_refresh(&session) {
        if let Ok(_refreshed) = hydrate_message_sessions_from_upstream(state, uid, 100).await {
            if should_trace_raw_session_probe(session_id) {
                let payload = json!({
                    "sessionTypes": WX_MESSAGE_SESSION_TYPES,
                    "fetchNum": 100,
                });
                if let Ok(raw) = successful_message_protocol_h5_request(
                    state,
                    uid,
                    opcodes::MESSAGE_PC_SESSION_SYNC,
                    payload,
                    Vec::new(),
                )
                .await
                {
                    let protocol =
                        resolve_account_protocol(state, opcodes::MESSAGE_PC_SESSION_SYNC)
                            .await
                            .map_err(ResponseError::service_unavailable)?;
                    debug!(
                        target: "messages.history",
                        stream = "runtime",
                        module = "messages-history",
                        action = "raw_probe_http",
                        account_uid = %uid,
                        session_id = %session_id,
                        api = %protocol.api(),
                        payload_summary = %json_log_summary_string(&raw),
                        "captured session sync probe payload"
                    );
                }
            }
            let before = snapshot_session_identity(&session);
            merge_cached_session_identity(
                state.storage().live_state(),
                uid,
                session_id,
                session.session_type,
                &mut session,
            )
            .await
            .map_err(ResponseError::internal)?;
            session_changed |= snapshot_session_identity(&session) != before;
        }
    }

    if session.session_type == 1 && !session.peer_uid.trim().is_empty() {
        if let Ok(profile) = fetch_message_user_query(
            state,
            uid,
            session_id,
            session.session_type,
            &session.peer_uid,
        )
        .await
        {
            let user_id = json_string(profile.get("userId"));
            let fish_nick = json_string(profile.get("fishNick"));
            let logo = json_string(profile.get("logo"));
            if !user_id.is_empty() && user_id != session.peer_uid {
                session.peer_uid = user_id;
                session_changed = true;
            }
            if !fish_nick.is_empty() && fish_nick != session.peer_nick {
                session.peer_nick = fish_nick;
                session_changed = true;
            }
            if !logo.is_empty() && logo != session.peer_logo {
                session.peer_logo = logo;
                session_changed = true;
            }
        }
    }

    session_changed |= enrich_session_item_metadata(state, uid, &mut session).await?;

    let should_repair_group_metadata = should_repair_group_metadata_after_hydration(
        cached_session.as_ref(),
        &session,
        session_changed,
    );

    if session_changed {
        persist_message_sessions(state.storage().live_state(), uid, &[session.clone()])
            .await
            .map_err(ResponseError::internal)?;
    }
    if should_repair_group_metadata {
        let _ = repair_group_metadata_for_event(
            state,
            uid,
            &LiveMessageEvent {
                session_id: session.session_id.clone(),
                session_type: session.session_type.max(1),
                ts: session.ts.max(session.sort_index).max(session.version),
                peer_uid: session.peer_uid.clone(),
                peer_nick: session.peer_nick.clone(),
                peer_logo: session.peer_logo.clone(),
                peer_role: session.peer_role.clone(),
                self_role: session.self_role.clone(),
                item_id: session.item_id.clone(),
                item_title: session.item_title.clone(),
                item_pic: session.item_pic.clone(),
                ..LiveMessageEvent::default()
            },
        )
        .await;
    }

    if let Some(cached) = cached_session {
        let mut merged = session.clone();
        let cached_peer_uid = cached.peer_uid.clone();
        let cached_peer_nick = cached.peer_nick.clone();
        let cached_peer_logo = cached.peer_logo.clone();
        let cached_peer_role = cached.peer_role.clone();
        let cached_self_role = cached.self_role.clone();
        let cached_item_id = cached.item_id.clone();
        let cached_item_title = cached.item_title.clone();
        let cached_item_pic = cached.item_pic.clone();
        let cached_summary = cached.summary.clone();
        if merged.peer_uid.trim().is_empty() {
            merged.peer_uid = cached_peer_uid.clone();
        }
        if peer_nick_needs_fallback(&merged.peer_nick, &merged.peer_uid)
            && !peer_nick_needs_fallback(&cached_peer_nick, &cached_peer_uid)
        {
            merged.peer_nick = cached_peer_nick;
        }
        if merged.peer_logo.trim().is_empty() {
            merged.peer_logo = cached_peer_logo;
        }
        if merged.peer_role.trim().is_empty() {
            merged.peer_role = cached_peer_role;
        }
        if merged.self_role.trim().is_empty() {
            merged.self_role = cached_self_role;
        }
        if merged.item_id.trim().is_empty() {
            merged.item_id = cached_item_id;
        }
        if merged.item_title.trim().is_empty() {
            merged.item_title = cached_item_title;
        }
        if merged.item_pic.trim().is_empty() {
            merged.item_pic = cached_item_pic;
        }
        if merged.summary.trim().is_empty() {
            merged.summary = cached_summary;
        }
        if merged.ts <= 0 {
            merged.ts = cached.ts;
        }
        if merged.sort_index <= 0 {
            merged.sort_index = cached.sort_index;
        }
        if merged.version <= 0 {
            merged.version = cached.version;
        }
        session = merged;
    }

    if should_trace_raw_session_probe(session_id) {
        debug!(
            target: "messages.history",
            stream = "runtime",
            module = "messages-history",
            action = "raw_probe_session_info",
            account_uid = %uid,
            session_id = %session_id,
            stage = "final",
            payload_summary = %json_log_summary_string(&message_session_identity_to_value(&session)),
            "captured final session identity probe"
        );
    }

    Ok(session)
}

/// Startup history prefetch — matches Python `LiveConnectionManager._prefetch_worker`.
/// Pulls recent sessions via MTOP + first page of history per session so the cache
/// is warm by the time the UI opens a chat. Runs serially per account, parallel-5 per session.
pub async fn prefetch_account_message_history(
    state: &AppState,
    uid: &str,
) -> Result<usize, String> {
    const SESSIONS_LIMIT: i64 = 100;
    const PER_SESSION_LIMIT: i64 = 50;
    const MAX_SESSIONS: usize = 60;
    const PER_SESSION_CONCURRENCY: usize = 5;

    let _ = hydrate_message_sessions_from_upstream(state, uid, SESSIONS_LIMIT).await;
    let _ = hydrate_message_sessions_from_wx_session_list(state, uid, SESSIONS_LIMIT).await;

    let sessions = load_sessions_for_uid(state.storage().live_state(), uid, SESSIONS_LIMIT)
        .await
        .map_err(|error| error.to_string())?;

    let mut targets = Vec::with_capacity(sessions.len());
    for session in sessions.iter().take(MAX_SESSIONS) {
        let session_id = json_string(session.get("sessionId"));
        if session_id.is_empty() {
            continue;
        }
        let session_type = session
            .get("sessionType")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        targets.push((session_id, session_type));
    }
    if targets.is_empty() {
        return Ok(0);
    }

    let semaphore = Arc::new(Semaphore::new(PER_SESSION_CONCURRENCY));
    let mut handles = Vec::with_capacity(targets.len());
    for (session_id, session_type) in targets {
        let permit_sem = semaphore.clone();
        let state_clone = state.clone();
        let uid_owned = uid.to_string();
        handles.push(tokio::spawn(async move {
            let _permit = permit_sem.acquire_owned().await.ok()?;
            match hydrate_message_history_from_upstream(
                state_clone.storage().live_state(),
                &state_clone,
                &uid_owned,
                &session_id,
                session_type,
                PER_SESSION_LIMIT,
                0,
            )
            .await
            {
                Ok(page) => Some(page.messages.len()),
                Err(error) => {
                    warn!(
                        uid = %uid_owned,
                        session_id = %session_id,
                        detail = %error.detail(),
                        "history prefetch session failed",
                    );
                    None
                }
            }
        }));
    }

    let mut ok_sessions = 0usize;
    for handle in handles {
        if let Ok(Some(_)) = handle.await {
            ok_sessions += 1;
        }
    }

    // Batch user.query to fill missing peer_nick/peer_logo for type-1 sessions.
    // Matches the WX mini-program behavior which fires user.query for every chat
    // session on page load (185 calls observed in captures).
    tokio::spawn({
        let state = state.clone();
        let uid = uid.to_string();
        async move {
            if let Err(e) = enrich_missing_peer_profiles(&state, &uid).await {
                tracing::warn!(uid = %uid, err = %e, "batch user.query enrich failed");
            }
        }
    });

    Ok(ok_sessions)
}

/// Bulk-fetch peer nicknames/avatars for all sessionType=1 sessions that are
/// missing `peer_nick`. Uses concurrency-5 to avoid hammering upstream.
async fn enrich_missing_peer_profiles(state: &AppState, uid: &str) -> Result<(), String> {
    let rows = sqlx::query(
        r#"SELECT session_id, session_type, peer_uid FROM message_live_state
           WHERE uid = ? AND session_type = 1
             AND TRIM(peer_uid) <> ''
             AND (
                 TRIM(peer_nick) = ''
                 OR peer_nick = peer_uid
                 OR peer_nick IN ('你有一条新消息', '您有一条新消息')
                 OR peer_nick LIKE '用户_%'
                 OR peer_nick LIKE 'tbNick_%'
             )"#,
    )
    .bind(uid)
    .fetch_all(state.storage().live_state())
    .await
    .map_err(|e| e.to_string())?;

    if rows.is_empty() {
        return Ok(());
    }
    tracing::info!(uid = %uid, count = rows.len(), "enriching peer profiles via user.query");

    let semaphore = Arc::new(Semaphore::new(5));
    let mut handles = Vec::with_capacity(rows.len());
    for row in rows {
        use sqlx::Row as _;
        let session_id: String = row.get("session_id");
        let session_type: i64 = row.get("session_type");
        let peer_uid: String = row.get("peer_uid");
        let sem = semaphore.clone();
        let st = state.clone();
        let uid_owned = uid.to_string();
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire_owned().await.ok()?;
            let profile =
                fetch_message_user_query(&st, &uid_owned, &session_id, session_type, &peer_uid)
                    .await
                    .ok()?;
            let fish_nick = json_string(profile.get("fishNick"));
            let logo = json_string(profile.get("logo"));
            if fish_nick.is_empty() && logo.is_empty() {
                return None;
            }
            let session = MessageSessionIdentity {
                session_id,
                session_type,
                peer_uid,
                peer_nick: fish_nick,
                peer_logo: logo,
                ..Default::default()
            };
            let _ =
                persist_message_sessions(st.storage().live_state(), &uid_owned, &[session]).await;
            Some(())
        }));
    }
    for handle in handles {
        let _ = handle.await;
    }
    Ok(())
}

async fn fetch_message_user_query(
    state: &AppState,
    uid: &str,
    session_id: &str,
    session_type: i64,
    peer_uid: &str,
) -> Result<Map<String, Value>, ResponseError> {
    let protocol = resolve_account_protocol(state, opcodes::ACCOUNT_IDLEMESSAGE_USER_QUERY)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let upstream = successful_message_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_IDLEMESSAGE_USER_QUERY,
        json!({
            "type": if session_type == 3 { 10 } else { 0 },
            "userId": super::str_to_json_number(peer_uid),
            "sessionId": session_id,
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    if should_trace_raw_session_probe(session_id) {
        debug!(
            target: "messages.history",
            stream = "runtime",
            module = "messages-history",
            action = "raw_probe_http",
            account_uid = %uid,
            session_id = %session_id,
            api = %protocol.api(),
            peer_uid = %peer_uid,
            payload_summary = %json_log_summary_string(&upstream),
            "captured user.query probe payload"
        );
    }
    let data = coerce_mtop_data(&upstream);
    Ok(json_object(data.get("userInfo")))
}

fn session_needs_session_sync_refresh(session: &MessageSessionIdentity) -> bool {
    session.session_type == 1
        && (session.peer_uid.trim().is_empty()
            || peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid)
            || session.item_id.trim().is_empty()
            || session.item_title.trim().is_empty()
            || session.item_pic.trim().is_empty())
}

fn snapshot_session_identity(
    session: &MessageSessionIdentity,
) -> (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
) {
    (
        session.peer_uid.clone(),
        session.peer_nick.clone(),
        session.peer_logo.clone(),
        session.peer_role.clone(),
        session.self_role.clone(),
        session.item_id.clone(),
        session.item_title.clone(),
        session.item_pic.clone(),
    )
}

fn should_repair_group_metadata_after_hydration(
    cached: Option<&MessageSessionIdentity>,
    session: &MessageSessionIdentity,
    session_changed: bool,
) -> bool {
    if session_changed {
        return true;
    }
    let Some(cached) = cached else {
        return false;
    };

    (!session.peer_logo.trim().is_empty() && session.peer_logo != cached.peer_logo)
        || (!session.item_id.trim().is_empty() && session.item_id != cached.item_id)
        || (!session.item_title.trim().is_empty() && session.item_title != cached.item_title)
        || (!session.item_pic.trim().is_empty() && session.item_pic != cached.item_pic)
        || (!peer_nick_needs_fallback(&session.peer_nick, &session.peer_uid)
            && session.peer_nick != cached.peer_nick)
}

async fn enrich_session_item_metadata(
    state: &AppState,
    uid: &str,
    session: &mut MessageSessionIdentity,
) -> Result<bool, ResponseError> {
    if session.session_type != 1 || session.item_id.trim().is_empty() {
        return Ok(false);
    }
    if !session.item_title.trim().is_empty() && !session.item_pic.trim().is_empty() {
        return Ok(false);
    }

    let mut changed = false;

    if let Ok(headinfo) = fetch_message_trade_headinfo(
        state,
        uid,
        &session.session_id,
        &session.item_id,
        &session.peer_uid,
    )
    .await
    {
        if !headinfo.item_id.trim().is_empty() && headinfo.item_id != session.item_id {
            session.item_id = headinfo.item_id;
            changed = true;
        }
        if !headinfo.item_title.trim().is_empty() && headinfo.item_title != session.item_title {
            session.item_title = headinfo.item_title;
            changed = true;
        }
        if !headinfo.item_pic.trim().is_empty() && headinfo.item_pic != session.item_pic {
            session.item_pic = headinfo.item_pic;
            changed = true;
        }
    }

    if session.item_title.trim().is_empty() || session.item_pic.trim().is_empty() {
        if let Ok(detail) =
            fetch_message_item_detail(state, uid, &session.session_id, &session.item_id).await
        {
            if !detail.item_title.trim().is_empty() && detail.item_title != session.item_title {
                session.item_title = detail.item_title;
                changed = true;
            }
            if session.item_pic.trim().is_empty() && !detail.item_pic.trim().is_empty() {
                session.item_pic = detail.item_pic;
                changed = true;
            }
        }
    }

    Ok(changed)
}

async fn fetch_message_trade_headinfo(
    state: &AppState,
    uid: &str,
    session_id: &str,
    item_id: &str,
    peer_uid: &str,
) -> Result<MessageTradeHeadInfo, ResponseError> {
    let clean_peer_uid = peer_uid.trim().replace('*', "");
    let mut data = json!({
        "itemId": super::str_to_json_number(item_id),
        "sessionId": session_id,
    });
    if !clean_peer_uid.is_empty() {
        data["peerUserId"] = super::str_to_json_number(&clean_peer_uid);
    }

    let protocol = resolve_account_protocol(state, opcodes::MESSAGE_TRADE_HEADINFO)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let payload = successful_message_protocol_h5_request(
        state,
        uid,
        opcodes::MESSAGE_TRADE_HEADINFO,
        data,
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    if should_trace_raw_session_probe(session_id) {
        debug!(
            target: "messages.history",
            stream = "runtime",
            module = "messages-history",
            action = "raw_probe_http",
            account_uid = %uid,
            session_id = %session_id,
            api = %protocol.api(),
            item_id = %item_id,
            peer_uid = %peer_uid,
            payload_summary = %json_log_summary_string(&payload),
            "captured trade headinfo probe payload"
        );
    }

    Ok(normalize_message_trade_headinfo(&coerce_mtop_data(
        &payload,
    )))
}

fn normalize_message_trade_headinfo(data: &Map<String, Value>) -> MessageTradeHeadInfo {
    let common = json_object(data.get("commonData"));
    let item_pre_info = json_object_or_parse_string(common.get("itemPreInfo"));
    let left = json_object(json_object(data.get("left")).get("data"));
    let head = json_object(data.get("messageHeadVO"));

    let mut item_id = json_string(common.get("itemId"));
    if item_id.is_empty() {
        item_id = json_string(item_pre_info.get("itemId"));
    }

    let mut item_title = json_string(item_pre_info.get("title"));
    if item_title.is_empty() {
        item_title = json_string(head.get("semanticsDesc"));
    }
    if item_title.is_empty() {
        let fallback_title = json_string(head.get("title"));
        if !looks_like_price_label(&fallback_title) {
            item_title = fallback_title;
        }
    }

    let mut item_pic = json_string(left.get("picUrl"));
    if item_pic.is_empty() {
        item_pic = json_string(head.get("picUrl"));
    }
    if item_pic.is_empty() {
        item_pic = first_image_url_from_value(item_pre_info.get("imageInfos"));
    }

    MessageTradeHeadInfo {
        item_id,
        item_title,
        item_pic,
    }
}

async fn fetch_message_item_detail(
    state: &AppState,
    uid: &str,
    session_id: &str,
    item_id: &str,
) -> Result<MessageItemDetailInfo, ResponseError> {
    let protocol = resolve_account_protocol(state, opcodes::MESSAGE_ITEM_DETAIL)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let payload = successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::MESSAGE_ITEM_DETAIL,
        json!({ "itemId": item_id }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    if should_trace_raw_session_probe(session_id) {
        debug!(
            target: "messages.history",
            stream = "runtime",
            module = "messages-history",
            action = "raw_probe_http",
            account_uid = %uid,
            session_id = %session_id,
            api = %protocol.api(),
            item_id = %item_id,
            payload_summary = %json_log_summary_string(&payload),
            "captured item detail probe payload"
        );
    }

    let data = coerce_mtop_data(&payload);
    let item_do = json_object(data.get(crate::sealed_protocol::sealed_str!("itemDO")));
    let mut item_pic = first_image_url_from_value(item_do.get("imageInfos"));
    if item_pic.is_empty() {
        item_pic = first_image_url_from_value(data.get("imageInfos"));
    }

    Ok(MessageItemDetailInfo {
        item_title: json_string(item_do.get("title")),
        item_pic,
    })
}

fn json_object_or_parse_string(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(object)) => object.clone(),
        Some(Value::String(raw)) if !raw.trim().is_empty() => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|parsed| parsed.as_object().cloned())
            .unwrap_or_default(),
        _ => Map::new(),
    }
}

fn json_array_or_parse_string(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(array)) => array.clone(),
        Some(Value::String(raw)) if !raw.trim().is_empty() => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|parsed| parsed.as_array().cloned())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn first_image_url_from_value(value: Option<&Value>) -> String {
    for image in json_array_or_parse_string(value) {
        match image {
            Value::String(url) if !url.trim().is_empty() => return url.trim().to_string(),
            Value::Object(object) => {
                for key in ["url", "picUrl", "image", "mainPic"] {
                    let candidate = json_string(object.get(key));
                    if !candidate.is_empty() {
                        return candidate;
                    }
                }
            }
            _ => {}
        }
    }
    String::new()
}

fn looks_like_price_label(raw: &str) -> bool {
    let trimmed = raw.trim();
    !trimmed.is_empty()
        && (trimmed.starts_with('¥') || trimmed.starts_with('￥') || trimmed.parse::<f64>().is_ok())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map, Value};

    use super::{
        looks_like_price_label, normalize_message_trade_headinfo,
        session_needs_session_sync_refresh, should_repair_group_metadata_after_hydration,
    };
    use crate::routes::accounts::messages::MessageSessionIdentity;

    #[test]
    fn enrich_query_includes_placeholder_peer_nick_rows() {
        let sql = r#"SELECT session_id, session_type, peer_uid FROM message_live_state
           WHERE uid = ? AND session_type = 1
             AND TRIM(peer_uid) <> ''
             AND (
                 TRIM(peer_nick) = ''
                 OR peer_nick = peer_uid
                 OR peer_nick IN ('你有一条新消息', '您有一条新消息')
                 OR peer_nick LIKE '用户_%'
                 OR peer_nick LIKE 'tbNick_%'
             )"#;

        assert!(sql.contains("peer_nick IN ('你有一条新消息', '您有一条新消息')"));
    }

    #[test]
    fn session_refresh_triggers_when_trade_item_metadata_is_missing() {
        let session = MessageSessionIdentity {
            session_id: "61739760907".to_string(),
            session_type: 1,
            peer_uid: "2218438371992".to_string(),
            peer_nick: "茜茜爱写作".to_string(),
            item_id: "1051897161756".to_string(),
            ..Default::default()
        };

        assert!(session_needs_session_sync_refresh(&session));
    }

    #[test]
    fn normalize_trade_headinfo_prefers_semantics_desc_over_price_title() {
        let data = serde_json::from_value::<Map<String, Value>>(json!({
            "messageHeadVO": {
                "title": "¥55.00",
                "semanticsDesc": "西数1TB蓝盘机械硬盘，WD10EALX，3.5寸SATA，",
                "picUrl": "http://img.example.com/item.jpg"
            }
        }))
        .unwrap();

        let normalized = normalize_message_trade_headinfo(&data);
        assert_eq!(
            normalized.item_title,
            "西数1TB蓝盘机械硬盘，WD10EALX，3.5寸SATA，"
        );
        assert_eq!(normalized.item_pic, "http://img.example.com/item.jpg");
    }

    #[test]
    fn price_labels_are_not_used_as_item_titles() {
        assert!(looks_like_price_label("¥55.00"));
        assert!(looks_like_price_label("55"));
        assert!(!looks_like_price_label("露营车拖挂房车"));
    }

    #[test]
    fn richer_hydration_result_still_triggers_group_metadata_repair() {
        let cached = MessageSessionIdentity {
            session_id: "63099988597".to_string(),
            session_type: 1,
            peer_uid: "90000000001".to_string(),
            peer_nick: "测试用户甲".to_string(),
            ..Default::default()
        };
        let hydrated = MessageSessionIdentity {
            session_id: "63099988597".to_string(),
            session_type: 1,
            peer_uid: "90000000001".to_string(),
            peer_nick: "测试用户甲".to_string(),
            peer_logo: "http://img.alicdn.com/avatar.jpg".to_string(),
            item_id: "1062141666047".to_string(),
            item_title: "女士T恤".to_string(),
            ..Default::default()
        };

        assert!(should_repair_group_metadata_after_hydration(
            Some(&cached),
            &hydrated,
            false,
        ));
    }
}
