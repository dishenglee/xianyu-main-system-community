use axum::{
    extract::{Extension, Query, State},
    routing::get,
    Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
};

use super::{
    load_location_or_default, location_payload, successful_upstream_protocol_h5_request, ApiResult,
    ResponseError,
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/gray"),
            get(gray_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/begin-time"),
            get(begin_time),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/merchant"),
            get(merchant_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/summary"),
            get(summary_bundle),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/flow"),
            get(flow_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/items"),
            get(item_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/item-indicators"),
            get(item_indicators),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/fans"),
            get(fans_bundle),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/refund"),
            get(refund_summary),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/shop/datacompass/cs"),
            get(cs_bundle),
        )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatacompassQuery {
    uid: String,
    #[serde(default)]
    date_type: Option<String>,
    #[serde(default)]
    date_range: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

async fn gray_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_GRAY_INFO).await
}

async fn begin_time(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_BEGIN_TIME).await
}

async fn merchant_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_MERCHANT_INFO).await
}

async fn flow_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_FLOW_DETAIL).await
}

async fn item_list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_ITEM_LIST).await
}

async fn item_indicators(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_ITEM_INDICATORS).await
}

async fn refund_summary(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    single_endpoint(state, identity, query, opcodes::DATACOMPASS_REFUND_SUMMARY).await
}

async fn summary_bundle(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    ensure_datacompass_uid_access(&state, &identity, &query.uid).await?;
    let uid = query.uid.clone();
    let data = request_data(&state, &query).await;
    let seller = call_h5(
        &state,
        &uid,
        opcodes::DATACOMPASS_SELLER_SUMMARY,
        data.clone(),
    )
    .await?;
    let browse = call_h5(
        &state,
        &uid,
        opcodes::DATACOMPASS_BROWSE_SUMMARY,
        data.clone(),
    )
    .await?;
    let item = call_h5(
        &state,
        &uid,
        opcodes::DATACOMPASS_ITEM_SUMMARY,
        data.clone(),
    )
    .await?;
    let repurchase = call_h5(&state, &uid, opcodes::DATACOMPASS_REPURCHASE_SUMMARY, data).await?;
    Ok(ResponseJson(bundle_response(json!({
        "seller": seller,
        "browse": browse,
        "item": item,
        "repurchase": repurchase,
    }))))
}

async fn fans_bundle(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    ensure_datacompass_uid_access(&state, &identity, &query.uid).await?;
    let uid = query.uid.clone();
    let data = request_data(&state, &query).await;
    let summary = call_h5(
        &state,
        &uid,
        opcodes::DATACOMPASS_FANS_SUMMARY,
        data.clone(),
    )
    .await?;
    let insights = call_h5(&state, &uid, opcodes::DATACOMPASS_FANS_INSIGHTS, data).await?;
    Ok(ResponseJson(bundle_response(json!({
        "summary": summary,
        "insights": insights,
    }))))
}

async fn cs_bundle(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DatacompassQuery>,
) -> ApiResult {
    ensure_datacompass_uid_access(&state, &identity, &query.uid).await?;
    let uid = query.uid.clone();
    let data = request_data(&state, &query).await;
    let overview = call_h5(&state, &uid, opcodes::DATACOMPASS_CS_OVERVIEW, data.clone()).await?;
    let detail = call_h5(&state, &uid, opcodes::DATACOMPASS_CS_DETAIL, data.clone()).await?;
    let evaluation = call_h5(&state, &uid, opcodes::DATACOMPASS_CS_EVALUATION, data).await?;
    Ok(ResponseJson(bundle_response(json!({
        "overview": overview,
        "detail": detail,
        "evaluation": evaluation,
    }))))
}

async fn single_endpoint(
    state: AppState,
    identity: RequestIdentity,
    query: DatacompassQuery,
    opcode: u32,
) -> ApiResult {
    ensure_datacompass_uid_access(&state, &identity, &query.uid).await?;
    let uid = query.uid.clone();
    let data = request_data(&state, &query).await;
    let upstream = call_h5(&state, &uid, opcode, data).await?;
    Ok(ResponseJson(upstream))
}

async fn ensure_datacompass_uid_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid.trim()).await
}

async fn call_h5(
    state: &AppState,
    uid: &str,
    opcode: u32,
    data: Value,
) -> Result<Value, ResponseError> {
    let upstream = successful_upstream_protocol_h5_request(state, uid, opcode, data, Vec::new())
        .await
        .map_err(ResponseError::service_unavailable)?;
    Ok(with_normalized(upstream))
}

async fn request_data(state: &AppState, query: &DatacompassQuery) -> Value {
    let location = load_location_or_default(state, &query.uid).await;
    let mut data = location_payload(&location);
    data.insert(
        "dateRange".to_string(),
        Value::String(query.date_range.clone().unwrap_or_default()),
    );
    data.insert(
        "dateType".to_string(),
        Value::String(
            query
                .date_type
                .clone()
                .unwrap_or_else(|| "recent1d".to_string()),
        ),
    );
    for (key, value) in query.extra.clone() {
        if key != "uid" {
            data.insert(key, value);
        }
    }
    Value::Object(data)
}

fn bundle_response(data: Value) -> Value {
    let normalized = normalize_value(&data);
    let data = match data {
        Value::Object(mut object) => {
            object.insert("normalized".to_string(), normalized);
            Value::Object(object)
        }
        raw => json!({
            "raw": raw,
            "normalized": normalized,
        }),
    };
    json!({
        "ret": ["SUCCESS::调用成功"],
        "data": data,
    })
}

fn with_normalized(mut upstream: Value) -> Value {
    let normalized = normalize_value(upstream.get("data").unwrap_or(&upstream));
    if let Some(data) = upstream.get_mut("data") {
        match data {
            Value::Object(object) => {
                object.insert("normalized".to_string(), normalized);
            }
            _ => {
                upstream["data"] = json!({
                    "raw": data.clone(),
                    "normalized": normalized,
                });
            }
        }
    } else if let Value::Object(object) = &mut upstream {
        object.insert(
            "data".to_string(),
            json!({
                "normalized": normalized,
            }),
        );
    }
    upstream
}

fn normalize_value(value: &Value) -> Value {
    let (start, end) = real_date_range(value);
    json!({
        "realDateRange": [start, end],
        "metrics": collect_metrics(value),
    })
}

fn real_date_range(value: &Value) -> (String, String) {
    let start = first_string(
        value,
        &["realStartDate", "startDate", "beginDate", "beginTime"],
    );
    let end = first_string(value, &["realEndDate", "endDate", "finishDate", "endTime"]);
    let fallback = first_string(value, &["realDate", "statDate", "date", "day"]);
    let start = if start.is_empty() {
        fallback.clone()
    } else {
        start
    };
    let end = if end.is_empty() { fallback } else { end };
    (start, end)
}

fn first_string(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(found) = find_key(value, key).and_then(value_to_string) {
            return found;
        }
    }
    String::new()
}

fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(object) => {
            if let Some(found) = object.get(key) {
                return Some(found);
            }
            object.values().find_map(|item| find_key(item, key))
        }
        Value::Array(items) => items.iter().find_map(|item| find_key(item, key)),
        _ => None,
    }
}

fn collect_metrics(value: &Value) -> Vec<Value> {
    let mut metrics = Vec::new();
    collect_metrics_from_value(value, &mut metrics);
    metrics
}

fn collect_metrics_from_value(value: &Value, metrics: &mut Vec<Value>) {
    match value {
        Value::Object(object) => {
            if looks_like_metric(object) {
                metrics.push(metric_from_object(object));
            }
            for (key, item) in object {
                if is_metric_scalar(item) {
                    metrics.push(json!({
                        "name": key,
                        "label": label_for_metric(key),
                        "value": item.clone(),
                        "display": display_for_value(item),
                        "ratio": "-",
                    }));
                } else {
                    collect_metrics_from_value(item, metrics);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_metrics_from_value(item, metrics);
            }
        }
        _ => {}
    }
}

fn looks_like_metric(object: &Map<String, Value>) -> bool {
    object.contains_key("value")
        && (object.contains_key("name")
            || object.contains_key("key")
            || object.contains_key("label")
            || object.contains_key("title"))
}

fn metric_from_object(object: &Map<String, Value>) -> Value {
    let name = object
        .get("name")
        .or_else(|| object.get("key"))
        .and_then(value_to_string)
        .unwrap_or_default();
    let label = object
        .get("label")
        .or_else(|| object.get("title"))
        .and_then(value_to_string)
        .unwrap_or_else(|| label_for_metric(&name));
    let value = object.get("value").cloned().unwrap_or(Value::Null);
    let display = object
        .get("display")
        .or_else(|| object.get("displayValue"))
        .or_else(|| object.get("valueStr"))
        .and_then(value_to_string)
        .unwrap_or_else(|| display_for_value(&value));
    let ratio = object
        .get("ratio")
        .or_else(|| object.get("rate"))
        .or_else(|| object.get("compareValue"))
        .and_then(value_to_string)
        .unwrap_or_else(|| "-".to_string());
    json!({
        "name": name,
        "label": label,
        "value": value,
        "display": display,
        "ratio": ratio,
    })
}

fn is_metric_scalar(value: &Value) -> bool {
    matches!(value, Value::Number(_))
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => Some(raw.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn display_for_value(value: &Value) -> String {
    value_to_string(value).unwrap_or_default()
}

fn label_for_metric(name: &str) -> String {
    match name {
        "payAmt" => "支付金额",
        "payBuyerCnt" => "支付买家数",
        "visitorCnt" => "访客数",
        "browseCnt" => "浏览量",
        "itemCnt" => "商品数",
        "refundAmt" => "退款金额",
        "refundCnt" => "退款笔数",
        "fansCnt" => "粉丝数",
        "csReplyRate" => "客服回复率",
        _ => name,
    }
    .to_string()
}
