use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Extension, Query, State},
    routing::get,
    Json as ResponseJson, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

use crate::{
    models::auth::RequestIdentity,
    models::runtime::StartupDependencyStatus,
    routes::{
        accounts::messages::should_suppress_chat_unread,
        error::{ApiResult, ResponseError},
        tenant::owner_username_filter,
    },
    state::AppState,
    storage::StorageState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/stats/today"),
            get(today_stats),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/stats/data-center"),
            get(data_center_stats),
        )
}

async fn today_stats(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = owner_username_filter(&state, &identity);
    let stats = collect_dashboard_stats(state.storage(), owner.as_deref())
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(
        serde_json::to_value(stats).map_err(ResponseError::internal)?,
    ))
}

async fn data_center_stats(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DataCenterQuery>,
) -> ApiResult {
    let owner = owner_username_filter(&state, &identity);
    let mut stats = collect_data_center_stats(state.storage(), owner.as_deref())
        .await
        .map_err(ResponseError::internal)?;
    let trend_uid = query
        .trend_uid
        .as_deref()
        .map(str::trim)
        .filter(|uid| !uid.is_empty());
    if let Some(uid) = trend_uid {
        if !stats.accounts.iter().any(|account| account.uid == uid) {
            return Err(ResponseError::bad_request(
                "selected account is unavailable",
            ));
        }
    }
    if query.refresh {
        let refreshed = match trend_uid {
            Some(uid) => {
                crate::routes::accounts::datacompass_metrics::refresh_daily_metrics_for_account(
                    &state, uid,
                )
                .await
            }
            None => {
                crate::routes::accounts::datacompass_metrics::refresh_daily_metrics_for_owner(
                    &state,
                    owner.as_deref(),
                )
                .await
            }
        };
        if let Err(error) = refreshed {
            tracing::debug!(error = %error, "dashboard requested data compass refresh completed with account failures");
        }
        stats = collect_data_center_stats(state.storage(), owner.as_deref())
            .await
            .map_err(ResponseError::internal)?;
    }
    if let Some(uid) = trend_uid {
        let account_uids = HashSet::from([uid.to_string()]);
        stats.seller_trend = load_seller_trend_bundle(state.storage().xianyu(), &account_uids)
            .await
            .map_err(ResponseError::internal)?;
    }
    patch_data_center_runtime_status(&mut stats.sync, &state);
    Ok(ResponseJson(
        serde_json::to_value(stats).map_err(ResponseError::internal)?,
    ))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DataCenterQuery {
    #[serde(default)]
    trend_uid: Option<String>,
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardStats {
    date: String,
    generated_at: f64,
    today_messages: i64,
    unread_messages: i64,
    unread_sessions: i64,
    overdue_reply_sessions: i64,
    today_orders: i64,
    today_revenue: f64,
    pending_pay_orders: i64,
    wait_send_orders: i64,
    wait_receive_orders: i64,
    closed_orders: i64,
    account_total: i64,
    active_accounts: i64,
    inactive_accounts: i64,
    restricted_accounts: i64,
    session_error_accounts: i64,
    account_rows: Vec<AccountBusinessRow>,
    recent_events: Vec<DashboardEvent>,
    risk_items: Vec<DashboardRiskItem>,
    trend7d: Vec<DashboardTrendPoint>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountBusinessRow {
    uid: String,
    nick: String,
    status: String,
    today_messages: i64,
    unread_messages: i64,
    unread_sessions: i64,
    today_orders: i64,
    today_revenue: f64,
    wait_send_orders: i64,
    pending_pay_orders: i64,
    last_message_at: i64,
    last_order_at: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardEvent {
    kind: String,
    order_id: String,
    title: String,
    subtitle: String,
    uid: String,
    nick: String,
    buyer_nick: String,
    status: String,
    amount: f64,
    ts: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardRiskItem {
    kind: String,
    title: String,
    subtitle: String,
    severity: String,
    target_path: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DashboardTrendPoint {
    date: String,
    messages: i64,
    orders: i64,
    revenue: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterStats {
    date: String,
    generated_at: f64,
    totals: DataCenterTotals,
    accounts: Vec<DataCenterAccountRow>,
    queue: Vec<DataCenterQueueItem>,
    share: DataCenterShareBundle,
    trend: DataCenterTrendBundle,
    seller_trend: DataCenterSellerTrendBundle,
    sync: DataCenterSyncSummary,
    automation: DataCenterAutomationSummary,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterTotals {
    today_revenue: f64,
    today_orders: i64,
    wait_send_orders: i64,
    wait_send_earliest_at: f64,
    pending_pay_orders: i64,
    pending_pay_amount: f64,
    unread_sessions: i64,
    unread_messages: i64,
    overdue_reply_sessions: i64,
    online_items: i64,
    total_items: i64,
    aftersale_orders: i64,
    closed_orders: i64,
    account_total: i64,
    active_accounts: i64,
    live_online_accounts: i64,
    session_error_accounts: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterAccountRow {
    uid: String,
    nick: String,
    avatar: String,
    status: String,
    live_status: String,
    last_error: String,
    today_revenue: f64,
    week_revenue: f64,
    total_revenue: f64,
    today_orders: i64,
    week_orders: i64,
    total_orders: i64,
    unread_messages: i64,
    unread_sessions: i64,
    wait_send_orders: i64,
    pending_pay_orders: i64,
    online_items: i64,
    total_items: i64,
    last_message_at: i64,
    last_order_at: f64,
    items_synced_at: f64,
    orders_synced_at: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterQueueItem {
    id: String,
    kind: String,
    uid: String,
    account_nick: String,
    title: String,
    subtitle: String,
    customer: String,
    amount: f64,
    status: String,
    status_label: String,
    target_path: String,
    action_label: String,
    ts: f64,
    item_id: String,
    item_pic: String,
    session_id: String,
    order_id: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterShareBundle {
    day: DataCenterShareRange,
    week: DataCenterShareRange,
    total: DataCenterShareRange,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterShareRange {
    range: String,
    label: String,
    total_amount: f64,
    total_orders: i64,
    segments: Vec<DataCenterShareSegment>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterShareSegment {
    uid: String,
    nick: String,
    amount: f64,
    orders: i64,
    percent: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterTrendBundle {
    week: DataCenterTrendRange,
    month: DataCenterTrendRange,
    total: DataCenterTrendRange,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterTrendRange {
    range: String,
    label: String,
    total_amount: f64,
    total_orders: i64,
    points: Vec<DataCenterTrendPoint>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterTrendPoint {
    date: String,
    label: String,
    amount: f64,
    orders: i64,
    messages: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterSellerTrendBundle {
    week: DataCenterSellerTrendRange,
    half_month: DataCenterSellerTrendRange,
    month: DataCenterSellerTrendRange,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterSellerTrendRange {
    available: bool,
    range: String,
    label: String,
    total_amount: f64,
    total_orders: i64,
    points: Vec<DataCenterTrendPoint>,
    synced_at: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterSyncSummary {
    status: String,
    subtitle: String,
    sqlite_ok: bool,
    live_online: i64,
    live_total: i64,
    stale_accounts: i64,
    sign_service_ok: bool,
    sign_service_status_code: Option<u16>,
    license_service_ok: bool,
    license_service_status_code: Option<u16>,
    data_compass: DataCenterDataCompassSummary,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterDataCompassSummary {
    status: String,
    account_count: i64,
    success_count: i64,
    failed_count: i64,
    last_success_at: f64,
    error_summary: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterAutomationSummary {
    delivery: DataCenterAutomationMetric,
    scheduled_tasks: DataCenterAutomationMetric,
    replies: DataCenterAutomationMetric,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataCenterAutomationMetric {
    enabled: Option<i64>,
    running: Option<i64>,
    pending: Option<i64>,
    errors: Option<i64>,
}

fn patch_data_center_runtime_status(sync: &mut DataCenterSyncSummary, state: &AppState) {
    let runtime = state
        .runtime()
        .inspection(state.config().service_name.clone())
        .dependencies;
    patch_sync_dependency_status(sync, &runtime);
}

fn patch_sync_dependency_status(
    sync: &mut DataCenterSyncSummary,
    dependencies: &StartupDependencyStatus,
) {
    sync.sign_service_ok = dependencies.sign_service.ok;
    sync.sign_service_status_code = dependencies.sign_service.status_code;
    sync.license_service_ok = dependencies.license_service.ok;
    sync.license_service_status_code = dependencies.license_service.status_code;
}

pub(crate) async fn collect_dashboard_stats(
    storage: &StorageState,
    owner_username: Option<&str>,
) -> Result<DashboardStats, sqlx::Error> {
    collect_dashboard_stats_from_pools(
        storage.accounts(),
        storage.xianyu(),
        storage.live_state(),
        owner_username,
    )
    .await
}

pub(crate) async fn collect_data_center_stats(
    storage: &StorageState,
    owner_username: Option<&str>,
) -> Result<DataCenterStats, sqlx::Error> {
    collect_data_center_stats_from_pools(
        storage.accounts(),
        storage.xianyu(),
        storage.live_state(),
        owner_username,
    )
    .await
}

async fn collect_data_center_stats_from_pools(
    accounts_pool: &SqlitePool,
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    owner_username: Option<&str>,
) -> Result<DataCenterStats, sqlx::Error> {
    let date = today_local_date(live_pool)
        .await
        .unwrap_or_else(|| "今日".to_string());
    let generated_at = unix_seconds_now();

    let mut accounts = load_data_center_accounts(accounts_pool, owner_username).await?;
    let mut by_uid = accounts
        .iter()
        .enumerate()
        .map(|(idx, row)| (row.uid.clone(), idx))
        .collect::<HashMap<_, _>>();
    let account_uids = accounts
        .iter()
        .map(|row| row.uid.clone())
        .collect::<HashSet<_>>();
    let platform_messages_disabled_uids =
        load_platform_message_disabled_uids(accounts_pool, &account_uids).await?;

    apply_item_metrics(xianyu_pool, &mut accounts, &mut by_uid).await?;
    apply_order_metrics(xianyu_pool, &mut accounts, &mut by_uid).await?;
    apply_message_metrics(
        live_pool,
        &mut accounts,
        &mut by_uid,
        &platform_messages_disabled_uids,
    )
    .await?;
    apply_sync_metrics(xianyu_pool, live_pool, &mut accounts, &mut by_uid).await?;

    accounts.sort_by(|left, right| {
        right
            .today_revenue
            .partial_cmp(&left.today_revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.today_orders.cmp(&left.today_orders))
            .then_with(|| right.unread_messages.cmp(&left.unread_messages))
            .then_with(|| {
                right
                    .week_revenue
                    .partial_cmp(&left.week_revenue)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| left.uid.cmp(&right.uid))
    });

    let order_totals = load_data_center_order_totals(xianyu_pool, &account_uids).await?;
    let unread_messages = accounts.iter().map(|row| row.unread_messages).sum();
    let unread_sessions = accounts.iter().map(|row| row.unread_sessions).sum();
    let overdue_reply_sessions =
        load_overdue_reply_sessions(live_pool, &account_uids, &platform_messages_disabled_uids)
            .await
            .unwrap_or(0);
    let total_items = accounts.iter().map(|row| row.total_items).sum();
    let online_items = accounts.iter().map(|row| row.online_items).sum();
    let account_total = accounts.len() as i64;
    let active_accounts = accounts.iter().filter(|row| row.status == "active").count() as i64;
    let active_account_uids = accounts
        .iter()
        .filter(|row| row.status == "active")
        .map(|row| row.uid.clone())
        .collect::<HashSet<_>>();
    let session_error_accounts = accounts
        .iter()
        .filter(|row| account_has_session_error(row))
        .count() as i64;
    let live_online_accounts = accounts
        .iter()
        .filter(|row| row.live_status == "connected")
        .count() as i64;

    let queue = load_data_center_queue(xianyu_pool, live_pool, &accounts).await?;
    let share = DataCenterShareBundle {
        day: build_share_range("day", "今日成交", &accounts, |row| {
            (row.today_revenue, row.today_orders)
        }),
        week: build_share_range("week", "7 天成交", &accounts, |row| {
            (row.week_revenue, row.week_orders)
        }),
        total: build_share_range("total", "总成交", &accounts, |row| {
            (row.total_revenue, row.total_orders)
        }),
    };
    let trend = DataCenterTrendBundle {
        week: load_trend_range(xianyu_pool, live_pool, &account_uids, "week", "7 天成交", 7)
            .await?,
        month: load_trend_range(
            xianyu_pool,
            live_pool,
            &account_uids,
            "month",
            "30 天成交",
            30,
        )
        .await?,
        total: load_total_trend_range(xianyu_pool, live_pool, &account_uids).await?,
    };
    let seller_trend = load_seller_trend_bundle(xianyu_pool, &account_uids).await?;

    let stale_accounts = accounts
        .iter()
        .filter(|row| {
            row.orders_synced_at > 0.0 && generated_at - row.orders_synced_at > 6.0 * 3600.0
                || row.items_synced_at > 0.0 && generated_at - row.items_synced_at > 6.0 * 3600.0
        })
        .count() as i64;
    let data_compass = load_data_compass_sync_summary(xianyu_pool, &active_account_uids).await?;
    let sync = DataCenterSyncSummary {
        status: if session_error_accounts > 0 {
            "attention".to_string()
        } else {
            "ok".to_string()
        },
        subtitle: format!(
            "IM 在线 {live_online_accounts}/{account_total}，SQLite 写入正常，{} 个账号同步可能滞后",
            stale_accounts
        ),
        sqlite_ok: true,
        live_online: live_online_accounts,
        live_total: account_total,
        stale_accounts,
        sign_service_ok: false,
        sign_service_status_code: None,
        license_service_ok: false,
        license_service_status_code: None,
        data_compass,
    };
    let automation =
        load_data_center_automation(accounts_pool, xianyu_pool, owner_username, &account_uids)
            .await?;

    Ok(DataCenterStats {
        date,
        generated_at,
        totals: DataCenterTotals {
            today_revenue: order_totals.today_revenue,
            today_orders: order_totals.today_orders,
            wait_send_orders: order_totals.wait_send_orders,
            wait_send_earliest_at: order_totals.wait_send_earliest_at,
            pending_pay_orders: order_totals.pending_pay_orders,
            pending_pay_amount: order_totals.pending_pay_amount,
            unread_sessions,
            unread_messages,
            overdue_reply_sessions,
            online_items,
            total_items,
            aftersale_orders: order_totals.aftersale_orders,
            closed_orders: order_totals.closed_orders,
            account_total,
            active_accounts,
            live_online_accounts,
            session_error_accounts,
        },
        accounts,
        queue,
        share,
        trend,
        seller_trend,
        sync,
        automation,
    })
}

async fn load_data_center_automation(
    accounts_pool: &SqlitePool,
    xianyu_pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uids: &HashSet<String>,
) -> Result<DataCenterAutomationSummary, sqlx::Error> {
    let owner_keys =
        load_data_center_owner_scope_keys(accounts_pool, owner_username, account_uids).await?;
    let delivery = load_delivery_automation(accounts_pool, account_uids, &owner_keys).await?;
    let scheduled_tasks = load_scheduled_automation(xianyu_pool, owner_username).await?;
    let replies = load_reply_automation(accounts_pool, account_uids).await?;
    Ok(DataCenterAutomationSummary {
        delivery,
        scheduled_tasks,
        replies,
    })
}

async fn load_data_center_owner_scope_keys(
    accounts_pool: &SqlitePool,
    owner_username: Option<&str>,
    account_uids: &HashSet<String>,
) -> Result<HashSet<String>, sqlx::Error> {
    if let Some(owner) = owner_username {
        let mut keys = account_uids.clone();
        keys.insert(user_platform_scope_key(owner));
        return Ok(keys);
    }

    let rows =
        sqlx::query("SELECT uid, COALESCE(owner_username, '') AS owner_username FROM accounts")
            .fetch_all(accounts_pool)
            .await?;
    let mut keys = account_uids.clone();
    keys.extend(rows.into_iter().filter_map(|row| {
        let uid = row.get::<String, _>("uid");
        account_uids
            .contains(&uid)
            .then(|| user_platform_scope_key(&row.get::<String, _>("owner_username")))
    }));
    Ok(keys)
}

async fn has_table(pool: &SqlitePool, table: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
    )
    .bind(table)
    .fetch_one(pool)
    .await
    .map(|count| count > 0)
}

async fn has_columns(
    pool: &SqlitePool,
    table: &str,
    required: &[&str],
) -> Result<bool, sqlx::Error> {
    let columns = table_columns(pool, table).await?;
    Ok(required.iter().all(|column| columns.contains(*column)))
}

async fn table_columns(pool: &SqlitePool, table: &str) -> Result<HashSet<String>, sqlx::Error> {
    Ok(sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|row| row.get::<String, _>("name"))
        .collect())
}

async fn load_delivery_automation(
    accounts_pool: &SqlitePool,
    account_uids: &HashSet<String>,
    owner_keys: &HashSet<String>,
) -> Result<DataCenterAutomationMetric, sqlx::Error> {
    let mut metric = DataCenterAutomationMetric::default();
    if has_table(accounts_pool, "delivery_rules").await?
        && has_columns(accounts_pool, "delivery_rules", &["cookie_id", "enabled"]).await?
    {
        let rows =
            sqlx::query("SELECT cookie_id, COALESCE(enabled, 0) AS enabled FROM delivery_rules")
                .fetch_all(accounts_pool)
                .await?;
        metric.pending = Some(
            rows.into_iter()
                .filter(|row| owner_keys.contains(&row.get::<String, _>("cookie_id")))
                .filter(|row| row.get::<i64, _>("enabled") != 0)
                .count() as i64,
        );
    }
    if !has_table(accounts_pool, "ai_reply_settings").await?
        || !has_columns(
            accounts_pool,
            "ai_reply_settings",
            &["cookie_id", "auto_ship_enabled"],
        )
        .await?
    {
        return Ok(metric);
    }
    let rows = sqlx::query(
        "SELECT COALESCE(cookie_id, '') AS cookie_id, COALESCE(auto_ship_enabled, 0) AS auto_ship_enabled FROM ai_reply_settings",
    )
    .fetch_all(accounts_pool)
    .await?;
    metric.enabled = Some(
        rows.into_iter()
            .filter(|row| account_uids.contains(&row.get::<String, _>("cookie_id")))
            .filter(|row| row.get::<i64, _>("auto_ship_enabled") != 0)
            .count() as i64,
    );
    Ok(metric)
}

async fn load_scheduled_automation(
    xianyu_pool: &SqlitePool,
    owner_username: Option<&str>,
) -> Result<DataCenterAutomationMetric, sqlx::Error> {
    if !has_table(xianyu_pool, "automation_schedules").await?
        || !has_columns(
            xianyu_pool,
            "automation_schedules",
            &["owner_username", "enabled", "last_status", "next_run_at"],
        )
        .await?
    {
        return Ok(DataCenterAutomationMetric::default());
    }
    let owner = owner_username.unwrap_or("");
    let rows = sqlx::query(
        "SELECT COALESCE(enabled, 0) AS enabled, COALESCE(last_status, '') AS last_status, COALESCE(next_run_at, 0) AS next_run_at FROM automation_schedules WHERE ? = '' OR owner_username = ?",
    )
    .bind(owner)
    .bind(owner)
    .fetch_all(xianyu_pool)
    .await?;
    let mut metric = DataCenterAutomationMetric {
        enabled: Some(0),
        running: Some(0),
        pending: Some(0),
        errors: Some(0),
    };
    for row in rows {
        update_scheduled_automation_metric(
            &mut metric,
            row.get::<i64, _>("enabled"),
            &row.get::<String, _>("last_status"),
            row.get::<f64, _>("next_run_at"),
        );
    }
    Ok(metric)
}

fn update_scheduled_automation_metric(
    metric: &mut DataCenterAutomationMetric,
    enabled: i64,
    last_status: &str,
    next_run_at: f64,
) {
    if enabled != 0 {
        metric.enabled = metric.enabled.map(|count| count + 1);
        match last_status {
            "running" => metric.running = metric.running.map(|count| count + 1),
            "failed" | "error" => metric.errors = metric.errors.map(|count| count + 1),
            _ => {}
        }
        if next_run_at > 0.0 {
            metric.pending = metric.pending.map(|count| count + 1);
        }
    }
}

async fn load_reply_automation(
    accounts_pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<DataCenterAutomationMetric, sqlx::Error> {
    if !has_table(accounts_pool, "ai_reply_settings").await? {
        return Ok(DataCenterAutomationMetric::default());
    }
    let columns = table_columns(accounts_pool, "ai_reply_settings").await?;
    let has_reply_mode = columns.contains("reply_mode");
    let has_ai_enabled = columns.contains("ai_enabled");
    let has_auto_reply_enabled = columns.contains("auto_reply_enabled");
    if !columns.contains("cookie_id")
        || !(has_reply_mode || has_ai_enabled || has_auto_reply_enabled)
    {
        return Ok(DataCenterAutomationMetric::default());
    }

    let reply_mode = if has_reply_mode {
        "COALESCE(reply_mode, '')"
    } else {
        "''"
    };
    let ai_enabled = if has_ai_enabled {
        "COALESCE(ai_enabled, 0)"
    } else {
        "0"
    };
    let auto_reply_enabled = if has_auto_reply_enabled {
        "COALESCE(auto_reply_enabled, 0)"
    } else {
        "0"
    };
    let rows = sqlx::query(&format!(
        "SELECT COALESCE(cookie_id, '') AS cookie_id, {reply_mode} AS reply_mode, {ai_enabled} AS ai_enabled, {auto_reply_enabled} AS auto_reply_enabled FROM ai_reply_settings"
    ))
    .fetch_all(accounts_pool)
    .await?;
    let enabled = rows
        .into_iter()
        .filter(|row| account_uids.contains(&row.get::<String, _>("cookie_id")))
        .filter(|row| {
            matches!(
                row.get::<String, _>("reply_mode").trim(),
                "default" | "keyword" | "ai"
            ) || row.get::<i64, _>("ai_enabled") != 0
                || row.get::<i64, _>("auto_reply_enabled") != 0
        })
        .count() as i64;
    Ok(DataCenterAutomationMetric {
        enabled: Some(enabled),
        ..Default::default()
    })
}

async fn collect_dashboard_stats_from_pools(
    accounts_pool: &SqlitePool,
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    owner_username: Option<&str>,
) -> Result<DashboardStats, sqlx::Error> {
    let date = today_local_date(live_pool)
        .await
        .unwrap_or_else(|| "今日".to_string());
    let generated_at = unix_seconds_now();

    let accounts = load_account_rows(accounts_pool, owner_username).await?;
    let mut account_map = accounts
        .into_iter()
        .map(|row| (row.uid.clone(), row))
        .collect::<HashMap<_, _>>();

    let scoped_uids = account_map.keys().cloned().collect::<HashSet<_>>();
    let platform_messages_disabled_uids =
        load_platform_message_disabled_uids(accounts_pool, &scoped_uids).await?;
    let message_rows =
        load_message_rows(live_pool, &scoped_uids, &platform_messages_disabled_uids).await?;
    let order_rows = load_order_rows(xianyu_pool, &scoped_uids).await?;

    for row in message_rows {
        let account = account_map
            .entry(row.uid.clone())
            .or_insert_with(|| AccountBusinessRow {
                uid: row.uid,
                nick: String::new(),
                status: "unknown".to_string(),
                ..Default::default()
            });
        account.today_messages += row.today_messages;
        account.unread_messages += row.unread_messages;
        account.unread_sessions += row.unread_sessions;
        account.last_message_at = account.last_message_at.max(row.last_message_at);
    }

    for row in order_rows {
        let account = account_map
            .entry(row.uid.clone())
            .or_insert_with(|| AccountBusinessRow {
                uid: row.uid,
                nick: String::new(),
                status: "unknown".to_string(),
                ..Default::default()
            });
        account.today_orders += 1;
        account.last_order_at = account.last_order_at.max(row.order_at);
        if is_wait_pay_order(&row.status, &row.status_text) {
            account.pending_pay_orders += 1;
        } else if is_wait_send_order(&row.status, &row.status_text) {
            account.wait_send_orders += 1;
        }
        if revenue_counts(&row.status, &row.status_text) {
            account.today_revenue += row.price;
        }
    }

    let mut account_rows = account_map.into_values().collect::<Vec<_>>();
    account_rows.sort_by(|left, right| {
        right
            .today_revenue
            .partial_cmp(&left.today_revenue)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.today_orders.cmp(&left.today_orders))
            .then_with(|| right.unread_messages.cmp(&left.unread_messages))
            .then_with(|| left.uid.cmp(&right.uid))
    });

    let today_messages = account_rows.iter().map(|row| row.today_messages).sum();
    let unread_messages = account_rows.iter().map(|row| row.unread_messages).sum();
    let unread_sessions = account_rows.iter().map(|row| row.unread_sessions).sum();
    let today_orders = account_rows.iter().map(|row| row.today_orders).sum();
    let today_revenue = round_money(account_rows.iter().map(|row| row.today_revenue).sum());
    let pending_pay_orders = account_rows.iter().map(|row| row.pending_pay_orders).sum();
    let wait_send_orders = account_rows.iter().map(|row| row.wait_send_orders).sum();

    let order_stage_counts = load_order_stage_counts(xianyu_pool, &scoped_uids).await?;
    let wait_receive_orders = order_stage_counts.wait_receive;
    let closed_orders = order_stage_counts.closed;

    let account_total = account_rows.len() as i64;
    let active_accounts = account_rows
        .iter()
        .filter(|row| row.status == "active")
        .count() as i64;
    let inactive_accounts = account_rows
        .iter()
        .filter(|row| row.status == "inactive")
        .count() as i64;
    let restricted_accounts = account_rows
        .iter()
        .filter(|row| row.status == "restricted")
        .count() as i64;
    let session_error_accounts = account_rows
        .iter()
        .filter(|row| {
            let status = row.status.to_ascii_lowercase();
            status == "session_error"
        })
        .count() as i64;
    let overdue_reply_sessions =
        load_overdue_reply_sessions(live_pool, &scoped_uids, &platform_messages_disabled_uids)
            .await
            .unwrap_or(0);
    let recent_events = load_recent_events(xianyu_pool, &account_rows).await?;
    let risk_items = build_risk_items(
        unread_messages,
        overdue_reply_sessions,
        wait_send_orders,
        pending_pay_orders,
        restricted_accounts,
        inactive_accounts,
        session_error_accounts,
    );
    let trend7d = load_trend7d(xianyu_pool, live_pool, &scoped_uids).await?;

    Ok(DashboardStats {
        date,
        generated_at,
        today_messages,
        unread_messages,
        unread_sessions,
        overdue_reply_sessions,
        today_orders,
        today_revenue,
        pending_pay_orders,
        wait_send_orders,
        wait_receive_orders,
        closed_orders,
        account_total,
        active_accounts,
        inactive_accounts,
        restricted_accounts,
        session_error_accounts,
        account_rows,
        recent_events,
        risk_items,
        trend7d,
    })
}

#[derive(Debug, Clone)]
struct OrderRow {
    uid: String,
    price: f64,
    status: String,
    status_text: String,
    order_at: f64,
}

#[derive(Debug, Clone, Default)]
struct MessageAggregateRow {
    uid: String,
    today_messages: i64,
    unread_messages: i64,
    unread_sessions: i64,
    last_message_at: i64,
}

#[derive(Debug, Clone, Default)]
struct OrderStageCounts {
    wait_receive: i64,
    closed: i64,
}

#[derive(Debug, Clone, Default)]
struct DataCenterOrderTotals {
    today_revenue: f64,
    today_orders: i64,
    wait_send_orders: i64,
    wait_send_earliest_at: f64,
    pending_pay_orders: i64,
    pending_pay_amount: f64,
    aftersale_orders: i64,
    closed_orders: i64,
}

async fn load_data_center_accounts(
    pool: &SqlitePool,
    owner_username: Option<&str>,
) -> Result<Vec<DataCenterAccountRow>, sqlx::Error> {
    let rows = if let Some(owner) = owner_username {
        sqlx::query(
            r#"
            SELECT uid, nick, status, extra, last_session_status, last_session_reason
            FROM accounts
            WHERE owner_username = ?
            ORDER BY updated_at DESC, created_at DESC, uid ASC
            "#,
        )
        .bind(owner)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT uid, nick, status, extra, last_session_status, last_session_reason
            FROM accounts
            ORDER BY updated_at DESC, created_at DESC, uid ASC
            "#,
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|row| {
            let uid = row.get::<String, _>("uid");
            let raw_nick = row.get::<String, _>("nick");
            let extra_raw = row.get::<String, _>("extra");
            let extra = serde_json::from_str::<Value>(&extra_raw).unwrap_or(Value::Null);
            let avatar = extra
                .get("avatar")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let status = row.get::<String, _>("status");
            let session_status = row.get::<String, _>("last_session_status");
            let session_reason = row.get::<String, _>("last_session_reason");
            let last_error = if session_reason.trim().is_empty() {
                session_status
            } else if session_status.trim().is_empty() {
                session_reason
            } else {
                format!("{session_status}: {session_reason}")
            };
            let nick = account_display_nick_from_extra(&uid, &raw_nick, &extra);
            DataCenterAccountRow {
                uid,
                nick,
                avatar,
                status,
                last_error,
                ..Default::default()
            }
        })
        .collect())
}

fn get_data_center_account_mut<'a>(
    accounts: &'a mut [DataCenterAccountRow],
    by_uid: &HashMap<String, usize>,
    uid: &str,
) -> Option<&'a mut DataCenterAccountRow> {
    by_uid
        .get(uid)
        .copied()
        .and_then(|idx| accounts.get_mut(idx))
}

async fn apply_item_metrics(
    pool: &SqlitePool,
    accounts: &mut Vec<DataCenterAccountRow>,
    by_uid: &mut HashMap<String, usize>,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            uid,
            COUNT(*) AS total_items,
            COALESCE(SUM(CASE WHEN online = 1 AND UPPER(COALESCE(status, '')) NOT LIKE '%DELETE%' THEN 1 ELSE 0 END), 0) AS online_items
        FROM account_items
        GROUP BY uid
        "#,
    )
    .fetch_all(pool)
    .await?;
    for row in rows {
        let uid = row.get::<String, _>("uid");
        let Some(account) = get_data_center_account_mut(accounts, by_uid, &uid) else {
            continue;
        };
        account.total_items = row.get::<i64, _>("total_items");
        account.online_items = row.get::<i64, _>("online_items");
    }
    Ok(())
}

async fn apply_order_metrics(
    pool: &SqlitePool,
    accounts: &mut Vec<DataCenterAccountRow>,
    by_uid: &mut HashMap<String, usize>,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT uid, price, status, status_text, gmt_create, created_at
        FROM account_orders
        "#,
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let uid = row.get::<String, _>("uid");
        let price = parse_money(&row.get::<String, _>("price"));
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        let Some(account) = get_data_center_account_mut(accounts, by_uid, &uid) else {
            continue;
        };
        account.last_order_at = account.last_order_at.max(order_at);
        if is_wait_pay_order(&status, &status_text) {
            account.pending_pay_orders += 1;
        } else if is_wait_send_order(&status, &status_text) {
            account.wait_send_orders += 1;
        }
        if revenue_counts(&status, &status_text) {
            account.total_orders += 1;
            account.total_revenue += price;
            if is_order_in_last_days(order_at, 7) {
                account.week_orders += 1;
                account.week_revenue += price;
            }
            if is_order_today(order_at) {
                account.today_orders += 1;
                account.today_revenue += price;
            }
        }
    }

    for account in accounts {
        account.today_revenue = round_money(account.today_revenue);
        account.week_revenue = round_money(account.week_revenue);
        account.total_revenue = round_money(account.total_revenue);
    }
    Ok(())
}

async fn apply_message_metrics(
    pool: &SqlitePool,
    accounts: &mut Vec<DataCenterAccountRow>,
    by_uid: &mut HashMap<String, usize>,
    platform_messages_disabled_uids: &HashSet<String>,
) -> Result<(), sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT
            uid,
            session_id,
            session_type,
            COALESCE(latest_version, 0) AS latest_version,
            COALESCE(latest_message_id, '') AS latest_message_id,
            unread_count,
            COALESCE(summary, '') AS summary,
            COALESCE(last_message_ts, 0) AS last_message_at
        FROM message_live_state
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut unread_by_uid: HashMap<String, (i64, i64, i64)> = HashMap::new();
    for row in rows {
        let uid = row.get::<String, _>("uid");
        let session_id = row.get::<String, _>("session_id");
        let session_type = row.get::<i64, _>("session_type");
        let latest_version = row.get::<i64, _>("latest_version");
        let latest_message_id = row.get::<String, _>("latest_message_id");
        let unread_count = effective_message_session_unread_count(
            pool,
            &uid,
            &session_id,
            session_type,
            latest_version,
            &latest_message_id,
            row.get::<i64, _>("unread_count"),
            row.get::<String, _>("summary").as_str(),
        )
        .await?;
        let summary = row.get::<String, _>("summary");
        let last_message_at = row.get::<i64, _>("last_message_at");
        let hidden_platform =
            platform_messages_disabled_uids.contains(&uid) && matches!(session_type, 3 | 6 | 53);
        let hidden_legacy = should_suppress_chat_unread(session_type, &summary);
        let entry = unread_by_uid.entry(uid).or_insert((0, 0, 0));
        if !hidden_platform && !hidden_legacy && unread_count > 0 {
            entry.0 += unread_count;
            entry.1 += 1;
        }
        entry.2 = entry.2.max(last_message_at);
    }

    for (uid, (unread_messages, unread_sessions, last_message_at)) in unread_by_uid {
        let Some(account) = get_data_center_account_mut(accounts, by_uid, &uid) else {
            continue;
        };
        account.unread_messages = unread_messages;
        account.unread_sessions = unread_sessions;
        account.last_message_at = last_message_at;
    }
    Ok(())
}

async fn effective_message_session_unread_count(
    pool: &SqlitePool,
    uid: &str,
    session_id: &str,
    session_type: i64,
    latest_version: i64,
    latest_message_id: &str,
    unread_count: i64,
    summary: &str,
) -> Result<i64, sqlx::Error> {
    if unread_count <= 0 || should_suppress_chat_unread(session_type, summary) {
        return Ok(unread_count.max(0));
    }
    if latest_message_id.trim().is_empty() && latest_version <= 0 {
        return Ok(unread_count.max(0));
    }

    let row = sqlx::query(
        r#"
        SELECT MIN(unread_count) AS min_unread
        FROM message_history_cache
        WHERE uid = ?
          AND session_id = ?
          AND is_self = 0
          AND unread_count >= 0
          AND (
                (
                    ? <> ''
                    AND (
                        message_id = ?
                        OR upstream_message_id = ?
                        OR ? LIKE '%' || message_id || '%'
                        OR ? LIKE '%' || upstream_message_id || '%'
                    )
                )
                OR (? > 0 AND version = ?)
          )
        "#,
    )
    .bind(uid)
    .bind(session_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_message_id)
    .bind(latest_version.max(0))
    .bind(latest_version.max(0))
    .fetch_one(pool)
    .await?;

    let min_unread = row.try_get::<Option<i64>, _>("min_unread").ok().flatten();
    Ok(effective_unread_count_from_history(
        unread_count,
        min_unread,
    ))
}

fn effective_unread_count_from_history(unread_count: i64, min_unread: Option<i64>) -> i64 {
    if min_unread == Some(0) {
        0
    } else {
        unread_count.max(0)
    }
}

async fn load_latest_message_directions(
    pool: &SqlitePool,
    account_uids: &HashSet<&str>,
) -> Result<HashMap<(String, String), bool>, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(account_uids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        WITH ranked_messages AS (
            SELECT
                history.uid,
                history.session_id,
                history.is_self,
                ROW_NUMBER() OVER (
                    PARTITION BY history.uid, history.session_id
                    ORDER BY
                        COALESCE(history.version, 0) DESC,
                        COALESCE(history.ts, 0) DESC,
                        history.rowid DESC
                ) AS latest_rank
            FROM message_history_cache AS history
            INNER JOIN message_live_state AS session
               ON session.uid = history.uid
               AND session.session_id = history.session_id
               AND session.session_type = 1
            WHERE session.uid IN ({placeholders})
        )
        SELECT uid, session_id, is_self
        FROM ranked_messages
        WHERE latest_rank = 1
        "#
    );
    let mut query = sqlx::query(&sql);
    for uid in account_uids {
        query = query.bind(*uid);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let is_self = row.try_get::<Option<i64>, _>("is_self").ok().flatten()?;
            Some((
                (
                    row.get::<String, _>("uid"),
                    row.get::<String, _>("session_id"),
                ),
                is_self != 0,
            ))
        })
        .collect())
}

async fn load_effective_unread_matches(
    pool: &SqlitePool,
    account_uids: &HashSet<&str>,
) -> Result<HashMap<(String, String), i64>, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(account_uids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT
            session.uid,
            session.session_id,
            MIN(history.unread_count) AS min_unread
        FROM message_live_state AS session
        INNER JOIN message_history_cache AS history
            ON history.uid = session.uid
           AND history.session_id = session.session_id
           AND history.is_self = 0
           AND history.unread_count >= 0
           AND (
                (
                    TRIM(COALESCE(session.latest_message_id, '')) <> ''
                    AND (
                        history.message_id = session.latest_message_id
                        OR history.upstream_message_id = session.latest_message_id
                        OR session.latest_message_id LIKE '%' || history.message_id || '%'
                        OR session.latest_message_id LIKE '%' || history.upstream_message_id || '%'
                    )
                )
                OR (
                    COALESCE(session.latest_version, 0) > 0
                    AND history.version = session.latest_version
                )
           )
        WHERE session.session_type = 1
          AND session.unread_count > 0
          AND session.uid IN ({placeholders})
        GROUP BY session.uid, session.session_id
        "#
    );
    let mut query = sqlx::query(&sql);
    for uid in account_uids {
        query = query.bind(*uid);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let min_unread = row.try_get::<Option<i64>, _>("min_unread").ok().flatten()?;
            Some((
                (
                    row.get::<String, _>("uid"),
                    row.get::<String, _>("session_id"),
                ),
                min_unread,
            ))
        })
        .collect())
}

async fn apply_sync_metrics(
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    accounts: &mut Vec<DataCenterAccountRow>,
    by_uid: &mut HashMap<String, usize>,
) -> Result<(), sqlx::Error> {
    let sync_rows = sqlx::query(
        r#"
        SELECT uid, kind, last_synced_at
        FROM account_sync_state
        WHERE kind IN ('items', 'orders')
        "#,
    )
    .fetch_all(xianyu_pool)
    .await?;
    for row in sync_rows {
        let uid = row.get::<String, _>("uid");
        let kind = row.get::<String, _>("kind");
        let Some(account) = get_data_center_account_mut(accounts, by_uid, &uid) else {
            continue;
        };
        if kind == "items" {
            account.items_synced_at = row.get::<f64, _>("last_synced_at");
        } else if kind == "orders" {
            account.orders_synced_at = row.get::<f64, _>("last_synced_at");
        }
    }

    let live_rows = sqlx::query(
        r#"
        SELECT uid, status, last_error
        FROM live_connection_state
        WHERE status != ''
        "#,
    )
    .fetch_all(live_pool)
    .await?;
    for row in live_rows {
        let uid = row.get::<String, _>("uid");
        let Some(account) = get_data_center_account_mut(accounts, by_uid, &uid) else {
            continue;
        };
        account.live_status = row.get::<String, _>("status");
        let last_error = row.get::<String, _>("last_error");
        if !last_error.trim().is_empty() {
            account.last_error = last_error;
        }
    }
    Ok(())
}

async fn load_data_center_order_totals(
    pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<DataCenterOrderTotals, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT uid, price, status, status_text, gmt_create, created_at FROM account_orders",
    )
    .fetch_all(pool)
    .await?;
    let mut totals = DataCenterOrderTotals::default();
    for row in rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let price = parse_money(&row.get::<String, _>("price"));
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        if is_wait_pay_order(&status, &status_text) {
            totals.pending_pay_orders += 1;
            totals.pending_pay_amount += price;
        } else if is_wait_send_order(&status, &status_text) {
            totals.wait_send_orders += 1;
            if totals.wait_send_earliest_at <= 0.0 || order_at < totals.wait_send_earliest_at {
                totals.wait_send_earliest_at = order_at;
            }
        }
        if is_aftersale_order(&status, &status_text) {
            totals.aftersale_orders += 1;
        }
        if is_closed_order(&status, &status_text) {
            totals.closed_orders += 1;
        }
        if revenue_counts(&status, &status_text) && is_order_today(order_at) {
            totals.today_orders += 1;
            totals.today_revenue += price;
        }
    }
    totals.today_revenue = round_money(totals.today_revenue);
    totals.pending_pay_amount = round_money(totals.pending_pay_amount);
    Ok(totals)
}

async fn load_data_center_queue(
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    accounts: &[DataCenterAccountRow],
) -> Result<Vec<DataCenterQueueItem>, sqlx::Error> {
    let account_uids = accounts
        .iter()
        .map(|row| row.uid.as_str())
        .collect::<HashSet<_>>();
    let nick_by_uid = accounts
        .iter()
        .map(|row| (row.uid.clone(), display_account_nick(&row.uid, &row.nick)))
        .collect::<HashMap<_, _>>();
    let mut queue = Vec::new();

    let order_rows = sqlx::query(
        r#"
        SELECT tid, uid, title, price, pic_url, status, status_text, buyer_nick, gmt_create, created_at, updated_at
        FROM account_orders
        WHERE
            UPPER(COALESCE(status, '')) LIKE '%WAIT_SELLER_SEND%'
            OR UPPER(COALESCE(status, '')) LIKE '%WAIT_SEND%'
            OR UPPER(COALESCE(status, '')) LIKE '%SELLER_TO_SEND%'
            OR UPPER(COALESCE(status, '')) LIKE '%WAIT_BUYER_PAY%'
            OR UPPER(COALESCE(status, '')) LIKE '%WAIT_PAY%'
            OR UPPER(COALESCE(status, '')) LIKE '%WAIT_BUYER_CONFIRM%'
            OR UPPER(COALESCE(status, '')) LIKE '%BUYER_TO_CONFIRM%'
            OR UPPER(COALESCE(status, '')) LIKE '%BUYER_CONFIRM_GOODS%'
            OR UPPER(COALESCE(status, '')) LIKE '%WAIT_RECEIVE%'
            OR UPPER(COALESCE(status, '')) LIKE '%CONSIGNED%'
            OR UPPER(COALESCE(status, '')) LIKE '%REFUND%'
            OR UPPER(COALESCE(status, '')) LIKE '%AFTERSALE%'
            OR UPPER(COALESCE(status, '')) LIKE '%AFTER_SALE%'
            OR COALESCE(status_text, '') LIKE '%待发货%'
            OR COALESCE(status_text, '') LIKE '%等待卖家发货%'
            OR COALESCE(status_text, '') LIKE '%待付款%'
            OR COALESCE(status_text, '') LIKE '%等待付款%'
            OR COALESCE(status_text, '') LIKE '%待收货%'
            OR COALESCE(status_text, '') LIKE '%确认收货%'
            OR COALESCE(status_text, '') LIKE '%已发货%'
            OR COALESCE(status_text, '') LIKE '%等待见面交易%'
            OR COALESCE(status_text, '') LIKE '%退款%'
            OR COALESCE(status_text, '') LIKE '%售后%'
        ORDER BY
            CASE
                WHEN UPPER(COALESCE(status, '')) LIKE '%WAIT_SELLER_SEND%' OR COALESCE(status_text, '') LIKE '%待发货%' THEN 0
                WHEN UPPER(COALESCE(status, '')) LIKE '%WAIT_BUYER_PAY%' OR COALESCE(status_text, '') LIKE '%待付款%' THEN 1
                ELSE 2
            END ASC,
            COALESCE(NULLIF(gmt_create, ''), '') DESC,
            created_at DESC
        "#,
    )
    .fetch_all(xianyu_pool)
    .await?;
    for row in order_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(uid.as_str()) {
            continue;
        }
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        let order_id = row.get::<String, _>("tid");
        let title = row.get::<String, _>("title");
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        queue.push(DataCenterQueueItem {
            id: format!("order:{order_id}"),
            kind: "order".to_string(),
            uid: uid.clone(),
            account_nick: nick_by_uid
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| display_account_nick(&uid, "")),
            title: if title.trim().is_empty() {
                "订单事项".to_string()
            } else {
                title
            },
            subtitle: status_text.clone(),
            customer: row.get::<String, _>("buyer_nick"),
            amount: parse_money(&row.get::<String, _>("price")),
            status: status.clone(),
            status_label: order_status_label(&status, &status_text),
            target_path: "/orders".to_string(),
            action_label: action_label_for_order_status(&status, &status_text),
            ts: nonzero_f64(order_at, row.get::<f64, _>("updated_at")),
            item_id: String::new(),
            item_pic: row.get::<String, _>("pic_url"),
            session_id: String::new(),
            order_id,
        });
    }

    let message_rows = sqlx::query(
        r#"
        SELECT
            uid,
            session_id,
            COALESCE(latest_version, 0) AS latest_version,
            COALESCE(latest_message_id, '') AS latest_message_id,
            item_id,
            item_title,
            item_pic,
            peer_nick,
            summary,
            unread_count,
            last_message_ts,
            updated_at
        FROM message_live_state
        WHERE session_type = 1
        ORDER BY
            CASE WHEN unread_count > 0 THEN 0 ELSE 1 END ASC,
            last_message_ts DESC,
            updated_at DESC
        "#,
    )
    .fetch_all(live_pool)
    .await?;
    let latest_message_directions =
        load_latest_message_directions(live_pool, &account_uids).await?;
    let effective_unread_matches = load_effective_unread_matches(live_pool, &account_uids).await?;
    for row in message_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(uid.as_str()) {
            continue;
        }
        let session_id = row.get::<String, _>("session_id");
        let item_title = row.get::<String, _>("item_title");
        let summary = row.get::<String, _>("summary");
        if should_suppress_chat_unread(1, &summary) {
            continue;
        }
        let direction_key = (uid.clone(), session_id.clone());
        match latest_message_directions.get(&direction_key) {
            Some(true) => continue,
            Some(false) => {}
            None => {
                let unread_count = row.get::<i64, _>("unread_count");
                let latest_version = row.get::<i64, _>("latest_version");
                let latest_message_id = row.get::<String, _>("latest_message_id");
                let effective_unread = if unread_count <= 0 {
                    0
                } else if latest_message_id.trim().is_empty() && latest_version <= 0 {
                    unread_count.max(0)
                } else {
                    effective_unread_count_from_history(
                        unread_count,
                        effective_unread_matches.get(&direction_key).copied(),
                    )
                };
                if effective_unread <= 0 {
                    continue;
                }
            }
        }
        queue.push(DataCenterQueueItem {
            id: format!("message:{uid}:{session_id}"),
            kind: "message".to_string(),
            uid: uid.clone(),
            account_nick: nick_by_uid
                .get(&uid)
                .cloned()
                .unwrap_or_else(|| display_account_nick(&uid, "")),
            title: if item_title.trim().is_empty() {
                "商品咨询".to_string()
            } else {
                item_title
            },
            subtitle: if summary.trim().is_empty() {
                "等待消息内容同步".to_string()
            } else {
                summary
            },
            customer: non_empty(row.get::<String, _>("peer_nick"), "客户未知"),
            amount: 0.0,
            status: "unread".to_string(),
            status_label: "未回复".to_string(),
            target_path: "/messages".to_string(),
            action_label: "回复".to_string(),
            ts: nonzero_f64(
                millis_to_seconds(row.get::<i64, _>("last_message_ts")),
                row.get::<f64, _>("updated_at"),
            ),
            item_id: row.get::<String, _>("item_id"),
            item_pic: row.get::<String, _>("item_pic"),
            session_id,
            order_id: String::new(),
        });
    }

    queue.sort_by(|left, right| {
        queue_priority(left)
            .cmp(&queue_priority(right))
            .then_with(|| {
                right
                    .ts
                    .partial_cmp(&left.ts)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    queue.truncate(24);
    Ok(queue)
}

fn build_share_range<F>(
    range: &str,
    label: &str,
    accounts: &[DataCenterAccountRow],
    value_for: F,
) -> DataCenterShareRange
where
    F: Fn(&DataCenterAccountRow) -> (f64, i64),
{
    let mut rows = accounts
        .iter()
        .map(|account| {
            let (amount, orders) = value_for(account);
            (account.uid.clone(), account.nick.clone(), amount, orders)
        })
        .filter(|(_, _, amount, orders)| *amount > 0.0 || *orders > 0)
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right
            .2
            .partial_cmp(&left.2)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.3.cmp(&left.3))
    });

    let total_amount = round_money(rows.iter().map(|row| row.2).sum());
    let total_orders = rows.iter().map(|row| row.3).sum();
    let mut segments = Vec::new();
    let mut other_amount = 0.0;
    let mut other_orders = 0;
    for (index, (uid, nick, amount, orders)) in rows.into_iter().enumerate() {
        if index < 3 {
            segments.push(DataCenterShareSegment {
                uid,
                nick,
                amount: round_money(amount),
                orders,
                percent: percent_of(amount, total_amount),
            });
        } else {
            other_amount += amount;
            other_orders += orders;
        }
    }
    if other_amount > 0.0 || other_orders > 0 {
        segments.push(DataCenterShareSegment {
            uid: "others".to_string(),
            nick: "其他账号".to_string(),
            amount: round_money(other_amount),
            orders: other_orders,
            percent: percent_of(other_amount, total_amount),
        });
    }

    DataCenterShareRange {
        range: range.to_string(),
        label: label.to_string(),
        total_amount,
        total_orders,
        segments,
    }
}

async fn load_seller_trend_bundle(
    xianyu_pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<DataCenterSellerTrendBundle, sqlx::Error> {
    if !has_table(xianyu_pool, "data_compass_daily_metrics").await? {
        return Ok(DataCenterSellerTrendBundle {
            week: unavailable_seller_trend_range("week", "7 天成交"),
            half_month: unavailable_seller_trend_range("halfMonth", "15 天成交"),
            month: unavailable_seller_trend_range("month", "30 天成交"),
        });
    }
    Ok(DataCenterSellerTrendBundle {
        week: load_seller_trend_range(xianyu_pool, account_uids, "week", "7 天成交", 7).await?,
        half_month: load_seller_trend_range(
            xianyu_pool,
            account_uids,
            "halfMonth",
            "15 天成交",
            15,
        )
        .await?,
        month: load_seller_trend_range(xianyu_pool, account_uids, "month", "30 天成交", 30).await?,
    })
}

async fn load_data_compass_sync_summary(
    xianyu_pool: &SqlitePool,
    active_account_uids: &HashSet<String>,
) -> Result<DataCenterDataCompassSummary, sqlx::Error> {
    let mut summary = DataCenterDataCompassSummary {
        status: "pending".to_string(),
        account_count: active_account_uids.len() as i64,
        ..Default::default()
    };
    if active_account_uids.is_empty() || !has_table(xianyu_pool, "data_compass_sync_state").await? {
        return Ok(summary);
    }

    let placeholders = std::iter::repeat("?")
        .take(active_account_uids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT uid, last_attempt_at, last_success_at, last_error FROM data_compass_sync_state WHERE uid IN ({placeholders}) ORDER BY last_attempt_at DESC, uid ASC"
    );
    let mut query = sqlx::query(&sql);
    for uid in active_account_uids {
        query = query.bind(uid);
    }

    for row in query.fetch_all(xianyu_pool).await? {
        let attempted_at = row.get::<f64, _>("last_attempt_at");
        let success_at = row.get::<f64, _>("last_success_at");
        let error = normalize_data_compass_error(&row.get::<String, _>("last_error"));
        if success_at > 0.0 {
            summary.success_count += 1;
            summary.last_success_at = summary.last_success_at.max(success_at);
        }
        if attempted_at > success_at || !error.is_empty() {
            summary.failed_count += 1;
            if summary.error_summary.is_empty() && !error.is_empty() {
                summary.error_summary = error;
            }
        }
    }

    summary.status = if summary.failed_count > 0 {
        "failed".to_string()
    } else if summary.success_count > 0 {
        "ready".to_string()
    } else {
        "pending".to_string()
    };
    Ok(summary)
}

fn normalize_data_compass_error(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn unavailable_seller_trend_range(range: &str, label: &str) -> DataCenterSellerTrendRange {
    DataCenterSellerTrendRange {
        range: range.to_string(),
        label: label.to_string(),
        ..Default::default()
    }
}

async fn load_seller_trend_range(
    xianyu_pool: &SqlitePool,
    account_uids: &HashSet<String>,
    range: &str,
    label: &str,
    days: i64,
) -> Result<DataCenterSellerTrendRange, sqlx::Error> {
    let clamped_days = days.clamp(1, 90);
    let start_offset = clamped_days - 1;
    let day_sql = format!(
        r#"
        WITH RECURSIVE days(n, d) AS (
            SELECT {start_offset}, date('now', '-{start_offset} day', 'localtime')
            UNION ALL
            SELECT n - 1, date(d, '+1 day') FROM days WHERE n > 0
        )
        SELECT d FROM days ORDER BY d ASC
        "#
    );
    let dates = sqlx::query_scalar::<_, String>(&day_sql)
        .fetch_all(xianyu_pool)
        .await?;
    let mut by_date = dates
        .iter()
        .map(|date| {
            (
                date.clone(),
                DataCenterTrendPoint {
                    date: date.clone(),
                    label: short_date_label(date),
                    ..Default::default()
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let rows = sqlx::query(
        r#"
        SELECT uid, stat_date, pay_amount, pay_order_count, source_synced_at
        FROM data_compass_daily_metrics
        WHERE stat_date >= date('now', ?, 'localtime')
        "#,
    )
    .bind(format!("-{} day", start_offset))
    .fetch_all(xianyu_pool)
    .await?;

    let mut available = false;
    let mut synced_at: f64 = 0.0;
    for row in rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let date = row.get::<String, _>("stat_date");
        let Some(point) = by_date.get_mut(&date) else {
            continue;
        };
        available = true;
        point.amount += row.get::<f64, _>("pay_amount");
        point.orders += row.get::<i64, _>("pay_order_count");
        synced_at = synced_at.max(row.get::<f64, _>("source_synced_at"));
    }
    let mut points = dates
        .into_iter()
        .filter_map(|date| by_date.remove(&date))
        .collect::<Vec<_>>();
    for point in &mut points {
        point.amount = round_money(point.amount);
    }
    Ok(DataCenterSellerTrendRange {
        available,
        range: range.to_string(),
        label: label.to_string(),
        total_amount: round_money(points.iter().map(|point| point.amount).sum()),
        total_orders: points.iter().map(|point| point.orders).sum(),
        points,
        synced_at,
    })
}

async fn load_trend_range(
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    account_uids: &HashSet<String>,
    range: &str,
    label: &str,
    days: i64,
) -> Result<DataCenterTrendRange, sqlx::Error> {
    let clamped_days = days.clamp(1, 90);
    let start_offset = clamped_days - 1;
    let day_sql = format!(
        r#"
        WITH RECURSIVE days(n, d) AS (
            SELECT {start_offset}, date('now', '-{start_offset} day', 'localtime')
            UNION ALL
            SELECT n - 1, date(d, '+1 day') FROM days WHERE n > 0
        )
        SELECT d FROM days ORDER BY d ASC
        "#
    );
    let dates = sqlx::query_scalar::<_, String>(&day_sql)
        .fetch_all(live_pool)
        .await?;
    let mut by_date = dates
        .iter()
        .map(|date| {
            (
                date.clone(),
                DataCenterTrendPoint {
                    date: date.clone(),
                    label: short_date_label(date),
                    ..Default::default()
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let message_sql = format!(
        r#"
        SELECT uid, date(ts / 1000, 'unixepoch', 'localtime') AS d, COUNT(*) AS c
        FROM message_history_cache
        WHERE is_self = 0
          AND ts > 0
          AND date(ts / 1000, 'unixepoch', 'localtime') >= date('now', '-{start_offset} day', 'localtime')
        GROUP BY uid, d
        "#
    );
    for row in sqlx::query(&message_sql).fetch_all(live_pool).await? {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        if let Some(point) = by_date.get_mut(&row.get::<String, _>("d")) {
            point.messages = row.get::<i64, _>("c");
        }
    }

    let order_rows = sqlx::query(
        "SELECT uid, price, status, status_text, gmt_create, created_at FROM account_orders WHERE created_at > 0 OR gmt_create != ''",
    )
    .fetch_all(xianyu_pool)
    .await?;
    for row in order_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        if !is_order_in_last_days(order_at, clamped_days) {
            continue;
        }
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        if !revenue_counts(&status, &status_text) {
            continue;
        }
        let date = local_date_string(order_at);
        if let Some(point) = by_date.get_mut(&date) {
            point.orders += 1;
            point.amount += parse_money(&row.get::<String, _>("price"));
        }
    }

    let mut points = dates
        .into_iter()
        .filter_map(|date| by_date.remove(&date))
        .collect::<Vec<_>>();
    points.sort_by(|left, right| left.date.cmp(&right.date));
    for point in &mut points {
        point.amount = round_money(point.amount);
    }
    Ok(DataCenterTrendRange {
        range: range.to_string(),
        label: label.to_string(),
        total_amount: round_money(points.iter().map(|point| point.amount).sum()),
        total_orders: points.iter().map(|point| point.orders).sum(),
        points,
    })
}

async fn load_total_trend_range(
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<DataCenterTrendRange, sqlx::Error> {
    let mut by_month: HashMap<String, DataCenterTrendPoint> = HashMap::new();

    let message_rows = sqlx::query(
        r#"
        SELECT uid, strftime('%Y-%m', ts / 1000, 'unixepoch', 'localtime') AS m, COUNT(*) AS c
        FROM message_history_cache
        WHERE is_self = 0 AND ts > 0
        GROUP BY uid, m
        "#,
    )
    .fetch_all(live_pool)
    .await?;
    for row in message_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let month = row.get::<String, _>("m");
        let point = by_month
            .entry(month.clone())
            .or_insert_with(|| DataCenterTrendPoint {
                date: month.clone(),
                label: month_label(&month),
                ..Default::default()
            });
        point.messages = row.get::<i64, _>("c");
    }

    let order_rows = sqlx::query(
        "SELECT uid, price, status, status_text, gmt_create, created_at FROM account_orders WHERE created_at > 0 OR gmt_create != ''",
    )
    .fetch_all(xianyu_pool)
    .await?;
    for row in order_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        if !revenue_counts(&status, &status_text) {
            continue;
        }
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        let month = local_month_string(order_at);
        let point = by_month
            .entry(month.clone())
            .or_insert_with(|| DataCenterTrendPoint {
                date: month.clone(),
                label: month_label(&month),
                ..Default::default()
            });
        point.orders += 1;
        point.amount += parse_money(&row.get::<String, _>("price"));
    }

    let total_amount = round_money(by_month.values().map(|point| point.amount).sum());
    let total_orders = by_month.values().map(|point| point.orders).sum();
    let mut points = by_month.into_values().collect::<Vec<_>>();
    points.sort_by(|left, right| left.date.cmp(&right.date));
    if points.len() > 12 {
        points = points.split_off(points.len() - 12);
    }
    for point in &mut points {
        point.amount = round_money(point.amount);
    }
    Ok(DataCenterTrendRange {
        range: "total".to_string(),
        label: "总成交".to_string(),
        total_amount,
        total_orders,
        points,
    })
}

async fn load_account_rows(
    pool: &SqlitePool,
    owner_username: Option<&str>,
) -> Result<Vec<AccountBusinessRow>, sqlx::Error> {
    let rows = if let Some(owner) = owner_username {
        sqlx::query(
            r#"
            SELECT uid, nick, status
            FROM accounts
            WHERE owner_username = ?
            ORDER BY updated_at DESC, uid ASC
            "#,
        )
        .bind(owner)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            r#"
            SELECT uid, nick, status
            FROM accounts
            ORDER BY updated_at DESC, uid ASC
            "#,
        )
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(|row| AccountBusinessRow {
            uid: row.get::<String, _>("uid"),
            nick: row.get::<String, _>("nick"),
            status: row.get::<String, _>("status"),
            ..Default::default()
        })
        .collect())
}

async fn load_message_rows(
    pool: &SqlitePool,
    account_uids: &HashSet<String>,
    platform_messages_disabled_uids: &HashSet<String>,
) -> Result<Vec<MessageAggregateRow>, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        r#"
        SELECT
            s.uid AS uid,
            s.session_id AS session_id,
            s.session_type AS session_type,
            COALESCE(s.latest_version, 0) AS latest_version,
            COALESCE(s.latest_message_id, '') AS latest_message_id,
            s.unread_count AS unread_count,
            COALESCE(s.summary, '') AS summary,
            COALESCE(s.last_message_ts, 0) AS last_message_at,
            COALESCE(today.c, 0) AS today_messages,
            COALESCE(s.last_message_ts, 0) AS aggregate_last_message_at
        FROM message_live_state s
        LEFT JOIN (
            SELECT uid, COUNT(*) AS c
            FROM message_history_cache
            WHERE is_self = 0
              AND date(ts / 1000, 'unixepoch', 'localtime') = date('now', 'localtime')
            GROUP BY uid
        ) today ON today.uid = s.uid
        "#,
    )
    .fetch_all(pool)
    .await?;

    let mut by_uid: HashMap<String, MessageAggregateRow> = HashMap::new();
    for row in rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let entry = by_uid
            .entry(uid.clone())
            .or_insert_with(|| MessageAggregateRow {
                uid: uid.clone(),
                today_messages: row.get::<i64, _>("today_messages"),
                unread_messages: 0,
                unread_sessions: 0,
                last_message_at: 0,
            });
        entry.today_messages = row.get::<i64, _>("today_messages");
        entry.last_message_at = entry
            .last_message_at
            .max(row.get::<i64, _>("aggregate_last_message_at"));

        let session_id = row.get::<String, _>("session_id");
        let session_type = row.get::<i64, _>("session_type");
        let summary = row.get::<String, _>("summary");
        let unread_count = effective_message_session_unread_count(
            pool,
            &uid,
            &session_id,
            session_type,
            row.get::<i64, _>("latest_version"),
            row.get::<String, _>("latest_message_id").as_str(),
            row.get::<i64, _>("unread_count"),
            &summary,
        )
        .await?;
        let hidden_platform =
            platform_messages_disabled_uids.contains(&uid) && matches!(session_type, 3 | 6 | 53);
        let hidden_legacy = should_suppress_chat_unread(session_type, &summary);
        if !hidden_platform && !hidden_legacy && unread_count > 0 {
            entry.unread_messages += unread_count;
            entry.unread_sessions += 1;
        }
    }

    Ok(by_uid.into_values().collect())
}

async fn load_order_rows(
    pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<Vec<OrderRow>, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        r#"
        SELECT uid, price, status, status_text, gmt_create, created_at
        FROM account_orders
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| OrderRow {
            uid: row.get::<String, _>("uid"),
            price: parse_money(&row.get::<String, _>("price")),
            status: row.get::<String, _>("status"),
            status_text: row.get::<String, _>("status_text"),
            order_at: order_business_ts(
                &row.get::<String, _>("gmt_create"),
                row.get::<f64, _>("created_at"),
            ),
        })
        .filter(|row| account_uids.contains(&row.uid))
        .filter(|row| is_order_today(row.order_at))
        .collect())
}

async fn load_order_stage_counts(
    pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<OrderStageCounts, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(OrderStageCounts::default());
    }

    let rows = sqlx::query("SELECT uid, status, status_text FROM account_orders")
        .fetch_all(pool)
        .await?;
    let mut counts = OrderStageCounts::default();
    for row in rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        if is_wait_receive_order(&status, &status_text) {
            counts.wait_receive += 1;
        }
        if is_closed_order(&status, &status_text) {
            counts.closed += 1;
        }
    }
    Ok(counts)
}

async fn load_session_error_accounts(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)
        FROM accounts
        WHERE UPPER(COALESCE(last_session_status, '')) LIKE '%SESSION%'
           OR UPPER(COALESCE(last_session_reason, '')) LIKE '%SESSION%'
           OR UPPER(COALESCE(last_session_reason, '')) LIKE '%TOKEN%'
           OR UPPER(COALESCE(last_session_reason, '')) LIKE '%NOT_LOGIN%'
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(count)
}

async fn load_overdue_reply_sessions(
    pool: &SqlitePool,
    account_uids: &HashSet<String>,
    platform_messages_disabled_uids: &HashSet<String>,
) -> Result<i64, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(0);
    }

    let rows = sqlx::query(
        r#"
        SELECT uid, session_type, COALESCE(summary, '') AS summary
        FROM message_live_state
        WHERE unread_count > 0
          AND last_message_ts > 0
          AND last_message_ts < ((unixepoch('subsec') - 600) * 1000)
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            let uid = row.get::<String, _>("uid");
            if !account_uids.contains(&uid) {
                return false;
            }
            let session_type = row.get::<i64, _>("session_type");
            let summary = row.get::<String, _>("summary");
            !(platform_messages_disabled_uids.contains(&uid) && matches!(session_type, 3 | 6 | 53))
                && !should_suppress_chat_unread(session_type, &summary)
        })
        .count() as i64)
}

async fn load_platform_message_disabled_uids(
    accounts_pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<HashSet<String>, sqlx::Error> {
    if account_uids.is_empty() {
        return Ok(HashSet::new());
    }

    if !has_table(accounts_pool, "ai_reply_settings").await?
        || !has_columns(
            accounts_pool,
            "ai_reply_settings",
            &["cookie_id", "platform_messages_enabled"],
        )
        .await?
    {
        return Ok(HashSet::new());
    }

    let settings_rows = sqlx::query(
        "SELECT COALESCE(cookie_id, '') AS cookie_id, COALESCE(platform_messages_enabled, 1) AS platform_messages_enabled FROM ai_reply_settings",
    )
    .fetch_all(accounts_pool)
    .await?;
    let settings_by_cookie = settings_rows
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("cookie_id"),
                row.get::<i64, _>("platform_messages_enabled") != 0,
            )
        })
        .collect::<HashMap<_, _>>();

    let account_rows =
        sqlx::query("SELECT uid, COALESCE(owner_username, '') AS owner_username FROM accounts")
            .fetch_all(accounts_pool)
            .await?;
    let mut disabled = HashSet::new();
    for row in account_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let owner_username = row.get::<String, _>("owner_username");
        let cookie_id = user_platform_scope_key(&owner_username);
        if matches!(settings_by_cookie.get(&cookie_id), Some(false)) {
            disabled.insert(uid);
        }
    }
    Ok(disabled)
}

fn user_platform_scope_key(owner_username: &str) -> String {
    let owner = owner_username.trim();
    if owner.is_empty() {
        "__global__".to_string()
    } else {
        format!("__global__::user::{owner}")
    }
}

async fn load_recent_events(
    xianyu_pool: &SqlitePool,
    account_rows: &[AccountBusinessRow],
) -> Result<Vec<DashboardEvent>, sqlx::Error> {
    let nick_by_uid = account_rows
        .iter()
        .map(|row| (row.uid.clone(), row.nick.clone()))
        .collect::<HashMap<_, _>>();
    let mut events = Vec::new();

    let order_rows = sqlx::query(
        r#"
        SELECT tid, uid, title, price, status, status_text, buyer_nick, gmt_create, created_at
        FROM account_orders
        ORDER BY COALESCE(NULLIF(gmt_create, ''), '') DESC, created_at DESC
        "#,
    )
    .fetch_all(xianyu_pool)
    .await?;
    for row in order_rows {
        let uid = row.get::<String, _>("uid");
        if !nick_by_uid.contains_key(&uid) {
            continue;
        }
        let title = row.get::<String, _>("title");
        let buyer_nick = row.get::<String, _>("buyer_nick");
        let status_text = row.get::<String, _>("status_text");
        let price = parse_money(&row.get::<String, _>("price"));
        let account_nick = nick_by_uid.get(&uid).cloned().unwrap_or_default();
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        let buyer_text = if buyer_nick.trim().is_empty() {
            "买家未知".to_string()
        } else {
            format!("买家 {buyer_nick}")
        };
        events.push(DashboardEvent {
            kind: "order".to_string(),
            order_id: row.get::<String, _>("tid"),
            title: if price > 0.0 {
                format!("新订单 ¥{price:.2}")
            } else {
                "新订单".to_string()
            },
            subtitle: if title.trim().is_empty() {
                format!("{buyer_text} · {status_text}")
            } else {
                format!("{title} · {buyer_text} · {status_text}")
            },
            uid: uid.clone(),
            nick: account_nick,
            buyer_nick,
            status: row.get::<String, _>("status"),
            amount: price,
            ts: order_at,
        });
    }

    events.sort_by(|left, right| {
        right
            .ts
            .partial_cmp(&left.ts)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    events.truncate(8);
    Ok(events)
}

fn build_risk_items(
    unread_messages: i64,
    overdue_reply_sessions: i64,
    wait_send_orders: i64,
    pending_pay_orders: i64,
    restricted_accounts: i64,
    inactive_accounts: i64,
    session_error_accounts: i64,
) -> Vec<DashboardRiskItem> {
    let mut items = Vec::new();
    if unread_messages > 0 {
        items.push(DashboardRiskItem {
            kind: "message".to_string(),
            title: format!("{unread_messages} 条客户消息待处理"),
            subtitle: if overdue_reply_sessions > 0 {
                format!("{overdue_reply_sessions} 个会话超过 10 分钟未回复")
            } else {
                "及时回复可减少成交流失".to_string()
            },
            severity: if overdue_reply_sessions > 0 {
                "high"
            } else {
                "medium"
            }
            .to_string(),
            target_path: "/messages".to_string(),
        });
    }
    if wait_send_orders > 0 {
        items.push(DashboardRiskItem {
            kind: "order".to_string(),
            title: format!("{wait_send_orders} 单待发货"),
            subtitle: "先处理已付款订单".to_string(),
            severity: "high".to_string(),
            target_path: "/orders".to_string(),
        });
    }
    if pending_pay_orders > 0 {
        items.push(DashboardRiskItem {
            kind: "order".to_string(),
            title: format!("{pending_pay_orders} 单等待付款"),
            subtitle: "可跟进买家或调整价格".to_string(),
            severity: "medium".to_string(),
            target_path: "/orders".to_string(),
        });
    }
    let abnormal_accounts = restricted_accounts + inactive_accounts + session_error_accounts;
    if abnormal_accounts > 0 {
        items.push(DashboardRiskItem {
            kind: "account".to_string(),
            title: format!("{abnormal_accounts} 个账号需要处理"),
            subtitle: format!(
                "受限 {restricted_accounts} · 离线 {inactive_accounts} · 会话异常 {session_error_accounts}"
            ),
            severity: "high".to_string(),
            target_path: "/accounts".to_string(),
        });
    }
    items
}

async fn load_trend7d(
    xianyu_pool: &SqlitePool,
    live_pool: &SqlitePool,
    account_uids: &HashSet<String>,
) -> Result<Vec<DashboardTrendPoint>, sqlx::Error> {
    let mut dates = sqlx::query_scalar::<_, String>(
        r#"
        WITH RECURSIVE days(n, d) AS (
            SELECT 6, date('now', '-6 day', 'localtime')
            UNION ALL
            SELECT n - 1, date(d, '+1 day') FROM days WHERE n > 0
        )
        SELECT d FROM days ORDER BY d ASC
        "#,
    )
    .fetch_all(live_pool)
    .await?;
    if dates.is_empty() {
        dates.push("今日".to_string());
    }

    let mut by_date = dates
        .iter()
        .map(|date| {
            (
                date.clone(),
                DashboardTrendPoint {
                    date: date.clone(),
                    ..Default::default()
                },
            )
        })
        .collect::<HashMap<_, _>>();

    let message_rows = sqlx::query(
        r#"
        SELECT uid, date(ts / 1000, 'unixepoch', 'localtime') AS d, COUNT(*) AS c
        FROM message_history_cache
        WHERE is_self = 0
          AND date(ts / 1000, 'unixepoch', 'localtime') >= date('now', '-6 day', 'localtime')
        GROUP BY uid, d
        "#,
    )
    .fetch_all(live_pool)
    .await?;
    for row in message_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        if let Some(point) = by_date.get_mut(&row.get::<String, _>("d")) {
            point.messages = row.get::<i64, _>("c");
        }
    }

    let order_rows = sqlx::query(
        r#"
        SELECT uid, price, status, status_text, gmt_create, created_at
        FROM account_orders
        "#,
    )
    .fetch_all(xianyu_pool)
    .await?;
    for row in order_rows {
        let uid = row.get::<String, _>("uid");
        if !account_uids.contains(&uid) {
            continue;
        }
        let status = row.get::<String, _>("status");
        let status_text = row.get::<String, _>("status_text");
        if !revenue_counts(&status, &status_text) {
            continue;
        }
        let order_at = order_business_ts(
            &row.get::<String, _>("gmt_create"),
            row.get::<f64, _>("created_at"),
        );
        if !is_order_in_last_days(order_at, 7) {
            continue;
        }
        if let Some(point) = by_date.get_mut(&local_date_string(order_at)) {
            point.orders += 1;
            point.revenue += parse_money(&row.get::<String, _>("price"));
        }
    }

    let mut points = dates
        .into_iter()
        .filter_map(|date| by_date.remove(&date))
        .collect::<Vec<_>>();
    points.sort_by(|left, right| left.date.cmp(&right.date));
    for point in &mut points {
        point.revenue = round_money(point.revenue);
    }
    Ok(points)
}

async fn today_local_date(pool: &SqlitePool) -> Option<String> {
    sqlx::query_scalar::<_, String>("SELECT date('now', 'localtime')")
        .fetch_one(pool)
        .await
        .ok()
}

fn is_wait_pay_order(status: &str, status_text: &str) -> bool {
    let s = status.trim().to_ascii_uppercase();
    let t = status_text.trim();
    s.contains("WAIT_BUYER_PAY")
        || s == "WAIT_PAY"
        || t.contains("待付款")
        || t.contains("等待付款")
}

fn is_wait_send_order(status: &str, status_text: &str) -> bool {
    let s = status.trim().to_ascii_uppercase();
    let t = status_text.trim();
    s.contains("WAIT_SELLER_SEND")
        || s.contains("WAIT_SEND")
        || s.contains("SELLER_TO_SEND")
        || t.contains("待发货")
        || t.contains("等待卖家发货")
}

fn is_wait_receive_order(status: &str, status_text: &str) -> bool {
    let s = status.trim().to_ascii_uppercase();
    let t = status_text.trim();
    s.contains("WAIT_BUYER_CONFIRM")
        || s.contains("BUYER_TO_CONFIRM")
        || s.contains("BUYER_CONFIRM_GOODS")
        || s.contains("WAIT_RECEIVE")
        || s.contains("CONSIGNED")
        || t.contains("待收货")
        || t.contains("确认收货")
        || t.contains("已发货")
        || t.contains("等待见面交易")
}

fn is_closed_order(status: &str, status_text: &str) -> bool {
    let s = status.trim().to_ascii_uppercase();
    let t = status_text.trim();
    s.contains("CLOSE") || s.contains("CANCEL") || t.contains("关闭") || t.contains("取消")
}

fn revenue_counts(status: &str, status_text: &str) -> bool {
    !is_closed_order(status, status_text) && !is_wait_pay_order(status, status_text)
}

fn is_aftersale_order(status: &str, status_text: &str) -> bool {
    let s = status.trim().to_ascii_uppercase();
    let t = status_text.trim();
    s.contains("REFUND")
        || s.contains("AFTERSALE")
        || s.contains("AFTER_SALE")
        || t.contains("退款")
        || t.contains("售后")
}

fn order_status_label(status: &str, status_text: &str) -> String {
    let text = status_text.trim();
    if !text.is_empty() {
        return text.to_string();
    }
    if is_wait_send_order(status, status_text) {
        return "待发货".to_string();
    }
    if is_wait_pay_order(status, status_text) {
        return "待付款".to_string();
    }
    if is_wait_receive_order(status, status_text) {
        return "待收货".to_string();
    }
    if is_closed_order(status, status_text) {
        return "已关闭".to_string();
    }
    if revenue_counts(status, status_text) {
        return "交易中".to_string();
    }
    "待处理".to_string()
}

fn action_label_for_order_status(status: &str, status_text: &str) -> String {
    if is_wait_send_order(status, status_text) {
        "发货".to_string()
    } else if is_wait_pay_order(status, status_text) {
        "催付".to_string()
    } else if is_wait_receive_order(status, status_text) {
        "跟进".to_string()
    } else {
        "详情".to_string()
    }
}

fn account_has_session_error(row: &DataCenterAccountRow) -> bool {
    let status = row.status.to_ascii_lowercase();
    let error = row.last_error.to_ascii_uppercase();
    status == "session_error"
        || error.contains("SESSION")
        || error.contains("TOKEN")
        || error.contains("NOT_LOGIN")
}

fn display_account_nick(uid: &str, nick: &str) -> String {
    let trimmed = nick.trim();
    if trimmed.is_empty()
        || trimmed == uid
        || trimmed.eq_ignore_ascii_case("unknown")
        || trimmed.chars().all(|ch| ch.is_ascii_digit())
    {
        "未识别账号".to_string()
    } else {
        trimmed.to_string()
    }
}

fn account_display_nick_from_extra(uid: &str, nick: &str, extra: &Value) -> String {
    let direct = display_account_nick(uid, nick);
    if direct != "未识别账号" {
        return direct;
    }
    for key in [
        "fishNick",
        "nick",
        "nickName",
        "nickname",
        "displayName",
        "userNick",
        "userName",
        "sellerNick",
    ] {
        if let Some(value) = extra.get(key).and_then(Value::as_str) {
            let candidate = display_account_nick(uid, value);
            if candidate != "未识别账号" {
                return candidate;
            }
        }
    }
    "未识别账号".to_string()
}

fn queue_priority(item: &DataCenterQueueItem) -> i32 {
    if item.kind == "message" && item.status == "unread" {
        return 0;
    }
    if item.kind == "order" && item.status_label.contains("待发货") {
        return 1;
    }
    if item.kind == "order" && item.status_label.contains("待付款") {
        return 2;
    }
    3
}

fn nonzero_f64(primary: f64, fallback: f64) -> f64 {
    if primary > 0.0 {
        primary
    } else {
        fallback
    }
}

fn order_business_ts(gmt_create: &str, fallback: f64) -> f64 {
    parse_local_datetime_to_unix_seconds(gmt_create).unwrap_or(fallback)
}

fn parse_local_datetime_to_unix_seconds(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.len() < 19 {
        return None;
    }
    let year = trimmed.get(0..4)?.parse::<i64>().ok()?;
    let month = trimmed.get(5..7)?.parse::<u32>().ok()?;
    let day = trimmed.get(8..10)?.parse::<u32>().ok()?;
    let hour = trimmed.get(11..13)?.parse::<i64>().ok()?;
    let minute = trimmed.get(14..16)?.parse::<i64>().ok()?;
    let second = trimmed.get(17..19)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second - 8 * 3_600) as f64)
}

fn non_empty(value: String, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value
    }
}

fn percent_of(value: f64, total: f64) -> f64 {
    if total <= 0.0 {
        0.0
    } else {
        ((value / total) * 1000.0).round() / 10.0
    }
}

fn millis_to_seconds(value: i64) -> f64 {
    if value <= 0 {
        0.0
    } else if value > 10_000_000_000 {
        value as f64 / 1000.0
    } else {
        value as f64
    }
}

fn is_order_today(ts: f64) -> bool {
    local_date_string(ts) == local_date_string(unix_seconds_now())
}

fn is_order_in_last_days(ts: f64, days: i64) -> bool {
    if ts <= 0.0 {
        return false;
    }
    let now = unix_seconds_now();
    ts >= now - (days.max(1) as f64 * 86_400.0)
}

fn local_date_string(ts: f64) -> String {
    let secs = ts as i64 + 8 * 3600;
    let days_since_epoch = secs.div_euclid(86400);
    let (year, month, day) = civil_from_days(days_since_epoch);
    format!("{year:04}-{month:02}-{day:02}")
}

fn local_month_string(ts: f64) -> String {
    let secs = ts as i64 + 8 * 3600;
    let days_since_epoch = secs.div_euclid(86400);
    let (year, month, _) = civil_from_days(days_since_epoch);
    format!("{year:04}-{month:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719468;
    let era = z.div_euclid(146097);
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m, d)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = adjusted_year.div_euclid(400);
    let yoe = adjusted_year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let month_phase = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * month_phase + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn short_date_label(date: &str) -> String {
    let parts = date.split('-').collect::<Vec<_>>();
    if parts.len() >= 3 {
        format!(
            "{}/{}",
            parts[1].parse::<i64>().unwrap_or(0),
            parts[2].parse::<i64>().unwrap_or(0)
        )
    } else {
        date.to_string()
    }
}

fn month_label(month: &str) -> String {
    month
        .split('-')
        .nth(1)
        .and_then(|value| value.parse::<i64>().ok())
        .map(|value| format!("{value}月"))
        .unwrap_or_else(|| month.to_string())
}

fn parse_money(raw: &str) -> f64 {
    let mut text = raw.trim().replace(',', "");
    if text.is_empty() {
        return 0.0;
    }
    text = text.replace('¥', "");
    text = text.replace("￥", "");
    text = text.replace("元", "");
    text.trim().parse::<f64>().unwrap_or(0.0)
}

fn round_money(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn unix_seconds_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use sqlx::SqlitePool;

    use crate::models::runtime::{StartupDependencyProbe, StartupDependencyStatus};

    use super::{
        collect_dashboard_stats_from_pools, collect_data_center_stats_from_pools,
        load_data_center_owner_scope_keys, load_data_compass_sync_summary,
        load_delivery_automation, load_reply_automation, load_scheduled_automation,
        load_seller_trend_bundle, local_date_string, parse_money, patch_sync_dependency_status,
        unix_seconds_now,
    };

    #[tokio::test]
    async fn seller_trend_reads_persisted_data_compass_metrics_per_account() {
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE data_compass_daily_metrics (
                uid TEXT NOT NULL,
                stat_date TEXT NOT NULL,
                pay_amount REAL NOT NULL,
                pay_order_count INTEGER NOT NULL,
                refund_amount REAL NOT NULL,
                refund_order_count INTEGER NOT NULL,
                source_synced_at REAL NOT NULL,
                PRIMARY KEY (uid, stat_date)
            )
            "#,
        )
        .execute(&xianyu)
        .await
        .unwrap();
        let today = local_date_string(unix_seconds_now());
        for (uid, amount, orders) in [("u1", 12.5, 2_i64), ("u2", 7.5, 1_i64)] {
            sqlx::query("INSERT INTO data_compass_daily_metrics VALUES (?, ?, ?, ?, 0, 0, 100)")
                .bind(uid)
                .bind(&today)
                .bind(amount)
                .bind(orders)
                .execute(&xianyu)
                .await
                .unwrap();
        }

        let all = load_seller_trend_bundle(
            &xianyu,
            &HashSet::from(["u1".to_string(), "u2".to_string()]),
        )
        .await
        .unwrap();
        let one = load_seller_trend_bundle(&xianyu, &HashSet::from(["u1".to_string()]))
            .await
            .unwrap();
        let all_payload = serde_json::to_value(all).unwrap();
        let one_payload = serde_json::to_value(one).unwrap();

        assert_eq!(all_payload["week"]["available"], true);
        assert_eq!(all_payload["week"]["totalAmount"], 20.0);
        assert_eq!(all_payload["week"]["totalOrders"], 3);
        assert_eq!(one_payload["week"]["totalAmount"], 12.5);
        assert_eq!(one_payload["month"]["points"].as_array().unwrap().len(), 30);
    }

    #[tokio::test]
    async fn data_compass_sync_summary_reports_failed_attempts_for_the_current_accounts() {
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE data_compass_sync_state (
                uid TEXT PRIMARY KEY,
                last_attempt_at REAL NOT NULL DEFAULT 0,
                last_success_at REAL NOT NULL DEFAULT 0,
                last_error TEXT NOT NULL DEFAULT ''
            )
            "#,
        )
        .execute(&xianyu)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO data_compass_sync_state VALUES ('u1', 100, 100, ''), ('u2', 200, 0, 'non-success upstream ret: FAIL_SYS_ILLEGAL_ACCESS'), ('outside-scope', 200, 0, 'ignored')",
        )
        .execute(&xianyu)
        .await
        .unwrap();

        let summary = load_data_compass_sync_summary(
            &xianyu,
            &HashSet::from(["u1".to_string(), "u2".to_string()]),
        )
        .await
        .unwrap();
        let payload = serde_json::to_value(summary).unwrap();

        assert_eq!(payload["status"], "failed");
        assert_eq!(payload["accountCount"], 2);
        assert_eq!(payload["successCount"], 1);
        assert_eq!(payload["failedCount"], 1);
        assert_eq!(payload["lastSuccessAt"], 100.0);
        assert_eq!(
            payload["errorSummary"],
            "non-success upstream ret: FAIL_SYS_ILLEGAL_ACCESS"
        );
    }

    #[tokio::test]
    async fn dashboard_stats_prioritizes_trade_message_and_account_metrics() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        let stats = collect_dashboard_stats_from_pools(&accounts, &xianyu, &live, None)
            .await
            .unwrap();
        let payload = serde_json::to_value(stats).unwrap();

        assert_eq!(payload["todayOrders"], 4);
        assert_eq!(payload["todayRevenue"], 280.5);
        assert_eq!(payload["waitSendOrders"], 1);
        assert_eq!(payload["pendingPayOrders"], 1);
        assert_eq!(payload["unreadMessages"], 5);
        assert_eq!(payload["unreadSessions"], 2);
        assert_eq!(payload["accountTotal"], 3);
        assert_eq!(payload["activeAccounts"], 2);
        assert_eq!(payload["accountRows"][0]["uid"], "u1");
        assert_eq!(payload["accountRows"][0]["todayRevenue"], 280.5);
        let risks = payload["riskItems"].as_array().unwrap();
        assert!(risks.iter().any(|risk| risk["kind"] == "message"));
        assert!(risks.iter().any(|risk| risk["kind"] == "order"));
        assert!(risks.iter().any(|risk| risk["kind"] == "account"));
        let recent_events = payload["recentEvents"].as_array().unwrap();
        assert!(!recent_events.is_empty());
        assert!(recent_events.iter().all(|event| event["kind"] == "order"));
        assert_eq!(payload["trend7d"].as_array().unwrap().len(), 7);
    }

    #[tokio::test]
    async fn data_center_stats_builds_real_modules_from_existing_tables() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        let stats = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, None)
            .await
            .unwrap();
        let payload = serde_json::to_value(stats).unwrap();

        assert_eq!(payload["totals"]["todayOrders"], 2);
        assert_eq!(payload["totals"]["todayRevenue"], 280.5);
        assert_eq!(payload["totals"]["waitSendOrders"], 1);
        assert_eq!(payload["totals"]["pendingPayOrders"], 1);
        assert_eq!(payload["totals"]["pendingPayAmount"], 99.0);
        assert_eq!(payload["totals"]["unreadMessages"], 5);
        assert_eq!(payload["totals"]["onlineItems"], 2);
        assert_eq!(payload["totals"]["activeAccounts"], 2);
        assert_eq!(payload["totals"]["sessionErrorAccounts"], 1);

        let account = &payload["accounts"][0];
        assert_eq!(account["uid"], "u1");
        assert_eq!(account["todayRevenue"], 280.5);
        assert_eq!(account["weekOrders"], 2);
        assert_eq!(account["onlineItems"], 1);
        assert_eq!(account["liveStatus"], "connected");

        assert_eq!(payload["share"]["day"]["totalAmount"], 280.5);
        assert_eq!(payload["share"]["day"]["segments"][0]["uid"], "u1");
        assert_eq!(payload["trend"]["total"]["totalAmount"], 1880.5);
        assert_eq!(payload["trend"]["total"]["totalOrders"], 3);
        assert_eq!(
            payload["trend"]["week"]["points"].as_array().unwrap().len(),
            7
        );
        let queue = payload["queue"].as_array().unwrap();
        assert!(queue
            .iter()
            .any(|item| item["kind"] == "message" && item["status"] == "unread"));
        assert!(!queue.iter().any(|item| item["orderId"] == "o2"));
        assert!(!queue.iter().any(|item| item["orderId"] == "o4"));
        assert_eq!(payload["sync"]["liveOnline"], 1);
        assert_eq!(payload["sync"]["signServiceOk"], false);
        assert_eq!(
            payload["sync"]["signServiceStatusCode"],
            serde_json::Value::Null
        );
        assert_eq!(payload["sync"]["licenseServiceOk"], false);
        assert_eq!(
            payload["sync"]["licenseServiceStatusCode"],
            serde_json::Value::Null
        );
    }

    #[tokio::test]
    async fn data_center_ignores_metric_rows_for_accounts_not_in_account_list() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        sqlx::query(
            r#"
            INSERT INTO account_orders (
                tid, uid, title, price, pic_url, status, status_text, buyer_nick,
                gmt_create, created_at, updated_at
            ) VALUES (
                'legacy-order', 'legacy-uid', '旧账号订单', '88', '',
                'WAIT_SELLER_SEND_GOODS', '待发货', '旧客户',
                datetime('now', 'localtime'), unixepoch('subsec'), unixepoch('subsec')
            )
            "#,
        )
        .execute(&xianyu)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO live_connection_state (uid, status, last_error, updated_at) VALUES ('legacy-uid', 'connected', '', unixepoch('subsec'))",
        )
        .execute(&live)
        .await
        .unwrap();

        let stats = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, None)
            .await
            .unwrap();
        let payload = serde_json::to_value(stats).unwrap();

        let account_uids = payload["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["uid"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert!(!account_uids.contains(&"legacy-uid"));
        assert!(payload["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["nick"] != "未识别账号"));
        assert!(!payload["queue"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["uid"] == "legacy-uid"));
    }

    #[tokio::test]
    async fn data_center_automation_is_scoped_to_the_current_owner() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        sqlx::query("CREATE TABLE delivery_rules (cookie_id TEXT, enabled INTEGER)")
            .execute(&accounts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE ai_reply_settings (cookie_id TEXT, reply_mode TEXT DEFAULT 'off', ai_enabled INTEGER, auto_reply_enabled INTEGER, auto_ship_enabled INTEGER, platform_messages_enabled INTEGER DEFAULT 1)")
            .execute(&accounts).await.unwrap();
        sqlx::query("CREATE TABLE automation_schedules (owner_username TEXT, enabled INTEGER, last_status TEXT, next_run_at REAL)")
            .execute(&xianyu).await.unwrap();

        for (cookie_id, enabled) in [
            ("__global__::user::user-a", 1_i64),
            ("__global__::user::user-b", 1_i64),
            ("u1", 1_i64),
        ] {
            sqlx::query("INSERT INTO delivery_rules (cookie_id, enabled) VALUES (?, ?)")
                .bind(cookie_id)
                .bind(enabled)
                .execute(&accounts)
                .await
                .unwrap();
        }
        for (cookie_id, reply_mode, ai_enabled, auto_reply_enabled, auto_ship_enabled) in [
            ("__global__::user::user-a", "ai", 1_i64, 0_i64, 1_i64),
            ("__global__::user::user-b", "default", 0_i64, 1_i64, 1_i64),
            ("u1", "keyword", 0_i64, 0_i64, 1_i64),
        ] {
            sqlx::query("INSERT INTO ai_reply_settings (cookie_id, reply_mode, ai_enabled, auto_reply_enabled, auto_ship_enabled) VALUES (?, ?, ?, ?, ?)")
                .bind(cookie_id).bind(reply_mode).bind(ai_enabled).bind(auto_reply_enabled).bind(auto_ship_enabled).execute(&accounts).await.unwrap();
        }
        for (owner, enabled, status, next_run_at) in [
            ("user-a", 1_i64, "running", 100.0_f64),
            ("user-a", 1_i64, "failed", 200.0_f64),
            ("user-a", 0_i64, "failed", 400.0_f64),
            ("user-b", 1_i64, "running", 300.0_f64),
        ] {
            sqlx::query("INSERT INTO automation_schedules (owner_username, enabled, last_status, next_run_at) VALUES (?, ?, ?, ?)")
                .bind(owner).bind(enabled).bind(status).bind(next_run_at).execute(&xianyu).await.unwrap();
        }

        let stats = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, Some("user-a"))
            .await
            .unwrap();
        let payload = serde_json::to_value(stats).unwrap();
        assert_eq!(payload["automation"]["delivery"]["enabled"], 1);
        assert_eq!(payload["automation"]["delivery"]["pending"], 2);
        assert!(payload["automation"]["delivery"]["running"].is_null());
        assert!(payload["automation"]["delivery"]["errors"].is_null());
        assert_eq!(payload["automation"]["scheduledTasks"]["enabled"], 2);
        assert_eq!(payload["automation"]["scheduledTasks"]["running"], 1);
        assert_eq!(payload["automation"]["scheduledTasks"]["pending"], 2);
        assert_eq!(payload["automation"]["scheduledTasks"]["errors"], 1);
        assert_eq!(payload["automation"]["replies"]["enabled"], 1);
    }

    #[tokio::test]
    async fn reply_automation_supports_reply_modes_and_legacy_flags_for_accounts_only() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE ai_reply_settings (cookie_id TEXT, reply_mode TEXT, ai_enabled INTEGER, auto_reply_enabled INTEGER)",
        )
        .execute(&accounts)
        .await
        .unwrap();
        for (cookie_id, reply_mode, ai_enabled, auto_reply_enabled) in [
            ("mode-default", "default", 0_i64, 0_i64),
            ("mode-keyword", "keyword", 0_i64, 0_i64),
            ("mode-ai", "ai", 0_i64, 0_i64),
            ("legacy-ai", "off", 1_i64, 0_i64),
            ("legacy-auto", "off", 0_i64, 1_i64),
            ("disabled", "off", 0_i64, 0_i64),
            ("__global__::user::user-a", "ai", 1_i64, 1_i64),
        ] {
            sqlx::query("INSERT INTO ai_reply_settings (cookie_id, reply_mode, ai_enabled, auto_reply_enabled) VALUES (?, ?, ?, ?)")
                .bind(cookie_id)
                .bind(reply_mode)
                .bind(ai_enabled)
                .bind(auto_reply_enabled)
                .execute(&accounts)
                .await
                .unwrap();
        }
        let account_uids = [
            "mode-default",
            "mode-keyword",
            "mode-ai",
            "legacy-ai",
            "legacy-auto",
            "disabled",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<HashSet<_>>();

        let metric = load_reply_automation(&accounts, &account_uids)
            .await
            .unwrap();

        assert_eq!(metric.enabled, Some(5));
    }

    #[tokio::test]
    async fn legacy_owner_scope_keeps_account_uids_and_all_matching_global_keys() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        sqlx::query(
            "INSERT INTO accounts (uid, owner_username, nick) VALUES ('legacy-empty-owner', '', '旧账号')",
        )
        .execute(&accounts)
        .await
        .unwrap();
        let account_uids = ["u1", "u2", "legacy-empty-owner"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();

        let keys = load_data_center_owner_scope_keys(&accounts, None, &account_uids)
            .await
            .unwrap();

        for expected in [
            "u1",
            "u2",
            "legacy-empty-owner",
            "__global__::user::user-a",
            "__global__::user::user-b",
            "__global__",
        ] {
            assert!(keys.contains(expected), "missing scope key {expected}");
        }
        assert!(!keys.contains("u3"));
        assert!(!keys.contains("__global__::user::admin"));
    }

    #[tokio::test]
    async fn unavailable_automation_sources_are_null_instead_of_zero() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let account_uids = HashSet::from(["u1".to_string()]);
        let owner_keys = HashSet::from(["u1".to_string(), "__global__::user::user-a".to_string()]);

        let delivery = load_delivery_automation(&accounts, &account_uids, &owner_keys)
            .await
            .unwrap();
        let replies = load_reply_automation(&accounts, &account_uids)
            .await
            .unwrap();
        let schedules = load_scheduled_automation(&xianyu, Some("user-a"))
            .await
            .unwrap();
        assert_eq!(delivery.enabled, None);
        assert_eq!(delivery.pending, None);
        assert_eq!(replies.enabled, None);
        assert_eq!(schedules.enabled, None);
        assert_eq!(schedules.running, None);
        assert_eq!(schedules.pending, None);
        assert_eq!(schedules.errors, None);

        sqlx::query("CREATE TABLE delivery_rules (cookie_id TEXT, enabled INTEGER)")
            .execute(&accounts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE ai_reply_settings (cookie_id TEXT, unrelated INTEGER)")
            .execute(&accounts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE automation_schedules (owner_username TEXT, enabled INTEGER, last_status TEXT)")
            .execute(&xianyu)
            .await
            .unwrap();

        let delivery = load_delivery_automation(&accounts, &account_uids, &owner_keys)
            .await
            .unwrap();
        let replies = load_reply_automation(&accounts, &account_uids)
            .await
            .unwrap();
        let schedules = load_scheduled_automation(&xianyu, Some("user-a"))
            .await
            .unwrap();
        assert_eq!(delivery.enabled, None);
        assert_eq!(delivery.pending, Some(0));
        assert_eq!(replies.enabled, None);
        assert_eq!(schedules.enabled, None);
        assert_eq!(schedules.running, None);
        assert_eq!(schedules.pending, None);
        assert_eq!(schedules.errors, None);
    }

    #[tokio::test]
    async fn data_center_tolerates_incomplete_platform_message_settings_columns() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        for create_table in [
            "CREATE TABLE ai_reply_settings (cookie_id TEXT)",
            "CREATE TABLE ai_reply_settings (platform_messages_enabled INTEGER)",
        ] {
            sqlx::query(create_table).execute(&accounts).await.unwrap();

            let stats =
                collect_data_center_stats_from_pools(&accounts, &xianyu, &live, Some("user-a"))
                    .await
                    .expect("incomplete optional settings schema must not fail data center stats");
            let payload = serde_json::to_value(stats).unwrap();
            assert_eq!(payload["totals"]["accountTotal"], 1);
            assert_eq!(payload["totals"]["unreadMessages"], 3);

            sqlx::query("DROP TABLE ai_reply_settings")
                .execute(&accounts)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn data_center_ignores_null_ai_settings_cookie_id() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        sqlx::query(
            r#"
            CREATE TABLE ai_reply_settings (
                cookie_id TEXT PRIMARY KEY,
                reply_mode TEXT,
                ai_enabled INTEGER,
                auto_reply_enabled INTEGER,
                auto_ship_enabled INTEGER,
                platform_messages_enabled INTEGER
            )
            "#,
        )
        .execute(&accounts)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO ai_reply_settings (
                cookie_id, reply_mode, ai_enabled, auto_reply_enabled,
                auto_ship_enabled, platform_messages_enabled
            ) VALUES
                (NULL, 'ai', 1, 1, 1, 0),
                ('u1', 'default', 0, 0, 1, 1)
            "#,
        )
        .execute(&accounts)
        .await
        .unwrap();

        let stats = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, Some("user-a"))
            .await
            .expect("NULL optional settings scope must not fail data center stats");
        let payload = serde_json::to_value(stats).unwrap();

        assert_eq!(payload["totals"]["unreadMessages"], 3);
        assert_eq!(payload["automation"]["delivery"]["enabled"], 1);
        assert_eq!(payload["automation"]["replies"]["enabled"], 1);
    }

    #[tokio::test]
    async fn data_center_stats_respects_owner_scope() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        let stats = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, Some("user-a"))
            .await
            .unwrap();
        let payload = serde_json::to_value(stats).unwrap();

        let account_uids = payload["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["uid"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(account_uids, vec!["u1"]);
        assert_eq!(payload["totals"]["accountTotal"], 1);
        assert_eq!(payload["totals"]["unreadMessages"], 3);
        assert_eq!(payload["totals"]["pendingPayOrders"], 0);
        assert!(payload["queue"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["uid"] == "u1"));
        let week_message_total = payload["trend"]["week"]["points"]
            .as_array()
            .unwrap()
            .iter()
            .map(|point| point["messages"].as_i64().unwrap_or_default())
            .sum::<i64>();
        assert_eq!(week_message_total, 1);
    }

    #[tokio::test]
    async fn stats_ignore_platform_notification_unread_when_owner_disables_them() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;
        setup_platform_message_settings(&accounts).await;

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, unread_count, item_id, item_title, item_pic,
                peer_nick, summary, last_message_ts, updated_at
            ) VALUES
                ('u1', 'sys-1', 3, 9, '', '系统通知', '', '通知中心', '系统消息', CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')),
                ('u1', 'fav-1', 6, 4, '', '互动消息', '', '通知中心', '互动提醒', CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')),
                ('u1', 'chat-tip-1', 1, 12, '', '提醒会话', '', '卡券俱乐部', '快给ta一个评价吧～', CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')),
                ('u2', 'sys-2', 3, 7, '', '系统通知', '', '通知中心', '别家的系统消息', CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec'))
            "#,
        )
        .execute(&live)
        .await
        .unwrap();
        sqlx::query(
            r#"
            UPDATE message_live_state
            SET last_message_ts = CAST((unixepoch('subsec') - 1800) * 1000 AS INTEGER)
            WHERE uid = 'u1' AND session_id IN ('s1', 'sys-1', 'fav-1', 'chat-tip-1')
            "#,
        )
        .execute(&live)
        .await
        .unwrap();

        let dashboard =
            collect_dashboard_stats_from_pools(&accounts, &xianyu, &live, Some("user-a"))
                .await
                .unwrap();
        let dashboard_payload = serde_json::to_value(dashboard).unwrap();
        assert_eq!(dashboard_payload["unreadMessages"], 3);
        assert_eq!(dashboard_payload["unreadSessions"], 1);

        let data_center =
            collect_data_center_stats_from_pools(&accounts, &xianyu, &live, Some("user-a"))
                .await
                .unwrap();
        let data_center_payload = serde_json::to_value(data_center).unwrap();
        assert_eq!(data_center_payload["totals"]["unreadMessages"], 3);
        assert_eq!(data_center_payload["totals"]["unreadSessions"], 1);
        assert_eq!(data_center_payload["totals"]["overdueReplySessions"], 1);
        let queue = data_center_payload["queue"].as_array().unwrap();
        assert!(!queue
            .iter()
            .any(|item| item["id"] == "message:u1:chat-tip-1"));
    }

    #[tokio::test]
    async fn data_center_queue_keeps_read_but_unanswered_inbound_session() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                item_id, item_title, item_pic, peer_nick, summary, last_message_ts, updated_at
            ) VALUES (
                'u1', 'stale-read', 1, 200, 'stale-msg', 0,
                'item-stale', '二手教材', '', '老客户', '这条在官方已经读掉',
                CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')
            )
            "#,
        )
        .execute(&live)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO message_history_cache (
                uid, session_id, message_id, upstream_message_id, version, ts, is_self, unread_count
            ) VALUES (
                'u1', 'stale-read', 'stale-msg', 'stale-msg.PNM', 200, 200, 0, 0
            )
            "#,
        )
        .execute(&live)
        .await
        .unwrap();

        let dashboard = collect_dashboard_stats_from_pools(&accounts, &xianyu, &live, None)
            .await
            .unwrap();
        let dashboard_payload = serde_json::to_value(dashboard).unwrap();
        assert_eq!(dashboard_payload["unreadMessages"], 5);
        assert_eq!(dashboard_payload["unreadSessions"], 2);
        assert_eq!(dashboard_payload["accountRows"][0]["uid"], "u1");
        assert_eq!(dashboard_payload["accountRows"][0]["unreadMessages"], 3);
        assert_eq!(dashboard_payload["accountRows"][0]["unreadSessions"], 1);

        let data_center = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, None)
            .await
            .unwrap();
        let data_center_payload = serde_json::to_value(data_center).unwrap();
        assert_eq!(data_center_payload["totals"]["unreadMessages"], 5);
        assert_eq!(data_center_payload["totals"]["unreadSessions"], 2);
        let queue = data_center_payload["queue"].as_array().unwrap();
        assert!(queue.iter().any(|item| {
            item["id"] == "message:u1:stale-read" && item["statusLabel"] == "未回复"
        }));
    }

    #[tokio::test]
    async fn data_center_queue_excludes_replied_sessions_and_falls_back_without_history() {
        let accounts = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let xianyu = SqlitePool::connect("sqlite::memory:").await.unwrap();
        let live = SqlitePool::connect("sqlite::memory:").await.unwrap();
        setup_accounts(&accounts).await;
        setup_orders(&xianyu).await;
        setup_messages(&live).await;

        sqlx::query(
            r#"
            INSERT INTO message_live_state (
                uid, session_id, session_type, latest_version, latest_message_id, unread_count,
                item_id, item_title, peer_nick, summary, last_message_ts, updated_at
            ) VALUES
                ('u1', 'explicit-reply', 1, 301, 'reply-301', 5,
                 'item-reply', '已回复商品', '客户甲', '卖家已经回复',
                 CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')),
                ('u1', 'no-direction-unread', 1, 0, '', 2,
                 'item-fallback', '无历史商品', '客户乙', '尚未同步历史',
                 CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')),
                ('u1', 'no-direction-read', 1, 0, '', 0,
                 'item-fallback-read', '无历史已读商品', '客户丙', '没有待处理未读',
                 CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec'))
            "#,
        )
        .execute(&live)
        .await
        .unwrap();
        for (message_id, version, is_self) in [
            ("incoming-300", 300_i64, 0_i64),
            ("reply-301", 301_i64, 1_i64),
        ] {
            sqlx::query(
                r#"
                INSERT INTO message_history_cache (
                    uid, session_id, message_id, upstream_message_id, version, ts, is_self, unread_count
                ) VALUES (
                    'u1', 'explicit-reply', ?, ?, ?, ?, ?, 0
                )
                "#,
            )
            .bind(message_id)
            .bind(format!("{message_id}.PNM"))
            .bind(version)
            .bind(version)
            .bind(is_self)
            .execute(&live)
            .await
            .unwrap();
        }

        let stats = collect_data_center_stats_from_pools(&accounts, &xianyu, &live, None)
            .await
            .unwrap();
        let payload = serde_json::to_value(stats).unwrap();
        let queue = payload["queue"].as_array().unwrap();

        assert!(!queue
            .iter()
            .any(|item| item["id"] == "message:u1:explicit-reply"));
        assert!(queue
            .iter()
            .any(|item| item["id"] == "message:u1:no-direction-unread"));
        assert!(!queue
            .iter()
            .any(|item| item["id"] == "message:u1:no-direction-read"));
    }

    #[test]
    fn data_center_sync_runtime_status_uses_dependency_snapshot() {
        let mut sync = super::DataCenterSyncSummary::default();
        let dependencies = StartupDependencyStatus {
            sign_service: StartupDependencyProbe {
                ok: true,
                status_code: Some(200),
                ..StartupDependencyProbe::default()
            },
            license_service: StartupDependencyProbe {
                ok: false,
                status_code: Some(503),
                ..StartupDependencyProbe::default()
            },
            ..StartupDependencyStatus::default()
        };

        patch_sync_dependency_status(&mut sync, &dependencies);

        assert!(sync.sign_service_ok);
        assert_eq!(sync.sign_service_status_code, Some(200));
        assert!(!sync.license_service_ok);
        assert_eq!(sync.license_service_status_code, Some(503));
    }

    #[test]
    fn parse_money_accepts_yuan_symbols_and_commas() {
        assert_eq!(parse_money("¥1,234.50"), 1234.5);
        assert_eq!(parse_money("38元"), 38.0);
        assert_eq!(parse_money("bad"), 0.0);
    }

    async fn setup_accounts(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE accounts (
                uid TEXT PRIMARY KEY,
                owner_username TEXT DEFAULT '',
                nick TEXT DEFAULT '',
                status TEXT DEFAULT 'active',
                extra TEXT DEFAULT '{}',
                last_session_status TEXT DEFAULT '',
                last_session_reason TEXT DEFAULT '',
                created_at REAL DEFAULT 0,
                updated_at REAL DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        for (uid, owner, nick, status, session_status, session_reason) in [
            ("u1", "user-a", "测试用户甲", "active", "", ""),
            ("u2", "user-b", "旧时光书屋", "active", "", ""),
            (
                "u3",
                "admin",
                "敲厉害的糯糯学姐",
                "inactive",
                "SESSION_EXPIRED",
                "TOKEN_EXPIRED",
            ),
        ] {
            sqlx::query(
                "INSERT INTO accounts (uid, owner_username, nick, status, extra, last_session_status, last_session_reason, created_at, updated_at) VALUES (?, ?, ?, ?, '{}', ?, ?, unixepoch('subsec'), unixepoch('subsec'))",
            )
            .bind(uid)
            .bind(owner)
            .bind(nick)
            .bind(status)
            .bind(session_status)
            .bind(session_reason)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn setup_orders(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE account_orders (
                tid TEXT NOT NULL,
                uid TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                price TEXT NOT NULL DEFAULT '',
                pic_url TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT '',
            status_text TEXT NOT NULL DEFAULT '',
            buyer_nick TEXT NOT NULL DEFAULT '',
            gmt_create TEXT NOT NULL DEFAULT '',
            created_at REAL NOT NULL DEFAULT 0,
            updated_at REAL NOT NULL DEFAULT 0,
            PRIMARY KEY (uid, tid)
        )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE account_items (
                item_id TEXT NOT NULL,
                uid TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'ON_SALE',
                online INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (uid, item_id)
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE account_sync_state (
                uid TEXT NOT NULL,
                kind TEXT NOT NULL,
                last_synced_at REAL NOT NULL DEFAULT 0,
                last_ok INTEGER NOT NULL DEFAULT 0,
                last_reason TEXT NOT NULL DEFAULT '',
                total_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (uid, kind)
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        for (tid, uid, title, price, status, status_text, buyer_nick, created_at) in [
            (
                "o1",
                "u1",
                "硬盘",
                "180.50",
                "WAIT_SELLER_SEND_GOODS",
                "待发货",
                "买家A",
            ),
            (
                "o2",
                "u1",
                "露营车",
                "100",
                "TRADE_SUCCESS",
                "交易成功",
                "买家B",
            ),
            (
                "o3",
                "u2",
                "论文修改",
                "99",
                "WAIT_BUYER_PAY",
                "等待买家付款",
                "买家C",
            ),
            (
                "o4",
                "u2",
                "门票",
                "200",
                "TRADE_CLOSED",
                "交易关闭",
                "买家D",
            ),
        ]
        .into_iter()
        .map(
            |(tid, uid, title, price, status, status_text, buyer_nick)| {
                (
                    tid,
                    uid,
                    title,
                    price,
                    status,
                    status_text,
                    buyer_nick,
                    "unixepoch('subsec')".to_string(),
                )
            },
        )
        .chain(std::iter::once((
            "o-old",
            "u1",
            "老款 MacBook",
            "1600",
            "TRADE_SUCCESS",
            "交易成功",
            "买家E",
            "unixepoch('subsec') - 400 * 86400".to_string(),
        ))) {
            let sql = format!(
                r#"
                INSERT INTO account_orders (tid, uid, title, price, pic_url, status, status_text, buyer_nick, gmt_create, created_at, updated_at)
                VALUES (?, ?, ?, ?, '', ?, ?, ?, '', {created_at}, unixepoch('subsec'))
                "#
            );
            sqlx::query(&sql)
                .bind(tid)
                .bind(uid)
                .bind(title)
                .bind(price)
                .bind(status)
                .bind(status_text)
                .bind(buyer_nick)
                .execute(pool)
                .await
                .unwrap();
        }
        for (uid, item_id, online) in [("u1", "i1", 1), ("u1", "i2", 0), ("u2", "i3", 1)] {
            sqlx::query("INSERT INTO account_items (uid, item_id, online) VALUES (?, ?, ?)")
                .bind(uid)
                .bind(item_id)
                .bind(online)
                .execute(pool)
                .await
                .unwrap();
        }
        for (uid, kind) in [("u1", "items"), ("u1", "orders"), ("u2", "items")] {
            sqlx::query(
                "INSERT INTO account_sync_state (uid, kind, last_synced_at, last_ok, total_count) VALUES (?, ?, unixepoch('subsec'), 1, 1)",
            )
            .bind(uid)
            .bind(kind)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn setup_messages(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE message_live_state (
                uid TEXT NOT NULL,
                session_id TEXT NOT NULL,
                session_type INTEGER DEFAULT 1,
                latest_version INTEGER DEFAULT 0,
                latest_message_id TEXT DEFAULT '',
                unread_count INTEGER DEFAULT 0,
                item_id TEXT DEFAULT '',
                item_title TEXT DEFAULT '',
                item_pic TEXT DEFAULT '',
                peer_nick TEXT DEFAULT '',
                summary TEXT DEFAULT '',
                last_message_ts INTEGER DEFAULT 0,
                updated_at REAL DEFAULT 0,
                PRIMARY KEY (uid, session_id)
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE live_connection_state (
                uid TEXT PRIMARY KEY,
                status TEXT DEFAULT '',
                last_error TEXT DEFAULT '',
                updated_at REAL DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE message_history_cache (
                uid TEXT NOT NULL,
                session_id TEXT NOT NULL,
                message_id TEXT DEFAULT '',
                upstream_message_id TEXT DEFAULT '',
                version INTEGER DEFAULT 0,
                ts INTEGER DEFAULT 0,
                is_self INTEGER DEFAULT 0,
                unread_count INTEGER DEFAULT -1,
                PRIMARY KEY (uid, session_id, version, message_id)
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        for (uid, session_id, unread, peer, summary) in [
            ("u1", "s1", 3, "客户A", "还在吗"),
            ("u1", "s2", 0, "客户B", "已付款"),
            ("u2", "s3", 2, "客户C", "怎么收费"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO message_live_state (
                    uid, session_id, latest_version, latest_message_id, unread_count,
                    item_id, item_title, item_pic, peer_nick, summary, last_message_ts, updated_at
                )
                VALUES (
                    ?, ?, 1, ?, ?, 'item-1', '希捷企业硬盘', '', ?, ?,
                    CAST((unixepoch('subsec') * 1000) AS INTEGER), unixepoch('subsec')
                )
                "#,
            )
            .bind(uid)
            .bind(session_id)
            .bind(format!("{session_id}-latest"))
            .bind(unread)
            .bind(peer)
            .bind(summary)
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO live_connection_state (uid, status, last_error, updated_at) VALUES ('u1', 'connected', '', unixepoch('subsec'))",
        )
        .execute(pool)
        .await
        .unwrap();
        for (uid, session_id, message_id, upstream_message_id, version, is_self, unread_count) in [
            ("u1", "s1", "m1", "m1.PNM", 1, 0, 1),
            ("u1", "s1", "m2", "m2.PNM", 2, 1, 0),
            ("u2", "s3", "m3", "m3.PNM", 1, 0, 1),
        ] {
            sqlx::query(
                r#"
                INSERT INTO message_history_cache (
                    uid, session_id, message_id, upstream_message_id, version, ts, is_self, unread_count
                )
                VALUES (?, ?, ?, ?, ?, CAST((unixepoch('subsec') * 1000) AS INTEGER), ?, ?)
                "#,
            )
            .bind(uid)
            .bind(session_id)
            .bind(message_id)
            .bind(upstream_message_id)
            .bind(version)
            .bind(is_self)
            .bind(unread_count)
            .execute(pool)
            .await
            .unwrap();
        }
    }

    async fn setup_platform_message_settings(pool: &SqlitePool) {
        sqlx::query(
            r#"
            CREATE TABLE ai_reply_settings (
                cookie_id TEXT PRIMARY KEY,
                platform_messages_enabled INTEGER DEFAULT 1,
                updated_at REAL DEFAULT 0
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO ai_reply_settings (cookie_id, platform_messages_enabled, updated_at) VALUES ('__global__::user::user-a', 0, unixepoch('subsec'))",
        )
        .execute(pool)
        .await
        .unwrap();
    }
}
