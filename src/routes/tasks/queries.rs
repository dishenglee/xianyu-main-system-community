use std::collections::HashSet;

use serde_json::{json, Map, Value};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::routes::{error::ResponseError, tenant::ensure_account_uids_accessible};

use super::{
    bool_to_int, delete_job_row_to_value, task_attempt_row_to_value, task_log_row_to_value,
    task_row_to_value, unix_seconds_now, CommentTargetRecord,
};

fn default_progress(task_id: &str, status: &str, total_targets: i64, updated_at: f64) -> Value {
    json!({
        "task_id": task_id,
        "status": status,
        "total_targets": total_targets,
        "completed_targets": 0,
        "successful_targets": 0,
        "failed_targets": 0,
        "skipped_targets": 0,
        "pending_targets": total_targets.max(0),
        "percent": 0.0,
        "current_target_id": "",
        "current_account_uid": "",
        "last_message": "",
        "updated_at": updated_at,
    })
}

pub(super) async fn fetch_task_progress(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<Value>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
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
        FROM comment_task_progress
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| {
        json!({
            "task_id": row.get::<String, _>("task_id"),
            "status": row.get::<String, _>("status"),
            "total_targets": row.get::<i64, _>("total_targets"),
            "completed_targets": row.get::<i64, _>("completed_targets"),
            "successful_targets": row.get::<i64, _>("successful_targets"),
            "failed_targets": row.get::<i64, _>("failed_targets"),
            "skipped_targets": row.get::<i64, _>("skipped_targets"),
            "pending_targets": row.get::<i64, _>("pending_targets"),
            "percent": row.get::<f64, _>("percent"),
            "current_target_id": row.get::<String, _>("current_target_id"),
            "current_account_uid": row.get::<String, _>("current_account_uid"),
            "last_message": row.get::<String, _>("last_message"),
            "updated_at": row.get::<f64, _>("updated_at"),
        })
    }))
}

pub(super) async fn fetch_task_account_uids(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<String>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT account_uid
        FROM comment_task_accounts
        WHERE task_id = ?
        ORDER BY position ASC, created_at ASC, account_uid ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| row.get::<String, _>("account_uid"))
        .collect())
}

pub(super) async fn fetch_task_row(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Option<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    sqlx::query("SELECT * FROM comment_tasks WHERE task_id = ?")
        .bind(task_id)
        .fetch_optional(pool)
        .await
}

pub(super) async fn fetch_task_value(
    pool: &SqlitePool,
    task_id: &str,
    include_targets: bool,
    include_summaries: bool,
) -> Result<Option<Value>, sqlx::Error> {
    let Some(row) = fetch_task_row(pool, task_id).await? else {
        return Ok(None);
    };
    let task_id_value = row.get::<String, _>("task_id");
    let status = row.get::<String, _>("status");
    let total_targets = row.get::<i64, _>("target_total");
    let updated_at = row.get::<f64, _>("updated_at");

    let mut task = task_row_to_value(row);
    task["account_uids"] = Value::Array(
        fetch_task_account_uids(pool, &task_id_value)
            .await?
            .into_iter()
            .map(Value::from)
            .collect(),
    );
    task["progress"] = fetch_task_progress(pool, &task_id_value)
        .await?
        .unwrap_or_else(|| default_progress(&task_id_value, &status, total_targets, updated_at));

    if include_targets {
        let targets = fetch_task_targets(pool, &task_id_value).await?;
        task["targets"] = Value::Array(targets);
    }
    if include_summaries {
        task["attempt_summary"] = attempt_summary(pool, &task_id_value).await?;
        task["delete_job_summary"] = delete_job_summary(pool, &task_id_value).await?;
    }
    Ok(Some(task))
}

pub(super) async fn fetch_task_targets(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT item_id, position, title, price, seller_id, seller_nick, status, created_at, updated_at, commented_account_uid
        FROM comment_task_targets
        WHERE task_id = ?
        ORDER BY position ASC, item_id ASC
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            json!({
                "item_id": row.get::<String, _>("item_id"),
                "position": row.get::<i64, _>("position"),
                "title": row.get::<String, _>("title"),
                "price": row.get::<String, _>("price"),
                "seller_id": row.get::<String, _>("seller_id"),
                "seller_nick": row.get::<String, _>("seller_nick"),
                "status": row.get::<String, _>("status"),
                "commented_account_uid": row.get::<String, _>("commented_account_uid"),
                "created_at": row.get::<f64, _>("created_at"),
                "updated_at": row.get::<f64, _>("updated_at"),
            })
        })
        .collect())
}

pub(super) async fn fetch_targets_by_ids(
    pool: &SqlitePool,
    owner_username: &str,
    item_ids: &[String],
) -> Result<Vec<CommentTargetRecord>, ResponseError> {
    if item_ids.is_empty() {
        return Err(ResponseError::bad_request("target_item_ids 不能为空"));
    }

    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT item_id, title, price, seller_id, seller_nick FROM comment_targets WHERE owner_username = ",
    );
    query.push_bind(owner_username.trim());
    query.push(" AND item_id IN (");
    let mut separated = query.separated(", ");
    for item_id in item_ids {
        separated.push_bind(item_id);
    }
    separated.push_unseparated(")");

    let rows = query
        .build()
        .fetch_all(pool)
        .await
        .map_err(ResponseError::internal)?;

    let records = rows
        .into_iter()
        .map(|row| CommentTargetRecord {
            item_id: row.get("item_id"),
            title: row.get("title"),
            price: row.get("price"),
            seller_id: row.get("seller_id"),
            seller_nick: row.get("seller_nick"),
        })
        .collect::<Vec<_>>();
    if records.len() != item_ids.len() {
        return Err(ResponseError::bad_request("存在未持久化的评论目标"));
    }

    let mut ordered = Vec::with_capacity(item_ids.len());
    for item_id in item_ids {
        if let Some(record) = records.iter().find(|record| &record.item_id == item_id) {
            ordered.push(record.clone());
        }
    }
    Ok(ordered)
}

pub(super) async fn ensure_accounts_exist(
    pool: &SqlitePool,
    account_uids: &[String],
) -> Result<(), ResponseError> {
    ensure_account_uids_accessible(pool, None, account_uids).await
}

pub(super) async fn ensure_task_exists(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<(), ResponseError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comment_tasks WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(pool)
        .await
        .map_err(ResponseError::internal)?;
    if count <= 0 {
        return Err(ResponseError::not_found("任务不存在"));
    }
    Ok(())
}

pub(super) async fn count_existing_attempts(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM comment_task_attempts WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(pool)
        .await
}

pub(super) async fn query_task_logs(
    pool: &SqlitePool,
    task_id: &str,
    limit: i64,
    offset: i64,
    level: &str,
    account_uid: &str,
    item_id: &str,
) -> Result<Vec<Value>, sqlx::Error> {
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT id, task_id, level, event, message, context_json, created_at, account_uid, item_id FROM comment_task_logs WHERE task_id = ",
    );
    builder.push_bind(task_id);
    if !level.trim().is_empty() {
        builder.push(" AND level = ").push_bind(level.trim());
    }
    if !account_uid.trim().is_empty() {
        builder
            .push(" AND account_uid = ")
            .push_bind(account_uid.trim());
    }
    if !item_id.trim().is_empty() {
        builder.push(" AND item_id = ").push_bind(item_id.trim());
    }
    builder
        .push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = builder.build().fetch_all(pool).await?;
    Ok(rows.into_iter().map(task_log_row_to_value).collect())
}

pub(super) async fn count_task_logs(
    pool: &SqlitePool,
    task_id: &str,
    level: &str,
    account_uid: &str,
    item_id: &str,
) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) as total FROM comment_task_logs WHERE task_id = ",
    );
    builder.push_bind(task_id);
    if !level.trim().is_empty() {
        builder.push(" AND level = ").push_bind(level.trim());
    }
    if !account_uid.trim().is_empty() {
        builder
            .push(" AND account_uid = ")
            .push_bind(account_uid.trim());
    }
    if !item_id.trim().is_empty() {
        builder.push(" AND item_id = ").push_bind(item_id.trim());
    }
    let row = builder.build().fetch_one(pool).await?;
    Ok(row.get::<i64, _>("total"))
}

pub(super) async fn query_task_attempts(
    pool: &SqlitePool,
    task_id: &str,
    limit: i64,
    offset: i64,
    account_uid: &str,
    item_id: &str,
    success: Option<bool>,
) -> Result<Vec<Value>, sqlx::Error> {
    let mut builder =
        QueryBuilder::<Sqlite>::new("SELECT * FROM comment_task_attempts WHERE task_id = ");
    builder.push_bind(task_id);
    if !account_uid.trim().is_empty() {
        builder
            .push(" AND account_uid = ")
            .push_bind(account_uid.trim());
    }
    if !item_id.trim().is_empty() {
        builder.push(" AND item_id = ").push_bind(item_id.trim());
    }
    if let Some(success) = success {
        builder
            .push(" AND success = ")
            .push_bind(bool_to_int(success));
    }
    builder
        .push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = builder.build().fetch_all(pool).await?;
    Ok(rows.into_iter().map(task_attempt_row_to_value).collect())
}

pub(super) async fn count_task_attempts(
    pool: &SqlitePool,
    task_id: &str,
    account_uid: &str,
    item_id: &str,
    success: Option<bool>,
) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) as total FROM comment_task_attempts WHERE task_id = ",
    );
    builder.push_bind(task_id);
    if !account_uid.trim().is_empty() {
        builder
            .push(" AND account_uid = ")
            .push_bind(account_uid.trim());
    }
    if !item_id.trim().is_empty() {
        builder.push(" AND item_id = ").push_bind(item_id.trim());
    }
    if let Some(success) = success {
        builder
            .push(" AND success = ")
            .push_bind(bool_to_int(success));
    }
    let row = builder.build().fetch_one(pool).await?;
    Ok(row.get::<i64, _>("total"))
}

pub(super) async fn query_delete_jobs(
    pool: &SqlitePool,
    task_id: &str,
    status: &str,
    account_uid: &str,
    item_id: &str,
    allowed_account_uids: Option<&HashSet<String>>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    let mut builder = QueryBuilder::<Sqlite>::new("SELECT * FROM comment_delete_jobs WHERE 1 = 1");
    if !task_id.trim().is_empty() {
        builder.push(" AND task_id = ").push_bind(task_id.trim());
    }
    if !status.trim().is_empty() {
        builder.push(" AND status = ").push_bind(status.trim());
    }
    if !account_uid.trim().is_empty() {
        builder
            .push(" AND account_uid = ")
            .push_bind(account_uid.trim());
    }
    push_allowed_account_uid_filter(&mut builder, allowed_account_uids);
    if !item_id.trim().is_empty() {
        builder.push(" AND item_id = ").push_bind(item_id.trim());
    }
    builder
        .push(" ORDER BY created_at DESC, id DESC LIMIT ")
        .push_bind(limit)
        .push(" OFFSET ")
        .push_bind(offset);

    let rows = builder.build().fetch_all(pool).await?;
    Ok(rows.into_iter().map(delete_job_row_to_value).collect())
}

pub(super) async fn count_delete_jobs(
    pool: &SqlitePool,
    task_id: &str,
    status: &str,
    account_uid: &str,
    item_id: &str,
    allowed_account_uids: Option<&HashSet<String>>,
) -> Result<i64, sqlx::Error> {
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT COUNT(*) as total FROM comment_delete_jobs WHERE 1 = 1",
    );
    if !task_id.trim().is_empty() {
        builder.push(" AND task_id = ").push_bind(task_id.trim());
    }
    if !status.trim().is_empty() {
        builder.push(" AND status = ").push_bind(status.trim());
    }
    if !account_uid.trim().is_empty() {
        builder
            .push(" AND account_uid = ")
            .push_bind(account_uid.trim());
    }
    push_allowed_account_uid_filter(&mut builder, allowed_account_uids);
    if !item_id.trim().is_empty() {
        builder.push(" AND item_id = ").push_bind(item_id.trim());
    }
    let row = builder.build().fetch_one(pool).await?;
    Ok(row.get::<i64, _>("total"))
}

fn push_allowed_account_uid_filter<'a>(
    builder: &mut QueryBuilder<'a, Sqlite>,
    allowed_account_uids: Option<&'a HashSet<String>>,
) {
    let Some(allowed) = allowed_account_uids else {
        return;
    };
    if allowed.is_empty() {
        builder.push(" AND 1 = 0");
        return;
    }

    builder.push(" AND account_uid IN (");
    let mut separated = builder.separated(", ");
    for uid in allowed {
        separated.push_bind(uid);
    }
    separated.push_unseparated(")");
}

pub(super) async fn fetch_delete_job(
    pool: &SqlitePool,
    job_id: i64,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query("SELECT * FROM comment_delete_jobs WHERE id = ?")
        .bind(job_id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(delete_job_row_to_value))
}

pub(super) async fn attempt_summary(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Value, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT
            COUNT(*) AS total,
            COALESCE(SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END), 0) AS success_total,
            COALESCE(SUM(CASE WHEN success = 0 AND target_status = 'skipped' THEN 1 ELSE 0 END), 0) AS skipped_total,
            COALESCE(SUM(CASE WHEN success = 0 AND target_status != 'skipped' THEN 1 ELSE 0 END), 0) AS failed_total,
            CAST(COALESCE(MAX(created_at), 0) AS REAL) AS latest_created_at
        FROM comment_task_attempts
        WHERE task_id = ?
        "#,
    )
    .bind(task_id)
    .fetch_one(pool)
    .await?;

    Ok(json!({
        "total": row.get::<i64, _>("total"),
        "success": row.get::<i64, _>("success_total"),
        "failed": row.get::<i64, _>("failed_total"),
        "skipped": row.get::<i64, _>("skipped_total"),
        "latest_created_at": row.get::<f64, _>("latest_created_at"),
    }))
}

pub(super) async fn delete_job_summary(
    pool: &SqlitePool,
    task_id: &str,
) -> Result<Value, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT status, COUNT(*) AS total, COALESCE(MAX(created_at), 0) AS latest_created_at
        FROM comment_delete_jobs
        WHERE task_id = ?
        GROUP BY status
        "#,
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;
    let mut counts = Map::new();
    let mut total = 0_i64;
    let mut latest_created_at = 0.0_f64;
    for row in rows {
        let status: String = row.get("status");
        let count: i64 = row.get("total");
        total += count;
        latest_created_at = latest_created_at.max(row.get::<f64, _>("latest_created_at"));
        counts.insert(status, Value::from(count));
    }

    Ok(json!({
        "total": total,
        "pending": counts.get("pending").cloned().unwrap_or(Value::from(0)),
        "running": counts.get("running").cloned().unwrap_or(Value::from(0)),
        "retryable": counts.get("retryable").cloned().unwrap_or(Value::from(0)),
        "succeeded": counts.get("succeeded").cloned().unwrap_or(Value::from(0)),
        "failed": counts.get("failed").cloned().unwrap_or(Value::from(0)),
        "cancelled": counts.get("cancelled").cloned().unwrap_or(Value::from(0)),
        "already_removed": counts.get("already_removed").cloned().unwrap_or(Value::from(0)),
        "item_gone": counts.get("item_gone").cloned().unwrap_or(Value::from(0)),
        "latest_created_at": latest_created_at,
    }))
}

pub(super) async fn append_task_log(
    pool: &SqlitePool,
    task_id: &str,
    level: &str,
    event: &str,
    message: &str,
    account_uid: &str,
    item_id: &str,
    context: Value,
) -> Result<(), sqlx::Error> {
    let now = unix_seconds_now();
    sqlx::query(
        r#"
        INSERT INTO comment_task_logs (task_id, level, event, message, context_json, account_uid, item_id, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(task_id)
    .bind(level)
    .bind(event)
    .bind(message)
    .bind(context.to_string())
    .bind(account_uid)
    .bind(item_id)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query("UPDATE comment_tasks SET last_log_at = ?, updated_at = ? WHERE task_id = ?")
        .bind(now)
        .bind(now)
        .bind(task_id)
        .execute(pool)
        .await?;
    Ok(())
}
