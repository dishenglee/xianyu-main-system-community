//! Process, hydrate and persist `LiveMessageEvent` to the live-state database.

use std::sync::OnceLock;

use serde_json::{json, Value};
use sqlx::Row;
use tracing::{debug, info, warn};

use crate::{logging::env_flag_enabled, routes::error::ResponseError, state::AppState};

use crate::routes::accounts::messages::{
    hydrate_message_session_info_from_upstream, json_string, load_single_session_info,
    normalize_message_history, peer_nick_needs_fallback, persist_message_history_page,
    persist_message_sessions, LiveMessageEvent, LiveSessionCursor, MessageHistoryEntry,
    MessageHistoryPage, MessageSessionIdentity,
};

use super::event_parser::{
    decode_wx_sync_data, extract_sync_push_data, live_message_preview, parse_live_sync_messages,
};
use super::ws_sync::fetch_message_history_via_active_socket;

#[derive(Debug, Clone, Default)]
struct LiveDiagnosticCapture {
    matched: bool,
    include_payloads: bool,
    username: String,
    device_id: String,
}

fn live_message_event_type(event: &LiveMessageEvent) -> &'static str {
    if event.is_self {
        "message.sent"
    } else {
        "message.received"
    }
}

fn live_debug_logs_enabled() -> bool {
    if !env_flag_enabled("XIANYU_LIVE_DEBUG") {
        return false;
    }
    if std::env::var("APP_RUNTIME_MODE")
        .ok()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("debug"))
    {
        return true;
    }

    static WARN_ONCE: OnceLock<()> = OnceLock::new();
    WARN_ONCE.get_or_init(|| {
        warn!(
            target: "live.event_processor",
            stream = "runtime",
            module = "live-event-processor",
            action = "diagnostic_bypass_ignored",
            outcome = "skipped",
            error_code = "diagnostic_bypass_disabled",
            "ignoring XIANYU_LIVE_DEBUG outside debug runtime mode"
        );
    });
    false
}

async fn live_diagnostic_capture(
    state: &AppState,
    module: &str,
    uid: &str,
    session_id: Option<&str>,
    sampling_key: Option<&str>,
) -> LiveDiagnosticCapture {
    let username = sqlx::query_scalar::<_, String>(
        "SELECT owner_username FROM accounts WHERE uid = ? LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(state.storage().accounts())
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let device_id = crate::license_runtime::get_sign_license_context()
        .map(|context| context.device_id)
        .unwrap_or_default();

    if live_debug_logs_enabled() {
        return LiveDiagnosticCapture {
            matched: true,
            include_payloads: true,
            username,
            device_id,
        };
    }

    let decision = state
        .storage()
        .runtime_diagnostic_policy_decision(
            Some(module),
            (!username.trim().is_empty()).then_some(username.as_str()),
            (!device_id.trim().is_empty()).then_some(device_id.as_str()),
            Some(uid),
            session_id.filter(|value| !value.trim().is_empty()),
            None,
            sampling_key.or(Some(uid)),
        )
        .await
        .ok();

    LiveDiagnosticCapture {
        matched: decision
            .as_ref()
            .and_then(|value| value.get("matched"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        include_payloads: decision
            .as_ref()
            .and_then(|value| value.get("includePayloads"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        username,
        device_id,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveEventOrigin {
    Realtime,
    HistoryReplay,
}

pub(super) fn live_message_event_to_value(uid: &str, event: &LiveMessageEvent) -> Value {
    let latest_msg_id = if event.accs_pts > 0 {
        format!("{}.PNM", event.accs_pts)
    } else {
        event.message_id.clone()
    };
    // Expand the stored `card_meta` JSON string into a nested object so the
    // frontend can read `message.cardMeta.title` etc. directly without
    // re-parsing. Empty / unparseable → null (mirrors history_row_to_value).
    let card_meta = if event.card_meta.is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(&event.card_meta).unwrap_or(Value::Null)
    };
    json!({
        "type": live_message_event_type(event),
        "uid": uid,
        "sessionId": event.session_id,
        "sessionType": event.session_type,
        "messageId": event.message_id,
        "latestMessageId": latest_msg_id,
        "version": event.version,
        "ts": event.ts,
        "peerUid": event.peer_uid,
        "peerNick": event.peer_nick,
        "peerLogo": event.peer_logo,
        "peerRole": event.peer_role,
        "selfRole": event.self_role,
        "itemId": event.item_id,
        "itemTitle": event.item_title,
        "itemPic": event.item_pic,
        "kind": event.kind,
        "title": event.title,
        "text": event.text,
        "imageUrl": event.image_url,
        "rawPreview": event.preview,
        "senderUid": event.sender_uid,
        "senderNick": event.sender_nick,
        "senderLogo": event.sender_logo,
        "isSelf": event.is_self,
        "unreadCount": event.unread_count,
        "contentType": event.content_type,
        "cardMeta": card_meta,
    })
}

pub(super) async fn load_live_session_cursor(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
) -> Result<Option<LiveSessionCursor>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT latest_version, latest_message_id, unread_count
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| LiveSessionCursor {
        latest_version: row.get::<i64, _>("latest_version"),
        latest_message_id: row.get::<String, _>("latest_message_id"),
        unread_count: row.get::<i64, _>("unread_count"),
    }))
}

async fn load_live_session_last_message_ts(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
) -> Result<Option<i64>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT last_message_ts
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|row| row.get::<i64, _>("last_message_ts")))
}

async fn session_metadata_needs_persist(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session: &MessageSessionIdentity,
) -> Result<bool, sqlx::Error> {
    let Some(row) = sqlx::query(
        r#"
        SELECT
            session_type,
            latest_version,
            peer_uid,
            peer_nick,
            peer_role,
            peer_logo,
            self_role,
            item_id,
            item_title,
            item_pic,
            last_message_ts
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(&session.session_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(true);
    };

    let existing_session_type = row.get::<i64, _>("session_type");
    let existing_version = row.get::<i64, _>("latest_version");
    let existing_ts = row.get::<i64, _>("last_message_ts");
    if session.session_type != existing_session_type
        || session.version > existing_version
        || session.ts > existing_ts
    {
        return Ok(true);
    }

    fn incoming_changes(row: &sqlx::sqlite::SqliteRow, column: &str, incoming: &str) -> bool {
        let incoming = incoming.trim();
        if incoming.is_empty() {
            return false;
        }
        row.get::<String, _>(column).trim() != incoming
    }

    Ok(incoming_changes(&row, "peer_uid", &session.peer_uid)
        || incoming_changes(&row, "peer_nick", &session.peer_nick)
        || incoming_changes(&row, "peer_role", &session.peer_role)
        || incoming_changes(&row, "peer_logo", &session.peer_logo)
        || incoming_changes(&row, "self_role", &session.self_role)
        || incoming_changes(&row, "item_id", &session.item_id)
        || incoming_changes(&row, "item_title", &session.item_title)
        || incoming_changes(&row, "item_pic", &session.item_pic))
}

async fn history_message_exists(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    version: i64,
    message_id: &str,
    upstream_message_id: &str,
) -> Result<bool, sqlx::Error> {
    if version <= 0 {
        return Ok(false);
    }
    if message_id.trim().is_empty() && upstream_message_id.trim().is_empty() {
        return Ok(false);
    }

    let row = sqlx::query(
        r#"
        SELECT 1
        FROM message_history_cache
        WHERE uid = ?1
          AND session_id = ?2
          AND version = ?3
          AND (
            (TRIM(?4) <> '' AND message_id = ?4)
            OR (TRIM(?5) <> '' AND upstream_message_id = ?5)
          )
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(version)
    .bind(message_id)
    .bind(upstream_message_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.is_some())
}

async fn history_message_exists_any_identity(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    message_id: &str,
    upstream_message_id: &str,
) -> Result<bool, sqlx::Error> {
    if message_id.trim().is_empty() && upstream_message_id.trim().is_empty() {
        return Ok(false);
    }

    let row = sqlx::query(
        r#"
        SELECT 1
        FROM message_history_cache
        WHERE uid = ?1
          AND session_id = ?2
          AND (
            (TRIM(?3) <> '' AND message_id = ?3)
            OR (TRIM(?4) <> '' AND upstream_message_id = ?4)
          )
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(message_id)
    .bind(upstream_message_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.is_some())
}

fn merge_live_event_with_identity(event: &mut LiveMessageEvent, session: &Value) {
    let peer_uid = json_string(session.get("peerUid"));
    let peer_nick = json_string(session.get("peerNick"));
    let peer_logo = json_string(session.get("peerLogo"));
    let peer_role = json_string(session.get("peerRole"));
    let self_role = json_string(session.get("selfRole"));
    let item_id = json_string(session.get("itemId"));
    let item_title = json_string(session.get("itemTitle"));
    let item_pic = json_string(session.get("itemPic"));

    if event.peer_uid.trim().is_empty() {
        event.peer_uid = peer_uid;
    }
    if peer_nick_needs_fallback(&event.peer_nick, &event.peer_uid) {
        event.peer_nick = peer_nick;
    }
    if event.peer_logo.trim().is_empty() {
        event.peer_logo = peer_logo;
    }
    if event.peer_role.trim().is_empty() {
        event.peer_role = peer_role;
    }
    if event.self_role.trim().is_empty() {
        event.self_role = self_role;
    }
    if event.item_id.trim().is_empty() {
        event.item_id = item_id;
    }
    if event.item_title.trim().is_empty() {
        event.item_title = item_title;
    }
    if event.item_pic.trim().is_empty() {
        event.item_pic = item_pic;
    }
}

fn live_event_needs_session_hydration(event: &LiveMessageEvent) -> bool {
    peer_nick_needs_fallback(&event.peer_nick, &event.peer_uid)
        || event.peer_uid.trim().is_empty()
        || event.peer_logo.trim().is_empty()
        || event.peer_role.trim().is_empty()
        || event.self_role.trim().is_empty()
        || event.item_id.trim().is_empty()
        || event.item_title.trim().is_empty()
        || event.item_pic.trim().is_empty()
}

/// Fast local-only hydration: merge cached session identity into the event.
/// Does NOT call any upstream HTTP APIs so it completes in <1ms.
async fn hydrate_live_message_event_from_cache(
    state: &AppState,
    uid: &str,
    event: &mut LiveMessageEvent,
) -> Result<(), ResponseError> {
    if let Some(cached) = load_single_session_info(
        state.storage().live_state(),
        uid,
        &event.session_id,
        event.session_type,
    )
    .await
    .map_err(ResponseError::internal)?
    {
        merge_live_event_with_identity(event, &cached);
    }
    Ok(())
}

/// Spawn a background task to hydrate missing session fields (peer nick/logo,
/// item info) from upstream HTTP APIs. This does NOT block the WS event
/// processing pipeline — the event gets emitted immediately with whatever
/// data is available from the WS push + local DB cache.
fn spawn_background_hydration(state: &AppState, uid: &str, event: &LiveMessageEvent) {
    let state = state.clone();
    let uid = uid.to_string();
    let session_id = event.session_id.clone();
    let session_type = event.session_type;
    tokio::spawn(async move {
        match hydrate_message_session_info_from_upstream(&state, &uid, &session_id, session_type)
            .await
        {
            Ok(info) => {
                debug!(
                    target: "live.event_processor",
                    stream = "runtime",
                    module = "live-event-processor",
                    action = "background_hydration",
                    outcome = "success",
                    account_uid = %uid,
                    session_id = %session_id,
                    peer_nick = ?info.peer_nick,
                    peer_logo_present = !info.peer_logo.is_empty(),
                    "hydrated live message session metadata in background"
                );
            }
            Err(e) => {
                warn!(
                    target: "live.event_processor",
                    stream = "runtime",
                    module = "live-event-processor",
                    action = "background_hydration",
                    outcome = "warn",
                    account_uid = %uid,
                    session_id = %session_id,
                    error_code = "session_hydration_failed",
                    retryable = true,
                    detail = %e,
                    "background hydration for live message session failed"
                );
            }
        }
    });
}

fn merge_session_metadata_seed_into_history_page(
    page: &mut MessageHistoryPage,
    seed: &LiveMessageEvent,
) {
    if page.session.session_id.trim().is_empty() {
        page.session.session_id = seed.session_id.clone();
    }
    if page.session.session_type <= 0 {
        page.session.session_type = seed.session_type.max(1);
    }
    if page.session.peer_uid.trim().is_empty() {
        page.session.peer_uid = seed.peer_uid.clone();
    }
    if peer_nick_needs_fallback(&page.session.peer_nick, &page.session.peer_uid)
        && !seed.peer_nick.trim().is_empty()
    {
        page.session.peer_nick = seed.peer_nick.clone();
    }
    if page.session.peer_logo.trim().is_empty() {
        page.session.peer_logo = seed.peer_logo.clone();
    }
    if page.session.peer_role.trim().is_empty() {
        page.session.peer_role = seed.peer_role.clone();
    }
    if page.session.self_role.trim().is_empty() {
        page.session.self_role = seed.self_role.clone();
    }
    if page.session.item_id.trim().is_empty() {
        page.session.item_id = seed.item_id.clone();
    }
    if page.session.item_title.trim().is_empty() {
        page.session.item_title = seed.item_title.clone();
    }
    if page.session.item_pic.trim().is_empty() {
        page.session.item_pic = seed.item_pic.clone();
    }
    if page.session.version <= 0 {
        page.session.version = page.latest_cursor.max(seed.version).max(seed.ts);
    }
    if page.session.ts <= 0 {
        page.session.ts = page.latest_cursor.max(seed.ts).max(page.session.version);
    }
    if page.session.sort_index <= 0 {
        page.session.sort_index = page.session.version.max(page.session.ts);
    }
    if page.session.summary.trim().is_empty() {
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
}

fn live_event_from_history_entry(
    uid: &str,
    session: &MessageSessionIdentity,
    message: &MessageHistoryEntry,
) -> LiveMessageEvent {
    let version = message.version.max(message.ts).max(1);
    LiveMessageEvent {
        session_id: session.session_id.clone(),
        session_type: session.session_type.max(1),
        message_id: message.message_id.clone(),
        upstream_message_id: message.upstream_message_id.clone(),
        version,
        ts: message.ts.max(version),
        peer_uid: session.peer_uid.clone(),
        peer_nick: session.peer_nick.clone(),
        peer_logo: session.peer_logo.clone(),
        peer_role: session.peer_role.clone(),
        self_role: session.self_role.clone(),
        item_id: session.item_id.clone(),
        item_title: session.item_title.clone(),
        item_pic: session.item_pic.clone(),
        kind: message.kind.clone(),
        title: message.title.clone(),
        text: message.text.clone(),
        image_url: message.image_url.clone(),
        preview: message.preview.clone(),
        sender_uid: if message.is_self {
            uid.to_string()
        } else {
            session.peer_uid.clone()
        },
        sender_nick: message.sender_nick.clone(),
        sender_logo: message.sender_logo.clone(),
        is_self: message.is_self,
        unread_count: message.unread_count,
        accs_pts: 0,
        card_meta: message.card_meta.clone(),
        content_type: message.content_type,
        reply_to_message_id: message.reply_to_message_id.clone(),
    }
}

fn spawn_session_metadata_backfill(state: &AppState, uid: &str, seed: &LiveMessageEvent) {
    let state = state.clone();
    let uid = uid.to_string();
    let seed = seed.clone();
    tokio::spawn(async move {
        let initial_cursor =
            match load_live_session_cursor(state.storage().live_state(), &uid, &seed.session_id)
                .await
            {
                Ok(cursor) => cursor,
                Err(error) => {
                    warn!(
                        target: "live.event_processor",
                        stream = "runtime",
                        module = "live-event-processor",
                        action = "metadata_backfill_cursor",
                        outcome = "warn",
                        account_uid = %uid,
                        session_id = %seed.session_id,
                        error_code = "backfill_cursor_load_failed",
                        retryable = true,
                        detail = %error,
                        "failed to load cursor before metadata backfill"
                    );
                    None
                }
            };
        let previous_version = initial_cursor
            .as_ref()
            .map(|cursor| cursor.latest_version)
            .unwrap_or(0);

        let payload =
            match fetch_message_history_via_active_socket(&state, &uid, &seed.session_id, 20, 0)
                .await
            {
                Ok(payload) => payload,
                Err(error) => {
                    warn!(
                        target: "live.event_processor",
                        stream = "runtime",
                        module = "live-event-processor",
                        action = "metadata_backfill_history",
                        outcome = "warn",
                        account_uid = %uid,
                        session_id = %seed.session_id,
                        error_code = "backfill_history_failed",
                        retryable = true,
                        detail = %error.detail(),
                        "failed to fetch history during metadata backfill"
                    );
                    return;
                }
            };

        let mut page = normalize_message_history(&payload, &uid);
        if page.messages.is_empty() {
            debug!(
                target: "live.event_processor",
                stream = "runtime",
                module = "live-event-processor",
                action = "metadata_backfill_empty_history",
                outcome = "empty",
                account_uid = %uid,
                session_id = %seed.session_id,
                "metadata backfill returned empty history"
            );
            return;
        }
        merge_session_metadata_seed_into_history_page(&mut page, &seed);

        let replay_messages = page
            .messages
            .iter()
            .filter(|message| {
                !message.is_self && message.version.max(message.ts) > previous_version
            })
            .cloned()
            .collect::<Vec<_>>();

        for message in &replay_messages {
            let live_event = live_event_from_history_entry(&uid, &page.session, message);
            match process_live_message_event_with_origin(
                &state,
                &uid,
                live_event,
                LiveEventOrigin::HistoryReplay,
            )
            .await
            {
                Ok(Some(event)) => state.live_events().publish(event),
                Ok(None) => {}
                Err(error) => {
                    warn!(
                        target: "live.event_processor",
                        stream = "runtime",
                        module = "live-event-processor",
                        action = "metadata_backfill_replay",
                        outcome = "warn",
                        account_uid = %uid,
                        session_id = %seed.session_id,
                        error_code = "backfill_replay_failed",
                        retryable = true,
                        detail = %error,
                        "failed to replay metadata backfill history event"
                    );
                }
            }
        }

        if let Ok(Some(cursor)) =
            load_live_session_cursor(state.storage().live_state(), &uid, &seed.session_id).await
        {
            page.session.unread = cursor.unread_count;
            if cursor.latest_version > page.session.version {
                page.session.version = cursor.latest_version;
            }
            if !cursor.latest_message_id.trim().is_empty() {
                page.session.latest_message_id = cursor.latest_message_id;
            }
        }
        page.session.ts = page.session.ts.max(page.session.version).max(seed.ts);
        page.session.sort_index = page
            .session
            .sort_index
            .max(page.session.ts)
            .max(page.session.version);

        if let Err(error) =
            persist_message_history_page(state.storage().live_state(), &uid, &page, true).await
        {
            warn!(
                target: "live.event_processor",
                stream = "runtime",
                module = "live-event-processor",
                action = "metadata_backfill_persist",
                outcome = "warn",
                account_uid = %uid,
                session_id = %seed.session_id,
                error_code = "backfill_persist_failed",
                retryable = true,
                detail = %error,
                "failed to persist metadata backfill page"
            );
            return;
        }

        info!(
            target: "live.event_processor",
            stream = "runtime",
            module = "live-event-processor",
            action = "metadata_backfill_complete",
            outcome = "success",
            account_uid = %uid,
            session_id = %seed.session_id,
            replayed_count = replay_messages.len(),
            latest_version = page.session.version,
            "completed metadata backfill"
        );
    });
}

pub(crate) async fn process_live_message_event(
    state: &AppState,
    uid: &str,
    event: LiveMessageEvent,
) -> Result<Option<Value>, ResponseError> {
    process_live_message_event_with_origin(state, uid, event, LiveEventOrigin::Realtime).await
}

async fn process_live_message_event_with_origin(
    state: &AppState,
    uid: &str,
    mut event: LiveMessageEvent,
    origin: LiveEventOrigin,
) -> Result<Option<Value>, ResponseError> {
    // Drop malformed events that the fallback parser greedily extracted from
    // RPC reply / sync envelopes without a real message context. These have
    // an empty `session_id` and no peer info, just a stray `text` field —
    // emitting them to the frontend produces the dreaded "未知" session row
    // in the message list. We CAN'T even persist them safely (no session_id
    // means we'd overwrite an unrelated session_id="" row in the cache).
    if event.session_id.trim().is_empty() {
        warn!(
            target: "live.event_processor",
            stream = "runtime",
            module = "live-event-processor",
            action = "drop_message_event",
            outcome = "warn",
            account_uid = %uid,
            error_code = "missing_session_id",
            retryable = false,
            kind = %event.kind,
            text_preview = %event.text.chars().take(30).collect::<String>(),
            "dropping malformed live message event without session_id"
        );
        return Ok(None);
    }
    // 撤回事件 — 按 PNM 找原 cache 行，把它改成 kind='recalled' + pill 文本，
    // 同时把原 text/image_url 备份到 recalled_original_* 列，前端在 pill 下方
    // 渲染原内容（反撤回价值）。直接 UPDATE 而不是 UPSERT，因为撤回 event 的
    // version/message_id 跟原消息不一样，UPSERT 会写新行不会改旧行。
    if event.kind == "recalled" && !event.upstream_message_id.trim().is_empty() {
        let recall_text = event.text.clone();
        let pnm = event.upstream_message_id.clone();
        let session_id = event.session_id.clone();
        // 只有第一次撤回时备份原文，后续 UPDATE（重复处理 / 同步推送多次）
        // 不会再覆盖 recalled_original_*。COALESCE(NULLIF(...,''), text) 保证
        // 已经备份过的就保留，没备份过的就把当前 text/image_url 复制过去。
        let updated = sqlx::query(
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
             WHERE uid = ?3 AND session_id = ?4 AND upstream_message_id = ?5",
        )
        .bind(event.ts.max(1))
        .bind(&recall_text)
        .bind(uid)
        .bind(&session_id)
        .bind(&pnm)
        .execute(state.storage().live_state())
        .await
        .map_err(ResponseError::internal)?;

        // Session summary 也同步成「xxx撤回了一条消息」，否则消息列表的
        // 最后一条预览还是旧文本。
        let _ = sqlx::query(
            "UPDATE message_live_state \
             SET summary = ?1, \
                 updated_at = unixepoch('subsec') \
             WHERE uid = ?2 AND session_id = ?3 \
               AND TRIM(latest_message_id) <> '' \
               AND latest_message_id LIKE ?4",
        )
        .bind(&recall_text)
        .bind(uid)
        .bind(&session_id)
        .bind(format!("%{}%", &pnm))
        .execute(state.storage().live_state())
        .await;

        info!(
            target: "live.event_processor",
            stream = "runtime",
            module = "live-event-processor",
            action = "recall_update",
            outcome = "success",
            account_uid = %uid,
            session_id = %session_id,
            upstream_message_id = %pnm,
            is_self = event.is_self,
            rows_updated = updated.rows_affected(),
            "updated recalled live message rows"
        );

        // 即使 rows_affected==0（原行还没缓存到——peer 端可能此时还没拉过
        // 这条消息的 history）也照常 emit 事件，前端的 live SSE 监听会处理。
        return Ok(Some(live_message_event_to_value(uid, &event)));
    }

    // session_metadata events carry only session-level identity (peer, item)
    // from chatType pushes like sessionArouse. Persist them as session info
    // updates without creating a message entry or emitting a frontend event.
    if event.kind == "session_metadata" {
        hydrate_live_message_event_from_cache(state, uid, &mut event).await?;
        if event.session_type == 1 && live_event_needs_session_hydration(&event) {
            spawn_background_hydration(state, uid, &event);
        }
        let cursor = load_live_session_cursor(state.storage().live_state(), uid, &event.session_id)
            .await
            .map_err(ResponseError::internal)?;
        let existing_ts =
            load_live_session_last_message_ts(state.storage().live_state(), uid, &event.session_id)
                .await
                .map_err(ResponseError::internal)?
                .unwrap_or(0);
        let existing_version = cursor.as_ref().map(|row| row.latest_version).unwrap_or(0);
        let activity_version = if existing_version > 0 {
            existing_version
        } else {
            event.version.max(event.ts).max(0)
        };
        let activity_ts = if existing_ts > 0 {
            existing_ts
        } else {
            event.ts.max(activity_version).max(0)
        };
        let session = MessageSessionIdentity {
            session_id: event.session_id.clone(),
            session_type: event.session_type.max(1),
            version: activity_version,
            unread: cursor.as_ref().map(|row| row.unread_count).unwrap_or(0),
            latest_message_id: String::new(),
            peer_nick: event.peer_nick.clone(),
            peer_logo: event.peer_logo.clone(),
            peer_uid: event.peer_uid.clone(),
            peer_role: event.peer_role.clone(),
            self_role: event.self_role.clone(),
            item_id: event.item_id.clone(),
            item_title: event.item_title.clone(),
            item_pic: event.item_pic.clone(),
            ts: activity_ts,
            sort_index: activity_version.max(activity_ts),
            ..Default::default()
        };
        if !session_metadata_needs_persist(state.storage().live_state(), uid, &session)
            .await
            .map_err(ResponseError::internal)?
        {
            return Ok(None);
        }
        persist_message_sessions(state.storage().live_state(), uid, &[session])
            .await
            .map_err(ResponseError::internal)?;
        let should_backfill_history = event.session_type == 1
            && (existing_version <= 0 || event.ts > existing_ts.max(existing_version));
        if should_backfill_history {
            spawn_session_metadata_backfill(state, uid, &event);
        }
        debug!(
            target: "live.event_processor",
            stream = "runtime",
            module = "live-event-processor",
            action = "session_metadata_persisted",
            outcome = "success",
            account_uid = %uid,
            session_id = %event.session_id,
            peer_uid = %event.peer_uid,
            item_id = %event.item_id,
            item_title = ?event.item_title,
            "persisted live session metadata"
        );
        return Ok(None);
    }

    // Fast path: merge from local DB cache (~1ms). This fills peer_logo,
    // peer_nick, item info etc. from previously hydrated sessions.
    hydrate_live_message_event_from_cache(state, uid, &mut event).await?;

    // If essential fields are still missing after cache merge, schedule a
    // background upstream hydration. Previously this was blocking — every
    // incoming message waited 2-5s for HTTP mtop calls before the SSE event
    // reached the frontend. Now the event is emitted immediately and the
    // hydration populates the DB cache for subsequent events.
    if event.session_type == 1 && live_event_needs_session_hydration(&event) {
        spawn_background_hydration(state, uid, &event);
    }

    let cursor = load_live_session_cursor(state.storage().live_state(), uid, &event.session_id)
        .await
        .map_err(ResponseError::internal)?;
    let previous_version = cursor.as_ref().map(|row| row.latest_version).unwrap_or(0);
    let previous_message_id = cursor
        .as_ref()
        .map(|row| row.latest_message_id.as_str())
        .unwrap_or_default();
    if previous_version > event.version {
        return Ok(None);
    }
    if previous_version == event.version
        && !event.message_id.trim().is_empty()
        && event.message_id.trim() == previous_message_id.trim()
    {
        return Ok(None);
    }
    if previous_version == event.version
        && history_message_exists(
            state.storage().live_state(),
            uid,
            &event.session_id,
            event.version,
            &event.message_id,
            &event.upstream_message_id,
        )
        .await
        .map_err(ResponseError::internal)?
    {
        return Ok(None);
    }
    if history_message_exists_any_identity(
        state.storage().live_state(),
        uid,
        &event.session_id,
        &event.message_id,
        &event.upstream_message_id,
    )
    .await
    .map_err(ResponseError::internal)?
    {
        return Ok(None);
    }

    persist_live_message_event(state.storage().live_state(), uid, &event, cursor.as_ref())
        .await
        .map_err(ResponseError::internal)?;
    debug!(
        target: "live.event_processor",
        stream = "runtime",
        module = "live-event-processor",
        action = "persist_live_message_event",
        outcome = "success",
        account_uid = %uid,
        session_id = %event.session_id,
        is_self = event.is_self,
        peer_uid = %event.peer_uid,
        item_id = %event.item_id,
        item_title = ?event.item_title,
        text_preview = %event.text.chars().take(30).collect::<String>(),
        "persisted live message event"
    );

    // Fire-and-forget WeChat / notifications dispatch. Don't block the WS
    // event pipeline if a downstream channel is slow / unreachable. Only
    // forward peer-originated messages — outbound (`is_self`) chatter would
    // just echo our own replies back to the operator's WeChat.
    if !event.is_self && origin == LiveEventOrigin::Realtime {
        let state_clone = state.clone();
        let uid_owned = uid.to_string();
        let event_clone = event.clone();
        tokio::spawn(async move {
            crate::notifications::dispatch_message_event_to_channels(
                &state_clone,
                &uid_owned,
                &event_clone,
            )
            .await;
        });

        let state_clone = state.clone();
        let uid_owned = uid.to_string();
        let event_clone = event.clone();
        tokio::spawn(async move {
            if let Err(error) = crate::business_auto::pipeline::process_incoming_message(
                &state_clone,
                &uid_owned,
                &event_clone,
            )
            .await
            {
                tracing::warn!(uid = %uid_owned, session_id = %event_clone.session_id, error = %error, "business automation pipeline failed");
            }
        });
    }

    Ok(Some(live_message_event_to_value(uid, &event)))
}

/// Python parity (`xianyu_wx_live_client.py:624-634`): when a non-sync-push
/// frame arrives (e.g. an RPC response to `/r/IMessageManager/createChat` or
/// other side-channel pushes that aren't `body.syncPushPackage`), Python
/// tries `normalize_live_message_event` as a generic fallback parser.
/// Several edge-case events (just-created chat's first message, system
/// notifications dressed as RPC envelopes) only show up via this fallback.
///
/// Rust previously dropped non-sync-push frames after a debug log line.
/// This function plugs the same fallback in: it recursively unwraps
/// `body`/`data`/`message`/`payload` keys via `parse_live_sync_message`,
/// and if anything message-shaped surfaces, runs it through the normal
/// persist+enrich pipeline.
pub(crate) async fn try_process_fallback_live_event(
    state: &AppState,
    uid: &str,
    payload: &Value,
) -> Result<Vec<Value>, ResponseError> {
    let parsed = parse_live_sync_messages(uid, payload);
    if parsed.is_empty() {
        return Ok(Vec::new());
    }

    let mut values = Vec::new();
    for event in parsed {
        if let Some(value) = process_live_message_event(state, uid, event).await? {
            values.push(value);
        }
    }

    if values.is_empty() {
        return Ok(Vec::new());
    }

    Ok(values)
}

pub(crate) async fn decode_and_process_live_sync_payload(
    state: &AppState,
    uid: &str,
    payload: &Value,
) -> Result<Vec<Value>, ResponseError> {
    let mut events = Vec::new();
    for (encoded, pts) in extract_sync_push_data(payload) {
        let decoded = decode_wx_sync_data(&encoded)?;
        let diagnostic =
            live_diagnostic_capture(state, "live-event-processor", uid, None, Some(uid)).await;
        if diagnostic.matched {
            if diagnostic.include_payloads {
                info!(
                    target: "live.event_processor",
                    stream = "runtime",
                    module = "live-event-processor",
                    action = "sync_payload_decoded",
                    username = %diagnostic.username,
                    device_id = %diagnostic.device_id,
                    account_uid = %uid,
                    decoded_preview = %serde_json::to_string(&decoded)
                        .unwrap_or_default()
                        .chars()
                        .take(2000)
                        .collect::<String>(),
                    "decoded live sync payload"
                );
            } else {
                info!(
                    target: "live.event_processor",
                    stream = "runtime",
                    module = "live-event-processor",
                    action = "sync_payload_decoded",
                    username = %diagnostic.username,
                    device_id = %diagnostic.device_id,
                    account_uid = %uid,
                    "decoded live sync payload metadata"
                );
            }
        }
        for mut event in parse_live_sync_messages(uid, &decoded) {
            if pts > 0 {
                event.accs_pts = pts;
            }
            if let Some(value) = process_live_message_event(state, uid, event).await? {
                events.push(value);
            }
        }
    }
    Ok(events)
}

pub(super) async fn persist_live_message_event(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    event: &LiveMessageEvent,
    cursor: Option<&LiveSessionCursor>,
) -> Result<(), sqlx::Error> {
    let version = event.version.max(event.ts).max(1);
    let ts = event.ts.max(version);
    let preview = live_message_preview(&event.kind, &event.preview, &event.text, &event.title);
    let image_urls = if !event.image_url.trim().is_empty() {
        vec![event.image_url.clone()]
    } else {
        Vec::new()
    };
    // 之前硬编码 1/2，丢了上游真实 contentType（7/8/30 等卡片就被当文本写进 DB）。
    // 改成：event.content_type 非零就用，零再按 kind 派生（兼容旧 event 路径）。
    let content_type = if event.content_type > 0 {
        event.content_type
    } else {
        match event.kind.as_str() {
            "image" => 2,
            _ => 1,
        }
    };
    let unread = if !event.is_self && version > cursor.map(|row| row.latest_version).unwrap_or(0) {
        cursor.map(|row| row.unread_count).unwrap_or(0) + 1
    } else {
        cursor.map(|row| row.unread_count).unwrap_or(0)
    };
    let msg_id = if event.message_id.trim().is_empty() {
        format!("ws:{}:{version}", event.session_id)
    } else {
        event.message_id.clone()
    };
    let latest_msg_id = if event.accs_pts > 0 {
        format!("{}.PNM", event.accs_pts)
    } else {
        event.message_id.clone()
    };
    let page = MessageHistoryPage {
        session: MessageSessionIdentity {
            session_id: event.session_id.clone(),
            session_type: event.session_type.max(1),
            version,
            unread,
            latest_message_id: latest_msg_id,
            peer_uid: event.peer_uid.clone(),
            peer_nick: event.peer_nick.clone(),
            peer_logo: event.peer_logo.clone(),
            peer_role: event.peer_role.clone(),
            self_role: event.self_role.clone(),
            item_id: event.item_id.clone(),
            item_title: event.item_title.clone(),
            item_pic: event.item_pic.clone(),
            summary: preview.clone(),
            ts,
            sort_index: version.max(ts),
        },
        has_more: false,
        next_cursor: 0,
        latest_cursor: version,
        messages: vec![MessageHistoryEntry {
            message_id: msg_id,
            upstream_message_id: event.upstream_message_id.clone(),
            version,
            ts,
            kind: event.kind.clone(),
            title: event.title.clone(),
            text: event.text.clone(),
            image_url: event.image_url.clone(),
            image_urls,
            content_type,
            preview,
            sender_nick: event.sender_nick.clone(),
            sender_logo: event.sender_logo.clone(),
            is_self: event.is_self,
            unread_count: event.unread_count,
            // 把上游解析出的卡片元数据带到 message_history_cache.card_meta，
            // 前端按 contentType + cardMeta 渲染富卡片（item / location）。
            card_meta: event.card_meta.clone(),
            // WS-pushed reply 上游不携带引用文本，只下发 PNM。reply_to_text 留空，
            // 等接收侧前端按 PNM 在本地历史里反查（或下次 mtop history sync
            // 回填）再渲染气泡 quote 块。
            reply_to_message_id: event.reply_to_message_id.clone(),
            reply_to_text: String::new(),
            ..Default::default()
        }],
    };
    persist_message_history_page(pool, uid, &page, true).await
}
