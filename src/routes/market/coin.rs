use axum::{
    extract::{Extension, Json, Query, State},
    routing::{get, post},
    Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::{
    ensure_item_belongs_to_uid, ensure_market_uid_access, load_location_or_default,
    successful_upstream_h5_request, upstream_h5_request,
};

const LOCAL_CONFIRM_REQUIRED_RET: &str =
    "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认";
const LOCAL_EXPECTED_COST_MISMATCH_RET: &str =
    "FAIL_LOCAL_EXPECTED_COST_MISMATCH::expectedCostCoinNum 必须等于 costCoinNum";
const LOCAL_PROMOTION_PACKAGE_NOT_FOUND_RET: &str =
    "FAIL_LOCAL_PROMOTION_PACKAGE_NOT_FOUND::未从 promotion.view 查询到对应推广套餐";
const LOCAL_PROMOTION_PACKAGE_COST_MISMATCH_RET: &str =
    "FAIL_LOCAL_PROMOTION_PACKAGE_COST_MISMATCH::创建消耗与 promotion.view 套餐价格不一致";
#[allow(non_snake_case)]
fn COIN_ACTIVITY_USER_INFO_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.activity.user.info")
}
#[allow(non_snake_case)]
fn COIN_USER_DETAIL_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.user.detail.list")
}
#[allow(non_snake_case)]
fn COIN_VIRTUAL_CARD_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.virtual.card.list")
}
#[allow(non_snake_case)]
fn COIN_TASK_DETAIL_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.task.getolivertaskdetail")
}
#[allow(non_snake_case)]
fn COIN_WLJ_TASK_DETAIL_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.wlj.task.detail")
}
#[allow(non_snake_case)]
fn COIN_SUBSCRIBE_GET_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.subscribe.get")
}
#[allow(non_snake_case)]
fn COIN_SIGN_CHALLENGE_INFO_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.idle.challenge.idleCoinSign.info")
}
#[allow(non_snake_case)]
fn COIN_TASK_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.task.queryolivertasklist")
}
#[allow(non_snake_case)]
fn COIN_TASK_BENEFIT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.task.getolivertaskbenefit")
}
#[allow(non_snake_case)]
fn COIN_TASK_AD_EVENT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.play.ad.event.push")
}
const COIN_TASK_VERSION: &str = "1.0";
const COIN_TASK_LIST_VERSION: &str = "2.0";
const COIN_SUBSCRIBE_VERSION: &str = "2.0";
const COIN_VIRTUAL_CARD_VERSION: &str = "2.0";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/coin/signin/home"),
            get(signin_home),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/signin/challenge-info"),
            get(signin_challenge_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/signin"),
            post(signin),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/activity/user-info"),
            get(activity_user_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/user-detail-list"),
            get(user_detail_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/virtual-card-list"),
            get(virtual_card_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/tasks"),
            get(coin_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/task/detail"),
            get(task_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/task/wlj-detail"),
            get(wlj_task_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/tasks/complete-view"),
            post(complete_coin_view_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/tasks/claim"),
            post(claim_coin_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/subscribe"),
            get(subscribe_get),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/promotion/view"),
            get(promotion_view),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/promotion/page"),
            get(promotion_page),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/promotion/detail"),
            get(promotion_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/promotion/create"),
            post(promotion_create),
        )
}

pub(super) fn promotion_alias_routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/coin/signin/home"),
            get(signin_home),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/signin/challenge-info"),
            get(signin_challenge_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/signin"),
            post(signin),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/activity/user-info"),
            get(activity_user_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/user-detail-list"),
            get(user_detail_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/virtual-card-list"),
            get(virtual_card_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/tasks"),
            get(coin_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/task/detail"),
            get(task_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/task/wlj-detail"),
            get(wlj_task_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/tasks/complete-view"),
            post(complete_coin_view_tasks),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/tasks/claim"),
            post(claim_coin_task),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/subscribe"),
            get(subscribe_get),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/packages"),
            get(promotion_view),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/orders"),
            get(promotion_page),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/detail"),
            get(promotion_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/create"),
            post(promotion_create),
        )
}

#[derive(Debug, Deserialize)]
struct CoinQuery {
    uid: String,
    #[serde(flatten)]
    params: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct CoinWritePayload {
    uid: String,
    #[serde(default)]
    confirm: bool,
    #[serde(flatten)]
    params: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PromotionCreatePayload {
    uid: String,
    item_id: String,
    cost_coin_num: Value,
    expected_cost_coin_num: Value,
    #[serde(default)]
    confirm: bool,
    #[serde(flatten)]
    params: Map<String, Value>,
}

async fn signin_home(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        data.entry("playId".to_string()).or_insert_with(|| json!(4));
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.play.sign.in.home"),
        "1.0",
        data,
    )
    .await
}

async fn signin_challenge_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |_| {}).await;
    read_h5(
        &state,
        &query.uid,
        COIN_SIGN_CHALLENGE_INFO_API(),
        COIN_TASK_VERSION,
        data,
    )
    .await
}

async fn activity_user_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |_| {}).await;
    read_h5(
        &state,
        &query.uid,
        COIN_ACTIVITY_USER_INFO_API(),
        COIN_TASK_VERSION,
        data,
    )
    .await
}

async fn user_detail_list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        normalize_page_params(data);
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        COIN_USER_DETAIL_LIST_API(),
        COIN_TASK_VERSION,
        data,
    )
    .await
}

async fn virtual_card_list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        data.entry("cardType".to_string())
            .or_insert_with(|| json!(7));
        normalize_numeric_page_params(data, 1, 10);
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        COIN_VIRTUAL_CARD_LIST_API(),
        COIN_VIRTUAL_CARD_VERSION,
        data,
    )
    .await
}

async fn signin(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CoinWritePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(ResponseJson(local_failure(LOCAL_CONFIRM_REQUIRED_RET)));
    }
    let data = coin_data_with_location(&state, &payload.uid, payload.params, |data| {
        data.entry("playId".to_string()).or_insert_with(|| json!(4));
        data.entry("asac".to_string())
            .or_insert_with(|| Value::String(String::new()));
        data.entry("usertoken".to_string())
            .or_insert_with(|| Value::String(String::new()));
    })
    .await;
    write_h5(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.play.sign.in"),
        "1.0",
        data,
    )
    .await
}

pub(crate) async fn run_coin_signin_action(
    state: &AppState,
    uid: &str,
    params: Map<String, Value>,
) -> Result<Value, String> {
    let data = coin_data_with_location(state, uid, params, |data| {
        data.entry("playId".to_string()).or_insert_with(|| json!(4));
        data.entry("asac".to_string())
            .or_insert_with(|| Value::String(String::new()));
        data.entry("usertoken".to_string())
            .or_insert_with(|| Value::String(String::new()));
    })
    .await;
    upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.play.sign.in"),
        "1.0",
        data,
        Vec::new(),
    )
    .await
    .map_err(|error| error.to_string())
}

async fn coin_tasks(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        data.entry("brand".to_string())
            .or_insert_with(|| Value::String("redmi".to_string()));
        data.entry("isWidgetInstalled".to_string())
            .or_insert_with(|| Value::Bool(false));
        data.entry("osVersion".to_string())
            .or_insert_with(|| Value::String("13".to_string()));
        data.entry("platform".to_string())
            .or_insert_with(|| Value::String("Android".to_string()));
        data.entry("sceneId".to_string())
            .or_insert_with(|| json!(496));
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        COIN_TASK_LIST_API(),
        COIN_TASK_LIST_VERSION,
        data,
    )
    .await
}

async fn task_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        data.entry("sceneId".to_string())
            .or_insert_with(|| json!(496));
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        COIN_TASK_DETAIL_API(),
        COIN_TASK_VERSION,
        data,
    )
    .await
}

async fn wlj_task_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let mut data = query.params;
    data.entry("sceneId".to_string())
        .or_insert_with(|| json!(496));
    let data = Value::Object(data);
    read_h5(
        &state,
        &query.uid,
        COIN_WLJ_TASK_DETAIL_API(),
        COIN_TASK_VERSION,
        data,
    )
    .await
}

async fn complete_coin_view_tasks(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CoinWritePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(ResponseJson(local_failure(LOCAL_CONFIRM_REQUIRED_RET)));
    }

    let initial = fetch_coin_task_list(&state, &payload.uid).await?;
    let initial_tasks = extract_coin_tasks(&initial);
    let task_capabilities: Vec<Value> = initial_tasks
        .iter()
        .map(coin_task_capability_value)
        .collect();
    let skipped_tasks: Vec<Value> = initial_tasks
        .iter()
        .filter_map(skipped_coin_task_value)
        .collect();
    let tasks_to_trigger: Vec<CoinTaskRef> = initial_tasks
        .into_iter()
        .filter(is_triggerable_safe_coin_view_task)
        .collect();

    let mut triggered_tasks = Vec::new();
    for task in &tasks_to_trigger {
        let Some(trigger_payload) = coin_task_ad_event_payload(task) else {
            triggered_tasks.push(json!({
                "deliveryId": task.delivery_id,
                "title": task.title,
                "actionType": task.action_type,
                "ret": ["FAIL_LOCAL_UNSUPPORTED_COIN_VIEW_TASK::未识别到广告浏览参数"],
                "success": false,
            }));
            continue;
        };
        let upstream = upstream_h5_request(
            &state,
            &payload.uid,
            COIN_TASK_AD_EVENT_API(),
            COIN_TASK_VERSION,
            trigger_payload,
            vec![],
        )
        .await
        .map_err(ResponseError::internal_message)?;
        triggered_tasks.push(json!({
            "deliveryId": task.delivery_id,
            "title": task.title,
            "actionType": task.action_type,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": ret_success(&upstream),
        }));
    }

    let after_trigger = fetch_coin_task_list(&state, &payload.uid).await?;
    let mut claimed_rewards = Vec::new();
    let location = load_location_or_default(&state, &payload.uid).await;
    for task in extract_coin_tasks(&after_trigger)
        .into_iter()
        .filter(is_claimable_safe_coin_view_task)
    {
        let upstream = upstream_h5_request(
            &state,
            &payload.uid,
            COIN_TASK_BENEFIT_API(),
            COIN_TASK_VERSION,
            coin_task_claim_payload(&task, &location),
            vec![],
        )
        .await
        .map_err(ResponseError::internal_message)?;
        claimed_rewards.push(json!({
            "deliveryId": task.delivery_id,
            "title": task.title,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": ret_success(&upstream),
        }));
    }

    let final_list = fetch_coin_task_list(&state, &payload.uid).await?;
    Ok(ResponseJson(json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "success": true,
            "autoSafeCriteria": {
                "actionType": "VISIT",
                "needTimes": 1,
                "requiresAdParam": ["adPositionCode", "adCode"],
                "safePages": ["alimama-ads", "/xianyu/search.html", "/xianyu/like.html"],
                "riskySemanticsDenied": true,
            },
            "taskCapabilities": task_capabilities,
            "skippedTasks": skipped_tasks,
            "triggeredTasks": triggered_tasks,
            "claimedRewards": claimed_rewards,
            "before": initial.get("data").cloned().unwrap_or(Value::Null),
            "afterTrigger": after_trigger.get("data").cloned().unwrap_or(Value::Null),
            "final": final_list.get("data").cloned().unwrap_or(Value::Null),
        }
    })))
}

pub(crate) async fn run_coin_complete_view_tasks_action(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let initial = fetch_coin_task_list(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    let initial_tasks = extract_coin_tasks(&initial);
    let task_capabilities: Vec<Value> = initial_tasks
        .iter()
        .map(coin_task_capability_value)
        .collect();
    let skipped_tasks: Vec<Value> = initial_tasks
        .iter()
        .filter_map(skipped_coin_task_value)
        .collect();
    let tasks_to_trigger: Vec<CoinTaskRef> = initial_tasks
        .into_iter()
        .filter(is_triggerable_safe_coin_view_task)
        .collect();

    let mut triggered_tasks = Vec::new();
    for task in &tasks_to_trigger {
        let Some(trigger_payload) = coin_task_ad_event_payload(task) else {
            triggered_tasks.push(json!({
                "deliveryId": task.delivery_id,
                "title": task.title,
                "actionType": task.action_type,
                "ret": ["FAIL_LOCAL_UNSUPPORTED_COIN_VIEW_TASK::未识别到广告浏览参数"],
                "success": false,
            }));
            continue;
        };
        let upstream = upstream_h5_request(
            state,
            uid,
            COIN_TASK_AD_EVENT_API(),
            COIN_TASK_VERSION,
            trigger_payload,
            vec![],
        )
        .await
        .map_err(|error| error.to_string())?;
        triggered_tasks.push(json!({
            "deliveryId": task.delivery_id,
            "title": task.title,
            "actionType": task.action_type,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": ret_success(&upstream),
        }));
    }

    let after_trigger = fetch_coin_task_list(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    let mut claimed_rewards = Vec::new();
    let location = load_location_or_default(state, uid).await;
    for task in extract_coin_tasks(&after_trigger)
        .into_iter()
        .filter(is_claimable_safe_coin_view_task)
    {
        let upstream = upstream_h5_request(
            state,
            uid,
            COIN_TASK_BENEFIT_API(),
            COIN_TASK_VERSION,
            coin_task_claim_payload(&task, &location),
            vec![],
        )
        .await
        .map_err(|error| error.to_string())?;
        claimed_rewards.push(json!({
            "deliveryId": task.delivery_id,
            "title": task.title,
            "ret": upstream.get("ret").cloned().unwrap_or(Value::Null),
            "success": ret_success(&upstream),
        }));
    }

    let final_list = fetch_coin_task_list(state, uid)
        .await
        .map_err(|error| error.to_string())?;
    Ok(json!({
        "ret": ["SUCCESS::调用成功"],
        "data": {
            "success": true,
            "taskCapabilities": task_capabilities,
            "skippedTasks": skipped_tasks,
            "triggeredTasks": triggered_tasks,
            "claimedRewards": claimed_rewards,
            "before": initial.get("data").cloned().unwrap_or(Value::Null),
            "afterTrigger": after_trigger.get("data").cloned().unwrap_or(Value::Null),
            "final": final_list.get("data").cloned().unwrap_or(Value::Null),
        }
    }))
}

async fn claim_coin_task(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CoinWritePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(ResponseJson(local_failure(LOCAL_CONFIRM_REQUIRED_RET)));
    }
    let data = coin_data_with_location(&state, &payload.uid, payload.params, |data| {
        data.entry("sceneId".to_string())
            .or_insert_with(|| json!(496));
        data.entry("asac".to_string())
            .or_insert_with(|| Value::String(String::new()));
    })
    .await;
    write_h5(
        &state,
        &payload.uid,
        COIN_TASK_BENEFIT_API(),
        COIN_TASK_VERSION,
        data,
    )
    .await
}

async fn subscribe_get(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        data.entry("biz".to_string())
            .or_insert_with(|| Value::String("xianyu_coin".to_string()));
        data.entry("topic".to_string())
            .or_insert_with(|| Value::String("sign_in".to_string()));
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        COIN_SUBSCRIBE_GET_API(),
        COIN_SUBSCRIBE_VERSION,
        data,
    )
    .await
}

async fn promotion_view(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        data.entry("cardId".to_string())
            .or_insert_with(|| Value::String(String::new()));
        data.entry("needList".to_string())
            .or_insert_with(|| Value::Bool(true));
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.promotion.view"),
        "2.0",
        data,
    )
    .await
}

async fn promotion_page(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        normalize_page_params(data);
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.promotion.page"),
        "1.0",
        data,
    )
    .await
}

async fn promotion_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let data = coin_data_with_location(&state, &query.uid, query.params, |data| {
        if !data.contains_key("id") {
            if let Some(value) = data.remove("promotionId") {
                data.insert("id".to_string(), value);
            }
        }
    })
    .await;
    read_h5(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.promotion.detail"),
        "1.0",
        data,
    )
    .await
}

async fn promotion_create(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<PromotionCreatePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    if !payload.confirm {
        return Ok(ResponseJson(local_failure(LOCAL_CONFIRM_REQUIRED_RET)));
    }
    if !same_json_scalar(&payload.expected_cost_coin_num, &payload.cost_coin_num) {
        return Ok(ResponseJson(local_failure(
            LOCAL_EXPECTED_COST_MISMATCH_RET,
        )));
    }

    let promotion_num = payload
        .params
        .get("promotionNum")
        .and_then(value_text)
        .or_else(|| {
            value_i64(&payload.cost_coin_num).map(|cost| cost_to_promotion_num(cost).to_string())
        })
        .unwrap_or_default();
    let promotion_view =
        fetch_promotion_view_for_create(&state, &payload.uid, &payload.item_id).await?;
    let Some(view_cost) = promotion_package_cost(&promotion_view, &promotion_num) else {
        return Ok(ResponseJson(local_failure(
            LOCAL_PROMOTION_PACKAGE_NOT_FOUND_RET,
        )));
    };
    if !same_json_scalar(&Value::from(view_cost), &payload.cost_coin_num) {
        return Ok(ResponseJson(local_failure(
            LOCAL_PROMOTION_PACKAGE_COST_MISMATCH_RET,
        )));
    }

    let mut data = payload.params;
    data.remove("expectedCostCoinNum");
    data.remove("expected_cost_coin_num");
    data.insert(
        "itemId".to_string(),
        json_value_from_numeric_string(&payload.item_id),
    );
    data.insert("costCoinNum".to_string(), payload.cost_coin_num);
    let data = coin_data_with_location(&state, &payload.uid, data, |data| {
        data.entry("aimedTime".to_string())
            .or_insert_with(|| Value::String("00:00~24:00".to_string()));
        data.entry("asac".to_string())
            .or_insert_with(|| Value::String(String::new()));
        if !data.contains_key("promotionNum") && !promotion_num.is_empty() {
            data.insert(
                "promotionNum".to_string(),
                Value::String(promotion_num.clone()),
            );
        }
    })
    .await;
    write_h5(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.promotion.create"),
        "2.0",
        data,
    )
    .await
}

pub(crate) async fn run_coin_promotion_create_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    cost_coin_num: Value,
    mut params: Map<String, Value>,
) -> Result<Value, String> {
    let promotion_num = params
        .get("promotionNum")
        .and_then(value_text)
        .or_else(|| value_i64(&cost_coin_num).map(|cost| cost_to_promotion_num(cost).to_string()))
        .unwrap_or_default();
    let promotion_view = fetch_promotion_view_for_create(state, uid, item_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(view_cost) = promotion_package_cost(&promotion_view, &promotion_num) else {
        return Ok(local_failure(LOCAL_PROMOTION_PACKAGE_NOT_FOUND_RET));
    };
    if !same_json_scalar(&Value::from(view_cost), &cost_coin_num) {
        return Ok(local_failure(LOCAL_PROMOTION_PACKAGE_COST_MISMATCH_RET));
    }

    params.remove("expectedCostCoinNum");
    params.remove("expected_cost_coin_num");
    params.insert(
        "itemId".to_string(),
        json_value_from_numeric_string(item_id),
    );
    params.insert("costCoinNum".to_string(), cost_coin_num);
    let data = coin_data_with_location(state, uid, params, |data| {
        data.entry("aimedTime".to_string())
            .or_insert_with(|| Value::String("00:00~24:00".to_string()));
        data.entry("asac".to_string())
            .or_insert_with(|| Value::String(String::new()));
        if !data.contains_key("promotionNum") && !promotion_num.is_empty() {
            data.insert(
                "promotionNum".to_string(),
                Value::String(promotion_num.clone()),
            );
        }
    })
    .await;
    upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.promotion.create"),
        "2.0",
        data,
        Vec::new(),
    )
    .await
    .map_err(|error| error.to_string())
}

async fn fetch_promotion_view_for_create(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, ResponseError> {
    let mut params = Map::new();
    params.insert(
        "itemId".to_string(),
        json_value_from_numeric_string(item_id),
    );
    let data = coin_data_with_location(state, uid, params, |data| {
        data.entry("cardId".to_string())
            .or_insert_with(|| Value::String(String::new()));
        data.entry("needList".to_string())
            .or_insert_with(|| Value::Bool(true));
    })
    .await;
    successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.promotion.view"),
        "2.0",
        data,
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)
}

async fn read_h5(state: &AppState, uid: &str, api: &str, version: &str, data: Value) -> ApiResult {
    let upstream = successful_upstream_h5_request(state, uid, api, version, data, Vec::new())
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream))
}

async fn write_h5(state: &AppState, uid: &str, api: &str, version: &str, data: Value) -> ApiResult {
    let upstream = upstream_h5_request(state, uid, api, version, data, Vec::new())
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream))
}

fn local_failure(ret: &str) -> Value {
    json!({
        "ret": [ret],
        "data": { "success": false }
    })
}

async fn coin_data_with_location(
    state: &AppState,
    uid: &str,
    mut params: Map<String, Value>,
    apply_defaults: impl FnOnce(&mut Map<String, Value>),
) -> Value {
    let location = load_location_or_default(state, uid).await;
    params
        .entry("fishAdCode".to_string())
        .or_insert_with(|| Value::String(location.fish_ad_code));
    params
        .entry("gps".to_string())
        .or_insert_with(|| Value::String(location.gps));
    apply_defaults(&mut params);
    Value::Object(params)
}

fn normalize_page_params(data: &mut Map<String, Value>) {
    if !data.contains_key("pageNumber") {
        if let Some(value) = data.remove("page") {
            data.insert("pageNumber".to_string(), value);
        }
    }
    data.entry("pageNumber".to_string())
        .or_insert_with(|| Value::String("1".to_string()));
    data.entry("pageSize".to_string())
        .or_insert_with(|| Value::String("20".to_string()));
}

fn normalize_numeric_page_params(
    data: &mut Map<String, Value>,
    default_page: i64,
    default_page_size: i64,
) {
    if !data.contains_key("pageNumber") {
        if let Some(value) = data.remove("page") {
            data.insert("pageNumber".to_string(), coerce_i64_value(value));
        }
    }
    if !data.contains_key("pageSize") {
        if let Some(value) = data.remove("pageSize") {
            data.insert("pageSize".to_string(), coerce_i64_value(value));
        }
    }
    data.entry("pageNumber".to_string())
        .or_insert_with(|| Value::from(default_page));
    data.entry("pageSize".to_string())
        .or_insert_with(|| Value::from(default_page_size));
}

fn coerce_i64_value(value: Value) -> Value {
    match value {
        Value::String(text) => text
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or(Value::String(text)),
        other => other,
    }
}

fn json_value_from_numeric_string(value: &str) -> Value {
    value
        .parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(value.to_string()))
}

fn promotion_package_cost(payload: &Value, promotion_num: &str) -> Option<i64> {
    let data = payload.get("data").and_then(Value::as_object)?;
    let from_price_map = data
        .get("promotionPriceMap")
        .and_then(Value::as_object)
        .and_then(|price_map| price_map.get(promotion_num))
        .and_then(Value::as_object)
        .and_then(package_cost_from_object);
    if from_price_map.is_some() {
        return from_price_map;
    }

    for key in ["packageList", "promotionPackageList", "cardList"] {
        let Some(packages) = data.get(key).and_then(Value::as_array) else {
            continue;
        };
        for package in packages {
            let Some(object) = package.as_object() else {
                continue;
            };
            let matches_num = first_text(object, &["promotionNum", "num", "id"]) == promotion_num;
            if matches_num {
                return package_cost_from_object(object);
            }
        }
    }

    None
}

fn package_cost_from_object(object: &Map<String, Value>) -> Option<i64> {
    [
        "costCoinNum",
        "nowPrice",
        "originalPrice",
        "cost",
        "coinNum",
    ]
    .iter()
    .find_map(|key| object.get(*key).and_then(value_i64))
    .filter(|cost| *cost > 0)
}

fn cost_to_promotion_num(cost_coin_num: i64) -> i64 {
    match cost_coin_num {
        1500 => 50,
        4500 => 100,
        13500 => 300,
        value => value,
    }
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

async fn fetch_coin_task_list(state: &AppState, uid: &str) -> Result<Value, ResponseError> {
    let data = coin_data_with_location(state, uid, Map::new(), |data| {
        data.entry("brand".to_string())
            .or_insert_with(|| Value::String("redmi".to_string()));
        data.entry("isWidgetInstalled".to_string())
            .or_insert_with(|| Value::Bool(false));
        data.entry("osVersion".to_string())
            .or_insert_with(|| Value::String("13".to_string()));
        data.entry("platform".to_string())
            .or_insert_with(|| Value::String("Android".to_string()));
        data.entry("sceneId".to_string())
            .or_insert_with(|| json!(496));
    })
    .await;
    successful_upstream_h5_request(
        state,
        uid,
        COIN_TASK_LIST_API(),
        COIN_TASK_LIST_VERSION,
        data,
        vec![],
    )
    .await
    .map_err(ResponseError::internal_message)
}

#[derive(Debug, Clone)]
struct CoinTaskRef {
    delivery_id: String,
    title: String,
    action_type: String,
    action: String,
    status: String,
    scene_id: String,
    group_id: String,
    impl_id: String,
    from_token: String,
    times: i64,
    need_times: i64,
    material_extra: Value,
    task_benefit: Value,
}

fn extract_coin_tasks(payload: &Value) -> Vec<CoinTaskRef> {
    let mut tasks = Vec::new();
    collect_coin_tasks(payload, &mut tasks);
    tasks
}

fn collect_coin_tasks(value: &Value, tasks: &mut Vec<CoinTaskRef>) {
    match value {
        Value::Object(object) => {
            if object.contains_key("deliveryId") && object.contains_key("materialDTO") {
                let material = object
                    .get("materialDTO")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let progress = object
                    .get("progressDTO")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                tasks.push(CoinTaskRef {
                    delivery_id: first_text(object, &["deliveryId"]),
                    title: first_text(&material, &["title"]),
                    action_type: first_text(&material, &["actionType"]),
                    action: first_text(&material, &["action"]),
                    status: first_text(object, &["idleTaskStatus", "taskStatus", "status"]),
                    scene_id: first_text(object, &["sceneId"]),
                    group_id: first_text(object, &["groupId"]),
                    impl_id: first_text(object, &["implId"]),
                    from_token: first_text(object, &["fromToken"]),
                    times: first_i64(&progress, &["times"]),
                    need_times: first_i64(&progress, &["needTimes", "maxTimes"]),
                    material_extra: material.get("extra").cloned().unwrap_or(Value::Null),
                    task_benefit: object.get("taskBenefitDTO").cloned().unwrap_or(Value::Null),
                });
            }
            for child in object.values() {
                collect_coin_tasks(child, tasks);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_coin_tasks(item, tasks);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Copy)]
struct CoinTaskDecision {
    auto_safe: bool,
    claimable: bool,
    reason: &'static str,
}

fn is_verified_coin_view_task(task: &CoinTaskRef) -> bool {
    coin_task_decision(task).auto_safe
}

fn is_triggerable_safe_coin_view_task(task: &CoinTaskRef) -> bool {
    is_verified_coin_view_task(task) && task.status == "ACCEPTED" && task.times < task.need_times
}

fn is_claimable_safe_coin_view_task(task: &CoinTaskRef) -> bool {
    coin_task_decision(task).claimable
}

fn coin_task_ad_event_payload(task: &CoinTaskRef) -> Option<Value> {
    let ad_position_code = coin_task_ad_position_code(task)?;
    Some(json!({
        "adPositionCode": ad_position_code,
        "eventType": "award",
        "token": "",
        "implId": unix_millis_now(),
    }))
}

fn coin_task_decision(task: &CoinTaskRef) -> CoinTaskDecision {
    let ready_to_claim = coin_task_ready_to_claim(task);
    let base_safe = base_safe_coin_visit_task(task);
    if !base_safe.auto_safe {
        return CoinTaskDecision {
            auto_safe: false,
            claimable: false,
            reason: base_safe.reason,
        };
    }
    CoinTaskDecision {
        auto_safe: true,
        claimable: ready_to_claim,
        reason: if ready_to_claim {
            "safe_ad_visit_ready_to_claim"
        } else {
            "safe_ad_visit_can_trigger"
        },
    }
}

fn base_safe_coin_visit_task(task: &CoinTaskRef) -> CoinTaskDecision {
    if !task.action_type.eq_ignore_ascii_case("VISIT") {
        return CoinTaskDecision {
            auto_safe: false,
            claimable: false,
            reason: "unsupported_action_type",
        };
    }
    let ready_to_claim = coin_task_ready_to_claim(task);
    if task.need_times != 1 && !(ready_to_claim && task.need_times == 0) {
        return CoinTaskDecision {
            auto_safe: false,
            claimable: false,
            reason: "unsupported_need_times",
        };
    }
    if coin_task_ad_position_code(task).is_none() {
        return CoinTaskDecision {
            auto_safe: false,
            claimable: false,
            reason: "missing_ad_position_code",
        };
    }
    if has_risky_coin_task_semantics(task) {
        return CoinTaskDecision {
            auto_safe: false,
            claimable: false,
            reason: "risky_semantics",
        };
    }
    if !is_safe_coin_ad_browse_url(&task.action) {
        return CoinTaskDecision {
            auto_safe: false,
            claimable: false,
            reason: "unsupported_ad_page",
        };
    }
    CoinTaskDecision {
        auto_safe: true,
        claimable: false,
        reason: "safe_ad_visit",
    }
}

fn coin_task_ready_to_claim(task: &CoinTaskRef) -> bool {
    task.times > 0
        || matches!(
            task.status.as_str(),
            "AWARD" | "COMPLETE" | "COMPLETED" | "FINISHED"
        )
        || (task.need_times > 0 && task.times >= task.need_times)
}

fn coin_task_capability_value(task: &CoinTaskRef) -> Value {
    let decision = coin_task_decision(task);
    json!({
        "deliveryId": task.delivery_id,
        "title": task.title,
        "actionType": task.action_type,
        "status": task.status,
        "times": task.times,
        "needTimes": task.need_times,
        "adPositionCode": coin_task_ad_position_code(task).unwrap_or_default(),
        "autoSafe": decision.auto_safe,
        "claimable": decision.claimable,
        "reason": decision.reason,
    })
}

fn skipped_coin_task_value(task: &CoinTaskRef) -> Option<Value> {
    let decision = coin_task_decision(task);
    if decision.auto_safe || decision.claimable {
        return None;
    }
    Some(json!({
        "deliveryId": task.delivery_id,
        "title": task.title,
        "actionType": task.action_type,
        "status": task.status,
        "reason": decision.reason,
    }))
}

fn coin_task_ad_position_code(task: &CoinTaskRef) -> Option<String> {
    extract_query_param(&task.action, "adPositionCode")
        .or_else(|| extract_query_param(&task.action, "adCode"))
}

fn is_safe_coin_ad_browse_url(action: &str) -> bool {
    let decoded = urlencoding::decode(action)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| action.to_string());
    let lower = decoded.to_ascii_lowercase();
    if !(lower.contains("adpositioncode=") || lower.contains("adcode=")) {
        return false;
    }
    if contains_any(
        &lower,
        &[
            "alipay",
            "cashier",
            "payment",
            "pay.html",
            "buy.html",
            "orderconfirm",
            "placeorder",
            "trade",
        ],
    ) {
        return false;
    }
    contains_any(
        &lower,
        &["alimama-ads", "/xianyu/search.html", "/xianyu/like.html"],
    )
}

fn has_risky_coin_task_semantics(task: &CoinTaskRef) -> bool {
    let title = task.title.to_lowercase();
    if contains_any(
        &title,
        &[
            "下单",
            "购买",
            "支付",
            "付款",
            "支付宝",
            "积分",
            "开通",
            "充值",
        ],
    ) {
        return true;
    }

    let decoded_action = urlencoding::decode(&task.action)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| task.action.clone())
        .to_ascii_lowercase();
    if contains_any(
        &decoded_action,
        &[
            "alipay",
            "cashier",
            "payment",
            "pay.html",
            "buy.html",
            "orderconfirm",
            "placeorder",
            "integral",
        ],
    ) {
        return true;
    }

    coin_task_ad_position_code(task)
        .map(|code| {
            let lower = code.to_ascii_lowercase();
            contains_any(&lower, &["buy", "pay", "cashier", "order_pay"])
        })
        .unwrap_or(false)
}

fn coin_task_claim_payload(
    task: &CoinTaskRef,
    location: &crate::xianyu_client::LocationContext,
) -> Value {
    let mut data = Map::new();
    data.insert("fishAdCode".to_string(), json!(location.fish_ad_code));
    data.insert("gps".to_string(), json!(location.gps));
    data.insert(
        "sceneId".to_string(),
        json!(fallback_text(&task.scene_id, "496")),
    );
    data.insert("asac".to_string(), Value::String(String::new()));
    insert_non_empty(&mut data, "deliveryId", &task.delivery_id);
    insert_non_empty(&mut data, "groupId", &task.group_id);
    insert_non_empty(&mut data, "implId", &task.impl_id);
    insert_non_empty(&mut data, "fromToken", &task.from_token);

    merge_extra_claim_fields(&mut data, &task.task_benefit);
    merge_extra_claim_fields(&mut data, &task.material_extra);
    Value::Object(data)
}

fn merge_extra_claim_fields(target: &mut Map<String, Value>, source: &Value) {
    let Some(object) = source.as_object() else {
        return;
    };
    for key in ["oliverActivityId", "oliverBenefitCode", "oliverType"] {
        if target.contains_key(key) {
            continue;
        }
        if let Some(value) = object.get(key) {
            target.insert(key.to_string(), value.clone());
        }
    }
}

fn first_text(object: &Map<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_text))
        .unwrap_or_default()
}

fn first_i64(object: &Map<String, Value>, keys: &[&str]) -> i64 {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(value_i64))
        .unwrap_or_default()
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn value_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn insert_non_empty(data: &mut Map<String, Value>, key: &str, value: &str) {
    if !value.trim().is_empty() {
        data.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn fallback_text<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.trim().is_empty() {
        fallback
    } else {
        value
    }
}

fn extract_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    for pair in query.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name == key && !value.trim().is_empty() {
            return Some(
                urlencoding::decode(value)
                    .map(|decoded| decoded.into_owned())
                    .unwrap_or_else(|_| value.to_string()),
            );
        }
    }
    None
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn unix_millis_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

fn ret_success(payload: &Value) -> bool {
    payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_uppercase()
        .contains("SUCCESS")
}
