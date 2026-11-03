use std::{
    collections::VecDeque,
    ops::Deref,
    sync::{Arc, Mutex},
};

use tempfile::TempDir;
use xianyu_rust_backend_full::{
    business_auto::delivery::{
        compose_delivery_message, execute_delivery_run,
        ports::{
            BuyerMessenger, ConsignOutcome, GatewayError, MessageDeliveryResult, OrderConfirmation,
            OrderGateway, OrderSnapshot,
        },
        WAITING_STOCK_BUYER_NOTICE,
    },
    config::AppConfig,
    models::delivery::{
        DeliveryRunStatus, NewDeliveryRun, SaveDeliveryConfigurationRequest, SideEffectClaim,
    },
    storage::StorageState,
};

#[derive(Clone)]
struct FakeOrderGateway {
    confirmations: Arc<Mutex<VecDeque<Result<OrderConfirmation, GatewayError>>>>,
    purchase_quantity: Option<i64>,
    consignments: Arc<Mutex<VecDeque<Result<ConsignOutcome, GatewayError>>>>,
    calls: Arc<Mutex<Vec<String>>>,
}

impl FakeOrderGateway {
    fn ready(consignments: Vec<Result<ConsignOutcome, GatewayError>>) -> Self {
        Self {
            confirmations: Arc::new(Mutex::new(VecDeque::from([Ok(OrderConfirmation::Ready)]))),
            purchase_quantity: Some(1),
            consignments: Arc::new(Mutex::new(consignments.into())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn cancelled() -> Self {
        Self {
            confirmations: Arc::new(Mutex::new(VecDeque::from([Ok(
                OrderConfirmation::Cancelled,
            )]))),
            purchase_quantity: Some(1),
            consignments: Arc::new(Mutex::new(VecDeque::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn already_consigned(safe_to_send: bool) -> Self {
        Self {
            confirmations: Arc::new(Mutex::new(VecDeque::from([Ok(
                OrderConfirmation::AlreadyConsigned { safe_to_send },
            )]))),
            purchase_quantity: Some(1),
            consignments: Arc::new(Mutex::new(VecDeque::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn quantity(mut self, purchase_quantity: Option<i64>) -> Self {
        self.purchase_quantity = purchase_quantity;
        self
    }
}

impl OrderGateway for FakeOrderGateway {
    async fn refresh_and_confirm_order(
        &self,
        _uid: &str,
        _order_id: &str,
    ) -> Result<OrderSnapshot, GatewayError> {
        self.calls.lock().unwrap().push("confirm".to_string());
        let confirmation = self
            .confirmations
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(OrderConfirmation::Ready))?;
        Ok(OrderSnapshot {
            confirmation,
            purchase_quantity: self.purchase_quantity,
            purchase_quantity_source: "test_fixture".to_string(),
            refreshed_at: 1,
        })
    }

    async fn consign_without_shipping(
        &self,
        _uid: &str,
        _order_id: &str,
    ) -> Result<ConsignOutcome, GatewayError> {
        self.calls.lock().unwrap().push("consign".to_string());
        self.consignments
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Ok(ConsignOutcome::Consigned))
    }
}

#[derive(Clone)]
struct FakeMessenger {
    results: Arc<Mutex<VecDeque<MessageDeliveryResult>>>,
    messages: Arc<Mutex<Vec<String>>>,
}

impl FakeMessenger {
    fn new(results: Vec<MessageDeliveryResult>) -> Self {
        Self {
            results: Arc::new(Mutex::new(results.into())),
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }
}

impl BuyerMessenger for FakeMessenger {
    async fn send_text_and_wait_ack(
        &self,
        _uid: &str,
        _buyer_id: &str,
        content: &str,
    ) -> MessageDeliveryResult {
        self.messages.lock().unwrap().push(content.to_string());
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(acked)
    }
}

fn acked() -> MessageDeliveryResult {
    MessageDeliveryResult::Acked {
        message_id: "test-message-id".to_string(),
        uuid: "test-message-uuid".to_string(),
    }
}

struct TestStorage {
    storage: StorageState,
    _temp_dir: TempDir,
}

impl Deref for TestStorage {
    type Target = StorageState;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

#[test]
fn combines_fixed_message_and_cards_without_extra_whitespace() {
    assert_eq!(
        compose_delivery_message("  使用说明  ", &[" A ".into(), "B".into()]),
        "使用说明\n\n兑换码：\n1. A\n2. B"
    );
    assert_eq!(compose_delivery_message(" 使用说明 ", &[]), "使用说明");
    assert_eq!(
        compose_delivery_message("", &[" A ".into(), "  ".into(), "B".into()]),
        "兑换码：\n1. A\n2. B"
    );
    assert_eq!(compose_delivery_message("  ", &[]), "");
}

#[tokio::test]
async fn insufficient_stock_reserves_nothing_and_never_consigns() {
    let fixture = setup_run("o-insufficient", "说明", true, Some(2), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::WaitingStock);
    assert_eq!(fixture.reserved_count().await, 0);
    assert!(!gateway.calls().iter().any(|call| call == "consign"));
    assert_eq!(messenger.messages(), vec![WAITING_STOCK_BUYER_NOTICE]);
}

#[tokio::test]
async fn two_cards_follow_reserve_consign_send_ack_consume_order() {
    let fixture = setup_run("o-complete", "使用说明", true, Some(2), &["A", "B"]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Completed);
    assert_eq!(fixture.used_count().await, 2);
    assert!(fixture.message_sent_at().await > 0);
    assert_eq!(fixture.delivery_send_claimed_at().await, 0);
    assert_eq!(gateway.calls(), vec!["confirm", "consign"]);
    assert_eq!(
        messenger.messages(),
        vec!["使用说明\n\n兑换码：\n1. A\n2. B"]
    );
    assert_eq!(fixture.alert_status().await.as_deref(), None);
}

#[tokio::test]
async fn fixed_only_sends_once_when_purchase_quantity_is_unknown() {
    let fixture = setup_run("o-fixed", "固定说明", false, None, &[]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Completed);
    assert_eq!(fixture.required_card_count().await, 0);
    assert_eq!(messenger.messages(), vec!["固定说明"]);
}

#[tokio::test]
async fn unknown_quantity_with_one_time_cards_schedules_retry() {
    let fixture = setup_run("o-unknown", "", true, None, &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]).quantity(None);
    let messenger = FakeMessenger::new(vec![]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Retrying);
    assert_eq!(
        fixture.last_error_code().await,
        "quantity_unknown_retryable"
    );
    assert!(fixture.next_retry_at().await > 0);
    assert_eq!(fixture.reserved_count().await, 0);
    assert!(!gateway.calls().iter().any(|call| call == "consign"));
}

#[tokio::test]
async fn transient_consign_failure_keeps_reserved_cards_and_records_retry() {
    let fixture = setup_run("o-consign-retry", "", true, Some(1), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Err(GatewayError::transient(
        "upstream_temporary",
        "平台服务暂时不可用",
    ))]);
    let messenger = FakeMessenger::new(vec![]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Retrying);
    assert_eq!(fixture.reserved_count().await, 1);
    assert_eq!(fixture.used_count().await, 0);
    assert!(fixture.next_retry_at().await > 0);
    assert_eq!(fixture.attempt_count().await, 1);
}

#[tokio::test]
async fn permanent_consign_failure_requires_manual_action_and_keeps_cards() {
    let fixture = setup_run("o-consign-permanent", "", true, Some(1), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Err(GatewayError::permanent(
        "consign_rejected",
        "订单无法自动发货",
    ))]);
    let messenger = FakeMessenger::new(vec![]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert_eq!(fixture.last_error_code().await, "consign_rejected");
    assert_eq!(fixture.reserved_count().await, 1);
    assert_eq!(fixture.used_count().await, 0);
    assert!(messenger.messages().is_empty());
}

#[tokio::test]
async fn upstream_manual_consign_cancels_auto_delivery_without_side_effects() {
    let fixture = setup_run("o-upstream-unsafe", "固定说明", false, None, &[]).await;
    let gateway = FakeOrderGateway::already_consigned(false);
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Cancelled);
    assert_eq!(fixture.last_error_code().await, "external_already_shipped");
    assert!(!gateway.calls().iter().any(|call| call == "consign"));
    assert!(messenger.messages().is_empty());
}

#[tokio::test]
async fn send_failure_retries_same_cards_without_second_consign() {
    let fixture = setup_run("o-send-retry", "", true, Some(1), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![
        MessageDeliveryResult::TransientFailure {
            code: "network_temporary".to_string(),
            message: "消息发送暂时失败".to_string(),
        },
        acked(),
    ]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();
    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Retrying);
    assert_eq!(fixture.reserved_contents().await, vec!["A"]);
    assert_eq!(fixture.used_count().await, 0);

    fixture.make_retry_due().await;
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Completed);
    assert_eq!(
        gateway
            .calls()
            .iter()
            .filter(|call| *call == "consign")
            .count(),
        1
    );
    assert_eq!(
        messenger.messages(),
        vec!["兑换码：\n1. A", "兑换码：\n1. A"]
    );
    assert_eq!(fixture.used_count().await, 1);
}

#[tokio::test]
async fn uncertain_send_never_blindly_resends_codes() {
    let fixture = setup_run("o-uncertain", "", true, Some(1), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![MessageDeliveryResult::Uncertain]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert_eq!(fixture.last_error_code().await, "send_result_uncertain");
    assert_eq!(messenger.messages(), vec!["兑换码：\n1. A"]);
    assert_eq!(fixture.reserved_count().await, 1);
    assert_eq!(fixture.used_count().await, 0);
}

#[tokio::test]
async fn waiting_notice_non_ack_never_sets_sent_at() {
    let transient = setup_run("o-wait-transient", "", true, Some(2), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![MessageDeliveryResult::TransientFailure {
        code: "network_temporary".to_string(),
        message: "明确未送达".to_string(),
    }]);

    execute_delivery_run(&transient.storage, &gateway, &messenger, transient.run_id)
        .await
        .unwrap();

    assert_eq!(transient.waiting_notice_sent_at().await, 0);
    assert_eq!(transient.waiting_notice_claimed_at().await, 0);

    let uncertain = setup_run("o-wait-uncertain", "", true, Some(2), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![MessageDeliveryResult::Uncertain]);
    execute_delivery_run(&uncertain.storage, &gateway, &messenger, uncertain.run_id)
        .await
        .unwrap();

    assert_eq!(uncertain.waiting_notice_sent_at().await, 0);
    assert!(uncertain.waiting_notice_claimed_at().await > 0);
    assert_eq!(
        uncertain.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert_eq!(
        uncertain.last_error_code().await,
        "waiting_notice_result_uncertain"
    );
}

#[tokio::test]
async fn interrupted_waiting_notice_claim_becomes_manual_without_resending() {
    let fixture = setup_run("o-wait-interrupted", "", true, Some(2), &["A"]).await;
    fixture.mark_waiting_stock().await;
    assert!(
        fixture
            .storage
            .claim_waiting_notice(fixture.run_id, "interrupted-worker", 1, 1)
            .await
            .unwrap()
            == SideEffectClaim::Acquired
    );
    let gateway = FakeOrderGateway::ready(vec![]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.waiting_notice_sent_at().await, 0);
    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert_eq!(
        fixture.last_error_code().await,
        "waiting_notice_result_uncertain"
    );
    assert!(messenger.messages().is_empty());
}

#[tokio::test]
async fn manual_required_run_is_not_processed_again() {
    let fixture = setup_run("o-manual-stable", "固定说明", false, None, &[]).await;
    sqlx::query(
        "UPDATE delivery_runs SET status = 'manual_required', waiting_notice_claimed_at = 123, last_error_code = 'waiting_notice_result_uncertain', attempt_count = 7 WHERE id = ?",
    )
    .bind(fixture.run_id)
    .execute(fixture.storage.accounts())
    .await
    .unwrap();
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert_eq!(fixture.attempt_count().await, 7);
    assert!(gateway.calls().is_empty());
    assert!(messenger.messages().is_empty());
}

#[tokio::test]
async fn cancelled_order_releases_all_unconsumed_reservations() {
    let fixture = setup_run("o-cancelled", "", true, Some(1), &["A"]).await;
    fixture.reserve().await;
    let gateway = FakeOrderGateway::cancelled();
    let messenger = FakeMessenger::new(vec![]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Cancelled);
    assert_eq!(fixture.reserved_count().await, 0);
    assert_eq!(fixture.used_count().await, 0);
    assert!(!gateway.calls().iter().any(|call| call == "consign"));
}

#[tokio::test]
async fn waiting_notice_is_sent_once_after_alert_is_persisted() {
    let fixture = setup_run("o-waiting", "", true, Some(2), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.alert_status().await.as_deref(), Some("open"));
    assert!(fixture.waiting_notice_sent_at().await > 0);
    assert_eq!(fixture.waiting_notice_claimed_at().await, 0);
    assert_eq!(messenger.messages(), vec![WAITING_STOCK_BUYER_NOTICE]);
}

#[tokio::test]
async fn replenished_stock_completes_and_resolves_alert_atomically() {
    let fixture = setup_run("o-replenished", "", true, Some(2), &["A"]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]).quantity(Some(2));
    let messenger = FakeMessenger::new(vec![acked(), acked()]);
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();
    assert_eq!(fixture.alert_status().await.as_deref(), Some("open"));

    fixture.add_cards(&["B"]).await;
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Completed);
    assert_eq!(fixture.used_count().await, 2);
    assert_eq!(fixture.alert_status().await.as_deref(), Some("resolved"));
}

#[tokio::test]
async fn interrupted_sending_claim_becomes_manual_without_resending() {
    let fixture = setup_run("o-send-interrupted", "固定说明", false, None, &[]).await;
    fixture.mark_sending().await;
    assert!(
        fixture
            .storage
            .claim_delivery_send(fixture.run_id, "interrupted-worker", 1, 1)
            .await
            .unwrap()
            == SideEffectClaim::Acquired
    );
    assert_eq!(fixture.message_sent_at().await, 0);
    let gateway = FakeOrderGateway::ready(vec![]);
    let messenger = FakeMessenger::new(vec![acked()]);

    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert_eq!(fixture.last_error_code().await, "send_result_uncertain");
    assert!(messenger.messages().is_empty());
}

#[tokio::test]
async fn retry_after_confirmed_send_failure_uses_sending_and_never_reconsigns() {
    let fixture = setup_run("o-safe-manual-retry", "固定说明", false, None, &[]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![
        MessageDeliveryResult::PermanentFailure {
            code: "send_permanent_failure".to_string(),
            message: "明确未送达".to_string(),
        },
        acked(),
    ]);
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();
    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
    assert!(fixture.consigned_at().await > 0);

    fixture
        .storage
        .retry_delivery_run(fixture.run_id, &fixture.uid)
        .await
        .unwrap()
        .expect("safe manual retry should be accepted");
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Completed);
    assert_eq!(
        gateway
            .calls()
            .iter()
            .filter(|call| *call == "consign")
            .count(),
        1
    );
}

#[tokio::test]
async fn generic_retry_rejects_uncertain_delivery_results() {
    let fixture = setup_run("o-retry-uncertain", "固定说明", false, None, &[]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![MessageDeliveryResult::Uncertain]);
    execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id)
        .await
        .unwrap();

    let error = fixture
        .storage
        .retry_delivery_run(fixture.run_id, &fixture.uid)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("人工确认"));
    assert_eq!(
        fixture.run_status().await,
        DeliveryRunStatus::ManualRequired
    );
}

#[tokio::test]
async fn independent_storage_connections_allow_only_one_database_claim() {
    let fixture = setup_run("o-db-cas", "固定说明", false, None, &[]).await;
    fixture.mark_waiting_stock().await;
    let second = open_storage(fixture._temp_dir.path()).await;
    let (first_notice, second_notice) = tokio::join!(
        fixture
            .storage
            .claim_waiting_notice(fixture.run_id, "worker-a", 1_000, 60_000),
        second.claim_waiting_notice(fixture.run_id, "worker-b", 1_000, 60_000)
    );
    assert_eq!(
        usize::from(first_notice.unwrap() == SideEffectClaim::Acquired)
            + usize::from(second_notice.unwrap() == SideEffectClaim::Acquired),
        1
    );

    fixture.mark_sending().await;
    let (first_send, second_send) = tokio::join!(
        fixture
            .storage
            .claim_delivery_send(fixture.run_id, "worker-a", 1_000, 60_000),
        second.claim_delivery_send(fixture.run_id, "worker-b", 1_000, 60_000)
    );
    assert_eq!(
        usize::from(first_send.unwrap() == SideEffectClaim::Acquired)
            + usize::from(second_send.unwrap() == SideEffectClaim::Acquired),
        1
    );
}

#[tokio::test]
async fn concurrent_execution_allows_each_external_side_effect_once() {
    let fixture = setup_run("o-concurrent", "固定说明", false, None, &[]).await;
    let gateway = FakeOrderGateway::ready(vec![Ok(ConsignOutcome::Consigned)]);
    let messenger = FakeMessenger::new(vec![acked()]);

    let first = execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id);
    let second = execute_delivery_run(&fixture.storage, &gateway, &messenger, fixture.run_id);
    let (first, second) = tokio::join!(first, second);
    first.unwrap();
    second.unwrap();

    assert_eq!(
        gateway
            .calls()
            .iter()
            .filter(|call| *call == "consign")
            .count(),
        1
    );
    assert_eq!(messenger.messages(), vec!["固定说明"]);
    assert_eq!(fixture.run_status().await, DeliveryRunStatus::Completed);
}

struct RunFixture {
    storage: StorageState,
    _temp_dir: TempDir,
    run_id: i64,
    uid: String,
    pool_tag: String,
    required: i64,
}

impl RunFixture {
    async fn run_status(&self) -> DeliveryRunStatus {
        self.storage
            .delivery_run(self.run_id)
            .await
            .unwrap()
            .unwrap()
            .status
    }

    async fn required_card_count(&self) -> i64 {
        self.storage
            .delivery_run(self.run_id)
            .await
            .unwrap()
            .unwrap()
            .required_card_count
    }

    async fn reserved_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE reserved_run_id = ? AND used = 0")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn used_count(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE used_order_id = (SELECT order_id FROM delivery_runs WHERE id = ?)")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn reserved_contents(&self) -> Vec<String> {
        sqlx::query_scalar(
            "SELECT content_snapshot FROM delivery_run_cards WHERE run_id = ? ORDER BY position",
        )
        .bind(self.run_id)
        .fetch_all(self.storage.accounts())
        .await
        .unwrap()
    }

    async fn alert_status(&self) -> Option<String> {
        sqlx::query_scalar("SELECT status FROM delivery_alerts WHERE run_id = ?")
            .bind(self.run_id)
            .fetch_optional(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn waiting_notice_sent_at(&self) -> i64 {
        sqlx::query_scalar("SELECT waiting_notice_sent_at FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn waiting_notice_claimed_at(&self) -> i64 {
        sqlx::query_scalar("SELECT waiting_notice_claimed_at FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn message_sent_at(&self) -> i64 {
        sqlx::query_scalar("SELECT message_sent_at FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn delivery_send_claimed_at(&self) -> i64 {
        sqlx::query_scalar("SELECT delivery_send_claimed_at FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn consigned_at(&self) -> i64 {
        sqlx::query_scalar("SELECT consigned_at FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn last_error_code(&self) -> String {
        sqlx::query_scalar("SELECT last_error_code FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn next_retry_at(&self) -> i64 {
        sqlx::query_scalar("SELECT next_retry_at FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn attempt_count(&self) -> i64 {
        sqlx::query_scalar("SELECT attempt_count FROM delivery_runs WHERE id = ?")
            .bind(self.run_id)
            .fetch_one(self.storage.accounts())
            .await
            .unwrap()
    }

    async fn reserve(&self) {
        self.storage
            .reserve_cards_for_run(
                self.run_id,
                &self.uid,
                "account",
                &self.pool_tag,
                self.required,
            )
            .await
            .unwrap();
    }

    async fn make_retry_due(&self) {
        sqlx::query("UPDATE delivery_runs SET next_retry_at = 0 WHERE id = ?")
            .bind(self.run_id)
            .execute(self.storage.accounts())
            .await
            .unwrap();
    }

    async fn add_cards(&self, cards: &[&str]) {
        let run = self
            .storage
            .delivery_run(self.run_id)
            .await
            .unwrap()
            .unwrap();
        let configuration = self
            .storage
            .delivery_configuration_record(run.rule_id)
            .await
            .unwrap()
            .unwrap();
        self.storage
            .add_delivery_cards(
                &configuration,
                &cards
                    .iter()
                    .map(|card| card.to_string())
                    .collect::<Vec<_>>(),
            )
            .await
            .unwrap();
    }

    async fn mark_sending(&self) {
        sqlx::query(
            "UPDATE delivery_runs SET status = 'sending', consigned_at = 123, next_retry_at = 0, delivery_send_claimed_at = 0 WHERE id = ?",
        )
        .bind(self.run_id)
        .execute(self.storage.accounts())
        .await
        .unwrap();
    }

    async fn mark_waiting_stock(&self) {
        sqlx::query(
            "UPDATE delivery_runs SET status = 'waiting_stock', waiting_notice_claimed_at = 0, waiting_notice_sent_at = 0 WHERE id = ?",
        )
        .bind(self.run_id)
        .execute(self.storage.accounts())
        .await
        .unwrap();
    }
}

async fn setup_run(
    order_id: &str,
    fixed_message: &str,
    one_time_enabled: bool,
    purchase_quantity: Option<i64>,
    cards: &[&str],
) -> RunFixture {
    let TestStorage { storage, _temp_dir } = setup_storage().await;
    let uid = "u1".to_string();
    let pool_tag = format!("pool-{order_id}");
    let card_contents = cards
        .iter()
        .map(|card| card.to_string())
        .collect::<Vec<_>>();
    let configuration = storage
        .create_delivery_configuration(
            &SaveDeliveryConfigurationRequest {
                uid: uid.clone(),
                item_id: "item-1".to_string(),
                pool_scope: "account".to_string(),
                pool_tag: pool_tag.clone(),
                pool_tags: vec![pool_tag.clone()],
                fixed_pool_tags: Vec::new(),
                fixed_message: fixed_message.to_string(),
                one_time_enabled,
                cards: Vec::new(),
                enabled: true,
            },
            &pool_tag,
            &[],
        )
        .await
        .unwrap();
    if !card_contents.is_empty() {
        storage
            .add_delivery_cards(&configuration, &card_contents)
            .await
            .unwrap();
    }
    let required = if one_time_enabled {
        purchase_quantity.unwrap_or(0)
    } else {
        0
    };
    let run = storage
        .ensure_delivery_run(NewDeliveryRun {
            uid: uid.clone(),
            rule_id: configuration.id,
            order_id: order_id.to_string(),
            buyer_id: "buyer-1".to_string(),
            item_id: "item-1".to_string(),
            purchase_quantity,
            purchase_quantity_source: "test_fixture".to_string(),
            required_card_count: required,
        })
        .await
        .unwrap();

    RunFixture {
        storage,
        _temp_dir,
        run_id: run.id,
        uid,
        pool_tag,
        required,
    }
}

async fn setup_storage() -> TestStorage {
    let temp_dir = TempDir::new().unwrap();
    let mut config = AppConfig::default();
    config.sqlite.data_dir = temp_dir.path().to_path_buf();
    config.sqlite.accounts_db_path = temp_dir.path().join("accounts.db");
    config.sqlite.xianyu_db_path = temp_dir.path().join("xianyu.db");
    config.sqlite.live_state_db_path = temp_dir.path().join("live_state.db");
    config.sqlite.notifications_db_path = temp_dir.path().join("notifications.db");
    config.sqlite.max_connections = 8;
    let storage = StorageState::bootstrap(&config, "2026-07-10T00:00:00Z")
        .await
        .unwrap();
    TestStorage {
        storage,
        _temp_dir: temp_dir,
    }
}

async fn open_storage(data_dir: &std::path::Path) -> StorageState {
    let mut config = AppConfig::default();
    config.sqlite.data_dir = data_dir.to_path_buf();
    config.sqlite.accounts_db_path = data_dir.join("accounts.db");
    config.sqlite.xianyu_db_path = data_dir.join("xianyu.db");
    config.sqlite.live_state_db_path = data_dir.join("live_state.db");
    config.sqlite.notifications_db_path = data_dir.join("notifications.db");
    config.sqlite.max_connections = 8;
    StorageState::bootstrap(&config, "2026-07-10T00:00:01Z")
        .await
        .unwrap()
}
