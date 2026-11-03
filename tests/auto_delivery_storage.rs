use std::{collections::HashSet, ops::Deref};

use sqlx::{Connection, Executor, Row};
use tempfile::TempDir;
use xianyu_rust_backend_full::{
    config::AppConfig,
    models::{
        business::CardCreateItem,
        delivery::{
            CardReservation, DeliveryRunStatus, NewDeliveryRun, SaveDeliveryConfigurationRequest,
            UpdateDeliveryConfigurationRequest,
        },
    },
    storage::{StorageError, StorageState},
};

const ACCOUNTS_MIGRATION_0022: &str =
    include_str!("../migrations/accounts/0022_auto_delivery_runs.sql");
const ACCOUNTS_MIGRATION_0023: &str =
    include_str!("../migrations/accounts/0023_delivery_run_required_card_count.sql");
const ACCOUNTS_MIGRATION_0025: &str =
    include_str!("../migrations/accounts/0025_delivery_run_send_claims.sql");

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
fn delivery_run_status_serde_uses_exact_snake_case_values() {
    let cases = [
        (DeliveryRunStatus::Pending, "pending"),
        (DeliveryRunStatus::WaitingStock, "waiting_stock"),
        (DeliveryRunStatus::Reserved, "reserved"),
        (DeliveryRunStatus::Consigning, "consigning"),
        (DeliveryRunStatus::Sending, "sending"),
        (DeliveryRunStatus::Completed, "completed"),
        (DeliveryRunStatus::Retrying, "retrying"),
        (DeliveryRunStatus::ManualRequired, "manual_required"),
        (DeliveryRunStatus::Cancelled, "cancelled"),
    ];

    for (status, expected) in cases {
        let serialized = serde_json::to_string(&status).unwrap();
        assert_eq!(serialized, format!(r#""{expected}""#));
        assert_eq!(
            serde_json::from_str::<DeliveryRunStatus>(&serialized).unwrap(),
            status
        );
    }

    assert!(serde_json::from_str::<DeliveryRunStatus>(r#""unknown_status""#).is_err());
}

#[tokio::test]
async fn creates_one_run_per_account_order() {
    let storage = setup_storage().await;

    let first = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();
    let second = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(first.required_card_count, 2);
    assert_eq!(second.required_card_count, 2);
}

#[tokio::test]
async fn same_order_id_creates_distinct_runs_for_different_accounts() {
    let storage = setup_storage().await;

    let first = storage
        .ensure_delivery_run(new_run("u1", "shared-order", Some(1), 1))
        .await
        .unwrap();
    let second = storage
        .ensure_delivery_run(new_run("u2", "shared-order", Some(1), 1))
        .await
        .unwrap();

    assert_ne!(first.id, second.id);
    let runs: Vec<(String, String)> = sqlx::query_as(
        "SELECT uid, order_id FROM delivery_runs WHERE order_id = ? ORDER BY uid ASC",
    )
    .bind("shared-order")
    .fetch_all(storage.accounts())
    .await
    .unwrap();
    assert_eq!(
        runs,
        vec![
            ("u1".to_string(), "shared-order".to_string()),
            ("u2".to_string(), "shared-order".to_string()),
        ]
    );
}

#[tokio::test]
async fn delivery_run_summary_pagination_caps_page_size_and_orders_newest_first() {
    let storage = setup_storage().await;
    let mut ids = Vec::new();
    for index in 0..205 {
        let run = storage
            .ensure_delivery_run(new_run("u-page", &format!("order-{index:03}"), Some(1), 1))
            .await
            .unwrap();
        sqlx::query("UPDATE delivery_runs SET updated_at = ? WHERE id = ?")
            .bind(index as i64)
            .bind(run.id)
            .execute(storage.accounts())
            .await
            .unwrap();
        ids.push(run.id);
    }
    let uids = vec!["u-page".to_string()];
    let first = storage
        .list_delivery_runs_summary(&uids, None, None, 1, 999)
        .await
        .unwrap();
    let second = storage
        .list_delivery_runs_summary(&uids, None, None, 2, 999)
        .await
        .unwrap();
    assert_eq!(first.len(), 200);
    assert_eq!(second.len(), 5);
    assert_eq!(first[0].id, *ids.last().unwrap());
    assert!(first
        .windows(2)
        .all(|pair| pair[0].updated_at >= pair[1].updated_at));

    for invalid_page in [0, -1, i64::MAX] {
        let error = storage
            .list_delivery_runs_summary(&uids, None, None, invalid_page, 50)
            .await
            .unwrap_err();
        assert!(matches!(error, StorageError::Conflict { .. }));
    }
    let empty_scope_error = storage
        .list_delivery_runs_summary(&[], None, None, i64::MAX, 50)
        .await
        .unwrap_err();
    assert!(matches!(empty_scope_error, StorageError::Conflict { .. }));
    storage
        .list_delivery_runs_summary(&uids, None, None, 10_000, 200)
        .await
        .unwrap();

    let plan = sqlx::query(
        r#"
        EXPLAIN QUERY PLAN
        SELECT id, uid, rule_id, updated_at
        FROM delivery_runs INDEXED BY idx_delivery_runs_updated_id_uid
        WHERE uid IN (?, ?)
        ORDER BY updated_at DESC, id DESC
        LIMIT 50 OFFSET 0
        "#,
    )
    .bind("u-page")
    .bind("u-other")
    .fetch_all(storage.accounts())
    .await
    .unwrap();
    let details = plan
        .iter()
        .map(|row| row.get::<String, _>("detail"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        details.contains("idx_delivery_runs_updated_id_uid"),
        "默认排序未使用 order-leading 索引: {details}"
    );
    assert!(
        !details.contains("USE TEMP B-TREE FOR ORDER BY"),
        "默认排序仍使用临时 B-Tree: {details}"
    );
}

#[tokio::test]
async fn insufficient_cards_reserves_nothing() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();

    let result = storage
        .reserve_cards_for_run(run.id, "u1", "account", "pool", 2)
        .await
        .unwrap();

    assert!(matches!(
        result,
        CardReservation::Insufficient {
            available: 1,
            required: 2
        }
    ));
    assert_eq!(reserved_card_count(&storage, run.id).await, 0);
}

#[tokio::test]
async fn reserves_exact_quantity_with_distinct_contents_without_consuming() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B", "C"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();

    let cards = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 2)
            .await
            .unwrap(),
    );

    assert_eq!(cards.len(), 2);
    assert_eq!(
        cards
            .iter()
            .map(|card| card.content_snapshot.as_str())
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    let state: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(used), 0) FROM cards WHERE reserved_run_id = ?",
    )
    .bind(run.id)
    .fetch_one(storage.accounts())
    .await
    .unwrap();
    assert_eq!(state, (2, 0));
}

#[tokio::test]
async fn repeated_reservation_for_same_run_reuses_mapping() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B", "C"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();

    let first = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 2)
            .await
            .unwrap(),
    );
    let second = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 2)
            .await
            .unwrap(),
    );

    assert_eq!(card_ids(&first), card_ids(&second));
    assert_eq!(mapped_card_count(&storage, run.id).await, 2);
}

#[tokio::test]
async fn reservation_rejects_quantity_different_from_run_snapshot() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 2))
        .await
        .unwrap();

    let error = storage
        .reserve_cards_for_run(run.id, "u1", "account", "pool", 1)
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::Conflict { .. }));
    assert_eq!(reserved_card_count(&storage, run.id).await, 0);
    assert_eq!(mapped_card_count(&storage, run.id).await, 0);
}

#[tokio::test]
async fn concurrent_runs_never_share_a_card() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B"]).await;
    let first_run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 1))
        .await
        .unwrap();
    let second_run = storage
        .ensure_delivery_run(new_run("u1", "o2", Some(1), 1))
        .await
        .unwrap();
    let first_storage = storage.storage.clone();
    let second_storage = storage.storage.clone();

    let (first, second) = tokio::join!(
        first_storage.reserve_cards_for_run(first_run.id, "u1", "account", "pool", 1),
        second_storage.reserve_cards_for_run(second_run.id, "u1", "account", "pool", 1)
    );
    let first = reserved_cards(first.unwrap());
    let second = reserved_cards(second.unwrap());

    assert_ne!(first[0].card_id, second[0].card_id);
}

#[tokio::test]
async fn reservation_excludes_reusable_used_and_other_run_reserved_cards() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["reserved-by-other"]).await;
    let other_run = storage
        .ensure_delivery_run(new_run("u1", "other-order", Some(1), 1))
        .await
        .unwrap();
    let other_cards = reserved_cards(
        storage
            .reserve_cards_for_run(other_run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );
    assert_eq!(other_cards[0].content_snapshot, "reserved-by-other");

    storage
        .bulk_create_cards(
            "u1",
            &[
                CardCreateItem {
                    card_key: "reusable".to_string(),
                    card_content: "reusable".to_string(),
                    card_pool_tag: "pool".to_string(),
                    reusable: true,
                },
                CardCreateItem {
                    card_key: "used".to_string(),
                    card_content: "used".to_string(),
                    card_pool_tag: "pool".to_string(),
                    reusable: false,
                },
                CardCreateItem {
                    card_key: "eligible".to_string(),
                    card_content: "eligible".to_string(),
                    card_pool_tag: "pool".to_string(),
                    reusable: false,
                },
            ],
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE cards SET used = 1, used_order_id = 'previous-order' WHERE cookie_id = 'u1' AND card_content = 'used'",
    )
    .execute(storage.accounts())
    .await
    .unwrap();

    let target_run = storage
        .ensure_delivery_run(new_run("u1", "target-order", Some(1), 1))
        .await
        .unwrap();
    let selected = reserved_cards(
        storage
            .reserve_cards_for_run(target_run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );

    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].content_snapshot, "eligible");
    let states: Vec<(String, i64, i64, Option<i64>)> = sqlx::query_as(
        r#"
        SELECT card_content, reusable, used, reserved_run_id
        FROM cards
        WHERE cookie_id = 'u1' AND card_pool_tag = 'pool'
        ORDER BY id ASC
        "#,
    )
    .fetch_all(storage.accounts())
    .await
    .unwrap();
    assert_eq!(
        states,
        vec![
            ("reserved-by-other".to_string(), 0, 0, Some(other_run.id)),
            ("reusable".to_string(), 1, 0, None),
            ("used".to_string(), 0, 1, None),
            ("eligible".to_string(), 0, 0, Some(target_run.id)),
        ]
    );
}

#[tokio::test]
async fn consume_marks_reserved_cards_used_for_order() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();
    storage
        .reserve_cards_for_run(run.id, "u1", "account", "pool", 2)
        .await
        .unwrap();

    let consumed = storage.consume_reserved_cards(run.id, "o1").await.unwrap();

    assert_eq!(consumed.len(), 2);
    assert!(consumed.iter().all(|card| card.status == "consumed"));
    let consumed_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM cards
        WHERE used = 1
          AND used_order_id = 'o1'
          AND reserved_run_id IS NULL
        "#,
    )
    .fetch_one(storage.accounts())
    .await
    .unwrap();
    assert_eq!(consumed_count, 2);
}

#[tokio::test]
async fn consume_rejects_incomplete_mapping_for_required_card_count() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 2))
        .await
        .unwrap();
    let reserved = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 2)
            .await
            .unwrap(),
    );
    sqlx::query("DELETE FROM delivery_run_cards WHERE run_id = ? AND card_id = ?")
        .bind(run.id)
        .bind(reserved[1].card_id)
        .execute(storage.accounts())
        .await
        .unwrap();

    let error = storage
        .consume_reserved_cards(run.id, "o1")
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::Conflict { .. }));
    let card_state: (i64, Option<i64>) =
        sqlx::query_as("SELECT used, reserved_run_id FROM cards WHERE id = ?")
            .bind(reserved[0].card_id)
            .fetch_one(storage.accounts())
            .await
            .unwrap();
    assert_eq!(card_state, (0, Some(run.id)));
}

#[tokio::test]
async fn zero_card_run_allows_idempotent_empty_consumption() {
    let storage = setup_storage().await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "fixed-message-order", Some(3), 0))
        .await
        .unwrap();
    let reserved = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 0)
            .await
            .unwrap(),
    );
    assert!(reserved.is_empty());

    let first = storage
        .consume_reserved_cards(run.id, "fixed-message-order")
        .await
        .unwrap();
    let second = storage
        .consume_reserved_cards(run.id, "fixed-message-order")
        .await
        .unwrap();

    assert!(first.is_empty());
    assert!(second.is_empty());
}

#[tokio::test]
async fn release_makes_unused_cards_available_to_another_run() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A"]).await;
    let first_run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 1))
        .await
        .unwrap();
    let first_cards = reserved_cards(
        storage
            .reserve_cards_for_run(first_run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );

    assert_eq!(
        storage.release_reserved_cards(first_run.id).await.unwrap(),
        1
    );
    assert_eq!(mapped_card_count(&storage, first_run.id).await, 0);

    let second_run = storage
        .ensure_delivery_run(new_run("u1", "o2", Some(1), 1))
        .await
        .unwrap();
    let second_cards = reserved_cards(
        storage
            .reserve_cards_for_run(second_run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );
    assert_eq!(first_cards[0].card_id, second_cards[0].card_id);
}

#[tokio::test]
async fn released_run_cannot_consume_card_reassigned_to_another_run() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A"]).await;
    let first_run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 1))
        .await
        .unwrap();
    let first_cards = reserved_cards(
        storage
            .reserve_cards_for_run(first_run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );
    storage.release_reserved_cards(first_run.id).await.unwrap();

    let second_run = storage
        .ensure_delivery_run(new_run("u1", "o2", Some(1), 1))
        .await
        .unwrap();
    let second_cards = reserved_cards(
        storage
            .reserve_cards_for_run(second_run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );
    assert_eq!(first_cards[0].card_id, second_cards[0].card_id);

    let error = storage
        .consume_reserved_cards(first_run.id, "o1")
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::Conflict { .. }));
    let reassigned_state: (i64, Option<i64>, String) =
        sqlx::query_as("SELECT used, reserved_run_id, reserved_order_id FROM cards WHERE id = ?")
            .bind(second_cards[0].card_id)
            .fetch_one(storage.accounts())
            .await
            .unwrap();
    assert_eq!(reassigned_state, (0, Some(second_run.id), "o2".to_string()));

    let consumed = storage
        .consume_reserved_cards(second_run.id, "o2")
        .await
        .unwrap();
    assert_eq!(card_ids(&consumed), card_ids(&second_cards));
}

#[tokio::test]
async fn null_purchase_quantity_cannot_bypass_released_card_validation() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B"]).await;
    let first_run = storage
        .ensure_delivery_run(new_run("u1", "o1", None, 2))
        .await
        .unwrap();
    assert_eq!(first_run.purchase_quantity, None);
    assert_eq!(first_run.required_card_count, 2);
    let first_cards = reserved_cards(
        storage
            .reserve_cards_for_run(first_run.id, "u1", "account", "pool", 2)
            .await
            .unwrap(),
    );
    storage.release_reserved_cards(first_run.id).await.unwrap();

    let second_run = storage
        .ensure_delivery_run(new_run("u1", "o2", Some(2), 2))
        .await
        .unwrap();
    let second_cards = reserved_cards(
        storage
            .reserve_cards_for_run(second_run.id, "u1", "account", "pool", 2)
            .await
            .unwrap(),
    );
    assert_eq!(card_ids(&first_cards), card_ids(&second_cards));

    let error = storage
        .consume_reserved_cards(first_run.id, "o1")
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::Conflict { .. }));
    assert_eq!(reserved_card_count(&storage, second_run.id).await, 2);
    let consumed = storage
        .consume_reserved_cards(second_run.id, "o2")
        .await
        .unwrap();
    assert_eq!(card_ids(&consumed), card_ids(&second_cards));
}

#[tokio::test]
async fn zero_card_run_rejects_non_empty_mapping_during_consume() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "fixed-message-order", Some(3), 0))
        .await
        .unwrap();
    let reserved = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 0)
            .await
            .unwrap(),
    );
    assert!(reserved.is_empty());

    let now: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(created_at), 0) + 1 FROM delivery_run_cards")
            .fetch_one(storage.accounts())
            .await
            .unwrap();
    let card_id = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM cards WHERE cookie_id = 'u1' AND card_pool_tag = 'pool'",
    )
    .fetch_one(storage.accounts())
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO delivery_run_cards (
            run_id, card_id, position, content_snapshot, status, created_at, updated_at
        )
        VALUES (?, ?, 0, 'A', 'reserved', ?, ?)
        "#,
    )
    .bind(run.id)
    .bind(card_id)
    .bind(now)
    .bind(now)
    .execute(storage.accounts())
    .await
    .unwrap();

    let error = storage
        .consume_reserved_cards(run.id, "fixed-message-order")
        .await
        .unwrap_err();

    assert!(matches!(error, StorageError::Conflict { .. }));
}

#[tokio::test]
async fn release_does_not_free_consumed_cards() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 1))
        .await
        .unwrap();
    storage
        .reserve_cards_for_run(run.id, "u1", "account", "pool", 1)
        .await
        .unwrap();
    storage.consume_reserved_cards(run.id, "o1").await.unwrap();

    assert_eq!(storage.release_reserved_cards(run.id).await.unwrap(), 0);
    let used: i64 = sqlx::query_scalar("SELECT used FROM cards WHERE used_order_id = 'o1'")
        .fetch_one(storage.accounts())
        .await
        .unwrap();
    assert_eq!(used, 1);
}

#[tokio::test]
async fn reserved_card_cannot_be_deleted() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 1))
        .await
        .unwrap();
    let cards = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );

    let error = storage.delete_card(cards[0].card_id).await.unwrap_err();

    assert!(matches!(&error, StorageError::Conflict { .. }));
    assert!(error.to_string().contains("reserved"));
    assert!(storage.get_card(cards[0].card_id).await.unwrap().is_some());
}

#[tokio::test]
async fn legacy_claim_skips_cards_reserved_by_delivery_run() {
    let storage = setup_storage().await;
    seed_one_time_cards(&storage, "u1", "pool", &["A", "B"]).await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(1), 1))
        .await
        .unwrap();
    let reserved = reserved_cards(
        storage
            .reserve_cards_for_run(run.id, "u1", "account", "pool", 1)
            .await
            .unwrap(),
    );

    let claimed = storage
        .claim_next_card_by_pool_scoped("u1", "account", "pool", "legacy-order")
        .await
        .unwrap()
        .expect("unreserved card should remain compatible with legacy claim");

    assert_ne!(claimed.id, reserved[0].card_id);
    assert!(claimed.used);
}

#[tokio::test]
async fn waiting_stock_alert_upserts_once_and_resolves() {
    let storage = setup_storage().await;
    let run = storage
        .ensure_delivery_run(new_run("u1", "o1", Some(2), 2))
        .await
        .unwrap();

    let first = storage
        .upsert_waiting_stock_alert(&run, "库存不足", "还差 1 张")
        .await
        .unwrap();
    let second = storage
        .upsert_waiting_stock_alert(&run, "仍然不足", "还差 2 张")
        .await
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(second.title, "仍然不足");
    assert_eq!(second.body, "还差 2 张");
    assert_eq!(second.status, "open");
    let alert_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM delivery_alerts WHERE run_id = ?")
            .bind(run.id)
            .fetch_one(storage.accounts())
            .await
            .unwrap();
    assert_eq!(alert_count, 1);

    storage.resolve_delivery_alert(run.id).await.unwrap();

    let resolved: (String, i64) =
        sqlx::query_as("SELECT status, resolved_at FROM delivery_alerts WHERE run_id = ?")
            .bind(run.id)
            .fetch_one(storage.accounts())
            .await
            .unwrap();
    assert_eq!(resolved.0, "resolved");
    assert!(resolved.1 > 0);
}

#[tokio::test]
async fn quantity_columns_keep_unknown_values_null() {
    let storage = setup_storage().await;
    sqlx::query("INSERT INTO account_items (uid, item_id) VALUES ('u1', 'i1')")
        .execute(storage.xianyu())
        .await
        .unwrap();
    sqlx::query("INSERT INTO account_orders (uid, tid) VALUES ('u1', 'o1')")
        .execute(storage.xianyu())
        .await
        .unwrap();

    let item_quantity: (Option<i64>, Option<String>, Option<i64>) = sqlx::query_as(
        "SELECT quantity, quantity_source, quantity_synced_at FROM account_items WHERE uid = 'u1' AND item_id = 'i1'",
    )
    .fetch_one(storage.xianyu())
    .await
    .unwrap();
    let order_quantity: (Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT purchase_quantity, purchase_quantity_source FROM account_orders WHERE uid = 'u1' AND tid = 'o1'",
    )
    .fetch_one(storage.xianyu())
    .await
    .unwrap();

    assert_eq!(item_quantity, (None, None, None));
    assert_eq!(order_quantity, (None, None));
}

#[tokio::test]
async fn migrated_delivery_rule_fields_are_not_null_with_expected_defaults() {
    let storage = setup_storage().await;

    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(delivery_rules)")
            .fetch_all(storage.accounts())
            .await
            .unwrap();
    for (name, expected_default) in [
        ("fixed_message", "''"),
        ("one_time_enabled", "1"),
        ("needs_repair", "0"),
    ] {
        let column = columns
            .iter()
            .find(|column| column.1 == name)
            .unwrap_or_else(|| panic!("missing migrated delivery_rules.{name} column"));
        assert_eq!(column.3, 1, "delivery_rules.{name} must be NOT NULL");
        assert_eq!(
            column.4.as_deref(),
            Some(expected_default),
            "delivery_rules.{name} has the wrong default"
        );
    }

    let rule_id = sqlx::query(
        r#"
        INSERT INTO delivery_rules (
            cookie_id, keyword, item_id, card_pool_tag, card_pool_scope, enabled, created_at
        )
        VALUES ('u-default', 'keyword', 'item-default', 'pool', 'account', 1, 1)
        "#,
    )
    .execute(storage.accounts())
    .await
    .unwrap()
    .last_insert_rowid();
    let defaults: (String, i64, i64) = sqlx::query_as(
        "SELECT fixed_message, one_time_enabled, needs_repair FROM delivery_rules WHERE id = ?",
    )
    .bind(rule_id)
    .fetch_one(storage.accounts())
    .await
    .unwrap();
    assert_eq!(defaults, (String::new(), 1, 0));
}

#[tokio::test]
async fn fresh_migrations_add_delivery_run_required_card_count_with_default_zero() {
    let storage = setup_storage().await;

    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(delivery_runs)")
            .fetch_all(storage.accounts())
            .await
            .unwrap();
    let column = columns
        .iter()
        .find(|column| column.1 == "required_card_count")
        .expect("missing migrated delivery_runs.required_card_count column");
    assert_eq!(
        column.3, 1,
        "delivery_runs.required_card_count must be NOT NULL"
    );
    assert_eq!(
        column.4.as_deref(),
        Some("0"),
        "delivery_runs.required_card_count has the wrong default"
    );

    let run_id = sqlx::query(
        r#"
        INSERT INTO delivery_runs (uid, rule_id, order_id, buyer_id, item_id, created_at, updated_at)
        VALUES ('u1', 1, 'fresh-order', 'buyer-1', 'item-1', 1, 1)
        "#,
    )
    .execute(storage.accounts())
    .await
    .unwrap()
    .last_insert_rowid();
    let required_card_count: i64 =
        sqlx::query_scalar("SELECT required_card_count FROM delivery_runs WHERE id = ?")
            .bind(run_id)
            .fetch_one(storage.accounts())
            .await
            .unwrap();
    assert_eq!(required_card_count, 0);
}

#[tokio::test]
async fn delivery_run_required_card_count_upgrade_migration_preserves_old_0022_and_adds_default_zero(
) {
    let mut conn = sqlx::SqliteConnection::connect(":memory:").await.unwrap();
    conn.execute("CREATE TABLE delivery_rules (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .unwrap();
    conn.execute("CREATE TABLE cards (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .unwrap();

    sqlx::raw_sql(ACCOUNTS_MIGRATION_0022)
        .execute(&mut conn)
        .await
        .unwrap();
    let before_columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(delivery_runs)")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert!(before_columns
        .iter()
        .all(|column| column.1 != "required_card_count"));

    conn.execute(
        r#"
        INSERT INTO delivery_runs (
            uid, rule_id, order_id, buyer_id, item_id, purchase_quantity, status,
            attempt_count, waiting_notice_sent_at, admin_notified_at, consigned_at,
            message_sent_at, completed_at, next_retry_at, last_error_code, last_error_message,
            created_at, updated_at
        )
        VALUES (
            'legacy-user', 7, 'legacy-order', 'legacy-buyer', 'legacy-item', 3, 'retrying',
            4, 11, 12, 13, 14, 15, 16, 'legacy-code', 'legacy-message', 17, 18
        )
        "#,
    )
    .await
    .unwrap();

    sqlx::raw_sql(ACCOUNTS_MIGRATION_0023)
        .execute(&mut conn)
        .await
        .unwrap();
    let after_columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(delivery_runs)")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    let column = after_columns
        .iter()
        .find(|column| column.1 == "required_card_count")
        .expect("0023 must add delivery_runs.required_card_count");
    assert_eq!(
        column.3, 1,
        "delivery_runs.required_card_count must be NOT NULL after 0023"
    );
    assert_eq!(column.4.as_deref(), Some("0"));

    let preserved_row = sqlx::query(
        r#"
        SELECT
            uid, rule_id, order_id, buyer_id, item_id, purchase_quantity, status,
            attempt_count, waiting_notice_sent_at, admin_notified_at, consigned_at,
            message_sent_at, completed_at, next_retry_at, last_error_code, last_error_message,
            created_at, updated_at, required_card_count
        FROM delivery_runs
        WHERE uid = 'legacy-user' AND order_id = 'legacy-order'
        "#,
    )
    .fetch_one(&mut conn)
    .await
    .unwrap();
    assert_eq!(preserved_row.get::<String, _>(0), "legacy-user");
    assert_eq!(preserved_row.get::<i64, _>(1), 7);
    assert_eq!(preserved_row.get::<String, _>(2), "legacy-order");
    assert_eq!(preserved_row.get::<String, _>(3), "legacy-buyer");
    assert_eq!(preserved_row.get::<String, _>(4), "legacy-item");
    assert_eq!(preserved_row.get::<Option<i64>, _>(5), Some(3));
    assert_eq!(preserved_row.get::<String, _>(6), "retrying");
    assert_eq!(preserved_row.get::<i64, _>(7), 4);
    assert_eq!(preserved_row.get::<i64, _>(8), 11);
    assert_eq!(preserved_row.get::<i64, _>(9), 12);
    assert_eq!(preserved_row.get::<i64, _>(10), 13);
    assert_eq!(preserved_row.get::<i64, _>(11), 14);
    assert_eq!(preserved_row.get::<i64, _>(12), 15);
    assert_eq!(preserved_row.get::<i64, _>(13), 16);
    assert_eq!(preserved_row.get::<String, _>(14), "legacy-code");
    assert_eq!(preserved_row.get::<String, _>(15), "legacy-message");
    assert_eq!(preserved_row.get::<i64, _>(16), 17);
    assert_eq!(preserved_row.get::<i64, _>(17), 18);
    assert_eq!(preserved_row.get::<i64, _>(18), 0);
}

#[tokio::test]
async fn delivery_send_claim_migration_adds_independent_not_null_claim_columns() {
    let mut conn = sqlx::SqliteConnection::connect(":memory:").await.unwrap();
    conn.execute("CREATE TABLE delivery_rules (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .unwrap();
    conn.execute("CREATE TABLE cards (id INTEGER PRIMARY KEY AUTOINCREMENT)")
        .await
        .unwrap();
    sqlx::raw_sql(ACCOUNTS_MIGRATION_0022)
        .execute(&mut conn)
        .await
        .unwrap();
    sqlx::raw_sql(ACCOUNTS_MIGRATION_0023)
        .execute(&mut conn)
        .await
        .unwrap();

    sqlx::raw_sql(ACCOUNTS_MIGRATION_0025)
        .execute(&mut conn)
        .await
        .unwrap();

    let columns: Vec<(i64, String, String, i64, Option<String>, i64)> =
        sqlx::query_as("PRAGMA table_info(delivery_runs)")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    for name in ["waiting_notice_claimed_at", "delivery_send_claimed_at"] {
        let column = columns
            .iter()
            .find(|column| column.1 == name)
            .unwrap_or_else(|| panic!("missing delivery_runs.{name}"));
        assert_eq!(column.3, 1, "delivery_runs.{name} must be NOT NULL");
        assert_eq!(column.4.as_deref(), Some("0"));
    }
}

#[tokio::test]
async fn update_configuration_rejects_empty_final_content_inside_transaction() {
    let storage = setup_storage().await;
    let created = storage
        .create_delivery_configuration(
            &SaveDeliveryConfigurationRequest {
                uid: "u-final".to_string(),
                item_id: "item-final".to_string(),
                pool_scope: "account".to_string(),
                pool_tag: "pool-final".to_string(),
                pool_tags: vec!["pool-final".to_string()],
                fixed_pool_tags: Vec::new(),
                fixed_message: "固定说明".to_string(),
                one_time_enabled: true,
                cards: vec![],
                enabled: true,
            },
            "pool-final",
            &["ONLY-CARD".to_string()],
        )
        .await
        .unwrap();
    sqlx::query(
        "UPDATE cards SET used = 1 WHERE cookie_id = 'u-final' AND card_pool_tag = 'pool-final'",
    )
    .execute(storage.accounts())
    .await
    .unwrap();

    let result = storage
        .update_delivery_configuration(
            created.id,
            &UpdateDeliveryConfigurationRequest {
                fixed_message: Some(String::new()),
                ..Default::default()
            },
            &[],
        )
        .await;
    assert!(matches!(result, Err(StorageError::Conflict { .. })));
    let after = storage
        .delivery_configuration_record(created.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.fixed_message, "固定说明");
}

#[tokio::test]
async fn update_configuration_can_add_stock_and_clear_fixed_message_atomically() {
    let storage = setup_storage().await;
    let created = storage
        .create_delivery_configuration(
            &SaveDeliveryConfigurationRequest {
                uid: "u-supplement".to_string(),
                item_id: "item-supplement".to_string(),
                pool_scope: "account".to_string(),
                pool_tag: "pool-supplement".to_string(),
                pool_tags: vec!["pool-supplement".to_string()],
                fixed_pool_tags: Vec::new(),
                fixed_message: "固定说明".to_string(),
                one_time_enabled: true,
                cards: vec![],
                enabled: true,
            },
            "pool-supplement",
            &[],
        )
        .await
        .unwrap();
    let updated = storage
        .update_delivery_configuration(
            created.id,
            &UpdateDeliveryConfigurationRequest {
                fixed_message: Some(String::new()),
                one_time_enabled: Some(true),
                ..Default::default()
            },
            &["NEW-CARD".to_string()],
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.fixed_message, "");
    assert_eq!(
        storage
            .available_delivery_card_count(&updated)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn creates_delivery_runs_from_ready_orders_only() {
    let storage = setup_storage().await;
    let configuration = storage
        .create_delivery_configuration(
            &SaveDeliveryConfigurationRequest {
                uid: "u-auto".to_string(),
                item_id: "item-auto".to_string(),
                pool_scope: "account".to_string(),
                pool_tag: "pool-auto".to_string(),
                pool_tags: vec!["pool-auto".to_string()],
                fixed_pool_tags: Vec::new(),
                fixed_message: String::new(),
                one_time_enabled: true,
                cards: vec![],
                enabled: true,
            },
            "pool-auto",
            &["A".to_string(), "B".to_string()],
        )
        .await
        .unwrap();
    seed_order(
        &storage,
        "u-auto",
        "order-ready",
        "item-auto",
        "buyer-1",
        Some(2),
        "WAIT_SELLER_SEND_GOODS",
        "待发货",
    )
    .await;
    seed_order(
        &storage,
        "u-auto",
        "order-consigned",
        "item-auto",
        "buyer-2",
        Some(1),
        "SELLER_CONSIGNED",
        "已发货",
    )
    .await;

    assert_eq!(
        storage
            .create_delivery_runs_for_ready_orders("u-auto")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        storage
            .create_delivery_runs_for_ready_orders("u-auto")
            .await
            .unwrap(),
        0
    );

    let run = sqlx::query(
        "SELECT rule_id, order_id, buyer_id, purchase_quantity, required_card_count FROM delivery_runs WHERE uid = ?",
    )
    .bind("u-auto")
    .fetch_one(storage.accounts())
    .await
    .unwrap();
    assert_eq!(run.get::<i64, _>("rule_id"), configuration.id);
    assert_eq!(run.get::<String, _>("order_id"), "order-ready");
    assert_eq!(run.get::<String, _>("buyer_id"), "buyer-1");
    assert_eq!(run.get::<Option<i64>, _>("purchase_quantity"), Some(2));
    assert_eq!(run.get::<i64, _>("required_card_count"), 2);
}

#[tokio::test]
async fn due_delivery_run_listing_excludes_future_manual_and_terminal_states() {
    let storage = setup_storage().await;
    let pending = storage
        .ensure_delivery_run(new_run("u-due", "pending", Some(1), 1))
        .await
        .unwrap();
    let future = storage
        .ensure_delivery_run(new_run("u-due", "future", Some(1), 1))
        .await
        .unwrap();
    let manual = storage
        .ensure_delivery_run(new_run("u-due", "manual", Some(1), 1))
        .await
        .unwrap();
    sqlx::query("UPDATE delivery_runs SET status = 'retrying', next_retry_at = ? WHERE id = ?")
        .bind(9_999_999_999_999_i64)
        .bind(future.id)
        .execute(storage.accounts())
        .await
        .unwrap();
    sqlx::query("UPDATE delivery_runs SET status = 'manual_required' WHERE id = ?")
        .bind(manual.id)
        .execute(storage.accounts())
        .await
        .unwrap();

    assert_eq!(
        storage
            .list_due_delivery_run_ids("u-due", 100)
            .await
            .unwrap(),
        vec![pending.id]
    );
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
    sqlx::query(
        r#"
        INSERT INTO delivery_rules (
            id, cookie_id, keyword, item_id, card_pool_tag, card_pool_scope,
            fixed_message, one_time_enabled, enabled, needs_repair, created_at
        ) VALUES (1, 'u1', '', 'item-1', 'pool', 'account', '', 1, 1, 0, 1)
        "#,
    )
    .execute(storage.accounts())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO delivery_rule_card_pools (rule_id, pool_tag, position, created_at) VALUES (1, 'pool', 0, 1)",
    )
    .execute(storage.accounts())
    .await
    .unwrap();

    TestStorage {
        storage,
        _temp_dir: temp_dir,
    }
}

fn new_run(
    uid: &str,
    order_id: &str,
    purchase_quantity: Option<i64>,
    required_card_count: i64,
) -> NewDeliveryRun {
    NewDeliveryRun {
        uid: uid.to_string(),
        rule_id: 1,
        order_id: order_id.to_string(),
        buyer_id: "buyer-1".to_string(),
        item_id: "item-1".to_string(),
        purchase_quantity,
        purchase_quantity_source: "test_fixture".to_string(),
        required_card_count,
    }
}

fn reserved_cards(
    reservation: CardReservation,
) -> Vec<xianyu_rust_backend_full::models::delivery::DeliveryRunCard> {
    match reservation {
        CardReservation::Reserved { cards } => cards,
        CardReservation::Insufficient {
            available,
            required,
        } => panic!("expected reservation, only {available} of {required} cards available"),
    }
}

fn card_ids(cards: &[xianyu_rust_backend_full::models::delivery::DeliveryRunCard]) -> Vec<i64> {
    cards.iter().map(|card| card.card_id).collect()
}

async fn seed_one_time_cards(storage: &StorageState, uid: &str, pool_tag: &str, contents: &[&str]) {
    let cards = contents
        .iter()
        .map(|content| CardCreateItem {
            card_key: (*content).to_string(),
            card_content: (*content).to_string(),
            card_pool_tag: pool_tag.to_string(),
            reusable: false,
        })
        .collect::<Vec<_>>();

    storage.bulk_create_cards(uid, &cards).await.unwrap();
}

async fn seed_order(
    storage: &StorageState,
    uid: &str,
    order_id: &str,
    item_id: &str,
    buyer_id: &str,
    purchase_quantity: Option<i64>,
    status: &str,
    status_text: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO account_orders (
            tid, uid, item_id, title, price, pic_url, status, status_text,
            buyer_id, buyer_nick, buyer_avatar, gmt_create,
            purchase_quantity, purchase_quantity_source, created_at, updated_at
        )
        VALUES (?, ?, ?, '商品', '1.00', '', ?, ?, ?, '买家', '', '', ?, 'white_list', 1, 1)
        "#,
    )
    .bind(order_id)
    .bind(uid)
    .bind(item_id)
    .bind(status)
    .bind(status_text)
    .bind(buyer_id)
    .bind(purchase_quantity)
    .execute(storage.xianyu())
    .await
    .unwrap();
}

async fn reserved_card_count(storage: &StorageState, run_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM cards WHERE reserved_run_id = ?")
        .bind(run_id)
        .fetch_one(storage.accounts())
        .await
        .unwrap()
}

async fn mapped_card_count(storage: &StorageState, run_id: i64) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM delivery_run_cards WHERE run_id = ?")
        .bind(run_id)
        .fetch_one(storage.accounts())
        .await
        .unwrap()
}
