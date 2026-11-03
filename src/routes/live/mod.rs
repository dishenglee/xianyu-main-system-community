use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use axum::{
    extract::Extension,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::ResponseError,
    routes::tenant::ensure_admin_or_legacy,
    state::{AppState, RuntimeState},
    telemetry::IncidentInput,
};

mod messaging;
mod protocol;
mod session;
pub(super) mod worker;
mod ws_handler;

use session::{reconcile_live_account_tasks, shutdown_live_account_tasks, LiveAccountTask};
use ws_handler::live_ws;

// Re-export pub(crate) items consumed by runtime_supervisor and other modules
pub use session::list_active_account_uids;

const LIVE_MESSAGE_RUNTIME_WORKER: &str = "live_message_runtime";
const LIVE_RUNTIME_TICK: Duration = Duration::from_secs(2);
pub(super) const LIVE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
// Python parity: `xianyu_wx_live_client.py:267-268` uses
// `min(300.0, 2 ** min(attempts, 8))` — i.e. exponential backoff capped at 5
// minutes. Earlier 15s cap caused 40 hits / 10 min when the upstream RST'd,
// triggering IP-level rate limits. 300s lets us sit out longer when goofish
// is genuinely refusing.
pub(super) const LIVE_RECONNECT_MAX: Duration = Duration::from_secs(300);
pub(super) const AUTOSYNC_ORDERS_DEBOUNCE: Duration = Duration::from_secs(60);
/// Keep the autosync debounce map from growing unbounded — entries older than
/// this (10x debounce) are stale and safe to drop on next insert.
pub(super) const AUTOSYNC_MAP_TTL: Duration = Duration::from_secs(600);
/// Minimum interval between cookie-refresh attempts triggered by ACCS handshake
/// failures for the same account. Prevents hammering the passport endpoint
/// when the upstream is genuinely down.
pub(super) const COOKIE_REFRESH_COOLDOWN: Duration = Duration::from_secs(300);
const LIVE_SENDER_UNAVAILABLE_INCIDENT_THROTTLE: Duration = Duration::from_secs(60);
const LIVE_SENDER_READY_WAIT: Duration = Duration::from_secs(5);

fn live_sender_unavailable_throttle() -> &'static Mutex<HashMap<String, Instant>> {
    static THROTTLE: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    THROTTLE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn should_report_live_sender_unavailable(uid: &str) -> bool {
    let Ok(mut throttle) = live_sender_unavailable_throttle().lock() else {
        return true;
    };
    let now = Instant::now();
    throttle
        .retain(|_, last| now.duration_since(*last) < LIVE_SENDER_UNAVAILABLE_INCIDENT_THROTTLE);
    if let Some(last) = throttle.get(uid) {
        if now.duration_since(*last) < LIVE_SENDER_UNAVAILABLE_INCIDENT_THROTTLE {
            return false;
        }
    }
    throttle.insert(uid.to_string(), now);
    true
}

fn live_sender_unavailable_user_message(active: bool, db_status: Option<&str>) -> &'static str {
    match db_status.unwrap_or_default() {
        "reconnecting" | "connecting" => "消息连接正在重连，请稍后再试",
        "connected" if active => "消息连接暂未就绪，请稍后再试",
        _ if active => "消息连接暂未就绪，请稍后再试",
        _ => "账号未在线，请先重新连接消息服务",
    }
}

pub(crate) async fn live_sender_unavailable_detail(state: &AppState, uid: &str) -> String {
    let readiness = state.live_senders().readiness(uid).await;
    let db_state = state
        .storage()
        .get_live_connection_state(uid)
        .await
        .ok()
        .flatten();
    let active = list_active_account_uids(state.storage().accounts())
        .await
        .map(|uids| uids.iter().any(|active_uid| active_uid == uid))
        .unwrap_or(false);
    match db_state {
        Some(db_state) => format!(
            "live socket for uid {uid} is not connected; hasSender={}, senderClosed={}, activeAccount={}, dbStatus={}, reconnectAttempts={}, lastError={}, aliveSenders={}/{}",
            readiness.has_sender,
            readiness.sender_closed,
            active,
            db_state.status,
            db_state.reconnect_attempts,
            db_state.last_error,
            readiness.alive_sender_count,
            readiness.total_sender_count,
        ),
        None => format!(
            "live socket for uid {uid} is not connected; hasSender={}, senderClosed={}, activeAccount={}, dbStatus=missing, aliveSenders={}/{}",
            readiness.has_sender,
            readiness.sender_closed,
            active,
            readiness.alive_sender_count,
            readiness.total_sender_count,
        ),
    }
}

pub(crate) async fn live_sender_unavailable_error(state: &AppState, uid: &str) -> ResponseError {
    let readiness = state.live_senders().readiness(uid).await;
    let db_state = state
        .storage()
        .get_live_connection_state(uid)
        .await
        .ok()
        .flatten();
    let active = list_active_account_uids(state.storage().accounts())
        .await
        .map(|uids| uids.iter().any(|active_uid| active_uid == uid))
        .unwrap_or(false);
    let detail = match db_state.as_ref() {
        Some(db_state) => format!(
            "live socket for uid {uid} is not connected; hasSender={}, senderClosed={}, activeAccount={}, dbStatus={}, reconnectAttempts={}, lastError={}, aliveSenders={}/{}",
            readiness.has_sender,
            readiness.sender_closed,
            active,
            db_state.status,
            db_state.reconnect_attempts,
            db_state.last_error,
            readiness.alive_sender_count,
            readiness.total_sender_count,
        ),
        None => format!(
            "live socket for uid {uid} is not connected; hasSender={}, senderClosed={}, activeAccount={}, dbStatus=missing, aliveSenders={}/{}",
            readiness.has_sender,
            readiness.sender_closed,
            active,
            readiness.alive_sender_count,
            readiness.total_sender_count,
        ),
    };
    if should_report_live_sender_unavailable(uid) {
        let _ = state
            .incident_reporter()
            .report_from_state(
                state,
                IncidentInput {
                    error_level: "warn".to_string(),
                    module: "live".to_string(),
                    action: "sender_lookup".to_string(),
                    route: crate::sealed_protocol::sealed_str!("/api/live").to_string(),
                    error_code: "live_sender_unavailable".to_string(),
                    error_message: detail,
                    http_status: Some(503),
                    upstream_ret: String::new(),
                    retryable: true,
                    context_json: json!({
                        "uid": uid,
                        "activeAccount": active,
                        "readiness": {
                            "hasSender": readiness.has_sender,
                            "senderClosed": readiness.sender_closed,
                            "aliveSenderCount": readiness.alive_sender_count,
                            "totalSenderCount": readiness.total_sender_count,
                        },
                        "dbState": db_state.as_ref().map(|entry| json!({
                            "status": entry.status,
                            "reconnectAttempts": entry.reconnect_attempts,
                            "lastError": entry.last_error,
                        })).unwrap_or_else(|| json!(null)),
                    }),
                    environment: None,
                },
            )
            .await;
    }
    ResponseError::service_unavailable(live_sender_unavailable_user_message(
        active,
        db_state.as_ref().map(|entry| entry.status.as_str()),
    ))
}

pub(crate) async fn wait_for_live_sender(
    state: &AppState,
    uid: &str,
) -> Result<mpsc::UnboundedSender<String>, ResponseError> {
    match state
        .live_senders()
        .wait_alive(uid, LIVE_SENDER_READY_WAIT)
        .await
    {
        Some(sender) => Ok(sender),
        None => Err(live_sender_unavailable_error(state, uid).await),
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(crate::sealed_protocol::sealed_str!("/ws"), get(live_ws))
        .route(
            crate::sealed_protocol::sealed_str!("/debug/senders"),
            get(debug_live_senders),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/debug/push"),
            post(debug_push_live_event),
        )
}

async fn debug_live_senders(
    axum::extract::State(state): axum::extract::State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<Value>, ResponseError> {
    ensure_admin_or_legacy(&state, &identity)?;
    let sender_snapshot = state.live_senders().snapshot().await;
    let sender_uids = sender_snapshot
        .iter()
        .map(|entry| entry.uid.clone())
        .collect::<Vec<_>>();
    let alive_uids = sender_snapshot
        .iter()
        .filter(|entry| !entry.is_closed)
        .map(|entry| entry.uid.clone())
        .collect::<Vec<_>>();
    let closed_uids = sender_snapshot
        .iter()
        .filter(|entry| entry.is_closed)
        .map(|entry| entry.uid.clone())
        .collect::<Vec<_>>();

    let active_uids = list_active_account_uids(state.storage().accounts())
        .await
        .unwrap_or_default();
    let active_set = active_uids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let db_states = state
        .storage()
        .list_live_connection_states()
        .await
        .unwrap_or_default();
    let mut db_connected_uids = db_states
        .iter()
        .into_iter()
        .filter(|entry| entry.status == "connected")
        .filter(|entry| active_set.contains(entry.uid.as_str()))
        .map(|entry| entry.uid.clone())
        .collect::<Vec<_>>();
    db_connected_uids.sort();

    let sender_set = sender_uids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let db_connected_set = db_connected_uids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
    let mut db_connected_missing_sender = db_connected_uids
        .iter()
        .filter(|uid| !sender_set.contains(*uid))
        .cloned()
        .collect::<Vec<_>>();
    let mut sender_missing_db_connected = sender_uids
        .iter()
        .filter(|uid| !db_connected_set.contains(*uid))
        .cloned()
        .collect::<Vec<_>>();
    db_connected_missing_sender.sort();
    sender_missing_db_connected.sort();

    Ok(Json(json!({
        "memory": {
            "total": sender_snapshot.len(),
            "alive": alive_uids.len(),
            "closed": closed_uids.len(),
            "uids": sender_snapshot,
            "aliveUids": alive_uids,
            "closedUids": closed_uids,
        },
        "db": {
            "connected": db_connected_uids.len(),
            "connectedUids": db_connected_uids,
        },
        "diff": {
            "dbConnectedMissingSender": db_connected_missing_sender,
            "senderMissingDbConnected": sender_missing_db_connected,
        },
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DebugPushLiveEventPayload {
    #[serde(default)]
    uid: String,
    #[serde(default)]
    session_id: String,
    #[serde(default = "default_debug_session_type")]
    session_type: i64,
    #[serde(default)]
    peer_nick: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    raw_preview: String,
    #[serde(default)]
    ts: i64,
}

fn default_debug_session_type() -> i64 {
    1
}

async fn debug_push_live_event(
    axum::extract::State(state): axum::extract::State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<DebugPushLiveEventPayload>,
) -> Result<Json<Value>, ResponseError> {
    ensure_admin_or_legacy(&state, &identity)?;
    let ts = if payload.ts > 0 {
        payload.ts
    } else {
        crate::routes::live::protocol::unix_timestamp_millis() as i64
    };
    let preview = if payload.raw_preview.trim().is_empty() {
        payload.text.clone()
    } else {
        payload.raw_preview.clone()
    };
    let event = json!({
        "type": "message.received",
        "uid": payload.uid,
        "sessionId": payload.session_id,
        "sessionType": payload.session_type.max(1),
        "peerNick": payload.peer_nick,
        "text": payload.text,
        "rawPreview": preview,
        "ts": ts,
        "version": ts,
        "isSelf": false,
    });
    state.live_events().publish(event.clone());
    Ok(Json(json!({
        "ok": true,
        "event": event,
    })))
}

pub(crate) fn worker_name() -> &'static str {
    LIVE_MESSAGE_RUNTIME_WORKER
}

pub(crate) async fn run_live_message_runtime(
    state: AppState,
    runtime: RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(LIVE_RUNTIME_TICK);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;
    let mut tasks: HashMap<String, LiveAccountTask> = HashMap::new();
    let mut last_reconcile_error = None::<String>;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            _ = interval.tick() => {
                tick_count += 1;
                match reconcile_live_account_tasks(&state, &mut tasks).await {
                    Ok(active_count) => {
                        let detail = format!("{active_count} active live workers");
                        let _ = state.storage()
                            .upsert_worker_state(LIVE_MESSAGE_RUNTIME_WORKER, "running", tick_count, Some(detail.as_str()))
                            .await;
                        runtime.record_worker_tick(LIVE_MESSAGE_RUNTIME_WORKER, "running", tick_count, None);
                        last_reconcile_error = None;
                    }
                    Err(error) => {
                        let detail = error.to_string();
                        let _ = state.storage()
                            .upsert_worker_state(LIVE_MESSAGE_RUNTIME_WORKER, "degraded", tick_count, Some(detail.as_str()))
                            .await;
                        runtime.record_worker_tick(
                            LIVE_MESSAGE_RUNTIME_WORKER,
                            "degraded",
                            tick_count,
                            Some(detail.clone()),
                        );
                        if last_reconcile_error.as_deref() != Some(detail.as_str()) {
                            let _ = state
                                .incident_reporter()
                                .report_from_state(
                                    &state,
                                    IncidentInput {
                                        error_level: "error".to_string(),
                                        module: "live".to_string(),
                                        action: "runtime_reconcile".to_string(),
                                        route: crate::sealed_protocol::sealed_str!("/api/live/runtime").to_string(),
                                        error_code: "live_runtime_reconcile_failed".to_string(),
                                        error_message: detail.clone(),
                                        http_status: None,
                                        upstream_ret: String::new(),
                                        retryable: true,
                                        context_json: json!({
                                            "worker": LIVE_MESSAGE_RUNTIME_WORKER,
                                            "tickCount": tick_count,
                                            "trackedTaskCount": tasks.len(),
                                        }),
                                        environment: None,
                                    },
                                )
                                .await;
                            last_reconcile_error = Some(detail);
                        }
                    }
                }
            }
        }
    }

    shutdown_live_account_tasks(&mut tasks).await;
    let _ = state
        .storage()
        .upsert_worker_state(LIVE_MESSAGE_RUNTIME_WORKER, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(LIVE_MESSAGE_RUNTIME_WORKER);
}
