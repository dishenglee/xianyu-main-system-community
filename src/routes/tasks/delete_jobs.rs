use axum::extract::{Extension, Path, Query, State};
use axum::Json as ResponseJson;
use serde_json::{json, Value};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::queries::{count_delete_jobs, fetch_delete_job, query_delete_jobs};
use super::DeleteJobsQuery;
use super::{accessible_task_account_uids, unix_seconds_now};

pub(super) async fn list_comment_delete_jobs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DeleteJobsQuery>,
) -> ApiResult {
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let offset = (page - 1) * page_size;
    let allowed_uids = accessible_task_account_uids(&state, &identity).await?;

    let items = query_delete_jobs(
        state.storage().xianyu(),
        &query.task_id,
        &query.status,
        &query.account_uid,
        &query.item_id,
        allowed_uids.as_ref(),
        page_size,
        offset,
    )
    .await
    .map_err(ResponseError::internal)?;
    let total = count_delete_jobs(
        state.storage().xianyu(),
        &query.task_id,
        &query.status,
        &query.account_uid,
        &query.item_id,
        allowed_uids.as_ref(),
    )
    .await
    .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "items": items,
        "pagination": {
            "page": page,
            "page_size": page_size,
            "total": total,
        }
    })))
}

pub(super) async fn retry_comment_delete_job(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(job_id): Path<i64>,
) -> ApiResult {
    let job = fetch_delete_job(state.storage().xianyu(), job_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("删评任务不存在"))?;
    ensure_delete_job_access(&state, &identity, &job).await?;
    let now = unix_seconds_now();
    sqlx::query(
        r#"
        UPDATE comment_delete_jobs
        SET status = 'pending',
            retry_count = retry_count + 1,
            scheduled_at = ?,
            cancelled_at = 0,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(now)
    .bind(job_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    let updated = fetch_delete_job(state.storage().xianyu(), job_id)
        .await
        .map_err(ResponseError::internal)?
        .or(Some(job))
        .ok_or_else(|| ResponseError::internal_message("删评任务重试后读取失败"))?;

    Ok(ResponseJson(json!({
        "status": "ok",
        "job": updated,
    })))
}

pub(super) async fn cancel_comment_delete_job(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(job_id): Path<i64>,
) -> ApiResult {
    let job = fetch_delete_job(state.storage().xianyu(), job_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("删评任务不存在"))?;
    ensure_delete_job_access(&state, &identity, &job).await?;
    let now = unix_seconds_now();
    sqlx::query(
        r#"
        UPDATE comment_delete_jobs
        SET status = 'cancelled',
            cancelled_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(now)
    .bind(job_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;

    let updated = fetch_delete_job(state.storage().xianyu(), job_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::internal_message("删评任务取消后读取失败"))?;

    Ok(ResponseJson(json!({
        "status": "ok",
        "job": updated,
    })))
}

async fn ensure_delete_job_access(
    state: &AppState,
    identity: &RequestIdentity,
    job: &Value,
) -> Result<(), ResponseError> {
    let Some(allowed_uids) = accessible_task_account_uids(state, identity).await? else {
        return Ok(());
    };
    let account_uid = job
        .get("account_uid")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if allowed_uids.contains(account_uid) {
        Ok(())
    } else {
        Err(ResponseError::not_found("删评任务不存在"))
    }
}
