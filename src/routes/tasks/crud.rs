use axum::extract::{Extension, Json, Path, Query, State};
use axum::Json as ResponseJson;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    routes::tenant::{ensure_account_uids_accessible, owner_username_filter},
    state::AppState,
};

use super::queries::{
    append_task_log, fetch_targets_by_ids, fetch_task_account_uids, fetch_task_value,
};
use super::{
    accessible_task_account_uids, ensure_task_access, json_string, normalize_string_list,
    string_list_from_value, task_uids_accessible, unix_millis_now, unix_seconds_now,
    validate_create_payload, TaskCreatePayload, TaskListQuery, TaskUpdatePayload,
};

pub(super) async fn create_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<TaskCreatePayload>,
) -> ApiResult {
    validate_create_payload(&payload)?;
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        &payload.account_uids,
    )
    .await?;
    let target_owner = owner.as_deref().unwrap_or_default();
    let targets = fetch_targets_by_ids(
        state.storage().xianyu(),
        target_owner,
        &payload.target_item_ids,
    )
    .await?;

    let now = unix_seconds_now();
    let task_id = format!("task-{}", unix_millis_now());
    let title = payload.title.trim().to_string();
    let account_uids = normalize_string_list(payload.account_uids);
    let comment_templates = normalize_string_list(payload.comment_templates);

    sqlx::query(
        r#"
        INSERT INTO comment_tasks (
            task_id,
            title,
            status,
            source,
            target_total,
            account_total,
            comment_templates_json,
            publish_interval_min_seconds,
            publish_interval_max_seconds,
            delete_interval_min_seconds,
            delete_interval_max_seconds,
            delete_after_seconds,
            enable_delete_comments,
            created_at,
            updated_at,
            last_transition_at
        )
        VALUES (?, ?, 'draft', 'comment_targets', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&task_id)
    .bind(&title)
    .bind(targets.len() as i64)
    .bind(account_uids.len() as i64)
    .bind(json_string(&comment_templates))
    .bind(payload.publish_interval_min_seconds)
    .bind(payload.publish_interval_max_seconds)
    .bind(payload.delete_interval_min_seconds)
    .bind(payload.delete_interval_max_seconds)
    .bind(payload.delete_after_seconds)
    .bind(super::bool_to_int(payload.enable_delete_comments))
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    for (index, account_uid) in account_uids.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO comment_task_accounts (task_id, account_uid, position, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&task_id)
        .bind(account_uid)
        .bind(index as i64)
        .bind(now)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    }

    for (index, target) in targets.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO comment_task_targets (
                task_id,
                item_id,
                position,
                title,
                price,
                seller_id,
                seller_nick,
                status,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)
            "#,
        )
        .bind(&task_id)
        .bind(target.item_id.as_str())
        .bind((index + 1) as i64)
        .bind(target.title.as_str())
        .bind(target.price.as_str())
        .bind(target.seller_id.as_str())
        .bind(target.seller_nick.as_str())
        .bind(now)
        .bind(now)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    }

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
        VALUES (?, 'draft', ?, 0, 0, 0, 0, ?, 0, '', '', '任务已创建', ?)
        "#,
    )
    .bind(&task_id)
    .bind(targets.len() as i64)
    .bind(targets.len() as i64)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    append_task_log(
        state.storage().xianyu(),
        &task_id,
        "info",
        "task_created",
        "任务已创建",
        "",
        "",
        json!({
            "target_total": targets.len(),
            "account_total": account_uids.len(),
        }),
    )
    .await
    .map_err(ResponseError::internal)?;

    let task = fetch_task_value(state.storage().xianyu(), &task_id, true, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::internal_message("任务创建后读取失败"))?;
    Ok(ResponseJson(task))
}

pub(super) async fn list_tasks(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<TaskListQuery>,
) -> ApiResult {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 100);
    let offset = (page - 1) * page_size;
    let allowed_uids = accessible_task_account_uids(&state, &identity).await?;

    let rows = if query.status.trim().is_empty() {
        sqlx::query(
            r#"
            SELECT task_id
            FROM comment_tasks
            ORDER BY updated_at DESC, created_at DESC, task_id DESC
            "#,
        )
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?
    } else {
        sqlx::query(
            r#"
            SELECT task_id
            FROM comment_tasks
            WHERE status = ?
            ORDER BY updated_at DESC, created_at DESC, task_id DESC
            "#,
        )
        .bind(query.status.trim())
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?
    };

    let mut accessible_task_ids = Vec::new();
    for row in rows {
        let task_id: String = row.get("task_id");
        let task_uids = fetch_task_account_uids(state.storage().xianyu(), &task_id)
            .await
            .map_err(ResponseError::internal)?;
        if !task_uids_accessible(&task_uids, allowed_uids.as_ref()) {
            continue;
        }
        accessible_task_ids.push(task_id);
    }

    let total = accessible_task_ids.len() as i64;
    let mut items = Vec::new();
    for task_id in accessible_task_ids
        .into_iter()
        .skip(offset as usize)
        .take(page_size as usize)
    {
        if let Some(task) = fetch_task_value(state.storage().xianyu(), &task_id, false, true)
            .await
            .map_err(ResponseError::internal)?
        {
            items.push(task);
        }
    }

    Ok(ResponseJson(json!({
        "items": items,
        "pagination": {
            "page": page,
            "page_size": page_size,
            "total": total,
        }
    })))
}

pub(super) async fn get_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let task = fetch_task_value(state.storage().xianyu(), &task_id, true, true)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("任务不存在"))?;
    Ok(ResponseJson(task))
}

pub(super) async fn update_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
    Json(payload): Json<TaskUpdatePayload>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let existing = fetch_task_value(state.storage().xianyu(), &task_id, true, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("任务不存在"))?;
    let status = existing["status"].as_str().unwrap_or("draft");
    if !matches!(status, "draft" | "paused" | "failed") {
        return Err(ResponseError::bad_request("当前状态不可编辑"));
    }

    let account_uids = payload
        .account_uids
        .map(normalize_string_list)
        .unwrap_or_else(|| string_list_from_value(&existing["account_uids"]));
    let target_item_ids = payload.target_item_ids.clone().unwrap_or_else(|| {
        existing["targets"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("item_id").and_then(Value::as_str))
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let comment_templates = payload
        .comment_templates
        .map(normalize_string_list)
        .unwrap_or_else(|| string_list_from_value(&existing["comment_templates"]));

    if account_uids.is_empty() {
        return Err(ResponseError::bad_request("account_uids 不能为空"));
    }
    if target_item_ids.is_empty() {
        return Err(ResponseError::bad_request("target_item_ids 不能为空"));
    }
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(state.storage().accounts(), owner.as_deref(), &account_uids)
        .await?;
    let target_owner = owner.as_deref().unwrap_or_default();
    let targets =
        fetch_targets_by_ids(state.storage().xianyu(), target_owner, &target_item_ids).await?;
    let now = unix_seconds_now();

    sqlx::query(
        r#"
        UPDATE comment_tasks
        SET title = ?,
            account_total = ?,
            comment_templates_json = ?,
            publish_interval_min_seconds = ?,
            publish_interval_max_seconds = ?,
            delete_interval_min_seconds = ?,
            delete_interval_max_seconds = ?,
            delete_after_seconds = ?,
            enable_delete_comments = ?,
            target_total = ?,
            updated_at = ?,
            last_transition_at = ?
        WHERE task_id = ?
        "#,
    )
    .bind(
        payload
            .title
            .unwrap_or_else(|| existing["title"].as_str().unwrap_or("").to_string()),
    )
    .bind(account_uids.len() as i64)
    .bind(json_string(&comment_templates))
    .bind(
        payload.publish_interval_min_seconds.unwrap_or(
            existing["publish_interval_min_seconds"]
                .as_f64()
                .unwrap_or(0.0),
        ),
    )
    .bind(
        payload.publish_interval_max_seconds.unwrap_or(
            existing["publish_interval_max_seconds"]
                .as_f64()
                .unwrap_or(0.0),
        ),
    )
    .bind(
        payload.delete_interval_min_seconds.unwrap_or(
            existing["delete_interval_min_seconds"]
                .as_f64()
                .unwrap_or(0.0),
        ),
    )
    .bind(
        payload.delete_interval_max_seconds.unwrap_or(
            existing["delete_interval_max_seconds"]
                .as_f64()
                .unwrap_or(0.0),
        ),
    )
    .bind(
        payload.delete_after_seconds.unwrap_or(
            existing["delete_after_seconds"]
                .as_f64()
                .unwrap_or(super::default_delete_after_seconds()),
        ),
    )
    .bind(super::bool_to_int(
        payload
            .enable_delete_comments
            .unwrap_or(existing["enable_delete_comments"].as_bool().unwrap_or(true)),
    ))
    .bind(targets.len() as i64)
    .bind(now)
    .bind(now)
    .bind(&task_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    sqlx::query("DELETE FROM comment_task_accounts WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    for (index, account_uid) in account_uids.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO comment_task_accounts (task_id, account_uid, position, created_at)
            VALUES (?, ?, ?, ?)
            "#,
        )
        .bind(&task_id)
        .bind(account_uid)
        .bind(index as i64)
        .bind(now)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    }

    sqlx::query("DELETE FROM comment_task_targets WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    for (index, target) in targets.iter().enumerate() {
        sqlx::query(
            r#"
            INSERT INTO comment_task_targets (
                task_id,
                item_id,
                position,
                title,
                price,
                seller_id,
                seller_nick,
                status,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)
            "#,
        )
        .bind(&task_id)
        .bind(&target.item_id)
        .bind((index + 1) as i64)
        .bind(&target.title)
        .bind(&target.price)
        .bind(&target.seller_id)
        .bind(&target.seller_nick)
        .bind(now)
        .bind(now)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    }

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
        VALUES (
            ?,
            ?,
            ?,
            0,
            0,
            0,
            0,
            ?,
            0,
            '',
            '',
            '任务已更新',
            ?
        )
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
    .bind(status)
    .bind(targets.len() as i64)
    .bind(targets.len() as i64)
    .bind(now)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    append_task_log(
        state.storage().xianyu(),
        &task_id,
        "info",
        "task_updated",
        "任务已更新",
        "",
        "",
        json!({
            "target_total": targets.len(),
            "account_total": account_uids.len(),
        }),
    )
    .await
    .map_err(ResponseError::internal)?;

    let task = fetch_task_value(state.storage().xianyu(), &task_id, true, false)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::internal_message("任务更新后读取失败"))?;
    Ok(ResponseJson(task))
}

pub(super) async fn delete_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    sqlx::query("DELETE FROM comment_delete_jobs WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM comment_task_attempts WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM comment_task_logs WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM comment_task_progress WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM comment_task_accounts WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM comment_task_targets WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    sqlx::query("DELETE FROM comment_tasks WHERE task_id = ?")
        .bind(&task_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "status": "ok",
        "task_id": task_id,
    })))
}

pub(super) async fn list_task_logs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
    Query(query): Query<super::TaskLogsQuery>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let items = super::queries::query_task_logs(
        state.storage().xianyu(),
        &task_id,
        query.limit.clamp(1, 200),
        query.offset.max(0),
        &query.level,
        &query.account_uid,
        &query.item_id,
    )
    .await
    .map_err(ResponseError::internal)?;
    let total = super::queries::count_task_logs(
        state.storage().xianyu(),
        &task_id,
        &query.level,
        &query.account_uid,
        &query.item_id,
    )
    .await
    .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "items": items,
        "pagination": {
            "limit": query.limit.clamp(1, 200),
            "offset": query.offset.max(0),
            "total": total,
        }
    })))
}

pub(super) async fn list_task_attempts(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(task_id): Path<String>,
    Query(query): Query<super::TaskAttemptsQuery>,
) -> ApiResult {
    ensure_task_access(&state, &identity, &task_id).await?;
    let success_filter = super::normalize_success_filter(&query.success);
    let items = super::queries::query_task_attempts(
        state.storage().xianyu(),
        &task_id,
        query.limit.clamp(1, 200),
        query.offset.max(0),
        &query.account_uid,
        &query.item_id,
        success_filter,
    )
    .await
    .map_err(ResponseError::internal)?;
    let total = super::queries::count_task_attempts(
        state.storage().xianyu(),
        &task_id,
        &query.account_uid,
        &query.item_id,
        success_filter,
    )
    .await
    .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "items": items,
        "pagination": {
            "limit": query.limit.clamp(1, 200),
            "offset": query.offset.max(0),
            "total": total,
        }
    })))
}
