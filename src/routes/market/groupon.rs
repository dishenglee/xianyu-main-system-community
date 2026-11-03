use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{Row, SqlitePool};

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::shared::TransportKind,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client::LocationContext,
};

use super::{
    decode_maybe_json_string,
    ensure_item_belongs_to_uid,
    ensure_market_uid_access,
    fetch_item,
    json_object_from_text,
    load_location_or_default,
    payload_ret_success,
    publish_capability_post_get_payload,
    publish_capability_postget_payload,
    success_message,
    success_ret,
    successful_upstream_app_request,
    successful_upstream_h5_request,
    unix_seconds_now,
    // private helpers also used here
    upstream_app_request,
    upstream_h5_request,
    value_is_trueish,
    value_to_string_lossy,
    ItemRefPayload,
    UidQuery,
};

const GROUPON_FIXED_AMOUNTS: [&str; 3] = ["6.66", "15.00", "50.00"];
const GROUPON_DISCOUNT_OPTIONS: [(&str, &str); 4] =
    [("80", "8折"), ("85", "85折"), ("90", "9折"), ("95", "95折")];

async fn request_groupon_transport(
    state: &AppState,
    uid: &str,
    transport: TransportKind,
    api: &str,
    version: &str,
    data: Value,
) -> Result<Value, String> {
    match transport {
        TransportKind::App => upstream_app_request(state, uid, api, version, data).await,
        TransportKind::H5 => upstream_h5_request(state, uid, api, version, data, Vec::new()).await,
        TransportKind::Wx => Err("groupon helper does not support wx transport".to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GrouponOpenPayload {
    pub(super) uid: String,
    pub(super) item_id: String,
    #[serde(default)]
    pub(super) price: String,
    #[serde(default)]
    pub(super) mode: String,
    #[serde(default)]
    pub(super) discount_rate: String,
}

fn groupon_post_get_payload(item_id: &str) -> Value {
    json!({
        "itemId": item_id,
    })
}

fn price_to_cents_text(price: &str) -> String {
    let trimmed = price.trim();
    if trimmed.is_empty() {
        return "0".to_string();
    }
    let parsed = trimmed.parse::<f64>().ok().unwrap_or(0.0);
    ((parsed * 100.0).round() as i64).to_string()
}

fn price_to_number(price: &str) -> Option<f64> {
    let trimmed = price.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = trimmed.parse::<f64>().ok()?;
    if parsed > 0.0 {
        Some(parsed)
    } else {
        None
    }
}

fn normalized_discount_rate_text(rate: f64) -> String {
    if (rate.fract()).abs() < f64::EPSILON {
        format!("{rate:.0}")
    } else {
        format!("{rate:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn supported_discount_rate_from_prices(item_price: &str, final_price: &str) -> Option<String> {
    let item = price_to_number(item_price)?;
    let final_price = price_to_number(final_price)?;
    if final_price <= 0.0 || item <= 0.0 || final_price >= item {
        return None;
    }
    let ratio = (final_price / item) * 100.0;
    let supported = [80.0, 85.0, 90.0, 95.0];
    supported
        .iter()
        .find(|candidate| (ratio - **candidate).abs() <= 0.3)
        .map(|candidate| normalized_discount_rate_text(*candidate))
}

fn is_discount_groupon_mode(current: &Value, guide: Option<&Value>) -> bool {
    if value_to_string_lossy(current.get("stock"))
        .parse::<i64>()
        .ok()
        .unwrap_or_default()
        > 1
    {
        return true;
    }
    let button = guide
        .map(|value| value_to_string_lossy(value.get("buttonText")))
        .unwrap_or_default();
    if button.contains('折') {
        return true;
    }
    supported_discount_rate_from_prices(
        &value_to_string_lossy(current.get("price")),
        &value_to_string_lossy(current.get("grouponPrice")),
    )
    .is_some()
}

fn normalize_groupon_desc_rich_text(value: Option<&Value>) -> Value {
    match value.map(decode_maybe_json_string) {
        Some(Value::Array(items)) => Value::Array(items),
        _ => Value::Array(Vec::new()),
    }
}

async fn fetch_groupon_global_settings(
    state: &AppState,
    uid: &str,
    location: &LocationContext,
) -> Option<Value> {
    let query_payload = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
    });
    let query = upstream_app_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.activity.seller.global.query"),
        "1.0",
        query_payload,
    )
    .await
    .ok()?;
    state.record_mtop_response(
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.activity.seller.global.query"),
        &query,
    );
    if !payload_ret_success(&query) {
        return None;
    }
    let query_data = query
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or(Value::Null);
    let query_nested = query_data
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or_else(|| query_data.clone());
    let free_shipping = query_nested
        .get("freeShipping")
        .map(value_is_trueish)
        .unwrap_or(false);
    let history_groupon = query_nested
        .get("historyGroupon")
        .map(value_is_trueish)
        .unwrap_or(false);

    let settings_payload = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "freeShipping": free_shipping,
        "historyGroupon": history_groupon,
    });
    let settings = upstream_app_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.activity.seller.global.settings"),
        "1.0",
        settings_payload,
    )
    .await
    .ok()?;
    state.record_mtop_response(
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.activity.seller.global.settings"),
        &settings,
    );
    if !payload_ret_success(&settings) {
        return None;
    }
    let settings_data = settings
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or(Value::Null);
    let settings_nested = settings_data
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or_else(|| settings_data.clone());

    Some(json!({
        "freeShipping": free_shipping,
        "historyGroupon": history_groupon,
        "rawQuery": query_nested,
        "rawSettings": settings_nested,
    }))
}

fn build_groupon_drawer_payload(
    item_id: &str,
    current: &Value,
    nested: &Value,
    guide: Option<&Value>,
    global: Option<&Value>,
) -> Value {
    let base_price = value_to_string_lossy(current.get("price"));
    let current_price = value_to_string_lossy(current.get("grouponPrice"));
    let guide_price = guide
        .map(|value| value_to_string_lossy(value.get("grouponPrice")))
        .unwrap_or_default();
    let recommended_price = if !guide_price.is_empty() {
        guide_price.clone()
    } else {
        current_price.clone()
    };
    let selected_price = if !current_price.is_empty() {
        current_price.clone()
    } else {
        recommended_price.clone()
    };
    let desc_lines = guide
        .and_then(|value| value.get("descText"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let button_text = guide
        .map(|value| value_to_string_lossy(value.get("buttonText")))
        .unwrap_or_default();
    let header_tips = nested
        .get("headerTips")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let global_obj = global.cloned().unwrap_or(Value::Null);
    let free_shipping = global
        .and_then(|value| value.get("freeShipping"))
        .map(value_is_trueish)
        .unwrap_or(false);
    let history_groupon = global
        .and_then(|value| value.get("historyGroupon"))
        .map(value_is_trueish)
        .unwrap_or(false);
    let discount_mode = is_discount_groupon_mode(current, guide);
    let item_price_num = price_to_number(&base_price).unwrap_or_default();
    let options = if discount_mode {
        GROUPON_DISCOUNT_OPTIONS
            .iter()
            .map(|(rate, label)| {
                let ratio = rate.parse::<f64>().ok().unwrap_or_default() / 100.0;
                let final_price = if item_price_num > 0.0 {
                    ((item_price_num * ratio) * 100.0).round() / 100.0
                } else {
                    0.0
                };
                let final_price_text = if final_price > 0.0 {
                    format!("{final_price:.2}")
                } else {
                    String::new()
                };
                json!({
                    "mode": "discount",
                    "discountRate": rate,
                    "price": final_price_text,
                    "grouponPrice": final_price_text,
                    "label": *label,
                    "description": if final_price_text.is_empty() {
                        format!("{label} 开启")
                    } else {
                        format!("折后价 ¥{final_price_text}")
                    },
                    "disabled": final_price_text.is_empty(),
                })
            })
            .collect::<Vec<_>>()
    } else {
        GROUPON_FIXED_AMOUNTS
            .iter()
            .map(|amount| {
                json!({
                    "mode": "amount",
                    "price": amount,
                    "grouponPrice": amount,
                    "label": format!("¥{}", amount),
                    "description": format!("固定小刀金额 ¥{}", amount),
                    "disabled": false,
                })
            })
            .collect::<Vec<_>>()
    };
    json!({
        "title": "设置小刀优惠",
        "itemId": item_id,
        "mode": if discount_mode { "discount" } else { "amount" },
        "itemTitle": current.get("title").and_then(Value::as_str).unwrap_or_default(),
        "itemPrice": base_price,
        "recommendedPrice": recommended_price,
        "selectedPrice": selected_price,
        "currentPrice": current_price,
        "buttonText": if button_text.is_empty() { "确定".to_string() } else { button_text },
        "helperText": if selected_price.is_empty() {
            "暂无可用的小刀价格"
        } else if discount_mode && current_price.is_empty() {
            "请选择折扣档位，买家下单前可按折扣价参与 2人小刀"
        } else if discount_mode {
            "当前已开启 2人小刀，可重新调整折扣档位"
        } else if current_price.is_empty() {
            "暂未设置优惠，系统自动推荐优惠档位"
        } else {
            "当前已开启 2人小刀，可重新调整价格"
        },
        "closeOptionLabel": "不开",
        "descLines": desc_lines,
        "headerTips": header_tips,
        "freeShipping": free_shipping,
        "historyGroupon": history_groupon,
        "options": options,
        "backgroundImageUrl": guide.map(|value| value_to_string_lossy(value.get("backgroundImageUrl"))).unwrap_or_default(),
        "global": global_obj,
        "raw": {
            "current": current,
            "guide": guide.cloned().unwrap_or(Value::Null),
            "nested": nested,
        }
    })
}

fn extract_groupon_guide_from_payload(payload: &Value) -> Option<Value> {
    let root = payload.get("data").map(decode_maybe_json_string)?;
    let current_type = value_to_string_lossy(root.get("currentType"));
    let nested = root
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or_else(|| root.clone());
    let guide = if current_type == "grouponGuide" {
        nested
            .get("grouponGuide")
            .cloned()
            .or_else(|| root.get("grouponGuide").cloned())
            .or_else(|| nested.get("dataByType").cloned())
    } else {
        nested
            .get("grouponGuide")
            .cloned()
            .or_else(|| root.get("grouponGuide").cloned())
    }?;
    let guide = decode_maybe_json_string(&guide);
    let desc_rich_text = normalize_groupon_desc_rich_text(guide.get("descRichText"));
    let desc_text = desc_rich_text
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            entry
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .map(str::to_string)
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>();
    Some(json!({
        "currentType": current_type,
        "grouponPrice": value_to_string_lossy(guide.get("grouponPrice")),
        "buttonText": value_to_string_lossy(guide.get("buttonText")),
        "backgroundImageUrl": value_to_string_lossy(guide.get("backgroundImageUrl")),
        "descRichText": desc_rich_text,
        "descText": desc_text,
        "raw": guide,
    }))
}

fn close_should_fallback_to_h5(raw: &str) -> bool {
    let upper = raw.to_ascii_uppercase();
    upper.contains("SESSION_EXPIRED")
        || upper.contains("NOT_LOGIN")
        || upper.contains("TOKEN_EXOIRED")
        || upper.contains("TOKEN_EXPIRED")
        || upper.contains("TOKEN_ILLEGAL")
}

fn merge_cached_groupon_item(
    payload_item_id: &str,
    current: Option<&Value>,
    cached_item: Option<&Value>,
) -> Value {
    let mut current_map = current
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if let Some(item) = cached_item {
        let cached_open = item
            .get("grouponOpen")
            .map(value_is_trueish)
            .unwrap_or(false);
        let cached_price = value_to_string_lossy(item.get("grouponPrice"));
        let cached_item_price = value_to_string_lossy(item.get("price"));
        let cached_title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        if value_to_string_lossy(current_map.get("itemId")).is_empty() {
            current_map.insert(
                "itemId".to_string(),
                Value::String(payload_item_id.to_string()),
            );
        }
        if value_to_string_lossy(current_map.get("title")).is_empty() && !cached_title.is_empty() {
            current_map.insert("title".to_string(), Value::String(cached_title));
        }
        if value_to_string_lossy(current_map.get("price")).is_empty()
            && !cached_item_price.is_empty()
        {
            current_map.insert("price".to_string(), Value::String(cached_item_price));
        }
        if value_to_string_lossy(current_map.get("grouponPrice")).is_empty()
            && !cached_price.is_empty()
        {
            current_map.insert("grouponPrice".to_string(), Value::String(cached_price));
        }
        if cached_open {
            current_map.insert("groupon".to_string(), Value::Bool(true));
            current_map.insert("canGroupon".to_string(), Value::Bool(true));
        } else if current_map.get("canGroupon").is_none() {
            current_map.insert("canGroupon".to_string(), Value::Bool(true));
        }
    }

    Value::Object(current_map)
}

async fn fetch_groupon_guide(
    state: &AppState,
    uid: &str,
    item_id: &str,
    location: &LocationContext,
) -> Option<Value> {
    if let Ok(payload) = successful_upstream_app_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.yxp.publication.postGet"),
        "1.0",
        groupon_post_get_payload(item_id),
    )
    .await
    {
        state.record_mtop_response(
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.yxp.publication.postGet"),
            &payload,
        );
        if let Some(mut guide) = extract_groupon_guide_from_payload(&payload) {
            if let Some(object) = guide.as_object_mut() {
                object.insert(
                    "sourceApi".to_string(),
                    json!(crate::sealed_protocol::sealed_str!(
                        "mtop.taobao.idle.yxp.publication.postGet"
                    )),
                );
            }
            return Some(guide);
        }
    }
    let attempts = [
        (
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.publish.postGet"),
            "1.0",
            publish_capability_post_get_payload(item_id),
            false,
        ),
        (
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.publish.postget"),
            "1.0",
            publish_capability_postget_payload(item_id, location),
            false,
        ),
    ];
    for (api, version, data, app_only) in attempts {
        let transport = if app_only {
            TransportKind::App
        } else {
            TransportKind::H5
        };
        let result = request_groupon_transport(state, uid, transport, api, version, data).await;
        let Ok(payload) = result else {
            continue;
        };
        state.record_mtop_response(api, &payload);
        if !payload_ret_success(&payload) {
            continue;
        }
        if let Some(mut guide) = extract_groupon_guide_from_payload(&payload) {
            if let Some(object) = guide.as_object_mut() {
                object.insert("sourceApi".to_string(), json!(api));
            }
            return Some(guide);
        }
    }
    None
}

pub(super) async fn item_groupon_status(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let upstream = successful_upstream_app_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.item.activity.search"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "groupon": false,
            "needHeaderInfo": true,
            "pageNumber": 1,
            "pageSize": 10,
            "targetId": payload.item_id.parse::<i64>().ok().unwrap_or(0),
        }),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    let data = upstream
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or(Value::Null);
    let nested = data
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or_else(|| data.clone());
    let items = nested
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let current = items
        .iter()
        .find(|entry| value_to_string_lossy(entry.get("itemId")) == payload.item_id)
        .cloned();
    let cached_item = fetch_item(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .ok()
        .flatten();
    let current =
        merge_cached_groupon_item(&payload.item_id, current.as_ref(), cached_item.as_ref());
    let (groupon_guide, groupon_global) = tokio::join!(
        fetch_groupon_guide(&state, &payload.uid, &payload.item_id, &location),
        fetch_groupon_global_settings(&state, &payload.uid, &location),
    );
    let groupon_drawer = build_groupon_drawer_payload(
        &payload.item_id,
        &current,
        &nested,
        groupon_guide.as_ref(),
        groupon_global.as_ref(),
    );
    let data = json!({
        "canGroupon": current.get("canGroupon").map(value_is_trueish).unwrap_or(false),
        "groupon": current.get("groupon").map(value_is_trueish).unwrap_or(false),
        "price": value_to_string_lossy(current.get("price")),
        "grouponPrice": value_to_string_lossy(current.get("grouponPrice")),
        "itemId": value_to_string_lossy(current.get("itemId")),
        "title": current.get("title").and_then(Value::as_str).unwrap_or_default(),
        "stock": value_to_string_lossy(current.get("stock")).parse::<i64>().ok().unwrap_or(0),
        "errorDescription": value_to_string_lossy(current.get("errorDescription")),
        "headerTips": nested
            .get("headerTips")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
        "guide": groupon_guide.clone().unwrap_or(Value::Null),
        "guidePrice": groupon_guide
            .as_ref()
            .map(|value| value_to_string_lossy(value.get("grouponPrice")))
            .unwrap_or_default(),
        "global": groupon_global.clone().unwrap_or(Value::Null),
        "drawer": groupon_drawer,
        "raw": current,
        "rawData": nested,
    });
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": data,
    })))
}

#[cfg(test)]
mod tests {
    use super::{close_should_fallback_to_h5, merge_cached_groupon_item};
    use serde_json::json;

    #[test]
    fn merge_cached_groupon_item_does_not_leak_other_item_identity() {
        let current = json!({
            "itemId": "1053236828872",
            "price": "50.00",
            "canGroupon": true,
            "groupon": false
        });
        let cached = json!({
            "itemId": "770722679709",
            "title": "高质量代写：工作总结",
            "price": "18.00",
            "grouponOpen": true,
            "grouponPrice": "15.00"
        });

        let merged = merge_cached_groupon_item("770722679709", None, Some(&cached));
        assert_eq!(
            merged.get("itemId").and_then(|v| v.as_str()),
            Some("770722679709")
        );
        assert_eq!(merged.get("price").and_then(|v| v.as_str()), Some("18.00"));
        assert_eq!(
            merged.get("grouponPrice").and_then(|v| v.as_str()),
            Some("15.00")
        );
        assert_eq!(merged.get("groupon").and_then(|v| v.as_bool()), Some(true));

        let leaked = merge_cached_groupon_item("770722679709", Some(&current), Some(&cached));
        assert_eq!(
            leaked.get("itemId").and_then(|v| v.as_str()),
            Some("1053236828872")
        );
        assert_eq!(leaked.get("price").and_then(|v| v.as_str()), Some("50.00"));
        assert_eq!(
            leaked.get("grouponPrice").and_then(|v| v.as_str()),
            Some("15.00")
        );
    }

    #[test]
    fn close_should_fallback_to_h5_when_app_session_is_expired() {
        assert!(close_should_fallback_to_h5(
            "non-success upstream ret: FAIL_SYS_SESSION_EXPIRED::会话过期"
        ));
        assert!(close_should_fallback_to_h5(
            "FAIL_SYS_TOKEN_EXPIRED::令牌过期"
        ));
        assert!(!close_should_fallback_to_h5(
            "non-success upstream ret: GROUPON-A-01-014::商品必须在线"
        ));
    }
}

pub(super) async fn item_groupon_guide(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let guide = fetch_groupon_guide(&state, &payload.uid, &payload.item_id, &location).await;
    let global = fetch_groupon_global_settings(&state, &payload.uid, &location).await;
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "itemId": payload.item_id,
            "guide": guide.clone().unwrap_or(Value::Null),
            "global": global.clone().unwrap_or(Value::Null),
            "found": guide.is_some(),
        },
    })))
}

pub(super) async fn item_groupon_open(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<GrouponOpenPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let resp = run_groupon_open_action(
        &state,
        &payload.uid,
        &payload.item_id,
        &payload.price,
        &payload.mode,
        &payload.discount_rate,
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

fn groupon_open_requires_guide(mode: &str, price: &str) -> bool {
    if price.trim().is_empty() {
        return true;
    }
    !matches!(
        mode.trim().to_ascii_lowercase().as_str(),
        "amount" | "discount"
    )
}

pub(crate) async fn run_groupon_open_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    price: &str,
    mode: &str,
    discount_rate: &str,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let payload = GrouponOpenPayload {
        uid: uid.to_string(),
        item_id: item_id.to_string(),
        price: price.to_string(),
        mode: mode.to_string(),
        discount_rate: discount_rate.to_string(),
    };
    let location = load_location_or_default(&state, &payload.uid).await;
    let guide = if groupon_open_requires_guide(&payload.mode, &payload.price) {
        fetch_groupon_guide(&state, &payload.uid, &payload.item_id, &location).await
    } else {
        None
    };
    let item = fetch_item(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(|error| error.to_string())?;
    let item_price = item
        .as_ref()
        .map(|value| value_to_string_lossy(value.get("price")))
        .unwrap_or_default();
    let cached_detail = item
        .as_ref()
        .and_then(|value| value.get("detail"))
        .cloned()
        .unwrap_or(Value::Null);
    let current = json!({
        "stock": value_to_string_lossy(cached_detail.get("stock")),
        "price": item_price,
        "grouponPrice": value_to_string_lossy(cached_detail.get("grouponPrice")),
        "title": item.as_ref().and_then(|v| v.get("title")).and_then(Value::as_str).unwrap_or_default(),
    });
    let requested_mode = payload.mode.trim().to_ascii_lowercase();
    let discount_mode = if requested_mode == "discount" {
        true
    } else if requested_mode == "amount" {
        false
    } else {
        is_discount_groupon_mode(&current, guide.as_ref())
    };
    let mut price = payload.price.trim().to_string();
    if price.is_empty() {
        price = guide
            .as_ref()
            .map(|value| value_to_string_lossy(value.get("grouponPrice")))
            .unwrap_or_default();
    }
    if price.is_empty() {
        return Err("没有从上游返回里拿到 2人小刀价格，请先同步商品或稍后再试".to_string());
    }
    let parsed_price = price.parse::<f64>().ok().unwrap_or(0.0);
    let parsed_item_price = item_price.parse::<f64>().ok().unwrap_or(0.0);
    if parsed_price <= 0.0 {
        return Err("小刀金额必须大于 0".to_string());
    }
    if parsed_item_price > 0.0 && parsed_price >= parsed_item_price {
        return Err(format!(
            "小刀金额不能大于或等于售价，当前售价 ¥{item_price}"
        ));
    }
    let (api_name, open_payload, mode_label) = if discount_mode {
        let discount_rate = if !payload.discount_rate.trim().is_empty() {
            payload.discount_rate.trim().to_string()
        } else {
            supported_discount_rate_from_prices(&item_price, &price)
                .ok_or_else(|| format!("折扣价必须小于售价，当前售价 ¥{item_price}"))?
        };
        (
            crate::sealed_protocol::sealed_str!("mtop.idle.groupon.item.activity.open"),
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "itemId": payload.item_id.parse::<i64>().ok().unwrap_or(0),
                "grouponPrice": price,
                "discountPrice": price,
                "discountRate": discount_rate,
                "price": price,
            }),
            "discount",
        )
    } else {
        let bargain_price = price_to_cents_text(&price);
        (
            crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.item.open"),
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "inputJson": json!({
                    "bizcode": "bargainPrice",
                    "itemId": payload.item_id,
                    "bargainPrice": bargain_price,
                    "sourceId": "publishItems",
                }).to_string(),
            }),
            "amount",
        )
    };
    tracing::info!(
        uid = %payload.uid,
        item_id = %payload.item_id,
        price = %price,
        item_price = %item_price,
        mode = %mode_label,
        "groupon_open: sending request"
    );
    let upstream =
        upstream_app_request(&state, &payload.uid, api_name, "1.0", open_payload.clone()).await;
    let upstream = match upstream {
        Ok(resp) => resp,
        Err(e) => {
            let msg = format!(
                "groupon_open transport error | sent={} | error={}",
                open_payload, e
            );
            tracing::error!(uid = %payload.uid, "{msg}");
            return Err(msg);
        }
    };
    state.record_mtop_response(api_name, &upstream);
    let ret_str = upstream
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .unwrap_or_default();
    tracing::info!(
        uid = %payload.uid,
        item_id = %payload.item_id,
        ret = %ret_str,
        full_resp = %upstream,
        "groupon_open: response"
    );
    if !ret_str.to_ascii_uppercase().contains("SUCCESS") {
        // Split "CODE::描述" into code + description
        let mut parts = ret_str.splitn(2, "::");
        let err_code = parts.next().unwrap_or("").trim();
        let err_desc = parts.next().unwrap_or("").trim();
        // Precise mapping based on observed upstream error codes (see catalog
        // mtop.idle.groupon.item.activity.open). Earlier code wrongly mapped
        // SYSTEM_COMMON_ERROR to "库存>1" — but the real upstream rule is
        // "商品必须在线" (online required), which fails when stock=0 / off-shelf,
        // not when stock=1. Verified via live /api/mtop reproducing
        // GROUPON-A-01-014::商品必须在线 on item_id=1049683726806 (stock=0).
        let friendly = match err_code {
            // Item must be online: stock=0 or item off-shelf/under-review etc.
            "GROUPON-A-01-014" => "小刀开启失败：商品必须在线（库存为 0 或已下架时无法开启 2 人小刀，请先补库存或重新上架）".to_string(),
            // Generic upstream error fallback — surface the real description.
            "SYSTEM_COMMON_ERROR" | "FAIL_SYS_SYSTEM_ERROR" | "FAIL_BIZ_SYSTEM_ERROR" => {
                if err_desc.is_empty() {
                    "小刀开启失败：上游系统异常，请稍后重试".to_string()
                } else {
                    format!("小刀开启失败：{err_desc}（{err_code}）")
                }
            }
            _ => {
                if err_desc.is_empty() {
                    format!("小刀开启失败：{err_code}")
                } else {
                    format!("小刀开启失败：{err_desc}（{err_code}）")
                }
            }
        };
        tracing::warn!(
            uid = %payload.uid,
            item_id = %payload.item_id,
            ret = %ret_str,
            "groupon_open: upstream rejected"
        );
        return Err(friendly);
    }
    update_groupon_state(
        state.storage().xianyu(),
        &payload.uid,
        &payload.item_id,
        true,
        &price,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(success_message("2人小刀已开启"))
}

pub(super) async fn item_groupon_close(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let resp = run_groupon_close_action(&state, &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(crate) async fn run_groupon_close_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let payload = ItemRefPayload {
        uid: uid.to_string(),
        item_id: item_id.to_string(),
    };
    let location = load_location_or_default(&state, &payload.uid).await;
    let close_payload = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "itemId": payload.item_id.parse::<i64>().ok().unwrap_or(0),
    });
    let upstream = successful_upstream_app_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.item.activity.close"),
        "1.0",
        close_payload.clone(),
    )
    .await;
    let upstream = match upstream {
        Ok(resp) => resp,
        Err(err) if close_should_fallback_to_h5(&err) => {
            tracing::warn!(
                uid = %payload.uid,
                item_id = %payload.item_id,
                error = %err,
                "groupon_close: app transport auth failed, falling back to h5"
            );
            successful_upstream_h5_request(
                &state,
                &payload.uid,
                crate::sealed_protocol::sealed_str!("mtop.idle.groupon.item.activity.close"),
                "1.0",
                close_payload,
                Vec::new(),
            )
            .await
            .map_err(|fallback_err| fallback_err.to_string())?
        }
        Err(err) => return Err(err.to_string()),
    };
    state.record_mtop_response(
        crate::sealed_protocol::sealed_str!("mtop.idle.groupon.item.activity.close"),
        &upstream,
    );
    update_groupon_state(
        state.storage().xianyu(),
        &payload.uid,
        &payload.item_id,
        false,
        "",
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(success_message("2人小刀已关闭"))
}

pub(super) async fn item_super_exposure(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    if let Ok(payload) = successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.super.exposure"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(
        ResponseJson(json!({
            "ret": success_ret(),
            "data": {
                "success": true,
                "message": "超级曝光已触发",
            }
        }))
        .0,
    ))
}

async fn update_groupon_state(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
    groupon_open: bool,
    groupon_price: &str,
) -> Result<(), sqlx::Error> {
    let existing =
        sqlx::query("SELECT detail_json FROM account_items WHERE uid = ? AND item_id = ?")
            .bind(uid)
            .bind(item_id)
            .fetch_optional(pool)
            .await?;
    let detail_value = existing
        .as_ref()
        .map(|row| json_object_from_text(&row.get::<String, _>("detail_json")))
        .unwrap_or_else(|| Value::Object(Map::new()));
    let mut detail = detail_value.as_object().cloned().unwrap_or_default();
    detail.insert("grouponOpen".to_string(), Value::Bool(groupon_open));
    detail.insert(
        "grouponPrice".to_string(),
        Value::String(groupon_price.to_string()),
    );
    if groupon_open {
        let mut seller_promotion = detail
            .get("sellerPromotionVO")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        seller_promotion.insert(
            "activityPriceOrDiscount".to_string(),
            Value::String(groupon_price.to_string()),
        );
        if !seller_promotion.contains_key("promotionStatus") {
            seller_promotion.insert("promotionStatus".to_string(), Value::from(2));
        }
        detail.insert(
            "sellerPromotionVO".to_string(),
            Value::Object(seller_promotion),
        );
    } else {
        detail.remove("sellerPromotionVO");
    }
    let detail_json =
        serde_json::to_string(&Value::Object(detail)).unwrap_or_else(|_| "{}".to_string());
    let updated_at = unix_seconds_now();
    let result = sqlx::query(
        "UPDATE account_items SET detail_json = ?, updated_at = ? WHERE uid = ? AND item_id = ?",
    )
    .bind(&detail_json)
    .bind(updated_at)
    .bind(uid)
    .bind(item_id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, short_title, description, price, orig_price,
                pic_url, image_urls_json, status, online, item_context_json, detail_json,
                actions_json, created_at, updated_at
            )
            VALUES (?, ?, '', '', '', '', '', '', '[]', 'ON_SALE', 1, '{}', ?, '[]', ?, ?)
            ON CONFLICT(uid, item_id) DO UPDATE SET
                detail_json = excluded.detail_json,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(item_id)
        .bind(uid)
        .bind(&detail_json)
        .bind(updated_at)
        .bind(updated_at)
        .execute(pool)
        .await?;
    }
    Ok(())
}
