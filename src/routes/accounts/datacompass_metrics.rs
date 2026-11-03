use std::time::Duration;

use serde_json::Value;
use sqlx::{Row, SqlitePool};
use tokio::time::sleep;

use crate::{protocol_catalog::opcodes, state::AppState};

use super::{load_location_or_default, location_payload, successful_upstream_protocol_h5_request};
const SYNC_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);
const INTER_ACCOUNT_DELAY: Duration = Duration::from_millis(1_500);
const MAX_SYNC_ERROR_CHARS: usize = 240;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DailySellerMetric {
    pub stat_date: String,
    pub pay_amount: f64,
    pub pay_order_count: i64,
    pub refund_amount: f64,
    pub refund_order_count: i64,
}

pub(crate) fn spawn_daily_metrics_sync_worker(state: AppState) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = sync_stale_accounts(&state).await {
                tracing::warn!(error = %error, "data compass daily metrics sync pass failed");
            }
            sleep(SYNC_INTERVAL).await;
        }
    });
}

async fn sync_stale_accounts(state: &AppState) -> Result<(), sqlx::Error> {
    sync_accounts(state, None, false).await
}

pub(crate) async fn refresh_daily_metrics_for_owner(
    state: &AppState,
    owner_username: Option<&str>,
) -> Result<(), String> {
    sync_accounts(state, owner_username, true)
        .await
        .map_err(|error| error.to_string())
}

pub(crate) async fn refresh_daily_metrics_for_account(
    state: &AppState,
    uid: &str,
) -> Result<(), String> {
    sync_account_daily_metrics(state, uid).await
}

async fn sync_accounts(
    state: &AppState,
    owner_username: Option<&str>,
    force: bool,
) -> Result<(), sqlx::Error> {
    let accounts = sqlx::query(
        "SELECT uid FROM accounts WHERE status = 'active' AND TRIM(uid) <> '' AND (? = '' OR owner_username = ?) ORDER BY updated_at DESC, uid ASC",
    )
    .bind(owner_username.unwrap_or_default())
    .bind(owner_username.unwrap_or_default())
    .fetch_all(state.storage().accounts())
    .await?;
    let account_count = accounts.len();

    for (index, row) in accounts.into_iter().enumerate() {
        let uid: String = row.get("uid");
        if !force && !metrics_are_stale(state, &uid).await? {
            continue;
        }
        if let Err(error) = sync_account_daily_metrics(state, &uid).await {
            tracing::warn!(uid = %uid, error = %error, "data compass metrics sync failed for account");
        }
        if index + 1 < account_count {
            sleep(INTER_ACCOUNT_DELAY).await;
        }
    }
    Ok(())
}

async fn metrics_are_stale(state: &AppState, uid: &str) -> Result<bool, sqlx::Error> {
    let last_success_at = sqlx::query_scalar::<_, f64>(
        "SELECT last_success_at FROM data_compass_sync_state WHERE uid = ?",
    )
    .bind(uid)
    .fetch_optional(state.storage().xianyu())
    .await?
    .unwrap_or(0.0);
    Ok(unix_seconds_now() - last_success_at >= SYNC_INTERVAL.as_secs_f64())
}

async fn sync_account_daily_metrics(state: &AppState, uid: &str) -> Result<(), String> {
    let now = unix_seconds_now();
    let pool = state.storage().xianyu();
    record_attempt(pool, uid, now, "").await?;

    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("dateRange".to_string(), Value::String(String::new()));
    data.insert(
        "dateType".to_string(),
        Value::String("recent30d".to_string()),
    );
    let response = match successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::DATACOMPASS_SELLER_SUMMARY,
        Value::Object(data),
        Vec::new(),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            let message = sanitize_sync_error(&error);
            record_attempt(pool, uid, now, &message).await?;
            return Err(error);
        }
    };
    let metrics = parse_daily_seller_metrics(&response);
    if metrics.is_empty() {
        let message = "seller summary returned no daily graph data";
        record_attempt(pool, uid, now, message).await?;
        return Err(message.to_string());
    }

    let mut tx = state
        .storage()
        .xianyu()
        .begin()
        .await
        .map_err(|error| error.to_string())?;
    for metric in metrics {
        sqlx::query(
            r#"
            INSERT INTO data_compass_daily_metrics (
                uid, stat_date, pay_amount, pay_order_count, refund_amount, refund_order_count, source_synced_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(uid, stat_date) DO UPDATE SET
                pay_amount = excluded.pay_amount,
                pay_order_count = excluded.pay_order_count,
                refund_amount = excluded.refund_amount,
                refund_order_count = excluded.refund_order_count,
                source_synced_at = excluded.source_synced_at
            "#,
        )
        .bind(uid)
        .bind(metric.stat_date)
        .bind(metric.pay_amount)
        .bind(metric.pay_order_count)
        .bind(metric.refund_amount)
        .bind(metric.refund_order_count)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    }
    sqlx::query(
        r#"
        INSERT INTO data_compass_sync_state (uid, last_attempt_at, last_success_at, last_error)
        VALUES (?, ?, ?, '')
        ON CONFLICT(uid) DO UPDATE SET
            last_attempt_at = excluded.last_attempt_at,
            last_success_at = excluded.last_success_at,
            last_error = ''
        "#,
    )
    .bind(uid)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())
}

async fn record_attempt(
    pool: &SqlitePool,
    uid: &str,
    attempted_at: f64,
    error: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        INSERT INTO data_compass_sync_state (uid, last_attempt_at, last_success_at, last_error)
        VALUES (?, ?, 0, ?)
        ON CONFLICT(uid) DO UPDATE SET
            last_attempt_at = excluded.last_attempt_at,
            last_error = excluded.last_error
        "#,
    )
    .bind(uid)
    .bind(attempted_at)
    .bind(error)
    .execute(pool)
    .await
    .map_err(|value| value.to_string())?;
    Ok(())
}

fn sanitize_sync_error(error: &str) -> String {
    let normalized = error.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "upstream request failed".to_string();
    }
    normalized.chars().take(MAX_SYNC_ERROR_CHARS).collect()
}

fn parse_daily_seller_metrics(response: &Value) -> Vec<DailySellerMetric> {
    seller_graph_rows(response)
        .into_iter()
        .filter_map(|row| {
            let stat_date = row
                .get("ds")
                .and_then(Value::as_str)
                .and_then(normalize_stat_date)?;
            Some(DailySellerMetric {
                stat_date,
                pay_amount: decimal_value(row.get("payAmt")),
                pay_order_count: integer_value(row.get("payOrdCnt")),
                refund_amount: decimal_value(row.get("rfdAmt")),
                refund_order_count: integer_value(row.get("rfdOrdCnt")),
            })
        })
        .collect()
}

fn seller_graph_rows(response: &Value) -> &[Value] {
    response
        .pointer("/data/data/graphBannerBenchData/graphDataList")
        .or_else(|| response.pointer("/data/graphBannerBenchData/graphDataList"))
        .or_else(|| response.pointer("/graphBannerBenchData/graphDataList"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn normalize_stat_date(raw: &str) -> Option<String> {
    let compact = raw.trim().replace('-', "");
    if compact.len() != 8 || !compact.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}",
        &compact[..4],
        &compact[4..6],
        &compact[6..]
    ))
}

fn decimal_value(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(value)) => value.as_f64().unwrap_or(0.0),
        Some(Value::String(value)) => value.replace(',', "").parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn integer_value(value: Option<&Value>) -> i64 {
    decimal_value(value).round() as i64
}

fn unix_seconds_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use sqlx::SqlitePool;

    use super::{parse_daily_seller_metrics, record_attempt, sanitize_sync_error};

    #[test]
    fn uses_the_catalogued_seller_summary_protocol() {
        assert_eq!(
            crate::protocol_catalog::opcodes::DATACOMPASS_SELLER_SUMMARY,
            20011
        );
    }

    #[test]
    fn sync_errors_are_normalized_and_bounded_before_persistence() {
        let message = sanitize_sync_error(&format!("  upstream\nfailed {}", "x".repeat(300)));

        assert!(message.starts_with("upstream failed"));
        assert!(message.chars().count() <= super::MAX_SYNC_ERROR_CHARS);
    }

    #[tokio::test]
    async fn failed_sync_attempt_persists_a_visible_error() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE data_compass_sync_state (uid TEXT PRIMARY KEY, last_attempt_at REAL NOT NULL DEFAULT 0, last_success_at REAL NOT NULL DEFAULT 0, last_error TEXT NOT NULL DEFAULT '')",
        )
        .execute(&pool)
        .await
        .unwrap();

        record_attempt(
            &pool,
            "u1",
            123.0,
            "non-success upstream ret: FAIL_SYS_ILLEGAL_ACCESS",
        )
        .await
        .unwrap();

        let error = sqlx::query_scalar::<_, String>(
            "SELECT last_error FROM data_compass_sync_state WHERE uid = 'u1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(error, "non-success upstream ret: FAIL_SYS_ILLEGAL_ACCESS");
    }

    #[test]
    fn parses_reference_seller_summary_graph_rows() {
        let payload = json!({
            "data": { "data": { "graphBannerBenchData": { "graphDataList": [
                { "ds": "20260729", "payAmt": 12.5, "payOrdCnt": 2, "rfdAmt": "1.20", "rfdOrdCnt": 1 },
                { "ds": "2026-07-30", "payAmt": "0", "payOrdCnt": 0 }
            ] } } }
        });

        assert_eq!(
            parse_daily_seller_metrics(&payload),
            vec![
                super::DailySellerMetric {
                    stat_date: "2026-07-29".to_string(),
                    pay_amount: 12.5,
                    pay_order_count: 2,
                    refund_amount: 1.2,
                    refund_order_count: 1
                },
                super::DailySellerMetric {
                    stat_date: "2026-07-30".to_string(),
                    pay_amount: 0.0,
                    pay_order_count: 0,
                    refund_amount: 0.0,
                    refund_order_count: 0
                },
            ]
        );
    }
}
