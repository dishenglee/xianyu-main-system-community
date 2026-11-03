//! SQLite persistence helpers — write sessions + history to live_state DB,
//! and read them back (load_sessions_for_uid, load_single_session_info).

use serde_json::{json, Value};
use sqlx::Row;
use tracing::warn;

use super::{
    json_string, message_user_display, should_suppress_chat_unread, MessageHistoryPage,
    MessageSessionIdentity,
};

// ---------------------------------------------------------------------------
// Write helpers
// ---------------------------------------------------------------------------

pub async fn persist_message_sessions(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    sessions: &[MessageSessionIdentity],
) -> Result<(), sqlx::Error> {
    for session in sessions {
        // Defense in depth: refuse to persist sessions without a session_id.
        // Upstream/WS payloads occasionally produce identity rows with an
        // empty sessionId (e.g. malformed `userMessageModels` envelopes whose
        // `cid` is missing). Persisting them collapses every such caller into
        // the same `(uid, '')` PK and surfaces as a "未知" row in the message
        // list that survives refreshes. The early-return in
        // `process_live_message_event` already drops the WS path, but other
        // call sites (history hydrate, fallback parsers) could still reach
        // here — keep the guard here so the DB never accepts the bad row.
        if session.session_id.trim().is_empty() {
            warn!(
                target: "messages.persistence",
                stream = "runtime",
                module = "messages-persistence",
                action = "persist_message_session_dropped",
                outcome = "warn",
                account_uid = %uid,
                error_code = "missing_session_id",
                retryable = false,
                peer_uid = ?session.peer_uid,
                summary = %session.summary.chars().take(30).collect::<String>(),
                "dropping message session without session_id"
            );
            continue;
        }
        let summary = session_summary_for_persist(pool, uid, session).await?;
        let unread_count = if should_suppress_chat_unread(session.session_type, &summary) {
            0
        } else if let Some(override_unread) =
            cached_session_unread_override(pool, uid, session).await?
        {
            override_unread
        } else {
            session.unread.max(0)
        };
        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid,
                session_id,
                session_type,
                latest_version,
                latest_message_id,
                unread_count,
                peer_uid,
                peer_nick,
                peer_role,
                peer_logo,
                self_role,
                item_id,
                item_title,
                item_pic,
                summary,
                last_message_ts,
                last_sync_at,
                last_http_compensate_at,
                updated_at
            )
            VALUES (
                ?, ?, ?, ?, ?,
                ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?,
                unixepoch('subsec'),
                unixepoch('subsec'),
                unixepoch('subsec')
            )
            ON CONFLICT(uid, session_id) DO UPDATE SET
                session_type = excluded.session_type,
                latest_version = CASE
                    WHEN excluded.latest_version > message_live_state.latest_version THEN excluded.latest_version
                    ELSE message_live_state.latest_version
                END,
                latest_message_id = CASE
                    WHEN TRIM(excluded.latest_message_id) <> '' AND excluded.latest_version >= message_live_state.latest_version
                        THEN excluded.latest_message_id
                    ELSE message_live_state.latest_message_id
                END,
                unread_count = excluded.unread_count,
                peer_uid = CASE WHEN TRIM(excluded.peer_uid) <> '' THEN excluded.peer_uid ELSE message_live_state.peer_uid END,
                peer_nick = CASE
                    WHEN TRIM(excluded.peer_nick) = '' THEN message_live_state.peer_nick
                    WHEN (
                            excluded.peer_nick IN (
                                '你有一条新消息', '您有一条新消息',
                                '未知', '未知会话', '未知账号', '未知客户'
                            )
                            OR excluded.peer_nick LIKE '%待付款%'
                            OR excluded.peer_nick LIKE '%等待你发货%'
                            OR excluded.peer_nick LIKE '%待发货%'
                            OR excluded.peer_nick LIKE '%记得及时发货%'
                            OR excluded.peer_nick LIKE '%去发货%'
                            OR excluded.peer_nick LIKE '%退款成功%'
                            OR excluded.peer_nick LIKE '%闲鱼小红花%'
                            OR excluded.peer_nick LIKE '用户_%'
                            OR excluded.peer_nick LIKE 'tbNick_%'
                            OR (
                                TRIM(excluded.peer_uid) <> ''
                                AND excluded.peer_nick = excluded.peer_uid
                            )
                            OR (
                                excluded.peer_nick GLOB '[0-9]*'
                                AND excluded.peer_nick NOT GLOB '*[^0-9]*'
                            )
                         )
                         AND TRIM(message_live_state.peer_nick) <> ''
                         AND NOT (
                            message_live_state.peer_nick IN (
                                '你有一条新消息', '您有一条新消息',
                                '未知', '未知会话', '未知账号', '未知客户'
                            )
                            OR message_live_state.peer_nick LIKE '%待付款%'
                            OR message_live_state.peer_nick LIKE '%等待你发货%'
                            OR message_live_state.peer_nick LIKE '%待发货%'
                            OR message_live_state.peer_nick LIKE '%记得及时发货%'
                            OR message_live_state.peer_nick LIKE '%去发货%'
                            OR message_live_state.peer_nick LIKE '%退款成功%'
                            OR message_live_state.peer_nick LIKE '%闲鱼小红花%'
                            OR message_live_state.peer_nick LIKE '用户_%'
                            OR message_live_state.peer_nick LIKE 'tbNick_%'
                            OR (
                                TRIM(message_live_state.peer_uid) <> ''
                                AND message_live_state.peer_nick = message_live_state.peer_uid
                            )
                            OR (
                                message_live_state.peer_nick GLOB '[0-9]*'
                                AND message_live_state.peer_nick NOT GLOB '*[^0-9]*'
                            )
                         )
                        THEN message_live_state.peer_nick
                    ELSE excluded.peer_nick
                END,
                peer_role = CASE WHEN TRIM(excluded.peer_role) <> '' THEN excluded.peer_role ELSE message_live_state.peer_role END,
                peer_logo = CASE WHEN TRIM(excluded.peer_logo) <> '' THEN excluded.peer_logo ELSE message_live_state.peer_logo END,
                self_role = CASE WHEN TRIM(excluded.self_role) <> '' THEN excluded.self_role ELSE message_live_state.self_role END,
                item_id = CASE WHEN TRIM(excluded.item_id) <> '' THEN excluded.item_id ELSE message_live_state.item_id END,
                item_title = CASE WHEN TRIM(excluded.item_title) <> '' THEN excluded.item_title ELSE message_live_state.item_title END,
                item_pic = CASE WHEN TRIM(excluded.item_pic) <> '' THEN excluded.item_pic ELSE message_live_state.item_pic END,
                summary = CASE WHEN TRIM(excluded.summary) <> '' THEN excluded.summary ELSE message_live_state.summary END,
                last_message_ts = CASE
                    WHEN excluded.last_message_ts > message_live_state.last_message_ts THEN excluded.last_message_ts
                    ELSE message_live_state.last_message_ts
                END,
                last_sync_at = unixepoch('subsec'),
                last_http_compensate_at = unixepoch('subsec'),
                updated_at = unixepoch('subsec')
            "#,
        )
        .bind(uid)
        .bind(&session.session_id)
        .bind(session.session_type)
        .bind(session.version.max(0))
        .bind(&session.latest_message_id)
        .bind(unread_count)
        .bind(&session.peer_uid)
        .bind(&session.peer_nick)
        .bind(&session.peer_role)
        .bind(&session.peer_logo)
        .bind(&session.self_role)
        .bind(&session.item_id)
        .bind(&session.item_title)
        .bind(&session.item_pic)
        .bind(&summary)
        .bind(session.ts.max(0))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn cached_session_unread_override(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session: &MessageSessionIdentity,
) -> Result<Option<i64>, sqlx::Error> {
    if session.unread <= 0 {
        return Ok(None);
    }

    let mut latest_message_id = session.latest_message_id.trim().to_string();
    let mut latest_version = session.version.max(0);
    if latest_message_id.is_empty() || latest_version <= 0 {
        if let Some(row) = sqlx::query(
            "SELECT latest_message_id, latest_version FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(&session.session_id)
        .fetch_optional(pool)
        .await?
        {
            if latest_message_id.is_empty() {
                latest_message_id = row.get::<String, _>("latest_message_id");
            }
            if latest_version <= 0 {
                latest_version = row.get::<i64, _>("latest_version").max(0);
            }
        }
    }
    if latest_message_id.is_empty() && latest_version <= 0 {
        return Ok(None);
    }

    // `pc.session.sync` can keep a sticky unread>0 long after the latest
    // incoming message has been read in the official client. This affects
    // both chat sessions and platform notification channels. If our cached
    // history already contains the same latest incoming message with
    // unread_count=0, preserve that stronger read proof instead of letting
    // session-sync write the stale unread badge back into `message_live_state`.
    let row = sqlx::query(
        r#"
        SELECT MIN(unread_count) AS min_unread
        FROM message_history_cache
        WHERE uid = ?
          AND session_id = ?
          AND is_self = 0
          AND unread_count >= 0
          AND (
                (
                    ? <> ''
                    AND (
                        message_id = ?
                        OR upstream_message_id = ?
                        OR ? LIKE '%' || message_id || '%'
                        OR ? LIKE '%' || upstream_message_id || '%'
                    )
                )
                OR (? > 0 AND version = ?)
          )
        "#,
    )
    .bind(uid)
    .bind(&session.session_id)
    .bind(&latest_message_id)
    .bind(&latest_message_id)
    .bind(&latest_message_id)
    .bind(&latest_message_id)
    .bind(&latest_message_id)
    .bind(latest_version)
    .bind(latest_version)
    .fetch_one(pool)
    .await?;

    let min_unread = row.try_get::<Option<i64>, _>("min_unread").ok().flatten();
    if min_unread == Some(0) {
        Ok(Some(0))
    } else {
        Ok(None)
    }
}

async fn session_summary_for_persist(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session: &MessageSessionIdentity,
) -> Result<String, sqlx::Error> {
    if !is_unknown_type_summary(&session.summary) {
        return Ok(session.summary.clone());
    }

    let latest_message_id = session.latest_message_id.trim();
    let latest_version = session.version.max(0);
    if latest_message_id.is_empty() && latest_version <= 0 {
        return Ok(session.summary.clone());
    }

    let recalled = sqlx::query(
        r#"
        SELECT
            CASE
                WHEN TRIM(preview) <> '' THEN preview
                WHEN TRIM(text) <> '' THEN text
                ELSE '对方撤回了一条消息'
            END AS summary
        FROM message_history_cache
        WHERE uid = ?
          AND session_id = ?
          AND recalled_at > 0
          AND (
                (
                    ? <> ''
                    AND (
                        message_id = ?
                        OR upstream_message_id = ?
                        OR ? LIKE '%' || message_id || '%'
                        OR ? LIKE '%' || upstream_message_id || '%'
                    )
                )
                OR (? > 0 AND version = ?)
          )
        ORDER BY version DESC
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(&session.session_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_version)
    .bind(latest_version)
    .fetch_optional(pool)
    .await?;

    Ok(recalled
        .and_then(|row| row.try_get::<String, _>("summary").ok())
        .filter(|summary| !summary.trim().is_empty())
        .unwrap_or_else(|| session.summary.clone()))
}

fn is_unknown_type_summary(summary: &str) -> bool {
    let summary = summary.trim();
    summary.starts_with("[消息类型 ") && summary.ends_with(']')
}

async fn cached_session_read_proof_for_history_refresh(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    latest_version: i64,
) -> Result<bool, sqlx::Error> {
    if latest_version <= 0 {
        return Ok(false);
    }

    let row = sqlx::query(
        r#"
        SELECT latest_version, unread_count
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(row
        .map(|row| {
            row.get::<i64, _>("unread_count") <= 0
                && row.get::<i64, _>("latest_version") >= latest_version
        })
        .unwrap_or(false))
}

async fn clear_cached_incoming_session_unreads(
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
    Ok(())
}

async fn preserve_local_incoming_message_read_proof(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    message: &super::MessageHistoryEntry,
) -> Result<bool, sqlx::Error> {
    if message.is_self || message.unread_count <= 0 {
        return Ok(false);
    }

    let message_id = message.message_id.trim();
    let upstream_message_id = message.upstream_message_id.trim();
    if message_id.is_empty() && upstream_message_id.is_empty() {
        return Ok(false);
    }

    let row = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1
        FROM message_history_cache
        WHERE uid = ?1
          AND session_id = ?2
          AND is_self = 0
          AND unread_count = 0
          AND (
                (
                    ?3 <> ''
                    AND (
                        message_id = ?3
                        OR upstream_message_id = ?3
                        OR ?3 LIKE '%' || message_id || '%'
                        OR ?3 LIKE '%' || upstream_message_id || '%'
                    )
                )
                OR (
                    ?4 <> ''
                    AND (
                        message_id = ?4
                        OR upstream_message_id = ?4
                        OR ?4 LIKE '%' || message_id || '%'
                        OR ?4 LIKE '%' || upstream_message_id || '%'
                    )
                )
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

async fn clear_matching_incoming_message_unreads(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    message: &super::MessageHistoryEntry,
) -> Result<(), sqlx::Error> {
    if message.is_self {
        return Ok(());
    }

    let message_id = message.message_id.trim();
    let upstream_message_id = message.upstream_message_id.trim();
    if message_id.is_empty() && upstream_message_id.is_empty() {
        return Ok(());
    }

    sqlx::query(
        r#"
        UPDATE message_history_cache
        SET unread_count = 0,
            cached_at = unixepoch('subsec')
        WHERE uid = ?1
          AND session_id = ?2
          AND is_self = 0
          AND unread_count > 0
          AND (
                (
                    ?3 <> ''
                    AND (
                        message_id = ?3
                        OR upstream_message_id = ?3
                        OR ?3 LIKE '%' || message_id || '%'
                        OR ?3 LIKE '%' || upstream_message_id || '%'
                    )
                )
                OR (
                    ?4 <> ''
                    AND (
                        message_id = ?4
                        OR upstream_message_id = ?4
                        OR ?4 LIKE '%' || message_id || '%'
                        OR ?4 LIKE '%' || upstream_message_id || '%'
                    )
                )
          )
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(message_id)
    .bind(upstream_message_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn persist_message_history_page(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    page: &MessageHistoryPage,
    refresh_session_state: bool,
) -> Result<(), sqlx::Error> {
    // Mirror the guard in `persist_message_sessions`: a page whose session
    // identity has an empty session_id cannot be safely keyed in either
    // `message_history_cache` (PK: uid, session_id, version, message_id) or
    // `message_live_state` (PK: uid, session_id). Dropping early prevents the
    // recurring "未知" phantom session row.
    if page.session.session_id.trim().is_empty() {
        warn!(
            target: "messages.persistence",
            stream = "runtime",
            module = "messages-persistence",
            action = "persist_message_history_dropped",
            outcome = "warn",
            account_uid = %uid,
            error_code = "missing_session_id",
            retryable = false,
            message_count = page.messages.len(),
            summary = %page.session.summary.chars().take(30).collect::<String>(),
            "dropping message history page without session_id"
        );
        return Ok(());
    }

    let latest_version = page
        .messages
        .last()
        .map(|message| message.version.max(message.ts))
        .unwrap_or_else(|| page.session.version.max(page.latest_cursor));
    let preserve_cached_read_state = if refresh_session_state {
        cached_session_read_proof_for_history_refresh(
            pool,
            uid,
            &page.session.session_id,
            latest_version.max(0),
        )
        .await?
    } else {
        false
    };

    for message in &page.messages {
        let unread_count = if preserve_local_incoming_message_read_proof(
            pool,
            uid,
            &page.session.session_id,
            message,
        )
        .await?
        {
            0
        } else {
            message.unread_count
        };
        let image_urls =
            serde_json::to_string(&message.image_urls).unwrap_or_else(|_| "[]".to_string());
        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid,
                session_id,
                session_type,
                message_id,
                upstream_message_id,
                version,
                ts,
                kind,
                title,
                text,
                image_url,
                image_urls,
                content_type,
                preview,
                sender_nick,
                sender_logo,
                is_self,
                unread_count,
                card_meta,
                reply_to_message_id,
                reply_to_text,
                recalled_at,
                recalled_original_text,
                recalled_original_image_url,
                cached_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'))
            ON CONFLICT(uid, session_id, version, message_id) DO UPDATE SET
                upstream_message_id = CASE
                    WHEN TRIM(excluded.upstream_message_id) <> ''
                        THEN excluded.upstream_message_id
                    ELSE message_history_cache.upstream_message_id
                END,
                kind = CASE
                    WHEN message_history_cache.recalled_at > 0 THEN message_history_cache.kind
                    ELSE excluded.kind
                END,
                title = CASE
                    WHEN message_history_cache.recalled_at > 0 THEN message_history_cache.title
                    ELSE excluded.title
                END,
                text = CASE
                    WHEN message_history_cache.recalled_at > 0
                         AND excluded.recalled_at <= 0 THEN message_history_cache.text
                    ELSE excluded.text
                END,
                image_url = CASE
                    WHEN message_history_cache.recalled_at > 0
                         AND excluded.recalled_at <= 0 THEN message_history_cache.image_url
                    ELSE excluded.image_url
                END,
                image_urls = CASE
                    WHEN message_history_cache.recalled_at > 0
                         AND excluded.recalled_at <= 0 THEN message_history_cache.image_urls
                    ELSE excluded.image_urls
                END,
                content_type = CASE
                    WHEN message_history_cache.recalled_at > 0
                         AND excluded.recalled_at <= 0 THEN message_history_cache.content_type
                    ELSE excluded.content_type
                END,
                preview = CASE
                    WHEN message_history_cache.recalled_at > 0
                         AND excluded.recalled_at <= 0 THEN message_history_cache.preview
                    ELSE excluded.preview
                END,
                sender_nick = excluded.sender_nick,
                sender_logo = excluded.sender_logo,
                is_self = excluded.is_self,
                unread_count = excluded.unread_count,
                card_meta = excluded.card_meta,
                -- 引用字段只在 incoming 行带值时覆盖；否则保留旧值。这样后台
                -- WS 推送先到（无 reply 字段，因 extJson 只回 PNM 引用）后再
                -- 收到正经 mtop.history sync 时不会把 reply_to_text 抹掉。
                reply_to_message_id = CASE
                    WHEN TRIM(excluded.reply_to_message_id) <> ''
                        THEN excluded.reply_to_message_id
                    ELSE message_history_cache.reply_to_message_id
                END,
                reply_to_text = CASE
                    WHEN TRIM(excluded.reply_to_text) <> ''
                        THEN excluded.reply_to_text
                    ELSE message_history_cache.reply_to_text
                END,
                recalled_at = CASE
                    WHEN excluded.recalled_at > 0 THEN excluded.recalled_at
                    ELSE message_history_cache.recalled_at
                END,
                recalled_original_text = CASE
                    WHEN TRIM(excluded.recalled_original_text) <> ''
                        THEN excluded.recalled_original_text
                    WHEN excluded.recalled_at > 0
                         AND TRIM(COALESCE(message_history_cache.recalled_original_text, '')) = ''
                        THEN message_history_cache.text
                    ELSE message_history_cache.recalled_original_text
                END,
                recalled_original_image_url = CASE
                    WHEN TRIM(excluded.recalled_original_image_url) <> ''
                        THEN excluded.recalled_original_image_url
                    WHEN excluded.recalled_at > 0
                         AND TRIM(COALESCE(message_history_cache.recalled_original_image_url, '')) = ''
                        THEN message_history_cache.image_url
                    ELSE message_history_cache.recalled_original_image_url
                END,
                cached_at = unixepoch('subsec')
            "#,
        )
        .bind(uid)
        .bind(&page.session.session_id)
        .bind(page.session.session_type)
        .bind(&message.message_id)
        .bind(&message.upstream_message_id)
        .bind(message.version)
        .bind(message.ts)
        .bind(&message.kind)
        .bind(&message.title)
        .bind(&message.text)
        .bind(&message.image_url)
        .bind(image_urls)
        .bind(message.content_type)
        .bind(&message.preview)
        .bind(&message.sender_nick)
        .bind(&message.sender_logo)
        .bind(if message.is_self { 1_i64 } else { 0_i64 })
        .bind(unread_count)
        .bind(&message.card_meta)
        .bind(&message.reply_to_message_id)
        .bind(&message.reply_to_text)
        .bind(message.recalled_at.max(0))
        .bind(&message.recalled_original_text)
        .bind(&message.recalled_original_image_url)
        .execute(pool)
        .await?;

        if unread_count <= 0 {
            clear_matching_incoming_message_unreads(pool, uid, &page.session.session_id, message)
                .await?;
        }
    }

    if preserve_cached_read_state {
        clear_cached_incoming_session_unreads(pool, uid, &page.session.session_id).await?;
    }

    if refresh_session_state {
        let mut session = page.session.clone();
        if preserve_cached_read_state {
            session.unread = 0;
        }
        persist_message_sessions(pool, uid, &[session]).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Read helpers
// ---------------------------------------------------------------------------

pub async fn load_sessions_for_uid(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    limit: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT uid, session_id, session_type, latest_version, latest_message_id, unread_count, peer_uid, peer_nick, peer_role,
               peer_logo, self_role, item_id, item_title, item_pic, summary, last_message_ts,
               COALESCE(peer_last_read_version, 0) AS peer_last_read_version,
               updated_at
        FROM message_live_state
        WHERE uid = ?
          AND latest_version > 0
        "#,
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;

    let mut sessions = Vec::with_capacity(rows.len());
    for row in rows {
        let mut session = session_row_to_value(row);
        let unread = session.get("unread").and_then(Value::as_i64).unwrap_or(0);
        let session_type = session
            .get("sessionType")
            .and_then(Value::as_i64)
            .unwrap_or(1);
        if unread > 0 {
            let identity = MessageSessionIdentity {
                session_id: json_string(session.get("sessionId")),
                session_type,
                version: session.get("version").and_then(Value::as_i64).unwrap_or(0),
                latest_message_id: json_string(session.get("latestMessageId")),
                unread,
                ..Default::default()
            };
            if let Some(override_unread) =
                cached_session_unread_override(pool, uid, &identity).await?
            {
                if let Some(object) = session.as_object_mut() {
                    object.insert("unread".to_string(), Value::from(override_unread));
                }
            }
        }
        sessions.push(session);
    }
    sort_session_values(&mut sessions);
    sessions.truncate(limit.max(1) as usize);
    Ok(sessions)
}

pub async fn load_single_session_info(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    session_type: i64,
) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT uid, session_id, session_type, latest_version, latest_message_id, unread_count, peer_uid, peer_nick, peer_role,
               peer_logo, self_role, item_id, item_title, item_pic, summary, last_message_ts,
               COALESCE(peer_last_read_version, 0) AS peer_last_read_version,
               updated_at
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(session_row_to_value).map(|mut value| {
        if let Some(object) = value.as_object_mut() {
            object.insert("sessionType".to_string(), Value::from(session_type));
        }
        value
    }))
}

pub async fn load_trusted_single_session_info(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    session_id: &str,
    session_type: i64,
) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT uid, session_id, session_type, latest_version, latest_message_id, unread_count, peer_uid, peer_nick, peer_role,
               peer_logo, self_role, item_id, item_title, item_pic, summary, last_message_ts,
               COALESCE(peer_last_read_version, 0) AS peer_last_read_version,
               updated_at
        FROM message_live_state
        WHERE uid = ? AND session_id = ?
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
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(session_row_to_value).map(|mut value| {
        if let Some(object) = value.as_object_mut() {
            object.insert("sessionType".to_string(), Value::from(session_type));
        }
        value
    }))
}

pub fn session_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    let latest_version = row.get::<i64, _>("latest_version");
    let last_message_ts = row.get::<i64, _>("last_message_ts");
    let updated_at = row.get::<f64, _>("updated_at");
    let summary = row.get::<String, _>("summary");
    let activity_ts = if last_message_ts > 0 {
        last_message_ts
    } else if latest_version > 0 {
        latest_version
    } else {
        (updated_at * 1000.0) as i64
    };
    let peer_nick = decoded_nick(row.get::<String, _>("peer_nick"));
    let stored_type = row.get::<i64, _>("session_type");
    // Re-classify 闲鱼 platform notification-center / workbench virtual
    // sessions to sessionType=53 at read time too. The upstream
    // `pc.session.sync` filter dictates the response's sessionType (1 vs 53)
    // for the same underlying session, so older cache rows that were written
    // back when the upstream returned type=1 still need fixing up — without
    // this they'd leak into the chat list as "通知中心o*" duplicates on the
    // very next cached read. See `normalize/mod.rs::is_platform_notification_peer`.
    let session_type = if super::is_platform_notification_peer(&peer_nick) {
        53
    } else {
        stored_type
    };
    let unread_count = if should_suppress_chat_unread(stored_type, &summary) {
        0
    } else {
        row.get::<i64, _>("unread_count")
    };

    json!({
        "sessionId": row.get::<String, _>("session_id"),
        "sessionType": session_type,
        "version": latest_version,
        "unread": unread_count,
        "latestMessageId": row.get::<String, _>("latest_message_id"),
        "peerUid": row.get::<String, _>("peer_uid"),
        "peerNick": peer_nick,
        "peerLogo": row.get::<String, _>("peer_logo"),
        "peerRole": row.get::<String, _>("peer_role"),
        "selfRole": row.get::<String, _>("self_role"),
        "itemId": row.get::<String, _>("item_id"),
        "itemTitle": row.get::<String, _>("item_title"),
        "itemPic": row.get::<String, _>("item_pic"),
        "summary": summary,
        "ts": activity_ts,
        "sortIndex": latest_version.max(activity_ts),
        "peerLastReadVersion": row.get::<i64, _>("peer_last_read_version"),
    })
}

pub fn history_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    // Expand the stored `card_meta` JSON string into a nested object on the
    // way out so the frontend doesn't have to re-parse. Empty/invalid →
    // null. Unwrapping here keeps the wire format stable regardless of DB
    // migration state.
    let card_meta_raw = row.get::<String, _>("card_meta");
    let card_meta = if card_meta_raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_str::<Value>(&card_meta_raw).unwrap_or(Value::Null)
    };
    json!({
        "sessionType": row.get::<i64, _>("session_type"),
        "messageId": row.get::<String, _>("message_id"),
        // PNM 格式的上游 messageId — 撤回 API 必须用它（不是 message_id UUID）
        "upstreamMessageId": row.try_get::<String, _>("upstream_message_id").unwrap_or_default(),
        "version": row.get::<i64, _>("version"),
        "ts": row.get::<i64, _>("ts"),
        "kind": row.get::<String, _>("kind"),
        "title": row.get::<String, _>("title"),
        "text": row.get::<String, _>("text"),
        "imageUrl": row.get::<String, _>("image_url"),
        "imageUrls": super::parse_json_array(row.get::<String, _>("image_urls").as_str()),
        "contentType": row.get::<i64, _>("content_type"),
        "preview": row.get::<String, _>("preview"),
        "senderNick": decoded_nick(row.get::<String, _>("sender_nick")),
        "senderLogo": row.get::<String, _>("sender_logo"),
        "isSelf": row.get::<i64, _>("is_self") != 0,
        // -1 = unknown. Old rows upgraded by migration default to unknown so
        // the frontend doesn't falsely show "已读".
        "unreadCount": row.try_get::<i64, _>("unread_count").unwrap_or(-1),
        "cardMeta": card_meta,
        // 反撤回三件套：recalled_at > 0 表示这条消息被撤回过，
        // recalled_original_* 是撤回前的原内容。前端在 pill 下面渲染。
        // try_get 防御旧 DB（迁移没跑过时返回默认值）。
        "recalledAt": row.try_get::<i64, _>("recalled_at").unwrap_or_default(),
        "recalledOriginalText": row.try_get::<String, _>("recalled_original_text").unwrap_or_default(),
        "recalledOriginalImageUrl": row.try_get::<String, _>("recalled_original_image_url").unwrap_or_default(),
        // 引用回复：被引用消息的 PNM + 发送时显示的引用文本。前端在普通气泡
        // 顶部绘制 quote 块。try_get 防御旧 DB（迁移 0007 还没跑）。
        "replyToMessageId": row.try_get::<String, _>("reply_to_message_id").unwrap_or_default(),
        "replyToText": row.try_get::<String, _>("reply_to_text").unwrap_or_default(),
    })
}

fn decoded_nick(raw: String) -> String {
    let mut user = serde_json::Map::new();
    user.insert("nick".to_string(), Value::String(raw));
    message_user_display(&user)
}

#[cfg(test)]
mod tests {
    use sqlx::Row;

    use serde_json::json;

    use super::{
        history_row_to_value, load_sessions_for_uid, persist_message_history_page,
        persist_message_sessions, sort_session_values,
    };
    use crate::routes::accounts::messages::{
        MessageHistoryEntry, MessageHistoryPage, MessageSessionIdentity,
    };

    async fn setup_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE message_live_state (
                uid TEXT NOT NULL,
                session_id TEXT NOT NULL,
                session_type INTEGER DEFAULT 1,
                latest_version INTEGER DEFAULT 0,
                latest_message_id TEXT DEFAULT '',
                unread_count INTEGER DEFAULT 0,
                peer_uid TEXT DEFAULT '',
                peer_nick TEXT DEFAULT '',
                peer_role TEXT DEFAULT '',
                peer_logo TEXT DEFAULT '',
                self_role TEXT DEFAULT '',
                item_id TEXT DEFAULT '',
                item_title TEXT DEFAULT '',
                item_pic TEXT DEFAULT '',
                summary TEXT DEFAULT '',
                last_message_ts INTEGER DEFAULT 0,
                peer_last_read_version INTEGER DEFAULT 0,
                last_sync_at REAL DEFAULT 0,
                last_http_compensate_at REAL DEFAULT 0,
                updated_at REAL DEFAULT 0,
                PRIMARY KEY (uid, session_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE message_history_cache (
                uid TEXT NOT NULL,
                session_id TEXT NOT NULL,
                session_type INTEGER DEFAULT 1,
                message_id TEXT DEFAULT '',
                version INTEGER DEFAULT 0,
                ts INTEGER DEFAULT 0,
                kind TEXT DEFAULT 'text',
                title TEXT DEFAULT '',
                text TEXT DEFAULT '',
                image_url TEXT DEFAULT '',
                image_urls TEXT DEFAULT '',
                content_type INTEGER DEFAULT 0,
                preview TEXT DEFAULT '',
                sender_nick TEXT DEFAULT '',
                sender_logo TEXT DEFAULT '',
                is_self INTEGER DEFAULT 0,
                unread_count INTEGER NOT NULL DEFAULT -1,
                cached_at REAL DEFAULT 0,
                card_meta TEXT NOT NULL DEFAULT '',
                upstream_message_id TEXT NOT NULL DEFAULT '',
                recalled_at INTEGER NOT NULL DEFAULT 0,
                recalled_original_text TEXT NOT NULL DEFAULT '',
                recalled_original_image_url TEXT NOT NULL DEFAULT '',
                reply_to_message_id TEXT NOT NULL DEFAULT '',
                reply_to_text TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (uid, session_id, version, message_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn persist_message_history_page_round_trips_message_unread_count() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-read-hint";
        let message_id = "msg-1";
        let version = 321_i64;

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-1.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "text".to_string(),
                    text: "hello".to_string(),
                    preview: "hello".to_string(),
                    is_self: true,
                    unread_count: 3,
                    ..Default::default()
                }],
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT session_type, message_id, upstream_message_id, version, ts, kind, title, text, image_url, image_urls, \
                    content_type, preview, sender_nick, sender_logo, is_self, unread_count, card_meta, \
                    recalled_at, recalled_original_text, recalled_original_image_url, reply_to_message_id, reply_to_text \
             FROM message_history_cache WHERE uid = ? AND session_id = ? AND message_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        let value = history_row_to_value(row);
        assert_eq!(value.get("unreadCount").and_then(|v| v.as_i64()), Some(3));
    }

    #[tokio::test]
    async fn persist_message_history_page_keeps_cached_session_read_state_for_same_latest_history()
    {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-read-proof-history";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                summary, last_message_ts, updated_at
            ) VALUES (?, ?, 1, 300, 'msg-3', 0, '还卖吗？', 300, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version: 300,
                    latest_message_id: "msg-3".to_string(),
                    unread: 3,
                    summary: "还卖吗？".to_string(),
                    ts: 300,
                    sort_index: 300,
                    ..Default::default()
                },
                has_more: false,
                next_cursor: 0,
                latest_cursor: 300,
                messages: vec![
                    MessageHistoryEntry {
                        message_id: "msg-1".to_string(),
                        upstream_message_id: "pnm-1.PNM".to_string(),
                        version: 100,
                        ts: 100,
                        kind: "text".to_string(),
                        text: "700可以出吗？".to_string(),
                        preview: "700可以出吗？".to_string(),
                        unread_count: 1,
                        ..Default::default()
                    },
                    MessageHistoryEntry {
                        message_id: "msg-2".to_string(),
                        upstream_message_id: "pnm-2.PNM".to_string(),
                        version: 200,
                        ts: 200,
                        kind: "text".to_string(),
                        text: "现在拉走".to_string(),
                        preview: "现在拉走".to_string(),
                        unread_count: 1,
                        ..Default::default()
                    },
                    MessageHistoryEntry {
                        message_id: "msg-3".to_string(),
                        upstream_message_id: "pnm-3.PNM".to_string(),
                        version: 300,
                        ts: 300,
                        kind: "text".to_string(),
                        text: "还卖吗？".to_string(),
                        preview: "还卖吗？".to_string(),
                        unread_count: 1,
                        ..Default::default()
                    },
                ],
            },
            true,
        )
        .await
        .unwrap();

        let unread = sqlx::query_scalar::<_, i64>(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unread, 0);

        let unread_rows = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM message_history_cache WHERE uid = ? AND session_id = ? AND is_self = 0 AND unread_count > 0",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unread_rows, 0);
    }

    #[tokio::test]
    async fn persist_message_history_page_does_not_restore_incoming_unread_for_same_messages_after_local_read_proof_is_overwritten_in_live_state(
    ) {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-row-read-proof";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                summary, last_message_ts, updated_at
            ) VALUES (?, ?, 1, 300, 'msg-3', 6, '退款成功', 300, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        for (message_id, version, preview) in [
            ("msg-1", 100_i64, "我已拍下，待付款"),
            ("msg-2", 200_i64, "等待你发货"),
            ("msg-3", 300_i64, "退款成功"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO message_history_cache (
                    uid, session_id, session_type, message_id, upstream_message_id,
                    version, ts, kind, text, preview, is_self, unread_count, cached_at
                ) VALUES (?, ?, 1, ?, ?, ?, ?, 'text', ?, ?, 0, 0, unixepoch('subsec'))
                "#,
            )
            .bind(uid)
            .bind(session_id)
            .bind(message_id)
            .bind(format!("{message_id}.PNM"))
            .bind(version)
            .bind(version)
            .bind(preview)
            .bind(preview)
            .execute(&pool)
            .await
            .unwrap();
        }

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version: 300,
                    latest_message_id: "msg-3".to_string(),
                    unread: 3,
                    summary: "退款成功".to_string(),
                    ts: 300,
                    sort_index: 300,
                    ..Default::default()
                },
                has_more: false,
                next_cursor: 0,
                latest_cursor: 300,
                messages: vec![
                    MessageHistoryEntry {
                        message_id: "msg-1".to_string(),
                        upstream_message_id: "msg-1.PNM".to_string(),
                        version: 100,
                        ts: 100,
                        kind: "text".to_string(),
                        text: "我已拍下，待付款".to_string(),
                        preview: "我已拍下，待付款".to_string(),
                        unread_count: 1,
                        ..Default::default()
                    },
                    MessageHistoryEntry {
                        message_id: "msg-2".to_string(),
                        upstream_message_id: "msg-2.PNM".to_string(),
                        version: 200,
                        ts: 200,
                        kind: "text".to_string(),
                        text: "等待你发货".to_string(),
                        preview: "等待你发货".to_string(),
                        unread_count: 1,
                        ..Default::default()
                    },
                    MessageHistoryEntry {
                        message_id: "msg-3".to_string(),
                        upstream_message_id: "msg-3.PNM".to_string(),
                        version: 300,
                        ts: 300,
                        kind: "text".to_string(),
                        text: "退款成功".to_string(),
                        preview: "退款成功".to_string(),
                        unread_count: 1,
                        ..Default::default()
                    },
                ],
            },
            true,
        )
        .await
        .unwrap();

        let live_unread = sqlx::query_scalar::<_, i64>(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_unread, 0);

        let unread_rows = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM message_history_cache WHERE uid = ? AND session_id = ? AND is_self = 0 AND unread_count > 0",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unread_rows, 0);
    }

    #[tokio::test]
    async fn persist_message_history_page_keeps_read_proof_when_same_message_id_reappears_with_different_version(
    ) {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-row-read-proof-version-mismatch";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                summary, last_message_ts, updated_at
            ) VALUES (?, ?, 1, 1748921205690, 'msg-dup', 1, '请不要脱离闲鱼沟通及交易', 1748921205690, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 1, 'msg-dup', 'msg-dup.PNM', 10006, 1748921206000, 'text', '提醒', '提醒', 0, 0, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 1, 'msg-dup', 'msg-dup.PNM', 1748921205690, 1748921205690, 'text', '提醒', '提醒', 0, 1, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version: 1748921205690,
                    latest_message_id: "msg-dup".to_string(),
                    unread: 1,
                    summary: "请不要脱离闲鱼沟通及交易".to_string(),
                    ts: 1748921205690,
                    sort_index: 1748921205690,
                    ..Default::default()
                },
                has_more: false,
                next_cursor: 0,
                latest_cursor: 1748921205690,
                messages: vec![MessageHistoryEntry {
                    message_id: "msg-dup".to_string(),
                    upstream_message_id: "msg-dup.PNM".to_string(),
                    version: 1748921205690,
                    ts: 1748921205690,
                    kind: "text".to_string(),
                    text: "提醒".to_string(),
                    preview: "提醒".to_string(),
                    unread_count: 1,
                    ..Default::default()
                }],
            },
            true,
        )
        .await
        .unwrap();

        let live_unread = sqlx::query_scalar::<_, i64>(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(live_unread, 0);

        let unread_rows = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM message_history_cache WHERE uid = ? AND session_id = ? AND is_self = 0 AND unread_count > 0",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(unread_rows, 0);
    }

    #[tokio::test]
    async fn persist_message_history_page_preserves_recalled_rows_on_history_resync() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-recall";
        let message_id = "msg-1";
        let version = 123_i64;

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, title, text, image_url, image_urls,
                content_type, preview, sender_nick, sender_logo, is_self,
                card_meta, recalled_at, recalled_original_text, recalled_original_image_url,
                reply_to_message_id, reply_to_text, cached_at
            )
            VALUES (?, ?, 1, ?, 'pnm-1.PNM', ?, ?, 'recalled', '', '对方撤回了一条消息',
                    '', '[]', 27, '对方撤回了一条消息', '买家', '', 0,
                    '', 456, '原消息', '', '', '', unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(message_id)
        .bind(version)
        .bind(version)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-1.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "unknown".to_string(),
                    preview: "[消息类型 0]".to_string(),
                    sender_nick: "买家".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT kind, text, preview, content_type, recalled_at, recalled_original_text \
             FROM message_history_cache WHERE uid = ? AND session_id = ? AND message_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("kind"), "recalled");
        assert_eq!(row.get::<String, _>("text"), "对方撤回了一条消息");
        assert_eq!(row.get::<String, _>("preview"), "对方撤回了一条消息");
        assert_eq!(row.get::<i64, _>("content_type"), 27);
        assert_eq!(row.get::<i64, _>("recalled_at"), 456);
        assert_eq!(row.get::<String, _>("recalled_original_text"), "原消息");
    }

    #[tokio::test]
    async fn persist_message_history_page_preserves_recalled_session_summary_on_history_resync() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-recall-summary";
        let message_id = "msg-latest";
        let version = 456_i64;

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, title, text, image_url, image_urls,
                content_type, preview, sender_nick, sender_logo, is_self,
                card_meta, recalled_at, recalled_original_text, recalled_original_image_url,
                reply_to_message_id, reply_to_text, cached_at
            )
            VALUES (?, ?, 1, ?, 'pnm-latest.PNM', ?, ?, 'recalled', '', '对方撤回了一条消息',
                    '', '[]', 27, '对方撤回了一条消息', '买家', '', 0,
                    '', 789, '原消息', '', '', '', unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(message_id)
        .bind(version)
        .bind(version)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version,
                    latest_message_id: message_id.to_string(),
                    summary: "[消息类型 0]".to_string(),
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-latest.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "unknown".to_string(),
                    preview: "[消息类型 0]".to_string(),
                    sender_nick: "买家".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

        let row =
            sqlx::query("SELECT summary FROM message_live_state WHERE uid = ? AND session_id = ?")
                .bind(uid)
                .bind(session_id)
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(row.get::<String, _>("summary"), "对方撤回了一条消息");
    }

    #[tokio::test]
    async fn persist_message_history_page_upgrades_unknown_row_when_history_contains_recall_feature(
    ) {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-recall-upgrade";
        let message_id = "msg-recalled";
        let version = 789_i64;

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version,
                    latest_message_id: message_id.to_string(),
                    summary: "[消息类型 0]".to_string(),
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-recalled.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "unknown".to_string(),
                    content_type: 0,
                    preview: "[消息类型 0]".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version,
                    latest_message_id: message_id.to_string(),
                    summary: "对方撤回了一条消息".to_string(),
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-recalled.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "recalled".to_string(),
                    text: "对方撤回了一条消息".to_string(),
                    content_type: 27,
                    preview: "对方撤回了一条消息".to_string(),
                    recalled_at: version + 10,
                    recalled_original_text: "撤回前原文".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

        let history = sqlx::query(
            "SELECT kind, text, preview, content_type, recalled_at, recalled_original_text \
             FROM message_history_cache WHERE uid = ? AND session_id = ? AND message_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(history.get::<String, _>("kind"), "recalled");
        assert_eq!(history.get::<String, _>("text"), "对方撤回了一条消息");
        assert_eq!(history.get::<String, _>("preview"), "对方撤回了一条消息");
        assert_eq!(history.get::<i64, _>("content_type"), 27);
        assert_eq!(history.get::<i64, _>("recalled_at"), version + 10);
        assert_eq!(
            history.get::<String, _>("recalled_original_text"),
            "撤回前原文"
        );

        let live =
            sqlx::query("SELECT summary FROM message_live_state WHERE uid = ? AND session_id = ?")
                .bind(uid)
                .bind(session_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(live.get::<String, _>("summary"), "对方撤回了一条消息");
    }

    #[tokio::test]
    async fn persist_message_history_page_keeps_cached_original_text_when_recall_feature_has_no_text(
    ) {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-recall-original";
        let message_id = "msg-recalled";
        let version = 888_i64;

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version,
                    latest_message_id: message_id.to_string(),
                    summary: "撤回前缓存原文".to_string(),
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-recalled.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "text".to_string(),
                    text: "撤回前缓存原文".to_string(),
                    content_type: 1,
                    preview: "撤回前缓存原文".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

        persist_message_history_page(
            &pool,
            uid,
            &MessageHistoryPage {
                session: MessageSessionIdentity {
                    session_id: session_id.to_string(),
                    session_type: 1,
                    version,
                    latest_message_id: message_id.to_string(),
                    summary: "对方撤回了一条消息".to_string(),
                    ..Default::default()
                },
                messages: vec![MessageHistoryEntry {
                    message_id: message_id.to_string(),
                    upstream_message_id: "pnm-recalled.PNM".to_string(),
                    version,
                    ts: version,
                    kind: "recalled".to_string(),
                    text: "对方撤回了一条消息".to_string(),
                    content_type: 27,
                    preview: "对方撤回了一条消息".to_string(),
                    recalled_at: version + 10,
                    ..Default::default()
                }],
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT text, recalled_original_text FROM message_history_cache \
             WHERE uid = ? AND session_id = ? AND message_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<String, _>("text"), "对方撤回了一条消息");
        assert_eq!(
            row.get::<String, _>("recalled_original_text"),
            "撤回前缓存原文"
        );
    }

    #[test]
    fn sort_session_values_promotes_unread_user_sessions() {
        let mut sessions = vec![
            json!({"sessionId": "new-read", "sessionType": 1, "unread": 0, "ts": 300}),
            json!({"sessionId": "old-unread", "sessionType": 1, "unread": 3, "ts": 100}),
        ];

        sort_session_values(&mut sessions);

        assert_eq!(
            sessions[0].get("sessionId").and_then(|v| v.as_str()),
            Some("old-unread")
        );
    }

    #[tokio::test]
    async fn persist_message_sessions_does_not_overwrite_real_nick_with_placeholder_copy() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-1";

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 10,
                peer_uid: "peer-1".to_string(),
                peer_nick: "真实买家昵称".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 11,
                peer_uid: "peer-1".to_string(),
                peer_nick: "你有一条新消息".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT peer_nick FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("peer_nick"), "真实买家昵称");
    }

    #[tokio::test]
    async fn persist_message_sessions_protects_real_nick_from_all_placeholder_forms() {
        for placeholder in [
            "未知",
            "未知会话",
            "未知账号",
            "未知客户",
            "买家已拍下，待付款",
            "卖家人不错？送Ta闲鱼小红花",
            "用户_65320912",
            "tbNick_65320912",
            "65320912",
        ] {
            let pool = setup_pool().await;
            let uid = "uid-1";
            let session_id = format!("sess-{placeholder}");

            persist_message_sessions(
                &pool,
                uid,
                &[MessageSessionIdentity {
                    session_id: session_id.clone(),
                    session_type: 1,
                    version: 10,
                    peer_uid: "65320912".to_string(),
                    peer_nick: "Chen".to_string(),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

            persist_message_sessions(
                &pool,
                uid,
                &[MessageSessionIdentity {
                    session_id: session_id.clone(),
                    session_type: 1,
                    version: 11,
                    peer_uid: "65320912".to_string(),
                    peer_nick: placeholder.to_string(),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

            let row = sqlx::query(
                "SELECT peer_nick FROM message_live_state WHERE uid = ? AND session_id = ?",
            )
            .bind(uid)
            .bind(&session_id)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(
                row.get::<String, _>("peer_nick"),
                "Chen",
                "placeholder {placeholder:?} should not overwrite real nick"
            );
        }
    }

    #[tokio::test]
    async fn persist_message_sessions_replaces_placeholder_nick_with_real_nick() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-placeholder";

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 10,
                peer_uid: "65320912".to_string(),
                peer_nick: "用户_65320912".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 11,
                peer_uid: "65320912".to_string(),
                peer_nick: "Chen".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT peer_nick FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("peer_nick"), "Chen");
    }

    #[tokio::test]
    async fn persist_message_sessions_suppresses_legacy_platform_reminder_unread() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-reminder";

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 100,
                unread: 16,
                latest_message_id: "mid-1".to_string(),
                peer_uid: "peer-1".to_string(),
                peer_nick: "卡券俱乐部".to_string(),
                summary: "快给ta一个评价吧～".to_string(),
                ts: 100,
                sort_index: 100,
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT unread_count, summary FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<i64, _>("unread_count"), 0);
        assert_eq!(row.get::<String, _>("summary"), "快给ta一个评价吧～");
    }

    #[tokio::test]
    async fn persist_message_sessions_keeps_cached_read_state_for_same_latest_chat_message() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-read-proof";
        let latest_message_id = "msg-latest";

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES
                (?, ?, 1, ?, 'pnm-latest.PNM', 10002, 10002, 'text', '120整套出', '120整套出', 0, 0, unixepoch('subsec')),
                (?, ?, 1, ?, 'pnm-latest.PNM', 1780993530039, 1780993530039, 'text', '120整套出', '120整套出', 0, -1, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 1780993530039,
                latest_message_id: latest_message_id.to_string(),
                unread: 1,
                summary: "120整套出".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<i64, _>("unread_count"), 0);
    }

    #[tokio::test]
    async fn persist_message_sessions_keeps_cached_read_state_for_same_latest_notification_message()
    {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-notice-read-proof";

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 3, 'notice-msg-1', '', 1782173948000, 1782173948822, 'text', '有人@我，快来0.01元兑好礼🧧', '有人@我，快来0.01元兑好礼🧧', 0, 0, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 3,
                version: 1782173948000,
                latest_message_id: String::new(),
                unread: 2,
                summary: "有人@我，快来0.01元兑好礼🧧".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<i64, _>("unread_count"), 0);
    }

    #[tokio::test]
    async fn persist_message_sessions_keeps_chat_unread_when_cache_has_no_read_proof() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-no-read-proof";
        let latest_message_id = "msg-latest";

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 1, ?, 'pnm-latest.PNM', 1780993530039, 1780993530039, 'text', '120整套出', '120整套出', 0, -1, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 1780993530039,
                latest_message_id: latest_message_id.to_string(),
                unread: 1,
                summary: "120整套出".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<i64, _>("unread_count"), 1);
    }

    #[tokio::test]
    async fn persist_message_sessions_uses_cached_live_latest_message_when_incoming_latest_id_is_blank(
    ) {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-existing-latest";
        let latest_message_id = "msg-latest";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count, updated_at
            ) VALUES (?, ?, 1, 1780993530039, ?, 0, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 1, ?, 'pnm-latest.PNM', 10002, 10002, 'text', '120整套出', '120整套出', 0, 0, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .execute(&pool)
        .await
        .unwrap();

        persist_message_sessions(
            &pool,
            uid,
            &[MessageSessionIdentity {
                session_id: session_id.to_string(),
                session_type: 1,
                version: 1780993530039,
                latest_message_id: String::new(),
                unread: 1,
                summary: "120整套出".to_string(),
                ..Default::default()
            }],
        )
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT unread_count FROM message_live_state WHERE uid = ? AND session_id = ?",
        )
        .bind(uid)
        .bind(session_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.get::<i64, _>("unread_count"), 0);
    }

    #[tokio::test]
    async fn load_sessions_for_uid_hides_legacy_platform_reminder_unread_from_stale_rows() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-stale-reminder";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                peer_uid, peer_nick, summary, last_message_ts, updated_at
            ) VALUES (?, ?, 1, 100, 'mid-1', 9, 'peer-1', '卡券俱乐部', '你已被禁言，点此查看详情', 100, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        let sessions = load_sessions_for_uid(&pool, uid, 20).await.unwrap();
        let session = sessions
            .into_iter()
            .find(|value| value["sessionId"] == session_id)
            .expect("session should exist");

        assert_eq!(session["unread"], 0);
        assert_eq!(session["summary"], "你已被禁言，点此查看详情");
    }

    #[tokio::test]
    async fn load_sessions_for_uid_uses_cached_read_proof_for_stale_chat_unread() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-stale-chat";
        let latest_message_id = "msg-latest";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                summary, last_message_ts, updated_at
            ) VALUES (?, ?, 1, 1780993530039, ?, 1, '120整套出', 1780993530039, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 1, ?, 'pnm-latest.PNM', 10002, 10002, 'text', '120整套出', '120整套出', 0, 0, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .bind(latest_message_id)
        .execute(&pool)
        .await
        .unwrap();

        let sessions = load_sessions_for_uid(&pool, uid, 20).await.unwrap();
        let session = sessions
            .into_iter()
            .find(|value| value["sessionId"] == session_id)
            .expect("session should exist");

        assert_eq!(session["unread"], 0);
    }

    #[tokio::test]
    async fn load_sessions_for_uid_uses_cached_read_proof_for_stale_notification_unread() {
        let pool = setup_pool().await;
        let uid = "uid-1";
        let session_id = "sess-stale-notice";

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                summary, last_message_ts, updated_at
            ) VALUES (?, ?, 3, 1782173948000, '', 2, '有人@我，快来0.01元兑好礼🧧', 1782173948822, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, session_type, message_id, upstream_message_id,
                version, ts, kind, text, preview, is_self, unread_count, cached_at
            ) VALUES (?, ?, 3, 'notice-msg-1', '', 1782173948000, 1782173948822, 'text', '有人@我，快来0.01元兑好礼🧧', '有人@我，快来0.01元兑好礼🧧', 0, 0, unixepoch('subsec'))
            "#,
        )
        .bind(uid)
        .bind(session_id)
        .execute(&pool)
        .await
        .unwrap();

        let sessions = load_sessions_for_uid(&pool, uid, 20).await.unwrap();
        let session = sessions
            .into_iter()
            .find(|value| value["sessionId"] == session_id)
            .expect("session should exist");

        assert_eq!(session["unread"], 0);
    }
}

// ---------------------------------------------------------------------------
// Sort helpers (used by load_sessions_for_uid)
// ---------------------------------------------------------------------------

fn sort_session_values(sessions: &mut [Value]) {
    sessions.sort_by(|left, right| {
        session_type_priority(left)
            .cmp(&session_type_priority(right))
            .then_with(|| unread_priority(right).cmp(&unread_priority(left)))
            .then_with(|| {
                normalized_session_sort_value(right).cmp(&normalized_session_sort_value(left))
            })
            .then_with(|| {
                json_string(left.get("sessionId")).cmp(&json_string(right.get("sessionId")))
            })
    });
}

fn unread_priority(session: &Value) -> i64 {
    if session.get("unread").and_then(Value::as_i64).unwrap_or(0) > 0 {
        1
    } else {
        0
    }
}

fn session_type_priority(session: &Value) -> i64 {
    match session
        .get("sessionType")
        .and_then(Value::as_i64)
        .unwrap_or(1)
    {
        3 | 6 => 0,
        _ => 1,
    }
}

fn normalized_session_sort_value(session: &Value) -> i64 {
    let primary = session
        .get("sortIndex")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if primary > 0 {
        let primary_text = primary.to_string();
        if primary_text.len() == 14 && primary_text.starts_with("20") {
            let ts = session.get("ts").and_then(Value::as_i64).unwrap_or(0);
            if ts > 0 {
                return ts;
            }
        }
        return primary;
    }

    session
        .get("ts")
        .and_then(Value::as_i64)
        .or_else(|| session.get("version").and_then(Value::as_i64))
        .unwrap_or(0)
}
