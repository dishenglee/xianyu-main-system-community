use std::collections::HashMap;

use serde_json::{json, Value};

use crate::{
    automation_raw_report::{emit_automation_raw_report, AutomationRawReport},
    models::operations::{OperationJob, OperationJobItem},
    mtop_errors::classify_ret_code,
    routes::market::{groupon, items, items_edit, items_open, pricing},
    state::AppState,
    telemetry::IncidentInput,
};

#[derive(Debug, Clone)]
enum AuthRepairState {
    RepairedOnce,
    FailedOnce(String),
}

fn outcome_message(resp: &Value) -> String {
    resp.get("detail")
        .and_then(Value::as_str)
        .or_else(|| {
            resp.get("ret")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(Value::as_str)
        })
        .or_else(|| {
            resp.get("data")
                .and_then(Value::as_object)
                .and_then(|d| d.get("message").or_else(|| d.get("msg")))
                .and_then(Value::as_str)
        })
        .unwrap_or_default()
        .to_string()
}

fn response_ok(resp: &Value) -> bool {
    if resp.get("status").and_then(Value::as_str) == Some("error") {
        return false;
    }
    if let Some(arr) = resp.get("ret").and_then(Value::as_array) {
        return arr
            .first()
            .and_then(Value::as_str)
            .map(|s| s.to_ascii_uppercase().contains("SUCCESS"))
            .unwrap_or(false);
    }
    resp.get("ok").and_then(Value::as_bool).unwrap_or(false)
        || resp
            .get("data")
            .and_then(Value::as_object)
            .and_then(|d| d.get("success").or_else(|| d.get("result")))
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn first_ret_code(resp: &Value) -> String {
    resp.get("ret")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn is_auth_recoverable_job_failure(resp: &Value, err: &str) -> bool {
    let ret = first_ret_code(resp);
    if !ret.trim().is_empty() {
        return classify_ret_code(&ret).is_auth_recoverable();
    }
    let upper = err.to_ascii_uppercase();
    upper.contains("SESSION_EXPIRED")
        || upper.contains("USER_NOT_LOGIN")
        || upper.contains("NOT_LOGIN")
        || upper.contains("TOKEN_EXOIRED")
        || upper.contains("TOKEN_EXPIRED")
        || upper.contains("TOKEN_ILLEGAL")
}

fn should_emit_other_automation_raw_report(job: &OperationJob) -> bool {
    job.module == "other-automation" || job.kind == "automation"
}

fn job_schedule_id(job: &OperationJob) -> String {
    job.params
        .get("scheduleId")
        .or_else(|| job.params.get("schedule_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn job_schedule_name(job: &OperationJob) -> String {
    job.summary
        .get("scheduleName")
        .or_else(|| job.summary.get("schedule_name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn emit_item_job_raw_execution_report(
    job: &OperationJob,
    item: &OperationJobItem,
    status: &str,
    error_code: &str,
    message: &str,
    response: Value,
) {
    if !should_emit_other_automation_raw_report(job) {
        return;
    }
    let schedule_id = job_schedule_id(job);
    let schedule_name = job_schedule_name(job);
    emit_automation_raw_report(AutomationRawReport {
        schedule_id: &schedule_id,
        schedule_name: &schedule_name,
        action: &job.action,
        trigger_source: &job.trigger_source,
        owner_username: &job.operator,
        uid: &item.uid,
        account_nick: &item.account_nick,
        target_type: &item.target_type,
        target_id: &item.target_id,
        target_title: &item.target_title,
        job_id: &job.id,
        job_item_id: Some(item.id),
        status,
        error_code,
        message,
        params: json!({
            "jobParams": job.params.clone(),
            "itemParams": item.params.clone(),
        }),
        response,
    });
}

async fn run_item_job_action(
    state: &AppState,
    action: &str,
    uid: &str,
    item_id: &str,
    params: &Value,
) -> Result<Value, String> {
    match action {
        "batch_reduce_price" => {
            let amount = params.get("amount").cloned().unwrap_or_else(|| json!(0.0));
            pricing::run_item_reduce_price_action(state, uid, item_id, &amount).await
        }
        "batch_restore_price" => {
            let target_price = params
                .get("restore_price")
                .and_then(Value::as_str)
                .or_else(|| params.get("originalPrice").and_then(Value::as_str))
                .or_else(|| params.get("orig_price").and_then(Value::as_str))
                .unwrap_or_default()
                .trim()
                .to_string();
            if target_price.is_empty() {
                return Err("缺少可涨价的目标价".to_string());
            }
            items_edit::run_item_edit_price_action(
                state,
                uid,
                item_id,
                &target_price,
                &target_price,
            )
            .await
        }
        "batch_polish" => items::run_item_polish_action(state, uid, item_id).await,
        "batch_coin_open" => {
            let ratio = params
                .get("ratio")
                .and_then(Value::as_str)
                .unwrap_or("5")
                .to_string();
            pricing::run_coin_open_action(state, uid, item_id, &ratio).await
        }
        "batch_coin_close" => pricing::run_coin_close_action(state, uid, item_id).await,
        "batch_groupon_open" => {
            let price = params
                .get("price")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mode = params
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let discount_rate = params
                .get("discount_rate")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            groupon::run_groupon_open_action(state, uid, item_id, &price, &mode, &discount_rate)
                .await
        }
        "batch_groupon_close" => groupon::run_groupon_close_action(state, uid, item_id).await,
        "batch_delete" => items::run_item_delete_action(state, uid, item_id).await,
        "batch_downshelf" => items::run_item_downshelf_action(state, uid, item_id).await,
        "batch_upshelf" => {
            let item_context = params
                .get("item_context")
                .cloned()
                .unwrap_or_else(|| json!({}));
            items_open::run_item_open_action(state, uid, item_id, item_context).await
        }
        other => Err(format!("unsupported operation action: {other}")),
    }
}

pub async fn process_one_job(state: &AppState, job_id: &str) -> Result<(), String> {
    let Some(job) = state
        .storage()
        .get_operation_job(job_id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Err("job missing".to_string());
    };
    let items = state
        .storage()
        .list_operation_job_items(job_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut success_count = 0_i64;
    let mut fail_count = 0_i64;
    let skipped_count = 0_i64;
    let mut first_error = String::new();
    let mut auth_repairs_by_uid: HashMap<String, AuthRepairState> = HashMap::new();

    for item in items {
        let started_at = crate::storage::unix_millis_now() as f64 / 1000.0;
        state
            .storage()
            .update_operation_job_item(
                item.id,
                "running",
                json!({}),
                "",
                "",
                Some(started_at),
                None,
            )
            .await
            .map_err(|e| e.to_string())?;

        let repair_state = auth_repairs_by_uid.get(&item.uid).cloned();
        let mut attempt = match repair_state.as_ref() {
            Some(AuthRepairState::FailedOnce(reason)) => Err(reason.clone()),
            _ => {
                run_item_job_action(state, &job.action, &item.uid, &item.target_id, &item.params)
                    .await
            }
        };

        let needs_repair_retry = match &attempt {
            Ok(resp) if !response_ok(resp) => {
                let msg = outcome_message(resp);
                is_auth_recoverable_job_failure(resp, &msg)
            }
            Err(err) => is_auth_recoverable_job_failure(&json!({}), err),
            _ => false,
        } && !matches!(
            repair_state,
            Some(AuthRepairState::RepairedOnce | AuthRepairState::FailedOnce(_))
        );

        if needs_repair_retry {
            tracing::warn!(
                job_id = %job_id,
                uid = %item.uid,
                target_id = %item.target_id,
                action = %job.action,
                "operation job hit recoverable auth failure, attempting deep refresh + retry"
            );
            let refresh_ok = match crate::cookie_refresh::refresh_account_cookies_deep(
                state.storage().accounts(),
                &item.uid,
            )
            .await
            {
                Ok(_) => match crate::cookie_refresh::probe_and_repair_business_session(
                    state.config(),
                    state.storage().accounts(),
                    &item.uid,
                )
                .await
                {
                    Ok(probe) => probe.ok,
                    Err(_) => false,
                },
                Err(_) => false,
            };

            if refresh_ok {
                auth_repairs_by_uid.insert(item.uid.clone(), AuthRepairState::RepairedOnce);
                attempt = run_item_job_action(
                    state,
                    &job.action,
                    &item.uid,
                    &item.target_id,
                    &item.params,
                )
                .await;
            } else {
                let original_message = match &attempt {
                    Ok(resp) => outcome_message(resp),
                    Err(err) => err.clone(),
                };
                let failed_reason = if original_message.trim().is_empty() {
                    "账号会话已过期，自动刷新失败".to_string()
                } else {
                    format!("{original_message}（自动刷新失败）")
                };
                auth_repairs_by_uid.insert(
                    item.uid.clone(),
                    AuthRepairState::FailedOnce(failed_reason.clone()),
                );
                attempt = Err(failed_reason);
            }
        }

        match attempt {
            Ok(resp) if response_ok(&resp) => {
                success_count += 1;
                let message = outcome_message(&resp);
                let report_message = if message.is_empty() {
                    "执行成功".to_string()
                } else {
                    message.clone()
                };
                state
                    .storage()
                    .update_operation_job_item(
                        item.id,
                        "success",
                        resp.clone(),
                        &message,
                        "",
                        None,
                        Some(crate::storage::unix_millis_now() as f64 / 1000.0),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                emit_item_job_raw_execution_report(
                    &job,
                    &item,
                    "success",
                    "",
                    &report_message,
                    resp.clone(),
                );
                state
                    .storage()
                    .append_operation_log(crate::storage::NewOperationLog {
                        job_id,
                        module: &job.module,
                        action: &job.action,
                        operator: &job.operator,
                        trigger_source: &job.trigger_source,
                        uid: &item.uid,
                        account_nick: &item.account_nick,
                        target_type: &item.target_type,
                        target_id: &item.target_id,
                        target_title: &item.target_title,
                        status: "success",
                        error_code: "",
                        message: if message.is_empty() {
                            "执行成功"
                        } else {
                            &message
                        },
                        params: item.params.clone(),
                        before: json!({}),
                        after: resp.clone(),
                        extra: json!({
                            "job_item_id": item.id,
                        }),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Ok(resp) => {
                fail_count += 1;
                let message = outcome_message(&resp);
                let ret_code = first_ret_code(&resp);
                let raw_error_code = ret_code
                    .split("::")
                    .next()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or("upstream_failed")
                    .to_string();
                if first_error.is_empty() {
                    first_error = message.clone();
                }
                state
                    .storage()
                    .update_operation_job_item(
                        item.id,
                        "failed",
                        resp.clone(),
                        &message,
                        "upstream_failed",
                        None,
                        Some(crate::storage::unix_millis_now() as f64 / 1000.0),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                emit_item_job_raw_execution_report(
                    &job,
                    &item,
                    "failed",
                    &raw_error_code,
                    &message,
                    resp.clone(),
                );
                state
                    .storage()
                    .append_operation_log(crate::storage::NewOperationLog {
                        job_id,
                        module: &job.module,
                        action: &job.action,
                        operator: &job.operator,
                        trigger_source: &job.trigger_source,
                        uid: &item.uid,
                        account_nick: &item.account_nick,
                        target_type: &item.target_type,
                        target_id: &item.target_id,
                        target_title: &item.target_title,
                        status: "failed",
                        error_code: "upstream_failed",
                        message: &message,
                        params: item.params.clone(),
                        before: json!({}),
                        after: resp.clone(),
                        extra: json!({
                            "job_item_id": item.id,
                        }),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = state
                    .incident_reporter()
                    .report_from_state(
                        state,
                        IncidentInput {
                            error_level: "warn".to_string(),
                            module: job.module.clone(),
                            action: job.action.clone(),
                            route: crate::sealed_protocol::sealed_str!("/api/operations/jobs").to_string(),
                            error_code: "upstream_failed".to_string(),
                            error_message: message.clone(),
                            http_status: None,
                            upstream_ret: ret_code,
                            retryable: is_auth_recoverable_job_failure(&resp, &message),
                            context_json: json!({
                                "jobId": job_id,
                                "jobModule": job.module,
                                "jobAction": job.action,
                                "triggerSource": job.trigger_source,
                                "operator": job.operator,
                                "jobItemId": item.id,
                                "uid": item.uid,
                                "accountNick": item.account_nick,
                                "targetType": item.target_type,
                                "targetId": item.target_id,
                                "targetTitle": item.target_title,
                                "params": item.params,
                                "responseStatus": resp.get("status").cloned().unwrap_or_else(|| json!(null)),
                                "responseDetail": resp.get("detail").cloned().unwrap_or_else(|| json!(null)),
                            }),
                            environment: None,
                        },
                    )
                    .await;
            }
            Err(err) => {
                fail_count += 1;
                if first_error.is_empty() {
                    first_error = err.clone();
                }
                state
                    .storage()
                    .update_operation_job_item(
                        item.id,
                        "failed",
                        json!({}),
                        &err,
                        "runtime_error",
                        None,
                        Some(crate::storage::unix_millis_now() as f64 / 1000.0),
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                emit_item_job_raw_execution_report(
                    &job,
                    &item,
                    "failed",
                    "runtime_error",
                    &err,
                    json!({ "error": err.clone() }),
                );
                state
                    .storage()
                    .append_operation_log(crate::storage::NewOperationLog {
                        job_id,
                        module: &job.module,
                        action: &job.action,
                        operator: &job.operator,
                        trigger_source: &job.trigger_source,
                        uid: &item.uid,
                        account_nick: &item.account_nick,
                        target_type: &item.target_type,
                        target_id: &item.target_id,
                        target_title: &item.target_title,
                        status: "failed",
                        error_code: "runtime_error",
                        message: &err,
                        params: item.params.clone(),
                        before: json!({}),
                        after: json!({}),
                        extra: json!({
                            "job_item_id": item.id,
                        }),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                let _ = state
                    .incident_reporter()
                    .report_from_state(
                        state,
                        IncidentInput {
                            error_level: "error".to_string(),
                            module: job.module.clone(),
                            action: job.action.clone(),
                            route: crate::sealed_protocol::sealed_str!("/api/operations/jobs")
                                .to_string(),
                            error_code: "runtime_error".to_string(),
                            error_message: err.clone(),
                            http_status: None,
                            upstream_ret: String::new(),
                            retryable: false,
                            context_json: json!({
                                "jobId": job_id,
                                "jobModule": job.module,
                                "jobAction": job.action,
                                "triggerSource": job.trigger_source,
                                "operator": job.operator,
                                "jobItemId": item.id,
                                "uid": item.uid,
                                "accountNick": item.account_nick,
                                "targetType": item.target_type,
                                "targetId": item.target_id,
                                "targetTitle": item.target_title,
                                "params": item.params,
                            }),
                            environment: None,
                        },
                    )
                    .await;
            }
        }
    }

    let final_status = if fail_count == 0 {
        "success"
    } else if success_count > 0 {
        "partial_success"
    } else if skipped_count > 0 && fail_count == 0 {
        "skipped"
    } else {
        "failed"
    };
    state
        .storage()
        .finalize_operation_job(
            job_id,
            final_status,
            success_count,
            fail_count,
            skipped_count,
            json!({
                "job_id": job_id,
                "total": job.total_count,
                "success_count": success_count,
                "fail_count": fail_count,
                "skipped_count": skipped_count,
            }),
            &first_error,
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::operations::{OperationJob, OperationJobItem};
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

    fn job_fixture(module: &str, kind: &str) -> OperationJob {
        OperationJob {
            id: "auto-job-1".to_string(),
            kind: kind.to_string(),
            module: module.to_string(),
            action: "batch_reduce_price".to_string(),
            operator: "owner-a".to_string(),
            trigger_source: "schedule".to_string(),
            status: "running".to_string(),
            params: json!({ "scheduleId": "auto-1", "token": "secret-token" }),
            summary: json!({ "scheduleName": "定时所有商品批量降价" }),
            total_count: 1,
            success_count: 0,
            fail_count: 0,
            skipped_count: 0,
            started_at: 0.0,
            finished_at: 0.0,
            created_at: 0.0,
            updated_at: 0.0,
            last_error: String::new(),
        }
    }

    fn item_fixture() -> OperationJobItem {
        OperationJobItem {
            id: 42,
            job_id: "auto-job-1".to_string(),
            seq: 1,
            uid: "uid-1".to_string(),
            account_nick: "测试账号".to_string(),
            target_type: "item".to_string(),
            target_id: "ITEM-1".to_string(),
            target_title: "测试商品".to_string(),
            status: "running".to_string(),
            params: json!({ "amount": 1, "cookie": "raw-cookie" }),
            result: json!({}),
            message: String::new(),
            error_code: String::new(),
            started_at: 0.0,
            finished_at: 0.0,
            created_at: 0.0,
            updated_at: 0.0,
        }
    }

    #[test]
    fn item_job_raw_report_is_emitted_for_other_automation_jobs_only() {
        init_logging();
        let before_sequence = crate::logging::latest_sequence();

        let job = job_fixture("other-automation", "automation");
        let item = item_fixture();
        emit_item_job_raw_execution_report(
            &job,
            &item,
            "success",
            "",
            "执行成功",
            json!({ "ret": ["SUCCESS::调用成功"], "data": { "success": true } }),
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
                    && entry.job_id.as_deref() == Some("auto-job-1")
            })
            .expect("item automation raw report should be recorded");
        assert_eq!(entry.stream.as_deref(), Some("usage"));
        assert_eq!(entry.action.as_deref(), Some("batch_reduce_price"));
        assert_eq!(entry.outcome.as_deref(), Some("success"));
        assert_eq!(entry.account_uid.as_deref(), Some("uid-1"));
        assert_eq!(entry.item_id.as_deref(), Some("ITEM-1"));

        let raw = entry
            .fields_json
            .get("raw_payload_json")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(raw.contains("auto-1"));
        assert!(raw.contains("auto-job-1"));
        assert!(raw.contains("ITEM-1"));
        assert!(raw.contains("SUCCESS::调用成功"));
        assert!(!raw.contains("secret-token"));
        assert!(!raw.contains("raw-cookie"));

        let before_sequence = crate::logging::latest_sequence();
        let non_automation_job = job_fixture("items", "manual");
        emit_item_job_raw_execution_report(
            &non_automation_job,
            &item,
            "success",
            "",
            "执行成功",
            json!({ "ret": ["SUCCESS::调用成功"] }),
        );
        let logs = crate::logging::query_logs_after_sequence(
            before_sequence,
            None,
            None,
            Some("client.telemetry.action"),
            10,
        );
        assert!(!logs
            .iter()
            .any(|entry| entry.job_id.as_deref() == Some("auto-job-1")));
    }
}
