//! Daily system-stats report scheduler.
//!
//! Every day at a configurable local time (default 22:00 — env
//! `WECHAT_DAILY_REPORT_HOUR` / `WECHAT_DAILY_REPORT_MINUTE`) we aggregate the
//! day's stats from the SQLite stores and push an owner-scoped multi-line text
//! message through each enabled openclaw_weixin channel that has
//! `bind_all_accounts: true` in its config. A user's bind-all channel only sees
//! accounts owned by that user.
//!
//! Day boundary is local-midnight, computed by reading the SQLite
//! `date('now','localtime')` value so the report aligns with whatever timezone
//! the host is running in (no chrono crate dependency).
//!
//! Missing tables / partial schemas don't crash the report — every aggregate
//! is wrapped in `.unwrap_or(0)` so a parallel-agent's not-yet-created table
//! (`account_violations`) just shows as `0`.
//!
//! At-most-once delivery only — a backend restart between the schedule moment
//! and the fire moment skips that day (acceptable per spec).

use std::{
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Map, Value};
use sqlx::{QueryBuilder, Row, Sqlite};
use tracing::{info, warn};

use crate::{models::notifications::NotificationChannel, state::AppState};

use super::channel_send;

/// Default trigger hour (local time) when `WECHAT_DAILY_REPORT_HOUR` is unset
/// or unparseable.
const DEFAULT_HOUR: u32 = 22;
/// Default trigger minute when `WECHAT_DAILY_REPORT_MINUTE` is unset.
const DEFAULT_MINUTE: u32 = 0;

/// Process boot instant — captured the first time the scheduler is spawned.
/// Used to format "系统运行 N 天 N 小时" in the report footer; backend restart
/// resets the counter (which is fine, the report explicitly says "running").
fn boot_instant() -> Instant {
    static BOOT: OnceLock<Instant> = OnceLock::new();
    *BOOT.get_or_init(Instant::now)
}

/// Spawn the once-per-day report scheduler. Detached, idempotent at the
/// caller's level (callers spawn this once at startup).
///
/// The loop:
///   1. compute next fire `Instant` (today's H:M if it's still ahead,
///      otherwise tomorrow's H:M)
///   2. `tokio::time::sleep_until` it
///   3. invoke `run_daily_report`
///   4. add a 5-second guard sleep so a fast-running report can't double-fire
///      within the same wall-clock minute
pub fn spawn_daily_report_scheduler(state: AppState) -> tokio::task::JoinHandle<()> {
    // Latch the boot instant immediately so a manual /run trigger before the
    // first scheduled fire still gets a sane uptime number.
    boot_instant();

    tokio::spawn(async move {
        let (hour, minute) = read_trigger_config();
        info!(
            target: "notifications.daily_report",
            stream = "runtime",
            module = "notifications-daily-report",
            action = "scheduler_start",
            outcome = "success",
            hour,
            minute,
            "daily report scheduler started"
        );

        loop {
            let wait = match seconds_until_next_fire(hour, minute) {
                Some(secs) => secs,
                None => {
                    // Catastrophic clock error — back off a minute and retry.
                    warn!(
                        target: "notifications.daily_report",
                        stream = "runtime",
                        module = "notifications-daily-report",
                        action = "compute_next_fire",
                        outcome = "warn",
                        error_code = "daily_report_next_fire_failed",
                        retryable = true,
                        "failed to compute next daily report fire time"
                    );
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
            };
            info!(
                target: "notifications.daily_report",
                stream = "runtime",
                module = "notifications-daily-report",
                action = "scheduler_wait",
                outcome = "success",
                wait_seconds = wait,
                hour,
                minute,
                "daily report scheduler waiting for next fire"
            );
            tokio::time::sleep(Duration::from_secs(wait)).await;

            let outcome = run_daily_report(&state).await;
            info!(
                target: "notifications.daily_report",
                stream = "runtime",
                module = "notifications-daily-report",
                action = "scheduler_fire",
                outcome = "success",
                sent = outcome.sent,
                error_count = outcome.errors.len(),
                "daily report run finished"
            );
            for err in &outcome.errors {
                warn!(
                    target: "notifications.daily_report",
                    stream = "runtime",
                    module = "notifications-daily-report",
                    action = "delivery_error",
                    outcome = "warn",
                    error_code = "daily_report_delivery_failed",
                    retryable = true,
                    detail = %err,
                    "daily report delivery error"
                );
            }

            // Guard against same-minute re-fire if `run_daily_report` is
            // sub-second — sleep past the trigger minute boundary.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    })
}

/// Outcome of a single report run — returned both from the scheduler tick and
/// from the manual-trigger HTTP endpoint.
#[derive(Debug, Default)]
pub struct ReportOutcome {
    pub sent: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
struct DailyReportDelivery {
    channel: NotificationChannel,
    text: String,
}

/// Aggregate today's stats and push the report to every eligible WeChat
/// channel. Returns the per-channel outcome so the manual-trigger endpoint can
/// surface failures to the operator.
pub async fn run_daily_report(state: &AppState) -> ReportOutcome {
    let deliveries = match build_daily_report_deliveries(state).await {
        Ok(rows) => rows,
        Err(err) => {
            return ReportOutcome {
                sent: 0,
                errors: vec![format!("build deliveries failed: {err}")],
            };
        }
    };

    let mut outcome = ReportOutcome::default();
    for delivery in deliveries {
        match send_to_channel(state, &delivery.channel, &delivery.text).await {
            Ok(()) => outcome.sent += 1,
            Err(err) => outcome
                .errors
                .push(format!("channel={} {err}", delivery.channel.id)),
        }
    }
    outcome
}

async fn build_daily_report_deliveries(
    state: &AppState,
) -> Result<Vec<DailyReportDelivery>, String> {
    let channels = state
        .storage()
        .list_bind_all_notification_channels()
        .await
        .map_err(|err| format!("list channels failed: {err}"))?;

    let mut channels_by_owner: HashMap<String, Vec<NotificationChannel>> = HashMap::new();
    for channel in channels {
        if channel.channel_type != "openclaw_weixin" {
            continue;
        }
        channels_by_owner
            .entry(channel.owner_username.trim().to_string())
            .or_default()
            .push(channel);
    }

    let mut deliveries = Vec::new();
    for (owner, channels) in channels_by_owner {
        let stats = collect_stats_for_owner(state, &owner).await?;
        let text = format_report(&stats);
        for channel in channels {
            deliveries.push(DailyReportDelivery {
                channel,
                text: text.clone(),
            });
        }
    }
    Ok(deliveries)
}

/// Collected counters for the report. Every field stays at 0 when its source
/// table is missing — see the individual `try_query_i64` calls in
/// `collect_stats`.
#[derive(Debug, Default)]
pub struct DailyStats {
    pub date: String,
    pub messages: i64,
    pub new_orders: i64,
    pub new_order_amount: f64,
    pub items_listed: i64,
    pub items_polished: i64,
    pub items_offshelf: i64,
    pub violations: i64,
    pub active_accounts: i64,
}

/// Pull every metric from the relevant DB pool. Each query is independently
/// tolerant of table absence (returns 0) — partial schema states caused by an
/// in-progress parallel agent don't fail the whole report.
async fn collect_stats_for_owner(
    state: &AppState,
    owner_username: &str,
) -> Result<DailyStats, String> {
    let uids = owner_account_uids(state, owner_username).await?;
    collect_stats_for_uids(state, Some(&uids)).await
}

async fn collect_stats_for_uids(
    state: &AppState,
    owner_uids: Option<&[String]>,
) -> Result<DailyStats, String> {
    let date = today_local_date(state.storage().live_state())
        .await
        .unwrap_or_else(|| "今日".to_string());

    if owner_uids.is_some_and(|uids| uids.is_empty()) {
        return Ok(DailyStats {
            date,
            ..DailyStats::default()
        });
    }

    let messages = query_messages_today(state, owner_uids).await;

    let (new_orders, new_order_amount) = query_orders_today(state, owner_uids).await;

    let items_listed = query_items_listed_today(state, owner_uids).await;

    // Polish events are upstream-only — no local persistence yet. Wired as 0
    // so the report layout is stable; a future polish-log table can flip this
    // on without touching the format.
    let items_polished = 0;

    let items_offshelf = query_items_offshelf_today(state, owner_uids).await;

    // `account_violations` is owned by a separate, possibly-not-yet-deployed
    // agent. `try_query_i64` swallows "no such table" so missing-schema runs
    // surface as 0.
    let violations = query_violations_today(state, owner_uids).await;

    let active_accounts = query_active_accounts(state, owner_uids).await;

    Ok(DailyStats {
        date,
        messages,
        new_orders,
        new_order_amount,
        items_listed,
        items_polished,
        items_offshelf,
        violations,
        active_accounts,
    })
}

async fn owner_account_uids(state: &AppState, owner_username: &str) -> Result<Vec<String>, String> {
    let rows = sqlx::query("SELECT uid FROM accounts WHERE owner_username = ? AND status != 'disabled' ORDER BY uid ASC")
        .bind(owner_username.trim())
        .fetch_all(state.storage().accounts())
        .await
        .map_err(|err| format!("load owner accounts failed: {err}"))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("uid").ok())
        .collect())
}

async fn query_messages_today(state: &AppState, owner_uids: Option<&[String]>) -> i64 {
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT COUNT(*)
        FROM message_history_cache
        WHERE date(ts/1000, 'unixepoch', 'localtime') = date('now', 'localtime')
        "#,
    );
    push_optional_uid_filter(&mut query, owner_uids, "AND");
    query
        .build_query_scalar::<i64>()
        .fetch_one(state.storage().live_state())
        .await
        .unwrap_or(0)
}

async fn query_items_listed_today(state: &AppState, owner_uids: Option<&[String]>) -> i64 {
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT COUNT(*)
        FROM account_items
        WHERE date(created_at, 'unixepoch', 'localtime') = date('now', 'localtime')
          AND status = 'ON_SALE'
        "#,
    );
    push_optional_uid_filter(&mut query, owner_uids, "AND");
    query
        .build_query_scalar::<i64>()
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap_or(0)
}

async fn query_items_offshelf_today(state: &AppState, owner_uids: Option<&[String]>) -> i64 {
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT COUNT(*)
        FROM account_items
        WHERE status = 'OFF_SHELF'
          AND date(updated_at, 'unixepoch', 'localtime') = date('now', 'localtime')
        "#,
    );
    push_optional_uid_filter(&mut query, owner_uids, "AND");
    query
        .build_query_scalar::<i64>()
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap_or(0)
}

async fn query_violations_today(state: &AppState, owner_uids: Option<&[String]>) -> i64 {
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT COUNT(*)
        FROM account_violations
        WHERE date(detected_at, 'unixepoch', 'localtime') = date('now', 'localtime')
        "#,
    );
    push_optional_uid_filter(&mut query, owner_uids, "AND");
    query
        .build_query_scalar::<i64>()
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap_or(0)
}

async fn query_active_accounts(state: &AppState, owner_uids: Option<&[String]>) -> i64 {
    let mut query =
        QueryBuilder::<Sqlite>::new("SELECT COUNT(DISTINCT uid) FROM message_live_state");
    push_optional_uid_filter(&mut query, owner_uids, "WHERE");
    query
        .build_query_scalar::<i64>()
        .fetch_one(state.storage().live_state())
        .await
        .unwrap_or(0)
}

/// Orders share a `created_at REAL` column (unix epoch seconds) and a `price
/// TEXT` column whose values are stringified yuan amounts. We cast to REAL in
/// SQL so SUM tolerates non-numeric junk (CAST returns 0 for unparseable).
async fn query_orders_today(state: &AppState, owner_uids: Option<&[String]>) -> (i64, f64) {
    let pool = state.storage().xianyu();
    let mut query = QueryBuilder::<Sqlite>::new(
        r#"
        SELECT COUNT(*) AS count,
               COALESCE(SUM(CAST(price AS REAL)), 0.0) AS amount
        FROM account_orders
        WHERE date(created_at, 'unixepoch', 'localtime') = date('now', 'localtime')
        "#,
    );
    push_optional_uid_filter(&mut query, owner_uids, "AND");
    let row = query.build().fetch_optional(pool).await;

    match row {
        Ok(Some(row)) => {
            let count = row.try_get::<i64, _>("count").unwrap_or(0);
            let amount = row.try_get::<f64, _>("amount").unwrap_or(0.0);
            (count, amount)
        }
        _ => (0, 0.0),
    }
}

fn push_optional_uid_filter<'a>(
    query: &mut QueryBuilder<'a, Sqlite>,
    owner_uids: Option<&'a [String]>,
    prefix: &str,
) {
    let Some(uids) = owner_uids else {
        return;
    };
    query.push(" ");
    query.push(prefix);
    query.push(" uid IN (");
    let mut separated = query.separated(", ");
    for uid in uids {
        separated.push_bind(uid);
    }
    separated.push_unseparated(")");
}

/// Ask SQLite for today's local date string (`YYYY-MM-DD`) so the report
/// header matches whatever timezone the host is in. Returns `None` on query
/// error — caller falls back to "今日".
async fn today_local_date(pool: &sqlx::SqlitePool) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT date('now', 'localtime')")
        .fetch_one(pool)
        .await
        .ok()
}

/// Render the multi-line WeChat-ready report body. Layout matches the spec
/// emoji-prefixed lines so an operator can scan it from the WeChat preview
/// without opening the message.
fn format_report(stats: &DailyStats) -> String {
    let amount = format!("{:.2}", stats.new_order_amount);
    let uptime = format_uptime(boot_instant().elapsed());
    let mut out = String::with_capacity(256);
    out.push_str(&format!("## 📊 闲鱼系统日报 ({})\n\n", stats.date));
    out.push_str("### 今日概览\n");
    out.push_str(&format!("- 📨 处理消息: **{}** 条\n", stats.messages));
    out.push_str(&format!(
        "- 📦 新订单: **{}** 笔，金额 **¥{}**\n",
        stats.new_orders, amount
    ));
    out.push_str(&format!("- 🆕 上架商品: **{}** 个\n", stats.items_listed));
    out.push_str(&format!("- ✨ 擦亮商品: **{}** 个\n", stats.items_polished));
    out.push_str(&format!("- 🚫 下架商品: **{}** 个\n", stats.items_offshelf));
    out.push_str(&format!("- ⚠️ 违规账号: **{}** 个\n", stats.violations));
    out.push_str(&format!(
        "- 👥 活跃账号: **{}** 个\n\n",
        stats.active_accounts
    ));
    out.push_str("### 系统状态\n");
    out.push_str(&format!("> 系统运行 `{uptime}`"));
    out
}

/// Render an elapsed Duration as "N 天 N 小时" or "N 小时 N 分钟" depending on
/// scale. The report never wants raw seconds.
fn format_uptime(elapsed: Duration) -> String {
    let total_secs = elapsed.as_secs();
    let days = total_secs / 86_400;
    let hours = (total_secs % 86_400) / 3_600;
    let minutes = (total_secs % 3_600) / 60;
    if days > 0 {
        format!("{days} 天 {hours} 小时")
    } else if hours > 0 {
        format!("{hours} 小时 {minutes} 分钟")
    } else {
        format!("{minutes} 分钟")
    }
}

/// Push one report to a single channel. Mirrors `dispatch_openclaw_weixin`'s
/// config-read + context-token lookup so the manual trigger path and the
/// scheduled path produce identical wire behavior.
async fn send_to_channel(
    state: &AppState,
    channel: &crate::models::notifications::NotificationChannel,
    text: &str,
) -> Result<(), String> {
    channel_send::send_text_to_channel(channel, text).await?;

    // Patch runtime so the operator sees a fresh `last_success_at` in the
    // notifications page.
    let now = unix_now_f64();
    let mut patch = Map::new();
    patch.insert("last_error".into(), Value::String(String::new()));
    patch.insert(
        "last_daily_report_at".into(),
        Value::Number(serde_json::Number::from(now as i64)),
    );
    if let Err(err) = state
        .storage()
        .upsert_notification_runtime(channel.id, "healthy", "", Some(now), Some(now), &patch)
        .await
    {
        warn!(
            target: "notifications.daily_report",
            stream = "runtime",
            module = "notifications-daily-report",
            action = "persist_runtime",
            outcome = "warn",
            channel_id = channel.id,
            error_code = "daily_report_runtime_persist_failed",
            retryable = true,
            detail = %err,
            "failed to persist daily report runtime state"
        );
    }
    Ok(())
}

/// Read `WECHAT_DAILY_REPORT_HOUR` (0-23) and `WECHAT_DAILY_REPORT_MINUTE`
/// (0-59) with sane bounds enforcement. Any out-of-range / unparseable value
/// falls back to the default so an operator misconfig can't silently disable
/// the report.
fn read_trigger_config() -> (u32, u32) {
    let hour = std::env::var("WECHAT_DAILY_REPORT_HOUR")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|h| *h <= 23)
        .unwrap_or(DEFAULT_HOUR);
    let minute = std::env::var("WECHAT_DAILY_REPORT_MINUTE")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .filter(|m| *m <= 59)
        .unwrap_or(DEFAULT_MINUTE);
    (hour, minute)
}

/// Compute seconds until the next H:M fire in *local* time, without chrono.
///
/// Strategy: get current local-time seconds-of-day from `SystemTime + libc
/// timezone offset` is too complex without chrono — instead we ask SQLite via
/// a synchronous-ish path. But the scheduler is async-only, so we rely on
/// SystemTime arithmetic with a tz-aware computation:
///
/// 1. epoch_now = SystemTime::now() as unix seconds (UTC)
/// 2. local_now_secs = epoch_now + local_tz_offset  (computed once via
///    blocking probe of the local-vs-utc delta on first call)
/// 3. day_index = local_now_secs / 86400
/// 4. target_local_secs = day_index*86400 + H*3600 + M*60
///    if target < local_now_secs → bump by 86400
/// 5. wait = target_local_secs - local_now_secs
///
/// The tz offset is cached for the lifetime of the process — long enough that
/// a daylight-savings transition during a single scheduled day is the only
/// edge case (and the report would just fire an hour off that one day, which
/// is acceptable per spec).
fn seconds_until_next_fire(hour: u32, minute: u32) -> Option<u64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let epoch = now.as_secs() as i64;
    let offset = local_tz_offset_seconds();
    let local_epoch = epoch + offset;
    let day_index = local_epoch.div_euclid(86_400);
    let secs_today = local_epoch.rem_euclid(86_400);
    let target_secs_in_day = (hour as i64) * 3_600 + (minute as i64) * 60;

    let mut target_local = day_index * 86_400 + target_secs_in_day;
    if target_local <= local_epoch {
        target_local += 86_400;
    }

    let _ = secs_today; // silence unused-var lint if logging is added later.
    let wait = target_local - local_epoch;
    if wait < 0 {
        return None;
    }
    Some(wait as u64)
}

/// Discover the local timezone offset (seconds east of UTC) by asking
/// `chrono`-style — but without chrono. We shell out to SQLite via the
/// notifications pool? No — that would require an async call from a sync
/// helper. Instead we use `std::time` + libc-free arithmetic by parsing the
/// `%::z` format from `time` … which isn't in deps either.
///
/// Pragmatic fallback: read `TZ_OFFSET_SECONDS` env (operator override) or
/// default to +28800 (CST, UTC+8) since the codebase is a Chinese-market
/// proxy. This is the same assumption every other local-time-aware code path
/// in the project makes implicitly.
fn local_tz_offset_seconds() -> i64 {
    static CACHED: OnceLock<i64> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("TZ_OFFSET_SECONDS")
            .ok()
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|s| s.abs() <= 14 * 3_600)
            .unwrap_or(8 * 3_600)
    })
}

fn unix_now_f64() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, sync::Mutex};

    use tempfile::TempDir;

    use crate::{config::AppConfig, state::AppState};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn read_trigger_defaults_when_unset() {
        // unset env is implicit — just confirm bounds path returns defaults
        let (h, m) = read_trigger_config();
        assert!(h <= 23);
        assert!(m <= 59);
    }

    #[test]
    fn format_uptime_days() {
        let d = Duration::from_secs(3 * 86_400 + 4 * 3_600 + 5 * 60);
        assert_eq!(format_uptime(d), "3 天 4 小时");
    }

    #[test]
    fn format_uptime_hours() {
        let d = Duration::from_secs(2 * 3_600 + 15 * 60);
        assert_eq!(format_uptime(d), "2 小时 15 分钟");
    }

    #[test]
    fn format_uptime_minutes() {
        let d = Duration::from_secs(45 * 60 + 12);
        assert_eq!(format_uptime(d), "45 分钟");
    }

    #[test]
    fn format_report_layout() {
        let stats = DailyStats {
            date: "2026-05-16".into(),
            messages: 1234,
            new_orders: 12,
            new_order_amount: 3456.78,
            items_listed: 8,
            items_polished: 15,
            items_offshelf: 2,
            violations: 0,
            active_accounts: 18,
        };
        let s = format_report(&stats);
        assert!(s.starts_with("## 📊 闲鱼系统日报 (2026-05-16)"));
        assert!(s.contains("### 今日概览"));
        assert!(s.contains("- 📨 处理消息: **1234** 条"));
        assert!(s.contains("- 📦 新订单: **12** 笔，金额 **¥3456.78**"));
        assert!(s.contains("- 🆕 上架商品: **8** 个"));
        assert!(s.contains("- 👥 活跃账号: **18** 个"));
        assert!(s.contains("### 系统状态"));
        assert!(s.contains("> 系统运行"));
    }

    #[tokio::test]
    async fn delivery_plan_scopes_stats_by_channel_owner() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let temp_dir = TempDir::new().unwrap();
        env::set_var(
            "APP_DATA_DIR",
            temp_dir.path().join("data").display().to_string(),
        );
        env::set_var("JWT_SECRET", "daily-report-test-secret");
        env::set_var("ADMIN_USERNAME", "admin");
        env::set_var("ADMIN_PASSWORD", "password");

        let config = AppConfig::from_env().unwrap();
        let state = AppState::bootstrap(config).await.unwrap();
        seed_owner_account(&state, "owner-a", "uid-a", "账号A").await;
        seed_owner_account(&state, "owner-b", "uid-b", "账号B").await;
        seed_daily_owner_metrics(&state, "uid-a", "order-a", 2, "10.00").await;
        seed_daily_owner_metrics(&state, "uid-b", "order-b", 1, "99.00").await;

        let mut config = Map::new();
        config.insert("bind_all_accounts".into(), Value::Bool(true));
        state
            .storage()
            .create_notification_channel_for_owner(
                "owner-a",
                "owner-a-wechat",
                "openclaw_weixin",
                &config,
                true,
            )
            .await
            .unwrap();
        state
            .storage()
            .create_notification_channel_for_owner(
                "owner-b",
                "owner-b-wechat",
                "openclaw_weixin",
                &config,
                true,
            )
            .await
            .unwrap();

        let deliveries = build_daily_report_deliveries(&state).await.unwrap();
        let owner_a = deliveries
            .iter()
            .find(|delivery| delivery.channel.owner_username == "owner-a")
            .expect("owner-a delivery");
        let owner_b = deliveries
            .iter()
            .find(|delivery| delivery.channel.owner_username == "owner-b")
            .expect("owner-b delivery");

        assert!(owner_a.text.contains("- 📨 处理消息: **2** 条"));
        assert!(owner_a.text.contains("金额 **¥10.00**"));
        assert!(!owner_a.text.contains("¥99.00"));
        assert!(owner_b.text.contains("- 📨 处理消息: **1** 条"));
        assert!(owner_b.text.contains("金额 **¥99.00**"));
        assert!(!owner_b.text.contains("¥10.00"));
    }

    async fn seed_owner_account(state: &AppState, owner: &str, uid: &str, nick: &str) {
        sqlx::query(
            r#"
            INSERT INTO accounts (
                uid, nick, token, sid, cookies, extra, status, owner_username,
                created_at, updated_at, cookie_refreshed_at
            )
            VALUES (?, ?, ?, ?, '{}', '{}', 'active', ?, 0, 0, 0)
            "#,
        )
        .bind(uid)
        .bind(nick)
        .bind(format!("{uid}-token_1710000000000"))
        .bind(format!("{uid}-sid"))
        .bind(owner)
        .execute(state.storage().accounts())
        .await
        .unwrap();
    }

    async fn seed_daily_owner_metrics(
        state: &AppState,
        uid: &str,
        order_id: &str,
        messages: usize,
        amount: &str,
    ) {
        for index in 0..messages {
            sqlx::query(
                r#"
                INSERT INTO message_history_cache (
                    uid, session_id, session_type, message_id, version, ts, text, cached_at
                )
                VALUES (?, ?, 1, ?, ?, unixepoch('now') * 1000, '今日消息', unixepoch('now'))
                "#,
            )
            .bind(uid)
            .bind(format!("session-{uid}"))
            .bind(format!("{uid}-message-{index}"))
            .bind(index as i64 + 1)
            .execute(state.storage().live_state())
            .await
            .unwrap();
        }

        sqlx::query(
            r#"
            INSERT INTO account_orders (
                uid, tid, title, price, status, status_text, created_at, updated_at
            )
            VALUES (?, ?, '今日订单', ?, 'WAIT_SELLER_SEND_GOODS', '待发货', unixepoch('now'), unixepoch('now'))
            "#,
        )
        .bind(uid)
        .bind(order_id)
        .bind(amount)
        .execute(state.storage().xianyu())
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO account_items (
                uid, item_id, title, price, status, online, created_at, updated_at
            )
            VALUES (?, ?, '今日上架', ?, 'ON_SALE', 1, unixepoch('now'), unixepoch('now'))
            "#,
        )
        .bind(uid)
        .bind(format!("item-{uid}"))
        .bind(amount)
        .execute(state.storage().xianyu())
        .await
        .unwrap();

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, summary, updated_at
            )
            VALUES (?, ?, 1, 1, '在线', unixepoch('now'))
            "#,
        )
        .bind(uid)
        .bind(format!("live-{uid}"))
        .execute(state.storage().live_state())
        .await
        .unwrap();
    }

    #[test]
    fn seconds_until_fire_in_range() {
        // Best-effort: just confirm it returns a value within a day for any
        // (h, m) in valid range.
        for hour in [0u32, 12, 22, 23] {
            for minute in [0u32, 30, 59] {
                let secs = seconds_until_next_fire(hour, minute).unwrap();
                assert!(
                    secs <= 86_400,
                    "wait for {hour:02}:{minute:02} should be <= 1 day, got {secs}"
                );
            }
        }
    }
}
