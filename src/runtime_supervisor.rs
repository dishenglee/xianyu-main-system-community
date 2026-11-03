use std::{collections::HashSet, env, fs::OpenOptions, path::PathBuf, time::Duration};

use serde_json::json;
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tracing::{info, warn};

use crate::{
    automation_scheduler, business_auto,
    models::runtime::DeleteRuntimeStatus,
    operations_runtime,
    routes::{accounts, live, peer_monitor},
    state::AppState,
    storage::StorageState,
};

const HEARTBEAT_WORKER: &str = "service_heartbeat";
const SQLITE_MONITOR_WORKER: &str = "sqlite_monitor";
const CACHE_SYNC_WORKER: &str = "cache_sync_runtime";
const DELETE_RUNTIME_WORKER: &str = "comment_delete_runtime";
const HISTORY_PREFETCH_WORKER: &str = "live_history_prefetch";
const COOKIE_REFRESH_WORKER: &str = "cookie_refresh";
const OPERATION_JOBS_WORKER: &str = "operation_jobs";
const AUTOMATION_SCHEDULES_WORKER: &str = "automation_schedules";
const PEER_MONITOR_WORKER: &str = "peer_monitor";
const COOKIE_REFRESH_TICK: Duration = Duration::from_secs(300);
const COOKIE_REFRESH_THRESHOLD_SECS: f64 = 6000.0;
const PEER_MONITOR_TICK: Duration = Duration::from_secs(60);

pub struct RuntimeSupervisor {
    runtime: crate::state::RuntimeState,
    _live_runtime_guard: Option<LiveRuntimeGuard>,
    shutdown_tx: watch::Sender<bool>,
    handles: Vec<JoinHandle<()>>,
}

struct LiveRuntimeGuard {
    path: PathBuf,
    _file: std::fs::File,
}

impl Drop for LiveRuntimeGuard {
    fn drop(&mut self) {
        let _ = self._file.unlock();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl RuntimeSupervisor {
    pub fn start(state: AppState) -> Self {
        state.runtime().register_workers(&[
            HEARTBEAT_WORKER,
            SQLITE_MONITOR_WORKER,
            CACHE_SYNC_WORKER,
            DELETE_RUNTIME_WORKER,
            live::worker_name(),
            HISTORY_PREFETCH_WORKER,
            COOKIE_REFRESH_WORKER,
            OPERATION_JOBS_WORKER,
            AUTOMATION_SCHEDULES_WORKER,
            business_auto::delivery::worker_name(),
            PEER_MONITOR_WORKER,
        ]);

        let live_runtime_guard = acquire_live_runtime_guard();
        let live_runtime_enabled = live_runtime_guard.is_some();
        let live_runtime_block_detail = if live_runtime_enabled {
            None
        } else {
            Some(live_runtime_disabled_detail(&state))
        };

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let heartbeat_handle = tokio::spawn(run_heartbeat_worker(
            state.storage().clone(),
            state.runtime().clone(),
            shutdown_rx.clone(),
        ));
        let monitor_handle = tokio::spawn(run_sqlite_monitor_worker(
            state.storage().clone(),
            state.runtime().clone(),
            shutdown_rx,
        ));
        let cache_sync_handle = tokio::spawn(run_cache_sync_worker(
            state.storage().clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
        ));
        let delete_runtime_handle = tokio::spawn(run_delete_runtime_worker(
            state.storage().clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
        ));
        let mut handles = vec![
            heartbeat_handle,
            monitor_handle,
            cache_sync_handle,
            delete_runtime_handle,
        ];
        if live_runtime_enabled {
            handles.push(tokio::spawn(live::run_live_message_runtime(
                state.clone(),
                state.runtime().clone(),
                shutdown_tx.subscribe(),
            )));
            if cfg!(target_os = "android") {
                mark_worker_blocked(
                    &state,
                    HISTORY_PREFETCH_WORKER,
                    "disabled_on_android_startup",
                );
            } else {
                handles.push(tokio::spawn(run_history_prefetch_worker(
                    state.clone(),
                    state.runtime().clone(),
                    shutdown_tx.subscribe(),
                )));
            }
        } else if let Some(detail) = live_runtime_block_detail {
            mark_worker_blocked(&state, live::worker_name(), &detail);
            mark_worker_blocked(&state, HISTORY_PREFETCH_WORKER, &detail);
        }
        let cookie_refresh_handle = tokio::spawn(run_cookie_refresh_worker(
            state.clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
        ));
        let operation_jobs_handle = tokio::spawn(run_operation_jobs_worker(
            state.clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
        ));
        let automation_schedules_handle = tokio::spawn(automation_scheduler::run_scheduler_worker(
            state.clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
            AUTOMATION_SCHEDULES_WORKER,
        ));
        let peer_monitor_handle = tokio::spawn(run_peer_monitor_worker(
            state.clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
        ));
        let auto_delivery_handle = business_auto::delivery::start_runtime(
            state.clone(),
            state.runtime().clone(),
            shutdown_tx.subscribe(),
        );
        handles.push(cookie_refresh_handle);
        handles.push(operation_jobs_handle);
        handles.push(automation_schedules_handle);
        handles.push(peer_monitor_handle);
        handles.push(auto_delivery_handle);

        Self {
            runtime: state.runtime().clone(),
            _live_runtime_guard: live_runtime_guard,
            shutdown_tx,
            handles,
        }
    }

    pub async fn shutdown_and_wait(self) {
        let _ = self.shutdown_tx.send(true);
        for handle in self.handles {
            let _ = handle.await;
        }
        self.runtime.mark_all_workers_stopped();
    }
}

fn env_flag_enabled(key: &str) -> bool {
    env::var(key)
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

fn live_runtime_disabled_detail(state: &AppState) -> String {
    if env_flag_enabled("XIANYU_DISABLE_LIVE_RUNTIME") {
        return "live runtime disabled by environment".to_string();
    }
    format!(
        "live runtime lock is held by another backend process; data_dir={}",
        state.config().sqlite.data_dir.display()
    )
}

fn mark_worker_blocked(state: &AppState, worker_name: &str, detail: &str) {
    state
        .runtime()
        .record_worker_tick(worker_name, "blocked", 0, Some(detail.to_string()));
    let storage = state.storage().clone();
    let worker_name = worker_name.to_string();
    let detail = detail.to_string();
    tokio::spawn(async move {
        let _ = storage
            .upsert_worker_state(&worker_name, "blocked", 0, Some(&detail))
            .await;
    });
}

fn acquire_live_runtime_guard() -> Option<LiveRuntimeGuard> {
    if env_flag_enabled("XIANYU_DISABLE_LIVE_RUNTIME") {
        warn!("live runtime disabled by environment");
        return None;
    }

    let lock_dir = env::var("XIANYU_LIVE_RUNTIME_LOCK_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("xianyu-rust-backend-full"));
    if let Err(error) = std::fs::create_dir_all(&lock_dir) {
        warn!(
            lock_dir = %lock_dir.display(),
            error = %error,
            "failed to create live runtime lock dir; live runtime disabled"
        );
        return None;
    }
    let path = lock_dir.join("live-runtime.lock");
    let file = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(file) => file,
        Err(error) => {
            warn!(
                path = %path.display(),
                error = %error,
                "failed to open live runtime lock; live runtime disabled"
            );
            return None;
        }
    };

    match file.try_lock() {
        Ok(()) => {
            let _ = file.set_len(0);
            let _ = std::io::Write::write_all(
                &mut &file,
                format!("pid={}\n", std::process::id()).as_bytes(),
            );
            Some(LiveRuntimeGuard { path, _file: file })
        }
        Err(error) => {
            if matches!(error, std::fs::TryLockError::WouldBlock) {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "another backend already owns the live runtime; IM workers disabled in this process"
                );
            } else if cfg!(target_os = "android") {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "live runtime lock failed on Android; continuing because the mobile app owns a single backend process"
                );
                let _ = file.set_len(0);
                let _ = std::io::Write::write_all(
                    &mut &file,
                    format!("pid={}\nlock_fallback=android\n", std::process::id()).as_bytes(),
                );
                return Some(LiveRuntimeGuard { path, _file: file });
            } else {
                warn!(
                    path = %path.display(),
                    error = %error,
                    "failed to acquire live runtime lock; IM workers disabled in this process"
                );
            }
            None
        }
    }
}

async fn run_heartbeat_worker(
    storage: StorageState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                match storage
                    .upsert_worker_state(HEARTBEAT_WORKER, "running", tick_count, None)
                    .await
                {
                    Ok(()) => runtime.record_worker_tick(HEARTBEAT_WORKER, "running", tick_count, None),
                    Err(error) => runtime.record_worker_tick(
                        HEARTBEAT_WORKER,
                        "degraded",
                        tick_count,
                        Some(error.to_string()),
                    ),
                }
            }
        }
    }

    let _ = storage
        .upsert_worker_state(HEARTBEAT_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(HEARTBEAT_WORKER);
}

async fn run_sqlite_monitor_worker(
    storage: StorageState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                match storage.probe_databases().await {
                    Ok(()) => {
                        let _ = storage
                            .upsert_worker_state(SQLITE_MONITOR_WORKER, "running", tick_count, None)
                            .await;
                        runtime.record_worker_tick(SQLITE_MONITOR_WORKER, "running", tick_count, None);
                    }
                    Err(error) => {
                        let error_text = error.to_string();
                        let _ = storage
                            .upsert_worker_state(
                                SQLITE_MONITOR_WORKER,
                                "degraded",
                                tick_count,
                                Some(&error_text),
                            )
                            .await;
                        runtime.record_worker_tick(
                            SQLITE_MONITOR_WORKER,
                            "degraded",
                            tick_count,
                            Some(error_text),
                        );
                    }
                }
            }
        }
    }

    let _ = storage
        .upsert_worker_state(SQLITE_MONITOR_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(SQLITE_MONITOR_WORKER);
}

async fn run_cache_sync_worker(
    storage: StorageState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(Duration::from_secs(3));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                let snapshot = storage
                    .upsert_cache_sync_runtime_state("idle", &[], 0, 0.0)
                    .await;

                match snapshot {
                    Ok(snapshot) => {
                        runtime.update_cache_sync(snapshot);
                        let _ = storage
                            .upsert_worker_state(CACHE_SYNC_WORKER, "running", tick_count, None)
                            .await;
                        runtime.record_worker_tick(CACHE_SYNC_WORKER, "running", tick_count, None);
                    }
                    Err(error) => {
                        let error_text = error.to_string();
                        let _ = storage
                            .upsert_worker_state(
                                CACHE_SYNC_WORKER,
                                "degraded",
                                tick_count,
                                Some(&error_text),
                            )
                            .await;
                        runtime.record_worker_tick(
                            CACHE_SYNC_WORKER,
                            "degraded",
                            tick_count,
                            Some(error_text),
                        );
                    }
                }
            }
        }
    }

    let _ = storage
        .upsert_worker_state(CACHE_SYNC_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(CACHE_SYNC_WORKER);
}

async fn run_delete_runtime_worker(
    storage: StorageState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                let snapshot = storage
                    .upsert_delete_runtime_state(&DeleteRuntimeStatus {
                        running_job_ids: Vec::new(),
                        running_count: 0,
                        pending_count: 0,
                        retryable_count: 0,
                        persisted_running_count: 0,
                        runtime_ready: true,
                        last_poll_at: unix_timestamp_seconds(),
                        last_runtime_error: String::new(),
                        last_recovered_count: 0,
                        claim_batch_size: 10,
                        action_capacity: 4,
                    })
                    .await;

                match snapshot {
                    Ok(snapshot) => {
                        runtime.update_delete_runtime(snapshot);
                        let _ = storage
                            .upsert_worker_state(DELETE_RUNTIME_WORKER, "running", tick_count, None)
                            .await;
                        runtime.record_worker_tick(DELETE_RUNTIME_WORKER, "running", tick_count, None);
                    }
                    Err(error) => {
                        let error_text = error.to_string();
                        let _ = storage
                            .upsert_worker_state(
                                DELETE_RUNTIME_WORKER,
                                "degraded",
                                tick_count,
                                Some(&error_text),
                            )
                            .await;
                        runtime.record_worker_tick(
                            DELETE_RUNTIME_WORKER,
                            "degraded",
                            tick_count,
                            Some(error_text),
                        );
                    }
                }
            }
        }
    }

    let _ = storage
        .upsert_delete_runtime_state(&DeleteRuntimeStatus {
            runtime_ready: false,
            claim_batch_size: 10,
            action_capacity: 4,
            ..DeleteRuntimeStatus::default()
        })
        .await;
    let _ = storage
        .upsert_worker_state(DELETE_RUNTIME_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(DELETE_RUNTIME_WORKER);
}

async fn run_operation_jobs_worker(
    state: AppState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(Duration::from_secs(2));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                let mut last_error = String::new();
                match state.storage().claim_next_operation_job().await {
                    Ok(Some(job)) => {
                        if let Err(error) = operations_runtime::process_one_job(&state, &job.id).await {
                            warn!(job_id = %job.id, error = %error, "operation job processing failed");
                            last_error = error.clone();
                            let _ = state.storage().finalize_operation_job(
                                &job.id,
                                "failed",
                                0,
                                job.total_count,
                                0,
                                json!({
                                    "message": "后台任务执行失败",
                                }),
                                &error,
                            ).await;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        last_error = error.to_string();
                    }
                }

                let status = if last_error.is_empty() { "running" } else { "degraded" };
                let detail = if last_error.is_empty() { None } else { Some(last_error.as_str()) };
                let _ = state
                    .storage()
                    .upsert_worker_state(OPERATION_JOBS_WORKER, status, tick_count, detail)
                    .await;
                runtime.record_worker_tick(
                    OPERATION_JOBS_WORKER,
                    status,
                    tick_count,
                    if last_error.is_empty() { None } else { Some(last_error) },
                );
            }
        }
    }

    let _ = state
        .storage()
        .upsert_worker_state(OPERATION_JOBS_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(OPERATION_JOBS_WORKER);
}

async fn run_peer_monitor_worker(
    state: AppState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(PEER_MONITOR_TICK);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                match peer_monitor::refresh_due_peer_monitor_targets(&state).await {
                    Ok(batch) => {
                        let status = if batch.failed > 0 { "degraded" } else { "running" };
                        let detail = format!(
                            "采集={} 成功={} 部分成功={} 失败={}",
                            batch.attempted,
                            batch.succeeded,
                            batch.partial,
                            batch.failed,
                        );
                        let _ = state.storage()
                            .upsert_worker_state(PEER_MONITOR_WORKER, status, tick_count, Some(&detail))
                            .await;
                        runtime.record_worker_tick(
                            PEER_MONITOR_WORKER,
                            status,
                            tick_count,
                            (batch.failed > 0).then_some(detail),
                        );
                    }
                    Err(error) => {
                        let detail = "同行监控调度暂时不可用".to_string();
                        warn!(error = %error, "peer monitor worker tick failed");
                        let _ = state.storage()
                            .upsert_worker_state(PEER_MONITOR_WORKER, "degraded", tick_count, Some(&detail))
                            .await;
                        runtime.record_worker_tick(
                            PEER_MONITOR_WORKER,
                            "degraded",
                            tick_count,
                            Some(detail),
                        );
                    }
                }
            }
        }
    }

    let _ = state
        .storage()
        .upsert_worker_state(PEER_MONITOR_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(PEER_MONITOR_WORKER);
}

fn unix_timestamp_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs_f64()
}

async fn run_history_prefetch_worker(
    state: AppState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    const STARTUP_DELAY: Duration = Duration::from_secs(5);
    const TICK_INTERVAL: Duration = Duration::from_secs(30);
    const INTER_ACCOUNT_SLEEP: Duration = Duration::from_millis(200);

    tokio::select! {
        _ = tokio::time::sleep(STARTUP_DELAY) => {}
        changed = shutdown_rx.changed() => {
            if changed.is_err() || *shutdown_rx.borrow() {
                runtime.mark_worker_stopped(HISTORY_PREFETCH_WORKER);
                return;
            }
        }
    }

    let mut interval = time::interval(TICK_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;
    let mut prefetched: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                let active_uids = match live::list_active_account_uids(state.storage().accounts()).await {
                    Ok(uids) => uids,
                    Err(error) => {
                        let detail = error.to_string();
                        let _ = state.storage()
                            .upsert_worker_state(HISTORY_PREFETCH_WORKER, "degraded", tick_count, Some(&detail))
                            .await;
                        runtime.record_worker_tick(
                            HISTORY_PREFETCH_WORKER,
                            "degraded",
                            tick_count,
                            Some(detail),
                        );
                        continue;
                    }
                };
                let active_set: HashSet<String> = active_uids.iter().cloned().collect();
                prefetched.retain(|uid| active_set.contains(uid));

                let initial_detail = format!(
                    "active={} warmed={} target={}",
                    active_uids.len(),
                    prefetched.len(),
                    active_uids.iter().filter(|u| !prefetched.contains(*u)).count()
                );
                let _ = state.storage()
                    .upsert_worker_state(HISTORY_PREFETCH_WORKER, "running", tick_count, Some(&initial_detail))
                    .await;
                runtime.record_worker_tick(HISTORY_PREFETCH_WORKER, "running", tick_count, None);

                let mut new_count = 0usize;
                for uid in &active_uids {
                    if prefetched.contains(uid) {
                        continue;
                    }
                    match accounts::prefetch_account_message_history(&state, uid).await {
                        Ok(count) => {
                            info!(uid = %uid, sessions = count, "live history prefetched");
                            prefetched.insert(uid.clone());
                            new_count += 1;
                            let progress = format!(
                                "active={} warmed={} pass_new={}",
                                active_uids.len(),
                                prefetched.len(),
                                new_count
                            );
                            let _ = state.storage()
                                .upsert_worker_state(HISTORY_PREFETCH_WORKER, "running", tick_count, Some(&progress))
                                .await;
                        }
                        Err(detail) => {
                            warn!(uid = %uid, detail, "live history prefetch failed");
                        }
                    }
                    tokio::time::sleep(INTER_ACCOUNT_SLEEP).await;
                }

                let detail = format!(
                    "active={} warmed={} new={}",
                    active_uids.len(),
                    prefetched.len(),
                    new_count
                );
                let _ = state.storage()
                    .upsert_worker_state(HISTORY_PREFETCH_WORKER, "running", tick_count, Some(&detail))
                    .await;
                runtime.record_worker_tick(HISTORY_PREFETCH_WORKER, "running", tick_count, None);
            }
        }
    }

    let _ = state
        .storage()
        .upsert_worker_state(HISTORY_PREFETCH_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(HISTORY_PREFETCH_WORKER);
}

async fn run_cookie_refresh_worker(
    state: AppState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = time::interval(COOKIE_REFRESH_TICK);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                match cookie_refresh_tick_once(&state).await {
                    Ok(()) => {
                        let _ = state.storage()
                            .upsert_worker_state(COOKIE_REFRESH_WORKER, "running", tick_count, None)
                            .await;
                        runtime.record_worker_tick(COOKIE_REFRESH_WORKER, "running", tick_count, None);
                    }
                    Err(detail) => {
                        let _ = state.storage()
                            .upsert_worker_state(COOKIE_REFRESH_WORKER, "degraded", tick_count, Some(&detail))
                            .await;
                        runtime.record_worker_tick(
                            COOKIE_REFRESH_WORKER,
                            "degraded",
                            tick_count,
                            Some(detail),
                        );
                    }
                }
            }
        }
    }

    let _ = state
        .storage()
        .upsert_worker_state(COOKIE_REFRESH_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(COOKIE_REFRESH_WORKER);
}

async fn cookie_refresh_tick_once(state: &AppState) -> Result<(), String> {
    crate::protocol_runtime::resolve_protocol(
        state,
        crate::protocol_catalog::opcodes::ACCOUNT_USER_SIMPLE,
    )
    .await?;
    let pool = state.storage().accounts();
    let uids = live::list_active_account_uids(pool)
        .await
        .map_err(|error| error.to_string())?;
    let now_secs = unix_timestamp_seconds();

    for uid in uids {
        let last = sqlx::query_scalar::<_, Option<f64>>(
            "SELECT cookie_refreshed_at FROM accounts WHERE uid = ? LIMIT 1",
        )
        .bind(&uid)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?
        .flatten()
        .unwrap_or(0.0);

        let age = now_secs - last;
        if age < COOKIE_REFRESH_THRESHOLD_SECS {
            continue;
        }

        match crate::cookie_refresh::refresh_account_cookies(pool, &uid).await {
            Ok(outcome) if outcome.ok => {
                info!(uid = %uid, age_min = age / 60.0, "auto bootstrap ok");
                match crate::routes::accounts::summary::load_account_summary(
                    state.clone(),
                    &uid,
                    true,
                )
                .await
                {
                    Ok(_) => {
                        info!(uid = %uid, "auto summary refresh ok");
                    }
                    Err(err) => {
                        warn!(uid = %uid, error = ?err, "auto summary refresh failed");
                    }
                }
            }
            Ok(outcome) => {
                warn!(
                    uid = %uid,
                    age_min = age / 60.0,
                    reason = %outcome.reason,
                    "auto bootstrap soft-failed"
                );
            }
            Err(error) => {
                warn!(
                    uid = %uid,
                    age_min = age / 60.0,
                    error = %error,
                    "auto bootstrap errored"
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, sync::Mutex};

    use super::*;

    static LIVE_LOCK_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct LiveLockEnvGuard {
        previous: Option<OsString>,
    }

    impl LiveLockEnvGuard {
        fn set(path: PathBuf) -> Self {
            let previous = env::var_os("XIANYU_LIVE_RUNTIME_LOCK_DIR");
            unsafe {
                env::set_var("XIANYU_LIVE_RUNTIME_LOCK_DIR", path);
            }
            Self { previous }
        }
    }

    impl Drop for LiveLockEnvGuard {
        fn drop(&mut self) {
            unsafe {
                match self.previous.take() {
                    Some(value) => env::set_var("XIANYU_LIVE_RUNTIME_LOCK_DIR", value),
                    None => env::remove_var("XIANYU_LIVE_RUNTIME_LOCK_DIR"),
                }
            }
        }
    }

    #[tokio::test]
    async fn default_start_runs_full_worker_set_without_offline_blocks() {
        let temp = tempfile::tempdir().expect("temp dir");
        let _env_lock = LIVE_LOCK_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let _live_lock_env = LiveLockEnvGuard::set(temp.path().join("runtime-locks"));
        let mut config = crate::config::AppConfig::default();
        config.sqlite.data_dir = temp.path().to_path_buf();
        config.sqlite.accounts_db_path = temp.path().join("accounts.db");
        config.sqlite.live_state_db_path = temp.path().join("live_state.db");
        config.sqlite.notifications_db_path = temp.path().join("notifications.db");
        config.sqlite.xianyu_db_path = temp.path().join("xianyu.db");
        let state = AppState::bootstrap(config).await.expect("app state");

        let supervisor = RuntimeSupervisor::start(state.clone());
        let handle_count = supervisor.handles.len();
        let has_live_guard = supervisor._live_runtime_guard.is_some();
        let inspection = state.runtime().inspection("test-service".to_string());
        supervisor.shutdown_and_wait().await;

        let expected_worker_names = HashSet::from([
            HEARTBEAT_WORKER,
            SQLITE_MONITOR_WORKER,
            CACHE_SYNC_WORKER,
            DELETE_RUNTIME_WORKER,
            live::worker_name(),
            HISTORY_PREFETCH_WORKER,
            COOKIE_REFRESH_WORKER,
            OPERATION_JOBS_WORKER,
            AUTOMATION_SCHEDULES_WORKER,
            business_auto::delivery::worker_name(),
            PEER_MONITOR_WORKER,
        ]);
        let actual_worker_names = inspection
            .managers
            .iter()
            .map(|worker| worker.name.as_str())
            .collect::<HashSet<_>>();

        assert_eq!(actual_worker_names, expected_worker_names);
        assert_eq!(
            handle_count,
            if cfg!(target_os = "android") { 10 } else { 11 }
        );
        assert!(has_live_guard);
        assert!(inspection
            .managers
            .iter()
            .all(|worker| worker.last_error != "offline_snapshot"));
    }
}
