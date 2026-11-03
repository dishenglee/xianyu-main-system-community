use axum::{
    extract::{Extension, Json, Path, Query, State},
    routing::get,
    Json as ResponseJson, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

use crate::{
    models::{
        auth::RequestIdentity,
        operations::{
            MessageDailyStatFilter, OperationJobItem, OperationLogEntry, OperationLogFilter,
        },
    },
    routes::{
        error::{ApiResult, ResponseError},
        tenant::{accessible_account_uids, ensure_account_uids_accessible, owner_username_filter},
    },
    state::AppState,
};
use sqlx::Row;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/operation-logs"),
            get(list_operation_logs),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/operation-jobs"),
            get(list_operation_jobs).post(create_operation_job),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/operation-jobs/:id"),
            get(get_operation_job),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message-stats/daily"),
            get(list_message_daily_stats),
        )
}

#[derive(Debug, Deserialize, Default)]
struct OperationLogsQuery {
    #[serde(default)]
    module: String,
    #[serde(default)]
    action: String,
    #[serde(default)]
    uid: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    keyword: String,
    #[serde(default = "default_logs_limit")]
    limit: i64,
    #[serde(default)]
    since: f64,
}

#[derive(Debug, Deserialize, Default)]
struct MessageStatsQuery {
    #[serde(default)]
    uid: String,
    #[serde(default)]
    start_date: String,
    #[serde(default)]
    end_date: String,
    #[serde(default = "default_stats_limit")]
    limit: i64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateOperationJobPayload {
    pub kind: String,
    pub module: String,
    pub action: String,
    #[serde(default)]
    pub operator: String,
    #[serde(default = "default_trigger_source")]
    pub trigger_source: String,
    #[serde(default)]
    pub summary: Value,
    pub items: Vec<CreateOperationJobItemPayload>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct CreateOperationJobItemPayload {
    pub uid: String,
    #[serde(default)]
    pub account_nick: String,
    #[serde(default)]
    pub target_type: String,
    pub target_id: String,
    #[serde(default)]
    pub target_title: String,
    #[serde(default)]
    pub params: Value,
}

fn default_logs_limit() -> i64 {
    200
}

fn default_stats_limit() -> i64 {
    30
}

fn default_trigger_source() -> String {
    "manual".to_string()
}

fn next_operation_job_id() -> String {
    format!("op-{}", crate::storage::unix_millis_now())
}

fn operation_log_extra_with_correlation(summary: &Value, identity: &RequestIdentity) -> Value {
    let trace_id = identity.correlation.trace_id.trim();
    let request_id = identity
        .correlation
        .request_id
        .as_deref()
        .unwrap_or_default()
        .trim();

    let mut extra = match summary {
        Value::Object(map) => Value::Object(map.clone()),
        Value::Null => json!({}),
        other => json!({
            "summary": other.clone(),
        }),
    };

    if let Some(map) = extra.as_object_mut() {
        if !trace_id.is_empty() {
            map.insert("traceId".to_string(), Value::String(trace_id.to_string()));
        }
        if !request_id.is_empty() {
            map.insert(
                "requestId".to_string(),
                Value::String(request_id.to_string()),
            );
        }
        if !identity.username.trim().is_empty() {
            map.insert(
                "operatorUsername".to_string(),
                Value::String(identity.username.trim().to_string()),
            );
        }
    }

    extra
}

async fn list_operation_logs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<OperationLogsQuery>,
) -> ApiResult {
    let items = list_scoped_operation_logs(&state, &identity, &query).await?;
    Ok(ResponseJson(json!({
        "items": items,
        "count": items.len(),
    })))
}

async fn list_operation_jobs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let jobs = state
        .storage()
        .list_operation_jobs(200)
        .await
        .map_err(ResponseError::internal)?;
    let allowed_uids = scoped_account_uids(&state, &identity).await?;
    let mut items = Vec::new();
    for job in jobs {
        let job_items = state
            .storage()
            .list_operation_job_items(&job.id)
            .await
            .map_err(ResponseError::internal)?;
        if operation_items_accessible(&job_items, allowed_uids.as_ref()) {
            items.push(job);
        }
        if items.len() >= 100 {
            break;
        }
    }
    Ok(ResponseJson(json!({
        "items": items,
        "count": items.len(),
    })))
}

async fn get_operation_job(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<String>,
) -> ApiResult {
    let job = state
        .storage()
        .get_operation_job(&id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("任务不存在"))?;
    let items = state
        .storage()
        .list_operation_job_items(&id)
        .await
        .map_err(ResponseError::internal)?;
    let allowed_uids = scoped_account_uids(&state, &identity).await?;
    if !operation_items_accessible(&items, allowed_uids.as_ref()) {
        return Err(ResponseError::not_found("任务不存在"));
    }
    Ok(ResponseJson(json!({
        "job": job,
        "items": items,
    })))
}

async fn create_operation_job(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CreateOperationJobPayload>,
) -> ApiResult {
    if payload.items.is_empty() {
        return Err(ResponseError::bad_request("任务条目不能为空"));
    }
    let requested_uids = payload
        .items
        .iter()
        .map(|item| item.uid.trim().to_string())
        .collect::<Vec<_>>();
    let owner = owner_username_filter(&state, &identity);
    ensure_account_uids_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        &requested_uids,
    )
    .await?;

    let id = next_operation_job_id();
    state
        .storage()
        .create_operation_job(crate::storage::NewOperationJob {
            id: &id,
            kind: payload.kind.trim(),
            module: payload.module.trim(),
            action: payload.action.trim(),
            operator: payload.operator.trim(),
            trigger_source: payload.trigger_source.trim(),
            status: "queued",
            params: json!({
                "module": payload.module,
                "action": payload.action,
                "items": payload.items,
            }),
            summary: payload.summary.clone(),
            total_count: payload.items.len() as i64,
        })
        .await
        .map_err(ResponseError::internal)?;

    let refs = payload
        .items
        .iter()
        .enumerate()
        .map(|(idx, item)| crate::storage::NewOperationJobItem {
            job_id: &id,
            seq: idx as i64,
            uid: item.uid.trim(),
            account_nick: item.account_nick.trim(),
            target_type: item.target_type.trim(),
            target_id: item.target_id.trim(),
            target_title: item.target_title.trim(),
            status: "queued",
            params: item.params.clone(),
        })
        .collect::<Vec<_>>();
    state
        .storage()
        .add_operation_job_items(&refs)
        .await
        .map_err(ResponseError::internal)?;

    state
        .storage()
        .append_operation_log(crate::storage::NewOperationLog {
            job_id: &id,
            module: payload.module.trim(),
            action: payload.action.trim(),
            operator: payload.operator.trim(),
            trigger_source: payload.trigger_source.trim(),
            uid: "",
            account_nick: "",
            target_type: "job",
            target_id: &id,
            target_title: payload
                .summary
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(""),
            status: "queued",
            error_code: "",
            message: "后台任务已创建",
            params: json!({
                "kind": payload.kind,
                "items_count": payload.items.len(),
            }),
            before: json!({}),
            after: json!({
                "status": "queued",
            }),
            extra: operation_log_extra_with_correlation(&payload.summary, &identity),
        })
        .await
        .map_err(ResponseError::internal)?;

    let job = state
        .storage()
        .get_operation_job(&id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::internal_message("任务创建后未找到"))?;
    Ok(ResponseJson(json!({
        "job": job,
        "job_id": id,
    })))
}

async fn list_message_daily_stats(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<MessageStatsQuery>,
) -> ApiResult {
    let items = list_scoped_message_daily_stats(&state, &identity, &query).await?;
    Ok(ResponseJson(json!({
        "items": items,
        "count": items.len(),
    })))
}

async fn scoped_account_uids(
    state: &AppState,
    identity: &RequestIdentity,
) -> Result<Option<HashSet<String>>, ResponseError> {
    let Some(owner) = owner_username_filter(state, identity) else {
        return Ok(None);
    };
    accessible_account_uids(state.storage().accounts(), Some(&owner))
        .await
        .map(Some)
        .map_err(ResponseError::internal)
}

async fn list_scoped_operation_logs(
    state: &AppState,
    identity: &RequestIdentity,
    query: &OperationLogsQuery,
) -> Result<Vec<OperationLogEntry>, ResponseError> {
    let Some(allowed_uids) = scoped_account_uids(state, identity).await? else {
        let filter = OperationLogFilter {
            module: query.module.clone(),
            action: query.action.clone(),
            uid: query.uid.clone(),
            status: query.status.clone(),
            keyword: query.keyword.clone(),
            limit: query.limit,
            since: query.since,
        };
        return state
            .storage()
            .list_operation_logs(&filter)
            .await
            .map_err(ResponseError::internal);
    };

    let requested_uid = query.uid.trim();
    if !requested_uid.is_empty() && !allowed_uids.contains(requested_uid) {
        return Ok(Vec::new());
    }
    let scoped_uids = if requested_uid.is_empty() {
        allowed_uids.into_iter().collect::<Vec<_>>()
    } else {
        vec![requested_uid.to_string()]
    };
    let mut items = Vec::new();
    for uid in scoped_uids {
        let filter = OperationLogFilter {
            module: query.module.clone(),
            action: query.action.clone(),
            uid,
            status: query.status.clone(),
            keyword: query.keyword.clone(),
            limit: query.limit,
            since: query.since,
        };
        items.extend(
            state
                .storage()
                .list_operation_logs(&filter)
                .await
                .map_err(ResponseError::internal)?,
        );
    }
    items.sort_by(|left, right| {
        right
            .created_at
            .partial_cmp(&left.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    items.truncate(query.limit.max(1).min(500) as usize);
    Ok(items)
}

async fn list_scoped_message_daily_stats(
    state: &AppState,
    identity: &RequestIdentity,
    query: &MessageStatsQuery,
) -> Result<Vec<crate::models::operations::MessageDailyStat>, ResponseError> {
    let Some(allowed_uids) = scoped_account_uids(state, identity).await? else {
        return state
            .storage()
            .list_message_daily_stats(&MessageDailyStatFilter {
                uid: query.uid.clone(),
                start_date: query.start_date.clone(),
                end_date: query.end_date.clone(),
                limit: query.limit,
            })
            .await
            .map_err(ResponseError::internal);
    };

    let requested_uid = query.uid.trim();
    if !requested_uid.is_empty() && !allowed_uids.contains(requested_uid) {
        return Ok(Vec::new());
    }
    let scoped_uids = if requested_uid.is_empty() {
        allowed_uids.into_iter().collect::<Vec<_>>()
    } else {
        vec![requested_uid.to_string()]
    };
    let mut items = Vec::new();
    for uid in scoped_uids {
        items.extend(
            state
                .storage()
                .list_message_daily_stats(&MessageDailyStatFilter {
                    uid,
                    start_date: query.start_date.clone(),
                    end_date: query.end_date.clone(),
                    limit: query.limit,
                })
                .await
                .map_err(ResponseError::internal)?,
        );
    }
    items.sort_by(|left, right| {
        right
            .stat_date
            .cmp(&left.stat_date)
            .then_with(|| left.uid.cmp(&right.uid))
    });
    items.truncate(query.limit.max(1).min(365) as usize);
    Ok(items)
}

fn operation_items_accessible(
    items: &[OperationJobItem],
    allowed_uids: Option<&HashSet<String>>,
) -> bool {
    match allowed_uids {
        None => true,
        Some(allowed) => !items.is_empty() && items.iter().all(|item| allowed.contains(&item.uid)),
    }
}

pub(crate) async fn append_operation_log(
    state: &AppState,
    entry: crate::storage::NewOperationLog<'_>,
) {
    if let Err(error) = state.storage().append_operation_log(entry).await {
        tracing::warn!(error = %error, "append_operation_log failed");
    }
}

pub(crate) async fn bump_message_daily_stat(state: &AppState, uid: &str, field: &str, delta: i64) {
    let account_nick =
        sqlx::query_scalar::<_, String>("SELECT nick FROM accounts WHERE uid = ? LIMIT 1")
            .bind(uid)
            .fetch_optional(state.storage().accounts())
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
    let stat_date = sqlx::query("SELECT date('now', 'localtime') AS stat_date")
        .fetch_one(state.storage().accounts())
        .await
        .ok()
        .and_then(|row| row.try_get::<String, _>("stat_date").ok())
        .unwrap_or_else(|| "1970-01-01".to_string());
    if let Err(error) = state
        .storage()
        .bump_message_daily_stat(&stat_date, uid, &account_nick, field, delta)
        .await
    {
        tracing::warn!(uid, field, delta, error = %error, "bump_message_daily_stat failed");
    }
}
