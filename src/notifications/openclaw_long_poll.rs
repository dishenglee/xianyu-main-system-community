//! Background long-poll worker for an openclaw_weixin channel.
//!
//! Hits `<base_url>/ilink/bot/getupdates` continuously. On each inbound USER
//! message it (a) caches the `context_token` so outbound replies can thread,
//! and (b) dispatches the text to the command router.
//!
//! Cursor (`get_updates_buf`) is persisted to
//! `notification_channel_runtime.runtime_json.get_updates_buf` after every
//! successful poll, so a backend restart resumes where it left off.
//!
//! Lifecycle:
//!   * spawned on backend startup for every enabled openclaw_weixin channel
//!   * spawned on demand when the QR auto-create flow inserts a new channel
//!   * exits gracefully on `errcode == -14` (session timed out, re-scan QR)
//!   * exits when its `JoinHandle` is aborted (channel deletion / restart)

use std::time::Duration;

use serde_json::{Map, Value};
use tracing::{info, warn};

use crate::state::AppState;

use super::{
    openclaw_context_cache,
    openclaw_weixin::{self, GetUpdatesResp, InboundMsg},
    wechat_commands,
};

/// Sleep after a transport / non-zero `ret` failure before retrying. Short
/// enough that a transient blip doesn't keep the queue stalled; long enough
/// that a hard outage doesn't melt CPU.
const RETRY_BACKOFF: Duration = Duration::from_secs(5);

/// Fatal error code from the upstream — session/token timed out and a
/// re-scan is required. Worker exits when it sees this.
const ERRCODE_SESSION_TIMEOUT: i64 = -14;

/// Spawn the long-poll worker as a detached tokio task. The returned
/// `JoinHandle` is held by `WechatWorkersState` so the worker can be aborted
/// when the channel is deleted or replaced.
pub(crate) fn spawn_long_poll_worker(
    state: AppState,
    channel_id: i64,
    base_url: String,
    token: String,
    bot_agent: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        run_long_poll_loop(state, channel_id, base_url, token, bot_agent).await;
    })
}

/// Worker body. Single async function so we can early-return on fatal errors
/// without polluting the spawn site.
async fn run_long_poll_loop(
    state: AppState,
    channel_id: i64,
    base_url: String,
    token: String,
    bot_agent: String,
) {
    // Hydrate cursor + context tokens from the persisted runtime blob.
    let mut cursor = load_initial_cursor(&state, channel_id).await;

    info!(
        target: "notifications.openclaw_long_poll",
        stream = "runtime",
        module = "notifications-openclaw-long-poll",
        action = "worker_start",
        outcome = "success",
        channel_id,
        base_url = %base_url,
        bot_agent = %bot_agent,
        cursor_len = cursor.len(),
        "starting openclaw long-poll worker"
    );

    loop {
        let resp = match openclaw_weixin::get_updates(&base_url, &token, &bot_agent, &cursor).await
        {
            Ok(resp) => resp,
            Err(err) => {
                warn!(
                    target: "notifications.openclaw_long_poll",
                    stream = "runtime",
                    module = "notifications-openclaw-long-poll",
                    action = "get_updates_transport",
                    outcome = "warn",
                    channel_id,
                    error_code = "openclaw_get_updates_transport_failed",
                    retryable = true,
                    detail = %err,
                    "openclaw get_updates transport failed; backing off"
                );
                tokio::time::sleep(RETRY_BACKOFF).await;
                continue;
            }
        };

        if resp.ret != 0 {
            if resp.errcode == ERRCODE_SESSION_TIMEOUT {
                warn!(
                    target: "notifications.openclaw_long_poll",
                    stream = "runtime",
                    module = "notifications-openclaw-long-poll",
                    action = "session_timeout",
                    outcome = "warn",
                    channel_id,
                    error_code = "openclaw_session_timeout",
                    retryable = false,
                    upstream_errcode = resp.errcode,
                    detail = ?resp.errmsg,
                    "openclaw session timed out; worker exiting"
                );
                return;
            }
            warn!(
                target: "notifications.openclaw_long_poll",
                stream = "runtime",
                module = "notifications-openclaw-long-poll",
                action = "get_updates_nonzero_ret",
                outcome = "warn",
                channel_id,
                error_code = "openclaw_get_updates_nonzero_ret",
                retryable = true,
                ret = resp.ret,
                upstream_errcode = resp.errcode,
                detail = ?resp.errmsg,
                "openclaw get_updates returned non-zero ret"
            );
            tokio::time::sleep(RETRY_BACKOFF).await;
            continue;
        }

        process_messages(&state, channel_id, &resp).await;

        // Persist the new cursor BEFORE looping so a crash doesn't redeliver
        // an already-handled batch.
        if !resp.get_updates_buf.is_empty() && resp.get_updates_buf != cursor {
            cursor = resp.get_updates_buf.clone();
            if let Err(err) = persist_cursor(&state, channel_id, &cursor).await {
                warn!(
                    target: "notifications.openclaw_long_poll",
                    stream = "runtime",
                    module = "notifications-openclaw-long-poll",
                    action = "persist_cursor",
                    outcome = "warn",
                    channel_id,
                    error_code = "openclaw_cursor_persist_failed",
                    retryable = true,
                    detail = %err,
                    "failed to persist openclaw long-poll cursor"
                );
            }
        }
    }
}

/// Hydrate cursor + context_tokens from the persisted runtime row at startup.
async fn load_initial_cursor(state: &AppState, channel_id: i64) -> String {
    match state.storage().get_notification_runtime(channel_id).await {
        Ok(runtime) => {
            openclaw_context_cache::hydrate_from_runtime(channel_id, &runtime.runtime_json);
            runtime
                .runtime_json
                .get("get_updates_buf")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        }
        Err(err) => {
            warn!(
                target: "notifications.openclaw_long_poll",
                stream = "runtime",
                module = "notifications-openclaw-long-poll",
                action = "load_initial_cursor",
                outcome = "warn",
                channel_id,
                error_code = "openclaw_cursor_load_failed",
                retryable = true,
                detail = %err,
                "failed to load persisted openclaw cursor; starting empty"
            );
            String::new()
        }
    }
}

/// Fan-out per-message handling: skip non-USER / non-finished frames,
/// cache the context_token, and dispatch text to the command router.
async fn process_messages(state: &AppState, channel_id: i64, resp: &GetUpdatesResp) {
    for msg in &resp.msgs {
        if msg.message_type == 2 {
            // BOT echo — our own outbound surfaced on the long-poll stream.
            continue;
        }
        if msg.message_state != 2 {
            // 0 / 1 = streaming / incomplete chunk; only act on finished.
            continue;
        }
        if msg.from_user_id.trim().is_empty() {
            continue;
        }
        if !msg.context_token.trim().is_empty() {
            openclaw_context_cache::save(
                state,
                channel_id,
                msg.from_user_id.trim(),
                msg.context_token.trim(),
            );
        }

        dispatch_inbound(state, channel_id, msg).await;
    }
}

/// Per-message dispatcher: extract text, route to the command parser. The
/// command router itself returns no value — replies are sent back through
/// `send_text` directly inside the handler.
async fn dispatch_inbound(state: &AppState, channel_id: i64, msg: &InboundMsg) {
    let text = openclaw_weixin::extract_text_from_items(&msg.item_list);
    let ref_text = openclaw_weixin::extract_ref_text_from_items(&msg.item_list);
    if text.is_empty() {
        // Non-text inbound (image, link card, etc.) — phase 2A is text-only.
        return;
    }
    wechat_commands::dispatch_inbound_text(state, channel_id, &msg.from_user_id, &text, &ref_text)
        .await;
}

/// Patch the channel's runtime_json with the latest cursor. Uses
/// `patch_notification_runtime` so other runtime fields (status,
/// context_tokens, last_callback_*) are preserved.
async fn persist_cursor(state: &AppState, channel_id: i64, cursor: &str) -> Result<(), String> {
    let mut patch = Map::new();
    patch.insert(
        "get_updates_buf".to_string(),
        Value::String(cursor.to_string()),
    );
    state
        .storage()
        .patch_notification_runtime(channel_id, &patch)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}
