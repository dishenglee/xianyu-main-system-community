//! Read-receipts + message recall (撤回).
//!
//!  • `POST /api/messages/session/read` → upstream
//!    `mtop.taobao.idlemessage.message.read` v1.0 (HTTP, h5 channel).
//!    body: `{fishAdCode, gps, latitude, longitude,
//!            req: "{sessionId, msgIdList:[...], peerUid:<i64>}"}`
//!    response: `{}` on success. Supports batch via `msgIdList`.
//!
//!  • `POST /api/messages/recall` → ACCS WebSocket push (NOT HTTP MTOP).
//!    LWP topic `/r/MessageManager/recallMessageByFeature`, captured from
//!    `wss-goofish.dingtalk.com` PC web. body:
//!    `[{operater:{displayName:<sid>}}, "<PNM>", {showRecallStatusSetting:1,
//!      operatorType:0, operatorUid:<sid>, code:"200",
//!      extension:{originContentType:"30", textContent:""}}]`
//!    Response: `{"code":200}` (no body) keyed by the request's `mid`.
//!    The HTTP recall API is
//!    APP-channel-only (h5 gateway returns FAIL_SYS_UNAUTHORIZED_ENTRANCE),
//!    so going over WS avoids the sign-service hop entirely.
//!
//! Local effects:
//!  - read: zero `unread_count` for the session
//!  - recall: rewrite the cached message to `kind="recalled"` so the existing
//!    "kind != text/image → centered pill" rendering picks it up

use axum::{
    extract::{Extension, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashSet;

use crate::{
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    routes::accounts::runtime_access::{
        ensure_account_can_call_upstream, load_account_owner_username,
    },
    routes::live::wait_for_live_sender,
    routes::operations::{append_operation_log, bump_message_daily_stat},
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
    xianyu_client,
};

use super::{
    ensure_message_session_belongs_to_uid, json_string, load_single_session_info,
    str_to_json_number, successful_message_protocol_h5_request, ApiResult, ResponseError,
};

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LiveMarkReadPayload {
    pub uid: String,
    pub session_id: String,
    /// Optional — kept for backwards compat with frontend, no longer used.
    #[serde(default)]
    pub last_read_version: i64,
    /// One or more message ids to mark as read. The upstream API accepts a
    /// list, so the frontend can pass all currently-visible unread message
    /// ids in a single call. Singular `msg_id` is also accepted for callers
    /// that only have the latest message id.
    #[serde(default)]
    pub msg_id_list: Vec<String>,
    #[serde(default)]
    pub msg_id: String,
    /// Peer uid as string (matches `pc_session_info.peerUid` after stripping
    /// the `@goofish` suffix). Will be coerced to i64 in the upstream `req`
    /// payload.
    #[serde(default)]
    pub peer_uid: String,
}

#[derive(Debug, Deserialize)]
pub struct LiveRecallMessagePayload {
    pub uid: String,
    pub session_id: String,
    /// Our internal message id (UUID format from `extJson.messageId`,
    /// used as DB key + dedup key client-side).
    pub message_id: String,
    /// 闲鱼平台原始 messageId (PNM 格式，"<seq>.PNM")。撤回 API 唯一认的格式。
    /// 前端从 `message.upstreamMessageId` 拿到，如果客户端没传，后端会拿
    /// `message_id` 在 cache 里查找对应的 PNM。
    #[serde(default)]
    pub upstream_message_id: String,
}

// ---------------------------------------------------------------------------
// POST /api/messages/session/read
// ---------------------------------------------------------------------------

async fn resolve_session_read_msg_ids(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    requested_ids: &[String],
    cached_latest_msg_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let unread_rows = sqlx::query(
        r#"
        SELECT message_id, upstream_message_id
        FROM message_history_cache
        WHERE uid = ? AND session_id = ?
          AND is_self = 0
          AND unread_count > 0
        ORDER BY version ASC, ts ASC
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .fetch_all(pool)
    .await?;

    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    for row in unread_rows {
        let upstream_message_id = row.get::<String, _>("upstream_message_id");
        let message_id = row.get::<String, _>("message_id");
        let target = if !upstream_message_id.trim().is_empty() {
            upstream_message_id
        } else {
            message_id
        };
        if !target.trim().is_empty() && seen.insert(target.clone()) {
            resolved.push(target);
        }
    }
    if !resolved.is_empty() {
        return Ok(resolved);
    }

    let mut candidates = requested_ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect::<Vec<_>>();
    let cached_latest_msg_id = cached_latest_msg_id.trim();
    if !cached_latest_msg_id.is_empty() {
        candidates.push(cached_latest_msg_id.to_string());
    }

    for candidate in candidates {
        let row = sqlx::query(
            r#"
            SELECT message_id, upstream_message_id
            FROM message_history_cache
            WHERE uid = ? AND session_id = ?
              AND (message_id = ? OR upstream_message_id = ?)
            ORDER BY version DESC, ts DESC
            LIMIT 1
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(&candidate)
        .bind(&candidate)
        .fetch_optional(pool)
        .await?;

        let target = row
            .map(|row| {
                let upstream_message_id = row.get::<String, _>("upstream_message_id");
                let message_id = row.get::<String, _>("message_id");
                if !upstream_message_id.trim().is_empty() {
                    upstream_message_id
                } else {
                    message_id
                }
            })
            .unwrap_or(candidate);
        if !target.trim().is_empty() && seen.insert(target.clone()) {
            resolved.push(target);
        }
    }

    Ok(resolved)
}

async fn apply_local_session_read_state(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE message_history_cache \
         SET unread_count = 0, cached_at = unixepoch('subsec') \
         WHERE uid = ?1 AND session_id = ?2 AND is_self = 0 AND unread_count > 0",
    )
    .bind(uid)
    .bind(session_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "UPDATE message_live_state \
         SET unread_count = 0, \
             updated_at = unixepoch('subsec') \
         WHERE uid = ?1 AND session_id = ?2",
    )
    .bind(uid)
    .bind(session_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Mark messages in a chat session as read.
///
/// Confirmed via packet capture (output_readrecall/capture_20260513_155848.jsonl):
///
/// ```text
/// POST mtop.taobao.idlemessage.message.read v1.0
/// data: {
///   "fishAdCode": "430202",
///   "gps": "27.867268,113.164474",
///   "latitude": "27.867268",
///   "longitude": "113.164474",
///   "req": "{\"sessionId\":\"...\",\"msgIdList\":[\"...PNM\",...],\"peerUid\":<i64>}"
/// }
/// response: {}  (empty object = success)
/// ```
///
/// Falls back to the latest cached message id + cached peerUid when the
/// caller doesn't supply them, so the frontend can call with just
/// `(uid, session_id)` after entering a session.
pub async fn live_mark_session_read(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    ResponseJson(payload): ResponseJson<LiveMarkReadPayload>,
) -> ApiResult {
    let uid = payload.uid.trim().to_string();
    let session_id = payload.session_id.trim().to_string();
    if uid.is_empty() || session_id.is_empty() {
        return Err(ResponseError::bad_request("uid / session_id required"));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    let account_owner = load_account_owner_username(&state, &uid).await?;
    ensure_account_can_call_upstream(&state, account_owner.as_deref(), &uid).await?;
    ensure_message_session_belongs_to_uid(&state, &uid, &session_id).await?;

    // Resolve msgIdList + peerUid: prefer the request, fall back to the
    // cached session row so the frontend can call this with just
    // `(uid, session_id)` when it doesn't already track the latest cursor.
    let (msg_id_list, peer_uid) = {
        let cached = load_single_session_info(state.storage().live_state(), &uid, &session_id, 1)
            .await
            .ok()
            .flatten();

        let cached_msg_id = cached
            .as_ref()
            .and_then(|v| v.get("latestMessageId"))
            .map(|v| json_string(Some(v)))
            .unwrap_or_default();
        let cached_peer_uid = cached
            .as_ref()
            .and_then(|v| v.get("peerUid"))
            .map(|v| json_string(Some(v)))
            .unwrap_or_default();

        let requested_ids: Vec<String> = payload
            .msg_id_list
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let singular = payload.msg_id.trim();
        let mut requested_ids = requested_ids;
        if !singular.is_empty() {
            requested_ids.push(singular.to_string());
        }
        let list = resolve_session_read_msg_ids(
            state.storage().live_state(),
            &uid,
            &session_id,
            &requested_ids,
            &cached_msg_id,
        )
        .await
        .map_err(ResponseError::internal)?;
        let peer = if !payload.peer_uid.trim().is_empty() {
            payload.peer_uid.trim().replace('*', "")
        } else {
            cached_peer_uid
        };
        (list, peer)
    };

    // Best-effort upstream call. Server returns `{}` on success. Even if it
    // fails we still want the local DB updated so the UI doesn't show a stale
    // unread badge.
    let location = xianyu_client::load_location(state.storage().accounts(), &uid)
        .await
        .unwrap_or_default();
    let upstream = if !msg_id_list.is_empty() && !peer_uid.is_empty() {
        // `req` is a STRINGIFIED JSON inside the outer object — exactly as
        // the official app sends it. `peerUid` is encoded as a number.
        let req_obj = json!({
            "sessionId": session_id,
            "msgIdList": msg_id_list,
            "peerUid": str_to_json_number(&peer_uid),
        });
        let req_str = serde_json::to_string(&req_obj).unwrap_or_default();
        successful_message_protocol_h5_request(
            &state,
            &uid,
            opcodes::MESSAGE_READ,
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "latitude": location.latitude,
                "longitude": location.longitude,
                "req": req_str,
            }),
            vec![],
        )
        .await
        .unwrap_or_else(|err| json!({"fallback": true, "error": err}))
    } else {
        json!({"fallback": true, "reason": "missing msg id list or peer_uid"})
    };

    // Local read proof: clear both the session badge and cached incoming
    // message unread flags. Otherwise a later history refresh can rebuild the
    // unread badge from stale per-message unread_count rows.
    let _ = apply_local_session_read_state(state.storage().live_state(), &uid, &session_id).await;

    Ok(ResponseJson(json!({
        "ok": true,
        "uid": uid,
        "sessionId": session_id,
        "msgCount": msg_id_list.len(),
        "upstream": upstream,
    })))
}

// ---------------------------------------------------------------------------
// POST /api/messages/recall
// ---------------------------------------------------------------------------

/// Recall (撤回) one of the user's own outgoing messages.
///
/// Pushes `/r/MessageManager/recallMessageByFeature` over the user's active
/// ACCS WebSocket — same path PC web (goofish.com/im) uses, captured via
/// Playwright WS frame interception on 2026-05-15. The HTTP MTOP API
/// The recall endpoint is APP-channel-only at the
/// gateway (h5 returns `FAIL_SYS_UNAUTHORIZED_ENTRANCE`), and PC web does
/// NOT call it — it only ever uses the WS LWP topic.
///
/// `message_id` is our internal UUID (DB primary key). The upstream API
/// requires PNM format; we accept it on the payload as `upstream_message_id`
/// or look it up from `message_history_cache.upstream_message_id`.
pub async fn live_recall_message(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    ResponseJson(payload): ResponseJson<LiveRecallMessagePayload>,
) -> ApiResult {
    let uid = payload.uid.trim().to_string();
    let session_id = payload.session_id.trim().to_string();
    let message_id = payload.message_id.trim().to_string();
    if uid.is_empty() || session_id.is_empty() || message_id.is_empty() {
        return Err(ResponseError::bad_request(
            "uid / session_id / message_id required",
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), &uid).await?;
    ensure_message_session_belongs_to_uid(&state, &uid, &session_id).await?;

    // 撤回必须用 PNM 格式 messageId（"<seq>.PNM"），不是我们做主键的 UUID。
    // 优先用前端传的，没有就在 cache 里按 (uid, session_id, message_id) 查。
    let upstream_message_id = if !payload.upstream_message_id.trim().is_empty() {
        payload.upstream_message_id.trim().to_string()
    } else {
        let pool = state.storage().live_state();
        sqlx::query_scalar::<_, String>(
            "SELECT upstream_message_id FROM message_history_cache \
             WHERE uid = ?1 AND session_id = ?2 AND message_id = ?3 \
             LIMIT 1",
        )
        .bind(&uid)
        .bind(&session_id)
        .bind(&message_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| message_id.clone())
    };

    // 拿 ACCS WS sender —— 没 live socket 就 503，跟 send_text 一致。
    let sender = wait_for_live_sender(&state, &uid).await?;

    // Register the response waiter BEFORE sending the frame, so the worker's
    // frame dispatcher can't beat us to the wake.
    let mid = super::live_ops::generate_accs_mid();
    let (tx, rx) = tokio::sync::oneshot::channel::<Value>();
    state
        .live_response_waiters()
        .register(&uid, &mid, tx)
        .await
        .map_err(|_| ResponseError::service_unavailable("消息撤回请求标识已被占用，请重试"))?;

    // 抓包验证过的 payload 形状（PC web wss-goofish.dingtalk.com，2026-05-15）。
    // operatorUid / displayName 都用 sessionId —— PC web 自己就是这么发的，
    // 上游 200 OK。手拼字符串是为了固定 lwp/headers/body 顺序：serde_json 的
    // BTreeMap 序列化会按字母序输出，服务端对这个 RPC 也是 byte-strict。
    let esc_mid = super::live_ops::escape_json_string(&mid);
    let esc_pnm = super::live_ops::escape_json_string(&upstream_message_id);
    let esc_sid = super::live_ops::escape_json_string(&session_id);
    let ws_payload_str = format!(
        concat!(
            r#"{{"lwp":"/r/MessageManager/recallMessageByFeature","headers":{{"mid":"{mid}"}},"body":["#,
            r#"{{"operater":{{"displayName":"{sid}"}}}},"#,
            r#""{pnm}","#,
            r#"{{"showRecallStatusSetting":1,"operatorType":0,"operatorUid":"{sid}","code":"200","extension":{{"originContentType":"30","textContent":""}}}}"#,
            r#"]}}"#,
        ),
        mid = esc_mid,
        sid = esc_sid,
        pnm = esc_pnm,
    );

    if sender.send(ws_payload_str).is_err() {
        let _ = state.live_response_waiters().take(&uid, &mid).await;
        state
            .live_senders()
            .unregister_if_matches(&uid, &sender)
            .await;
        return Err(ResponseError::service_unavailable(
            "live socket dropped while sending recall",
        ));
    }

    // 等服务端响应。同一 mid 的 inbound frame 在 worker.rs:486-509 被路由到
    // 我们这个 oneshot。撤回响应是 `{headers:{mid,...}, code: 200}`（没 body）。
    // 任何 non-200 code（如 400/403）说明上游拒了，必须 surfaces 给前端，
    // 否则就是"假撤回" — 本地改了 bubble，对方那边其实还在。
    let upstream_payload = match tokio::time::timeout(std::time::Duration::from_secs(20), rx).await
    {
        Ok(Ok(resp)) => resp,
        Ok(Err(_)) => {
            let _ = state.live_response_waiters().take(&uid, &mid).await;
            tracing::warn!(
                uid = %uid, session_id = %session_id, message_id = %message_id,
                "recall WS waiter dropped before response"
            );
            return Err(ResponseError::bad_request(
                "撤回失败：live socket 在等待响应期间断开",
            ));
        }
        Err(_) => {
            let _ = state.live_response_waiters().take(&uid, &mid).await;
            tracing::warn!(
                uid = %uid, session_id = %session_id, message_id = %message_id,
                "recall WS timeout 20s"
            );
            return Err(ResponseError::bad_request("撤回失败：上游 20s 内未响应"));
        }
    };

    let upstream_code = upstream_payload
        .get("code")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if upstream_code != 200 {
        let reason = upstream_payload
            .get("body")
            .and_then(|b| b.get("reason"))
            .and_then(Value::as_str)
            .or_else(|| {
                upstream_payload
                    .get("body")
                    .and_then(|b| b.get("developerMessage"))
                    .and_then(Value::as_str)
            })
            .unwrap_or("");
        tracing::warn!(
            uid = %uid, session_id = %session_id, message_id = %message_id,
            upstream_message_id = %upstream_message_id,
            code = upstream_code, %reason,
            "recall WS upstream non-200"
        );
        return Err(ResponseError::bad_request(format!(
            "撤回失败（消息可能超过 2 分钟、已被撤回，或会话异常）：code={upstream_code} reason={reason}"
        )));
    }

    // Local rewrite: flip the cached row to `kind="recalled"` so the existing
    // "kind != text/image → centered pill" rendering picks it up. Stamp
    // `text` to "你撤回了一条消息" for the preview/summary fallbacks too.
    // 同时把原 text/image_url 备份到 recalled_original_* 列：反撤回价值的核心
    // ——pill 下方渲染原内容给用户看。COALESCE 防止重复 UPDATE 把备份覆盖空。
    let recall_text = "你撤回了一条消息";
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let update_result = sqlx::query(
        "UPDATE message_history_cache \
         SET recalled_at = ?1, \
             recalled_original_text = CASE \
                 WHEN TRIM(COALESCE(recalled_original_text, '')) = '' THEN text \
                 ELSE recalled_original_text \
             END, \
             recalled_original_image_url = CASE \
                 WHEN TRIM(COALESCE(recalled_original_image_url, '')) = '' THEN image_url \
                 ELSE recalled_original_image_url \
             END, \
             kind = 'recalled', \
             text = ?2, \
             title = '', \
             image_url = '', \
             image_urls = '[]', \
             preview = ?2 \
         WHERE uid = ?3 AND session_id = ?4 AND message_id = ?5",
    )
    .bind(now_ms.max(1))
    .bind(recall_text)
    .bind(&uid)
    .bind(&session_id)
    .bind(&message_id)
    .execute(state.storage().live_state())
    .await;
    let rows = update_result
        .as_ref()
        .map(|r| r.rows_affected())
        .unwrap_or(0);

    // Refresh the session summary too — if the recalled message is the
    // latest in the session, the list preview should reflect the recall.
    let _ = sqlx::query(
        "UPDATE message_live_state \
         SET summary = ?1, \
             updated_at = unixepoch('subsec') \
         WHERE uid = ?2 AND session_id = ?3 \
           AND TRIM(latest_message_id) <> '' \
           AND latest_message_id LIKE ?4",
    )
    .bind(recall_text)
    .bind(&uid)
    .bind(&session_id)
    .bind(format!("%{}%", &message_id))
    .execute(state.storage().live_state())
    .await;

    bump_message_daily_stat(&state, &uid, "recall_count", 1).await;
    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "message",
            action: "manual_recall_message",
            operator: &uid,
            trigger_source: "manual",
            uid: &uid,
            account_nick: "",
            target_type: "message",
            target_id: &message_id,
            target_title: &session_id,
            status: "success",
            error_code: "",
            message: "手动撤回消息成功",
            params: json!({
                "session_id": session_id,
                "upstream_message_id": upstream_message_id,
            }),
            before: json!({}),
            after: json!({
                "recalled": true,
            }),
            extra: upstream_payload.clone(),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "ok": true,
        "uid": uid,
        "sessionId": session_id,
        "messageId": message_id,
        "upstreamOk": true,
        "upstream": upstream_payload,
        "rowsUpdated": rows,
    })))
}
