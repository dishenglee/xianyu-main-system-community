use std::collections::HashSet;

use axum::{
    extract::{Extension, Json, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::shared::TransportKind,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client::H5RequestProfile,
};

use super::{
    bool_from_optional, build_image_info_list, build_post_fee_dto, cent_string_from_value,
    display_price_to_cent_string, ensure_item_belongs_to_uid, ensure_market_uid_access, fetch_item,
    image_urls_from_value, load_location_or_default, payload_ret_success, resolve_no_bargain,
    success_ret_value, successful_upstream_h5_request, successful_upstream_wx_request,
    unix_millis_now, value_is_trueish, value_to_string_lossy, ItemRefPayload,
};

async fn request_edit_transport(
    state: &AppState,
    uid: &str,
    transport: TransportKind,
    data: Value,
) -> Result<Value, String> {
    match transport {
        TransportKind::Wx => {
            successful_upstream_wx_request(
                state,
                uid,
                crate::sealed_protocol::sealed_str!("mtop.idle.wx.idleitem.edit"),
                "1.0",
                data,
            )
            .await
        }
        TransportKind::H5 => {
            successful_upstream_h5_request(
                state,
                uid,
                crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.edit"),
                "1.0",
                data,
                Vec::new(),
            )
            .await
        }
        TransportKind::App => Err("item_edit does not support app transport".to_string()),
    }
}

fn json_log_summary(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().take(24).cloned().collect::<Vec<_>>();
            keys.sort();
            let data_keys = map
                .get("data")
                .and_then(Value::as_object)
                .map(|data| {
                    let mut keys = data.keys().take(24).cloned().collect::<Vec<_>>();
                    keys.sort();
                    keys
                })
                .unwrap_or_default();
            json!({
                "kind": "object",
                "keyCount": map.len(),
                "keys": keys,
                "dataKeyCount": map.get("data").and_then(Value::as_object).map(|data| data.len()).unwrap_or(0),
                "dataKeys": data_keys,
            })
        }
        Value::Array(values) => json!({
            "kind": "array",
            "len": values.len(),
        }),
        Value::String(text) => json!({
            "kind": "string",
            "len": text.chars().count(),
        }),
        Value::Null => json!({ "kind": "null" }),
        Value::Bool(_) => json!({ "kind": "bool" }),
        Value::Number(_) => json!({ "kind": "number" }),
    }
}

fn json_log_summary_string(value: &Value) -> String {
    serde_json::to_string(&json_log_summary(value)).unwrap_or_else(|_| "{}".to_string())
}

// ---------------------------------------------------------------------------
// Struct (edit only — ItemOpenPayload lives in items_open.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct ItemEditPayload {
    pub(super) uid: String,
    pub(super) item_id: String,
    #[serde(default)]
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
    pub(super) no_bargain: Option<bool>,
    #[serde(default)]
    pub(super) can_free_shipping: Option<bool>,
    #[serde(default)]
    pub(super) shipping_type: Option<String>,
    #[serde(default)]
    pub(super) post_price: String,
    #[serde(default)]
    pub(super) freight_template_id: String,
    #[serde(default)]
    pub(super) quantity: Option<i64>,
    #[serde(default)]
    pub(super) image_urls: Option<Vec<String>>,
    #[serde(default)]
    pub(super) detail: Value,
    #[serde(default)]
    pub(super) card_list: Vec<Value>,
    #[serde(default)]
    pub(super) prop_selections: Value,
}

pub(crate) async fn run_item_edit_price_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    price: &str,
    orig_price: &str,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.to_string())?;
    let detail = load_edit_detail_for_action(state, uid, item_id).await?;
    let payload = ItemEditPayload {
        uid: uid.to_string(),
        item_id: item_id.to_string(),
        title: String::new(),
        short_title: String::new(),
        desc: String::new(),
        price: price.to_string(),
        orig_price: orig_price.to_string(),
        no_bargain: None,
        can_free_shipping: None,
        shipping_type: None,
        post_price: String::new(),
        freight_template_id: String::new(),
        quantity: None,
        image_urls: None,
        detail,
        card_list: Vec::new(),
        prop_selections: Value::Null,
    };
    run_item_edit_payload_action(state, &payload).await
}

pub(super) async fn load_edit_detail_for_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    if let Ok(detail) = fetch_pc_backend_edit_detail(state, uid, item_id).await {
        return Ok(detail);
    }

    if let Ok(detail) = fetch_wx_edit_detail(state, uid, item_id).await {
        return Ok(detail);
    }

    if let Ok(upstream) = successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
        "1.0",
        json!({ "itemId": item_id }),
        Vec::new(),
    )
    .await
    {
        return Ok(build_h5_detail_mapping(
            upstream.get("data").unwrap_or(&Value::Null),
            item_id,
        ));
    }

    let item = fetch_item(state.storage().xianyu(), uid, item_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "商品不存在".to_string())?;
    Ok(build_cached_edit_detail(&item, item_id))
}

pub(super) async fn fetch_pc_backend_edit_detail(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    let upstream = crate::xianyu_client::h5_request_with_profile(
        state.config(),
        state.storage().accounts(),
        uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.pc.backend.idleitem.editdetail"),
        "1.0",
        &json!({ "itemId": item_id }),
        vec![("spm_cnt".to_string(), "a21ybx.item".to_string())],
        H5RequestProfile::SellerCommonPro,
    )
    .await?;
    if !payload_ret_success(&upstream) {
        return Err(format!("pc editdetail failed: {upstream}"));
    }
    let data = upstream.get("data").cloned().unwrap_or(Value::Null);
    if data
        .as_object()
        .map(|object| object.is_empty())
        .unwrap_or(true)
    {
        return Err("pc editdetail returned empty data".to_string());
    }
    Ok(data)
}

pub(super) async fn fetch_wx_edit_detail(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    let upstream = crate::xianyu_client::wx_request(
        state.config(),
        state.storage().accounts(),
        uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.wx.idleitem.editdetail"),
        "1.0",
        &json!({ "itemId": item_id }),
    )
    .await?;
    if !payload_ret_success(&upstream) {
        return Err(format!("wx editdetail failed: {upstream}"));
    }
    let data = upstream.get("data").cloned().unwrap_or(Value::Null);
    if data
        .as_object()
        .map(|object| object.is_empty())
        .unwrap_or(true)
    {
        return Err("wx editdetail returned empty data".to_string());
    }
    Ok(data)
}

fn build_cached_edit_detail(item: &Value, item_id: &str) -> Value {
    let mut data = item.get("detail").cloned().unwrap_or_else(|| json!({}));
    if let Some(obj) = data.as_object_mut() {
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let desc = item
            .get("desc")
            .or_else(|| item.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let price = item
            .get("price")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !obj.contains_key("itemTextDTO") {
            obj.insert(
                "itemTextDTO".to_string(),
                json!({
                    "title": title,
                    "shortTitle": title.chars().take(20).collect::<String>(),
                    "desc": desc,
                }),
            );
        }
        if !obj.contains_key("itemPriceDTO") {
            let cents = (price.parse::<f64>().unwrap_or(0.0) * 100.0).round() as i64;
            obj.insert(
                "itemPriceDTO".to_string(),
                json!({
                    "priceInCent": cents.to_string(),
                    "origPriceInCent": "0",
                    "noBargain": false,
                }),
            );
        }
        if !obj.contains_key("imageInfoDOList")
            && !obj.contains_key(crate::sealed_protocol::sealed_str!("itemDO"))
        {
            let pic_url = item
                .get("picUrl")
                .or_else(|| item.get("pic_url"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !pic_url.is_empty() {
                obj.insert(
                    "imageInfoDOList".to_string(),
                    json!([{ "url": pic_url, "major": true, "type": 0 }]),
                );
            }
        }
        if !obj.contains_key("itemId") {
            obj.insert("itemId".to_string(), json!(item_id));
        }
    }
    data
}

// ---------------------------------------------------------------------------
// Item edit payload builders
// ---------------------------------------------------------------------------

fn build_h5_detail_mapping(raw: &Value, item_id: &str) -> Value {
    let item_do = raw
        .get(crate::sealed_protocol::sealed_str!("itemDO"))
        .cloned()
        .unwrap_or_else(|| raw.clone());
    let image_urls = image_urls_from_value(&item_do);
    let title = item_do
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let desc = item_do
        .get("desc")
        .or_else(|| item_do.get("description"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    // 透传上游原始 itemCatDTO（含 tbCatId/channelCatId/leafId/catName 等真权威字段），
    // 不再硬写空。pc.detail 实测下发：
    //   {catId, tbCatId, channelCatId, leafId, level2ChannelCatId, level3ChannelCatId, ...}
    let cat_dto = item_do.get("itemCatDTO").cloned().unwrap_or_else(|| {
        json!({
            "catId": value_to_string_lossy(item_do.get("categoryId")),
            "tbCatId": "",
            "channelCatId": "",
            "catName": "",
            "leafId": "",
        })
    });
    let cpv_labels = item_do
        .get("cpvLabels")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let quantity_v = item_do
        .get("quantity")
        .cloned()
        .map(|v| Value::String(value_to_string_lossy(Some(&v))))
        .filter(|s| s.as_str().map(|x| !x.is_empty()).unwrap_or(false))
        .unwrap_or_else(|| Value::String("1".to_string()));
    let simple_item_v = item_do
        .get("simpleItem")
        .cloned()
        .unwrap_or(Value::Bool(false));
    let no_bargain = resolve_no_bargain(item_do.get("noBargain"), item_do.get("bargain"), false);
    json!({
        "itemId": item_id,
        "itemTextDTO": {
            "title": title,
            "shortTitle": item_do.get("shortTitle").and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| title.chars().take(20).collect::<String>()),
            "desc": desc,
        },
        "itemPriceDTO": {
            "priceInCent": display_price_to_cent_string(item_do.get("soldPrice").or_else(|| item_do.get("price"))),
            "origPriceInCent": display_price_to_cent_string(item_do.get("originalPrice").or_else(|| item_do.get("origPrice"))),
            "noBargain": no_bargain,
        },
        "itemAddrDTO": {
            "divisionId": value_to_string_lossy(item_do.get("divisionId")),
            "city": item_do.get("city").and_then(Value::as_str).unwrap_or_default(),
            "prov": item_do.get("province").and_then(Value::as_str).unwrap_or_default(),
            "area": item_do.get("area").and_then(Value::as_str).unwrap_or_default(),
            "gps": item_do.get("gps").and_then(Value::as_str).unwrap_or_default(),
            "addressType": 5,
        },
        "itemCatDTO": cat_dto,
        "cpvLabels": cpv_labels,
        "itemLabelExtList": item_do.get("itemLabelExtList").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
        "itemPostFeeDTO": build_post_fee_dto(None, Some(true), "", "", false),
        "imageInfoDOList": build_image_info_list(&image_urls, false),
        "itemTypeStr": item_do.get("itemType").and_then(Value::as_str).unwrap_or("b"),
        "itemStatus": value_to_string_lossy(item_do.get("itemStatus")),
        "simpleItem": simple_item_v,
        "quantity": quantity_v,
        "editorVersion": "0.0.1",
    })
}

fn find_card_selected_value(
    card_list: &[Value],
    property_id: &str,
    selected_value_id: &str,
) -> Option<(String, String)> {
    if property_id.is_empty() || selected_value_id.is_empty() {
        return None;
    }
    card_list.iter().find_map(|card| {
        let card_property_id = value_to_string_lossy(card.get("propertyId"));
        if card_property_id != property_id {
            return None;
        }
        card.get("valuesList")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find_map(|value| {
                let value_id = value_to_string_lossy(
                    value
                        .get("valueId")
                        .or_else(|| value.get("channelCatId"))
                        .or_else(|| value.get("catId")),
                );
                if value_id != selected_value_id {
                    return None;
                }
                let value_name = value_to_string_lossy(
                    value
                        .get("valueName")
                        .or_else(|| value.get("catName"))
                        .or_else(|| value.get("text"))
                        .or_else(|| value.get("channelCatName")),
                );
                if value_name.is_empty() {
                    None
                } else {
                    Some((value_id, value_name))
                }
            })
    })
}

fn edit_label(
    channel_cat_id: &str,
    tb_cat_id: &str,
    property_id: &str,
    property_name: &str,
    value_id: &str,
    value_name: &str,
    is_user_click: &str,
) -> Value {
    json!({
        "channelCateId": channel_cat_id,
        "channelCateName": value_name,
        "from": "newPublishChoice",
        "isUserClick": is_user_click,
        "labelFrom": "newPublish",
        "labelType": "common",
        "properties": format!("{property_id}##{property_name}:{value_id}##{value_name}"),
        "propertyId": property_id,
        "propertyName": property_name,
        "tbCatId": tb_cat_id,
        "text": value_name,
    })
}

fn build_edit_label_list(
    payload: &ItemEditPayload,
    cat_dto_obj: &serde_json::Map<String, Value>,
    detail: &serde_json::Map<String, Value>,
) -> Vec<Value> {
    let channel_cat_id = value_to_string_lossy(
        cat_dto_obj
            .get("channelCatId")
            .or_else(|| cat_dto_obj.get("channelCat3Id")),
    );
    let tb_cat_id = value_to_string_lossy(cat_dto_obj.get("tbCatId"));
    let cat_name = value_to_string_lossy(
        cat_dto_obj
            .get("catName")
            .or_else(|| cat_dto_obj.get("channelCatName")),
    );
    let selections = payload
        .prop_selections
        .as_object()
        .cloned()
        .unwrap_or_default();
    let mut labels: Vec<Value> = Vec::new();
    let mut seen_property_ids: HashSet<String> = HashSet::new();

    let selected_category_id = selections
        .get("-10000")
        .map(|value| value_to_string_lossy(Some(value)))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| channel_cat_id.clone());
    let selected_category_name =
        find_card_selected_value(&payload.card_list, "-10000", selected_category_id.as_str())
            .map(|(_, name)| name)
            .filter(|value| !value.is_empty())
            .unwrap_or(cat_name);
    if !selected_category_id.is_empty() && !selected_category_name.is_empty() {
        labels.push(edit_label(
            selected_category_id.as_str(),
            tb_cat_id.as_str(),
            "-10000",
            "分类",
            selected_category_id.as_str(),
            selected_category_name.as_str(),
            "1",
        ));
        seen_property_ids.insert("-10000".to_string());
    }

    for card in &payload.card_list {
        let property_id = value_to_string_lossy(card.get("propertyId"));
        if property_id.is_empty()
            || property_id == "-10000"
            || seen_property_ids.contains(&property_id)
        {
            continue;
        }
        let selected_value_id = selections
            .get(&property_id)
            .map(|value| value_to_string_lossy(Some(value)))
            .unwrap_or_default();
        let Some((value_id, value_name)) =
            find_card_selected_value(&payload.card_list, &property_id, &selected_value_id)
        else {
            continue;
        };
        let property_name = value_to_string_lossy(card.get("propertyName"));
        if property_name.is_empty() {
            continue;
        }
        labels.push(edit_label(
            channel_cat_id.as_str(),
            tb_cat_id.as_str(),
            property_id.as_str(),
            property_name.as_str(),
            value_id.as_str(),
            value_name.as_str(),
            "1",
        ));
        seen_property_ids.insert(property_id);
    }

    for lbl in detail
        .get("cpvLabels")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let pid = value_to_string_lossy(lbl.get("propertyId"));
        let pname = value_to_string_lossy(lbl.get("propertyName"));
        let vid = value_to_string_lossy(lbl.get("valueId"));
        let vname = value_to_string_lossy(lbl.get("valueName"));
        if pid.is_empty()
            || pname.is_empty()
            || vid.is_empty()
            || vname.is_empty()
            || seen_property_ids.contains(&pid)
        {
            continue;
        }
        labels.push(edit_label(
            channel_cat_id.as_str(),
            tb_cat_id.as_str(),
            pid.as_str(),
            pname.as_str(),
            vid.as_str(),
            vname.as_str(),
            "1",
        ));
        seen_property_ids.insert(pid);
    }

    for raw in detail
        .get("itemLabelExtList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let pid = value_to_string_lossy(raw.get("propertyId"));
        if pid.is_empty() || seen_property_ids.contains(&pid) {
            continue;
        }
        labels.push(raw.clone());
        seen_property_ids.insert(pid);
    }

    labels
}

pub(super) fn build_h5_edit_payload(
    payload: &ItemEditPayload,
    fallback_location: &crate::xianyu_client::LocationContext,
) -> Value {
    // 严格按 Python 老项目 routers/items.py::_build_edit_payload 复刻字段。
    // 闲鱼对 mtop.idle.idleitem.edit 的入参做严格业务校验，缺任何字段都会返
    // FAIL_BIZ_IDLE_UNKNOWN_THROWABLE::发布失败，请稍后重试吧~。
    // 关键字段：uniqueCode / asyncSecurityInfo / baseParams / userRightsProtocols /
    //   itemGroupDTO / itemTopicParams / fishAdCode / gps / latitude / longitude /
    //   aiHostUsed / aigc / yhbItemInfoDTO / scene / bizcode / bucketId / 等。
    let detail = payload.detail.as_object().cloned().unwrap_or_default();
    let base_text = detail
        .get("itemTextDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let base_price = detail
        .get("itemPriceDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let base_addr = detail
        .get("itemAddrDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let image_urls = payload
        .image_urls
        .clone()
        .unwrap_or_else(|| image_urls_from_value(&Value::Object(detail.clone())));

    // 取最终的 title / desc / shortTitle
    let final_title = if payload.title.trim().is_empty() {
        base_text
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        payload.title.trim().to_string()
    };
    let final_short_title = if !payload.short_title.trim().is_empty() {
        payload
            .short_title
            .trim()
            .chars()
            .take(20)
            .collect::<String>()
    } else {
        let s = base_text
            .get("shortTitle")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let trimmed: String = s.chars().take(20).collect();
        if trimmed.is_empty() {
            final_title.chars().take(20).collect()
        } else {
            trimmed
        }
    };
    let final_desc = if payload.desc.trim().is_empty() {
        base_text
            .get("desc")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        payload.desc.trim().to_string()
    };

    // itemTextDTO：Python 版必带 richTextDesc/titleDescSeparate/shortTitleHighKeyword/trackParamsRichTextStyle
    let rich_text_desc = serde_json::to_string(&json!({
        "type": Value::Null,
        "children": [{"type":"Element","children":[{"type":"Text","text": final_desc.clone(),"properties":{}}],"properties":{}}],
        "properties": {},
        "version": "1.0",
    })).unwrap_or_else(|_| String::new());
    let mut text_dto = base_text.clone();
    text_dto.insert("title".to_string(), Value::String(final_title.clone()));
    text_dto.insert("shortTitle".to_string(), Value::String(final_short_title));
    text_dto.insert("desc".to_string(), Value::String(final_desc.clone()));
    text_dto.insert("richTextDesc".to_string(), Value::String(rich_text_desc));
    text_dto
        .entry("titleDescSeparate".to_string())
        .or_insert(Value::Bool(false));
    text_dto
        .entry("shortTitleHighKeyword".to_string())
        .or_insert(Value::Array(Vec::new()));
    text_dto
        .entry("trackParamsRichTextStyle".to_string())
        .or_insert(Value::String(String::new()));

    // itemPriceDTO
    let no_bargain = payload.no_bargain.unwrap_or_else(|| {
        resolve_no_bargain(base_price.get("noBargain"), detail.get("bargain"), false)
    });
    let mut price_dto = base_price.clone();
    let price_in_cent = if payload.price.trim().is_empty() {
        cent_string_from_value(base_price.get("priceInCent"))
    } else {
        super::price_to_cents(payload.price.trim())
    };
    let orig_price_in_cent = if payload.orig_price.trim().is_empty() {
        cent_string_from_value(base_price.get("origPriceInCent"))
    } else {
        super::price_to_cents(payload.orig_price.trim())
    };
    price_dto.insert("priceInCent".to_string(), Value::String(price_in_cent));
    price_dto.insert(
        "origPriceInCent".to_string(),
        Value::String(orig_price_in_cent),
    );
    price_dto.insert("noBargain".to_string(), Value::Bool(no_bargain));
    // Python 老版必带，缺会业务校验拒
    price_dto
        .entry("idleCoinDeductionPercent".to_string())
        .or_insert(Value::String("0".to_string()));

    // itemAddrDTO + 拆分出 fishAdCode/gps/lat/lng。
    // ⚠️ 闲鱼业务校验 FAIL_BIZ_ITEM_EDIT_INVALID_MAP_LOCATION：
    // 如果 itemAddrDTO 里 area/city/prov/divisionId/gps 全空，server 报 "系统无法定位行政区"，
    // 错误信息会被 wrap 成 FAIL_BIZ_IDLE_UNKNOWN_THROWABLE。
    // 编辑场景下 detail.itemAddrDTO 一般全空（pc.detail 不含位置），必须用兜底填充。
    let mut addr_dto = base_addr.clone();
    if let Some(at) = addr_dto.get("addressType").cloned() {
        if let Some(n) = at.as_str().and_then(|s| s.parse::<i64>().ok()) {
            addr_dto.insert("addressType".to_string(), Value::from(n));
        }
    }
    addr_dto
        .entry("addressType".to_string())
        .or_insert(Value::from(5));
    let raw_gps = base_addr
        .get("gps")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let gps_str = if raw_gps.is_empty() {
        // 之前是硬编码株洲 "27.867393,113.164539"。改成 fallback_location 来自
        // 账号的 publish_location 配置；如果账号没配置，落到空字符串，由上游
        // 报「系统无法定位行政区」给前端，比悄悄落到一个随机城市健康。
        fallback_location.gps.clone()
    } else {
        raw_gps
    };
    let (lat, lng) = if let Some((l, r)) = gps_str.split_once(',') {
        (l.to_string(), r.to_string())
    } else if !fallback_location.latitude.is_empty() {
        (
            fallback_location.latitude.clone(),
            fallback_location.longitude.clone(),
        )
    } else {
        (String::new(), String::new())
    };
    let raw_division = value_to_string_lossy(base_addr.get("divisionId"));
    let fish_ad_code = if raw_division.is_empty() {
        fallback_location.fish_ad_code.clone()
    } else {
        raw_division.clone()
    };
    // addr_dto 位置字段全空时用 fallback_location 兜底（之前是硬编码湖南郴州）。
    // fallback_location 来自账号的 publish_location 配置，账号没配置则字段为空，
    // 上游会拒，让用户去先配置发布地——比落到随机城市要明确。
    let addr_empty = base_addr
        .get("divisionId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
        && base_addr
            .get("city")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
        && base_addr
            .get("prov")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty();
    if addr_empty {
        addr_dto.insert(
            "divisionId".to_string(),
            Value::String(fish_ad_code.clone()),
        );
        addr_dto.insert(
            "prov".to_string(),
            Value::String(fallback_location.province.clone()),
        );
        addr_dto.insert(
            "city".to_string(),
            Value::String(fallback_location.city.clone()),
        );
        addr_dto.insert(
            "area".to_string(),
            Value::String(fallback_location.area.clone()),
        );
        addr_dto.insert("gps".to_string(), Value::String(gps_str.clone()));
    }

    let cat_dto_obj = detail
        .get("itemCatDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let label_list = build_edit_label_list(payload, &cat_dto_obj, &detail);

    // itemCatDTO：透传 + 重命名（Python 用 channelCat1Id/channelCat2Id/channelCat3Id 旧版字段名）
    let mut item_cat_out = serde_json::Map::new();
    item_cat_out.insert(
        "catId".to_string(),
        Value::String(value_to_string_lossy(cat_dto_obj.get("catId"))),
    );
    item_cat_out.insert(
        "tbCatId".to_string(),
        Value::String(value_to_string_lossy(cat_dto_obj.get("tbCatId"))),
    );
    item_cat_out.insert(
        "channelCatId".to_string(),
        Value::String(value_to_string_lossy(cat_dto_obj.get("channelCatId"))),
    );
    let l2 = value_to_string_lossy(
        cat_dto_obj
            .get("level2ChannelCatId")
            .or_else(|| cat_dto_obj.get("channelCat2Id")),
    );
    let l3 = value_to_string_lossy(
        cat_dto_obj
            .get("level3ChannelCatId")
            .or_else(|| cat_dto_obj.get("channelCat3Id")),
    );
    let l1 = value_to_string_lossy(
        cat_dto_obj
            .get("rootChannelCatId")
            .or_else(|| cat_dto_obj.get("channelCat1Id")),
    );
    if !l1.is_empty() {
        item_cat_out.insert("channelCat1Id".to_string(), Value::String(l1));
    }
    if !l2.is_empty() {
        item_cat_out.insert("channelCat2Id".to_string(), Value::String(l2));
    }
    if !l3.is_empty() {
        item_cat_out.insert("channelCat3Id".to_string(), Value::String(l3));
    }
    item_cat_out.insert(
        "catName".to_string(),
        cat_dto_obj
            .get("catName")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new())),
    );
    item_cat_out.insert(
        "leafId".to_string(),
        Value::String(value_to_string_lossy(cat_dto_obj.get("leafId"))),
    );

    // userRightsProtocols：item_edit 实际调 idleitem.publish，按 _build_publish_payload
    // 标准必带 4 条默认协议（enable=false）。Python publish 在 app.log 有大量 200 OK 成功记录，
    // 这是已被验证的格式。
    const DEFAULT_PROTOCOLS: &[&str] = &[
        "FAST_DELIVERY_48_HOUR",
        "FAST_DELIVERY_24_HOUR",
        "VIRTUAL_NONCONFORMITY_FREE_REFUND_SERVICE",
        "SKILL_PLAY_NO_MIND",
    ];
    let raw_protocols = detail
        .get("userRightsProtocols")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let protocols = if raw_protocols.is_empty() {
        DEFAULT_PROTOCOLS
            .iter()
            .map(|c| json!({"enable": false, "serviceCode": c}))
            .collect::<Vec<_>>()
    } else {
        raw_protocols
            .into_iter()
            .map(|p| {
                json!({
                    "serviceCode": value_to_string_lossy(p.get("serviceCode")),
                    "enable": p.get("enable").map(value_is_trueish).unwrap_or(false),
                })
            })
            .collect()
    };

    // simpleItem：item_edit 走 publish 通道，按 _build_publish_payload 标准用小写 "true"/"false"。
    // (edit API 用大写 "True"/"False" 是另一回事，但那条历史 0 成功不参考。)
    let simple_item_bool = match detail.get("simpleItem") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        Some(Value::Number(n)) => n.as_i64().map(|v| v != 0).unwrap_or(false),
        _ => true,
    };
    let simple_item = Value::String(if simple_item_bool {
        "true".to_string()
    } else {
        "false".to_string()
    });

    // uniqueCode：16 位时间戳（Python 用 int(time*1000) + "000"）
    let unique_code = format!(
        "{}000",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
    );

    let quantity_v = payload
        .quantity
        .map(|n| Value::String(n.to_string()))
        .unwrap_or_else(|| {
            detail
                .get("quantity")
                .cloned()
                .unwrap_or_else(|| Value::String("1".to_string()))
        });

    let editor_version = detail
        .get("editorVersion")
        .and_then(Value::as_str)
        .unwrap_or("0.0.1")
        .to_string();

    // 严格对齐 Python _build_publish_payload（item_edit 走 publish 通道，老版 200 OK 成功记录多）。
    // 关键差异：itemId 必带（不是 null），addressType=5（已有商品再发布）。
    json!({
        "aigc": "0",
        "aigcRequestId": "",
        "uniqueCode": unique_code,
        "trendySkuShopSingle": false,
        "yhbItemInfoDTO": {"idleAppraiseScene": "2", "useYhbService": false},
        "baseParams": {
            "bizcode": "mainPublish",
            "bucketId": "9",
            "rentMultiDayPackage": "1",
            "rentSmartPrice": "1",
            "scene": "ai_assist_desc_gen",
            "simpleItem": simple_item.clone(),
        },
        "bizLine": "",
        "bizRent": "{\"needConvert\":true}",
        "bizcode": "mainPublish",
        "brokerageAgree": false,
        "bucketId": "9",
        "editorVersion": editor_version,
        "enableRentThreeFreeOne": false,
        "aiHostUsed": false,
        "asyncSecurityInfo": {"securityStrategyHitResult": {"WARN": [], "FORBIDDEN": []}},
        "bizAuctionInfo": Value::Null,
        "bizFansPriceConfig": Value::Null,
        "bizFrom": Value::Null,
        "defaultPrice": false,
        "draftId": Value::Null,
        "fishAdCode": fish_ad_code,
        "freebies": false,
        "fromType": Value::Null,
        "gps": gps_str,
        "imageInfoDOList": build_image_info_list(&image_urls, false),
        "innerPublishType": Value::Null,
        "isRentalMode": false,
        "itemAddrDTO": Value::Object(addr_dto),
        "itemCatDTO": Value::Object(item_cat_out),
        "itemGroupDTO": {"groupId": "0"},
        "itemId": payload.item_id,
        "itemLabelExtList": label_list,
        "itemPostFeeDTO": build_post_fee_dto(
            payload.shipping_type.as_deref(),
            payload.can_free_shipping,
            payload.post_price.as_str(),
            payload.freight_template_id.as_str(),
            false,
        ),
        "itemPriceDTO": Value::Object(price_dto),
        "itemRentalMultiDayPackages": Value::Null,
        "itemTextDTO": Value::Object(text_dto),
        "itemTopicParams": detail.get("itemTopicParams").cloned().unwrap_or_else(|| json!({"topicInfos": []})),
        // publish 通道强制 "b"（Python _build_publish_payload 行 1541 硬编码）。
        // detail.itemTypeStr 可能是 "detailCommonBuy"/"detailService" 等，那是上游 editdetail
        // 返回的展示分类，不是 publish 接受的 type。
        "itemTypeStr": "b",
        "itemStatus": "0",
        "latitude": lat,
        "longitude": lng,
        "publishScene": "mainPublish",
        "publishSceneTab": Value::Null,
        "quantity": quantity_v,
        "rentMultiDayPackage": "1",
        "rentSmartPrice": "1",
        "scene": "ai_assist_desc_gen",
        "simpleItem": simple_item,
        "sourceId": "main_publish_item",
        "topics": [],
        "userRightsProtocols": protocols,
    })
}

pub(super) fn build_wx_edit_payload(
    payload: &ItemEditPayload,
    fallback_location: &crate::xianyu_client::LocationContext,
) -> Value {
    let detail = payload.detail.as_object().cloned().unwrap_or_default();
    let base_text = detail
        .get("itemTextDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let base_price = detail
        .get("itemPriceDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let base_addr = detail
        .get("itemAddrDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let cat_dto_obj = detail
        .get("itemCatDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let label_list = build_edit_label_list(payload, &cat_dto_obj, &detail);
    let simple_item = bool_from_optional(detail.get("simpleItem"), false);
    let no_bargain = payload.no_bargain.unwrap_or_else(|| {
        resolve_no_bargain(base_price.get("noBargain"), detail.get("bargain"), false)
    });
    let image_urls = payload
        .image_urls
        .clone()
        .unwrap_or_else(|| image_urls_from_value(&Value::Object(detail.clone())));
    // 把 addr 字段的 fallback 逻辑提到 json! 外面——json! 宏对嵌套的多行
    // chained expression（filter().unwrap_or()）解析不动，会报 unexpected
    // end of macro invocation。
    let addr_area = {
        let raw = base_addr
            .get("area")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if raw.is_empty() {
            fallback_location.area.clone()
        } else {
            raw.to_string()
        }
    };
    let addr_city = {
        let raw = base_addr
            .get("city")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if raw.is_empty() {
            fallback_location.city.clone()
        } else {
            raw.to_string()
        }
    };
    let addr_prov = {
        let raw = base_addr
            .get("prov")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if raw.is_empty() {
            fallback_location.province.clone()
        } else {
            raw.to_string()
        }
    };
    let addr_gps = {
        let raw = base_addr
            .get("gps")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if raw.is_empty() {
            fallback_location.gps.clone()
        } else {
            raw.to_string()
        }
    };
    let addr_division_id = {
        let raw = value_to_string_lossy(base_addr.get("divisionId"));
        let s = if raw.is_empty() {
            fallback_location.fish_ad_code.as_str()
        } else {
            raw.as_str()
        };
        s.parse::<i64>().unwrap_or_default()
    };
    json!({
        "itemId": payload.item_id,
        "utdid": "h5_client",
        "platform": "ios",
        "miniAppVersion": "0.3.77",
        "uniqueCode": unix_millis_now(),
        "sourceId": "main_publish_item",
        "publishScene": "wxMainPublish",
        "itemTypeStr": detail.get("itemTypeStr").and_then(Value::as_str).unwrap_or("b"),
        "simpleItem": simple_item,
        "defaultPrice": false,
        "imageInfoDOList": build_image_info_list(&image_urls, true),
        "itemTextDTO": {
            "title": if payload.title.trim().is_empty() { base_text.get("title").and_then(Value::as_str).unwrap_or_default() } else { payload.title.trim() },
            "desc": if payload.desc.trim().is_empty() { base_text.get("desc").and_then(Value::as_str).unwrap_or_default() } else { payload.desc.trim() },
        },
        "itemPriceDTO": {
            "priceInCent": if payload.price.trim().is_empty() {
                cent_string_from_value(base_price.get("priceInCent")).parse::<i64>().unwrap_or_default()
            } else {
                super::price_to_cents(payload.price.trim()).parse::<i64>().unwrap_or_default()
            },
            "origPriceInCent": if payload.orig_price.trim().is_empty() {
                cent_string_from_value(base_price.get("origPriceInCent")).parse::<i64>().unwrap_or_default()
            } else {
                super::price_to_cents(payload.orig_price.trim()).parse::<i64>().unwrap_or_default()
            },
            "noBargain": no_bargain,
        },
        "itemPostFeeDTO": build_post_fee_dto(
            payload.shipping_type.as_deref(),
            payload.can_free_shipping,
            payload.post_price.as_str(),
            payload.freight_template_id.as_str(),
            true,
        ),
        "itemAddrDTO": {
            "area": addr_area,
            "city": addr_city,
            "divisionId": addr_division_id,
            "gps": addr_gps,
            "prov": addr_prov,
        },
        "itemCatDTO": detail.get("itemCatDTO").cloned().unwrap_or_else(|| json!({})),
        "itemLabelExtList": label_list,
    })
}
// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub(super) async fn item_edit_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    if let Ok(data) = fetch_pc_backend_edit_detail(&state, &payload.uid, &payload.item_id).await {
        return Ok(ResponseJson(json!({
            "ret": super::success_ret(),
            "data": data,
        })));
    }
    if let Ok(data) = fetch_wx_edit_detail(&state, &payload.uid, &payload.item_id).await {
        return Ok(ResponseJson(json!({
            "ret": super::success_ret(),
            "data": data,
        })));
    }
    if let Ok(upstream) = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
        "1.0",
        json!({
            "itemId": payload.item_id.clone(),
        }),
        Vec::new(),
    )
    .await
    {
        let mapped = build_h5_detail_mapping(
            upstream.get("data").unwrap_or(&Value::Null),
            &payload.item_id,
        );
        return Ok(ResponseJson(json!({
            "ret": upstream.get("ret").cloned().unwrap_or_else(success_ret_value),
            "data": mapped,
        })));
    }
    let item = fetch_item(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("商品不存在"))?;
    let data = build_cached_edit_detail(&item, &payload.item_id);
    Ok(ResponseJson(json!({
        "ret": super::success_ret(),
        "data": data,
    })))
}

pub(super) async fn item_edit(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemEditPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let upstream = run_item_edit_payload_action(&state, &payload)
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream))
}

async fn run_item_edit_payload_action(
    state: &AppState,
    payload: &ItemEditPayload,
) -> Result<Value, String> {
    // 编辑在售商品：先走 wx edit，命中不支持再回退 H5 edit。
    // 重新上架是另一条链，走 /item/open → idleitem.publish。
    debug!(
        target: "market.items_edit",
        stream = "runtime",
        module = "market-items-edit",
        action = "item_edit_start",
        outcome = "success",
        account_uid = %payload.uid,
        item_id = %payload.item_id,
        "starting item edit flow"
    );
    // 账号配置的发布地：base_addr 字段全空时拿来兜底。账号没设置则字段全空，
    // 上游会拒 → 前端能看到清晰错误「请先配置发布地」而不是落到株洲。
    let fallback_location = load_location_or_default(state, &payload.uid).await;
    let wx_payload = build_wx_edit_payload(payload, &fallback_location);
    debug!(
        target: "market.items_edit",
        stream = "runtime",
        module = "market-items-edit",
        action = "wx_edit_request",
        outcome = "success",
        account_uid = %payload.uid,
        item_id = %payload.item_id,
        payload_summary = %json_log_summary_string(&wx_payload),
        "prepared wx item edit payload"
    );
    let wx_call = request_edit_transport(state, &payload.uid, TransportKind::Wx, wx_payload).await;
    match wx_call {
        Ok(upstream) => {
            debug!(
                target: "market.items_edit",
                stream = "runtime",
                module = "market-items-edit",
                action = "wx_edit_response",
                outcome = "success",
                account_uid = %payload.uid,
                item_id = %payload.item_id,
                response_summary = %json_log_summary_string(&upstream),
                "wx item edit succeeded"
            );
            Ok(upstream)
        }
        Err(e) => {
            warn!(
                target: "market.items_edit",
                stream = "runtime",
                module = "market-items-edit",
                action = "wx_edit_fallback",
                outcome = "warn",
                account_uid = %payload.uid,
                item_id = %payload.item_id,
                error_code = "wx_edit_failed",
                retryable = true,
                detail = %e,
                "wx item edit failed, falling back to H5"
            );
            let h5_payload = build_h5_edit_payload(payload, &fallback_location);
            debug!(
                target: "market.items_edit",
                stream = "runtime",
                module = "market-items-edit",
                action = "h5_edit_request",
                outcome = "success",
                account_uid = %payload.uid,
                item_id = %payload.item_id,
                payload_summary = %json_log_summary_string(&h5_payload),
                "prepared H5 fallback item edit payload"
            );
            let h5_call =
                request_edit_transport(state, &payload.uid, TransportKind::H5, h5_payload).await;
            match h5_call {
                Ok(upstream) => {
                    debug!(
                        target: "market.items_edit",
                        stream = "runtime",
                        module = "market-items-edit",
                        action = "h5_edit_response",
                        outcome = "success",
                        account_uid = %payload.uid,
                        item_id = %payload.item_id,
                        response_summary = %json_log_summary_string(&upstream),
                        "H5 fallback item edit succeeded"
                    );
                    Ok(upstream)
                }
                Err(err) => {
                    warn!(
                        target: "market.items_edit",
                        stream = "runtime",
                        module = "market-items-edit",
                        action = "h5_edit_response",
                        outcome = "warn",
                        account_uid = %payload.uid,
                        item_id = %payload.item_id,
                        error_code = "h5_edit_failed",
                        retryable = true,
                        detail = %err,
                        "H5 fallback item edit failed"
                    );
                    Err(err)
                }
            }
        }
    }
}
