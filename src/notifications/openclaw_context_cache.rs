//! Per-(channel, peer) `context_token` cache for the openclaw_weixin channel.
//!
//! Tencent iLink's outbound `sendmessage` requires a `context_token` that is
//! only minted by the SAME conversation's earlier inbound `getupdates` push.
//! Without it the upstream returns `ret=-2`. This module caches the latest
//! token per `(channel_id, from_user_id)` pair so the dispatcher can look it
//! up at send time.
//!
//! The cache is in-memory plus a write-back to
//! `notification_channel_runtime.runtime_json.context_tokens` so a backend
//! restart doesn't black-hole every active conversation.
//!
//! Layout:
//!   in-memory:  HashMap<(channel_id, from_user_id), ContextEntry>
//!   persisted:  runtime_json.context_tokens : { "<from_user_id>": "<token>" }
//!
//! Persistence is best-effort — a failed write logs but never blocks the
//! long-poll loop.

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock, RwLock},
};

use serde_json::{Map, Value};
use tracing::warn;

use crate::state::AppState;

/// Cached entry for a single (channel, peer) pair.
///
/// `last_updated` is kept for future TTL / staleness reporting — phase 2A
/// doesn't expire entries, but the field documents the contract and is
/// cheap to populate.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ContextEntry {
    pub context_token: String,
    pub last_updated: i64,
}

type CacheMap = HashMap<(i64, String), ContextEntry>;

/// Process-wide singleton. The cache is small (one entry per active WeChat
/// peer conversation) so a plain RwLock-backed HashMap is fine — there's no
/// per-key locking pressure.
fn cache() -> &'static Arc<RwLock<CacheMap>> {
    static INSTANCE: OnceLock<Arc<RwLock<CacheMap>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Arc::new(RwLock::new(HashMap::new())))
}

/// Save (or overwrite) the context token for a given (channel, peer). Updates
/// the in-memory map immediately and spawns a fire-and-forget task to
/// persist into the channel's `runtime_json.context_tokens` blob.
pub(crate) fn save(state: &AppState, channel_id: i64, from_user_id: &str, context_token: &str) {
    if context_token.trim().is_empty() {
        return;
    }
    let from_user_id = from_user_id.to_string();
    let context_token = context_token.to_string();
    let now = unix_now();

    if let Ok(mut guard) = cache().write() {
        guard.insert(
            (channel_id, from_user_id.clone()),
            ContextEntry {
                context_token: context_token.clone(),
                last_updated: now,
            },
        );
    }

    let state = state.clone();
    tokio::spawn(async move {
        if let Err(err) =
            persist_context_token(&state, channel_id, &from_user_id, &context_token).await
        {
            warn!(
                target: "notifications.openclaw_context_cache",
                stream = "runtime",
                module = "notifications-openclaw-context-cache",
                action = "persist_context_token",
                outcome = "warn",
                channel_id,
                peer_user_id = %from_user_id,
                error_code = "context_token_persist_failed",
                retryable = true,
                detail = %err,
                "failed to persist openclaw context token"
            );
        }
    });
}

/// Lookup the latest context token for (channel, peer). Returns `None` when
/// either the channel has never received an inbound message from this peer,
/// or the cache hasn't been hydrated yet (cold restart before the first
/// inbound poll completes).
pub(crate) fn get(channel_id: i64, from_user_id: &str) -> Option<String> {
    let guard = cache().read().ok()?;
    guard
        .get(&(channel_id, from_user_id.to_string()))
        .map(|entry| entry.context_token.clone())
}

/// Drop the cached context token for one (channel, peer) pair both from the
/// in-memory map and the persisted runtime blob. Used when upstream rejects a
/// previously cached token with `ret=-2`, which means the session context is
/// no longer valid and must be re-established by a fresh inbound message.
pub(crate) async fn clear(
    state: &AppState,
    channel_id: i64,
    from_user_id: &str,
) -> Result<(), String> {
    if let Ok(mut guard) = cache().write() {
        guard.remove(&(channel_id, from_user_id.to_string()));
    }

    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .map_err(|err| err.to_string())?;
    let mut tokens = existing
        .runtime_json
        .get("context_tokens")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    tokens.remove(from_user_id);

    let mut patch = Map::new();
    patch.insert("context_tokens".into(), Value::Object(tokens));
    state
        .storage()
        .patch_notification_runtime(channel_id, &patch)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Hydrate the in-memory cache from `runtime_json.context_tokens` at startup
/// / worker spawn. Idempotent — repeated calls just overwrite the same keys.
pub(crate) fn hydrate_from_runtime(channel_id: i64, runtime_json: &Map<String, Value>) {
    let Some(tokens) = runtime_json
        .get("context_tokens")
        .and_then(Value::as_object)
    else {
        return;
    };

    let now = unix_now();
    let Ok(mut guard) = cache().write() else {
        return;
    };
    for (peer, token) in tokens {
        if let Some(token_str) = token.as_str() {
            let trimmed = token_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            guard.insert(
                (channel_id, peer.to_string()),
                ContextEntry {
                    context_token: trimmed.to_string(),
                    last_updated: now,
                },
            );
        }
    }
}

/// Merge the new (peer, token) into the channel's runtime_json.context_tokens
/// blob using the storage layer's `patch_notification_runtime` (which itself
/// does a read-merge-write under one ON CONFLICT statement).
async fn persist_context_token(
    state: &AppState,
    channel_id: i64,
    from_user_id: &str,
    context_token: &str,
) -> Result<(), String> {
    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .map_err(|err| err.to_string())?;

    let mut tokens = existing
        .runtime_json
        .get("context_tokens")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    tokens.insert(
        from_user_id.to_string(),
        Value::String(context_token.to_string()),
    );

    let mut patch = Map::new();
    patch.insert("context_tokens".into(), Value::Object(tokens));

    state
        .storage()
        .patch_notification_runtime(channel_id, &patch)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
