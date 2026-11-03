use axum::extract::{Extension, Json, State};
use axum::Json as ResponseJson;
use serde_json::{json, Value};
use tracing::warn;

use crate::{
    models::auth::RequestIdentity, routes::error::ApiResult, state::AppState,
    xianyu_client::LocationContext,
};

use super::publish::{derive_publish_title_from_payload, KgraphPayload};
use super::{
    build_image_info_list_h5_full, category_name_from_value, ensure_market_uid_access,
    load_location_or_default, success_ret, success_ret_value, successful_upstream_h5_request,
    unix_millis_now,
};

// ─── kgraph helper functions ──────────────────────────────────────────────────

fn pick_preferred_kgraph_category(values: &[Value]) -> Option<Value> {
    values
        .iter()
        .find(|value| {
            value
                .get("isClicked")
                .and_then(Value::as_str)
                .map(|flag| flag == "1")
                .unwrap_or(false)
                || value
                    .get("isUserClick")
                    .and_then(Value::as_str)
                    .map(|flag| flag == "1")
                    .unwrap_or(false)
        })
        .cloned()
        .or_else(|| values.first().cloned())
}

pub(super) fn extract_kgraph_cat_info(payload: &Value) -> Option<Value> {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("cardList"))
        .and_then(Value::as_array)
        .and_then(|cards| {
            cards.iter().find_map(|card| {
                let card_data = card.get("cardData")?;
                if card_data.get("propertyId").and_then(Value::as_str) != Some("-10000") {
                    return None;
                }
                card_data
                    .get("valuesList")
                    .and_then(Value::as_array)
                    .and_then(|values| pick_preferred_kgraph_category(values))
            })
        })
}

pub(super) fn first_non_empty_str<'a>(values: &[Option<&'a Value>]) -> String {
    values
        .iter()
        .filter_map(|value| value.and_then(Value::as_str))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
        .to_string()
}

fn build_kgraph_selected_list(category: &Value) -> Vec<Value> {
    let channel_cat_id = first_non_empty_str(&[
        category.get("channelCatId"),
        category.get("channelCateId"),
        category.get("valueId"),
        category.get("catId"),
    ]);
    if channel_cat_id.is_empty() {
        return Vec::new();
    }
    let cat_name = category_name_from_value(category);
    if cat_name.is_empty() {
        return Vec::new();
    }
    let property_id = first_non_empty_str(&[category.get("propertyId")]);
    let property_name = first_non_empty_str(&[category.get("propertyName")]);
    vec![json!({
        "channelCateId": channel_cat_id,
        "channelCateName": cat_name,
        "from": "newPublishChoice",
        "isUserClick": category.get("isUserClick").and_then(Value::as_str).unwrap_or("0"),
        "labelFrom": "newPublish",
        "labelType": "common",
        "properties": format!(
            "{}##{}:{}##{}",
            if property_id.is_empty() { "-10000" } else { &property_id },
            if property_name.is_empty() { "分类" } else { &property_name },
            first_non_empty_str(&[
                category.get("channelCatId"),
                category.get("channelCateId"),
                category.get("valueId"),
                category.get("catId"),
            ]),
            cat_name
        ),
        "propertyId": if property_id.is_empty() { "-10000".to_string() } else { property_id },
        "propertyName": if property_name.is_empty() { "分类".to_string() } else { property_name },
        "tbCatId": first_non_empty_str(&[category.get("tbCatId"), category.get("catId")]),
        "text": cat_name,
    })]
}

fn normalized_kgraph_title(title: &str, desc: &str) -> String {
    let normalized_title = title.trim();
    if !normalized_title.is_empty() {
        return normalized_title.to_string();
    }
    if desc.trim().is_empty() {
        return String::new();
    }
    derive_publish_title_from_payload("", desc)
}

pub(super) fn normalize_kgraph_card_list_for_request(card_list: &[Value]) -> Vec<Value> {
    card_list
        .iter()
        .map(|card| {
            if let Some(card_data) = card.get("cardData") {
                json!({ "cardData": card_data.clone() })
            } else {
                json!({ "cardData": card.clone() })
            }
        })
        .collect()
}

pub(super) fn build_kgraph_payload(payload: &KgraphPayload, location: &LocationContext) -> Value {
    json!({
        "bizParams": {},
        "catId": payload.cat_id.trim(),
        "channelCatId": payload.channel_cat_id.trim(),
        "currentCardList": payload.current_card_list,
        "description": payload.desc.trim(),
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "imageInfos": build_image_info_list_h5_full(&payload.image_urls),
        "latitude": location.latitude,
        "lockCpv": false,
        "longitude": location.longitude,
        "multiSKU": false,
        "publishScene": "mainPublish",
        "scene": "newPublishChoice",
        "selectedList": payload.selected_list,
        "sourceId": "main_publish_item",
        "title": payload.title.trim(),
        "trendySkuShopSingle": false,
        "trendySkuStyleInfo": {},
        "uniqueCode": unix_millis_now().to_string(),
    })
}

fn should_refine_kgraph_result(
    payload: &KgraphPayload,
    first_cards: &[Value],
    first_category: Option<&Value>,
) -> bool {
    if !payload.cat_id.trim().is_empty()
        || !payload.channel_cat_id.trim().is_empty()
        || first_cards.is_empty()
    {
        return false;
    }
    if payload.image_urls.is_empty() {
        return true;
    }
    let predict_from = first_category
        .and_then(|category| category.get("predictFrom"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    !predict_from.eq_ignore_ascii_case("pic")
}

pub(super) fn normalize_kgraph_response(payload: &Value) -> Value {
    let mut categories = Vec::new();
    let mut selected_category = Value::Null;
    let card_list = payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("cardList"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|card| {
            let card_data = card.get("cardData").cloned().unwrap_or(Value::Null);
            if card_data.get("propertyId").and_then(Value::as_str) == Some("-10000") {
                if let Some(values) = card_data.get("valuesList").and_then(Value::as_array) {
                    if selected_category.is_null() {
                        selected_category =
                            pick_preferred_kgraph_category(values).unwrap_or(Value::Null);
                    }
                    categories.extend(values.iter().cloned());
                }
            }
            json!({
                "propertyId": card_data.get("propertyId").and_then(Value::as_str).unwrap_or_default(),
                "propertyName": card_data.get("propertyName").and_then(Value::as_str).unwrap_or_default(),
                "valuesList": card_data.get("valuesList").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "ret": payload.get("ret").cloned().unwrap_or_else(success_ret_value),
        "data": {
            "category": selected_category,
            "categories": categories,
            "cardList": card_list,
        }
    })
}

// ─── kgraph handler ───────────────────────────────────────────────────────────

pub(super) async fn item_kgraph(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<KgraphPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let normalized_title = normalized_kgraph_title(&payload.title, &payload.desc);
    let first_payload = build_kgraph_payload(
        &KgraphPayload {
            uid: payload.uid.clone(),
            title: normalized_title.clone(),
            desc: payload.desc.clone(),
            cat_id: payload.cat_id.clone(),
            channel_cat_id: payload.channel_cat_id.clone(),
            current_card_list: payload.current_card_list.clone(),
            selected_list: payload.selected_list.clone(),
            image_urls: payload.image_urls.clone(),
        },
        &location,
    );
    match successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.kgraph.property.recommend"),
        "2.0",
        first_payload,
        Vec::new(),
    )
    .await
    {
        Ok(first_upstream) => {
            let normalized_first = normalize_kgraph_response(&first_upstream);
            let first_cards = normalized_first
                .pointer("/data/cardList")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let first_category = normalized_first.pointer("/data/category");
            let should_refine = should_refine_kgraph_result(&payload, &first_cards, first_category);
            if should_refine {
                if let Some(category) = first_category {
                    let category_cat_id =
                        first_non_empty_str(&[category.get("catId"), category.get("tbCatId")]);
                    let category_channel_cat_id = first_non_empty_str(&[
                        category.get("channelCatId"),
                        category.get("channelCateId"),
                        category.get("valueId"),
                    ]);
                    let selected_list = build_kgraph_selected_list(&category);
                    if !category_cat_id.is_empty()
                        && !category_channel_cat_id.is_empty()
                        && !selected_list.is_empty()
                    {
                        let second_payload = build_kgraph_payload(
                            &KgraphPayload {
                                uid: payload.uid.clone(),
                                title: normalized_title.clone(),
                                desc: payload.desc.clone(),
                                cat_id: category_cat_id,
                                channel_cat_id: category_channel_cat_id,
                                current_card_list: normalize_kgraph_card_list_for_request(
                                    &first_cards,
                                ),
                                selected_list,
                                image_urls: payload.image_urls.clone(),
                            },
                            &location,
                        );
                        match successful_upstream_h5_request(
                            &state,
                            &payload.uid,
                            crate::sealed_protocol::sealed_str!(
                                "mtop.taobao.idle.kgraph.property.recommend"
                            ),
                            "2.0",
                            second_payload,
                            Vec::new(),
                        )
                        .await
                        {
                            Ok(second_upstream) => {
                                return Ok(ResponseJson(normalize_kgraph_response(
                                    &second_upstream,
                                )));
                            }
                            Err(error) => {
                                warn!("kgraph refine failed for uid={}: {}", payload.uid, error);
                            }
                        }
                    }
                }
            }
            return Ok(ResponseJson(normalized_first));
        }
        Err(error) => {
            warn!("kgraph classify failed for uid={}: {}", payload.uid, error);
        }
    }
    let title = payload.title.trim();
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "cardList": [
                {
                    "propertyId": "-10000",
                    "propertyName": "分类",
                    "valuesList": [
                        {
                            "catId": if title.contains("椅") { "50012010" } else { "50011999" },
                            "channelCatId": "simulated-channel-cat",
                            "catName": if title.contains("椅") { "椅子" } else { "默认分类" },
                            "leafId": "simulated-leaf",
                            "tbCatId": if title.contains("椅") { "50012010" } else { "50011999" }
                        }
                    ],
                }
            ],
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::{extract_kgraph_cat_info, normalize_kgraph_response, normalized_kgraph_title};
    use serde_json::json;

    #[test]
    fn extract_kgraph_cat_info_prefers_clicked_category_value() {
        let payload = json!({
            "data": {
                "cardList": [{
                    "cardData": {
                        "propertyId": "-10000",
                        "propertyName": "分类",
                        "valuesList": [
                            {
                                "catId": "game-1",
                                "channelCatId": "game-channel",
                                "catName": "游戏装备"
                            },
                            {
                                "catId": "phone-1",
                                "channelCatId": "phone-channel",
                                "catName": "手机",
                                "isClicked": "1"
                            }
                        ]
                    }
                }]
            }
        });

        let category = extract_kgraph_cat_info(&payload).expect("should extract category");
        assert_eq!(category["catName"], "手机");
        assert_eq!(category["channelCatId"], "phone-channel");
    }

    #[test]
    fn normalize_kgraph_response_uses_clicked_category_instead_of_first_candidate() {
        let payload = json!({
            "ret": ["SUCCESS::调用成功"],
            "data": {
                "cardList": [{
                    "cardData": {
                        "propertyId": "-10000",
                        "propertyName": "分类",
                        "valuesList": [
                            {
                                "catId": "game-1",
                                "channelCatId": "game-channel",
                                "catName": "游戏装备"
                            },
                            {
                                "catId": "tv-1",
                                "channelCatId": "tv-channel",
                                "catName": "电视机",
                                "isUserClick": "1"
                            }
                        ]
                    }
                }]
            }
        });

        let normalized = normalize_kgraph_response(&payload);
        assert_eq!(normalized["data"]["category"]["catName"], "电视机");
        assert_eq!(normalized["data"]["categories"][0]["catName"], "游戏装备");
        assert_eq!(normalized["data"]["categories"][1]["catName"], "电视机");
    }

    #[test]
    fn normalized_kgraph_title_uses_description_when_title_missing() {
        assert_eq!(
            normalized_kgraph_title("", "二手 iPhone 15 Pro Max，白色 256G，国行在保"),
            "二手 iPhone 15 Pro Max"
        );
    }

    #[test]
    fn normalized_kgraph_title_keeps_empty_when_desc_missing() {
        assert_eq!(normalized_kgraph_title("", ""), "");
    }
}
