use std::{env, sync::Mutex};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tokio::{net::TcpListener, sync::oneshot};
use tower::util::ServiceExt;
use tracing_subscriber::{layer::SubscriberExt, Registry};
use xianyu_rust_backend_full::{
    app::build_app,
    auth::encode_access_token,
    config::AppConfig,
    license_runtime::{set_sign_license_context, SignLicenseContext},
    logging::{clear_logs, memory_log_layer, recent_logs},
    state::AppState,
};

static ENV_LOCK: Mutex<()> = Mutex::new(());
const DELIVERY_INTEGRITY_MIGRATION: &str =
    include_str!("../migrations/accounts/0024_delivery_configuration_integrity.sql");

#[tokio::test]
async fn final_integrity_migration_cleans_only_valid_account_delivery_pools() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(
        r#"
        CREATE TABLE delivery_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cookie_id TEXT NOT NULL,
            item_id TEXT DEFAULT '',
            card_pool_tag TEXT DEFAULT '',
            card_pool_scope TEXT DEFAULT 'account',
            enabled INTEGER DEFAULT 1,
            fixed_message TEXT NOT NULL DEFAULT '',
            one_time_enabled INTEGER NOT NULL DEFAULT 1,
            needs_repair INTEGER NOT NULL DEFAULT 0,
            created_at REAL DEFAULT 0
        );
        CREATE TABLE cards (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cookie_id TEXT NOT NULL,
            card_content TEXT DEFAULT '',
            card_pool_tag TEXT DEFAULT '',
            reusable INTEGER DEFAULT 0,
            used INTEGER DEFAULT 0,
            reserved_run_id INTEGER,
            reserved_order_id TEXT NOT NULL DEFAULT '',
            reserved_at INTEGER NOT NULL DEFAULT 0,
            created_at REAL DEFAULT 0
        );
        CREATE TABLE delivery_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            uid TEXT NOT NULL,
            rule_id INTEGER NOT NULL,
            order_id TEXT NOT NULL,
            buyer_id TEXT NOT NULL DEFAULT '',
            item_id TEXT NOT NULL,
            purchase_quantity INTEGER,
            required_card_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending',
            attempt_count INTEGER NOT NULL DEFAULT 0,
            waiting_notice_sent_at INTEGER NOT NULL DEFAULT 0,
            admin_notified_at INTEGER NOT NULL DEFAULT 0,
            consigned_at INTEGER NOT NULL DEFAULT 0,
            message_sent_at INTEGER NOT NULL DEFAULT 0,
            completed_at INTEGER NOT NULL DEFAULT 0,
            next_retry_at INTEGER NOT NULL DEFAULT 0,
            last_error_code TEXT NOT NULL DEFAULT '',
            last_error_message TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE (uid, order_id)
        );
        CREATE TABLE delivery_run_cards (
            run_id INTEGER NOT NULL,
            card_id INTEGER NOT NULL,
            position INTEGER NOT NULL,
            content_snapshot TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'reserved',
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (run_id, position),
            UNIQUE (card_id)
        );
        CREATE TABLE delivery_configuration_integrity_runs (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            requested_at INTEGER NOT NULL DEFAULT 0
        );

        INSERT INTO delivery_rules (cookie_id, item_id, card_pool_tag, card_pool_scope, needs_repair)
        VALUES
          ('uid-used', 'item-used', 'historic-used', 'account', 0),
          ('uid-available', 'item-available', 'historic-available', 'account', 0),
          ('uid-repair', 'item-repair', 'historic-repair', 'account', 1),
          ('__global__', 'item-global', 'historic-global', 'global', 0);

        INSERT INTO cards (id, cookie_id, card_content, card_pool_tag, used, reserved_run_id, created_at)
        VALUES
          (1, 'uid-used', ' SAME-USED ', 'historic-used', 1, NULL, 1),
          (2, 'uid-used', 'SAME-USED', 'historic-used', 0, NULL, 2),
          (3, 'uid-used', ' SAME-USED', 'historic-used', 0, NULL, 3),
          (7, 'uid-available', 'SAME-AVAILABLE', 'historic-available', 0, NULL, 1),
          (8, 'uid-available', ' SAME-AVAILABLE ', 'historic-available', 0, NULL, 2),
          (9, '__global__', 'SAME-GLOBAL', 'historic-global', 0, NULL, 1),
          (10, '__global__', ' SAME-GLOBAL ', 'historic-global', 0, NULL, 2),
          (11, 'uid-ordinary', 'SAME-ORDINARY', 'historic-ordinary', 0, NULL, 1),
          (12, 'uid-ordinary', ' SAME-ORDINARY ', 'historic-ordinary', 0, NULL, 2),
          (13, 'uid-repair', 'SAME-REPAIR', 'historic-repair', 0, NULL, 1),
          (14, 'uid-repair', ' SAME-REPAIR ', 'historic-repair', 0, NULL, 2);
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(DELIVERY_INTEGRITY_MIGRATION)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO delivery_configuration_integrity_runs (id, requested_at) VALUES (1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let retained: Vec<i64> = sqlx::query_scalar("SELECT id FROM cards ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(retained, vec![1, 7, 9, 10, 11, 12, 13, 14]);
    let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run_count, 0);

    for (cookie_id, pool_tag, content, expected_rows) in [
        ("uid-used", "historic-used", "SAME-USED", 0),
        ("uid-available", "historic-available", "SAME-AVAILABLE", 0),
        ("__global__", "historic-global", "SAME-GLOBAL", 1),
        ("uid-ordinary", "historic-ordinary", "SAME-ORDINARY", 1),
        ("uid-repair", "historic-repair", "SAME-REPAIR", 1),
    ] {
        let result = sqlx::query(
            "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used) VALUES (?, ?, ?, 0, 0)",
        )
        .bind(cookie_id)
        .bind(content)
        .bind(pool_tag)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(
            result.rows_affected(),
            expected_rows,
            "{cookie_id}/{pool_tag}"
        );
    }
}

#[tokio::test]
async fn integrity_marker_runs_once_and_does_not_rewrite_later_user_changes() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(
        r#"
        CREATE TABLE delivery_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cookie_id TEXT NOT NULL,
            item_id TEXT DEFAULT '',
            card_pool_tag TEXT DEFAULT '',
            card_pool_scope TEXT DEFAULT 'account',
            enabled INTEGER DEFAULT 1,
            fixed_message TEXT NOT NULL DEFAULT '',
            one_time_enabled INTEGER NOT NULL DEFAULT 1,
            needs_repair INTEGER NOT NULL DEFAULT 0,
            created_at REAL DEFAULT 0
        );
        CREATE TABLE cards (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cookie_id TEXT NOT NULL,
            card_content TEXT DEFAULT '',
            card_pool_tag TEXT DEFAULT '',
            reusable INTEGER DEFAULT 0,
            used INTEGER DEFAULT 0,
            reserved_run_id INTEGER,
            reserved_order_id TEXT NOT NULL DEFAULT '',
            reserved_at INTEGER NOT NULL DEFAULT 0,
            created_at REAL DEFAULT 0
        );
        CREATE TABLE delivery_runs (
            id INTEGER PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'pending',
            last_error_code TEXT NOT NULL DEFAULT '',
            last_error_message TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE delivery_run_cards (run_id INTEGER, card_id INTEGER, position INTEGER);
        INSERT INTO delivery_rules (
            cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message,
            one_time_enabled, needs_repair
        ) VALUES ('uid-existing', 'item-existing', 'historic-existing', 'account', '旧说明', 1, 0);
        INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used, created_at)
        VALUES ('uid-existing', '旧一次性卡', 'historic-existing', 0, 0, 1);
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(DELIVERY_INTEGRITY_MIGRATION)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO delivery_configuration_integrity_runs (id, requested_at) VALUES (1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE delivery_rules SET fixed_message = '用户新说明', one_time_enabled = 0 WHERE cookie_id = 'uid-existing'",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT OR IGNORE INTO delivery_configuration_integrity_runs (id, requested_at) VALUES (1, 2)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let settings: (String, i64, i64) = sqlx::query_as(
        r#"
        SELECT fixed_message, one_time_enabled,
               (SELECT requested_at FROM delivery_configuration_integrity_runs WHERE id = 1)
        FROM delivery_rules WHERE cookie_id = 'uid-existing'
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(settings, ("用户新说明".to_string(), 0, 1));
}

#[tokio::test]
async fn legacy_delivery_migration_is_repeatable_and_marks_ambiguous_rules_for_repair() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(
        r#"
        CREATE TABLE delivery_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cookie_id TEXT NOT NULL,
            keyword TEXT DEFAULT '',
            item_id TEXT DEFAULT '',
            card_pool_tag TEXT DEFAULT '',
            card_pool_scope TEXT DEFAULT 'account',
            enabled INTEGER DEFAULT 1,
            created_at REAL DEFAULT 0,
            fixed_message TEXT NOT NULL DEFAULT '',
            one_time_enabled INTEGER NOT NULL DEFAULT 1,
            needs_repair INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE cards (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cookie_id TEXT NOT NULL,
            card_key TEXT DEFAULT '',
            card_content TEXT DEFAULT '',
            card_pool_tag TEXT DEFAULT '',
            reusable INTEGER DEFAULT 0,
            used INTEGER DEFAULT 0,
            reserved_run_id INTEGER,
            reserved_order_id TEXT NOT NULL DEFAULT '',
            reserved_at INTEGER NOT NULL DEFAULT 0,
            created_at REAL DEFAULT 0
        );
        CREATE TABLE delivery_runs (
            id INTEGER PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'pending',
            last_error_code TEXT NOT NULL DEFAULT '',
            last_error_message TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE delivery_run_cards (
            run_id INTEGER NOT NULL,
            card_id INTEGER NOT NULL,
            position INTEGER NOT NULL,
            PRIMARY KEY (run_id, position)
        );
        INSERT INTO delivery_rules (cookie_id, item_id, card_pool_tag, card_pool_scope, enabled, created_at)
        VALUES
            ('uid-ok', 'item-ok', 'delivery:pool-ok', 'account', 1, 1),
            ('uid-history', 'item-history', 'delivery:history', 'account', 1, 1),
            ('uid-reserved', 'item-reserved', 'pool-reserved', 'account', 1, 1),
            ('uid-pre-repair', 'item-pre-repair', 'pool-pre-repair', 'account', 1, 1),
            ('uid-multi', 'item-multi', 'pool-multi', 'account', 1, 2),
            ('uid-duplicate', 'same-item', 'pool-d1', 'account', 1, 3),
            ('uid-duplicate', 'same-item', 'pool-d2', 'account', 1, 4),
            ('__global__', 'global-item', 'pool-global', 'global', 1, 5),
            ('uid-empty', 'item-empty', 'pool-empty', 'account', 1, 6);
        INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used, created_at)
        VALUES
            ('uid-ok', '  固定说明  ', 'delivery:pool-ok', 1, 0, 1),
            ('uid-ok', 'ONE-TIME', 'delivery:pool-ok', 0, 0, 2),
            ('uid-pre-repair', '迁移不得覆盖', 'pool-pre-repair', 1, 0, 1),
            ('uid-pre-repair', '一次性卡', 'pool-pre-repair', 0, 0, 2),
            ('uid-multi', '第一条', 'pool-multi', 1, 0, 1),
            ('uid-multi', '第二条', 'pool-multi', 1, 0, 2),
            ('uid-duplicate', 'D1', 'pool-d1', 1, 0, 1),
            ('uid-duplicate', 'D2', 'pool-d2', 1, 0, 1),
            ('uid-history', ' USED-CODE ', 'delivery:history', 0, 1, 10),
            ('uid-history', 'USED-CODE', 'delivery:history', 0, 0, 11),
            ('uid-reserved', ' RESERVED-CODE ', 'pool-reserved', 0, 0, 12),
            ('uid-history', 'PURE-CODE', 'delivery:history', 0, 0, 14),
            ('uid-history', ' PURE-CODE ', 'delivery:history', 0, 0, 15),
            ('uid-history', 'LEGACY-CODE', 'legacy-pool', 0, 0, 16),
            ('uid-history', ' LEGACY-CODE ', 'legacy-pool', 0, 0, 17),
            ('__global__', 'GLOBAL-CODE', 'delivery:global', 0, 0, 18),
            ('__global__', ' GLOBAL-CODE ', 'delivery:global', 0, 0, 19);
        UPDATE cards SET reserved_run_id = 88 WHERE cookie_id = 'uid-reserved';
        UPDATE delivery_rules
        SET fixed_message = '维修中说明', one_time_enabled = 0, enabled = 1, needs_repair = 1
        WHERE cookie_id = 'uid-pre-repair';
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::raw_sql(DELIVERY_INTEGRITY_MIGRATION)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql(DELIVERY_INTEGRITY_MIGRATION)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT OR REPLACE INTO delivery_configuration_integrity_runs (id, requested_at) VALUES (1, 1)")
        .execute(&pool)
        .await
        .unwrap();
    let integrity_trigger: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'migrate_delivery_configuration_integrity_insert'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(integrity_trigger.contains("WHERE needs_repair = 0"));
    sqlx::query("UPDATE delivery_rules SET one_time_enabled = 0 WHERE cookie_id = 'uid-ok'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO delivery_configuration_integrity_runs (id, requested_at) VALUES (1, 2)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let migrated: (String, i64, i64) = sqlx::query_as(
        "SELECT fixed_message, one_time_enabled, needs_repair FROM delivery_rules WHERE cookie_id = 'uid-ok'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(migrated, ("固定说明".to_string(), 0, 0));

    let pre_repair: (String, i64, i64, i64) = sqlx::query_as(
        "SELECT fixed_message, one_time_enabled, enabled, needs_repair FROM delivery_rules WHERE cookie_id = 'uid-pre-repair'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pre_repair, ("维修中说明".to_string(), 0, 1, 1));

    let reserved_rule: (i64, i64) = sqlx::query_as(
        "SELECT needs_repair, enabled FROM delivery_rules WHERE cookie_id = 'uid-reserved'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reserved_rule, (1, 0));
    let reserved_cards: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cards WHERE cookie_id = 'uid-reserved' AND reserved_run_id = 88",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reserved_cards, 1);

    let one_time_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cards WHERE cookie_id = 'uid-ok' AND reusable = 0 AND card_content = 'ONE-TIME'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(one_time_count, 1);

    for cookie_id in ["uid-multi", "uid-duplicate", "__global__", "uid-empty"] {
        let rows: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT needs_repair, enabled FROM delivery_rules WHERE cookie_id = ? ORDER BY id",
        )
        .bind(cookie_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!rows.is_empty());
        assert!(
            rows.iter().all(|row| *row == (1, 0)),
            "{cookie_id}: {rows:?}"
        );
    }

    let duplicate_insert = sqlx::query(
        "INSERT INTO delivery_rules (cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message, one_time_enabled, needs_repair) VALUES ('uid-ok', 'item-ok', 'delivery:new-pool', 'account', 'new', 0, 0)",
    )
    .execute(&pool)
    .await;
    assert!(duplicate_insert.is_err());

    let duplicate_repair_insert = sqlx::query(
        "INSERT INTO delivery_rules (cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message, one_time_enabled, needs_repair) VALUES ('uid-duplicate', 'same-item', 'delivery:replacement', 'account', 'new', 0, 0)",
    )
    .execute(&pool)
    .await;
    assert!(duplicate_repair_insert.is_err());

    let duplicate_card = sqlx::query(
        "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used) VALUES ('uid-ok', 'ONE-TIME', 'delivery:pool-ok', 0, 0)",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(duplicate_card.rows_affected(), 0);

    let history_rows: Vec<(String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT card_content, used, reserved_run_id FROM cards WHERE cookie_id = 'uid-history' AND card_pool_tag = 'delivery:history' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        history_rows,
        vec![
            (" USED-CODE ".to_string(), 1, None),
            ("PURE-CODE".to_string(), 0, None),
        ]
    );
    let untouched_non_delivery: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cards WHERE cookie_id = 'uid-history' AND card_pool_tag = 'legacy-pool'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let untouched_global: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM cards WHERE cookie_id = '__global__' AND card_pool_tag = 'delivery:global'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((untouched_non_delivery, untouched_global), (2, 2));

    for content in ["USED-CODE", " PURE-CODE "] {
        let result = sqlx::query(
            "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used) VALUES ('uid-history', ?, 'delivery:history', 0, 0)",
        )
        .bind(content)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(result.rows_affected(), 0, "{content}");
    }
}

#[tokio::test]
async fn bootstrap_runs_legacy_integrity_once_and_never_rewrites_user_changes() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-bootstrap-once").await;
    let indexes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN ('idx_cards_delivery_normalized_content', 'idx_delivery_runs_uid_rule_status_updated', 'idx_delivery_runs_updated_id_uid')",
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    assert_eq!(indexes, 3);
    fixture.seed_account("uid-old", "owner-a").await;
    sqlx::query(
        "INSERT INTO delivery_rules (cookie_id, item_id, card_pool_tag, card_pool_scope, enabled, created_at) VALUES ('uid-old', 'item-old', 'legacy-old', 'account', 1, 1)",
    )
    .execute(&fixture.accounts)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used, created_at) VALUES ('uid-old', '旧固定说明', 'legacy-old', 1, 0, 1)",
    )
    .execute(&fixture.accounts)
    .await
    .unwrap();
    sqlx::query("DELETE FROM delivery_configuration_integrity_runs WHERE id = 1")
        .execute(&fixture.accounts)
        .await
        .unwrap();

    let _restarted = AppState::bootstrap(fixture.config.clone()).await.unwrap();
    let migrated: (String, i64) = sqlx::query_as(
        "SELECT fixed_message, one_time_enabled FROM delivery_rules WHERE cookie_id = 'uid-old'",
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    assert_eq!(migrated, ("旧固定说明".to_string(), 0));

    sqlx::query(
        "UPDATE delivery_rules SET fixed_message = '用户新说明', one_time_enabled = 0 WHERE cookie_id = 'uid-old'",
    )
    .execute(&fixture.accounts)
    .await
    .unwrap();
    let list_after_user_change = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/configurations?uid=uid-old")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(list_after_user_change.status(), StatusCode::OK);
    assert_eq!(
        json_body(list_after_user_change).await[0]["oneTimeEnabled"],
        false
    );
    let marker_before: i64 = sqlx::query_scalar(
        "SELECT requested_at FROM delivery_configuration_integrity_runs WHERE id = 1",
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    let _restarted_again = AppState::bootstrap(fixture.config.clone()).await.unwrap();
    let after: (String, i64, i64) = sqlx::query_as(
        "SELECT fixed_message, one_time_enabled, (SELECT requested_at FROM delivery_configuration_integrity_runs WHERE id = 1) FROM delivery_rules WHERE cookie_id = 'uid-old'",
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    assert_eq!(after, ("用户新说明".to_string(), 0, marker_before));
    fixture.clear_env();
}

#[tokio::test]
async fn create_configuration_is_owner_scoped_deduplicated_and_redacted() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-create").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture.seed_account("uid-b", "owner-b").await;
    fixture
        .seed_item("uid-a", "item-a", "A 商品", "19.90", 1, Some(3))
        .await;
    fixture
        .seed_item("uid-b", "item-b", "B 商品", "29.90", 1, Some(2))
        .await;
    fixture
        .seed_cards("uid-a", "pool-a", &["CARD-A", "CARD-B"])
        .await;

    let response = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a",
                        "itemId": "item-a",
                        "poolScope": "account",
                        "poolTags": ["pool-a"],
                        "fixedMessage": "  使用说明  ",
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = json_body(response).await;
    let configuration = &payload["configuration"];
    assert_eq!(configuration["uid"], "uid-a");
    assert_eq!(configuration["itemId"], "item-a");
    assert_eq!(configuration["fixedMessage"], "使用说明");
    assert_eq!(configuration["availableCardCount"], 2);
    assert_eq!(configuration["product"]["title"], "A 商品");
    assert_eq!(configuration["product"]["price"], "19.90");
    assert_eq!(configuration["product"]["quantity"], 3);
    assert!(!payload.to_string().contains("cardPoolTag"));
    assert!(!payload.to_string().contains("CARD-A"));

    let stored: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM delivery_rules WHERE cookie_id = 'uid-a'),
            (SELECT COUNT(*) FROM cards WHERE cookie_id = 'uid-a'),
            (SELECT auto_ship_enabled FROM ai_reply_settings WHERE cookie_id = 'uid-a')
        "#,
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    assert_eq!(stored, (1, 2, 1));

    let duplicate = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a",
                        "itemId": "item-a",
                        "fixedMessage": "另一个说明",
                        "oneTimeEnabled": false,
                        "cards": [],
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    let foreign_list = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/configurations?uid=uid-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(foreign_list.status(), StatusCode::NOT_FOUND);

    let own_list = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/configurations?uid=uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(own_list.status(), StatusCode::OK);
    let own_payload = json_body(own_list).await;
    assert_eq!(own_payload.as_array().map(Vec::len), Some(1));
    assert!(!own_payload.to_string().contains("cardPoolTag"));
    assert!(!own_payload.to_string().contains("CARD-B"));

    fixture.clear_env();
}

#[tokio::test]
async fn fixed_only_and_one_time_only_configurations_both_succeed() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-content-modes").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture
        .seed_item("uid-a", "fixed-item", "固定说明商品", "10.00", 1, Some(2))
        .await;
    fixture
        .seed_item("uid-a", "card-item", "一次性卡密商品", "20.00", 1, Some(2))
        .await;
    fixture
        .seed_cards("uid-a", "pool-card", &["ONLY-CARD"])
        .await;

    let fixed_only = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a", "itemId": "fixed-item", "fixedMessage": "固定说明",
                        "oneTimeEnabled": false, "cards": [], "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(fixed_only.status(), StatusCode::OK);
    let fixed_payload = json_body(fixed_only).await;
    assert_eq!(fixed_payload["configuration"]["oneTimeEnabled"], false);
    assert_eq!(fixed_payload["configuration"]["availableCardCount"], 0);

    let card_only = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a", "itemId": "card-item", "fixedMessage": "",
                        "poolScope": "account", "poolTags": ["pool-card"], "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(card_only.status(), StatusCode::OK);
    let card_payload = json_body(card_only).await;
    assert_eq!(card_payload["configuration"]["fixedMessage"], "");
    assert_eq!(card_payload["configuration"]["availableCardCount"], 1);
    assert!(!card_payload.to_string().contains("ONLY-CARD"));
    assert!(!card_payload.to_string().contains("delivery:"));
    fixture.clear_env();
}

#[tokio::test]
async fn new_configuration_is_never_rewritten_by_legacy_integrity_migration() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-new-config-not-migrated").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture
        .seed_item("uid-a", "item-a", "A 商品", "10.00", 1, Some(2))
        .await;
    let create = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a", "itemId": "item-a", "fixedMessage": "只使用固定说明",
                        "oneTimeEnabled": false, "cards": ["预存但未启用"], "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(create.status(), StatusCode::OK);
    assert_eq!(
        json_body(create).await["configuration"]["oneTimeEnabled"],
        false
    );

    let list = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/configurations?uid=uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(list.status(), StatusCode::OK);
    assert_eq!(json_body(list).await[0]["oneTimeEnabled"], false);
    fixture.clear_env();
}

#[tokio::test]
async fn cross_owner_configuration_create_update_and_cards_are_not_found() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-cross-owner").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture.seed_account("uid-b", "owner-b").await;
    fixture
        .seed_item("uid-b", "item-b", "B 商品", "9.90", 1, Some(2))
        .await;
    let configuration_id = sqlx::query(
        r#"
        INSERT INTO delivery_rules (
            cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message,
            one_time_enabled, enabled, needs_repair, created_at
        ) VALUES ('uid-b', 'item-b', 'delivery:owner-b', 'account', '说明', 0, 1, 0, 1)
        "#,
    )
    .execute(&fixture.accounts)
    .await
    .unwrap()
    .last_insert_rowid();

    let requests = [
        Request::post("/api/delivery/configurations")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "uid": "uid-b", "itemId": "item-b", "fixedMessage": "越权",
                    "oneTimeEnabled": false, "cards": [], "enabled": true
                })
                .to_string(),
            ))
            .unwrap(),
        Request::put(format!("/api/delivery/configurations/{configuration_id}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"fixedMessage": "越权"}).to_string()))
            .unwrap(),
        Request::post(format!(
            "/api/delivery/configurations/{configuration_id}/cards"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"cards": ["CROSS-OWNER-CARD"]}).to_string(),
        ))
        .unwrap(),
    ];
    for request in requests {
        let response = fixture.request("owner-a", request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    fixture.clear_env();
}

#[tokio::test]
async fn create_configuration_validates_product_content_and_rolls_back_last_step_failure() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-atomic").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture
        .seed_item("uid-a", "sold-out", "售罄商品", "9.90", 1, Some(0))
        .await;
    fixture
        .seed_item("uid-a", "offline", "下架商品", "8.90", 0, Some(8))
        .await;
    fixture
        .seed_item("uid-a", "good", "正常商品", "7.90", 1, Some(2))
        .await;

    for (item_id, fixed_message, cards) in [
        ("sold-out", "说明", json!([])),
        ("offline", "说明", json!([])),
        ("good", "  ", json!(["", "  "])),
    ] {
        let response = fixture
            .request(
                "owner-a",
                Request::post("/api/delivery/configurations")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "uid": "uid-a",
                            "itemId": item_id,
                            "fixedMessage": fixed_message,
                            "oneTimeEnabled": true,
                            "cards": cards,
                            "enabled": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{item_id}");
    }

    sqlx::query("INSERT INTO ai_reply_settings (cookie_id, auto_ship_enabled) VALUES ('uid-a', 0)")
        .execute(&fixture.accounts)
        .await
        .unwrap();
    sqlx::query(
        r#"
        CREATE TRIGGER fail_auto_ship_enable
        BEFORE UPDATE OF auto_ship_enabled ON ai_reply_settings
        WHEN NEW.cookie_id = 'uid-a' AND NEW.auto_ship_enabled = 1
        BEGIN
            SELECT RAISE(FAIL, 'forced auto ship failure');
        END
        "#,
    )
    .execute(&fixture.accounts)
    .await
    .unwrap();

    let response = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a",
                        "itemId": "good",
                        "fixedMessage": "说明",
                        "oneTimeEnabled": true,
                        "cards": ["ROLLBACK-A", "ROLLBACK-B"],
                        "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let error_payload = json_body(response).await;
    assert_eq!(error_payload["detail"], "自动发货配置保存失败，请稍后重试");
    for secret in ["forced", "sqlite", "trigger", "database", "SELECT", "http"] {
        assert!(!error_payload.to_string().contains(secret), "{secret}");
    }

    let counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM delivery_rules WHERE cookie_id = 'uid-a'),
            (SELECT COUNT(*) FROM cards WHERE cookie_id = 'uid-a'),
            (SELECT auto_ship_enabled FROM ai_reply_settings WHERE cookie_id = 'uid-a')
        "#,
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0));

    fixture.clear_env();
}

#[tokio::test]
async fn runs_are_owner_scoped_filtered_retryable_and_redacted() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-runs").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture.seed_account("uid-b", "owner-b").await;
    fixture.seed_account("uid-c", "owner-a").await;
    fixture
        .seed_item("uid-a", "item-a", "A 商品", "1.00", 1, Some(1))
        .await;

    let rule_id = sqlx::query(
        r#"
        INSERT INTO delivery_rules (
            cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message,
            one_time_enabled, enabled, needs_repair, created_at
        ) VALUES ('uid-a', 'item-a', 'internal-a', 'account', '说明', 0, 1, 0, 1)
        "#,
    )
    .execute(&fixture.accounts)
    .await
    .unwrap()
    .last_insert_rowid();
    let own_run = seed_run(
        &fixture.accounts,
        "uid-a",
        rule_id,
        "order-a",
        "manual_required",
    )
    .await;
    let completed_run = seed_run(
        &fixture.accounts,
        "uid-a",
        rule_id,
        "order-done",
        "completed",
    )
    .await;
    let foreign_run = seed_run(
        &fixture.accounts,
        "uid-b",
        999,
        "order-b",
        "manual_required",
    )
    .await;
    let unknown_run = seed_run(
        &fixture.accounts,
        "uid-a",
        rule_id,
        "order-unknown",
        "retrying",
    )
    .await;
    let same_owner_other_account_run =
        seed_run(&fixture.accounts, "uid-c", 1000, "order-c", "failed").await;
    sqlx::query(
        "UPDATE delivery_runs SET updated_at = CASE id WHEN ? THEN 300 WHEN ? THEN 250 WHEN ? THEN 200 ELSE updated_at END WHERE id IN (?, ?, ?)",
    )
    .bind(own_run)
    .bind(same_owner_other_account_run)
    .bind(unknown_run)
    .bind(own_run)
    .bind(same_owner_other_account_run)
    .bind(unknown_run)
    .execute(&fixture.accounts)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE delivery_runs SET last_error_code = 'card_stock_insufficient', last_error_message = 'SECRET-CARD SELECT * FROM cards https://backend.internal' WHERE id = ?",
    )
    .bind(own_run)
    .execute(&fixture.accounts)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE delivery_runs SET last_error_code = 'unknown_backend_failure', last_error_message = 'SQL /private/data/accounts.db https://backend.internal' WHERE id = ?",
    )
    .bind(unknown_run)
    .execute(&fixture.accounts)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO delivery_run_cards (run_id, card_id, position, content_snapshot, status, created_at, updated_at) VALUES (?, 99, 0, 'SECRET-CARD', 'reserved', 1, 1)",
    )
    .bind(own_run)
    .execute(&fixture.accounts)
    .await
    .unwrap();

    let list = fixture
        .request(
            "owner-a",
            Request::get(format!(
                "/api/delivery/runs?uid=uid-a&configurationId={rule_id}&status=manual_required"
            ))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(list.status(), StatusCode::OK);
    let payload = json_body(list).await;
    assert_eq!(payload.as_array().map(Vec::len), Some(1));
    assert_eq!(payload[0]["id"], own_run);
    assert_eq!(payload[0]["lastErrorMessage"], "一次性卡密库存不足");
    assert!(!payload.to_string().contains("SECRET-CARD"));
    assert!(!payload.to_string().contains("contentSnapshot"));
    assert!(!payload.to_string().contains("internal-a"));
    assert!(!payload.to_string().contains("SELECT"));
    assert!(!payload.to_string().contains("https://"));

    let all_runs = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/runs?uid=uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let all_payload = json_body(all_runs).await;
    let unknown_payload = all_payload
        .as_array()
        .unwrap()
        .iter()
        .find(|run| run["id"] == unknown_run)
        .unwrap();
    assert_eq!(
        unknown_payload["lastErrorMessage"],
        "自动发货执行失败，请查看日志或稍后重试"
    );
    for secret in ["accounts.db", "backend.internal", "unknown_backend_failure"] {
        assert!(!all_payload.to_string().contains(secret), "{secret}");
    }

    let second_page = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/runs?page=2&pageSize=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_page_payload = json_body(second_page).await;
    assert_eq!(second_page_payload.as_array().map(Vec::len), Some(1));
    assert_eq!(second_page_payload[0]["id"], same_owner_other_account_run);
    assert!(!second_page_payload.to_string().contains("uid-b"));

    for page in ["0", "-1", "10001", "9223372036854775807"] {
        let response = fixture
            .request(
                "owner-a",
                Request::get(format!("/api/delivery/runs?page={page}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "page={page}");
        assert_eq!(
            json_body(response).await["detail"],
            "page 必须在 1 到 10000 之间"
        );
    }
    let max_page = fixture
        .request(
            "owner-a",
            Request::get("/api/delivery/runs?page=10000")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(max_page.status(), StatusCode::OK);
    assert_eq!(json_body(max_page).await.as_array().map(Vec::len), Some(0));

    let retry = fixture
        .request(
            "owner-a",
            Request::post(format!("/api/delivery/runs/{own_run}/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(retry.status(), StatusCode::OK);
    let retry_payload = json_body(retry).await;
    assert_eq!(retry_payload["run"]["status"], "pending");
    for secret in ["SECRET-CARD", "SELECT", "https://", "delivery:"] {
        assert!(!retry_payload.to_string().contains(secret), "{secret}");
    }

    let uncertain_run = seed_run(
        &fixture.accounts,
        "uid-a",
        rule_id,
        "order-uncertain",
        "manual_required",
    )
    .await;
    sqlx::query(
        "UPDATE delivery_runs SET last_error_code = 'send_result_uncertain', delivery_send_claimed_at = 123 WHERE id = ?",
    )
    .bind(uncertain_run)
    .execute(&fixture.accounts)
    .await
    .unwrap();
    let uncertain_retry = fixture
        .request(
            "owner-a",
            Request::post(format!("/api/delivery/runs/{uncertain_run}/retry"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(uncertain_retry.status(), StatusCode::CONFLICT);

    for run_id in [completed_run, foreign_run] {
        let denied = fixture
            .request(
                "owner-a",
                Request::post(format!("/api/delivery/runs/{run_id}/retry"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert!(
            matches!(
                denied.status(),
                StatusCode::CONFLICT | StatusCode::NOT_FOUND
            ),
            "run {run_id} returned {}",
            denied.status()
        );
    }

    fixture.clear_env();
}

#[tokio::test]
async fn legacy_bulk_card_operation_log_never_contains_card_content() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-bulk-log").await;
    fixture.seed_account("uid-a", "owner-a").await;

    let response = fixture
        .request(
            "owner-a",
            Request::post("/api/cards/bulk")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "cookieId": "uid-a",
                        "cards": [{
                            "cardKey": "key-a",
                            "cardContent": "TOP-SECRET-CARD",
                            "cardPoolTag": "legacy-pool",
                            "reusable": false
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(json_body(response)
        .await
        .to_string()
        .contains("TOP-SECRET-CARD"));

    let log_payload: (String, String, String, String, String) = sqlx::query_as(
        r#"
        SELECT params_json, before_json, after_json, extra_json, message
        FROM operation_logs WHERE action = 'bulk_create_cards'
        ORDER BY id DESC LIMIT 1
        "#,
    )
    .fetch_one(&fixture.xianyu)
    .await
    .unwrap();
    let combined = format!(
        "{}{}{}{}{}",
        log_payload.0, log_payload.1, log_payload.2, log_payload.3, log_payload.4
    );
    assert!(!combined.contains("TOP-SECRET-CARD"));
    assert!(!combined.contains("card_content"));
    assert!(!combined.contains("cardContent"));
    assert!(!combined.contains("legacy-pool"));
    assert!(!combined.contains("cardPoolTag"));

    fixture.clear_env();
}

#[tokio::test]
async fn legacy_bulk_duplicate_never_returns_a_stale_card_from_another_pool() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    set_env("SQLITE_MAX_CONNECTIONS", "1");
    let fixture = Fixture::new("delivery-bulk-stale-rowid").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture.seed_account("uid-other", "owner-b").await;
    let accounts = fixture.state.storage().accounts();
    sqlx::query(
        r#"
        INSERT INTO delivery_rules (
            cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message,
            one_time_enabled, enabled, needs_repair, created_at
        ) VALUES ('uid-a', 'item-a', 'historic-delivery-pool', 'account', '', 1, 1, 0, 1)
        "#,
    )
    .execute(accounts)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used, created_at) VALUES ('uid-a', 'DUPLICATE-CARD', 'historic-delivery-pool', 0, 0, 1)",
    )
    .execute(accounts)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used, created_at) VALUES ('uid-other', 'STALE-OTHER-CARD', 'other-pool', 0, 0, 2)",
    )
    .execute(accounts)
    .await
    .unwrap();

    let response = fixture
        .request(
            "owner-a",
            Request::post("/api/cards/bulk")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "cookieId": "uid-a",
                        "cards": [{
                            "cardKey": "duplicate-key",
                            "cardContent": "  DUPLICATE-CARD  ",
                            "cardPoolTag": "historic-delivery-pool",
                            "reusable": false
                        }]
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let payload = json_body(response).await;
    for leaked in [
        "DUPLICATE-CARD",
        "STALE-OTHER-CARD",
        "uid-other",
        "historic-delivery-pool",
        "other-pool",
    ] {
        assert!(!payload.to_string().contains(leaked), "{leaked}");
    }

    let duplicate_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM cards
        WHERE cookie_id = 'uid-a'
          AND card_pool_tag = 'historic-delivery-pool'
          AND TRIM(card_content) = 'DUPLICATE-CARD'
        "#,
    )
    .fetch_one(&fixture.accounts)
    .await
    .unwrap();
    assert_eq!(duplicate_count, 1);

    clear_env_key("SQLITE_MAX_CONNECTIONS");
    fixture.clear_env();
}

#[tokio::test]
async fn sold_out_configuration_only_allows_pause_cards_and_delete() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-sold-out").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture.seed_account("uid-b", "owner-b").await;
    fixture
        .seed_item("uid-a", "item-a", "A 商品", "1.00", 1, Some(1))
        .await;
    fixture
        .seed_cards("uid-a", "pool-sold-out", &["SEED"])
        .await;
    let create = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/configurations")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "uid": "uid-a", "itemId": "item-a", "fixedMessage": "说明",
                        "poolScope": "account", "poolTags": ["pool-sold-out"], "enabled": true
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await;
    let create_status = create.status();
    let create_payload = json_body(create).await;
    assert_eq!(create_status, StatusCode::OK, "{create_payload}");
    let configuration_id = create_payload["configuration"]["id"].as_i64().unwrap();
    sqlx::query("UPDATE account_items SET quantity = 0 WHERE uid = 'uid-a' AND item_id = 'item-a'")
        .execute(&fixture.xianyu)
        .await
        .unwrap();

    let enable = fixture
        .request(
            "owner-a",
            Request::put(format!("/api/delivery/configurations/{configuration_id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"enabled": true}).to_string()))
                .unwrap(),
        )
        .await;
    assert_eq!(enable.status(), StatusCode::CONFLICT);

    let pause = fixture
        .request(
            "owner-a",
            Request::put(format!("/api/delivery/configurations/{configuration_id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({"enabled": false}).to_string()))
                .unwrap(),
        )
        .await;
    let pause_status = pause.status();
    let pause_payload = json_body(pause).await;
    assert_eq!(pause_status, StatusCode::OK, "{pause_payload}");
    assert_eq!(pause_payload["configuration"]["enabled"], false);

    let add_cards = fixture
        .request(
            "owner-a",
            Request::post(format!(
                "/api/delivery/configurations/{configuration_id}/cards"
            ))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"cards": [" A ", "A", "", "B"]}).to_string(),
            ))
            .unwrap(),
        )
        .await;
    assert_eq!(add_cards.status(), StatusCode::OK);
    let add_payload = json_body(add_cards).await;
    assert_eq!(add_payload["insertedCount"], 2);
    assert_eq!(add_payload["configuration"]["availableCardCount"], 3);
    assert!(!add_payload.to_string().contains("delivery:uid-a"));
    assert!(!add_payload.to_string().contains("\"A\""));

    let foreign_delete = fixture
        .request(
            "owner-b",
            Request::delete(format!("/api/delivery/configurations/{configuration_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(foreign_delete.status(), StatusCode::NOT_FOUND);

    let delete = fixture
        .request(
            "owner-a",
            Request::delete(format!("/api/delivery/configurations/{configuration_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(delete.status(), StatusCode::OK);
    fixture.clear_env();
}

#[tokio::test]
async fn delete_only_guard_rejects_all_configuration_writes_including_delete() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new("delivery-delete-only").await;
    fixture.seed_account("uid-delete-only", "owner-a").await;
    fixture
        .seed_item("uid-delete-only", "item-a", "A 商品", "1.00", 1, Some(1))
        .await;
    let configuration_id = sqlx::query(
        r#"
        INSERT INTO delivery_rules (
            cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message,
            one_time_enabled, enabled, needs_repair, created_at
        ) VALUES ('uid-delete-only', 'item-a', 'delivery:delete-only', 'account', '说明', 0, 0, 0, 1)
        "#,
    )
    .execute(&fixture.accounts)
    .await
    .unwrap()
    .last_insert_rowid();

    let (license_url, shutdown) = spawn_delete_only_license_server().await;
    set_env("XIANYU_LICENSE_SERVER_URL", &license_url);
    set_sign_license_context(Some(SignLicenseContext {
        server_url: license_url,
        auth_token: "auth-token".to_string(),
        signed_token: "signed-token".to_string(),
        license_id: 7001,
        username: "owner-a".to_string(),
        sku: "trial".to_string(),
        account_limit: 2,
        hmac_secret_hex: "11".repeat(32),
        device_id: "device-delete-only".to_string(),
    }));

    let mut denied_results = Vec::new();
    for request in [
        Request::post("/api/delivery/configurations")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "uid": "uid-delete-only", "itemId": "item-a", "fixedMessage": "说明",
                    "oneTimeEnabled": false, "cards": [], "enabled": true
                })
                .to_string(),
            ))
            .unwrap(),
        Request::post(format!(
            "/api/delivery/configurations/{configuration_id}/cards"
        ))
        .header("content-type", "application/json")
        .body(Body::from(json!({"cards": ["SECRET"]}).to_string()))
        .unwrap(),
        Request::put(format!("/api/delivery/configurations/{configuration_id}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"enabled": false}).to_string()))
            .unwrap(),
        Request::post("/api/cards/bulk")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "cookieId": "uid-delete-only",
                    "cards": [{"cardContent": "SECRET", "cardPoolTag": "pool"}]
                })
                .to_string(),
            ))
            .unwrap(),
    ] {
        let response = fixture.request("owner-a", request).await;
        let status = response.status();
        let detail = json_body(response).await["detail"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        denied_results.push((status, detail));
    }

    let delete = fixture
        .request(
            "owner-a",
            Request::delete(format!("/api/delivery/configurations/{configuration_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let delete_status = delete.status();
    let delete_detail = json_body(delete).await["detail"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delivery_rules WHERE id = ?")
        .bind(configuration_id)
        .fetch_one(&fixture.accounts)
        .await
        .unwrap();
    set_sign_license_context(None);
    clear_env_key("XIANYU_LICENSE_SERVER_URL");
    let _ = shutdown.send(());
    fixture.clear_env();

    for (status, detail) in denied_results {
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(detail.contains("删除"));
    }
    assert_eq!(delete_status, StatusCode::FORBIDDEN);
    assert!(delete_detail.contains("删除"));
    assert_eq!(remaining, 1);
}

#[tokio::test]
async fn refresh_stock_uses_only_item_do_quantity_and_preserves_unknown_as_null() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (gateway, shutdown) = spawn_quantity_missing_h5_server().await;
    set_env("XIANYU_H5_GATEWAY", &format!("{gateway}/h5"));
    let fixture = Fixture::new("delivery-refresh-stock").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture.seed_account("uid-b", "owner-b").await;
    fixture
        .seed_item("uid-a", "item-a", "A 商品", "1.00", 1, Some(8))
        .await;
    sqlx::query(
        r#"
        UPDATE accounts
        SET token = 'seed-token_1700000000000',
            cookies = '{"_m_h5_tk":"seed-token_1700000000000","_m_h5_tk_enc":"seed-enc","unb":"uid-a"}'
        WHERE uid = 'uid-a'
        "#,
    )
    .execute(&fixture.accounts)
    .await
    .unwrap();
    let foreign = fixture
        .request(
            "owner-b",
            Request::post("/api/delivery/products/item-a/refresh-stock?uid=uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);

    let response = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/products/item-a/refresh-stock?uid=uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let response_status = response.status();
    let response_payload = json_body(response).await;
    assert_eq!(response_status, StatusCode::OK, "{response_payload}");
    assert_eq!(response_payload["quantity"], Value::Null);
    let quantity: Option<i64> = sqlx::query_scalar(
        "SELECT quantity FROM account_items WHERE uid = 'uid-a' AND item_id = 'item-a'",
    )
    .fetch_one(&fixture.xianyu)
    .await
    .unwrap();
    assert_eq!(quantity, None);

    clear_env_key("XIANYU_H5_GATEWAY");
    let _ = shutdown.send(());
    fixture.clear_env();
}

#[tokio::test]
async fn refresh_stock_upstream_errors_return_only_safe_public_message() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let (gateway, shutdown) = spawn_failing_h5_server().await;
    set_env("XIANYU_H5_GATEWAY", &format!("{gateway}/h5"));
    let fixture = Fixture::new("delivery-refresh-error").await;
    fixture.seed_account("uid-a", "owner-a").await;
    fixture
        .seed_item("uid-a", "item-a", "A 商品", "1.00", 1, Some(8))
        .await;
    sqlx::query(
        r#"
        UPDATE accounts
        SET token = 'seed-token_1700000000000',
            cookies = '{"_m_h5_tk":"seed-token_1700000000000","_m_h5_tk_enc":"seed-enc","unb":"uid-a"}'
        WHERE uid = 'uid-a'
        "#,
    )
    .execute(&fixture.accounts)
    .await
    .unwrap();

    let response = fixture
        .request(
            "owner-a",
            Request::post("/api/delivery/products/item-a/refresh-stock?uid=uid-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let payload = json_body(response).await;
    assert_eq!(payload["detail"], "商品库存刷新失败，请稍后重试");
    for secret in ["backend.internal", "SELECT", "sqlite", "/private/"] {
        assert!(!payload.to_string().contains(secret), "{secret}");
    }

    clear_env_key("XIANYU_H5_GATEWAY");
    let _ = shutdown.send(());
    fixture.clear_env();
}

#[tokio::test]
async fn delivery_owner_and_auth_failures_are_redacted_in_responses_and_logs() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    clear_logs();
    let subscriber = Registry::default().with(memory_log_layer());
    let dispatch = tracing::Dispatch::new(subscriber);
    let _scope = tracing::dispatcher::set_default(&dispatch);

    {
        let fixture = Fixture::new("delivery-owner-db-error").await;
        fixture.seed_account("uid-db", "owner-a").await;
        sqlx::query("DROP TABLE accounts")
            .execute(&fixture.accounts)
            .await
            .unwrap();

        for uri in ["/api/delivery/configurations?uid=uid-db"] {
            let response = fixture
                .request("owner-a", Request::get(uri).body(Body::empty()).unwrap())
                .await;
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let payload = json_body(response).await;
            assert_eq!(payload["detail"], "自动发货服务暂时不可用，请稍后重试");
            for secret in ["accounts", "SQL", "sqlite", "/private/", "no such table"] {
                assert!(!payload.to_string().contains(secret), "{secret}");
            }
        }
        fixture.clear_env();
    }

    {
        let fixture = Fixture::new("delivery-product-db-error").await;
        fixture.seed_account("uid-product-db", "owner-a").await;
        sqlx::query(
            "INSERT INTO delivery_rules (cookie_id, item_id, card_pool_tag, card_pool_scope, fixed_message, one_time_enabled, needs_repair) VALUES ('uid-product-db', 'item-db', 'pool-db', 'account', '说明', 0, 0)",
        )
        .execute(&fixture.accounts)
        .await
        .unwrap();
        sqlx::query("DROP TABLE account_items")
            .execute(&fixture.xianyu)
            .await
            .unwrap();
        let response = fixture
            .request(
                "owner-a",
                Request::get("/api/delivery/configurations?uid=uid-product-db")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            json_body(response).await["detail"],
            "自动发货服务暂时不可用，请稍后重试"
        );
        fixture.clear_env();
    }

    {
        let (license_url, shutdown) = spawn_failing_license_server().await;
        let fixture = Fixture::new("delivery-auth-error").await;
        fixture.seed_account("uid-auth", "owner-a").await;
        fixture
            .seed_item("uid-auth", "item-auth", "鉴权商品", "1.00", 1, Some(1))
            .await;
        set_env("XIANYU_LICENSE_SERVER_URL", &license_url);
        set_sign_license_context(Some(SignLicenseContext {
            server_url: license_url,
            auth_token: "auth-token".to_string(),
            signed_token: "signed-token".to_string(),
            license_id: 8001,
            username: "owner-a".to_string(),
            sku: "trial".to_string(),
            account_limit: 2,
            hmac_secret_hex: "22".repeat(32),
            device_id: "device-auth-error".to_string(),
        }));

        let response = fixture
            .request(
                "owner-a",
                Request::post("/api/delivery/configurations")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "uid": "uid-auth", "itemId": "item-auth", "fixedMessage": "说明",
                            "oneTimeEnabled": false, "cards": [], "enabled": true
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let payload = json_body(response).await;
        assert_eq!(payload["detail"], "自动发货服务暂时不可用，请稍后重试");
        assert!(!payload.to_string().contains("opaque-auth-failure-7f3"));

        let bulk = fixture
            .request(
                "owner-a",
                Request::post("/api/cards/bulk")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "cookieId": "uid-auth",
                            "cards": [{"cardContent": "CARD", "cardPoolTag": "pool"}]
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await;
        assert_eq!(bulk.status(), StatusCode::SERVICE_UNAVAILABLE);
        let bulk_payload = json_body(bulk).await;
        assert_eq!(bulk_payload["detail"], "卡密导入服务暂时不可用，请稍后重试");
        assert!(!bulk_payload.to_string().contains("opaque-auth-failure-7f3"));

        set_sign_license_context(None);
        clear_env_key("XIANYU_LICENSE_SERVER_URL");
        let _ = shutdown.send(());
        fixture.clear_env();
    }

    let logs = recent_logs(200)
        .into_iter()
        .map(|entry| format!("{} {}", entry.message, entry.fields_json))
        .collect::<Vec<_>>()
        .join("\n");
    for secret in [
        "backend.internal",
        "SELECT *",
        "no such table",
        "/private/",
        "historic-used",
        "TOP-SECRET-CARD",
    ] {
        assert!(!logs.contains(secret), "日志泄露了 {secret}: {logs}");
    }
}

struct Fixture {
    _temp_dir: TempDir,
    config: AppConfig,
    state: AppState,
    accounts: SqlitePool,
    xianyu: SqlitePool,
}

impl Fixture {
    async fn new(secret: &str) -> Self {
        set_sign_license_context(None);
        clear_env_key("XIANYU_LICENSE_SERVER_URL");
        let temp_dir = TempDir::new().unwrap();
        set_env("APP_DATA_DIR", temp_dir.path().to_str().unwrap());
        set_env("JWT_SECRET", secret);
        set_env("API_TOKEN", "auto-delivery-routes");
        let config = AppConfig::from_env().unwrap();
        let state = AppState::bootstrap(config.clone()).await.unwrap();
        let accounts = connect_pool(&config.sqlite.accounts_db_path).await;
        let xianyu = connect_pool(&config.sqlite.xianyu_db_path).await;
        Self {
            _temp_dir: temp_dir,
            config,
            state,
            accounts,
            xianyu,
        }
    }

    async fn seed_account(&self, uid: &str, owner: &str) {
        sqlx::query(
            "INSERT INTO accounts (uid, nick, status, owner_username) VALUES (?, ?, 'active', ?)",
        )
        .bind(uid)
        .bind(format!("账号 {uid}"))
        .bind(owner)
        .execute(&self.accounts)
        .await
        .unwrap();
    }

    async fn seed_item(
        &self,
        uid: &str,
        item_id: &str,
        title: &str,
        price: &str,
        online: i64,
        quantity: Option<i64>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO account_items (
                uid, item_id, title, price, pic_url, status, online, quantity,
                quantity_source, created_at, updated_at
            ) VALUES (?, ?, ?, ?, 'https://img.example/item.png', 'ON_SALE', ?, ?,
                      'pc.detail:data.itemDO.quantity', 1, 1)
            "#,
        )
        .bind(uid)
        .bind(item_id)
        .bind(title)
        .bind(price)
        .bind(online)
        .bind(quantity)
        .execute(&self.xianyu)
        .await
        .unwrap();
    }

    async fn seed_cards(&self, uid: &str, pool_tag: &str, contents: &[&str]) {
        for content in contents {
            sqlx::query(
                "INSERT INTO cards (cookie_id, card_content, card_pool_tag, reusable, used, created_at) VALUES (?, ?, ?, 0, 0, 1)",
            )
            .bind(uid)
            .bind(content)
            .bind(pool_tag)
            .execute(&self.accounts)
            .await
            .unwrap();
        }
    }

    async fn request(
        &self,
        username: &str,
        mut request: Request<Body>,
    ) -> axum::response::Response {
        let token = encode_access_token(&self.config.auth, username, "user", false).unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        build_app(self.state.clone())
            .oneshot(request)
            .await
            .unwrap()
    }

    fn clear_env(&self) {
        clear_env_key("APP_DATA_DIR");
        clear_env_key("JWT_SECRET");
        clear_env_key("API_TOKEN");
    }
}

async fn seed_run(pool: &SqlitePool, uid: &str, rule_id: i64, order_id: &str, status: &str) -> i64 {
    sqlx::query(
        r#"
        INSERT INTO delivery_runs (
            uid, rule_id, order_id, buyer_id, item_id, purchase_quantity,
            required_card_count, status, created_at, updated_at
        ) VALUES (?, ?, ?, 'buyer', 'item-a', 1, 1, ?, 1, 1)
        "#,
    )
    .bind(uid)
    .bind(rule_id)
    .bind(order_id)
    .bind(status)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_rowid()
}

async fn connect_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePool::connect(&format!("sqlite://{}", path.display()))
        .await
        .unwrap()
}

async fn spawn_delete_only_license_server() -> (String, oneshot::Sender<()>) {
    async fn resolve() -> axum::Json<Value> {
        axum::Json(json!([{
            "account_uid": "uid-delete-only",
            "status": "owned_by_other",
            "owner_username": "other-user",
            "message": "该账号当前归属于其他用户"
        }]))
    }

    let app = axum::Router::new().route(
        "/api/me/xianyu-account-bindings/resolve",
        axum::routing::post(resolve),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{address}"), shutdown_tx)
}

async fn spawn_quantity_missing_h5_server() -> (String, oneshot::Sender<()>) {
    async fn detail() -> axum::Json<Value> {
        axum::Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "quantity": 99,
                "itemDO": {"itemId": "item-a", "title": "A 商品"}
            }
        }))
    }

    let app = axum::Router::new().route(
        "/h5/mtop.taobao.idle.pc.detail/1.0/",
        axum::routing::post(detail),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{address}"), shutdown_tx)
}

async fn spawn_failing_h5_server() -> (String, oneshot::Sender<()>) {
    async fn detail() -> axum::Json<Value> {
        axum::Json(json!({
            "ret": ["FAIL::https://backend.internal/private SELECT * FROM sqlite_master"],
            "data": {}
        }))
    }

    let app = axum::Router::new().route(
        "/h5/mtop.taobao.idle.pc.detail/1.0/",
        axum::routing::post(detail),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{address}"), shutdown_tx)
}

async fn spawn_failing_license_server() -> (String, oneshot::Sender<()>) {
    async fn resolve() -> (StatusCode, axum::Json<Value>) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({
                "detail": "opaque-auth-failure-7f3"
            })),
        )
    }

    let app = axum::Router::new().route(
        "/api/me/xianyu-account-bindings/resolve",
        axum::routing::post(resolve),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (format!("http://{address}"), shutdown_tx)
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn set_env(key: &str, value: &str) {
    unsafe { env::set_var(key, value) }
}

fn clear_env_key(key: &str) {
    unsafe { env::remove_var(key) }
}
