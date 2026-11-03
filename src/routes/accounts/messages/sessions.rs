//! Session CRUD handlers — list, batch, info, mark-read, headinfo.

use axum::{
    extract::{Extension, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
};

use super::{
    default_page_size_30, default_session_type, default_session_types,
    enrich_session_values_with_upstream, ensure_message_session_belongs_to_uid_with_type,
    filter_session_values_by_types, hydrate_message_history_from_upstream,
    hydrate_message_session_info_from_upstream, hydrate_message_sessions_from_upstream,
    hydrate_message_sessions_from_wx_session_list, info_has_identity, json_string,
    load_sessions_for_uid, message_session_identity_to_value, parse_message_session_types,
    str_to_json_number, successful_message_protocol_h5_request, ApiResult, ResponseError,
    MAX_SESSION_LIST_LIMIT,
};

// ---------------------------------------------------------------------------
// Query / payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SessionBatchQuery {
    #[serde(default = "default_page_size_30")]
    num: i64,
}

#[derive(Debug, Deserialize)]
pub struct SessionListQuery {
    uid: String,
    #[serde(default = "default_session_types")]
    types: String,
    #[serde(default = "default_page_size_30")]
    num: i64,
    #[serde(default)]
    cached: bool,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
pub struct SessionInfoQuery {
    uid: String,
    session_id: String,
    #[serde(default = "default_session_type")]
    session_type: i64,
}

#[derive(Debug, Deserialize)]
pub struct HeadinfoQuery {
    uid: String,
    session_id: String,
    item_id: String,
    #[serde(default)]
    peer_uid: String,
}

#[derive(Debug, Deserialize)]
pub struct PcMarkReadPayload {
    pub uid: String,
    pub session_id: String,
    #[serde(default = "default_session_type")]
    pub session_type: i64,
    #[serde(default)]
    pub msg_id: String,
    #[serde(default)]
    pub peer_uid: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn pc_sessions_batch(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SessionBatchQuery>,
) -> ApiResult {
    let limit = query.num.clamp(1, 50);
    let owner = owner_username_filter(&state, &identity);
    let account_rows = sqlx::query(
        "SELECT uid FROM accounts WHERE status != 'disabled' AND (? IS NULL OR owner_username = ?) ORDER BY updated_at DESC, uid ASC",
    )
    .bind(owner.as_deref())
    .bind(owner.as_deref())
    .fetch_all(state.storage().accounts())
    .await
    .map_err(ResponseError::internal)?;

    let mut accounts = Map::new();
    for row in account_rows {
        let uid = row.get::<String, _>("uid");
        let mut sessions = load_sessions_for_uid(state.storage().live_state(), &uid, limit)
            .await
            .map_err(ResponseError::internal)?;
        if sessions.is_empty() {
            if hydrate_message_sessions_from_upstream(&state, &uid, limit)
                .await
                .is_ok()
            {
                sessions = load_sessions_for_uid(state.storage().live_state(), &uid, limit)
                    .await
                    .map_err(ResponseError::internal)?;
            }
        }
        accounts.insert(uid, Value::Array(sessions));
    }

    Ok(ResponseJson(json!({
        "accounts": accounts,
        "cached": true,
        "limit": limit,
    })))
}

pub async fn pc_sessions(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SessionListQuery>,
) -> ApiResult {
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        query.uid.trim(),
    )
    .await?;
    let allowed_types = parse_message_session_types(&query.types);
    let cached_sessions = filter_session_values_by_types(
        load_sessions_for_uid(
            state.storage().live_state(),
            &query.uid,
            query.num.clamp(1, MAX_SESSION_LIST_LIMIT),
        )
        .await
        .map_err(ResponseError::internal)?,
        &allowed_types,
    );

    let should_refresh = query.force || !query.cached || cached_sessions.is_empty();
    if should_refresh {
        let limit = query.num.clamp(1, MAX_SESSION_LIST_LIMIT);
        // P-4 fix: removed `sync_recent_message_events_via_ws` — that opened
        // an independent short-lived WS connection per pc_sessions request,
        // re-running /reg + /ackDiff on the same uid as the live worker. Each
        // such reg made the goofish ACCS server treat the live worker as a
        // "secondary device" and silenced syncPushPackage delivery to it.
        // Python doesn't have any equivalent — sessions list comes purely
        // from MTOP HTTP + cached state.
        let pc_error = hydrate_message_sessions_from_upstream(&state, &query.uid, limit)
            .await
            .err()
            .map(|error| error.detail().to_owned());
        let wx_error = hydrate_message_sessions_from_wx_session_list(&state, &query.uid, limit)
            .await
            .err()
            .map(|error| error.detail().to_owned());
        let hydrate_error = pc_error.or(wx_error);

        // `pc.session.sync` sometimes keeps a sticky unread>0 even after the
        // official client has already read the latest incoming message. This
        // affects both chat sessions and platform notification channels. The
        // message history endpoint carries per-message unreadCount, so
        // re-sync a small set of unread sessions and let history-derived
        // session state override the stale session-sync value.
        if hydrate_error.is_none() {
            reconcile_recent_session_unreads_from_history(&state, &query.uid, limit)
                .await
                .ok();
        }

        let mut refreshed_sessions = filter_session_values_by_types(
            load_sessions_for_uid(state.storage().live_state(), &query.uid, limit)
                .await
                .map_err(ResponseError::internal)?,
            &allowed_types,
        );
        refreshed_sessions =
            enrich_session_values_with_upstream(&state, &query.uid, refreshed_sessions).await;
        if !refreshed_sessions.is_empty() || cached_sessions.is_empty() {
            return Ok(ResponseJson(json!({
                "normalized": {
                    "sessions": refreshed_sessions,
                    "cached": false,
                    "stale": hydrate_error.is_some(),
                    "warning": hydrate_error,
                }
            })));
        }
        if let Some(detail) = hydrate_error {
            return Err(ResponseError::service_unavailable(detail));
        }
    }

    Ok(ResponseJson(json!({
        "normalized": {
            "sessions": cached_sessions,
            "cached": true,
        }
    })))
}

fn should_reconcile_session_unread_from_history(session_type: i64) -> bool {
    matches!(session_type, 1 | 3 | 6 | 53)
}

async fn reconcile_recent_session_unreads_from_history(
    state: &AppState,
    uid: &str,
    limit: i64,
) -> Result<(), ResponseError> {
    const MAX_TARGETS: usize = 12;
    const HISTORY_LIMIT: i64 = 20;

    let sessions = load_sessions_for_uid(state.storage().live_state(), uid, limit)
        .await
        .map_err(ResponseError::internal)?;
    let targets = sessions
        .into_iter()
        .filter_map(|session| {
            let session_type = session
                .get("sessionType")
                .and_then(Value::as_i64)
                .unwrap_or(1);
            let unread = session.get("unread").and_then(Value::as_i64).unwrap_or(0);
            if !should_reconcile_session_unread_from_history(session_type) || unread <= 0 {
                return None;
            }
            let session_id = json_string(session.get("sessionId"));
            if session_id.is_empty() {
                None
            } else {
                Some((session_id, session_type))
            }
        })
        .take(MAX_TARGETS)
        .collect::<Vec<_>>();

    for (session_id, session_type) in targets {
        let _ = hydrate_message_history_from_upstream(
            state.storage().live_state(),
            state,
            uid,
            &session_id,
            session_type,
            HISTORY_LIMIT,
            0,
        )
        .await;
    }

    Ok(())
}

/// Mark a session as read upstream, then zero local unread count.
///
/// For sessionType=1 (chat): sends `relation.message.read` which notifies
/// the peer that messages have been read (requires msgId + peerUid).
/// For sessionType=3/6 (system): sends `pc.systems.unread.clean` which
/// only clears the unread badge.
pub async fn pc_mark_session_read(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    ResponseJson(payload): ResponseJson<PcMarkReadPayload>,
) -> ApiResult {
    let uid = payload.uid.trim();
    let session_id = payload.session_id.trim();
    if uid.is_empty() || session_id.is_empty() {
        return Err(ResponseError::bad_request("uid / session_id required"));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid).await?;
    ensure_message_session_belongs_to_uid_with_type(&state, uid, session_id, payload.session_type)
        .await?;

    let upstream = if payload.session_type == 1 {
        if !payload.msg_id.trim().is_empty() && !payload.peer_uid.trim().is_empty() {
            let correlation_group_id = state
                .storage()
                .get_correlation_group_id(uid)
                .await
                .unwrap_or_default();
            let mut req = json!({
                "sessionId": session_id,
                "msgId": payload.msg_id.trim(),
                "peerUid": str_to_json_number(payload.peer_uid.trim()),
            });
            if !correlation_group_id.is_empty() {
                req.as_object_mut().unwrap().insert(
                    "correlationGroupId".to_string(),
                    json!(correlation_group_id),
                );
            }
            successful_message_protocol_h5_request(
                &state,
                uid,
                opcodes::MESSAGE_RELATION_READ,
                json!({ "req": req }),
                vec![],
            )
            .await
            .unwrap_or_else(|_| json!({"fallback": true}))
        } else {
            json!({"fallback": true, "reason": "missing msg id or peer uid"})
        }
    } else {
        successful_message_protocol_h5_request(
            &state,
            uid,
            opcodes::MESSAGE_SYSTEM_UNREAD_CLEAN,
            json!({
                "sessionId": str_to_json_number(session_id),
            }),
            vec![],
        )
        .await
        .unwrap_or_else(|_| json!({"fallback": true}))
    };

    let _ = sqlx::query(
        "UPDATE message_live_state SET unread_count = 0 WHERE uid = ?1 AND session_id = ?2",
    )
    .bind(uid)
    .bind(session_id)
    .execute(state.storage().live_state())
    .await;

    let _ = sqlx::query(
        "UPDATE message_history_cache \
         SET unread_count = 0, \
             cached_at = unixepoch('subsec') \
         WHERE uid = ?1 AND session_id = ?2 AND is_self = 0 AND unread_count > 0",
    )
    .bind(uid)
    .bind(session_id)
    .execute(state.storage().live_state())
    .await;

    Ok(ResponseJson(json!({
        "ok": true,
        "upstream": upstream,
    })))
}

pub async fn pc_session_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SessionInfoQuery>,
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
    let cached = super::load_single_session_info(
        state.storage().live_state(),
        &query.uid,
        &query.session_id,
        query.session_type,
    )
    .await
    .map_err(ResponseError::internal)?;

    if let Ok(info) = hydrate_message_session_info_from_upstream(
        &state,
        &query.uid,
        &query.session_id,
        query.session_type,
    )
    .await
    {
        if info_has_identity(&info) || cached.is_none() {
            return Ok(ResponseJson(message_session_identity_to_value(&info)));
        }
    }

    let info = cached.ok_or_else(|| ResponseError::not_found("session not found"))?;
    Ok(ResponseJson(info))
}

/// GET /api/messages/pc/headinfo — proxy `mtop.idle.trade.wx.message.headinfo`
/// to fetch the product card shown at the top of a chat detail page.
/// Returns the upstream `messageHeadVO` with price, pic, description etc.
pub async fn pc_message_headinfo(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<HeadinfoQuery>,
) -> ApiResult {
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        query.uid.trim(),
    )
    .await?;
    let cached_session = ensure_message_session_belongs_to_uid_with_type(
        &state,
        query.uid.trim(),
        query.session_id.trim(),
        default_session_type(),
    )
    .await?;
    ensure_headinfo_matches_cached_session(&cached_session, &query)?;
    let clean_peer_uid = query.peer_uid.trim().replace('*', "");
    let mut data = json!({
        "itemId": str_to_json_number(&query.item_id),
        "sessionId": query.session_id,
    });
    if !clean_peer_uid.is_empty() {
        data["peerUserId"] = str_to_json_number(&clean_peer_uid);
    }

    let payload = successful_message_protocol_h5_request(
        &state,
        &query.uid,
        opcodes::MESSAGE_TRADE_HEADINFO,
        data,
        vec![],
    )
    .await
    .map_err(ResponseError::internal)?;

    let data = payload
        .get("data")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));
    let head = data
        .get("messageHeadVO")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));

    Ok(ResponseJson(json!({
        "headInfo": head,
        "itemProperty": data.get("itemProperty"),
        "hideHeader": data.get("hideHeader").and_then(Value::as_bool).unwrap_or(false),
    })))
}

fn ensure_headinfo_matches_cached_session(
    cached_session: &Value,
    query: &HeadinfoQuery,
) -> Result<(), ResponseError> {
    let cached_item_id = json_string(cached_session.get("itemId"));
    let requested_item_id = query.item_id.trim();
    if !cached_item_id.trim().is_empty()
        && !requested_item_id.is_empty()
        && cached_item_id.trim() != requested_item_id
    {
        return Err(ResponseError::not_found("会话商品不匹配"));
    }

    let cached_peer_uid = json_string(cached_session.get("peerUid")).replace('*', "");
    let requested_peer_uid = query.peer_uid.trim().replace('*', "");
    if !cached_peer_uid.trim().is_empty()
        && !requested_peer_uid.is_empty()
        && cached_peer_uid.trim() != requested_peer_uid
    {
        return Err(ResponseError::not_found("会话客户不匹配"));
    }

    Ok(())
}
