//! Unified worker lifecycle manager for notification channels that require
//! background listeners.
//!
//! This module is the single lifecycle surface for listener-backed channels.
//! It owns every spawned worker `JoinHandle` and normalizes four things across
//! startup scans, route-driven config updates, QR auto-provision, and
//! dispatcher self-heal restarts:
//!
//! 1. channel-type resolution
//! 2. stop + spawn behavior
//! 3. `notification_channel_runtime` restart patch semantics
//! 4. skip / error observability
//!
//! Public entrypoints:
//!   * `start_managed_channel_workers` — backend startup bulk scan for every
//!     enabled channel whose type has a managed worker implementation.
//!   * `restart_worker_for_channel` — per-channel stop + respawn through the
//!     same normalized worker lifecycle path used by routes and dispatcher
//!     hooks.
//!
//! `WechatWorkersState` stays in `AppState` as a cloneable handle registry so
//! bootstrap and runtime hooks can coordinate worker restarts without threading
//! extra state through every layer.

use std::{collections::HashMap, sync::Arc};

use serde_json::{Map, Value};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{models::notifications::NotificationChannel, state::AppState};

use super::{
    driver::{build_runtime_error_update, build_runtime_success_update},
    feishu_native::{ChannelStartRequest, ChannelStopRequest},
    openclaw_long_poll::spawn_long_poll_worker,
    sanitize::safe_worker_error_message,
};

/// Shared registry of running worker tasks, keyed by channel_id.
///
/// `Arc<Mutex<...>>` because:
///   * `AppState` is `Clone` — every clone shares the same registry.
///   * We need `&mut` access to insert / abort, hence the lock.
///   * `Mutex<HashMap<JoinHandle>>` is fine since the critical section is
///     short (just insert / abort) and contention is low (a handful of
///     workers at most).
#[derive(Debug, Clone, Default)]
pub struct WechatWorkersState {
    inner: Arc<Mutex<HashMap<i64, JoinHandle<()>>>>,
}

impl WechatWorkersState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Abort the worker (if any) for `channel_id` and forget the handle.
    /// Idempotent — calling on a channel without a running worker is a noop.
    pub async fn stop(&self, channel_id: i64) {
        let mut guard = self.inner.lock().await;
        if let Some(handle) = guard.remove(&channel_id) {
            handle.abort();
        }
    }

    /// Insert / replace the worker handle for `channel_id`. If a previous
    /// handle exists it is aborted first.
    pub async fn store(&self, channel_id: i64, handle: JoinHandle<()>) {
        let mut guard = self.inner.lock().await;
        if let Some(old) = guard.insert(channel_id, handle) {
            old.abort();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerKind {
    OpenclawWeixin,
    Feishu,
    Lark,
}

impl WorkerKind {
    fn label(self) -> &'static str {
        match self {
            Self::OpenclawWeixin => "openclaw_weixin",
            Self::Feishu => "feishu",
            Self::Lark => "lark",
        }
    }

    fn runtime_source(self) -> &'static str {
        match self {
            Self::OpenclawWeixin => "wechat.manager",
            Self::Feishu | Self::Lark => "feishu.manager",
        }
    }

    fn is_feishu_like(self) -> bool {
        matches!(self, Self::Feishu | Self::Lark)
    }
}

/// Backend startup scan for all enabled channels whose type has a managed
/// worker implementation. Unsupported types are ignored here because startup
/// only concerns listener-backed channels.
pub async fn start_managed_channel_workers(state: AppState) {
    let channels = match state.storage().list_notification_channels().await {
        Ok(rows) => rows,
        Err(err) => {
            eprintln!("[notify.manager] startup: failed to list notification channels: {err}");
            return;
        }
    };

    let mut scanned = 0usize;
    let mut spawned = 0usize;
    let mut skipped_disabled = 0usize;
    let mut skipped_unsupported = 0usize;
    let mut skipped_misconfigured = 0usize;

    for channel in channels {
        let Some(worker_kind) = worker_kind_for_channel_type(&channel.channel_type) else {
            skipped_unsupported += 1;
            continue;
        };

        scanned += 1;

        if !channel.enabled {
            skipped_disabled += 1;
            continue;
        }

        match spawn_worker_for_channel(state.clone(), &channel, worker_kind).await {
            Ok(()) => {
                spawned += 1;
                if worker_kind.is_feishu_like() {
                    patch_restart_spawned(&state, channel.id, worker_kind).await;
                }
            }
            Err(err) => {
                skipped_misconfigured += 1;
                let safe_err = safe_worker_error_message(&err);
                eprintln!(
                    "[{}] startup: channel {} skip — {}",
                    worker_kind.runtime_source(),
                    channel.id,
                    safe_err
                );
            }
        }
    }

    eprintln!(
        "[notify.manager] startup: scanned={scanned} spawned={spawned} skipped_disabled={skipped_disabled} skipped_unsupported={skipped_unsupported} skipped_misconfigured={skipped_misconfigured}"
    );
}

/// Stop any existing worker for `channel_id` and spawn a fresh one through a
/// single normalized channel-type entrypoint.
pub async fn restart_worker_for_channel(state: AppState, channel_id: i64) {
    let channel = match state.storage().get_notification_channel(channel_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            eprintln!("[notify.manager] restart: channel {channel_id} not found");
            return;
        }
        Err(err) => {
            eprintln!("[notify.manager] restart: channel {channel_id} load failed: {err}");
            return;
        }
    };

    restart_loaded_channel_worker(state, &channel).await;
}

pub async fn stop_worker_for_channel(state: AppState, channel: &NotificationChannel) {
    let Some(worker_kind) = worker_kind_for_channel_type(&channel.channel_type) else {
        return;
    };
    stop_worker_for_kind(&state, worker_kind, channel.id).await;
}

async fn restart_loaded_channel_worker(state: AppState, channel: &NotificationChannel) {
    let Some(worker_kind) = worker_kind_for_channel_type(&channel.channel_type) else {
        patch_restart_skip(
            &state,
            channel.id,
            &channel.channel_type,
            "no worker implemented for channel type",
        )
        .await;
        eprintln!(
            "[notify.manager] restart: channel {} skip — no worker implemented for type {}",
            channel.id, channel.channel_type
        );
        return;
    };

    stop_worker_for_kind(&state, worker_kind, channel.id).await;

    if !channel.enabled {
        patch_restart_skip(
            &state,
            channel.id,
            &channel.channel_type,
            "channel disabled",
        )
        .await;
        eprintln!(
            "[{}] restart: channel {} skip — disabled",
            worker_kind.runtime_source(),
            channel.id
        );
        return;
    }

    match spawn_worker_for_channel(state.clone(), channel, worker_kind).await {
        Ok(()) => {
            patch_restart_spawned(&state, channel.id, worker_kind).await;
        }
        Err(err) => {
            let safe_err = safe_worker_error_message(&err);
            patch_restart_error(&state, channel.id, worker_kind, &safe_err).await;
            eprintln!(
                "[{}] restart: channel {} skip — {}",
                worker_kind.runtime_source(),
                channel.id,
                safe_err
            );
        }
    }
}

fn worker_kind_for_channel_type(channel_type: &str) -> Option<WorkerKind> {
    match channel_type {
        "openclaw_weixin" => Some(WorkerKind::OpenclawWeixin),
        "feishu" => Some(WorkerKind::Feishu),
        "lark" => Some(WorkerKind::Lark),
        _ => None,
    }
}

async fn stop_worker_for_kind(state: &AppState, worker_kind: WorkerKind, channel_id: i64) {
    match worker_kind {
        WorkerKind::OpenclawWeixin => state.wechat_workers().stop(channel_id).await,
        WorkerKind::Feishu | WorkerKind::Lark => {
            if let Err(err) = state
                .feishu_native()
                .channel_stop(ChannelStopRequest { channel_id })
                .await
            {
                let safe_err = safe_worker_error_message(&err.to_string());
                eprintln!(
                    "[{}] channel {channel_id} native transport stop failed: {safe_err}",
                    worker_kind.runtime_source()
                );
            }
        }
    }
}

async fn spawn_worker_for_channel(
    state: AppState,
    channel: &NotificationChannel,
    worker_kind: WorkerKind,
) -> Result<(), String> {
    match worker_kind {
        WorkerKind::OpenclawWeixin => spawn_openclaw_worker(state, channel).await,
        WorkerKind::Feishu | WorkerKind::Lark => {
            spawn_feishu_native_channel(state, channel, worker_kind).await
        }
    }
}

/// Extract config + spawn a single worker, storing the handle in the
/// registry. Validates config presence up front so a half-configured
/// channel doesn't burn CPU with a tight transport-error retry loop.
async fn spawn_openclaw_worker(
    state: AppState,
    channel: &NotificationChannel,
) -> Result<(), String> {
    let base_url = channel
        .config
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let token = channel
        .config
        .get("bot_token")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let bot_agent = channel
        .config
        .get("bot_agent")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    if base_url.is_empty() || token.is_empty() {
        return Err("missing base_url or bot_token".to_string());
    }

    let workers = state.wechat_workers().clone();
    let handle = spawn_long_poll_worker(state, channel.id, base_url, token, bot_agent);
    workers.store(channel.id, handle).await;
    Ok(())
}

async fn spawn_feishu_native_channel(
    state: AppState,
    channel: &NotificationChannel,
    worker_kind: WorkerKind,
) -> Result<(), String> {
    let app_id = config_string(&channel.config, "app_id");
    let app_secret = config_string(&channel.config, "app_secret");
    if app_id.is_empty() || app_secret.is_empty() {
        return Err("missing app_id or app_secret".to_string());
    }

    let domain = config_string(&channel.config, "domain");
    let domain = if domain.is_empty() {
        worker_kind.label().to_string()
    } else {
        domain
    };
    let response = state
        .feishu_native()
        .channel_start(ChannelStartRequest {
            channel_id: channel.id,
            app_id,
            app_secret,
            domain,
            bot_open_id: config_optional_string(&channel.config, "bot_open_id"),
            bot_user_id: config_optional_string(&channel.config, "bot_user_id"),
        })
        .await
        .map_err(|err| err.to_string())?;

    if response.channel_id != channel.id {
        return Err(format!(
            "native Feishu channel id mismatch: expected {}, got {}",
            channel.id, response.channel_id
        ));
    }
    Ok(())
}

async fn patch_restart_spawned(state: &AppState, channel_id: i64, worker_kind: WorkerKind) {
    let mut patch = restart_runtime_patch(worker_kind, "spawned", "");
    patch.insert(
        "worker_channel_type".to_string(),
        Value::String(worker_kind.label().to_string()),
    );
    if worker_kind.is_feishu_like() {
        patch.insert(
            "transport_channel_status".to_string(),
            Value::String("started".to_string()),
        );
        patch.insert("transport_connected".to_string(), Value::Bool(true));
    }
    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .ok();
    let update = build_runtime_success_update(existing.as_ref(), &patch);
    persist_runtime_update(state, channel_id, update).await;
}

fn config_string(config: &Map<String, Value>, key: &str) -> String {
    config
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn config_optional_string(config: &Map<String, Value>, key: &str) -> Option<String> {
    let value = config_string(config, key);
    (!value.is_empty()).then_some(value)
}

async fn patch_restart_skip(state: &AppState, channel_id: i64, channel_type: &str, reason: &str) {
    let mut patch = Map::new();
    patch.insert(
        "worker_restart_status".to_string(),
        Value::String("skipped".to_string()),
    );
    patch.insert(
        "worker_restart_reason".to_string(),
        Value::String(reason.to_string()),
    );
    patch.insert(
        "worker_restart_source".to_string(),
        Value::String("notify.manager".to_string()),
    );
    patch.insert(
        "worker_channel_type".to_string(),
        Value::String(channel_type.to_string()),
    );
    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .ok();
    let update = build_runtime_success_update(existing.as_ref(), &patch);
    persist_runtime_update(state, channel_id, update).await;
}

async fn patch_restart_error(
    state: &AppState,
    channel_id: i64,
    worker_kind: WorkerKind,
    err: &str,
) {
    let mut patch = restart_runtime_patch(worker_kind, "error", err);
    patch.insert(
        "worker_channel_type".to_string(),
        Value::String(worker_kind.label().to_string()),
    );
    let existing = state
        .storage()
        .get_notification_runtime(channel_id)
        .await
        .ok();
    let update = build_runtime_error_update(existing.as_ref(), err, &patch);
    persist_runtime_update(state, channel_id, update).await;
}

fn restart_runtime_patch(
    worker_kind: WorkerKind,
    status: &str,
    reason: &str,
) -> Map<String, Value> {
    let mut patch = Map::new();
    patch.insert(
        "worker_restart_status".to_string(),
        Value::String(status.to_string()),
    );
    patch.insert(
        "worker_restart_reason".to_string(),
        Value::String(reason.to_string()),
    );
    patch.insert(
        "worker_restart_source".to_string(),
        Value::String(worker_kind.runtime_source().to_string()),
    );
    patch
}

async fn persist_runtime_update(
    state: &AppState,
    channel_id: i64,
    update: crate::notifications::driver::RuntimeUpdate,
) {
    if let Err(err) = state
        .storage()
        .upsert_notification_runtime(
            channel_id,
            update.status.as_str(),
            &update.last_error,
            Some(update.last_attempt_at),
            update.last_success_at,
            &update.runtime_json,
        )
        .await
    {
        eprintln!(
            "[notify.manager] channel={channel_id} failed to persist worker restart runtime: {err}"
        );
    }
}
