use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, RwLock,
    },
    time::{Duration, Instant},
};

use axum::{extract::State, Extension, Json};
use once_cell::sync::Lazy;
use sqlx::Row;
use tokio::{
    sync::{mpsc, watch, Mutex as AsyncMutex},
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};

use crate::{
    models::auth::RequestIdentity,
    routes::{
        accounts::messages::{pc_send_message_internal, PcSendPayload},
        live,
        market::items::refresh_item_quantity_with_edit_fallback,
        market::orders::{
            refresh_authoritative_order_snapshot, run_order_consign_dummy_upstream_action,
        },
    },
    state::{AppState, RuntimeState},
    storage::unix_millis_now,
};

use super::{
    execute_delivery_run_as,
    ports::{
        AdminNotifier, BuyerMessenger, ConsignOutcome, GatewayError, MessageDeliveryResult,
        OrderConfirmation, OrderGateway, OrderSnapshot, StockInsufficientNotice,
    },
};

const WAKE_QUEUE_CAPACITY: usize = 1024;
const PERIODIC_SCAN_INTERVAL: Duration = Duration::from_secs(30);
// 商品库存由闲鱼上游维护；自动发货仅需定期校验已启用规则对应的商品，
// 不能把全量商品刷新塞进 30 秒订单扫描。
const CONFIGURED_PRODUCT_STOCK_REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);
const CONFIGURED_PRODUCT_STOCK_REFRESH_FAILURE_RETRY_INTERVAL: Duration = Duration::from_secs(60);
const DUE_RUN_BATCH_SIZE: i64 = 50;
const RUN_LEASE_TTL_MS: i64 = 30_000;
const DELIVERY_WORKER_NAME: &str = "auto_delivery_runtime";

struct WakeSenderSlot {
    generation: u64,
    sender: mpsc::Sender<WakeReason>,
}

static NEXT_WAKE_GENERATION: AtomicU64 = AtomicU64::new(1);
static WAKE_SENDER: Lazy<RwLock<Option<WakeSenderSlot>>> = Lazy::new(|| RwLock::new(None));

static ACCOUNT_LOCKS: Lazy<RwLock<HashMap<String, Arc<AsyncMutex<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static PRODUCT_STOCK_REFRESHED_AT: Lazy<
    AsyncMutex<HashMap<(String, String), StockRefreshAttempt>>,
> = Lazy::new(|| AsyncMutex::new(HashMap::new()));

#[derive(Clone, Copy)]
struct StockRefreshAttempt {
    attempted_at: Instant,
    retry_after: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeReason {
    uid: String,
    reason: String,
}

/// Test/helper install of the process-wide wake sender with a unique generation.
#[doc(hidden)]
pub struct TestWakeInstall {
    pub generation: u64,
    receiver: mpsc::Receiver<WakeReason>,
}

impl TestWakeInstall {
    pub async fn recv_reason(&mut self) -> Option<String> {
        self.receiver.recv().await.map(|wake| wake.reason)
    }
}

/// Install a wake sender for tests; overwrites the process-wide slot.
#[doc(hidden)]
pub fn install_test_wake_sender() -> TestWakeInstall {
    let generation = NEXT_WAKE_GENERATION.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel(WAKE_QUEUE_CAPACITY);
    if let Ok(mut guard) = WAKE_SENDER.write() {
        *guard = Some(WakeSenderSlot {
            generation,
            sender: tx,
        });
    }
    TestWakeInstall {
        generation,
        receiver: rx,
    }
}

/// Clear the process-wide wake sender only when `generation` still owns the slot.
#[doc(hidden)]
pub fn clear_wake_sender_if_generation(generation: u64) {
    if let Ok(mut guard) = WAKE_SENDER.write() {
        if guard
            .as_ref()
            .map(|slot| slot.generation == generation)
            .unwrap_or(false)
        {
            *guard = None;
        }
    }
}

pub fn worker_name() -> &'static str {
    DELIVERY_WORKER_NAME
}

pub fn new_worker_instance_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn start_runtime(
    state: AppState,
    runtime: RuntimeState,
    shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let generation = NEXT_WAKE_GENERATION.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel(WAKE_QUEUE_CAPACITY);
    if let Ok(mut guard) = WAKE_SENDER.write() {
        *guard = Some(WakeSenderSlot {
            generation,
            sender: tx,
        });
    }
    let worker_instance_id = new_worker_instance_id();
    tokio::spawn(run_runtime(
        state,
        runtime,
        shutdown_rx,
        rx,
        generation,
        worker_instance_id,
    ))
}

pub async fn wake_account(uid: &str, reason: &str) {
    let uid = uid.trim();
    if uid.is_empty() {
        return;
    }

    #[cfg(test)]
    super::record_test_account_wake(uid, reason);

    let sender = WAKE_SENDER
        .read()
        .ok()
        .and_then(|guard| guard.as_ref().map(|slot| slot.sender.clone()));
    let Some(sender) = sender else {
        return;
    };
    if let Err(error) = sender.try_send(WakeReason {
        uid: uid.to_string(),
        reason: reason.trim().to_string(),
    }) {
        tracing::warn!(
            target: "auto_delivery",
            uid,
            reason,
            error = %error,
            "自动发货唤醒队列已满，本次唤醒会等待下一轮定时扫描兜底"
        );
    }
}

pub async fn wake_pool(uid: &str, _pool_scope: &str, _pool_tag: &str) {
    wake_account(uid, "card_pool_changed").await;
}

async fn run_runtime(
    state: AppState,
    runtime: RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
    mut wake_rx: mpsc::Receiver<WakeReason>,
    wake_generation: u64,
    worker_instance_id: String,
) {
    let mut interval = time::interval(PERIODIC_SCAN_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut tick_count = 0_u64;

    scan_all_accounts(&state, &runtime, tick_count, "startup", &worker_instance_id).await;

    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
            Some(wake) = wake_rx.recv() => {
                tick_count += 1;
                process_account_wake(&state, &runtime, tick_count, &wake, &worker_instance_id).await;
            }
            _ = interval.tick() => {
                tick_count += 1;
                scan_all_accounts(&state, &runtime, tick_count, "periodic_scan", &worker_instance_id).await;
            }
        }
    }

    clear_wake_sender_if_generation(wake_generation);
    let _ = state
        .storage()
        .upsert_worker_state(DELIVERY_WORKER_NAME, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(DELIVERY_WORKER_NAME);
}

async fn scan_all_accounts(
    state: &AppState,
    runtime: &RuntimeState,
    tick_count: u64,
    reason: &str,
    worker_instance_id: &str,
) {
    let active_uids = match live::list_active_account_uids(state.storage().accounts()).await {
        Ok(uids) => uids,
        Err(error) => {
            record_runtime_state(
                state,
                runtime,
                tick_count,
                "degraded",
                Some(error.to_string()),
            )
            .await;
            return;
        }
    };

    record_runtime_state(
        state,
        runtime,
        tick_count,
        "running",
        Some(format!(
            "扫描{}个在线账号，原因：{}",
            active_uids.len(),
            reason
        )),
    )
    .await;
    for uid in active_uids {
        process_account(state, &uid, reason, worker_instance_id).await;
    }
}

async fn process_account_wake(
    state: &AppState,
    runtime: &RuntimeState,
    tick_count: u64,
    wake: &WakeReason,
    worker_instance_id: &str,
) {
    record_runtime_state(
        state,
        runtime,
        tick_count,
        "running",
        Some(format!("收到账号{}自动发货唤醒：{}", wake.uid, wake.reason)),
    )
    .await;
    process_account(state, &wake.uid, &wake.reason, worker_instance_id).await;
}

async fn process_account(state: &AppState, uid: &str, reason: &str, worker_instance_id: &str) {
    let lock = account_lock(uid);
    let _guard = lock.lock().await;

    let enabled_configurations = match state
        .storage()
        .list_delivery_configuration_records(uid)
        .await
    {
        Ok(configurations) => configurations
            .into_iter()
            .filter(|configuration| configuration.enabled && !configuration.needs_repair)
            .collect::<Vec<_>>(),
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                uid,
                reason,
                error = %error,
                "自动发货读取账号配置失败"
            );
            return;
        }
    };
    if enabled_configurations.is_empty() {
        return;
    }

    if should_sync_orders_before_scan(reason) {
        match crate::routes::market::sync_orders_to_db(state, uid, true).await {
            Ok(outcome) if outcome.ok => {
                let _ = state
                    .storage()
                    .resolve_delivery_sync_failure_alert(uid)
                    .await;
                tracing::info!(
                    target: "auto_delivery",
                    uid,
                    reason,
                    total = outcome.total,
                    "自动发货已主动同步最新订单"
                );
            }
            Ok(outcome) => {
                let _ = state
                    .storage()
                    .upsert_delivery_sync_failure_alert(uid, &outcome.detail)
                    .await;
                tracing::warn!(
                    target: "auto_delivery",
                    uid,
                    reason,
                    detail = %outcome.detail,
                    "自动发货同步最新订单失败，继续检查本地订单"
                );
            }
            Err(error) => {
                let _ = state
                    .storage()
                    .upsert_delivery_sync_failure_alert(uid, &error.to_string())
                    .await;
                tracing::warn!(
                    target: "auto_delivery",
                    uid,
                    reason,
                    error = %error,
                    "自动发货同步最新订单异常，继续检查本地订单"
                );
            }
        }
    }

    // Inventory refresh is an independent best-effort cache update.  Never let a slow
    // upstream item request postpone creation or execution of a delivery task.
    let stock_refresh_state = state.clone();
    let stock_refresh_uid = uid.to_string();
    let stock_refresh_reason = reason.to_string();
    tokio::spawn(async move {
        refresh_configured_product_stock(
            &stock_refresh_state,
            &stock_refresh_uid,
            &enabled_configurations,
            &stock_refresh_reason,
        )
        .await;
    });

    match state
        .storage()
        .create_delivery_runs_for_ready_orders(uid)
        .await
    {
        Ok(created) if created > 0 => {
            tracing::info!(
                target: "auto_delivery",
                uid,
                created,
                reason,
                "自动发货已根据待发货订单创建执行任务"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                uid,
                reason,
                error = %error,
                "自动发货扫描待发货订单失败"
            );
            return;
        }
    }

    match state.storage().reconcile_unqueued_closed_orders(uid).await {
        Ok(recorded) if recorded > 0 => {
            tracing::warn!(
                target: "auto_delivery",
                uid,
                recorded,
                "自动发货发现已关闭且从未入队的疑似漏单"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                uid,
                error = %error,
                "自动发货漏单对账失败"
            );
        }
    }

    let run_ids = match state
        .storage()
        .claim_due_delivery_runs(
            uid,
            worker_instance_id,
            unix_millis_now(),
            RUN_LEASE_TTL_MS,
            DUE_RUN_BATCH_SIZE,
        )
        .await
    {
        Ok(run_ids) => run_ids,
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                uid,
                reason,
                error = %error,
                "自动发货读取待执行任务失败"
            );
            return;
        }
    };
    if run_ids.is_empty() {
        return;
    }

    let gateway = AppOrderGateway {
        state: state.clone(),
    };
    let messenger = AppBuyerMessenger {
        state: state.clone(),
    };
    let admin_notifier = AppAdminNotifier {
        state: state.clone(),
    };
    for run_id in run_ids {
        match execute_delivery_run_as(
            state.storage(),
            &gateway,
            &messenger,
            &admin_notifier,
            run_id,
            worker_instance_id,
        )
        .await
        {
            Ok(run) => {
                tracing::info!(
                    target: "auto_delivery",
                    uid = %run.uid,
                    order_id = %run.order_id,
                    run_id = run.id,
                    status = %run.status,
                    "自动发货任务执行完成一次状态推进"
                );
            }
            Err(error) => {
                tracing::warn!(
                    target: "auto_delivery",
                    uid,
                    run_id,
                    error = %error,
                    "自动发货任务执行失败，等待下次扫描或人工处理"
                );
            }
        }
    }
}

async fn refresh_configured_product_stock(
    state: &AppState,
    uid: &str,
    configurations: &[crate::models::delivery::DeliveryConfigurationRecord],
    reason: &str,
) {
    let item_ids = configurations
        .iter()
        .map(|configuration| configuration.item_id.trim())
        .filter(|item_id| !item_id.is_empty())
        .collect::<std::collections::HashSet<_>>();
    for item_id in item_ids {
        let key = (uid.to_string(), item_id.to_string());
        let should_refresh = {
            let mut refreshed_at = PRODUCT_STOCK_REFRESHED_AT.lock().await;
            let now = Instant::now();
            if refreshed_at
                .get(&key)
                .is_some_and(|last| now.duration_since(last.attempted_at) < last.retry_after)
            {
                false
            } else {
                refreshed_at.insert(
                    key.clone(),
                    StockRefreshAttempt {
                        attempted_at: now,
                        retry_after: CONFIGURED_PRODUCT_STOCK_REFRESH_FAILURE_RETRY_INTERVAL,
                    },
                );
                true
            }
        };
        if !should_refresh {
            continue;
        }
        match refresh_item_quantity_with_edit_fallback(state, uid, item_id).await {
            Ok(_) => {
                PRODUCT_STOCK_REFRESHED_AT.lock().await.insert(
                    key,
                    StockRefreshAttempt {
                        attempted_at: Instant::now(),
                        retry_after: CONFIGURED_PRODUCT_STOCK_REFRESH_INTERVAL,
                    },
                );
            }
            Err(error) => tracing::warn!(
                target: "auto_delivery",
                uid,
                item_id,
                reason,
                error = ?error,
                "自动发货后台刷新配置商品库存失败"
            ),
        }
    }
}

fn should_sync_orders_before_scan(reason: &str) -> bool {
    matches!(
        reason.trim(),
        "startup" | "periodic_scan" | "paid_message" | "sale_notification"
    )
}

async fn record_runtime_state(
    state: &AppState,
    runtime: &RuntimeState,
    tick_count: u64,
    status: &str,
    detail: Option<String>,
) {
    let detail_ref = detail.as_deref();
    let _ = state
        .storage()
        .upsert_worker_state(DELIVERY_WORKER_NAME, status, tick_count, detail_ref)
        .await;
    runtime.record_worker_tick(DELIVERY_WORKER_NAME, status, tick_count, detail);
}

fn account_lock(uid: &str) -> Arc<AsyncMutex<()>> {
    if let Ok(guard) = ACCOUNT_LOCKS.read() {
        if let Some(lock) = guard.get(uid) {
            return lock.clone();
        }
    }
    let mut guard = ACCOUNT_LOCKS
        .write()
        .expect("auto delivery account lock registry poisoned");
    guard
        .entry(uid.to_string())
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

#[derive(Clone)]
struct AppOrderGateway {
    state: AppState,
}

impl OrderGateway for AppOrderGateway {
    async fn refresh_and_confirm_order(
        &self,
        uid: &str,
        order_id: &str,
    ) -> Result<OrderSnapshot, GatewayError> {
        let snapshot = refresh_authoritative_order_snapshot(&self.state, uid, order_id)
            .await
            .map_err(|message| GatewayError::transient("order_refresh_failed", message))?;
        Ok(OrderSnapshot {
            confirmation: classify_order_confirmation(&snapshot.status, &snapshot.status_text),
            purchase_quantity: snapshot.purchase_quantity,
            purchase_quantity_source: snapshot.purchase_quantity_source,
            refreshed_at: snapshot.refreshed_at,
        })
    }

    async fn consign_without_shipping(
        &self,
        uid: &str,
        order_id: &str,
    ) -> Result<ConsignOutcome, GatewayError> {
        match run_order_consign_dummy_upstream_action(&self.state, uid, order_id).await {
            Ok(_) => Ok(ConsignOutcome::Consigned),
            // Order already shipped outside this pipeline (manual ship / prior consign).
            Err(error)
                if error.contains("已发货")
                    || error.contains("已无需快递")
                    || error.contains("无需快递")
                    || error.contains("已发过货") =>
            {
                Ok(ConsignOutcome::AlreadyConsigned {
                    safe_to_send: false,
                })
            }
            Err(error) if is_transient_upstream_error(&error) => {
                Err(GatewayError::transient("consign_temporary", error))
            }
            Err(error) => Err(GatewayError::permanent("consign_rejected", error)),
        }
    }
}

#[derive(Clone)]
struct AppBuyerMessenger {
    state: AppState,
}

impl BuyerMessenger for AppBuyerMessenger {
    async fn send_text_and_wait_ack(
        &self,
        uid: &str,
        buyer_id: &str,
        content: &str,
    ) -> MessageDeliveryResult {
        let session = match resolve_buyer_session(&self.state, uid, buyer_id).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                return MessageDeliveryResult::TransientFailure {
                    code: "message_session_missing".to_string(),
                    message: "买家会话暂未同步到本地，稍后重试".to_string(),
                };
            }
            Err(error) => {
                return MessageDeliveryResult::TransientFailure {
                    code: "message_session_db_error".to_string(),
                    message: error.to_string(),
                };
            }
        };

        let payload = PcSendPayload {
            uid: uid.trim().to_string(),
            session_id: session.session_id,
            to_uid: session.peer_uid,
            content: content.to_string(),
            reply_to_message_id: String::new(),
            reply_to_text: String::new(),
        };
        match pc_send_message_internal(
            State(self.state.clone()),
            system_identity(&self.state),
            Json(payload),
        )
        .await
        {
            Ok(Json(value)) => {
                let message_id = value
                    .get("upstreamMessageId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let uuid = value
                    .get("messageId")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if message_id.is_empty() || uuid.is_empty() {
                    MessageDeliveryResult::Uncertain
                } else {
                    MessageDeliveryResult::Acked { message_id, uuid }
                }
            }
            Err(error) => message_result_from_response_error(error),
        }
    }
}

#[derive(Debug, Clone)]
struct BuyerSession {
    session_id: String,
    peer_uid: String,
}

async fn resolve_buyer_session(
    state: &AppState,
    uid: &str,
    buyer_id: &str,
) -> Result<Option<BuyerSession>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT session_id, peer_uid
        FROM message_live_state
        WHERE uid = ? AND session_type = 1 AND TRIM(peer_uid) = ?
        ORDER BY updated_at DESC, last_message_ts DESC
        LIMIT 1
        "#,
    )
    .bind(uid.trim())
    .bind(buyer_id.trim())
    .fetch_optional(state.storage().live_state())
    .await?;
    Ok(row.map(|row| BuyerSession {
        session_id: row.get::<String, _>("session_id"),
        peer_uid: row.get::<String, _>("peer_uid"),
    }))
}

fn classify_order_confirmation(status: &str, status_text: &str) -> OrderConfirmation {
    let status_upper = status.trim().to_ascii_uppercase();
    let text = status_text.trim();
    if status_upper.contains("CANCEL")
        || status_upper.contains("CLOSE")
        || status_upper.contains("REFUND")
        || text.contains("已关闭")
        || text.contains("退款")
    {
        return OrderConfirmation::Cancelled;
    }
    // Post-ship states: order is already shipped on Xianyu (including manual ship).
    // Executor decides: if this pipeline consigned it, continue send; otherwise skip entirely.
    if status_upper.contains("CONSIGNED")
        || status_upper.contains("SUCCESS")
        || status_upper.contains("BUYER_TO_CONFIRM")
        || status_upper.contains("WAIT_BUYER")
        || text.contains("已发货")
        || text.contains("已完成")
        || text.contains("无需快递")
        || text.contains("等待见面交易")
        || text.contains("等待买家确认")
        || text.contains("待买家确认")
    {
        return OrderConfirmation::AlreadyConsigned {
            safe_to_send: false,
        };
    }
    OrderConfirmation::Ready
}

#[cfg(test)]
mod tests {
    use super::should_sync_orders_before_scan;

    #[test]
    fn configured_delivery_scans_refresh_orders_without_waiting_for_payment_message() {
        assert!(should_sync_orders_before_scan("startup"));
        assert!(should_sync_orders_before_scan("periodic_scan"));
        assert!(should_sync_orders_before_scan("paid_message"));
        assert!(should_sync_orders_before_scan("sale_notification"));
        assert!(!should_sync_orders_before_scan("card_pool_changed"));
    }
}

fn message_result_from_response_error(
    error: crate::routes::error::ResponseError,
) -> MessageDeliveryResult {
    let detail = error.detail().to_string();
    if detail.contains("未收到闲鱼确认")
        || detail.contains("ack_timeout")
        || detail.contains("ack_missing")
        || detail.contains("ACK")
    {
        return MessageDeliveryResult::Uncertain;
    }
    if error.status().is_client_error() {
        return MessageDeliveryResult::PermanentFailure {
            code: "message_send_rejected".to_string(),
            message: detail,
        };
    }
    MessageDeliveryResult::TransientFailure {
        code: "message_send_temporary".to_string(),
        message: detail,
    }
}

fn is_transient_upstream_error(error: &str) -> bool {
    let text = error.to_ascii_lowercase();
    text.contains("timeout")
        || text.contains("temporar")
        || text.contains("502")
        || text.contains("503")
        || text.contains("504")
        || text.contains("sign")
        || text.contains("鉴权")
        || text.contains("网络")
        || text.contains("稍后")
}

fn system_identity(state: &AppState) -> Extension<RequestIdentity> {
    Extension(RequestIdentity::new(
        state.config().auth.username.clone(),
        "admin".to_string(),
        true,
        false,
    ))
}

#[derive(Clone)]
struct AppAdminNotifier {
    state: AppState,
}

impl AdminNotifier for AppAdminNotifier {
    async fn notify_stock_insufficient(&self, notice: &StockInsufficientNotice) {
        dispatch_stock_insufficient_admin_notice(&self.state, notice).await;
    }
}

async fn dispatch_stock_insufficient_admin_notice(
    state: &AppState,
    notice: &StockInsufficientNotice,
) {
    // System mark is best-effort local flag for future in-app toast wiring.
    match state
        .storage()
        .mark_delivery_alert_system_notified(notice.alert_id)
        .await
    {
        Ok(true) => {
            tracing::info!(
                target: "auto_delivery",
                alert_id = notice.alert_id,
                uid = %notice.uid,
                order_id = %notice.order_id,
                "自动发货缺货告警已记录系统通知标记"
            );
        }
        Ok(false) => {
            // Already marked by another worker; continue external notify only if still needed.
        }
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                alert_id = notice.alert_id,
                error = %error,
                "自动发货缺货告警写入系统通知标记失败"
            );
        }
    }

    let owner = match state
        .storage()
        .notification_owner_for_uid(&notice.uid)
        .await
    {
        Ok(owner) => owner,
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                uid = %notice.uid,
                alert_id = notice.alert_id,
                error = %error,
                "自动发货缺货告警解析账号归属用户失败"
            );
            return;
        }
    };
    if owner.trim().is_empty() {
        tracing::info!(
            target: "auto_delivery",
            uid = %notice.uid,
            alert_id = notice.alert_id,
            "自动发货缺货告警账号无归属用户，跳过外部推送"
        );
        return;
    }
    let channels = match state
        .storage()
        .list_notification_channels_for_owner(&owner)
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(
                target: "auto_delivery",
                uid = %notice.uid,
                owner = %owner,
                alert_id = notice.alert_id,
                error = %error,
                "自动发货缺货告警列出管理员通知渠道失败"
            );
            return;
        }
    };

    let enabled: Vec<_> = channels
        .into_iter()
        .filter(|channel| channel.enabled)
        .collect();
    if enabled.is_empty() {
        tracing::info!(
            target: "auto_delivery",
            uid = %notice.uid,
            alert_id = notice.alert_id,
            "自动发货缺货告警无可用管理员通知渠道，跳过外部推送"
        );
        // Mark as handled so the 30s scanner does not re-enter every tick.
        let _ = state
            .storage()
            .mark_delivery_alert_external_notified(notice.alert_id)
            .await;
        return;
    }

    let account_nick = account_nick_for_uid(state, &notice.uid).await;
    let text = format!(
        "【自动发货·卡密不足】\n账号：{nick}（{uid}）\n订单：{order_id}\n商品：{item_id}\n需要 {required} 条，当前 {available} 条，还差 {missing} 条\n请尽快补充卡密库存，系统会在补货后自动继续发货。\n{title}\n{body}",
        nick = if account_nick.is_empty() { notice.uid.as_str() } else { account_nick.as_str() },
        uid = notice.uid,
        order_id = notice.order_id,
        item_id = notice.item_id,
        required = notice.required,
        available = notice.available,
        missing = notice.required.saturating_sub(notice.available),
        title = notice.title,
        body = notice.body,
    );

    let mut any_success = false;
    for channel in &enabled {
        match crate::notifications::driver::send_channel_text_with_runtime(state, channel, &text)
            .await
        {
            Ok(_) => {
                any_success = true;
                tracing::info!(
                    target: "auto_delivery",
                    alert_id = notice.alert_id,
                    channel_id = channel.id,
                    channel_type = %channel.channel_type,
                    "自动发货缺货告警已推送到管理员渠道"
                );
            }
            Err(error) => {
                tracing::warn!(
                    target: "auto_delivery",
                    alert_id = notice.alert_id,
                    channel_id = channel.id,
                    channel_type = %channel.channel_type,
                    error = %error,
                    "自动发货缺货告警推送管理员渠道失败"
                );
            }
        }
    }

    if any_success {
        if let Err(error) = state
            .storage()
            .mark_delivery_alert_external_notified(notice.alert_id)
            .await
        {
            tracing::warn!(
                target: "auto_delivery",
                alert_id = notice.alert_id,
                error = %error,
                "自动发货缺货告警写入外部通知标记失败"
            );
        }
    }
}

async fn account_nick_for_uid(state: &AppState, uid: &str) -> String {
    match sqlx::query_scalar::<_, String>(
        "SELECT COALESCE(nick, '') FROM accounts WHERE uid = ? LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(state.storage().accounts())
    .await
    {
        Ok(Some(nick)) => nick,
        _ => String::new(),
    }
}
