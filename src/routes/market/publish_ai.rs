use axum::extract::{Extension, Json, State};
use axum::Json as ResponseJson;
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::shared::first_ret_string,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client::LocationContext,
};

use super::publish::{ItemAiDescriptionPayload, KgraphPayload};
use super::publish_kgraph::{build_kgraph_payload, extract_kgraph_cat_info};
use super::{
    build_h5_publish_labels, category_name_from_value, ensure_market_uid_access,
    load_location_or_default, payload_ret_success, success_ret, successful_upstream_h5_request,
    unix_millis_now, upstream_h5_request, value_to_string_lossy,
};

// ─── AI description helper functions ─────────────────────────────────────────

fn rich_text_desc_string(text: &str) -> String {
    serde_json::to_string(&json!({
        "type": Value::Null,
        "children": [{
            "type": "Element",
            "children": [{
                "type": "Text",
                "text": text,
                "properties": {}
            }],
            "properties": {}
        }],
        "properties": {},
        "version": "1.0",
    }))
    .unwrap_or_else(|_| String::new())
}

fn build_ai_media_list(urls: &[String]) -> Vec<Value> {
    urls.iter()
        .enumerate()
        .map(|(index, url)| {
            json!({
                "url": url,
                "major": index == 0,
                "type": 0,
                "widthSize": 0,
                "heightSize": 0,
                "labels": [],
                "extraInfo": {
                    "raw": "true",
                    "isT": "false",
                    "isH": "false",
                },
                "longImage": false,
                "thumbnail": url,
                "imgPath": url,
                "isQrCode": false,
            })
        })
        .collect()
}

fn build_ai_item_cat_dto(category: &Value) -> Value {
    json!({
        "catId": value_to_string_lossy(category.get("catId")),
        "catName": category_name_from_value(category),
        "tbCatId": value_to_string_lossy(category.get("tbCatId")),
        "leafId": value_to_string_lossy(category.get("leafId")),
        "channelCatId": value_to_string_lossy(category.get("channelCatId")),
        "channelCat1Name": value_to_string_lossy(category.get("channelCat1Name")),
        "channelCat2Name": value_to_string_lossy(category.get("channelCat2Name")),
        "channelCat3Name": value_to_string_lossy(category.get("channelCat3Name")),
        "channelCat1Id": value_to_string_lossy(category.get("channelCat1Id")),
        "channelCat2Id": value_to_string_lossy(category.get("channelCat2Id")),
        "channelCat3Id": value_to_string_lossy(category.get("channelCat3Id")),
    })
}

fn build_ai_item_addr_dto(location: &LocationContext) -> Value {
    json!({
        "divisionId": location.fish_ad_code,
        "prov": location.province,
        "city": location.city,
        "area": location.area,
        "gps": location.gps,
        "poiId": "",
        "poiName": "",
        "aoiId": "",
        "aoiName": "",
        "addressType": 0,
    })
}

fn extract_publish_preget_base_params(payload: &Value) -> Option<Value> {
    payload
        .get("data")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|data| {
            data.get("pages")
                .and_then(Value::as_array)
                .and_then(|pages| pages.first())
                .and_then(|page| page.get("container"))
                .and_then(|container| container.get("data"))
                .and_then(|data| data.get("baseParams"))
                .cloned()
        })
}

fn build_ai_desc_cpv_cards(raw_kgraph: &Value, fallback_category: &Value) -> Vec<Value> {
    let mut cards = raw_kgraph
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("cardList"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if cards.is_empty() && !fallback_category.is_null() {
        cards.push(json!({
            "cardType": "20401",
            "cardData": {
                "propertyId": "-10000",
                "propertyName": "分类",
                "supportYanHuoBao": false,
                "supportYanHuoBaoV3": false,
                "valuesList": [fallback_category.clone()],
            }
        }));
    }
    cards.truncate(5);
    cards
}

fn build_ai_desc_selected_list(category: &Value) -> Vec<Value> {
    build_h5_publish_labels(Some(category))
}

fn build_ai_custom_item_input(title: &str, desc: &str) -> String {
    serde_json::to_string(&json!({
        "itemTextDTO": {
            "title": title,
            "desc": desc,
            "trackParamsRichTextStyle": "",
        }
    }))
    .unwrap_or_else(|_| {
        "{\"itemTextDTO\":{\"title\":\"\",\"desc\":\"\",\"trackParamsRichTextStyle\":\"\"}}"
            .to_string()
    })
}

fn build_ai_desc_item_input(
    payload: &ItemAiDescriptionPayload,
    category: &Value,
    location: &LocationContext,
    unique_code: &str,
    base_params: Option<&Value>,
) -> String {
    let title = payload.title.trim();
    let desc = payload.desc.trim();
    let rich_text_desc = rich_text_desc_string(desc);
    let base_params_value = base_params.cloned().unwrap_or_else(|| {
        json!({
            "simpleItem": "true",
            "rentMultiDayPackage": "1",
            "bizcode": "mainPublish",
            "bucketId": "4",
            "rentSmartPrice": "1",
            "scene": "ai_assist_desc_gen",
        })
    });
    serde_json::to_string(&json!({
        "simpleItem": "true",
        "rentMultiDayPackage": "1",
        "bizcode": "mainPublish",
        "bucketId": base_params_value.get("bucketId").and_then(Value::as_str).unwrap_or("4"),
        "rentSmartPrice": "1",
        "scene": base_params_value.get("scene").and_then(Value::as_str).unwrap_or("ai_assist_desc_gen"),
        "topics": [],
        "imageInfoDOList": build_ai_media_list(&payload.image_urls),
        "itemTextDTO": {
            "title": title,
            "desc": desc,
            "richTextDesc": rich_text_desc,
            "titleDescSeparate": false,
            "shortTitleHighKeyword": [],
            "trackParamsRichTextStyle": "",
            "shortTitle": "",
        },
        "editorVersion": "0.0.1",
        "itemTopicParams": { "topicInfos": [] },
        "aigc": "0",
        "aigcRequestId": "",
        "bizRent": "{\"needConvert\":true}",
        "itemCatDTO": build_ai_item_cat_dto(category),
        "itemLabelExtList": build_h5_publish_labels(Some(category)),
        "itemPriceDTO": {
            "priceInCent": "0",
            "origPriceInCent": "0",
            "noBargain": false,
        },
        "quantity": "1",
        "bizAuctionInfo": Value::Null,
        "auctionInfo": Value::Null,
        "defaultPrice": true,
        "itemTypeStr": "b",
        "bizFansPriceConfig": Value::Null,
        "itemRentalMultiDayPackages": Value::Null,
        "enableRentThreeFreeOne": false,
        "isRentalMode": false,
        "aiHostUsed": false,
        "itemPostFeeDTO": {
            "canFreeShipping": true,
            "supportFreight": true,
            "idleTemplateId": "0",
            "templateId": "0",
            "postPriceInCent": "0",
            "onlyTakeSelf": false,
        },
        "itemAddrDTO": build_ai_item_addr_dto(location),
        "userRightsProtocols": [],
        "itemStatus": "0",
        "itemGroupDTO": { "groupId": "0" },
        "brokerageAgree": false,
        "innerPublishType": Value::Null,
        "sourceId": Value::Null,
        "fromType": Value::Null,
        "publishScene": "mainPublish",
        "publishSceneTab": Value::Null,
        "itemId": "",
        "uniqueCode": unique_code,
        "draftId": Value::Null,
        "asyncSecurityInfo": Value::Null,
        "baseParams": base_params_value,
        "bizFrom": Value::Null,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

fn apply_prop_selections_to_cards(card_list: &[Value], prop_selections: &Value) -> Vec<Value> {
    let sel_map = prop_selections.as_object();
    card_list
        .iter()
        .map(|card| {
            let mut card = card.clone();
            let property_id = card
                .get("propertyId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let selected_value = sel_map
                .and_then(|map| map.get(&property_id))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !selected_value.is_empty() {
                if let Some(values) = card.get_mut("valuesList").and_then(Value::as_array_mut) {
                    for v in values.iter_mut() {
                        let vid = v
                            .get("valueId")
                            .or_else(|| v.get("channelCatId"))
                            .or_else(|| v.get("catId"))
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if vid == selected_value {
                            v.as_object_mut()
                                .map(|obj| obj.insert("isClicked".to_string(), json!("1")));
                        } else {
                            v.as_object_mut().map(|obj| obj.remove("isClicked"));
                        }
                    }
                }
            }
            json!({ "cardData": card })
        })
        .collect()
}

fn build_cpv_selected_list_from_cards(card_list: &[Value], prop_selections: &Value) -> Vec<Value> {
    let sel_map = match prop_selections.as_object() {
        Some(m) => m,
        None => return Vec::new(),
    };
    let mut selected = Vec::new();
    for card in card_list {
        let property_id = card
            .get("propertyId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let selected_vid = match sel_map.get(property_id).and_then(Value::as_str) {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let values = card
            .get("valuesList")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for v in &values {
            let vid = v
                .get("valueId")
                .or_else(|| v.get("channelCatId"))
                .or_else(|| v.get("catId"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if vid == selected_vid {
                let property_name = card
                    .get("propertyName")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let text = v
                    .get("catName")
                    .or_else(|| v.get("text"))
                    .or_else(|| v.get("valueName"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let mut entry = v.clone();
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("propertyId".to_string(), json!(property_id));
                    obj.insert("propertyName".to_string(), json!(property_name));
                    obj.insert("text".to_string(), json!(text));
                }
                selected.push(entry);
                break;
            }
        }
    }
    selected
}

fn build_ai_desc_generation_payload_from_cards(
    payload: &ItemAiDescriptionPayload,
    category: &Value,
    location: &LocationContext,
    base_params: Option<&Value>,
) -> Value {
    let unique_code = unix_millis_now().to_string();
    let request_id = format!("rust-{}-{}", payload.uid.trim(), unique_code);
    let cpv_cards = apply_prop_selections_to_cards(&payload.card_list, &payload.prop_selections);
    let cpv_selected =
        build_cpv_selected_list_from_cards(&payload.card_list, &payload.prop_selections);
    let cpv_selected = if cpv_selected.is_empty() {
        build_ai_desc_selected_list(category)
    } else {
        cpv_selected
    };
    json!({
        "bizCode": "mainPublish",
        "cpvCardList": cpv_cards,
        "cpvSelectedList": cpv_selected,
        "cpvSource": "itemPublishCpv",
        "customItemInputJson": build_ai_custom_item_input(payload.title.trim(), payload.desc.trim()),
        "extraParams": "",
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "itemInputJson": build_ai_desc_item_input(payload, category, location, &unique_code, base_params),
        "latitude": location.latitude,
        "longitude": location.longitude,
        "medias": build_ai_media_list(&payload.image_urls),
        "originalDesc": payload.desc.trim(),
        "requestId": request_id,
        "scene": base_params
            .and_then(|value| value.get("scene"))
            .and_then(Value::as_str)
            .unwrap_or("ai_assist_desc_gen"),
        "stream": true,
        "style": "concise",
        "supportPolish": true,
        "uniqueCode": unique_code,
    })
}

fn has_meaningful_frontend_card_context(card_list: &[Value], prop_selections: &Value) -> bool {
    if prop_selections
        .as_object()
        .map(|map| {
            map.iter().any(|(property_id, value)| {
                property_id != "-10000"
                    && value
                        .as_str()
                        .map(|text| !text.trim().is_empty())
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false)
    {
        return true;
    }

    card_list.iter().any(|card| {
        let property_id = card
            .get("propertyId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if property_id == "-10000" {
            return false;
        }
        card.get("valuesList")
            .and_then(Value::as_array)
            .map(|values| {
                values.iter().any(|value| {
                    value
                        .get("isClicked")
                        .and_then(Value::as_str)
                        .map(|flag| flag == "1")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    })
}

fn build_ai_desc_generation_payload(
    payload: &ItemAiDescriptionPayload,
    category: &Value,
    location: &LocationContext,
    raw_kgraph: &Value,
    base_params: Option<&Value>,
) -> Value {
    let unique_code = unix_millis_now().to_string();
    let request_id = format!("rust-{}-{}", payload.uid.trim(), unique_code);
    json!({
        "bizCode": "mainPublish",
        "cpvCardList": build_ai_desc_cpv_cards(raw_kgraph, category),
        "cpvSelectedList": build_ai_desc_selected_list(category),
        "cpvSource": "itemPublishCpv",
        "customItemInputJson": build_ai_custom_item_input(payload.title.trim(), payload.desc.trim()),
        "extraParams": "",
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "itemInputJson": build_ai_desc_item_input(payload, category, location, &unique_code, base_params),
        "latitude": location.latitude,
        "longitude": location.longitude,
        "medias": build_ai_media_list(&payload.image_urls),
        "originalDesc": payload.desc.trim(),
        "requestId": request_id,
        "scene": base_params
            .and_then(|value| value.get("scene"))
            .and_then(Value::as_str)
            .unwrap_or("ai_assist_desc_gen"),
        "stream": true,
        "style": "concise",
        "supportPolish": true,
        "uniqueCode": unique_code,
    })
}

fn normalize_description_headline(desc: &str) -> String {
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

fn build_input_subject(title: &str, desc: &str, category: &str) -> String {
    let normalized_title = title.trim();
    if !normalized_title.is_empty() {
        return normalized_title.to_string();
    }
    let desc_title = derive_title_from_description(desc);
    if !desc_title.is_empty() {
        return desc_title;
    }
    let desc_headline = normalize_description_headline(desc);
    if !desc_headline.is_empty() {
        return desc_headline.chars().take(60).collect::<String>();
    }
    if !category.trim().is_empty() {
        return category.trim().to_string();
    }
    "这件商品".to_string()
}

fn summarize_log_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let summary: String = trimmed.chars().take(limit).collect();
    if trimmed.chars().count() > limit {
        format!("{summary}...")
    } else {
        summary
    }
}

fn build_desc_context_line(desc: &str) -> String {
    let normalized = normalize_description_headline(desc);
    if normalized.is_empty() {
        return String::new();
    }
    let snippet = normalized.chars().take(90).collect::<String>();
    if snippet.is_empty() {
        String::new()
    } else {
        format!("当前已输入的描述重点是：{snippet}。")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiDraftKind {
    Service,
    Storage,
    Digital,
    Generic,
}

fn contains_any_keyword(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}

fn classify_ai_draft_kind(title: &str, category: &str) -> AiDraftKind {
    let text = format!("{title} {category}");
    if contains_any_keyword(
        &text,
        &[
            "服务", "代写", "论文", "降重", "润色", "辅导", "设计", "开发", "咨询", "翻译", "课程",
            "培训", "简历", "文案", "脚本",
        ],
    ) {
        return AiDraftKind::Service;
    }
    if contains_any_keyword(
        &text,
        &[
            "硬盘",
            "SSD",
            "ssd",
            "固态",
            "机械盘",
            "机械硬盘",
            "移动硬盘",
            "U盘",
            "u盘",
            "TF卡",
            "tf卡",
            "存储卡",
            "内存卡",
            "NAS",
            "nas",
        ],
    ) {
        return AiDraftKind::Storage;
    }
    if contains_any_keyword(
        &text,
        &[
            "手机",
            "iPhone",
            "iphone",
            "华为",
            "小米",
            "荣耀",
            "三星",
            "笔记本",
            "电脑",
            "主机",
            "MacBook",
            "macbook",
            "iPad",
            "ipad",
            "平板",
            "显示器",
            "显卡",
            "键盘",
            "鼠标",
            "耳机",
            "相机",
            "镜头",
            "路由器",
            "电池",
        ],
    ) {
        return AiDraftKind::Digital;
    }
    AiDraftKind::Generic
}

fn build_ai_intro_line(kind: AiDraftKind, subject: &str, seed: u64) -> String {
    match kind {
        AiDraftKind::Service => match seed % 3 {
            0 => format!(
                "{subject}，这类内容会先按需求沟通交付范围、完成时间和修改次数，确认细节后再安排。"
            ),
            1 => format!(
                "{subject}，适合先把目标、用途和交付标准聊清楚，确认范围后再开始处理。"
            ),
            _ => format!(
                "{subject}，会先根据你的具体需求确认服务边界、交付内容和处理时效，再进入正式交付。"
            ),
        },
        AiDraftKind::Storage => match seed % 3 {
            0 => format!(
                "{subject}，容量、接口和使用情况会按标题与实物逐项核对，适合做资料备份、扩容或日常存储。"
            ),
            1 => format!(
                "{subject}，这类存储设备更看重容量、接口和稳定性，具体细节会结合实物情况如实说明。"
            ),
            _ => format!(
                "{subject}，会重点说明容量、接口、通电或读写相关情况，方便先判断是否符合你的使用场景。"
            ),
        },
        AiDraftKind::Digital => match seed % 3 {
            0 => format!(
                "{subject}，型号、配置、版本和配件情况会按标题与实物整理，先把核心信息说清楚。"
            ),
            1 => format!(
                "{subject}，这类数码设备通常更关注版本、配置和功能状态，细节会按实际情况说明。"
            ),
            _ => format!(
                "{subject}，会围绕型号、配置、成色和功能表现来补充描述，方便先判断是否对口。"
            ),
        },
        AiDraftKind::Generic => match seed % 3 {
            0 => format!(
                "{subject}，会围绕标题里的关键信息整理描述，先把你最关心的重点说清楚。"
            ),
            1 => format!(
                "{subject}，当前先按标题生成参考描述，具体细节可以继续补充完善。"
            ),
            _ => format!(
                "{subject}，核心信息会结合标题先整理出来，后续细节再按实际情况补全。"
            ),
        },
    }
}

fn build_ai_detail_line(kind: AiDraftKind, has_images: bool, seed: u64) -> String {
    match (kind, has_images) {
        (AiDraftKind::Service, false) => match seed % 3 {
            0 => {
                "当前是根据标题生成的参考描述，需求、交付格式和完成周期都可以继续补充。".to_string()
            }
            1 => "暂时没有更多上下文时，会先按标题给出基础说明，后续可以把具体要求补进来。"
                .to_string(),
            _ => "如果你有明确的交付要求、截止时间或风格偏好，补充后描述会更贴合实际。".to_string(),
        },
        (AiDraftKind::Service, true) => match seed % 3 {
            0 => "已上传的素材会作为参考，服务范围、交付方式和修改要求仍建议单独确认。".to_string(),
            1 => "现有图片会参与参考，但真正影响成交的还是交付边界、时效和修改规则。".to_string(),
            _ => "图片会辅助理解需求，具体交付标准和服务细节建议下单前确认清楚。".to_string(),
        },
        (AiDraftKind::Storage, false) => match seed % 3 {
            0 => "当前没有图片时，会优先按标题生成说明；容量、接口、健康度和配件情况可以继续补充。"
                .to_string(),
            1 => "如果后面补充实拍图，容量标签、接口细节和外观状态还能再写得更完整。".to_string(),
            _ => "没有图片时先给出标题驱动的参考描述，通电情况、外观和配件信息建议后续补齐。"
                .to_string(),
        },
        (AiDraftKind::Storage, true) => match seed % 3 {
            0 => "图片会优先参考实物状态，容量标识、接口细节和外观痕迹以实际展示为准。".to_string(),
            1 => "有实拍图时会更适合补充外观、标签和接口细节，减少沟通成本。".to_string(),
            _ => "图片能帮助判断外观和配件情况，容量、接口和使用状态仍建议在文案里写清楚。"
                .to_string(),
        },
        (AiDraftKind::Digital, false) => {
            match seed % 3 {
                0 => "目前先按标题生成参考描述，版本、配置、功能状态和配件情况可以继续补充。"
                    .to_string(),
                1 => "没有图片时，会先把标题里的型号和配置语义整理出来，后续再补外观与细节。"
                    .to_string(),
                _ => "如果后面补充实拍图，成色、接口、边角和配件情况都能描述得更完整。".to_string(),
            }
        }
        (AiDraftKind::Digital, true) => match seed % 3 {
            0 => "实拍图会优先作为成色和配件参考，版本、配置和功能状态仍建议在文案里写明。"
                .to_string(),
            1 => "有图片时更适合补充外观细节，配置、版本和使用情况建议同步写清楚。".to_string(),
            _ => "图片能帮助判断成色和附件情况，功能状态和配置细节仍建议文字确认。".to_string(),
        },
        (AiDraftKind::Generic, false) => match seed % 3 {
            0 => "当前是根据标题生成的参考描述，后续补充实拍图或细节后会更准确。".to_string(),
            1 => "暂时没有图片时，会先按标题整理基础描述，细节信息可以继续往里补。".to_string(),
            _ => "没有更多素材时先给出标题版说明，成色、配件和使用情况建议后续补全。".to_string(),
        },
        (AiDraftKind::Generic, true) => match seed % 3 {
            0 => "图片会优先参考实物状态，文字部分再把关键信息补齐会更完整。".to_string(),
            1 => "有实拍图时更容易补充外观细节，文字里建议把核心卖点写得更明确。".to_string(),
            _ => "图片能帮助确认外观和细节，其他使用信息仍建议在文案里直接写清楚。".to_string(),
        },
    }
}

fn build_ai_closing_line(kind: AiDraftKind, seed: u64) -> String {
    match kind {
        AiDraftKind::Service => match seed % 3 {
            0 => "如果你对交付深度、完成时间或修改范围有要求，建议先沟通清楚再下单。".to_string(),
            1 => "适合先把需求边界确认完整，再安排交付，会更省来回沟通。".to_string(),
            _ => "确认需求、时效和交付方式都对齐后再拍，会更稳妥。".to_string(),
        },
        AiDraftKind::Storage => match seed % 3 {
            0 => "对容量、接口、通电时长或配件有要求的话，建议先沟通清楚再拍。".to_string(),
            1 => "如果你比较在意健康度、接口版本或外观细节，先聊清楚会更合适。".to_string(),
            _ => "确认容量、接口和配件情况都符合预期后再拍，后续会更顺。".to_string(),
        },
        AiDraftKind::Digital => match seed % 3 {
            0 => "如果你对版本、配置、成色或配件有硬要求，建议先沟通确认再拍。".to_string(),
            1 => "数码类商品更适合先把配置、功能状态和附件情况聊清楚，再决定是否下单。".to_string(),
            _ => "确认版本、功能和配件情况都没问题后再拍，会更省后续沟通。".to_string(),
        },
        AiDraftKind::Generic => match seed % 3 {
            0 => "对细节、配件或使用情况有要求的话，建议先沟通清楚再拍。".to_string(),
            1 => "确认关键信息没有偏差后再拍，会更稳妥。".to_string(),
            _ => "如果你比较在意细节，先把问题聊清楚再下单会更合适。".to_string(),
        },
    }
}

fn build_ai_description_draft(payload: &ItemAiDescriptionPayload) -> String {
    let title = payload.title.trim();
    let desc = payload.desc.trim();
    let category = category_name_from_value(&payload.category);
    let subject = build_input_subject(title, desc, &category);
    let kind = classify_ai_draft_kind(&subject, &category);
    let seed = unix_millis_now() as u64;
    let mut lines = vec![build_ai_intro_line(kind, &subject, seed)];
    let context_line = build_desc_context_line(desc);
    if !context_line.is_empty() {
        lines.push(context_line);
    }
    lines.extend([
        build_ai_detail_line(kind, !payload.image_urls.is_empty(), seed.wrapping_add(1)),
        build_ai_closing_line(kind, seed.wrapping_add(2)),
    ]);
    lines.join("\n")
}

fn is_generic_desc_title_segment(text: &str) -> bool {
    let normalized = text.trim();
    if normalized.is_empty() {
        return true;
    }
    [
        "图片实拍",
        "成色如图",
        "整体状态良好",
        "适合日常使用",
        "细节可沟通确认",
        "确认无误后再拍",
    ]
    .iter()
    .any(|item| normalized == *item || normalized.starts_with(item))
}

fn derive_title_from_description(desc: &str) -> String {
    let first_line = desc
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    if first_line.is_empty() {
        return String::new();
    }
    let normalized = [
        "图片版描述：",
        "图片版描述:",
        "AI描述：",
        "AI描述:",
        "商品描述：",
        "商品描述:",
    ]
    .iter()
    .find_map(|prefix| first_line.strip_prefix(prefix))
    .unwrap_or(first_line)
    .trim();
    if normalized.is_empty() {
        return String::new();
    }

    let sentence = normalized
        .split(|c| matches!(c, '。' | '！' | '？' | '!' | '?'))
        .next()
        .unwrap_or(normalized)
        .trim();
    if sentence.is_empty() {
        return String::new();
    }

    let mut fallback = String::new();
    for segment in sentence
        .split(|c| matches!(c, '，' | ',' | '；' | ';'))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        if fallback.is_empty() {
            fallback = segment.to_string();
        }
        if segment.chars().count() >= 4 && !is_generic_desc_title_segment(segment) {
            let title: String = segment.chars().take(60).collect();
            return title.trim().to_string();
        }
    }

    let title: String = if fallback.is_empty() {
        sentence.chars().take(60).collect()
    } else {
        fallback.chars().take(60).collect()
    };
    let title = title.trim().to_string();
    if is_generic_desc_title_segment(&title) {
        String::new()
    } else {
        title
    }
}

fn build_ai_title_suggestion(payload: &ItemAiDescriptionPayload, desc: Option<&str>) -> String {
    let title = payload.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    let desc_title = desc.map(derive_title_from_description).unwrap_or_default();
    if !desc_title.is_empty() {
        return desc_title;
    }
    String::new()
}

fn extract_ai_title_from_upstream(data: &Value) -> String {
    for key in ["title", "itemTitle", "shortTitle", "subject"] {
        let value = data
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    String::new()
}

fn parse_upstream_data_value(payload: &Value) -> Value {
    payload
        .get("data")
        .and_then(|value| match value {
            Value::String(text) => serde_json::from_str::<Value>(text).ok(),
            _ => Some(value.clone()),
        })
        .unwrap_or(Value::Null)
}

fn normalize_upstream_error_display_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .split("::")
        .last()
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

fn extract_upstream_error_text(payload: &Value) -> String {
    let ret = first_ret_string(payload).trim().to_string();
    if !ret.is_empty() {
        let normalized = normalize_upstream_error_display_text(&ret);
        if !normalized.is_empty() {
            return normalized;
        }
        return ret;
    }

    let data = parse_upstream_data_value(payload);
    for value in [
        payload.get("message"),
        payload.get("errorMsg"),
        payload.get("errorMessage"),
        payload.get("sub_msg"),
        payload.get("subMsg"),
        payload.get("detail"),
        data.get("message"),
        data.get("errorMsg"),
        data.get("errorMessage"),
        data.get("sub_msg"),
        data.get("subMsg"),
        data.get("detail"),
    ] {
        let text = normalize_upstream_error_display_text(&value_to_string_lossy(value));
        if !text.is_empty() {
            return text;
        }
    }

    "官方 AI 描述服务暂时不可用，请稍后重试".to_string()
}

// ─── AI description handler ───────────────────────────────────────────────────

pub(super) async fn item_ai_description(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemAiDescriptionPayload>,
) -> ApiResult {
    let uid = payload.uid.trim();
    let normalized_desc = payload.desc.trim().to_string();
    let normalized_title = build_input_subject(payload.title.trim(), &normalized_desc, "");
    debug!(
        target: "market.publish_ai",
        stream = "runtime",
        module = "market-publish-ai",
        action = "ai_desc_request",
        outcome = "success",
        account_uid = %uid,
        raw_title = %summarize_log_text(payload.title.trim(), 60),
        normalized_title = %summarize_log_text(&normalized_title, 60),
        raw_category = %summarize_log_text(&category_name_from_value(&payload.category), 40),
        card_list_len = payload.card_list.len(),
        image_count = payload.image_urls.len(),
        desc = %summarize_log_text(&normalized_desc, 120),
        "received ai description request"
    );
    let generation_payload = ItemAiDescriptionPayload {
        title: normalized_title,
        desc: normalized_desc,
        ..payload.clone()
    };
    if uid.is_empty() {
        return Err(ResponseError::bad_request("缺少账号，无法生成 AI 描述"));
    }
    ensure_market_uid_access(&state, &identity, uid).await?;

    let location = load_location_or_default(&state, uid).await;
    let preget = successful_upstream_h5_request(
        &state,
        uid,
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
    .ok();
    let preget_base_params = preget.as_ref().and_then(extract_publish_preget_base_params);

    let has_frontend_cards = !payload.card_list.is_empty()
        && has_meaningful_frontend_card_context(&payload.card_list, &payload.prop_selections);
    let raw_kgraph = if has_frontend_cards {
        None
    } else {
        match upstream_h5_request(
            &state,
            uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.kgraph.property.recommend"),
            "2.0",
            build_kgraph_payload(
                &KgraphPayload {
                    uid: payload.uid.clone(),
                    title: generation_payload.title.clone(),
                    desc: generation_payload.desc.clone(),
                    cat_id: String::new(),
                    channel_cat_id: String::new(),
                    current_card_list: Vec::new(),
                    selected_list: Vec::new(),
                    image_urls: generation_payload.image_urls.clone(),
                },
                &location,
            ),
            Vec::new(),
        )
        .await
        {
            Ok(payload) if payload_ret_success(&payload) => Some(payload),
            Ok(payload) => {
                let detail = extract_upstream_error_text(&payload);
                warn!("kgraph request returned non-success payload: {}", detail);
                return Err(ResponseError::service_unavailable(detail));
            }
            Err(error) => {
                warn!("kgraph request failed: {}", error);
                None
            }
        }
    };

    let category = if has_frontend_cards {
        if payload.category.is_null() {
            None
        } else {
            Some(payload.category.clone())
        }
    } else {
        raw_kgraph
            .as_ref()
            .and_then(extract_kgraph_cat_info)
            .or_else(|| {
                if payload.category.is_null() {
                    None
                } else {
                    Some(payload.category.clone())
                }
            })
    };

    let can_generate = if has_frontend_cards {
        category.is_some()
    } else {
        category.is_some() && raw_kgraph.is_some()
    };

    if !can_generate {
        if !has_frontend_cards && raw_kgraph.is_none() {
            return Err(ResponseError::service_unavailable(
                "官方 AI 描述服务暂时不可用，请稍后重试",
            ));
        }
        if category.is_none() {
            return Err(ResponseError::bad_request(
                "未识别到商品分类，请补充描述或图片后重试",
            ));
        }
        return Err(ResponseError::service_unavailable(
            "官方 AI 描述服务暂时不可用，请稍后重试",
        ));
    }

    let category = category.as_ref().unwrap();
    let request_payload = if has_frontend_cards {
        build_ai_desc_generation_payload_from_cards(
            &generation_payload,
            category,
            &location,
            preget_base_params.as_ref(),
        )
    } else {
        build_ai_desc_generation_payload(
            &generation_payload,
            category,
            &location,
            raw_kgraph.as_ref().unwrap(),
            preget_base_params.as_ref(),
        )
    };
    let upstream_result = upstream_h5_request(
        &state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.adv.item.desc.generation"),
        "2.0",
        request_payload,
        Vec::new(),
    )
    .await;
    if let Ok(upstream) = upstream_result {
        if !payload_ret_success(&upstream) {
            let detail = extract_upstream_error_text(&upstream);
            warn!(
                "ai desc generation returned non-success payload: {}",
                detail
            );
            return Err(ResponseError::service_unavailable(detail));
        }

        let data = parse_upstream_data_value(&upstream);
        let desc = data
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        if !desc.is_empty() {
            let category_title = category_name_from_value(category);
            let title = {
                let upstream_title = extract_ai_title_from_upstream(&data);
                if !upstream_title.is_empty() && upstream_title != category_title {
                    upstream_title
                } else {
                    build_ai_title_suggestion(&generation_payload, Some(&desc))
                }
            };
            debug!(
                target: "market.publish_ai",
                stream = "runtime",
                module = "market-publish-ai",
                action = "ai_desc_upstream_response",
                outcome = "success",
                account_uid = %uid,
                category = %summarize_log_text(&category_title, 40),
                title = %summarize_log_text(&title, 60),
                desc = %summarize_log_text(&desc, 120),
                "received upstream ai description response"
            );
            return Ok(ResponseJson(json!({
                "ret": success_ret(),
                "data": {
                    "title": title,
                    "desc": desc,
                    "source": "upstream_h5",
                    "requestId": value_to_string_lossy(data.get("requestId")),
                    "category": category,
                    "commonPropertiesExtraction": data.get("commonPropertiesExtraction").cloned().unwrap_or(Value::Null),
                }
            })));
        }

        return Err(ResponseError::service_unavailable(
            "官方 AI 描述未返回有效内容，请稍后重试",
        ));
    }

    warn!(
        "h5_request for ai desc generation failed: {}",
        upstream_result.unwrap_err()
    );
    Err(ResponseError::service_unavailable(
        "官方 AI 描述服务暂时不可用，请稍后重试",
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_ai_description_draft, has_meaningful_frontend_card_context};
    use crate::routes::market::publish::ItemAiDescriptionPayload;
    use serde_json::{json, Value};

    fn payload(title: &str) -> ItemAiDescriptionPayload {
        ItemAiDescriptionPayload {
            uid: "u1".to_string(),
            title: title.to_string(),
            desc: String::new(),
            category: Value::Null,
            image_urls: Vec::new(),
            card_list: Vec::new(),
            prop_selections: Value::Null,
        }
    }

    #[test]
    fn title_only_service_fallback_uses_service_specific_copy() {
        let desc = build_ai_description_draft(&payload("论文润色/降重服务"));
        assert!(desc.contains("论文润色/降重服务"));
        assert!(desc.contains("交付"));
        assert!(!desc.contains("整体状态良好"));
        assert!(!desc.contains("适合日常使用"));
    }

    #[test]
    fn title_only_storage_fallback_uses_title_specific_context() {
        let desc = build_ai_description_draft(&payload("西数1TB移动硬盘 USB3.0"));
        assert!(desc.contains("西数1TB移动硬盘 USB3.0"));
        assert!(desc.contains("容量") || desc.contains("接口") || desc.contains("存储"));
        assert!(!desc.contains("整体状态良好"));
    }

    #[test]
    fn desc_only_fallback_uses_existing_description_context() {
        let mut data = payload("");
        data.desc = "自用 iPhone 13 128G 蓝色，电池 89%，屏幕和面容正常。".to_string();
        let desc = build_ai_description_draft(&data);
        assert!(desc.contains("iPhone 13 128G 蓝色"));
        assert!(desc.contains("当前已输入的描述重点"));
        assert!(!desc.contains("整体状态良好"));
    }

    #[test]
    fn category_only_frontend_cards_are_not_treated_as_selected_props() {
        let card_list = vec![json!({
            "propertyId": "-10000",
            "propertyName": "分类",
            "valuesList": [{
                "catName": "手机",
                "channelCatId": "126862528",
                "isClicked": "1"
            }]
        })];
        let prop_selections = json!({ "-10000": "126862528" });

        assert!(!has_meaningful_frontend_card_context(
            &card_list,
            &prop_selections
        ));
    }

    #[test]
    fn explicit_non_category_frontend_cards_are_kept() {
        let card_list = vec![json!({
            "propertyId": "20123",
            "propertyName": "品牌",
            "valuesList": [{
                "valueId": "apple",
                "valueName": "Apple/苹果",
                "isClicked": "1"
            }]
        })];
        let prop_selections = json!({ "20123": "apple" });

        assert!(has_meaningful_frontend_card_context(
            &card_list,
            &prop_selections
        ));
    }
}
