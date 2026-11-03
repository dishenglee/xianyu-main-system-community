use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tracing::{debug, warn};

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::shared::first_ret_string,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client::LocationContext,
};

use super::publish_kgraph::{build_kgraph_payload, extract_kgraph_cat_info};
use super::{
    build_image_info_list, build_post_fee_dto, ensure_market_uid_access,
    infer_simple_item_from_category, load_location_or_default, payload_ret_success,
    publish_capability_post_get_payload, publish_capability_postget_payload, success_ret,
    successful_upstream_h5_request, unix_millis_now, upstream_h5_request, value_to_string_lossy,
    UidQuery,
};

fn json_log_summary(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().take(24).cloned().collect::<Vec<_>>();
            keys.sort();
            json!({
                "kind": "object",
                "keyCount": map.len(),
                "keys": keys,
                "dataKeyCount": map.get("data").and_then(Value::as_object).map(|data| data.len()).unwrap_or(0),
            })
        }
        Value::Array(values) => json!({ "kind": "array", "len": values.len() }),
        Value::String(text) => json!({ "kind": "string", "len": text.chars().count() }),
        Value::Null => json!({ "kind": "null" }),
        Value::Bool(_) => json!({ "kind": "bool" }),
        Value::Number(_) => json!({ "kind": "number" }),
    }
}

fn json_log_summary_string(value: &Value) -> String {
    serde_json::to_string(&json_log_summary(value)).unwrap_or_else(|_| "{}".to_string())
}

// ─── serde defaults ──────────────────────────────────────────────────────────

fn default_page() -> i64 {
    1
}
fn default_page_size_20() -> i64 {
    20
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_shipping_type() -> String {
    "free".to_string()
}
fn default_quantity_one() -> i64 {
    1
}

// ─── Structs ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct KgraphPayload {
    pub(super) uid: String,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) desc: String,
    #[serde(default)]
    pub(super) cat_id: String,
    #[serde(default)]
    pub(super) channel_cat_id: String,
    #[serde(default)]
    pub(super) current_card_list: Vec<Value>,
    #[serde(default)]
    pub(super) selected_list: Vec<Value>,
    #[serde(default)]
    pub(super) image_urls: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(super) struct ItemAiDescriptionPayload {
    pub(super) uid: String,
    #[serde(default)]
    pub(super) title: String,
    #[serde(default)]
    pub(super) desc: String,
    #[serde(default)]
    pub(super) category: Value,
    #[serde(default)]
    pub(super) image_urls: Vec<String>,
    #[serde(default)]
    pub(super) card_list: Vec<Value>,
    #[serde(default)]
    pub(super) prop_selections: Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemPublishPayload {
    pub(super) uid: String,
    pub(super) title: String,
    #[serde(default)]
    pub(super) short_title: String,
    #[serde(default)]
    pub(super) desc: String,
    #[serde(default)]
    pub(super) price: String,
    #[serde(default)]
    pub(super) orig_price: String,
    #[serde(default)]
    pub(super) image_urls: Vec<String>,
    #[serde(default = "default_false")]
    pub(super) simple_item: bool,
    #[serde(default)]
    pub(super) no_bargain: bool,
    #[serde(default = "default_true")]
    pub(super) can_free_shipping: bool,
    #[serde(default = "default_shipping_type")]
    pub(super) shipping_type: String,
    #[serde(default)]
    pub(super) post_price: String,
    #[serde(default)]
    pub(super) freight_template_id: String,
    #[serde(default = "default_quantity_one")]
    pub(super) quantity: i64,
    #[serde(default)]
    pub(super) card_list: Vec<Value>,
    #[serde(default)]
    pub(super) prop_selections: Value,
    #[serde(default)]
    pub(super) city: String,
    #[serde(default)]
    pub(super) province: String,
    #[serde(default)]
    pub(super) area: String,
    #[serde(default)]
    pub(super) division_id: String,
    #[serde(default)]
    pub(super) latitude: String,
    #[serde(default)]
    pub(super) longitude: String,
    #[serde(default)]
    pub(super) fish_ad_code: String,
}

fn normalize_publish_desc_headline(desc: &str) -> String {
    desc.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_start_matches("图片版描述：")
        .trim_start_matches("图片版描述:")
        .trim_start_matches("AI描述：")
        .trim_start_matches("AI描述:")
        .trim_start_matches("商品描述：")
        .trim_start_matches("商品描述:")
        .trim()
        .to_string()
}

pub(super) fn derive_publish_title_from_payload(title: &str, desc: &str) -> String {
    let normalized_title = title.trim();
    if !normalized_title.is_empty() {
        return normalized_title.to_string();
    }
    let headline = normalize_publish_desc_headline(desc);
    if headline.is_empty() {
        return "闲置商品".to_string();
    }
    headline
        .split(|c| matches!(c, '。' | '！' | '？' | '!' | '?'))
        .next()
        .unwrap_or(headline.as_str())
        .split(|c| matches!(c, '，' | ',' | '；' | ';'))
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or(headline.as_str())
        .trim()
        .chars()
        .take(60)
        .collect()
}

#[derive(Debug, Deserialize)]
pub(super) struct AwesomeDetailPayload {
    pub(super) uid: String,
    pub(super) item_id: String,
    #[serde(default)]
    pub(super) city: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemImportPayload {
    pub(super) uid: String,
    pub(super) text: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct BadwordsQuery {
    uid: String,
    title: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct SellerRecommendQuery {
    pub(super) uid: String,
    #[serde(default = "default_page")]
    pub(super) page: i64,
    #[serde(default = "default_page_size_20")]
    pub(super) page_size: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct PublishCapabilitiesQuery {
    uid: String,
    #[serde(default)]
    item_id: String,
    #[serde(default)]
    title: String,
}

// ─── publish-local helper functions ──────────────────────────────────────────

fn price_to_cents(value: &str) -> String {
    super::price_to_cents(value)
}

fn build_publish_labels(cat_info: Option<&Value>) -> Vec<Value> {
    build_publish_labels_from_selection(cat_info, &[], &Value::Null)
}

fn build_publish_labels_from_selection(
    cat_info: Option<&Value>,
    card_list: &[Value],
    prop_selections: &Value,
) -> Vec<Value> {
    let Some(cat) = cat_info.and_then(Value::as_object) else {
        return Vec::new();
    };
    let channel_cat_id = cat
        .get("channelCatId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if channel_cat_id.is_empty() {
        return Vec::new();
    }
    let cat_name = cat
        .get("catName")
        .or_else(|| cat.get("channelCatName"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let tb_cat_id = cat
        .get("tbCatId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut labels = vec![json!({
        "channelCateId": channel_cat_id,
        "channelCateName": cat_name,
        "from": "newPublishChoice",
        "isUserClick": "0",
        "labelFrom": "newPublish",
        "labelType": "common",
        "properties": format!("-10000##分类:{}##{}", channel_cat_id, cat_name),
        "propertyId": "-10000",
        "propertyName": "分类",
        "tbCatId": tb_cat_id,
        "text": cat_name,
    })];

    let selection_map = prop_selections.as_object().cloned().unwrap_or_default();
    let mut seen_property_ids = std::collections::HashSet::new();

    for card in card_list {
        let card_object = card.as_object().cloned().unwrap_or_default();
        let property_id = value_to_string_lossy(card_object.get("propertyId"));
        if property_id.is_empty()
            || property_id == "-10000"
            || !seen_property_ids.insert(property_id.clone())
        {
            continue;
        }
        let selected_value_id: String = selection_map
            .get(&property_id)
            .map(|value| value_to_string_lossy(Some(value)))
            .unwrap_or_default();
        if selected_value_id.trim().is_empty() {
            continue;
        }

        let property_name = value_to_string_lossy(card_object.get("propertyName"));
        let selected_value = card_object
            .get("valuesList")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|value| {
                let value_id = value_to_string_lossy(
                    value
                        .get("valueId")
                        .or_else(|| value.get("channelCatId"))
                        .or_else(|| value.get("catId")),
                );
                !value_id.is_empty() && value_id == selected_value_id
            });

        let value_name = selected_value
            .map(|value| {
                value_to_string_lossy(
                    value
                        .get("valueName")
                        .or_else(|| value.get("catName"))
                        .or_else(|| value.get("text"))
                        .or_else(|| value.get("channelCatName")),
                )
            })
            .unwrap_or_default();
        if value_name.is_empty() {
            continue;
        }

        labels.push(json!({
            "channelCateId": channel_cat_id,
            "channelCateName": value_name.clone(),
            "from": "newPublishChoice",
            "isUserClick": "1",
            "labelFrom": "newPublish",
            "labelType": "common",
            "properties": format!("{property_id}##{property_name}:{selected_value_id}##{value_name}"),
            "propertyId": property_id,
            "propertyName": property_name,
            "tbCatId": tb_cat_id,
            "text": value_name,
        }));
    }

    labels
}

pub(super) fn build_wx_publish_payload(
    payload: &ItemPublishPayload,
    cat_info: Option<&Value>,
    location: &LocationContext,
) -> Value {
    let simple_item = payload.simple_item || infer_simple_item_from_category(cat_info);
    let addr_latitude = if payload.latitude.trim().is_empty() {
        location.latitude.clone()
    } else {
        payload.latitude.trim().to_string()
    };
    let addr_longitude = if payload.longitude.trim().is_empty() {
        location.longitude.clone()
    } else {
        payload.longitude.trim().to_string()
    };
    let gps = if addr_latitude.is_empty() || addr_longitude.is_empty() {
        location.gps.clone()
    } else {
        format!("{addr_latitude},{addr_longitude}")
    };
    let division_id = payload
        .division_id
        .trim()
        .parse::<i64>()
        .ok()
        .or_else(|| location.fish_ad_code.trim().parse::<i64>().ok())
        .unwrap_or_default();
    let cat = cat_info
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let price_cent: i64 = price_to_cents(payload.price.trim())
        .parse()
        .unwrap_or_default();
    let mut price_dto = json!({ "priceInCent": price_cent });
    if !payload.orig_price.trim().is_empty() {
        let orig_cent: i64 = price_to_cents(payload.orig_price.trim())
            .parse()
            .unwrap_or_default();
        if orig_cent > 0 && orig_cent != price_cent {
            price_dto["origPriceInCent"] = json!(orig_cent);
        }
    }
    price_dto["noBargain"] = json!(payload.no_bargain);

    let mut addr_dto = json!({
        "area": if payload.area.trim().is_empty() { payload.city.trim() } else { payload.area.trim() },
        "city": payload.city.trim(),
        "divisionId": division_id,
        "gps": gps,
        "prov": payload.province.trim(),
    });
    if !addr_latitude.is_empty() {
        addr_dto["latitude"] = json!(addr_latitude);
    }
    if !addr_longitude.is_empty() {
        addr_dto["longitude"] = json!(addr_longitude);
    }

    json!({
        "utdid": "28b8IGorElQCAQAAAACuy6pp",
        "platform": "ios",
        "miniAppVersion": "0.3.77",
        "uniqueCode": unix_millis_now(),
        "sourceId": "main_publish_item",
        "publishScene": "wxMainPublish",
        "itemTypeStr": "b",
        "simpleItem": simple_item,
        "defaultPrice": false,
        "imageInfoDOList": build_image_info_list(&payload.image_urls, true),
        "itemTextDTO": {
            "title": payload.title.trim(),
            "desc": payload.desc.trim(),
        },
        "itemPriceDTO": price_dto,
        "itemPostFeeDTO": build_post_fee_dto(
            Some(payload.shipping_type.as_str()),
            Some(payload.can_free_shipping),
            payload.post_price.as_str(),
            payload.freight_template_id.as_str(),
            true,
        ),
        "itemAddrDTO": addr_dto,
        "itemCatDTO": {
            "catId": cat.get("catId").and_then(Value::as_str).unwrap_or_default(),
            "tbCatId": cat.get("tbCatId").and_then(Value::as_str).unwrap_or_default(),
            "channelCatId": cat.get("channelCatId").and_then(Value::as_str).unwrap_or_default(),
            "catName": cat.get("catName").or_else(|| cat.get("channelCatName")).and_then(Value::as_str).unwrap_or_default(),
            "leafId": cat.get("leafId").and_then(Value::as_str).unwrap_or_default(),
        },
        "itemLabelExtList": build_publish_labels_from_selection(
            cat_info,
            &payload.card_list,
            &payload.prop_selections,
        ),
    })
}

fn publish_capability_item_info(item_id: &str, title: &str, location: &LocationContext) -> Value {
    json!({
        "simpleItem": "true",
        "bizcode": "mainPublish",
        "bucketId": "16",
        "scene": "mainPublish",
        "topics": [],
        "imageInfoDOList": [],
        "itemTextDTO": {
            "title": title,
            "desc": title,
            "shortTitle": "",
            "shortTitleHighKeyword": [],
        },
        "itemCatDTO": {},
        "itemLabelExtList": [],
        "itemPriceDTO": {},
        "quantity": "1",
        "itemPostFeeDTO": { "canFreeShipping": true, "supportFreight": true },
        "itemAddrDTO": {
            "divisionId": location.fish_ad_code.clone(),
            "prov": location.province.clone(),
            "city": location.city.clone(),
            "area": location.area.clone(),
            "gps": location.gps.clone(),
        },
        "itemStatus": "0",
        "sourceId": "main_publish_item",
        "publishScene": "mainPublish",
        "itemId": if item_id.is_empty() { Value::Null } else { json!(item_id) },
        "uniqueCode": unix_millis_now().to_string(),
    })
}

fn publish_capability_photo_input(item_info: &Value) -> Value {
    json!({
        "itemCatDTO": item_info.get("itemCatDTO").cloned().unwrap_or_else(|| json!({})),
        "itemLabelExtList": item_info
            .get("itemLabelExtList")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    })
}

async fn h5_capability_result(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
) -> Value {
    match upstream_h5_request(state, uid, api, version, data, Vec::new()).await {
        Ok(payload) => {
            state.record_mtop_response(api, &payload);
            json!({
                "api": api,
                "version": version,
                "ok": payload_ret_success(&payload),
                "ret": first_ret_string(&payload),
                "data": payload.get("data").cloned().unwrap_or(Value::Null),
            })
        }
        Err(error) => json!({
            "api": api,
            "version": version,
            "ok": false,
            "ret": "",
            "error": error,
            "data": Value::Null,
        }),
    }
}

fn live_message_extra_params() -> Vec<(String, String)> {
    vec![
        ("accountSite".to_string(), "xianyu".to_string()),
        ("sessionOption".to_string(), "AutoLoginOnly".to_string()),
        ("spm_cnt".to_string(), "a21ybx.im.0.0".to_string()),
        ("spm_pre".to_string(), "a21ybx.home.sidebar.1".to_string()),
        (
            "dangerouslySetWindvaneParams".to_string(),
            "%5Bobject%20Object%5D".to_string(),
        ),
        ("smToken".to_string(), "token".to_string()),
        ("queryToken".to_string(), "sm".to_string()),
        ("sm".to_string(), "sm".to_string()),
    ]
}

fn generated_device_id(uid: &str) -> String {
    format!("rust-{}-{}", uid, unix_millis_now())
}

// ─── Handlers ────────────────────────────────────────────────────────────────

pub(super) async fn item_publish(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemPublishPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let mut normalized_payload = payload;
    normalized_payload.title =
        derive_publish_title_from_payload(&normalized_payload.title, &normalized_payload.desc);
    let location = load_location_or_default(&state, &normalized_payload.uid).await;
    let kgraph = successful_upstream_h5_request(
        &state,
        &normalized_payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.kgraph.property.recommend"),
        "2.0",
        build_kgraph_payload(
            &KgraphPayload {
                uid: normalized_payload.uid.clone(),
                title: normalized_payload.title.clone(),
                desc: normalized_payload.desc.clone(),
                cat_id: String::new(),
                channel_cat_id: String::new(),
                current_card_list: Vec::new(),
                selected_list: Vec::new(),
                image_urls: normalized_payload.image_urls.clone(),
            },
            &location,
        ),
        Vec::new(),
    )
    .await
    .ok();
    let cat_info = kgraph.as_ref().and_then(extract_kgraph_cat_info);
    let wx_payload = build_wx_publish_payload(&normalized_payload, cat_info.as_ref(), &location);
    debug!(
        target: "market.publish",
        stream = "runtime",
        module = "market-publish",
        action = "wx_publish_request",
        outcome = "success",
        account_uid = %normalized_payload.uid,
        title_len = normalized_payload.title.chars().count(),
        image_count = normalized_payload.image_urls.len(),
        kgraph_ok = cat_info.is_some(),
        payload_summary = %json_log_summary_string(&wx_payload),
        "prepared wx publish payload"
    );
    match successful_upstream_h5_request(
        &state,
        &normalized_payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.wx.idleitem.publish"),
        "1.0",
        wx_payload,
        Vec::new(),
    )
    .await
    {
        Ok(upstream) => {
            debug!(
                target: "market.publish",
                stream = "runtime",
                module = "market-publish",
                action = "wx_publish_response",
                outcome = "success",
                account_uid = %normalized_payload.uid,
                ret = ?upstream.get("ret"),
                "wx publish succeeded"
            );
            return Ok(ResponseJson(upstream));
        }
        Err(e) => {
            warn!(
                target: "market.publish",
                stream = "runtime",
                module = "market-publish",
                action = "wx_publish_response",
                outcome = "warn",
                account_uid = %normalized_payload.uid,
                error_code = "wx_publish_failed",
                retryable = true,
                detail = %e,
                "wx publish failed"
            );
            Err(ResponseError::internal_message(e))
        }
    }
}

pub(super) async fn publish_preget(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    if let Ok(payload) = successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.publish.preget"),
        "3.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "inputJson": "{\"sourceId\":\"main_publish_item\",\"isOldFriendly\":\"false\"}",
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "categories": ["数码", "家具", "家电"],
            "draftEnabled": true,
        }
    })))
}

pub(super) async fn publish_check(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    if let Ok(payload) = successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idleitem.prepublish.check"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "canPublish": true,
            "message": "可发布",
        }
    })))
}

pub(super) async fn publish_badwords(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<BadwordsQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    let title = query.title.clone();
    if let Ok(payload) = successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idleitem.badwords.prepubcheck"),
        "2.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "title": title.clone(),
            "tokens": title.clone(),
            "scene": "publish",
            "sessionId": "check001",
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(payload));
    }
    let matches = ["微信", "加V", "返现"]
        .into_iter()
        .filter(|word| title.contains(word))
        .collect::<Vec<_>>();
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "matched": matches,
            "safe": matches.is_empty(),
        }
    })))
}

pub(super) async fn publish_capabilities(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<PublishCapabilitiesQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    let title = if query.title.trim().is_empty() {
        "测试商品".to_string()
    } else {
        query.title.trim().to_string()
    };
    let item_id = query.item_id.trim();
    let publish_item_info = publish_capability_item_info(item_id, &title, &location);
    let publish_item_info_text =
        serde_json::to_string(&publish_item_info).unwrap_or_else(|_| "{}".to_string());
    let photo_input = publish_capability_photo_input(&publish_item_info);
    let photo_input_text = serde_json::to_string(&photo_input).unwrap_or_else(|_| "{}".to_string());

    let mut results = Map::new();
    for (key, api, version, data) in [
        (
            "preget",
            crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.publish.preget"),
            "3.0",
            json!({
                "fishAdCode": location.fish_ad_code.clone(),
                "gps": location.gps.clone(),
                "inputJson": "{\"sourceId\":\"main_publish_item\",\"isOldFriendly\":\"false\"}",
                "latitude": location.latitude.clone(),
                "longitude": location.longitude.clone(),
            }),
        ),
        (
            "photoTemplate",
            crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.publish.photo.template"),
            "1.0",
            json!({
                "fishAdCode": location.fish_ad_code.clone(),
                "gps": location.gps.clone(),
                "inputJson": photo_input_text,
                "latitude": location.latitude.clone(),
                "longitude": location.longitude.clone(),
            }),
        ),
        (
            "serviceCards",
            crate::sealed_protocol::sealed_str!("mtop.idle.item.publish.service.cards.list"),
            "1.0",
            json!({
                "cpvList": "[]",
                "fishAdCode": location.fish_ad_code.clone(),
                "gps": location.gps.clone(),
                "itemInfoJson": publish_item_info_text,
                "latitude": location.latitude.clone(),
                "longitude": location.longitude.clone(),
                "param": "{\"multiSkuEditingMode\":\"false\",\"settingsPreferences\":null,\"supportDefaultOpen\":true}",
            }),
        ),
        (
            "groupSelect",
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.group.item.publish.select.list"),
            "1.0",
            json!({ "page": 1 }),
        ),
        (
            "postGet",
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.publish.postGet"),
            "1.0",
            publish_capability_post_get_payload(item_id),
        ),
        (
            "postget",
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.publish.postget"),
            "1.0",
            publish_capability_postget_payload(item_id, &location),
        ),
        (
            "prepublishCheck",
            crate::sealed_protocol::sealed_str!("mtop.taobao.idleitem.prepublish.check"),
            "1.0",
            json!({
                "fishAdCode": location.fish_ad_code.clone(),
                "gps": location.gps.clone(),
            }),
        ),
        (
            "badwords",
            crate::sealed_protocol::sealed_str!("mtop.taobao.idleitem.badwords.prepubcheck"),
            "2.0",
            json!({
                "fishAdCode": location.fish_ad_code.clone(),
                "gps": location.gps.clone(),
                "title": title.clone(),
                "tokens": title.clone(),
                "scene": "publish",
                "sessionId": "check001",
            }),
        ),
    ] {
        let result = h5_capability_result(&state, &query.uid, api, version, data).await;
        results.insert(key.to_string(), result);
    }
    let success_count = results
        .values()
        .filter(|value| value.get("ok").and_then(Value::as_bool).unwrap_or(false))
        .count();
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "uid": query.uid,
            "successCount": success_count,
            "totalCount": results.len(),
            "results": results,
        }
    })))
}

pub(super) async fn message_token(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idlemessage.pc.login.token"),
        "1.0",
        json!({
            "appKey": "444e9908a51d1cb236a27862abc769c9",
            "deviceId": generated_device_id(&query.uid),
        }),
        live_message_extra_params(),
    )
    .await
    {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "uid": query.uid,
            "token": format!("simulated-im-token-{}", unix_millis_now()),
            "expiresIn": 3600,
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::{build_wx_publish_payload, derive_publish_title_from_payload, ItemPublishPayload};
    use crate::xianyu_client::LocationContext;
    use serde_json::{json, Value};

    #[test]
    fn derive_publish_title_prefers_explicit_title() {
        assert_eq!(
            derive_publish_title_from_payload("已填标题", "二手手机，成色不错"),
            "已填标题"
        );
    }

    #[test]
    fn derive_publish_title_falls_back_to_description() {
        assert_eq!(
            derive_publish_title_from_payload("", "自用 iPhone 13 128G 蓝色，电池 89%，功能正常。"),
            "自用 iPhone 13 128G 蓝色"
        );
    }

    #[test]
    fn build_wx_publish_payload_marks_service_category_and_no_bargain() {
        let payload = ItemPublishPayload {
            uid: "u1".to_string(),
            title: "远程装机".to_string(),
            short_title: String::new(),
            desc: "提供远程装机服务".to_string(),
            price: "12".to_string(),
            orig_price: String::new(),
            image_urls: vec![],
            simple_item: false,
            no_bargain: true,
            can_free_shipping: true,
            shipping_type: "free".to_string(),
            post_price: String::new(),
            freight_template_id: String::new(),
            quantity: 1,
            card_list: Vec::new(),
            prop_selections: Value::Null,
            city: "北京".to_string(),
            province: "北京".to_string(),
            area: "海淀".to_string(),
            division_id: "110108".to_string(),
            latitude: String::new(),
            longitude: String::new(),
            fish_ad_code: String::new(),
        };
        let location = LocationContext::default();
        let result = build_wx_publish_payload(
            &payload,
            Some(
                &json!({"catName": "软件开发", "channelCatId": "201449620", "tbCatId": "50003316"}),
            ),
            &location,
        );

        assert_eq!(result["simpleItem"], true);
        assert_eq!(result["itemPriceDTO"]["noBargain"], true);
    }

    #[test]
    fn build_wx_publish_payload_includes_selected_property_labels() {
        let payload = ItemPublishPayload {
            uid: "u1".to_string(),
            title: "iPhone 13".to_string(),
            short_title: String::new(),
            desc: "128G 蓝色".to_string(),
            price: "2999".to_string(),
            orig_price: String::new(),
            image_urls: vec![],
            simple_item: false,
            no_bargain: false,
            can_free_shipping: true,
            shipping_type: "free".to_string(),
            post_price: String::new(),
            freight_template_id: String::new(),
            quantity: 1,
            card_list: vec![json!({
                "propertyId": "2001",
                "propertyName": "成色",
                "valuesList": [
                    { "valueId": "1", "valueName": "全新" },
                    { "valueId": "2", "valueName": "95新" }
                ]
            })],
            prop_selections: json!({
                "2001": "2"
            }),
            city: "北京".to_string(),
            province: "北京".to_string(),
            area: "海淀".to_string(),
            division_id: "110108".to_string(),
            latitude: String::new(),
            longitude: String::new(),
            fish_ad_code: String::new(),
        };
        let location = LocationContext::default();
        let result = build_wx_publish_payload(
            &payload,
            Some(&json!({"catName": "手机", "channelCatId": "5001", "tbCatId": "1512"})),
            &location,
        );

        let labels = result["itemLabelExtList"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[1]["propertyId"], "2001");
        assert_eq!(labels[1]["text"], "95新");
    }

    #[test]
    fn build_wx_publish_payload_falls_back_to_account_location_division() {
        let payload = ItemPublishPayload {
            uid: "u1".to_string(),
            title: "iPhone 13".to_string(),
            short_title: String::new(),
            desc: "128G 蓝色".to_string(),
            price: "2999".to_string(),
            orig_price: String::new(),
            image_urls: vec![],
            simple_item: false,
            no_bargain: false,
            can_free_shipping: true,
            shipping_type: "free".to_string(),
            post_price: String::new(),
            freight_template_id: String::new(),
            quantity: 1,
            card_list: Vec::new(),
            prop_selections: Value::Null,
            city: String::new(),
            province: String::new(),
            area: String::new(),
            division_id: String::new(),
            latitude: String::new(),
            longitude: String::new(),
            fish_ad_code: String::new(),
        };
        let location = LocationContext {
            fish_ad_code: "430104".to_string(),
            gps: "28.231,112.938".to_string(),
            latitude: "28.231".to_string(),
            longitude: "112.938".to_string(),
            name: String::new(),
            address: String::new(),
            city: "长沙市".to_string(),
            province: "湖南省".to_string(),
            area: "岳麓区".to_string(),
        };

        let result = build_wx_publish_payload(
            &payload,
            Some(&json!({"catName": "手机", "channelCatId": "5001", "tbCatId": "1512"})),
            &location,
        );

        assert_eq!(result["itemAddrDTO"]["divisionId"], 430104);
    }
}
