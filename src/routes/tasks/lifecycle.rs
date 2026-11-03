use axum::extract::{Extension, Path, State};
use axum::Json as ResponseJson;
use serde_json::json;
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::queries::{
    append_task_log, fetch_task_account_uids, fetch_task_progress, fetch_task_targets,
    fetch_task_value,
};
use super::{
    default_delete_after_seconds, ensure_task_access, hash_text, unix_millis_now, unix_seconds_now,
};

pub(super) async fn get_task_progress(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let task = fetch_task_value(state.storage().xianyu(), &task_id, false, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("任务不存在"))?;
    let progress = task.get("progress").cloned().unwrap_or_else(|| json!({}));
    Ok(ResponseJson(progress))
}

pub(super) async fn start_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let current = fetch_task_value(state.storage().xianyu(), &task_id, true, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("任务不存在"))?;
    let current_status = current["status"].as_str().unwrap_or("draft");
    if !matches!(current_status, "draft" | "paused" | "failed") {
        return Err(ResponseError::bad_request("当前状态不可启动"));
    }

    let account_uids = fetch_task_account_uids(state.storage().xianyu(), &task_id)
        .await
        .map_err(ResponseError::internal)?;
    let comment_templates = current["comment_templates"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|value| value.as_str())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let targets = fetch_task_targets(state.storage().xianyu(), &task_id)
        .await
        .map_err(ResponseError::internal)?;
    let now = unix_seconds_now();
    let enable_delete_comments = current["enable_delete_comments"].as_bool().unwrap_or(true);
    let delete_after_seconds = current["delete_after_seconds"]
        .as_f64()
        .unwrap_or(default_delete_after_seconds());

    for (index, target) in targets.iter().enumerate() {
        let target_status = target["status"].as_str().unwrap_or("pending");
        if matches!(target_status, "success" | "failed" | "skipped") {
            continue;
        }
        let item_id = target["item_id"].as_str().unwrap_or_default();
        let existing_attempts: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM comment_task_attempts WHERE task_id = ? AND item_id = ?",
        )
        .bind(&task_id)
        .bind(item_id)
        .fetch_one(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
        if existing_attempts > 0 {
            continue;
        }

        let account_uid = account_uids
            .get(index % account_uids.len())
            .cloned()
            .unwrap_or_default();
        let comment_content = comment_templates
            .get(index % comment_templates.len().max(1))
            .cloned()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("自动评论 {}", index + 1));
        let comment_hash = hash_text(&comment_content);
        let published_comment_id = format!("comment-{}", unix_millis_now() + index as i64);
        let delete_job_payload = json!({
            "simulated": true,
            "scheduled_at": now + delete_after_seconds,
        });

        let attempt_result = sqlx::query(
            r#"
                INSERT INTO comment_task_attempts (
                    task_id,
                    account_uid,
                    item_id,
                    target_status,
                    success,
                    comment_content,
                    comment_content_hash,
                    source_comment_json,
                    published_comment_id,
                    published_at,
                    request_payload_json,
                    response_payload_json,
                    delete_job_payload_json,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, 'published', 1, ?, ?, '{}', ?, ?, ?, ?, ?, ?, ?)
                "#,
        )
        .bind(&task_id)
        .bind(&account_uid)
        .bind(item_id)
        .bind(&comment_content)
        .bind(&comment_hash)
        .bind(&published_comment_id)
        .bind(now)
        .bind(json!({"mode": "simulated"}).to_string())
        .bind(json!({"status": "published", "simulated": true}).to_string())
        .bind(delete_job_payload.to_string())
        .bind(now)
        .bind(now)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
        let attempt_id = attempt_result.last_insert_rowid();

        sqlx::query(
                "UPDATE comment_task_targets SET status = 'success', commented_account_uid = ?, updated_at = ? WHERE task_id = ? AND item_id = ?",
            )
            .bind(&account_uid)
            .bind(now)
            .bind(&task_id)
            .bind(item_id)
            .execute(state.storage().xianyu())
            .await
            .map_err(ResponseError::internal)?;

        if enable_delete_comments {
            let existing_delete_job: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM comment_delete_jobs WHERE attempt_id = ?")
                    .bind(attempt_id)
                    .fetch_one(state.storage().xianyu())
                    .await
                    .map_err(ResponseError::internal)?;
            if existing_delete_job > 0 {
                continue;
            }
            sqlx::query(
                r#"
                    INSERT INTO comment_delete_jobs (
                        task_id,
                        attempt_id,
                        account_uid,
                        item_id,
                        comment_id,
                        status,
                        payload_json,
                        scheduled_at,
                        created_at,
                        updated_at
                    )
                    VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?)
                    "#,
            )
            .bind(&task_id)
            .bind(attempt_id)
            .bind(&account_uid)
            .bind(item_id)
            .bind(&published_comment_id)
            .bind(delete_job_payload.to_string())
            .bind(now + delete_after_seconds)
            .bind(now)
            .bind(now)
            .execute(state.storage().xianyu())
            .await
            .map_err(ResponseError::internal)?;
        }
    }

    if enable_delete_comments {
        let rows = sqlx::query(
            r#"
            SELECT
                a.id AS attempt_id,
                a.account_uid,
                a.item_id,
                a.published_comment_id,
                a.delete_job_payload_json,
                a.published_at,
                d.id AS delete_job_id
            FROM comment_task_attempts a
            LEFT JOIN comment_delete_jobs d ON d.attempt_id = a.id
            WHERE a.task_id = ?
              AND a.success = 1
            ORDER BY a.id ASC
            "#,
        )
        .bind(&task_id)
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;

        for row in rows {
            let delete_job_id: Option<i64> = row.try_get("delete_job_id").ok();
            if delete_job_id.is_some() {
                continue;
            }
            let attempt_id: i64 = row.get("attempt_id");
            let account_uid: String = row.get("account_uid");
            let item_id: String = row.get("item_id");
            let comment_id: String = row.get("published_comment_id");
            let payload_json: String = row.get("delete_job_payload_json");
            let published_at: f64 = row.get("published_at");
            let scheduled_at =
                if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&payload_json) {
                    payload
                        .get("scheduled_at")
                        .and_then(|value| value.as_f64())
                        .unwrap_or(published_at + delete_after_seconds)
                } else {
                    published_at + delete_after_seconds
                };

            sqlx::query(
                r#"
                INSERT INTO comment_delete_jobs (
                    task_id,
                    attempt_id,
                    account_uid,
                    item_id,
                    comment_id,
                    status,
                    payload_json,
                    scheduled_at,
                    created_at,
                    updated_at
                )
                VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?)
                "#,
            )
            .bind(&task_id)
            .bind(attempt_id)
            .bind(&account_uid)
            .bind(&item_id)
            .bind(&comment_id)
            .bind(payload_json)
            .bind(scheduled_at)
            .bind(now)
            .bind(now)
            .execute(state.storage().xianyu())
            .await
            .map_err(ResponseError::internal)?;
        }
    }

    let total_targets: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM comment_task_targets WHERE task_id = ?")
            .bind(&task_id)
            .fetch_one(state.storage().xianyu())
            .await
            .map_err(ResponseError::internal)?;
    let successful_targets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comment_task_targets WHERE task_id = ? AND status = 'success'",
    )
    .bind(&task_id)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    let failed_targets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comment_task_targets WHERE task_id = ? AND status = 'failed'",
    )
    .bind(&task_id)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    let skipped_targets: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM comment_task_targets WHERE task_id = ? AND status = 'skipped'",
    )
    .bind(&task_id)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    let completed_targets = successful_targets + failed_targets + skipped_targets;
    let pending_targets = (total_targets - completed_targets).max(0);
    let percent = if total_targets <= 0 {
        0.0
    } else {
        ((completed_targets as f64 / total_targets as f64) * 10000.0).round() / 100.0
    };
    let next_status = if enable_delete_comments {
        "waiting_delete"
    } else {
        "finished"
    };
    let next_message = if enable_delete_comments {
        "已生成删评队列"
    } else {
        "任务已完成"
    };
    sqlx::query(
        r#"
        UPDATE comment_tasks
        SET status = ?,
            started_at = CASE WHEN started_at <= 0 THEN ? ELSE started_at END,
            finished_at = CASE WHEN ? = 'finished' THEN ? ELSE finished_at END,
            updated_at = ?,
            last_transition_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(next_status)
    .bind(now)
    .bind(next_status)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(&task_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    sqlx::query(
        r#"
        INSERT INTO comment_task_progress (
            task_id,
            status,
            total_targets,
            completed_targets,
            successful_targets,
            failed_targets,
            skipped_targets,
            pending_targets,
            percent,
            current_target_id,
            current_account_uid,
            last_message,
            updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '', '', ?, ?)
        ON CONFLICT(task_id) DO UPDATE SET
            status = excluded.status,
            total_targets = excluded.total_targets,
            completed_targets = excluded.completed_targets,
            successful_targets = excluded.successful_targets,
            failed_targets = excluded.failed_targets,
            skipped_targets = excluded.skipped_targets,
            pending_targets = excluded.pending_targets,
            percent = excluded.percent,
            current_target_id = excluded.current_target_id,
            current_account_uid = excluded.current_account_uid,
            last_message = excluded.last_message,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&task_id)
    .bind(next_status)
    .bind(total_targets)
    .bind(completed_targets)
    .bind(successful_targets)
    .bind(failed_targets)
    .bind(skipped_targets)
    .bind(pending_targets)
    .bind(percent)
    .bind(next_message)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    append_task_log(
        state.storage().xianyu(),
        &task_id,
        "info",
        "task_started",
        next_message,
        "",
        "",
        json!({
            "status": next_status,
            "successful_targets": successful_targets,
            "completed_targets": completed_targets,
            "pending_targets": pending_targets,
        }),
    )
    .await
    .map_err(ResponseError::internal)?;

    let task = fetch_task_value(state.storage().xianyu(), &task_id, true, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::internal_message("任务启动后读取失败"))?;
    Ok(ResponseJson(json!({
        "status": "ok",
        "task_id": task_id,
        "task": task,
    })))
}

pub(super) async fn pause_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let current = fetch_task_value(state.storage().xianyu(), &task_id, false, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("任务不存在"))?;
    let current_status = current["status"].as_str().unwrap_or("draft");
    if !matches!(current_status, "queued" | "running" | "waiting_delete") {
        return Err(ResponseError::bad_request("当前状态不可暂停"));
    }
    let now = unix_seconds_now();
    sqlx::query(
        r#"
        UPDATE comment_tasks
        SET status = 'paused',
            paused_at = ?,
            updated_at = ?,
            last_transition_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(&task_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    let progress = fetch_task_progress(state.storage().xianyu(), &task_id)
        .await
        .map_err(ResponseError::internal)?;
    let total_targets = current["target_total"].as_i64().unwrap_or(0);
    let completed_targets = progress
        .as_ref()
        .and_then(|value| value.get("completed_targets"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let successful_targets = progress
        .as_ref()
        .and_then(|value| value.get("successful_targets"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let failed_targets = progress
        .as_ref()
        .and_then(|value| value.get("failed_targets"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let skipped_targets = progress
        .as_ref()
        .and_then(|value| value.get("skipped_targets"))
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let pending_targets = (total_targets - completed_targets).max(0);
    let percent = if total_targets <= 0 {
        0.0
    } else {
        ((completed_targets as f64 / total_targets as f64) * 10000.0).round() / 100.0
    };

    sqlx::query(
        r#"
        INSERT INTO comment_task_progress (
            task_id,
            status,
            total_targets,
            completed_targets,
            successful_targets,
            failed_targets,
            skipped_targets,
            pending_targets,
            percent,
            current_target_id,
            current_account_uid,
            last_message,
            updated_at
        )
        VALUES (?, 'paused', ?, ?, ?, ?, ?, ?, ?, '', '', '任务已暂停', ?)
        ON CONFLICT(task_id) DO UPDATE SET
            status = excluded.status,
            total_targets = excluded.total_targets,
            completed_targets = excluded.completed_targets,
            successful_targets = excluded.successful_targets,
            failed_targets = excluded.failed_targets,
            skipped_targets = excluded.skipped_targets,
            pending_targets = excluded.pending_targets,
            percent = excluded.percent,
            current_target_id = excluded.current_target_id,
            current_account_uid = excluded.current_account_uid,
            last_message = excluded.last_message,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&task_id)
    .bind(total_targets)
    .bind(completed_targets)
    .bind(successful_targets)
    .bind(failed_targets)
    .bind(skipped_targets)
    .bind(pending_targets)
    .bind(percent)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    append_task_log(
        state.storage().xianyu(),
        &task_id,
        "warning",
        "task_paused",
        "任务已暂停",
        "",
        "",
        json!({}),
    )
    .await
    .map_err(ResponseError::internal)?;

    let task = fetch_task_value(state.storage().xianyu(), &task_id, true, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::internal_message("任务暂停后读取失败"))?;
    Ok(ResponseJson(json!({
        "status": "ok",
        "task_id": task_id,
        "task": task,
    })))
}
