use axum::{
    extract::{Extension, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{models::auth::RequestIdentity, routes::error::ResponseError, state::AppState};

use super::{
    ensure_item_belongs_to_uid, ensure_market_uid_access, load_location_or_default,
    payload_ret_success, successful_upstream_h5_request, upstream_h5_request, UidQuery,
};

const LOCAL_EXPECTED_EXPOSURE_MISMATCH_RET: &str =
    "FAIL_LOCAL_EXPECTED_EXPOSURE_MISMATCH::expectedExposureNum 必须等于 exposureNum";
#[allow(non_snake_case)]
fn TASK_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.shop.incentive.task.list")
}
#[allow(non_snake_case)]
fn TASK_SYNC_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.shop.incentive.task.sync")
}
#[allow(non_snake_case)]
fn TASK_RECEIVE_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.shop.incentive.task.receive.reward")
}
#[allow(non_snake_case)]
fn EXPOSURE_HISTORY_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.shop.incentive.task.exposure.history")
}
#[allow(non_snake_case)]
fn EXPOSURE_DETAILS_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.shop.incentive.task.exposure.details")
}
#[allow(non_snake_case)]
fn EXPOSURE_HOME_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.exposure.coin.home")
}
#[allow(non_snake_case)]
fn EXPOSURE_AMOUNT_SELECT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.exposure.order.amount.select")
}
#[allow(non_snake_case)]
fn EXPOSURE_ORDER_CREATE_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.exposure.order.create")
}
#[allow(non_snake_case)]
fn EXPOSURE_ACTIVITY_RENDERING_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.item.activity.rendering")
}
#[allow(non_snake_case)]
fn WORKBENCH_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pro.workbench.info.get")
}
const V1: &str = "1.0";
const WORKBENCH_VERSION: &str = "3.0";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/home"),
            get(exposure_home),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/amount-options"),
            get(exposure_amount_options),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/create"),
            post(exposure_create),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/item-rendering"),
            get(exposure_item_rendering),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/tasks"),
            get(task_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/tasks/sync"),
            post(sync_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/tasks/run"),
            post(run_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/tasks/complete-view"),
            post(complete_safe_view_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/tasks/receive"),
            post(receive_reward),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/history"),
            get(exposure_history),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/exposure/details"),
            get(exposure_details),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/workbench"),
            get(workbench),
        )
}

pub(super) fn promotion_alias_routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/home"),
            get(exposure_home),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/amount-options"),
            get(exposure_amount_options),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/create"),
            post(exposure_create),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/item-rendering"),
            get(exposure_item_rendering),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/tasks"),
            get(task_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/tasks/sync"),
            post(sync_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/tasks/run"),
            post(run_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/tasks/complete-view"),
            post(complete_safe_view_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/tasks/receive"),
            post(receive_reward),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/history"),
            get(exposure_history),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/exposure/details"),
            get(exposure_details),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/workbench"),
            get(workbench),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExposureQuery {
    uid: String,
    scene: Option<String>,
    task_id: Option<String>,
    task_type: Option<String>,
    benefit_id: Option<String>,
    item_id: Option<String>,
    activity_id: Option<String>,
    task_instance_id: Option<String>,
    instance_id: Option<String>,
    biz_code: Option<String>,
    page: Option<u64>,
    page_no: Option<u64>,
    page_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmPayload {
    uid: String,
    #[serde(default)]
    confirm: bool,
    #[serde(flatten)]
    extra: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExposureCreatePayload {
    uid: String,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    item_ids: Vec<String>,
    #[serde(default)]
    item_id_list: Option<Value>,
    exposure_num: Value,
    #[serde(default)]
    expected_exposure_num: Option<Value>,
    #[serde(default)]
    pic_url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(flatten)]
    params: Map<String, Value>,
}

async fn exposure_home(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        EXPOSURE_HOME_API(),
        V1,
        json!({}),
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn exposure_amount_options(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        EXPOSURE_AMOUNT_SELECT_API(),
        V1,
        json!({}),
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn exposure_item_rendering(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ExposureQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let item_ids = query
        .item_id
        .as_deref()
        .map(split_item_ids)
        .unwrap_or_default();
    for item_id in &item_ids {
        ensure_item_belongs_to_uid(&state, &query.uid, item_id).await?;
    }
    let request_json = json!({
        "sceneTag": "MY_PUBLISH_ADD_EXPOSURE",
        "itemIdList": item_ids,
    });
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        EXPOSURE_ACTIVITY_RENDERING_API(),
        V1,
        json!({ "requestJson": request_json.to_string() }),
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn exposure_create(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ExposureCreatePayload>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let item_ids = exposure_payload_item_ids(&payload);
    for item_id in &item_ids {
        ensure_item_belongs_to_uid(&state, &payload.uid, item_id).await?;
    }
    if !payload.confirm {
        return Ok(Json(confirm_required()));
    }
    if let Some(expected) = payload.expected_exposure_num.as_ref() {
        if !same_json_scalar(expected, &payload.exposure_num) {
            return Ok(Json(local_failure(LOCAL_EXPECTED_EXPOSURE_MISMATCH_RET)));
        }
    }

    let mut data = payload.params;
    data.insert("itemIdList".to_string(), json!(item_ids.join(",")));
    data.insert("exposureNum".to_string(), payload.exposure_num);
    if let Some(pic_url) = payload.pic_url.filter(|value| !value.trim().is_empty()) {
        data.insert("picUrl".to_string(), json!(pic_url));
    }
    if let Some(title) = payload.title.filter(|value| !value.trim().is_empty()) {
        data.insert("title".to_string(), json!(title));
    }
    let upstream = upstream_h5_request(
        &state,
        &payload.uid,
        EXPOSURE_ORDER_CREATE_API(),
        V1,
        Value::Object(data),
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

pub(crate) async fn run_exposure_create_action(
    state: &AppState,
    uid: &str,
    item_ids: Vec<String>,
    exposure_num: Value,
    expected_exposure_num: Option<Value>,
    mut params: Map<String, Value>,
) -> Result<Value, String> {
    for item_id in &item_ids {
        ensure_item_belongs_to_uid(state, uid, item_id)
            .await
            .map_err(|error| error.to_string())?;
    }
    if let Some(expected) = expected_exposure_num.as_ref() {
        if !same_json_scalar(expected, &exposure_num) {
            return Ok(local_failure(LOCAL_EXPECTED_EXPOSURE_MISMATCH_RET));
        }
    }

    params.insert("itemIdList".to_string(), json!(item_ids.join(",")));
    params.insert("exposureNum".to_string(), exposure_num);
    upstream_h5_request(
        state,
        uid,
        EXPOSURE_ORDER_CREATE_API(),
        V1,
        Value::Object(params),
        vec![],
    )
    .await
    .map_err(|error| error.to_string())
}

async fn task_list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ExposureQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = query_data(&state, &query, true).await;
    let upstream =
        successful_upstream_h5_request(&state, &query.uid, TASK_LIST_API(), V1, data, vec![])
            .await
            .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn receive_reward(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ConfirmPayload>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(Json(confirm_required()));
    }
    let data = write_data_with_location(&state, &payload.uid, payload.extra).await;
    let upstream = upstream_h5_request(&state, &payload.uid, TASK_RECEIVE_API(), V1, data, vec![])
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn sync_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ConfirmPayload>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(Json(confirm_required()));
    }
    let data = write_data_with_location(&state, &payload.uid, payload.extra).await;
    let upstream = upstream_h5_request(&state, &payload.uid, TASK_SYNC_API(), V1, data, vec![])
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn run_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ConfirmPayload>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;

    if !payload.confirm {
        return Ok(Json(task_run_local_failure(
            "FAIL_LOCAL_CONFIRM_REQUIRED",
            "该操作会改变账号状态或扣除闲鱼币，需要确认",
        )));
    }

    let requested = task_ref_from_payload(&payload.extra);
    if !exposure_task_decision(&requested).protocol_executable {
        return Ok(Json(task_run_local_failure(
            "FAIL_LOCAL_MISSING_TASK_IDENTITY",
            "缺少可执行任务身份，拒绝执行",
        )));
    }

    let before = fetch_task_list(&state, &payload.uid).await?;
    let before_tasks = extract_tasks(&before);
    let sync_target =
        find_best_matching_task(&before_tasks, &requested).unwrap_or_else(|| requested.clone());
    let sync_data = if task_ref_has_identity(&sync_target) {
        let location = load_location_or_default(&state, &payload.uid).await;
        task_sync_payload(&sync_target, &location)
    } else {
        write_data_with_location(&state, &payload.uid, payload.extra.clone()).await
    };
    let sync = upstream_h5_request(&state, &payload.uid, TASK_SYNC_API(), V1, sync_data, vec![])
        .await
        .map_err(ResponseError::internal_message)?;

    let after_sync = fetch_task_list(&state, &payload.uid).await?;
    let after_sync_tasks = extract_tasks(&after_sync);
    let matched_task = find_best_matching_task(&after_sync_tasks, &sync_target);
    let receive = if let Some(task) = matched_task
        .as_ref()
        .filter(|task| task.can_receive_reward())
    {
        let location = load_location_or_default(&state, &payload.uid).await;
        upstream_h5_request(
            &state,
            &payload.uid,
            TASK_RECEIVE_API(),
            V1,
            task_receive_payload(task, &location),
            vec![],
        )
        .await
        .map_err(ResponseError::internal_message)?
    } else {
        json!({
            "ret": ["FAIL_LOCAL_TASK_NOT_RECEIVABLE::sync 后未定位到可领取任务"],
            "data": { "success": false }
        })
    };

    let final_list = fetch_task_list(&state, &payload.uid).await?;
    let success = payload_ret_success(&sync) && payload_ret_success(&receive);
    Ok(Json(json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "success": success,
            "protocolCriteria": protocol_executable_criteria(),
            "before": before.get("data").cloned().unwrap_or(Value::Null),
            "sync": sync,
            "afterSync": after_sync.get("data").cloned().unwrap_or(Value::Null),
            "matchedTask": matched_task.map(task_ref_to_value).unwrap_or(Value::Null),
            "receive": receive,
            "final": final_list.get("data").cloned().unwrap_or(Value::Null),
        }
    })))
}

async fn exposure_history(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ExposureQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = query_data(&state, &query, false).await;
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        EXPOSURE_HISTORY_API(),
        V1,
        data,
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn exposure_details(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ExposureQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    if query.instance_id.is_none()
        && query.biz_code.is_none()
        && query.item_id.is_none()
        && query.task_instance_id.is_none()
    {
        return Ok(Json(json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "detailList": [],
                "skipped": true,
                "reason": "missing instanceId/bizCode"
            }
        })));
    }
    let data = query_data(&state, &query, false).await;
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        EXPOSURE_DETAILS_API(),
        V1,
        data,
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn workbench(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        WORKBENCH_API(),
        WORKBENCH_VERSION,
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
        }),
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(Json(upstream))
}

async fn complete_safe_view_tasks(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ConfirmPayload>,
) -> Result<Json<Value>, ResponseError> {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(Json(confirm_required()));
    }

    let location = load_location_or_default(&state, &payload.uid).await;
    let initial = fetch_task_list(&state, &payload.uid).await?;
    let initial_tasks = extract_tasks(&initial);
    let task_capabilities: Vec<Value> = initial_tasks
        .iter()
        .map(exposure_task_capability_value)
        .collect();
    let skipped_tasks: Vec<Value> = initial_tasks
        .iter()
        .filter_map(skipped_exposure_task_value)
        .collect();
    let tasks_to_sync: Vec<TaskRef> = initial_tasks
        .into_iter()
        .filter(|task| exposure_task_decision(task).auto_safe && task.status == "ACCEPTED")
        .collect();

    let mut synced_tasks = Vec::new();
    for task in &tasks_to_sync {
        let upstream = upstream_h5_request(
            &state,
            &payload.uid,
            TASK_SYNC_API(),
            V1,
            task_sync_payload(task, &location),
            vec![],
        )
        .await
        .map_err(ResponseError::internal_message)?;
        synced_tasks.push(json!({
            "taskId": task.task_id,
            "taskInstanceId": task.task_instance_id,
            "taskType": task.task_type,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": payload_ret_success(&upstream),
        }));
    }

    let after_sync = fetch_task_list(&state, &payload.uid).await?;
    let synced_ids: Vec<String> = tasks_to_sync
        .iter()
        .map(|task| task.task_id.clone())
        .filter(|task_id| !task_id.is_empty())
        .collect();
    let benefits_to_receive: Vec<TaskRef> = extract_tasks(&after_sync)
        .into_iter()
        .filter(|task| exposure_task_decision(task).auto_safe && task.can_receive_reward())
        .collect();

    let mut received_rewards = Vec::new();
    for task in &benefits_to_receive {
        let upstream = upstream_h5_request(
            &state,
            &payload.uid,
            TASK_RECEIVE_API(),
            V1,
            task_receive_payload(task, &location),
            vec![],
        )
        .await
        .map_err(ResponseError::internal_message)?;
        received_rewards.push(json!({
            "taskId": task.task_id,
            "taskInstanceId": task.task_instance_id,
            "taskType": task.task_type,
            "benefitId": task.benefit_id,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": payload_ret_success(&upstream),
        }));
    }

    let final_list = fetch_task_list(&state, &payload.uid).await?;
    let success = synced_tasks
        .iter()
        .chain(received_rewards.iter())
        .all(|entry| {
            entry
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    Ok(Json(json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "success": success,
            "autoSafeCriteria": exposure_auto_safe_criteria(),
            "protocolCriteria": protocol_executable_criteria(),
            "taskCapabilities": task_capabilities,
            "skippedTasks": skipped_tasks,
            "syncedTaskIds": synced_ids,
            "syncedTasks": synced_tasks,
            "receivedRewards": received_rewards,
            "before": initial.get("data").cloned().unwrap_or(Value::Null),
            "afterSync": after_sync.get("data").cloned().unwrap_or(Value::Null),
            "final": final_list.get("data").cloned().unwrap_or(Value::Null),
        }
    })))
}

pub(crate) async fn run_exposure_complete_view_tasks_action(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let initial = fetch_task_list(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    let initial_tasks = extract_tasks(&initial);
    let task_capabilities: Vec<Value> = initial_tasks
        .iter()
        .map(exposure_task_capability_value)
        .collect();
    let skipped_tasks: Vec<Value> = initial_tasks
        .iter()
        .filter_map(skipped_exposure_task_value)
        .collect();
    let tasks_to_sync: Vec<TaskRef> = initial_tasks
        .into_iter()
        .filter(|task| exposure_task_decision(task).auto_safe && task.status == "ACCEPTED")
        .collect();

    let mut synced_tasks = Vec::new();
    for task in &tasks_to_sync {
        let upstream = upstream_h5_request(
            state,
            uid,
            TASK_SYNC_API(),
            V1,
            task_sync_payload(task, &location),
            vec![],
        )
        .await
        .map_err(|error| error.to_string())?;
        synced_tasks.push(json!({
            "taskId": task.task_id,
            "taskInstanceId": task.task_instance_id,
            "taskType": task.task_type,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": payload_ret_success(&upstream),
        }));
    }

    let after_sync = fetch_task_list(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    let benefits_to_receive: Vec<TaskRef> = extract_tasks(&after_sync)
        .into_iter()
        .filter(|task| exposure_task_decision(task).auto_safe && task.can_receive_reward())
        .collect();

    let mut received_rewards = Vec::new();
    for task in &benefits_to_receive {
        let upstream = upstream_h5_request(
            state,
            uid,
            TASK_RECEIVE_API(),
            V1,
            task_receive_payload(task, &location),
            vec![],
        )
        .await
        .map_err(|error| error.to_string())?;
        received_rewards.push(json!({
            "taskId": task.task_id,
            "taskInstanceId": task.task_instance_id,
            "taskType": task.task_type,
            "benefitId": task.benefit_id,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": payload_ret_success(&upstream),
        }));
    }

    let final_list = fetch_task_list(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    let success = synced_tasks
        .iter()
        .chain(received_rewards.iter())
        .all(|entry| {
            entry
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    Ok(json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "success": success,
            "taskCapabilities": task_capabilities,
            "skippedTasks": skipped_tasks,
            "syncedTasks": synced_tasks,
            "receivedRewards": received_rewards,
            "before": initial.get("data").cloned().unwrap_or(Value::Null),
            "afterSync": after_sync.get("data").cloned().unwrap_or(Value::Null),
            "final": final_list.get("data").cloned().unwrap_or(Value::Null),
        }
    }))
}

async fn fetch_task_list(state: &AppState, uid: &str) -> Result<Value, ResponseError> {
    let location = load_location_or_default(state, uid).await;
    successful_upstream_h5_request(
        state,
        uid,
        TASK_LIST_API(),
        V1,
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "scene": "ONLY_TASK_LIST",
        }),
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)
}

async fn query_data(state: &AppState, query: &ExposureQuery, default_scene: bool) -> Value {
    let location = load_location_or_default(state, &query.uid).await;
    let mut data = serde_json::Map::new();
    data.insert("fishAdCode".to_string(), json!(location.fish_ad_code));
    data.insert("gps".to_string(), json!(location.gps));
    if default_scene || query.scene.is_some() {
        data.insert(
            "scene".to_string(),
            json!(query
                .scene
                .clone()
                .unwrap_or_else(|| "ONLY_TASK_LIST".to_string())),
        );
    }
    insert_string(&mut data, "taskId", &query.task_id);
    insert_string(&mut data, "taskType", &query.task_type);
    insert_string(&mut data, "benefitId", &query.benefit_id);
    insert_string(&mut data, "itemId", &query.item_id);
    insert_string(&mut data, "activityId", &query.activity_id);
    insert_string(&mut data, "taskInstanceId", &query.task_instance_id);
    insert_string(&mut data, "instanceId", &query.instance_id);
    insert_string(&mut data, "bizCode", &query.biz_code);
    if let Some(page) = query.page.or(query.page_no) {
        data.insert("page".to_string(), json!(page));
        data.insert("pageNo".to_string(), json!(page));
    }
    if let Some(page_size) = query.page_size {
        data.insert("pageSize".to_string(), json!(page_size));
    } else {
        data.insert("pageSize".to_string(), json!(20));
    }
    Value::Object(data)
}

fn insert_string(map: &mut serde_json::Map<String, Value>, key: &str, value: &Option<String>) {
    if let Some(value) = value {
        map.insert(key.to_string(), json!(value));
    }
}

async fn write_data_with_location(state: &AppState, uid: &str, extra: Value) -> Value {
    let location = load_location_or_default(state, uid).await;
    let mut data = match extra {
        Value::Object(mut object) => {
            object.remove("confirm");
            object
        }
        _ => Map::new(),
    };
    data.entry("fishAdCode".to_string())
        .or_insert_with(|| json!(location.fish_ad_code));
    data.entry("gps".to_string())
        .or_insert_with(|| json!(location.gps));
    Value::Object(data)
}

fn confirm_required() -> Value {
    json!({
        "ret": ["FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认"],
        "data": { "success": false }
    })
}

fn local_failure(ret: &str) -> Value {
    json!({
        "ret": [ret],
        "data": { "success": false }
    })
}

fn exposure_payload_item_ids(payload: &ExposureCreatePayload) -> Vec<String> {
    if !payload.item_ids.is_empty() {
        return payload
            .item_ids
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
    }
    payload
        .item_id_list
        .as_ref()
        .map(item_ids_from_value)
        .unwrap_or_default()
}

fn item_ids_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .flat_map(|value| split_item_ids(&value))
            .collect(),
        Value::String(value) => split_item_ids(value),
        _ => Vec::new(),
    }
}

fn split_item_ids(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn same_json_scalar(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    match (left, right) {
        (Value::String(left), Value::Number(right)) => right
            .as_i64()
            .is_some_and(|right| left == &right.to_string()),
        (Value::Number(left), Value::String(right)) => {
            left.as_i64().is_some_and(|left| &left.to_string() == right)
        }
        _ => false,
    }
}

fn task_run_local_failure(code: &str, message: &str) -> Value {
    json!({
        "ret": [format!("{code}::{message}")],
        "data": {
            "success": false,
            "protocolCriteria": protocol_executable_criteria(),
            "before": Value::Null,
            "sync": Value::Null,
            "afterSync": Value::Null,
            "matchedTask": Value::Null,
            "receive": Value::Null,
            "final": Value::Null,
        }
    })
}

#[derive(Debug, Clone)]
struct TaskRef {
    task_id: String,
    task_instance_id: String,
    task_type: String,
    title: String,
    desc: String,
    status: String,
    benefit_id: String,
    activity_id: String,
    benefit_vo: Value,
}

impl TaskRef {
    fn can_receive_reward(&self) -> bool {
        matches!(self.status.as_str(), "AWARD" | "COMPLETE" | "COMPLETED")
    }
}

fn task_ref_from_payload(payload: &Value) -> TaskRef {
    let object = payload.as_object().cloned().unwrap_or_default();
    TaskRef {
        task_id: first_string(&object, &["taskId", "id", "taskInstanceId"]),
        task_instance_id: first_string(&object, &["taskInstanceId", "taskId", "id"]),
        task_type: first_string(&object, &["taskType"]),
        title: first_string(&object, &["title", "taskName", "name"])
            .or_else(|| first_string_from_path(&object, &["materialVO", "title"])),
        desc: first_string(&object, &["desc", "description", "taskDesc"])
            .or_else(|| first_string_from_path(&object, &["materialVO", "desc"])),
        status: first_string(
            &object,
            &["status", "taskStatus", "state", "idleTaskStatus"],
        ),
        benefit_id: first_string(&object, &["benefitId", "benefitInstanceId", "awardId"]),
        activity_id: first_string(&object, &["activityId"])
            .or_else(|| first_string_from_path(&object, &["benefitVO", "activityId"])),
        benefit_vo: object.get("benefitVO").cloned().unwrap_or(Value::Null),
    }
}

fn extract_tasks(payload: &Value) -> Vec<TaskRef> {
    let mut tasks = Vec::new();
    collect_tasks(payload, &mut tasks);
    tasks
}

fn collect_tasks(value: &Value, tasks: &mut Vec<TaskRef>) {
    match value {
        Value::Object(object) => {
            if let Some(task_type) = task_type_from_object(object) {
                tasks.push(TaskRef {
                    task_id: first_string(object, &["taskId", "id", "taskInstanceId"]),
                    task_instance_id: first_string(object, &["taskInstanceId", "taskId", "id"]),
                    task_type,
                    title: first_string(object, &["title", "taskName", "name"])
                        .or_else(|| first_string_from_path(object, &["materialVO", "title"])),
                    desc: first_string(object, &["desc", "description", "taskDesc"])
                        .or_else(|| first_string_from_path(object, &["materialVO", "desc"])),
                    status: first_string(
                        object,
                        &["status", "taskStatus", "state", "idleTaskStatus"],
                    ),
                    benefit_id: first_string(
                        object,
                        &["benefitId", "benefitInstanceId", "awardId"],
                    ),
                    activity_id: first_string(object, &["activityId"])
                        .or_else(|| first_string_from_path(object, &["benefitVO", "activityId"])),
                    benefit_vo: object.get("benefitVO").cloned().unwrap_or(Value::Null),
                });
            }
            for child in object.values() {
                collect_tasks(child, tasks);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_tasks(item, tasks);
            }
        }
        _ => {}
    }
}

fn first_string(object: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_to_string))
        .unwrap_or_default()
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn task_type_from_object(object: &Map<String, Value>) -> Option<String> {
    let looks_like_task = object.contains_key("taskId")
        || object.contains_key("taskInstanceId")
        || object.contains_key("benefitId")
        || object.contains_key("benefitVO")
        || object.contains_key("progressVO")
        || object.contains_key("taskStatus")
        || object.contains_key("status")
        || object.contains_key("idleTaskStatus");
    if !looks_like_task {
        return None;
    }
    object
        .get("taskType")
        .and_then(value_to_string)
        .or_else(|| {
            object
                .get("extend")
                .and_then(Value::as_object)
                .and_then(|extend| extend.get("taskType"))
                .and_then(value_to_string)
        })
}

trait EmptyStringExt {
    fn or_else(self, fallback: impl FnOnce() -> String) -> String;
}

impl EmptyStringExt for String {
    fn or_else(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}

fn first_string_from_path(object: &Map<String, Value>, path: &[&str]) -> String {
    let mut current = Value::Object(object.clone());
    for key in path {
        let Some(next) = current.get(*key).cloned() else {
            return String::new();
        };
        current = next;
    }
    value_to_string(&current).unwrap_or_default()
}

#[derive(Debug, Clone, Copy)]
struct ExposureTaskDecision {
    auto_safe: bool,
    protocol_executable: bool,
    reason: &'static str,
}

fn exposure_task_decision(task: &TaskRef) -> ExposureTaskDecision {
    let protocol_executable = task_has_sync_identity(task) || task_has_receive_identity(task);
    if !protocol_executable {
        return ExposureTaskDecision {
            auto_safe: false,
            protocol_executable: false,
            reason: "missing_required_payload",
        };
    }
    if has_manual_risk_exposure_task_semantics(task) {
        return ExposureTaskDecision {
            auto_safe: false,
            protocol_executable: true,
            reason: "manual_risk_task",
        };
    }
    if !has_safe_view_exposure_task_semantics(task) {
        return ExposureTaskDecision {
            auto_safe: false,
            protocol_executable: true,
            reason: "unsupported_task_semantics",
        };
    }
    ExposureTaskDecision {
        auto_safe: true,
        protocol_executable: true,
        reason: "safe_view_task",
    }
}

fn task_has_sync_identity(task: &TaskRef) -> bool {
    (!task.task_instance_id.is_empty() && !task.activity_id.is_empty())
        || (!task.task_id.is_empty() && !task.task_type.is_empty())
}

fn task_has_receive_identity(task: &TaskRef) -> bool {
    (!task.activity_id.is_empty() && !task.benefit_vo.is_null())
        || (!task.task_id.is_empty() && !task.benefit_id.is_empty())
}

fn has_safe_view_exposure_task_semantics(task: &TaskRef) -> bool {
    let text = exposure_task_search_text(task);
    contains_any(
        &text,
        &[
            "visit",
            "view",
            "browse",
            "diagnosis",
            "data",
            "xy_coin",
            "浏览",
            "查看",
            "访问",
            "诊断",
            "数据",
            "闲鱼币",
        ],
    )
}

fn has_manual_risk_exposure_task_semantics(task: &TaskRef) -> bool {
    let text = exposure_task_search_text(task);
    contains_any(
        &text,
        &[
            "publish",
            "pubilsh",
            "follow",
            "subscribe",
            "open",
            "order",
            "buy",
            "pay",
            "groupon",
            "发布",
            "关注",
            "订阅",
            "开通",
            "订单",
            "下单",
            "购买",
            "支付",
            "拼团",
        ],
    )
}

fn exposure_task_search_text(task: &TaskRef) -> String {
    format!("{} {} {}", task.task_type, task.title, task.desc).to_lowercase()
}

fn exposure_task_capability_value(task: &TaskRef) -> Value {
    let decision = exposure_task_decision(task);
    json!({
        "taskId": task.task_id,
        "taskInstanceId": task.task_instance_id,
        "taskType": task.task_type,
        "title": task.title,
        "status": task.status,
        "benefitId": task.benefit_id,
        "activityId": task.activity_id,
        "autoSafe": decision.auto_safe,
        "protocolExecutable": decision.protocol_executable,
        "reason": decision.reason,
    })
}

fn skipped_exposure_task_value(task: &TaskRef) -> Option<Value> {
    let decision = exposure_task_decision(task);
    if decision.auto_safe {
        return None;
    }
    Some(json!({
        "taskId": task.task_id,
        "taskInstanceId": task.task_instance_id,
        "taskType": task.task_type,
        "title": task.title,
        "status": task.status,
        "protocolExecutable": decision.protocol_executable,
        "reason": decision.reason,
    }))
}

fn exposure_auto_safe_criteria() -> Value {
    json!({
        "safeSemantics": ["visit", "view", "browse", "diagnosis", "data", "浏览", "查看", "访问", "诊断", "数据"],
        "manualRiskSemantics": ["publish", "follow", "subscribe", "open", "order", "buy", "pay", "发布", "关注", "开通", "订单", "购买", "支付"],
        "requiresTaskIdentity": true,
    })
}

fn protocol_executable_criteria() -> Value {
    json!({
        "requires": [
            "taskId + taskType",
            "taskInstanceId + activityId",
            "activityId + benefitVO",
            "taskId + benefitId",
        ],
        "manualConfirmationRequired": true,
    })
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn task_ref_has_identity(task: &TaskRef) -> bool {
    !task.task_id.is_empty()
        || !task.task_instance_id.is_empty()
        || !task.task_type.is_empty()
        || !task.activity_id.is_empty()
}

fn find_best_matching_task(tasks: &[TaskRef], requested: &TaskRef) -> Option<TaskRef> {
    tasks
        .iter()
        .filter(|task| task_matches(task, requested))
        .max_by_key(|task| {
            (
                task.can_receive_reward() as usize,
                task_match_score(task, requested),
            )
        })
        .cloned()
}

fn task_matches(task: &TaskRef, requested: &TaskRef) -> bool {
    if !requested.task_type.is_empty() && task.task_type != requested.task_type {
        return false;
    }
    if !requested.task_id.is_empty() && task.task_id != requested.task_id {
        return false;
    }
    if !requested.task_instance_id.is_empty() && task.task_instance_id != requested.task_instance_id
    {
        return false;
    }
    if !requested.benefit_id.is_empty() && task.benefit_id != requested.benefit_id {
        return false;
    }
    if !requested.activity_id.is_empty() && task.activity_id != requested.activity_id {
        return false;
    }
    true
}

fn task_match_score(task: &TaskRef, requested: &TaskRef) -> usize {
    let mut score = 0;
    if !requested.task_type.is_empty() && task.task_type == requested.task_type {
        score += 1;
    }
    if !requested.task_id.is_empty() && task.task_id == requested.task_id {
        score += 2;
    }
    if !requested.task_instance_id.is_empty() && task.task_instance_id == requested.task_instance_id
    {
        score += 3;
    }
    if !requested.benefit_id.is_empty() && task.benefit_id == requested.benefit_id {
        score += 2;
    }
    if !requested.activity_id.is_empty() && task.activity_id == requested.activity_id {
        score += 2;
    }
    score
}

fn task_ref_to_value(task: TaskRef) -> Value {
    json!({
        "taskId": task.task_id,
        "taskInstanceId": task.task_instance_id,
        "taskType": task.task_type,
        "title": task.title,
        "desc": task.desc,
        "status": task.status,
        "benefitId": task.benefit_id,
        "activityId": task.activity_id,
        "benefitVO": task.benefit_vo,
    })
}

fn task_sync_payload(task: &TaskRef, location: &crate::xianyu_client::LocationContext) -> Value {
    if !task.task_instance_id.is_empty() && !task.activity_id.is_empty() {
        return json!({
            "activityId": task.activity_id,
            "complete": true,
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "taskInstanceId": task.task_instance_id,
        });
    }
    legacy_task_payload(task, location)
}

fn task_receive_payload(task: &TaskRef, location: &crate::xianyu_client::LocationContext) -> Value {
    if !task.activity_id.is_empty() && !task.benefit_vo.is_null() {
        return json!({
            "activityId": task.activity_id,
            "benefitList": [task.benefit_vo],
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
        });
    }
    legacy_task_payload(task, location)
}

fn legacy_task_payload(task: &TaskRef, location: &crate::xianyu_client::LocationContext) -> Value {
    let mut data = serde_json::Map::new();
    data.insert("fishAdCode".to_string(), json!(location.fish_ad_code));
    data.insert("gps".to_string(), json!(location.gps));
    if !task.task_id.is_empty() {
        data.insert("taskId".to_string(), json!(task.task_id));
    }
    if !task.task_type.is_empty() {
        data.insert("taskType".to_string(), json!(task.task_type));
    }
    if !task.benefit_id.is_empty() {
        data.insert("benefitId".to_string(), json!(task.benefit_id));
    }
    Value::Object(data)
}
