use std::collections::HashSet;

use axum::{
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    routes::{
        error::ResponseError,
        tenant::{accessible_account_uids, has_unscoped_legacy_access},
    },
    state::AppState,
};

mod crud;
mod delete_jobs;
mod lifecycle;
mod queries;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/tasks"),
            post(crud::create_task).get(crud::list_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/delete-jobs"),
            get(delete_jobs::list_comment_delete_jobs),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/delete-jobs/:job_id/retry"),
            post(delete_jobs::retry_comment_delete_job),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/delete-jobs/:job_id/cancel"),
            post(delete_jobs::cancel_comment_delete_job),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/:task_id"),
            get(crud::get_task)
                .patch(crud::update_task)
                .delete(crud::delete_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/:task_id/progress"),
            get(lifecycle::get_task_progress),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/:task_id/logs"),
            get(crud::list_task_logs),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/:task_id/attempts"),
            get(crud::list_task_attempts),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/:task_id/start"),
            post(lifecycle::start_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/tasks/:task_id/pause"),
            post(lifecycle::pause_task),
        )
}

pub(super) async fn accessible_task_account_uids(
    state: &AppState,
    identity: &RequestIdentity,
) -> Result<Option<HashSet<String>>, ResponseError> {
    if has_unscoped_legacy_access(identity) {
        return Ok(None);
    }
    accessible_account_uids(state.storage().accounts(), Some(identity.username.trim()))
        .await
        .map(Some)
        .map_err(ResponseError::internal)
}

pub(super) async fn ensure_task_access(
    state: &AppState,
    identity: &RequestIdentity,
    task_id: &str,
) -> Result<(), ResponseError> {
    if has_unscoped_legacy_access(identity) {
        return queries::ensure_task_exists(state.storage().xianyu(), task_id).await;
    }

    let allowed_uids =
        accessible_account_uids(state.storage().accounts(), Some(identity.username.trim()))
            .await
            .map_err(ResponseError::internal)?;
    let task_uids = queries::fetch_task_account_uids(state.storage().xianyu(), task_id)
        .await
        .map_err(ResponseError::internal)?;
    if !task_uids.is_empty() && task_uids.iter().all(|uid| allowed_uids.contains(uid)) {
        Ok(())
    } else {
        Err(ResponseError::not_found("任务不存在"))
    }
}

pub(super) fn task_uids_accessible(
    task_uids: &[String],
    allowed_uids: Option<&HashSet<String>>,
) -> bool {
    match allowed_uids {
        None => true,
        Some(allowed) => !task_uids.is_empty() && task_uids.iter().all(|uid| allowed.contains(uid)),
    }
}

// ── Shared payload types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct TaskCreatePayload {
    #[serde(default)]
    pub(super) account_uids: Vec<String>,
    #[serde(default)]
    pub(super) target_item_ids: Vec<String>,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) comment_templates: Vec<String>,
    #[serde(default)]
    pub(super) publish_interval_min_seconds: f64,
    #[serde(default)]
    pub(super) publish_interval_max_seconds: f64,
    #[serde(default)]
    pub(super) delete_interval_min_seconds: f64,
    #[serde(default)]
    pub(super) delete_interval_max_seconds: f64,
    #[serde(default = "default_delete_after_seconds")]
    pub(super) delete_after_seconds: f64,
    #[serde(default = "default_enable_delete_comments")]
    pub(super) enable_delete_comments: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct TaskUpdatePayload {
    pub(super) title: Option<String>,
    pub(super) comment_templates: Option<Vec<String>>,
    pub(super) account_uids: Option<Vec<String>>,
    pub(super) target_item_ids: Option<Vec<String>>,
    pub(super) publish_interval_min_seconds: Option<f64>,
    pub(super) publish_interval_max_seconds: Option<f64>,
    pub(super) delete_interval_min_seconds: Option<f64>,
    pub(super) delete_interval_max_seconds: Option<f64>,
    pub(super) delete_after_seconds: Option<f64>,
    pub(super) enable_delete_comments: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskListQuery {
    #[serde(default)]
    pub(super) status: String,
    #[serde(default = "default_page")]
    pub(super) page: i64,
    #[serde(default = "default_page_size_20")]
    pub(super) page_size: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskLogsQuery {
    #[serde(default = "default_page_size_50")]
    pub(super) limit: i64,
    #[serde(default)]
    pub(super) offset: i64,
    #[serde(default)]
    pub(super) level: String,
    #[serde(default)]
    pub(super) account_uid: String,
    #[serde(default)]
    pub(super) item_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TaskAttemptsQuery {
    #[serde(default = "default_page_size_20")]
    pub(super) limit: i64,
    #[serde(default)]
    pub(super) offset: i64,
    #[serde(default)]
    pub(super) account_uid: String,
    #[serde(default)]
    pub(super) item_id: String,
    #[serde(default)]
    pub(super) success: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct DeleteJobsQuery {
    #[serde(default)]
    pub(super) task_id: String,
    #[serde(default)]
    pub(super) status: String,
    #[serde(default)]
    pub(super) account_uid: String,
    #[serde(default)]
    pub(super) item_id: String,
    #[serde(default = "default_page")]
    pub(super) page: i64,
    #[serde(default = "default_page_size_20")]
    pub(super) page_size: i64,
}

// ── Shared data type ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub(super) struct CommentTargetRecord {
    pub(super) item_id: String,
    pub(super) title: String,
    pub(super) price: String,
    pub(super) seller_id: String,
    pub(super) seller_nick: String,
}

// ── Row converters ────────────────────────────────────────────────────────────

pub(super) fn task_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "task_id": row.get::<String, _>("task_id"),
        "title": row.get::<String, _>("title"),
        "status": row.get::<String, _>("status"),
        "source": row.get::<String, _>("source"),
        "target_total": row.get::<i64, _>("target_total"),
        "account_total": row.get::<i64, _>("account_total"),
        "account_uids": [],
        "comment_templates": json_array_from_text(&row.get::<String, _>("comment_templates_json")),
        "publish_interval_min_seconds": row.get::<f64, _>("publish_interval_min_seconds"),
        "publish_interval_max_seconds": row.get::<f64, _>("publish_interval_max_seconds"),
        "delete_interval_min_seconds": row.get::<f64, _>("delete_interval_min_seconds"),
        "delete_interval_max_seconds": row.get::<f64, _>("delete_interval_max_seconds"),
        "delete_after_seconds": row.get::<f64, _>("delete_after_seconds"),
        "enable_delete_comments": row.get::<i64, _>("enable_delete_comments") != 0,
        "created_at": row.get::<f64, _>("created_at"),
        "updated_at": row.get::<f64, _>("updated_at"),
        "started_at": row.get::<f64, _>("started_at"),
        "paused_at": row.get::<f64, _>("paused_at"),
        "finished_at": row.get::<f64, _>("finished_at"),
        "last_error": row.get::<String, _>("last_error"),
        "last_transition_at": row.get::<f64, _>("last_transition_at"),
        "last_log_at": row.get::<f64, _>("last_log_at"),
        "progress": Value::Null,
        "targets": [],
    })
}

pub(super) fn task_log_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<i64, _>("id"),
        "task_id": row.get::<String, _>("task_id"),
        "level": row.get::<String, _>("level"),
        "event": row.get::<String, _>("event"),
        "message": row.get::<String, _>("message"),
        "context": json_object_from_text(&row.get::<String, _>("context_json")),
        "created_at": row.get::<f64, _>("created_at"),
    })
}

pub(super) fn task_attempt_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<i64, _>("id"),
        "task_id": row.get::<String, _>("task_id"),
        "account_uid": row.get::<String, _>("account_uid"),
        "item_id": row.get::<String, _>("item_id"),
        "target_status": row.get::<String, _>("target_status"),
        "success": row.get::<i64, _>("success") != 0,
        "comment_content": row.get::<String, _>("comment_content"),
        "comment_content_hash": row.get::<String, _>("comment_content_hash"),
        "source_comment": json_object_from_text(&row.get::<String, _>("source_comment_json")),
        "published_comment_id": row.get::<String, _>("published_comment_id"),
        "published_at": row.get::<f64, _>("published_at"),
        "failure_reason_code": row.get::<String, _>("failure_reason_code"),
        "failure_reason_message": row.get::<String, _>("failure_reason_message"),
        "failure_scope": row.get::<String, _>("failure_scope"),
        "request_payload": json_object_from_text(&row.get::<String, _>("request_payload_json")),
        "response_payload": json_object_from_text(&row.get::<String, _>("response_payload_json")),
        "delete_job_payload": json_object_from_text(&row.get::<String, _>("delete_job_payload_json")),
        "created_at": row.get::<f64, _>("created_at"),
        "updated_at": row.get::<f64, _>("updated_at"),
    })
}

pub(super) fn delete_job_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    json!({
        "id": row.get::<i64, _>("id"),
        "task_id": row.get::<String, _>("task_id"),
        "attempt_id": row.get::<i64, _>("attempt_id"),
        "account_uid": row.get::<String, _>("account_uid"),
        "item_id": row.get::<String, _>("item_id"),
        "comment_id": row.get::<String, _>("comment_id"),
        "status": row.get::<String, _>("status"),
        "payload": json_object_from_text(&row.get::<String, _>("payload_json")),
        "scheduled_at": row.get::<f64, _>("scheduled_at"),
        "retry_count": row.get::<i64, _>("retry_count"),
        "started_at": row.get::<f64, _>("started_at"),
        "finished_at": row.get::<f64, _>("finished_at"),
        "cancelled_at": row.get::<f64, _>("cancelled_at"),
        "last_error_at": row.get::<f64, _>("last_error_at"),
        "last_error_code": row.get::<String, _>("last_error_code"),
        "last_error_message": row.get::<String, _>("last_error_message"),
        "claimed_by": row.get::<String, _>("claimed_by"),
        "claim_token": row.get::<String, _>("claim_token"),
        "created_at": row.get::<f64, _>("created_at"),
        "updated_at": row.get::<f64, _>("updated_at"),
    })
}

// ── Validation ────────────────────────────────────────────────────────────────

pub(super) fn validate_create_payload(payload: &TaskCreatePayload) -> Result<(), ResponseError> {
    if payload.account_uids.is_empty() {
        return Err(ResponseError::bad_request("account_uids 不能为空"));
    }
    if payload.target_item_ids.is_empty() {
        return Err(ResponseError::bad_request("target_item_ids 不能为空"));
    }
    Ok(())
}

// ── String/JSON helpers ───────────────────────────────────────────────────────

pub(super) fn normalize_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

pub(super) fn string_list_from_value(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn json_array_from_text(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Array(Vec::new()))
}

pub(super) fn json_object_from_text(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Object(Map::new()))
}

pub(super) fn json_string(values: &[String]) -> String {
    serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string())
}

pub(super) fn hash_text(text: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn bool_to_int(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub(super) fn normalize_success_filter(raw: &str) -> Option<bool> {
    match raw.trim().to_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

// ── Default value functions ───────────────────────────────────────────────────

pub(super) fn default_delete_after_seconds() -> f64 {
    3600.0
}

pub(super) fn default_enable_delete_comments() -> bool {
    true
}

pub(super) fn default_page() -> i64 {
    1
}

pub(super) fn default_page_size_20() -> i64 {
    20
}

pub(super) fn default_page_size_50() -> i64 {
    50
}

// ── Time helpers ──────────────────────────────────────────────────────────────

pub(super) fn unix_seconds_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs_f64()
}

pub(super) fn unix_millis_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}
