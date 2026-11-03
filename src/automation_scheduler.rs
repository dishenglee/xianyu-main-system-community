use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::Row;
use tokio::{
    sync::watch,
    time::{self, MissedTickBehavior},
};
use tracing::warn;

use crate::{
    automation_raw_report::{emit_automation_raw_report, AutomationRawReport},
    routes::market::{coin, exposure, sync_items_to_db},
    state::AppState,
    storage::{NewOperationJob, NewOperationJobItem, NewOperationLog, StorageError},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSchedule {
    pub id: String,
    pub owner_username: String,
    pub name: String,
    pub action: String,
    pub enabled: bool,
    pub time_of_day: String,
    pub timezone: String,
    pub account_scope: String,
    pub account_uids: Vec<String>,
    pub item_scope: String,
    pub item_ids: Vec<String>,
    pub config: Value,
    pub last_run_at: f64,
    pub last_status: String,
    pub last_message: String,
    pub next_run_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveAutomationSchedulePayload {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub action: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_time_of_day")]
    pub time_of_day: String,
    #[serde(default = "default_timezone")]
    pub timezone: String,
    #[serde(default = "default_scope")]
    pub account_scope: String,
    #[serde(default)]
    pub account_uids: Vec<String>,
    #[serde(default = "default_item_scope")]
    pub item_scope: String,
    #[serde(default)]
    pub item_ids: Vec<String>,
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone)]
struct AccountRef {
    uid: String,
    nick: String,
}

fn default_enabled() -> bool {
    true
}
fn default_time_of_day() -> String {
    "09:00".to_string()
}
fn default_timezone() -> String {
    "Asia/Shanghai".to_string()
}
fn default_scope() -> String {
    "all".to_string()
}
fn default_item_scope() -> String {
    "all".to_string()
}

pub fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub fn next_run_timestamp(time_of_day: &str, after: f64) -> f64 {
    let (hour, minute) = parse_time_of_day(time_of_day).unwrap_or((9, 0));
    let now = after.max(0.0) as i64;
    let local = now + 8 * 3600;
    let day_start = local - local.rem_euclid(86_400);
    let mut candidate_local = day_start + i64::from(hour) * 3600 + i64::from(minute) * 60;
    if candidate_local <= local + 5 {
        candidate_local += 86_400;
    }
    (candidate_local - 8 * 3600) as f64
}

fn parse_time_of_day(value: &str) -> Option<(u32, u32)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour = hour.parse::<u32>().ok()?.min(23);
    let minute = minute.parse::<u32>().ok()?.min(59);
    Some((hour, minute))
}

fn schedule_id() -> String {
    format!("auto-{}", crate::storage::unix_millis_now())
}

fn parse_json_array(text: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default()
}

fn parse_json(text: String) -> Value {
    serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
}

fn schedule_from_row(row: sqlx::sqlite::SqliteRow) -> AutomationSchedule {
    AutomationSchedule {
        id: row.get("id"),
        owner_username: row.get("owner_username"),
        name: row.get("name"),
        action: row.get("action"),
        enabled: row.get::<i64, _>("enabled") != 0,
        time_of_day: row.get("time_of_day"),
        timezone: row.get("timezone"),
        account_scope: row.get("account_scope"),
        account_uids: parse_json_array(row.get("account_uids_json")),
        item_scope: row.get("item_scope"),
        item_ids: parse_json_array(row.get("item_ids_json")),
        config: parse_json(row.get("config_json")),
        last_run_at: row.get("last_run_at"),
        last_status: row.get("last_status"),
        last_message: row.get("last_message"),
        next_run_at: row.get("next_run_at"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

pub async fn list_schedules(
    state: &AppState,
    owner_username: &str,
) -> Result<Vec<AutomationSchedule>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT * FROM automation_schedules
        WHERE owner_username = ?
        ORDER BY updated_at DESC
        "#,
    )
    .bind(owner_username)
    .fetch_all(state.storage().xianyu())
    .await?;
    Ok(rows.into_iter().map(schedule_from_row).collect())
}

pub async fn get_schedule(
    state: &AppState,
    owner_username: &str,
    id: &str,
) -> Result<Option<AutomationSchedule>, StorageError> {
    let row = sqlx::query("SELECT * FROM automation_schedules WHERE owner_username = ? AND id = ?")
        .bind(owner_username)
        .bind(id)
        .fetch_optional(state.storage().xianyu())
        .await?;
    Ok(row.map(schedule_from_row))
}

pub async fn save_schedule(
    state: &AppState,
    owner_username: &str,
    payload: SaveAutomationSchedulePayload,
) -> Result<AutomationSchedule, StorageError> {
    let id = if payload.id.trim().is_empty() {
        schedule_id()
    } else {
        payload.id.trim().to_string()
    };
    let now = now_seconds();
    let next_run_at = if payload.enabled {
        next_run_timestamp(&payload.time_of_day, now)
    } else {
        0.0
    };
    sqlx::query(
        r#"
        INSERT INTO automation_schedules (
            id, owner_username, name, action, enabled, time_of_day, timezone,
            account_scope, account_uids_json, item_scope, item_ids_json, config_json,
            next_run_at, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, unixepoch('subsec'), unixepoch('subsec'))
        ON CONFLICT(id) DO UPDATE SET
            name = excluded.name,
            action = excluded.action,
            enabled = excluded.enabled,
            time_of_day = excluded.time_of_day,
            timezone = excluded.timezone,
            account_scope = excluded.account_scope,
            account_uids_json = excluded.account_uids_json,
            item_scope = excluded.item_scope,
            item_ids_json = excluded.item_ids_json,
            config_json = excluded.config_json,
            next_run_at = excluded.next_run_at,
            updated_at = unixepoch('subsec')
        "#,
    )
    .bind(&id)
    .bind(owner_username)
    .bind(payload.name.trim())
    .bind(payload.action.trim())
    .bind(if payload.enabled { 1_i64 } else { 0_i64 })
    .bind(payload.time_of_day.trim())
    .bind(payload.timezone.trim())
    .bind(payload.account_scope.trim())
    .bind(serde_json::to_string(&payload.account_uids).unwrap_or_else(|_| "[]".to_string()))
    .bind(payload.item_scope.trim())
    .bind(serde_json::to_string(&payload.item_ids).unwrap_or_else(|_| "[]".to_string()))
    .bind(payload.config.to_string())
    .bind(next_run_at)
    .execute(state.storage().xianyu())
    .await?;
    Ok(get_schedule(state, owner_username, &id)
        .await?
        .expect("saved schedule must exist"))
}

pub async fn delete_schedule(
    state: &AppState,
    owner_username: &str,
    id: &str,
) -> Result<bool, StorageError> {
    let result =
        sqlx::query("DELETE FROM automation_schedules WHERE owner_username = ? AND id = ?")
            .bind(owner_username)
            .bind(id)
            .execute(state.storage().xianyu())
            .await?;
    Ok(result.rows_affected() > 0)
}

async fn due_schedules(state: &AppState) -> Result<Vec<AutomationSchedule>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT * FROM automation_schedules
        WHERE enabled = 1 AND next_run_at > 0 AND next_run_at <= ?
        ORDER BY next_run_at ASC
        LIMIT 20
        "#,
    )
    .bind(now_seconds())
    .fetch_all(state.storage().xianyu())
    .await?;
    Ok(rows.into_iter().map(schedule_from_row).collect())
}

async fn update_schedule_result(
    state: &AppState,
    schedule: &AutomationSchedule,
    status: &str,
    message: &str,
) -> Result<(), StorageError> {
    let now = now_seconds();
    let next_run_at = if schedule.enabled {
        next_run_timestamp(&schedule.time_of_day, now + 60.0)
    } else {
        0.0
    };
    sqlx::query(
        r#"
        UPDATE automation_schedules
        SET last_run_at = ?, last_status = ?, last_message = ?, next_run_at = ?, updated_at = unixepoch('subsec')
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(status)
    .bind(message.chars().take(500).collect::<String>())
    .bind(next_run_at)
    .bind(&schedule.id)
    .execute(state.storage().xianyu())
    .await?;
    Ok(())
}

async fn mark_schedule_running(
    state: &AppState,
    schedule: &AutomationSchedule,
    message: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        r#"
        UPDATE automation_schedules
        SET last_run_at = ?, last_status = 'running', last_message = ?, updated_at = unixepoch('subsec')
        WHERE id = ?
        "#,
    )
    .bind(now_seconds())
    .bind(message.chars().take(500).collect::<String>())
    .bind(&schedule.id)
    .execute(state.storage().xianyu())
    .await?;
    Ok(())
}

async fn list_active_accounts(
    state: &AppState,
    owner_username: &str,
) -> Result<Vec<AccountRef>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT uid, nick
        FROM accounts
        WHERE status = 'active' AND (? = '' OR owner_username = ?)
        ORDER BY bound_at DESC, updated_at DESC, uid ASC
        "#,
    )
    .bind(owner_username)
    .bind(owner_username)
    .fetch_all(state.storage().accounts())
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| AccountRef {
            uid: row.get("uid"),
            nick: row.get("nick"),
        })
        .collect())
}

async fn resolve_accounts(
    state: &AppState,
    schedule: &AutomationSchedule,
) -> Result<Vec<AccountRef>, StorageError> {
    let all = list_active_accounts(state, &schedule.owner_username).await?;
    if schedule.account_scope == "selected" {
        Ok(all
            .into_iter()
            .filter(|account| schedule.account_uids.contains(&account.uid))
            .collect())
    } else {
        Ok(all)
    }
}

async fn item_rows_for_account(
    state: &AppState,
    uid: &str,
    item_scope: &str,
    item_ids: &[String],
) -> Result<Vec<(String, String, String, String)>, StorageError> {
    let rows = sqlx::query(
        r#"
        SELECT item_id, title, price, orig_price
        FROM account_items
        WHERE uid = ? AND status = 'ON_SALE'
        ORDER BY updated_at DESC, item_id ASC
        "#,
    )
    .bind(uid)
    .fetch_all(state.storage().xianyu())
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let item_id: String = row.get("item_id");
            if item_scope == "selected" && !item_ids.contains(&item_id) {
                return None;
            }
            Some((
                item_id,
                row.get("title"),
                row.get("price"),
                row.get("orig_price"),
            ))
        })
        .collect())
}

async fn create_item_operation_job(
    state: &AppState,
    schedule: &AutomationSchedule,
    action: &str,
    params_for_item: impl Fn(&str, &str, &str, &str) -> Value,
) -> Result<String, String> {
    let accounts = resolve_accounts(state, schedule)
        .await
        .map_err(|error| error.to_string())?;
    let mut items = Vec::new();
    for account in accounts {
        let _ = sync_items_to_db(state, &account.uid).await;
        for (item_id, title, price, orig_price) in item_rows_for_account(
            state,
            &account.uid,
            &schedule.item_scope,
            &schedule.item_ids,
        )
        .await
        .map_err(|error| error.to_string())?
        {
            items.push((account.clone(), item_id, title, price, orig_price));
        }
    }
    if items.is_empty() {
        return Ok("没有匹配到可执行商品".to_string());
    }
    let job_id = format!(
        "auto-job-{}-{}",
        schedule.id,
        crate::storage::unix_millis_now()
    );
    state
        .storage()
        .create_operation_job(NewOperationJob {
            id: &job_id,
            kind: "automation",
            module: "other-automation",
            action,
            operator: &schedule.owner_username,
            trigger_source: "schedule",
            status: "queued",
            params: json!({ "scheduleId": schedule.id, "action": action }),
            summary: json!({ "scheduleName": schedule.name }),
            total_count: items.len() as i64,
        })
        .await
        .map_err(|error| error.to_string())?;
    let job_items: Vec<NewOperationJobItem<'_>> = items
        .iter()
        .enumerate()
        .map(
            |(index, (account, item_id, title, price, orig_price))| NewOperationJobItem {
                job_id: &job_id,
                seq: (index + 1) as i64,
                uid: &account.uid,
                account_nick: &account.nick,
                target_type: "item",
                target_id: item_id,
                target_title: title,
                status: "queued",
                params: params_for_item(item_id, title, price, orig_price),
            },
        )
        .collect();
    state
        .storage()
        .add_operation_job_items(&job_items)
        .await
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "已创建后台任务 {job_id}，共 {} 个商品",
        job_items.len()
    ))
}

fn config_object(config: &Value) -> Map<String, Value> {
    config.as_object().cloned().unwrap_or_default()
}

fn account_config_object(config: &Value, uid: &str) -> Map<String, Value> {
    let root = config_object(config);
    root.get("accountPromotions")
        .and_then(Value::as_object)
        .and_then(|items| items.get(uid))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or(root)
}

fn amount_value_to_cents(value: &Value) -> Option<i64> {
    let raw = match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    };
    let parsed = raw.parse::<f64>().ok()?;
    if parsed <= 0.0 {
        return None;
    }
    Some((parsed * 100.0).round() as i64)
}

fn price_text_to_cents(value: &str) -> Option<i64> {
    let parsed = value.trim().parse::<f64>().ok()?;
    if parsed <= 0.0 {
        return None;
    }
    Some((parsed * 100.0).round() as i64)
}

fn cents_to_price_text(cents: i64) -> String {
    format!("{:.2}", cents as f64 / 100.0)
}

async fn execute_account_action(
    state: &AppState,
    schedule: &AutomationSchedule,
    run: impl Fn(
        AppState,
        String,
        Value,
    )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send>>,
) -> Result<String, String> {
    let accounts = resolve_accounts(state, schedule)
        .await
        .map_err(|error| error.to_string())?;
    if accounts.is_empty() {
        return Ok("没有匹配到可执行账号".to_string());
    }
    let mut success = 0_usize;
    let mut fail = 0_usize;
    for account in accounts {
        let result = run(state.clone(), account.uid.clone(), schedule.config.clone()).await;
        let (payload, runtime_error) = match result {
            Ok(payload) => (payload, None),
            Err(error) => (json!({ "error": error.clone() }), Some(error)),
        };
        let ok = runtime_error.is_none() && payload_success(&payload);
        if ok {
            success += 1
        } else {
            fail += 1
        }
        let status = if ok { "success" } else { "failed" };
        let message = runtime_error
            .as_deref()
            .map(str::to_string)
            .unwrap_or_else(|| account_response_message(&payload, ok));
        let error_code = runtime_error
            .as_ref()
            .map(|_| "runtime_error".to_string())
            .unwrap_or_else(|| account_response_error_code(&payload, ok));
        emit_account_raw_execution_report(
            schedule,
            &account,
            status,
            &error_code,
            &message,
            payload.clone(),
        );
        append_schedule_log(
            state,
            schedule,
            &account.uid,
            &account.nick,
            status,
            &message,
            payload,
        )
        .await;
    }
    Ok(format!("账号任务完成：成功 {success}，失败 {fail}"))
}

fn payload_success(payload: &Value) -> bool {
    payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .map(|text| text.to_ascii_uppercase().contains("SUCCESS"))
        .unwrap_or_else(|| {
            payload
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("success"))
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
}

fn account_response_message(payload: &Value, ok: bool) -> String {
    payload
        .get("detail")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .get("ret")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(Value::as_str)
        })
        .or_else(|| {
            payload
                .get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("message").or_else(|| data.get("msg")))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
        .unwrap_or_else(|| {
            if ok {
                "执行成功".to_string()
            } else {
                "执行失败".to_string()
            }
        })
}

fn account_response_error_code(payload: &Value, ok: bool) -> String {
    if ok {
        return String::new();
    }
    payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
        .map(|text| text.split("::").next().unwrap_or(text).to_string())
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "upstream_failed".to_string())
}

fn emit_account_raw_execution_report(
    schedule: &AutomationSchedule,
    account: &AccountRef,
    status: &str,
    error_code: &str,
    message: &str,
    response: Value,
) {
    let params = json!({
        "scheduleConfig": schedule.config.clone(),
        "accountConfig": Value::Object(account_config_object(&schedule.config, &account.uid)),
    });
    emit_automation_raw_report(AutomationRawReport {
        schedule_id: &schedule.id,
        schedule_name: &schedule.name,
        action: &schedule.action,
        trigger_source: "schedule",
        owner_username: &schedule.owner_username,
        uid: &account.uid,
        account_nick: &account.nick,
        target_type: "account",
        target_id: &account.uid,
        target_title: &account.nick,
        job_id: &schedule.id,
        job_item_id: None,
        status,
        error_code,
        message,
        params,
        response,
    });
}

async fn append_schedule_log(
    state: &AppState,
    schedule: &AutomationSchedule,
    uid: &str,
    nick: &str,
    status: &str,
    message: &str,
    result: Value,
) {
    let _ = state
        .storage()
        .append_operation_log(NewOperationLog {
            job_id: &schedule.id,
            module: "other-automation",
            action: &schedule.action,
            operator: &schedule.owner_username,
            trigger_source: "schedule",
            uid,
            account_nick: nick,
            target_type: "account",
            target_id: uid,
            target_title: nick,
            status,
            error_code: "",
            message,
            params: schedule.config.clone(),
            before: json!({}),
            after: result,
            extra: json!({ "scheduleId": schedule.id }),
        })
        .await;
}

pub async fn execute_schedule_once(
    state: &AppState,
    schedule: &AutomationSchedule,
) -> Result<String, String> {
    match schedule.action.as_str() {
        "batch_reduce_price" => {
            let amount = schedule
                .config
                .get("amount")
                .cloned()
                .unwrap_or_else(|| json!(0));
            create_item_operation_job(
                state,
                schedule,
                "batch_reduce_price",
                move |_, _, _, _| json!({ "amount": amount.clone() }),
            )
            .await
        }
        "batch_restore_price" => {
            let amount = schedule
                .config
                .get("amount")
                .cloned()
                .unwrap_or_else(|| json!(0));
            create_item_operation_job(
                state,
                schedule,
                "batch_restore_price",
                move |_, _, price, _| {
                    let target_price = price_text_to_cents(price)
                        .zip(amount_value_to_cents(&amount))
                        .map(|(current, increase)| cents_to_price_text(current + increase))
                        .unwrap_or_default();
                    json!({ "restore_price": target_price, "increaseAmount": amount.clone() })
                },
            )
            .await
        }
        "batch_polish" => {
            create_item_operation_job(state, schedule, "batch_polish", |_, _, _, _| json!({})).await
        }
        "coin_signin" => {
            execute_account_action(state, schedule, |state, uid, config| {
                Box::pin(async move {
                    coin::run_coin_signin_action(&state, &uid, config_object(&config)).await
                })
            })
            .await
        }
        "coin_tasks" => {
            execute_account_action(state, schedule, |state, uid, _| {
                Box::pin(
                    async move { coin::run_coin_complete_view_tasks_action(&state, &uid).await },
                )
            })
            .await
        }
        "exposure_tasks" => {
            execute_account_action(state, schedule, |state, uid, _| {
                Box::pin(async move {
                    exposure::run_exposure_complete_view_tasks_action(&state, &uid).await
                })
            })
            .await
        }
        "coin_promotion" => {
            execute_account_action(state, schedule, |state, uid, config| {
                Box::pin(async move {
                    let object = account_config_object(&config, &uid);
                    let item_id = object
                        .get("itemId")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if item_id.trim().is_empty() {
                        return Err("缺少推广商品".to_string());
                    }
                    let cost = object
                        .get("costCoinNum")
                        .cloned()
                        .unwrap_or_else(|| json!(0));
                    coin::run_coin_promotion_create_action(&state, &uid, &item_id, cost, object)
                        .await
                })
            })
            .await
        }
        "exposure_promotion" => {
            execute_account_action(state, schedule, |state, uid, config| {
                Box::pin(async move {
                    let object = account_config_object(&config, &uid);
                    let item_ids = object
                        .get("itemIds")
                        .and_then(Value::as_array)
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_else(|| {
                            object
                                .get("itemId")
                                .and_then(Value::as_str)
                                .map(|value| vec![value.to_string()])
                                .unwrap_or_default()
                        });
                    if item_ids.is_empty() {
                        return Err("缺少曝光商品".to_string());
                    }
                    let exposure_num = object
                        .get("exposureNum")
                        .cloned()
                        .unwrap_or_else(|| json!(100));
                    let expected = object.get("expectedExposureNum").cloned();
                    exposure::run_exposure_create_action(
                        &state,
                        &uid,
                        item_ids,
                        exposure_num,
                        expected,
                        object,
                    )
                    .await
                })
            })
            .await
        }
        _ => Err(format!("不支持的自动化动作：{}", schedule.action)),
    }
}

pub async fn run_once_by_id(
    state: &AppState,
    owner_username: &str,
    id: &str,
) -> Result<String, String> {
    let schedule = get_schedule(state, owner_username, id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "自动化不存在".to_string())?;
    mark_schedule_running(state, &schedule, "正在执行...")
        .await
        .map_err(|error| error.to_string())?;
    let result = execute_schedule_once(state, &schedule).await;
    let (status, message) = match &result {
        Ok(message) => ("success", message.as_str()),
        Err(message) => ("failed", message.as_str()),
    };
    update_schedule_result(state, &schedule, status, message)
        .await
        .map_err(|error| error.to_string())?;
    result
}

pub async fn run_scheduler_worker(
    state: AppState,
    runtime: crate::state::RuntimeState,
    mut shutdown_rx: watch::Receiver<bool>,
    worker_name: &'static str,
) {
    let mut interval = time::interval(Duration::from_secs(15));
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
                let mut status = "running";
                let mut detail = None;
                match due_schedules(&state).await {
                    Ok(schedules) => {
                        for schedule in schedules {
                            if let Err(error) = mark_schedule_running(&state, &schedule, "定时任务正在执行...").await {
                                warn!(error = %error, schedule_id = %schedule.id, "failed to mark automation schedule running");
                            }
                            let result = execute_schedule_once(&state, &schedule).await;
                            let (run_status, message) = match &result {
                                Ok(message) => ("success", message.as_str()),
                                Err(message) => ("failed", message.as_str()),
                            };
                            if let Err(error) = update_schedule_result(&state, &schedule, run_status, message).await {
                                warn!(error = %error, schedule_id = %schedule.id, "failed to update automation schedule result");
                            }
                        }
                    }
                    Err(error) => {
                        status = "degraded";
                        detail = Some(error.to_string());
                    }
                }
                runtime.record_worker_tick(worker_name, status, tick_count, detail.clone());
                let _ = state.storage().upsert_worker_state(worker_name, status, tick_count, detail.as_deref()).await;
            }
        }
    }
    let _ = state
        .storage()
        .upsert_worker_state(worker_name, "stopped", tick_count, None)
        .await;
    runtime.mark_worker_stopped(worker_name);
}

#[allow(dead_code)]
fn local_time_label(ts: f64) -> String {
    let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) else {
        return String::new();
    };
    let local = dt + chrono::Duration::hours(8);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        local.year(),
        local.month(),
        local.day(),
        local.hour(),
        local.minute()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    use tracing_subscriber::prelude::*;

    fn init_logging() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = tracing_subscriber::registry()
                .with(crate::logging::memory_log_layer())
                .try_init();
        });
    }

    fn schedule_fixture() -> AutomationSchedule {
        AutomationSchedule {
            id: "auto-raw-1".to_string(),
            owner_username: "owner-a".to_string(),
            name: "定时闲鱼币签到".to_string(),
            action: "coin_signin".to_string(),
            enabled: true,
            time_of_day: "09:00".to_string(),
            timezone: "Asia/Shanghai".to_string(),
            account_scope: "selected".to_string(),
            account_uids: vec!["uid-1".to_string()],
            item_scope: "all".to_string(),
            item_ids: vec![],
            config: json!({
                "sceneId": 496,
                "token": "secret-token",
                "accountPromotions": {
                    "uid-1": {
                        "itemId": "ITEM-1",
                        "costCoinNum": 100
                    }
                }
            }),
            last_run_at: 0.0,
            last_status: String::new(),
            last_message: String::new(),
            next_run_at: 0.0,
            created_at: 0.0,
            updated_at: 0.0,
        }
    }

    #[test]
    fn account_raw_report_is_emitted_to_uploadable_usage_stream() {
        init_logging();
        let before_sequence = crate::logging::latest_sequence();

        let schedule = schedule_fixture();
        let account = AccountRef {
            uid: "uid-1".to_string(),
            nick: "测试账号".to_string(),
        };

        emit_account_raw_execution_report(
            &schedule,
            &account,
            "failed",
            "FAIL_SYS_TOKEN_EXOIRED",
            "令牌过期",
            json!({ "ret": ["FAIL_SYS_TOKEN_EXOIRED::令牌过期"], "data": { "success": false } }),
        );

        let logs = crate::logging::query_logs_after_sequence(
            before_sequence,
            None,
            None,
            Some("client.telemetry.action"),
            10,
        );
        let entry = logs
            .iter()
            .find(|entry| {
                entry.module.as_deref() == Some("other-automation")
                    && entry.job_id.as_deref() == Some("auto-raw-1")
            })
            .expect("account automation raw report should be recorded");

        assert_eq!(entry.stream.as_deref(), Some("usage"));
        assert_eq!(entry.action.as_deref(), Some("coin_signin"));
        assert_eq!(entry.outcome.as_deref(), Some("failed"));
        assert_eq!(entry.account_uid.as_deref(), Some("uid-1"));

        let raw = entry
            .fields_json
            .get("raw_payload_json")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(raw.contains("auto-raw-1"));
        assert!(raw.contains("FAIL_SYS_TOKEN_EXOIRED"));
        assert!(raw.contains("ITEM-1"));
        assert!(!raw.contains("secret-token"));
    }
}
