use axum::{
    extract::{Extension, Json, Path, State},
    routing::get,
    Json as ResponseJson, Router,
};
use serde_json::json;
use tracing::{info, warn};

use crate::{
    automation_scheduler::{self, SaveAutomationSchedulePayload},
    models::auth::RequestIdentity,
    routes::{
        error::{ApiResult, ResponseError},
        tenant::owner_username_filter,
    },
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/automation/schedules"),
            get(list_schedules).post(save_schedule),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/automation/schedules/:id"),
            get(get_schedule).delete(delete_schedule),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/automation/schedules/:id/run"),
            axum::routing::post(run_schedule_once),
        )
}

fn owner_username(state: &AppState, identity: &RequestIdentity) -> String {
    owner_username_filter(state, identity).unwrap_or_else(|| identity.username.clone())
}

async fn list_schedules(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = owner_username(&state, &identity);
    let items = automation_scheduler::list_schedules(&state, &owner)
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(
        json!({ "items": items, "count": items.len() }),
    ))
}

async fn get_schedule(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<String>,
) -> ApiResult {
    let owner = owner_username(&state, &identity);
    let item = automation_scheduler::get_schedule(&state, &owner, &id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("自动化不存在"))?;
    Ok(ResponseJson(json!({ "item": item })))
}

async fn save_schedule(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<SaveAutomationSchedulePayload>,
) -> ApiResult {
    if payload.name.trim().is_empty() {
        warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "other-automation",
            action = "save_schedule_validation",
            outcome = "warn",
            username = %identity.username,
            error_code = "empty_schedule_name",
            "automation schedule save rejected: empty name"
        );
        return Err(ResponseError::bad_request("自动化名称不能为空"));
    }
    if payload.action.trim().is_empty() {
        warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "other-automation",
            action = "save_schedule_validation",
            outcome = "warn",
            username = %identity.username,
            error_code = "empty_schedule_action",
            "automation schedule save rejected: empty action"
        );
        return Err(ResponseError::bad_request("自动化动作不能为空"));
    }
    let owner = owner_username(&state, &identity);
    let requested_id = payload.id.trim().to_string();
    let action = payload.action.trim().to_string();
    let name = payload.name.trim().to_string();
    let enabled = payload.enabled;
    let account_scope = payload.account_scope.trim().to_string();
    let account_count = payload.account_uids.len();
    let item_scope = payload.item_scope.trim().to_string();
    let item_count = payload.item_ids.len();
    if !payload.id.trim().is_empty()
        && automation_scheduler::get_schedule(&state, &owner, payload.id.trim())
            .await
            .map_err(ResponseError::internal)?
            .is_none()
    {
        warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "other-automation",
            action = "save_schedule",
            outcome = "failed",
            username = %owner,
            job_id = %requested_id,
            error_code = "schedule_not_found",
            "automation schedule save rejected: schedule not found"
        );
        return Err(ResponseError::not_found("自动化不存在"));
    }
    let item = match automation_scheduler::save_schedule(&state, &owner, payload).await {
        Ok(item) => item,
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "other-automation",
                action = "save_schedule",
                outcome = "failed",
                username = %owner,
                job_id = %requested_id,
                error_code = "save_schedule_failed",
                error = %error,
                "automation schedule save failed"
            );
            return Err(ResponseError::internal(error));
        }
    };
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "other-automation",
        action = "save_schedule",
        outcome = "success",
        username = %owner,
        job_id = %item.id,
        schedule_action = %action,
        schedule_name = %name,
        enabled,
        account_scope = %account_scope,
        account_count,
        item_scope = %item_scope,
        item_count,
        "automation schedule saved"
    );
    Ok(ResponseJson(json!({ "item": item })))
}

async fn delete_schedule(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<String>,
) -> ApiResult {
    let owner = owner_username(&state, &identity);
    let deleted = match automation_scheduler::delete_schedule(&state, &owner, &id).await {
        Ok(deleted) => deleted,
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "other-automation",
                action = "delete_schedule",
                outcome = "failed",
                username = %owner,
                job_id = %id,
                error_code = "delete_schedule_failed",
                error = %error,
                "automation schedule delete failed"
            );
            return Err(ResponseError::internal(error));
        }
    };
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "other-automation",
        action = "delete_schedule",
        outcome = "success",
        username = %owner,
        job_id = %id,
        deleted,
        "automation schedule deleted"
    );
    Ok(ResponseJson(json!({ "deleted": deleted })))
}

async fn run_schedule_once(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<String>,
) -> ApiResult {
    let owner = owner_username(&state, &identity);
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "other-automation",
        action = "run_schedule_now",
        outcome = "started",
        username = %owner,
        job_id = %id,
        "automation schedule manual run started"
    );
    let message = match automation_scheduler::run_once_by_id(&state, &owner, &id).await {
        Ok(message) => message,
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "other-automation",
                action = "run_schedule_now",
                outcome = "failed",
                username = %owner,
                job_id = %id,
                error_code = "run_schedule_failed",
                error = %error,
                "automation schedule manual run failed"
            );
            return Err(ResponseError::internal_message(error));
        }
    };
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "other-automation",
        action = "run_schedule_now",
        outcome = "success",
        username = %owner,
        job_id = %id,
        "automation schedule manual run completed"
    );
    Ok(ResponseJson(json!({ "success": true, "message": message })))
}
