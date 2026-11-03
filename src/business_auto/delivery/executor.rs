use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    models::delivery::{
        CardReservation, DeliveryCompletionOutcome, DeliveryRun, DeliveryRunStatus, SideEffectClaim,
    },
    storage::{unix_millis_now, StorageError, StorageState},
};

use super::{
    compose_delivery_message,
    ports::{
        AdminNotifier, BuyerMessenger, ConsignOutcome, GatewayError, MessageDeliveryResult,
        NoopAdminNotifier, OrderConfirmation, OrderGateway, StockInsufficientNotice,
    },
};

pub const WAITING_STOCK_BUYER_NOTICE: &str =
    "您好，当前卡密库存不足，系统已通知管理员补充库存，请耐心等待。库存补充后会自动为您发货。";
const RUN_LEASE_TTL_MS: i64 = 30_000;

static RUN_LOCKS: Lazy<Mutex<HashMap<i64, Weak<AsyncMutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static COMPAT_WORKER_INSTANCE_ID: Lazy<String> =
    Lazy::new(|| format!("compat-{}", uuid::Uuid::new_v4()));

pub async fn execute_delivery_run<G, M>(
    storage: &StorageState,
    order_gateway: &G,
    messenger: &M,
    run_id: i64,
) -> Result<DeliveryRun, StorageError>
where
    G: OrderGateway,
    M: BuyerMessenger,
{
    execute_delivery_run_with_admin(
        storage,
        order_gateway,
        messenger,
        &NoopAdminNotifier,
        run_id,
    )
    .await
}

pub async fn execute_delivery_run_with_admin<G, M, A>(
    storage: &StorageState,
    order_gateway: &G,
    messenger: &M,
    admin_notifier: &A,
    run_id: i64,
) -> Result<DeliveryRun, StorageError>
where
    G: OrderGateway,
    M: BuyerMessenger,
    A: AdminNotifier,
{
    let run = load_run(storage, run_id).await?;
    if is_terminal(run.status) || run.status == DeliveryRunStatus::ManualRequired {
        return Ok(run);
    }
    let owner = COMPAT_WORKER_INSTANCE_ID.as_str();
    let claimed = storage
        .claim_due_delivery_runs(&run.uid, owner, unix_millis_now(), RUN_LEASE_TTL_MS, 200)
        .await?;
    if !claimed.contains(&run_id) {
        return load_run(storage, run_id).await;
    }
    execute_delivery_run_as(
        storage,
        order_gateway,
        messenger,
        admin_notifier,
        run_id,
        owner,
    )
    .await
}

pub async fn execute_delivery_run_as<G, M, A>(
    storage: &StorageState,
    order_gateway: &G,
    messenger: &M,
    admin_notifier: &A,
    run_id: i64,
    worker_instance_id: &str,
) -> Result<DeliveryRun, StorageError>
where
    G: OrderGateway,
    M: BuyerMessenger,
    A: AdminNotifier,
{
    let run_lock = run_lock(run_id);
    let _guard = run_lock.lock().await;
    execute_locked(
        storage,
        order_gateway,
        messenger,
        admin_notifier,
        run_id,
        worker_instance_id,
    )
    .await
}

async fn execute_locked<G, M, A>(
    storage: &StorageState,
    order_gateway: &G,
    messenger: &M,
    admin_notifier: &A,
    run_id: i64,
    worker_instance_id: &str,
) -> Result<DeliveryRun, StorageError>
where
    G: OrderGateway,
    M: BuyerMessenger,
    A: AdminNotifier,
{
    let mut run = load_run(storage, run_id).await?;
    if is_terminal(run.status) {
        return Ok(run);
    }
    if run.status == DeliveryRunStatus::ManualRequired {
        return Ok(run);
    }
    let mut send_claim_acquired = false;
    if run.status == DeliveryRunStatus::Sending {
        match storage
            .claim_delivery_send(
                run.id,
                worker_instance_id,
                unix_millis_now(),
                RUN_LEASE_TTL_MS,
            )
            .await?
        {
            SideEffectClaim::Acquired => send_claim_acquired = true,
            SideEffectClaim::ActiveElsewhere | SideEffectClaim::AlreadyAcknowledged => {
                return Ok(run)
            }
            SideEffectClaim::ExpiredUncertain => {
                transition_error(
                    storage,
                    &run,
                    DeliveryRunStatus::ManualRequired,
                    "send_result_uncertain",
                    "上次交付消息发送被中断，结果不确定，请人工核对历史消息",
                    0,
                )
                .await?;
                return load_run(storage, run_id).await;
            }
        }
    }
    if run.status == DeliveryRunStatus::WaitingStock && run.waiting_notice_claimed_at > 0 {
        match storage
            .claim_waiting_notice(
                run.id,
                worker_instance_id,
                unix_millis_now(),
                RUN_LEASE_TTL_MS,
            )
            .await?
        {
            SideEffectClaim::Acquired
            | SideEffectClaim::ActiveElsewhere
            | SideEffectClaim::AlreadyAcknowledged => return Ok(run),
            SideEffectClaim::ExpiredUncertain => {
                transition_error(
                    storage,
                    &run,
                    DeliveryRunStatus::ManualRequired,
                    "waiting_notice_result_uncertain",
                    "上次库存等待通知发送被中断，结果不确定，请人工核对历史消息",
                    0,
                )
                .await?;
                return load_run(storage, run_id).await;
            }
        }
    }
    if run.next_retry_at > unix_millis_now() {
        return Ok(run);
    }

    if !renew_lease(storage, run.id, worker_instance_id).await? {
        return Ok(run);
    }

    let order_snapshot = order_gateway
        .refresh_and_confirm_order(&run.uid, &run.order_id)
        .await;
    let upstream_consigned = match order_snapshot {
        Ok(snapshot) => {
            if matches!(
                run.status,
                DeliveryRunStatus::Pending
                    | DeliveryRunStatus::WaitingStock
                    | DeliveryRunStatus::Retrying
            ) {
                run = storage
                    .confirm_run_quantity_before_reservation(
                        run.id,
                        snapshot.purchase_quantity,
                        &snapshot.purchase_quantity_source,
                        snapshot.refreshed_at,
                    )
                    .await?;
                if run.status == DeliveryRunStatus::Retrying
                    && run.last_error_code == "quantity_unknown_retryable"
                {
                    return Ok(run);
                }
            }
            match snapshot.confirmation {
                OrderConfirmation::Ready => run.consigned_at > 0,
                OrderConfirmation::Cancelled => {
                    storage.cancel_delivery_run(run.id).await?;
                    return load_run(storage, run_id).await;
                }
                // Already shipped on Xianyu.
                // - If this pipeline already consigned (consigned_at > 0), continue to send content.
                // - If someone else shipped (manual ship etc.), skip the whole auto-delivery:
                //   do NOT send fixed text / cards again.
                OrderConfirmation::AlreadyConsigned { .. } if run.consigned_at > 0 => true,
                OrderConfirmation::AlreadyConsigned { .. } => {
                    storage
                        .skip_delivery_run_external_ship(run.id, "订单已手动发货，跳过自动发货")
                        .await?;
                    return load_run(storage, run_id).await;
                }
            }
        }
        Err(error) => {
            persist_gateway_error(storage, &run, error).await?;
            return load_run(storage, run_id).await;
        }
    };

    if run.status == DeliveryRunStatus::ManualRequired {
        return Ok(run);
    }
    let latest_run =
        storage
            .delivery_run_raw(run.id)
            .await?
            .ok_or_else(|| StorageError::Conflict {
                message: format!("delivery run {} does not exist", run.id),
            })?;
    if !latest_run.fixed_message_snapshot_captured {
        transition_error(
            storage,
            &latest_run,
            DeliveryRunStatus::ManualRequired,
            "snapshot_missing",
            "执行记录缺少稳定配置快照，请人工处理",
            0,
        )
        .await?;
        return load_run(storage, run_id).await;
    }

    if run.status == DeliveryRunStatus::Consigning {
        if upstream_consigned {
            storage
                .transition_delivery_run(
                    run.id,
                    DeliveryRunStatus::Consigning,
                    DeliveryRunStatus::Sending,
                    "",
                    "",
                    0,
                    false,
                )
                .await?;
            run = load_run(storage, run_id).await?;
        } else {
            transition_error(
                storage,
                &run,
                DeliveryRunStatus::ManualRequired,
                "consign_result_uncertain",
                "发货确认结果不确定，请人工核对订单状态",
                0,
            )
            .await?;
            return load_run(storage, run_id).await;
        }
    }

    if run.status == DeliveryRunStatus::Retrying && run.consigned_at > 0 {
        storage
            .transition_delivery_run(
                run.id,
                DeliveryRunStatus::Retrying,
                DeliveryRunStatus::Sending,
                "",
                "",
                0,
                false,
            )
            .await?;
        run = load_run(storage, run_id).await?;
    }

    if matches!(
        run.status,
        DeliveryRunStatus::Pending | DeliveryRunStatus::WaitingStock | DeliveryRunStatus::Retrying
    ) {
        let reservation = storage.reserve_cards_for_run_snapshot(run.id).await?;
        match reservation {
            CardReservation::Reserved { .. } => run = load_run(storage, run_id).await?,
            CardReservation::Insufficient {
                available,
                required,
            } => {
                handle_waiting_stock(
                    storage,
                    messenger,
                    admin_notifier,
                    &run,
                    available,
                    required,
                    worker_instance_id,
                )
                .await?;
                return load_run(storage, run_id).await;
            }
        }
    }

    if run.status == DeliveryRunStatus::Reserved {
        if upstream_consigned {
            storage
                .transition_delivery_run(
                    run.id,
                    DeliveryRunStatus::Reserved,
                    DeliveryRunStatus::Sending,
                    "",
                    "",
                    0,
                    false,
                )
                .await?;
        } else {
            let claimed = storage
                .transition_delivery_run(
                    run.id,
                    DeliveryRunStatus::Reserved,
                    DeliveryRunStatus::Consigning,
                    "",
                    "",
                    0,
                    false,
                )
                .await?;
            if !claimed {
                return load_run(storage, run_id).await;
            }
            run = load_run(storage, run_id).await?;
            if !renew_lease(storage, run.id, worker_instance_id).await? {
                return Ok(run);
            }
            match order_gateway
                .consign_without_shipping(&run.uid, &run.order_id)
                .await
            {
                Ok(ConsignOutcome::Consigned) => {
                    storage
                        .transition_delivery_run(
                            run.id,
                            DeliveryRunStatus::Consigning,
                            DeliveryRunStatus::Sending,
                            "",
                            "",
                            0,
                            false,
                        )
                        .await?;
                }
                // Consign API says already shipped, but this pipeline has not completed consign yet
                // (we are still in Consigning and consigned_at is still 0 until transition to Sending).
                // That means the seller shipped manually / elsewhere — skip, do not send content.
                Ok(ConsignOutcome::AlreadyConsigned { .. }) => {
                    storage
                        .skip_delivery_run_external_ship(run.id, "订单已手动发货，跳过自动发货")
                        .await?;
                    return load_run(storage, run_id).await;
                }
                Err(error) => {
                    persist_gateway_error(storage, &run, error).await?;
                    return load_run(storage, run_id).await;
                }
            }
        }
        run = load_run(storage, run_id).await?;
    }

    if run.status == DeliveryRunStatus::Sending {
        let claim = if send_claim_acquired {
            SideEffectClaim::Acquired
        } else {
            storage
                .claim_delivery_send(
                    run.id,
                    worker_instance_id,
                    unix_millis_now(),
                    RUN_LEASE_TTL_MS,
                )
                .await?
        };
        match claim {
            SideEffectClaim::Acquired => {
                if renew_lease(storage, run.id, worker_instance_id).await? {
                    send_delivery_message(storage, messenger, &run, worker_instance_id).await?;
                }
            }
            SideEffectClaim::ExpiredUncertain => {
                transition_error(
                    storage,
                    &run,
                    DeliveryRunStatus::ManualRequired,
                    "send_result_uncertain",
                    "上次交付消息发送被中断，结果不确定，请人工核对历史消息",
                    0,
                )
                .await?;
            }
            SideEffectClaim::ActiveElsewhere | SideEffectClaim::AlreadyAcknowledged => {}
        }
    }
    load_run(storage, run_id).await
}

async fn handle_waiting_stock<M: BuyerMessenger, A: AdminNotifier>(
    storage: &StorageState,
    messenger: &M,
    admin_notifier: &A,
    run: &DeliveryRun,
    available: i64,
    required: i64,
    worker_instance_id: &str,
) -> Result<(), StorageError> {
    let missing = required.saturating_sub(available);
    let title = "自动发货等待补充卡密".to_string();
    let body = format!(
        "订单 {} 需要 {required} 条卡密，当前 {available} 条，还差 {missing} 条",
        run.order_id
    );
    let alert = storage
        .upsert_waiting_stock_alert(run, &title, &body)
        .await?;
    storage
        .transition_delivery_run(
            run.id,
            run.status,
            DeliveryRunStatus::WaitingStock,
            "card_stock_insufficient",
            "一次性卡密库存不足，等待管理员补充",
            0,
            true,
        )
        .await?;

    // Best-effort admin channels (Feishu etc.). Once external_notified_at is set,
    // do not push again. Failures never roll back the persisted alert.
    if alert.external_notified_at == 0 {
        admin_notifier
            .notify_stock_insufficient(&StockInsufficientNotice {
                alert_id: alert.id,
                uid: run.uid.clone(),
                order_id: run.order_id.clone(),
                item_id: run.item_id.clone(),
                title,
                body,
                available,
                required,
            })
            .await;
    }

    match storage
        .claim_waiting_notice(
            run.id,
            worker_instance_id,
            unix_millis_now(),
            RUN_LEASE_TTL_MS,
        )
        .await?
    {
        SideEffectClaim::Acquired => {}
        SideEffectClaim::ExpiredUncertain => {
            transition_error(
                storage,
                &load_run(storage, run.id).await?,
                DeliveryRunStatus::ManualRequired,
                "waiting_notice_result_uncertain",
                "上次库存等待通知发送被中断，结果不确定，请人工核对历史消息",
                0,
            )
            .await?;
            return Ok(());
        }
        SideEffectClaim::ActiveElsewhere | SideEffectClaim::AlreadyAcknowledged => return Ok(()),
    }
    if !renew_lease(storage, run.id, worker_instance_id).await? {
        return Ok(());
    };
    match messenger
        .send_text_and_wait_ack(&run.uid, &run.buyer_id, WAITING_STOCK_BUYER_NOTICE)
        .await
    {
        MessageDeliveryResult::Acked { .. } => {
            storage
                .finish_waiting_notice(run.id, worker_instance_id)
                .await?;
        }
        MessageDeliveryResult::TransientFailure { code, message: _ } => {
            storage
                .reset_waiting_notice_claim(run.id, worker_instance_id)
                .await?;
            storage
                .transition_delivery_run(
                    run.id,
                    DeliveryRunStatus::WaitingStock,
                    DeliveryRunStatus::WaitingStock,
                    &code,
                    "库存等待通知发送暂时失败，系统将稍后重试",
                    retry_at(run.attempt_count),
                    true,
                )
                .await?;
        }
        MessageDeliveryResult::Uncertain => {
            transition_error(
                storage,
                &load_run(storage, run.id).await?,
                DeliveryRunStatus::ManualRequired,
                "waiting_notice_result_uncertain",
                "库存等待通知发送结果不确定，请人工确认",
                0,
            )
            .await?;
        }
        MessageDeliveryResult::PermanentFailure { code, message: _ } => {
            storage
                .reset_waiting_notice_claim(run.id, worker_instance_id)
                .await?;
            transition_error(
                storage,
                &load_run(storage, run.id).await?,
                DeliveryRunStatus::ManualRequired,
                &code,
                "库存等待通知无法自动发送，请人工处理",
                0,
            )
            .await?;
        }
    }
    Ok(())
}

async fn send_delivery_message<M: BuyerMessenger>(
    storage: &StorageState,
    messenger: &M,
    run: &DeliveryRun,
    worker_instance_id: &str,
) -> Result<(), StorageError> {
    let one_time_contents = storage
        .delivery_run_cards(run.id)
        .await?
        .into_iter()
        .map(|card| card.content_snapshot)
        .collect::<Vec<_>>();
    let fixed_contents = storage.fixed_delivery_card_contents_for_run(run).await?;
    let mut sections = Vec::new();
    if !run.fixed_message_snapshot.trim().is_empty() {
        sections.push(run.fixed_message_snapshot.trim().to_string());
    }
    sections.extend(fixed_contents);
    let cards_message = compose_delivery_message("", &one_time_contents);
    if !cards_message.is_empty() {
        sections.push(cards_message);
    }
    let message = sections.join("\n\n");
    if message.is_empty() {
        transition_error(
            storage,
            run,
            DeliveryRunStatus::ManualRequired,
            "delivery_message_empty",
            "自动发货内容为空，请人工检查配置",
            0,
        )
        .await?;
        return Ok(());
    }

    if !renew_lease(storage, run.id, worker_instance_id).await? {
        return Ok(());
    }
    match messenger
        .send_text_and_wait_ack(&run.uid, &run.buyer_id, &message)
        .await
    {
        MessageDeliveryResult::Acked { message_id, uuid } => {
            match storage
                .acknowledge_and_complete_delivery_run(
                    run.id,
                    worker_instance_id,
                    &run.order_id,
                    &message_id,
                    &uuid,
                )
                .await?
            {
                DeliveryCompletionOutcome::Completed
                | DeliveryCompletionOutcome::AlreadyCompleted
                | DeliveryCompletionOutcome::AckPersistedButCompletionConflict => {}
            }
        }
        MessageDeliveryResult::Uncertain => {
            transition_error(
                storage,
                run,
                DeliveryRunStatus::ManualRequired,
                "send_result_uncertain",
                "交付消息发送结果不确定，请先核对历史消息",
                0,
            )
            .await?;
        }
        MessageDeliveryResult::TransientFailure { code, message } => {
            storage.clear_delivery_send_claim(run.id).await?;
            let detail = if message.trim().is_empty() {
                "交付消息发送暂时失败，系统将稍后重试".to_string()
            } else {
                format!("交付消息发送暂时失败：{message}")
            };
            transition_error(
                storage,
                run,
                DeliveryRunStatus::Retrying,
                &code,
                &detail,
                retry_at(run.attempt_count),
            )
            .await?;
        }
        MessageDeliveryResult::PermanentFailure { code, message } => {
            storage.clear_delivery_send_claim(run.id).await?;
            let detail = if message.trim().is_empty() {
                "交付消息无法自动发送，请人工处理".to_string()
            } else {
                format!("交付消息无法自动发送：{message}")
            };
            transition_error(
                storage,
                run,
                DeliveryRunStatus::ManualRequired,
                &code,
                &detail,
                0,
            )
            .await?;
        }
    }
    Ok(())
}

async fn persist_gateway_error(
    storage: &StorageState,
    run: &DeliveryRun,
    error: GatewayError,
) -> Result<(), StorageError> {
    match error {
        GatewayError::Transient { code, message: _ } => {
            transition_error(
                storage,
                run,
                DeliveryRunStatus::Retrying,
                &code,
                "平台服务暂时不可用，系统将稍后重试",
                retry_at(run.attempt_count),
            )
            .await
        }
        GatewayError::Permanent { code, message: _ } => {
            transition_error(
                storage,
                run,
                DeliveryRunStatus::ManualRequired,
                &code,
                "订单无法自动发货，请人工处理",
                0,
            )
            .await
        }
    }
}

async fn transition_error(
    storage: &StorageState,
    run: &DeliveryRun,
    next: DeliveryRunStatus,
    code: &str,
    message: &str,
    next_retry_at: i64,
) -> Result<(), StorageError> {
    storage
        .transition_delivery_run(run.id, run.status, next, code, message, next_retry_at, true)
        .await?;
    Ok(())
}

fn retry_at(attempt_count: i64) -> i64 {
    let exponent = attempt_count.clamp(0, 5) as u32;
    unix_millis_now() + 30_000_i64.saturating_mul(1_i64 << exponent)
}

fn is_terminal(status: DeliveryRunStatus) -> bool {
    matches!(
        status,
        DeliveryRunStatus::Completed | DeliveryRunStatus::Cancelled
    )
}

async fn renew_lease(
    storage: &StorageState,
    run_id: i64,
    worker_instance_id: &str,
) -> Result<bool, StorageError> {
    storage
        .renew_delivery_run_lease(
            run_id,
            worker_instance_id,
            unix_millis_now(),
            RUN_LEASE_TTL_MS,
        )
        .await
}

async fn load_run(storage: &StorageState, run_id: i64) -> Result<DeliveryRun, StorageError> {
    storage
        .delivery_run(run_id)
        .await?
        .ok_or_else(|| StorageError::Conflict {
            message: format!("delivery run {run_id} does not exist"),
        })
}

fn run_lock(run_id: i64) -> Arc<AsyncMutex<()>> {
    let mut locks = RUN_LOCKS
        .lock()
        .expect("delivery run lock registry poisoned");
    if let Some(lock) = locks.get(&run_id).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(run_id, Arc::downgrade(&lock));
    lock
}
