use std::{
    collections::HashMap,
    collections::HashSet,
    sync::{Arc, OnceLock, RwLock},
    time::{Duration, SystemTime},
};

use serde_json::Value;
use sqlx::query_scalar;
use tokio::sync::{broadcast, mpsc, oneshot, RwLock as AsyncRwLock};

use crate::{
    config::AppConfig,
    models::runtime::{
        CacheSyncRuntimeStatus, DatabaseStatus, DeleteRuntimeStatus, HealthStatus,
        RuntimeInspection, RuntimeWorkerStatus, StartupDependencyProbe, StartupDependencyStatus,
    },
    mtop_metrics::MtopMetrics,
    notifications::{
        feishu_events::handle_feishu_im_event,
        feishu_native::{FeishuNativeEventHandler, FeishuNativeRuntime},
        manager::WechatWorkersState,
        sanitize::safe_worker_error_message,
    },
    proxy_store::ProxyState,
    runtime_supervisor::RuntimeSupervisor,
    storage::{StorageError, StorageState},
    telemetry::{EnvironmentSnapshot, IncidentReporter},
};

/// Per-uid channel handle for writing to the live ACCS WebSocket owned by
/// `connect_and_stream_live_account`. The live runtime registers a sender
/// here each time it establishes a socket; HTTP send-message handlers look
/// up this map by uid to push outbound payloads into the active socket.
#[derive(Debug, Clone, Default)]
pub struct LiveSendersState {
    inner: Arc<AsyncRwLock<HashMap<String, mpsc::UnboundedSender<String>>>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveSenderSnapshot {
    pub uid: String,
    pub is_closed: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSenderReadiness {
    pub uid: String,
    pub has_sender: bool,
    pub sender_closed: bool,
    pub alive_sender_count: usize,
    pub total_sender_count: usize,
}

impl LiveSendersState {
    pub async fn register(&self, uid: String, tx: mpsc::UnboundedSender<String>) {
        self.inner.write().await.insert(uid, tx);
    }
    pub async fn unregister(&self, uid: &str) {
        self.inner.write().await.remove(uid);
    }
    /// Remove only if the stored sender is the same instance (same channel).
    ///
    /// Fixes a race: when a live worker's socket drops, its `SenderGuard`
    /// spawns an async unregister. If a reconnect completes before that spawn
    /// runs, the new `register(..)` will have overwritten the map entry — and
    /// the stale spawn would then wipe the *fresh* sender. By comparing the
    /// channel identity (`same_channel`), the old guard only clears the entry
    /// if nobody else has replaced it.
    pub async fn unregister_if_matches(&self, uid: &str, tx: &mpsc::UnboundedSender<String>) {
        let mut guard = self.inner.write().await;
        if let Some(existing) = guard.get(uid) {
            if existing.same_channel(tx) {
                guard.remove(uid);
            }
        }
    }
    pub async fn get(&self, uid: &str) -> Option<mpsc::UnboundedSender<String>> {
        self.inner.read().await.get(uid).cloned()
    }

    pub async fn snapshot(&self) -> Vec<LiveSenderSnapshot> {
        let guard = self.inner.read().await;
        let mut entries = guard
            .iter()
            .map(|(uid, sender)| LiveSenderSnapshot {
                uid: uid.clone(),
                is_closed: sender.is_closed(),
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.uid.cmp(&right.uid));
        entries
    }

    pub async fn readiness(&self, uid: &str) -> LiveSenderReadiness {
        let guard = self.inner.read().await;
        let total_sender_count = guard.len();
        let alive_sender_count = guard.values().filter(|sender| !sender.is_closed()).count();
        let sender_closed = guard
            .get(uid)
            .map(|sender| sender.is_closed())
            .unwrap_or(false);
        LiveSenderReadiness {
            uid: uid.to_string(),
            has_sender: guard.contains_key(uid),
            sender_closed,
            alive_sender_count,
            total_sender_count,
        }
    }

    /// Synchronous variant of `unregister_if_matches`, suitable for use in
    /// `Drop` impls. Returns `true` if the entry was removed (sender
    /// identity matched and lock was free), `false` if either the lock was
    /// contended or the stored sender belongs to a newer worker. Caller
    /// should fall back to async `unregister_if_matches` on `false` if
    /// cleanup is critical.
    pub fn try_unregister_if_matches_sync(
        &self,
        uid: &str,
        tx: &mpsc::UnboundedSender<String>,
    ) -> bool {
        let Ok(mut guard) = self.inner.try_write() else {
            return false;
        };
        if let Some(existing) = guard.get(uid) {
            if existing.same_channel(tx) {
                guard.remove(uid);
                return true;
            }
        }
        false
    }

    /// Fetch a sender for `uid` ONLY if the channel is still alive.
    /// Eagerly cleans up stale (channel-closed) entries — these happen when
    /// the worker's `out_rx` was dropped (WS disconnected) but the
    /// `SenderGuard`'s spawned async unregister hasn't yet been scheduled.
    /// Returns `None` after cleanup, so callers can treat it identically to
    /// "no worker connected".
    pub async fn get_alive(&self, uid: &str) -> Option<mpsc::UnboundedSender<String>> {
        let candidate = {
            let guard = self.inner.read().await;
            guard.get(uid).cloned()
        };
        let candidate = candidate?;
        if candidate.is_closed() {
            // Stale entry — drop it now so subsequent calls don't see it.
            self.unregister_if_matches(uid, &candidate).await;
            return None;
        }
        Some(candidate)
    }

    pub async fn wait_alive(
        &self,
        uid: &str,
        timeout: Duration,
    ) -> Option<mpsc::UnboundedSender<String>> {
        if let Some(sender) = self.get_alive(uid).await {
            return Some(sender);
        }
        if timeout.is_zero() {
            return None;
        }

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return self.get_alive(uid).await;
            }
            let remaining = deadline.saturating_duration_since(now);
            tokio::time::sleep(remaining.min(Duration::from_millis(150))).await;
            if let Some(sender) = self.get_alive(uid).await {
                return Some(sender);
            }
        }
    }
}

/// Per-(uid, mid) waiter registry for synchronous WS RPC calls (create_chat,
/// list_messages, send_image, ...). Mirrors Python's
/// `xianyu_wx_live_client.py::_pending_responses[mid] = future` pattern.
///
/// Caller flow:
///   1. `register(uid, mid, tx)` BEFORE sending the WS frame
///   2. push payload to `live_senders().get(uid)`
///   3. `tokio::time::timeout(t, rx).await` for the response
///   4. on timeout: `take(uid, mid)` to drop the dangling slot
///
/// Frame dispatcher in `connect_and_stream_live_account` calls
/// `take(uid, mid)` on every inbound frame whose headers carry a mid that
/// matches a registered waiter, and forwards the parsed JSON `Value` via
/// `tx.send(payload)`. Frame is still processed by sync_push / fallback
/// paths afterwards (the take just wakes the caller; it doesn't consume
/// the frame).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveResponseWaiterKind {
    Generic,
    ListMessages,
    SendMessage { expected_uuid: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaiterRegistrationError {
    Occupied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckDispatch {
    Delivered,
    Rejected,
    Missing,
}

#[derive(Debug)]
pub struct LiveResponseWaiter {
    pub tx: oneshot::Sender<Value>,
    pub kind: LiveResponseWaiterKind,
}

#[derive(Debug, Clone, Default)]
pub struct LiveResponseWaitersState {
    inner: Arc<AsyncRwLock<HashMap<String, LiveResponseWaiter>>>,
}

impl LiveResponseWaitersState {
    fn key(uid: &str, mid: &str) -> String {
        // Use a tab separator — neither uid nor mid contains it in practice
        // (uid is digits, mid is "{rand}{ts} 0"), keeps lookup O(hash).
        format!("{uid}\t{mid}")
    }
    pub async fn register(
        &self,
        uid: &str,
        mid: &str,
        tx: oneshot::Sender<Value>,
    ) -> Result<(), WaiterRegistrationError> {
        self.register_with_kind(uid, mid, tx, LiveResponseWaiterKind::Generic)
            .await
    }
    pub async fn register_send(
        &self,
        uid: &str,
        mid: &str,
        expected_uuid: &str,
        tx: oneshot::Sender<Value>,
    ) -> Result<(), WaiterRegistrationError> {
        self.register_with_kind(
            uid,
            mid,
            tx,
            LiveResponseWaiterKind::SendMessage {
                expected_uuid: expected_uuid.to_string(),
            },
        )
        .await
    }
    pub async fn register_with_kind(
        &self,
        uid: &str,
        mid: &str,
        tx: oneshot::Sender<Value>,
        kind: LiveResponseWaiterKind,
    ) -> Result<(), WaiterRegistrationError> {
        let mut guard = self.inner.write().await;
        match guard.entry(Self::key(uid, mid)) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(LiveResponseWaiter { tx, kind });
                Ok(())
            }
            std::collections::hash_map::Entry::Occupied(_) => {
                Err(WaiterRegistrationError::Occupied)
            }
        }
    }
    pub async fn contains(&self, uid: &str, mid: &str) -> bool {
        self.inner.read().await.contains_key(&Self::key(uid, mid))
    }
    pub async fn kind(&self, uid: &str, mid: &str) -> Option<LiveResponseWaiterKind> {
        self.inner
            .read()
            .await
            .get(&Self::key(uid, mid))
            .map(|waiter| waiter.kind.clone())
    }
    pub async fn dispatch_if_matches(&self, uid: &str, mid: &str, payload: &Value) -> AckDispatch {
        let key = Self::key(uid, mid);
        let mut guard = self.inner.write().await;
        let Some(waiter) = guard.get(&key) else {
            return AckDispatch::Missing;
        };
        if let LiveResponseWaiterKind::SendMessage { expected_uuid } = &waiter.kind {
            let body = payload.get("body").or_else(|| payload.get("data"));
            let accepted = payload.get("code").and_then(Value::as_i64) == Some(200)
                && body
                    .and_then(|value| value.get("messageId"))
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty())
                && body
                    .and_then(|value| value.get("uuid"))
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == expected_uuid);
            if !accepted {
                return AckDispatch::Rejected;
            }
        }
        let waiter = guard.remove(&key).expect("waiter exists while locked");
        drop(guard);
        let _ = waiter.tx.send(payload.clone());
        AckDispatch::Delivered
    }
    pub async fn take(&self, uid: &str, mid: &str) -> Option<LiveResponseWaiter> {
        self.inner.write().await.remove(&Self::key(uid, mid))
    }

    pub async fn take_first_of_kind_for_uid(
        &self,
        uid: &str,
        kind: LiveResponseWaiterKind,
    ) -> Option<LiveResponseWaiter> {
        let prefix = format!("{uid}\t");
        let mut guard = self.inner.write().await;
        let key = guard.iter().find_map(|(key, waiter)| {
            if key.starts_with(&prefix) && waiter.kind == kind {
                Some(key.clone())
            } else {
                None
            }
        })?;
        guard.remove(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_response_waiter_preserves_kind() {
        let state = LiveResponseWaitersState::default();
        let (tx, _rx) = oneshot::channel();
        state
            .register_with_kind("u1", "m1", tx, LiveResponseWaiterKind::ListMessages)
            .await
            .unwrap();

        let waiter = state.take("u1", "m1").await.expect("waiter");
        assert_eq!(waiter.kind, LiveResponseWaiterKind::ListMessages);
    }

    #[tokio::test]
    async fn live_response_waiter_can_fallback_to_uid_scoped_kind_lookup() {
        let state = LiveResponseWaitersState::default();
        let (tx, _rx) = oneshot::channel();
        state
            .register_with_kind("u1", "m1", tx, LiveResponseWaiterKind::ListMessages)
            .await
            .unwrap();

        let waiter = state
            .take_first_of_kind_for_uid("u1", LiveResponseWaiterKind::ListMessages)
            .await
            .expect("waiter");
        assert_eq!(waiter.kind, LiveResponseWaiterKind::ListMessages);
    }

    #[tokio::test]
    async fn registering_same_uid_mid_never_overwrites_existing_waiter() {
        let state = LiveResponseWaitersState::default();
        let (first_tx, _first_rx) = oneshot::channel();
        let (second_tx, _second_rx) = oneshot::channel();

        assert_eq!(
            state.register_send("u1", "mid-1", "uuid-1", first_tx).await,
            Ok(())
        );
        assert_eq!(
            state
                .register_send("u1", "mid-1", "uuid-2", second_tx)
                .await,
            Err(WaiterRegistrationError::Occupied)
        );
        assert!(state.contains("u1", "mid-1").await);
    }

    #[tokio::test]
    async fn send_waiter_requires_code_message_id_and_matching_uuid() {
        let state = LiveResponseWaitersState::default();
        let (tx, rx) = oneshot::channel();
        state
            .register_send("u1", "mid-1", "uuid-1", tx)
            .await
            .unwrap();

        for bad in [
            serde_json::json!({"code": 500, "body": {"messageId": "PNM-1", "uuid": "uuid-1"}}),
            serde_json::json!({"code": 200, "body": {"messageId": "", "uuid": "uuid-1"}}),
            serde_json::json!({"code": 200, "body": {"messageId": "PNM-1", "uuid": "uuid-other"}}),
        ] {
            assert_eq!(
                state.dispatch_if_matches("u1", "mid-1", &bad).await,
                AckDispatch::Rejected
            );
            assert!(state.contains("u1", "mid-1").await);
        }

        let valid = serde_json::json!({
            "code": 200,
            "body": {"messageId": "PNM-1", "uuid": "uuid-1"}
        });
        assert_eq!(
            state.dispatch_if_matches("u1", "mid-1", &valid).await,
            AckDispatch::Delivered
        );
        assert!(!state.contains("u1", "mid-1").await);
        assert_eq!(rx.await.unwrap(), valid);
    }

    #[tokio::test]
    async fn live_sender_wait_alive_returns_sender_registered_during_wait() {
        let senders = LiveSendersState::default();
        let delayed = senders.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let (tx, _rx) = mpsc::unbounded_channel::<String>();
            delayed.register("uid-1".to_string(), tx).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let sender = senders
            .wait_alive("uid-1", Duration::from_millis(250))
            .await;
        assert!(sender.is_some());
    }

    #[tokio::test]
    async fn live_sender_wait_alive_ignores_closed_sender_and_times_out() {
        let senders = LiveSendersState::default();
        let (tx, rx) = mpsc::unbounded_channel::<String>();
        senders.register("uid-1".to_string(), tx).await;
        drop(rx);

        let sender = senders.wait_alive("uid-1", Duration::from_millis(20)).await;
        assert!(sender.is_none());
        assert!(!senders.readiness("uid-1").await.has_sender);
    }

    #[test]
    fn runtime_health_reports_blocked_startup_dependencies() {
        let state = RuntimeState::for_bootstrap(true, "now".to_string(), 1, Vec::new());
        state.update_startup_dependencies(StartupDependencyStatus {
            startup_blocked: true,
            summary: "sign service unavailable".to_string(),
            sign_service: StartupDependencyProbe {
                name: "sign_service".to_string(),
                url: "https://sign.example.com/health".to_string(),
                ok: false,
                critical: true,
                status_code: Some(503),
                detail: "HTTP 503".to_string(),
                checked_at: "2026-05-23T10:00:00+08:00".to_string(),
            },
            license_service: StartupDependencyProbe {
                name: "license_service".to_string(),
                url: "https://license.example.com/health".to_string(),
                ok: true,
                critical: true,
                status_code: Some(200),
                detail: "ok".to_string(),
                checked_at: "2026-05-23T10:00:00+08:00".to_string(),
            },
            version_policy: StartupDependencyProbe {
                name: "version_policy".to_string(),
                url: "https://license.example.com/api/client/update-policy".to_string(),
                ok: true,
                critical: true,
                status_code: Some(200),
                detail: "ok".to_string(),
                checked_at: "2026-05-23T10:00:00+08:00".to_string(),
            },
        });

        let health = state.health_status("svc".to_string());
        assert_eq!(health.status, "blocked");
        assert!(!health.ready);
        assert!(health.startup_blocked);
        assert_eq!(health.dependencies.summary, "sign service unavailable");
        assert!(state.is_startup_blocked());
        assert_eq!(
            state.startup_block_summary().as_deref(),
            Some("sign service unavailable")
        );
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    config: AppConfig,
    runtime: RuntimeState,
    storage: StorageState,
    proxy: ProxyState,
    live_events: LiveEventState,
    live_senders: LiveSendersState,
    live_response_waiters: LiveResponseWaitersState,
    mtop_metrics: Arc<MtopMetrics>,
    wechat_workers: WechatWorkersState,
    feishu_native: FeishuNativeRuntime,
    feishu_qr_registrations: FeishuQrRegistrationState,
    incident_reporter: IncidentReporter,
}

#[derive(Debug, Clone, Default)]
pub struct FeishuQrRegistrationState {
    inner: Arc<AsyncRwLock<HashMap<String, FeishuQrRegistrationRecord>>>,
}

#[derive(Debug, Clone)]
pub struct FeishuQrRegistrationRecord {
    pub owner_username: String,
    pub domain: String,
}

impl FeishuQrRegistrationState {
    pub async fn remember(&self, device_code: &str, owner_username: &str, domain: &str) {
        let device_code = device_code.trim();
        if device_code.is_empty() {
            return;
        }
        self.inner.write().await.insert(
            device_code.to_string(),
            FeishuQrRegistrationRecord {
                owner_username: owner_username.trim().to_string(),
                domain: domain.trim().to_string(),
            },
        );
    }

    pub async fn get(&self, device_code: &str) -> Option<FeishuQrRegistrationRecord> {
        self.inner.read().await.get(device_code.trim()).cloned()
    }

    pub async fn forget(&self, device_code: &str) {
        self.inner.write().await.remove(device_code.trim());
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeState {
    inner: Arc<RwLock<RuntimeSnapshot>>,
}

#[derive(Debug, Clone)]
pub struct LiveEventState {
    sender: broadcast::Sender<Value>,
    capacity: usize,
}

#[derive(Debug, Clone)]
struct RuntimeSnapshot {
    state_backend: String,
    auth_configured: bool,
    ready: bool,
    version: String,
    started_at: String,
    startup_count: u64,
    databases: Vec<DatabaseStatus>,
    managers: Vec<RuntimeWorkerStatus>,
    cache_sync: CacheSyncRuntimeStatus,
    delete_runtime: DeleteRuntimeStatus,
    dependencies: StartupDependencyStatus,
    started_unix: u64,
}

fn feishu_native_runtime(state_cell: Arc<OnceLock<AppState>>) -> FeishuNativeRuntime {
    let event_handler: FeishuNativeEventHandler = Arc::new(move |event| {
        let state_cell = state_cell.clone();
        Box::pin(async move {
            let Some(state) = state_cell.get().cloned() else {
                eprintln!("[feishu.native] event dropped: AppState not initialized");
                return;
            };
            if let Err(error) = handle_feishu_im_event(&state, event).await {
                let safe_error = safe_worker_error_message(&error);
                eprintln!("[feishu.native] event handler failed: {safe_error}");
            }
        })
    });
    FeishuNativeRuntime::native(Some(event_handler))
}

impl AppState {
    pub async fn bootstrap(config: AppConfig) -> Result<Self, StorageError> {
        let auth_configured = false;
        let started_at = format!("{:?}", SystemTime::now());
        let storage = StorageState::bootstrap(&config, &started_at).await?;
        let proxy = ProxyState::bootstrap(&config.sqlite.data_dir);
        let state_cell = Arc::new(OnceLock::new());
        let feishu_native = feishu_native_runtime(state_cell.clone());

        let state = Self {
            config,
            runtime: RuntimeState::for_bootstrap(
                auth_configured,
                started_at,
                storage.startup_count(),
                storage.databases().to_vec(),
            ),
            storage,
            proxy,
            live_events: LiveEventState::new(2048),
            live_senders: LiveSendersState::default(),
            live_response_waiters: LiveResponseWaitersState::default(),
            mtop_metrics: Arc::new(MtopMetrics::new()),
            wechat_workers: WechatWorkersState::new(),
            feishu_native,
            feishu_qr_registrations: FeishuQrRegistrationState::default(),
            incident_reporter: IncidentReporter::new(),
        };
        let _ = state_cell.set(state.clone());
        Ok(state)
    }

    pub fn with_feishu_native(mut self, feishu_native: FeishuNativeRuntime) -> Self {
        self.feishu_native = feishu_native;
        self
    }

    pub fn wechat_workers(&self) -> &WechatWorkersState {
        &self.wechat_workers
    }

    pub fn feishu_native(&self) -> &FeishuNativeRuntime {
        &self.feishu_native
    }

    pub fn feishu_qr_registrations(&self) -> &FeishuQrRegistrationState {
        &self.feishu_qr_registrations
    }

    pub fn live_senders(&self) -> &LiveSendersState {
        &self.live_senders
    }

    pub fn live_response_waiters(&self) -> &LiveResponseWaitersState {
        &self.live_response_waiters
    }

    pub fn start_runtime_supervisor(&self) -> RuntimeSupervisor {
        RuntimeSupervisor::start(self.clone())
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn runtime(&self) -> &RuntimeState {
        &self.runtime
    }

    pub fn storage(&self) -> &StorageState {
        &self.storage
    }

    pub fn proxy(&self) -> &ProxyState {
        &self.proxy
    }

    pub fn live_events(&self) -> &LiveEventState {
        &self.live_events
    }

    pub fn mtop_metrics(&self) -> &Arc<MtopMetrics> {
        &self.mtop_metrics
    }

    pub fn incident_reporter(&self) -> &IncidentReporter {
        &self.incident_reporter
    }

    pub async fn collect_environment_snapshot(&self) -> EnvironmentSnapshot {
        let active_uids = crate::routes::live::list_active_account_uids(self.storage.accounts())
            .await
            .unwrap_or_default();
        let active_set = active_uids.into_iter().collect::<HashSet<_>>();
        let configured_account_count = query_scalar::<_, i64>("SELECT COUNT(*) FROM accounts")
            .fetch_one(self.storage.accounts())
            .await
            .unwrap_or(0);
        let active_account_count = active_set.len() as i64;

        let sender_snapshot = self.live_senders().snapshot().await;
        let scoped_sender_snapshot = sender_snapshot
            .iter()
            .filter(|entry| active_set.contains(entry.uid.as_str()))
            .collect::<Vec<_>>();
        let total_live_sender_count = scoped_sender_snapshot.len() as i64;
        let alive_live_sender_count = scoped_sender_snapshot
            .iter()
            .filter(|entry| !entry.is_closed)
            .count() as i64;
        let closed_live_sender_count = total_live_sender_count - alive_live_sender_count;
        let sender_set = sender_snapshot
            .iter()
            .filter(|entry| active_set.contains(entry.uid.as_str()))
            .map(|entry| entry.uid.clone())
            .collect::<HashSet<_>>();
        let db_states = self
            .storage
            .list_live_connection_states()
            .await
            .unwrap_or_default();
        let db_connected_uids = db_states
            .iter()
            .filter(|entry| entry.status == "connected")
            .filter(|entry| active_set.contains(entry.uid.as_str()))
            .map(|entry| entry.uid.clone())
            .collect::<Vec<_>>();
        let db_connected_set = db_connected_uids.iter().cloned().collect::<HashSet<_>>();
        let db_connected_missing_sender_count = db_connected_uids
            .iter()
            .filter(|uid| !sender_set.contains(*uid))
            .count() as i64;
        let sender_missing_db_connected_count = scoped_sender_snapshot
            .iter()
            .filter(|entry| !db_connected_set.contains(entry.uid.as_str()))
            .count() as i64;
        let online_live_account_count = db_connected_uids.len() as i64;

        let uptime_seconds = self.runtime.uptime_seconds();
        let proxy_status = self.proxy.status();

        EnvironmentSnapshot {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            build_channel: runtime_build_channel(),
            os: std::env::consts::OS.to_string(),
            os_version: String::new(),
            arch: std::env::consts::ARCH.to_string(),
            hostname_hash: String::new(),
            network_type: String::new(),
            proxy_enabled: proxy_status.enabled,
            proxy_type: proxy_status.strategy,
            public_ip_hash: String::new(),
            isp_region_hint: String::new(),
            control_plane_rtt_ms: 0,
            configured_account_count,
            active_account_count,
            online_live_account_count,
            alive_live_sender_count,
            total_live_sender_count,
            closed_live_sender_count,
            db_connected_missing_sender_count,
            sender_missing_db_connected_count,
            memory_usage_mb: 0,
            cpu_load: 0.0,
            disk_free_mb: 0,
            uptime_seconds,
        }
    }

    /// Classify an MTOP response, bump metrics, and emit a structured log for
    /// anomalies (signature/schema/unknown codes) so ops can spot upstream
    /// changes before they cascade.
    ///
    /// Returns the classification so callers can branch on it.
    pub fn record_mtop_response(
        &self,
        api: &str,
        payload: &Value,
    ) -> crate::mtop_errors::MtopErrorKind {
        let kind = crate::mtop_errors::classify_mtop_payload(payload);
        self.mtop_metrics.record(api, &kind);
        if kind.is_anomaly() {
            let raw = payload
                .get("ret")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
                .unwrap_or("");
            tracing::error!(
                target: "mtop_anomaly",
                api = %api,
                kind = %kind.tag(),
                ret = %raw,
                "MTOP anomaly — possible upstream breaking change"
            );
        }
        kind
    }
}

impl RuntimeState {
    fn for_bootstrap(
        auth_configured: bool,
        started_at: String,
        startup_count: u64,
        databases: Vec<DatabaseStatus>,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(RuntimeSnapshot {
                state_backend: "sqlite".to_string(),
                auth_configured,
                ready: true,
                version: env!("CARGO_PKG_VERSION").to_string(),
                started_at,
                startup_count,
                databases,
                managers: Vec::new(),
                cache_sync: CacheSyncRuntimeStatus::default(),
                delete_runtime: DeleteRuntimeStatus::default(),
                dependencies: StartupDependencyStatus {
                    startup_blocked: false,
                    summary: "startup dependency checks pending".to_string(),
                    sign_service: StartupDependencyProbe {
                        name: "sign_service".to_string(),
                        critical: true,
                        detail: "not checked yet".to_string(),
                        ..StartupDependencyProbe::default()
                    },
                    license_service: StartupDependencyProbe {
                        name: "license_service".to_string(),
                        critical: true,
                        detail: "not checked yet".to_string(),
                        ..StartupDependencyProbe::default()
                    },
                    version_policy: StartupDependencyProbe {
                        name: "version_policy".to_string(),
                        critical: true,
                        detail: "not checked yet".to_string(),
                        ..StartupDependencyProbe::default()
                    },
                },
                started_unix: unix_seconds_now(),
            })),
        }
    }

    pub fn uptime_seconds(&self) -> u64 {
        self.inner
            .read()
            .map(|snapshot| unix_seconds_now().saturating_sub(snapshot.started_unix))
            .unwrap_or(0)
    }

    pub fn register_workers(&self, worker_names: &[&str]) {
        let mut snapshot = self.inner.write().unwrap();

        for worker_name in worker_names {
            if snapshot
                .managers
                .iter()
                .any(|worker| worker.name == *worker_name)
            {
                continue;
            }
            snapshot.managers.push(RuntimeWorkerStatus {
                name: (*worker_name).to_string(),
                status: "starting".to_string(),
                tick_count: 0,
                last_tick_at: String::new(),
                last_error: String::new(),
            });
        }
    }

    pub fn record_worker_tick(
        &self,
        worker_name: &str,
        status: &str,
        tick_count: u64,
        last_error: Option<String>,
    ) {
        let mut snapshot = self.inner.write().unwrap();
        let worker = snapshot
            .managers
            .iter_mut()
            .find(|worker| worker.name == worker_name);

        if let Some(worker) = worker {
            worker.status = status.to_string();
            worker.tick_count = tick_count;
            worker.last_tick_at = format!("{:?}", SystemTime::now());
            worker.last_error = last_error.unwrap_or_default();
        }
    }

    pub fn mark_worker_stopped(&self, worker_name: &str) {
        let mut snapshot = self.inner.write().unwrap();
        let worker = snapshot
            .managers
            .iter_mut()
            .find(|worker| worker.name == worker_name);

        if let Some(worker) = worker {
            worker.status = "stopped".to_string();
        }
    }

    pub fn mark_all_workers_stopped(&self) {
        let mut snapshot = self.inner.write().unwrap();
        for worker in &mut snapshot.managers {
            worker.status = "stopped".to_string();
        }
    }

    pub fn update_cache_sync(&self, status: CacheSyncRuntimeStatus) {
        let mut snapshot = self.inner.write().unwrap();
        snapshot.cache_sync = status;
    }

    pub fn update_delete_runtime(&self, status: DeleteRuntimeStatus) {
        let mut snapshot = self.inner.write().unwrap();
        snapshot.delete_runtime = status;
    }

    pub fn update_startup_dependencies(&self, status: StartupDependencyStatus) {
        let mut snapshot = self.inner.write().unwrap();
        snapshot.dependencies = status;
    }

    pub fn is_startup_blocked(&self) -> bool {
        self.inner
            .read()
            .map(|snapshot| snapshot.dependencies.startup_blocked)
            .unwrap_or(false)
    }

    pub fn startup_block_summary(&self) -> Option<String> {
        self.inner.read().ok().and_then(|snapshot| {
            snapshot
                .dependencies
                .startup_blocked
                .then(|| snapshot.dependencies.summary.clone())
        })
    }

    pub fn inspection(&self, service: String) -> RuntimeInspection {
        let snapshot = self.inner.read().unwrap().clone();
        let ready = snapshot.ready && !snapshot.dependencies.startup_blocked;

        RuntimeInspection {
            service,
            version: snapshot.version,
            state_backend: snapshot.state_backend,
            auth_configured: snapshot.auth_configured,
            ready,
            started_at: snapshot.started_at,
            startup_count: snapshot.startup_count,
            databases: snapshot.databases,
            managers: snapshot.managers,
            cache_sync: snapshot.cache_sync,
            delete_runtime: snapshot.delete_runtime,
            dependencies: snapshot.dependencies,
        }
    }

    pub fn health_status(&self, service: String) -> HealthStatus {
        let snapshot = self.inner.read().unwrap().clone();
        let startup_blocked = snapshot.dependencies.startup_blocked;
        let ready = snapshot.ready && !startup_blocked;

        HealthStatus {
            status: if startup_blocked {
                "blocked"
            } else if ready {
                "ok"
            } else {
                "degraded"
            }
            .to_string(),
            ready,
            service,
            state_backend: snapshot.state_backend,
            started_at: snapshot.started_at,
            startup_blocked,
            dependencies: snapshot.dependencies,
        }
    }
}

impl LiveEventState {
    fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender, capacity }
    }

    pub fn publish(&self, event: Value) {
        let len = self.sender.len();
        if len * 5 >= self.capacity * 4 {
            tracing::warn!(
                buffered = len,
                capacity = self.capacity,
                subscribers = self.sender.receiver_count(),
                "live_events broadcast approaching capacity — slow subscriber may drop events"
            );
        }
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.sender.subscribe()
    }

    pub fn pressure(&self) -> (usize, usize, usize) {
        (
            self.sender.len(),
            self.capacity,
            self.sender.receiver_count(),
        )
    }
}

fn runtime_build_channel() -> String {
    std::env::var("APP_BUILD_CHANNEL").unwrap_or_else(|_| {
        if cfg!(debug_assertions) {
            "debug".to_string()
        } else {
            "release".to_string()
        }
    })
}

fn unix_seconds_now() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
