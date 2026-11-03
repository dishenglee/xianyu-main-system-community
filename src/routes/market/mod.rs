pub(crate) mod coin;
mod comments;
pub(crate) mod exposure;
pub(crate) mod groupon;
pub(crate) mod items;
mod items_batch;
pub(crate) mod items_edit;
pub(crate) mod items_open;
mod items_sync;
mod items_sync_all;
pub(crate) mod orders;
pub(crate) mod pricing;
mod promotion;
mod publish;
mod publish_ai;
mod publish_import;
mod publish_kgraph;
mod search;
mod super_polish;
mod transport_fallback;
mod upload;
mod violations;

// Re-exports: write_sync_state used by orders sub-module via `super::`
use items_sync::write_sync_state;
// Re-export: sync_items_to_db used by routes::live and items list_items
pub(crate) use items_sync::sync_items_to_db;
// Re-export: sync_orders_to_db used by routes::live
pub(crate) use orders::sync_orders_to_db;

use axum::{
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{Row, SqlitePool};

use crate::{
    models::auth::RequestIdentity,
    routes::{
        error::ResponseError,
        tenant::{ensure_account_uid_accessible, owner_username_filter},
    },
    state::AppState,
    xianyu_client::{self, LocationContext},
};

pub(crate) use crate::routes::accounts::shared::{
    payload_ret_success, successful_upstream_app_request, successful_upstream_h5_request,
    successful_upstream_wx_request, upstream_app_request, upstream_h5_request,
};

// ─── router ─────────────────────────────────────────────────────────────────

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(coin::routes())
        .merge(exposure::routes())
        .merge(promotion::routes())
        .merge(super_polish::router())
        .route(
            crate::sealed_protocol::sealed_str!("/search"),
            post(search::search),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/search/shade"),
            post(search::search_shade),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/detail"),
            post(items::item_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/share_link"),
            post(items::item_share_link),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/comment/list"),
            post(comments::item_comment_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/comment/capture"),
            post(comments::item_comment_capture),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/comment/publish"),
            post(comments::item_comment_publish),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/items"),
            post(items::list_items),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/items/sync"),
            post(items_sync::sync_items_route),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/items/sync_all"),
            post(items_sync_all::sync_all_items),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/items/management_live"),
            post(items::items_management_live),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/orders/sync"),
            post(orders::sync_orders_route),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/polish"),
            post(items::item_polish),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/reduce_price"),
            post(pricing::item_reduce_price),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/reduce_price_feeds"),
            post(pricing::item_reduce_price_feeds),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/groupon_status"),
            post(groupon::item_groupon_status),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/groupon_guide"),
            post(groupon::item_groupon_guide),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/groupon_open"),
            post(groupon::item_groupon_open),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/groupon_close"),
            post(groupon::item_groupon_close),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/super_exposure"),
            post(groupon::item_super_exposure),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/delete"),
            post(items::item_delete),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/items/batch_delete"),
            post(items_batch::item_batch_delete),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/downshelf"),
            post(items::item_downshelf),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/edit_detail"),
            post(items_edit::item_edit_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/edit"),
            post(items_edit::item_edit),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/kgraph"),
            post(publish_kgraph::item_kgraph),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/ai_description"),
            post(publish_ai::item_ai_description),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/publish"),
            post(publish::item_publish),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/bargain_price_range"),
            post(pricing::item_bargain_price_range),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/open"),
            post(items_open::item_open),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/seller_recommend"),
            get(publish_import::item_seller_recommend),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/resell_feeds"),
            get(publish_import::item_resell_feeds),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/awesome_detail"),
            post(publish_import::item_awesome_detail),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/awesome_detail_unit"),
            post(publish_import::item_awesome_detail_unit),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/trade/close"),
            post(orders::trade_close),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/trade/adjust_price"),
            post(orders::trade_adjust_price),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/collect"),
            post(items::item_collect),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/comment/delete"),
            post(comments::item_comment_delete),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/import_link"),
            post(publish_import::item_import_link),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/items/batch_reduce"),
            post(pricing::items_batch_reduce),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/trade/sold"),
            get(orders::trade_sold),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/deduction/open"),
            post(pricing::coin_deduction_open),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/deduction/close"),
            post(pricing::coin_deduction_close),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/properties"),
            post(pricing::item_properties),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/modify_price/render"),
            post(orders::order_modify_price_render),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/adjust_price"),
            post(orders::order_adjust_price),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/red-flower"),
            post(orders::order_red_flower),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/delete"),
            post(orders::order_delete),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/close_reason"),
            get(orders::order_close_reason),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/close"),
            post(orders::order_close),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/consign/render"),
            post(orders::order_consign_render),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/consign"),
            post(orders::order_consign),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/order/consign_dummy"),
            post(orders::order_consign_dummy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/trade/orders"),
            get(orders::trade_orders),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/trade/bill"),
            get(orders::trade_bill),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/coin/deduction/ratio"),
            get(pricing::coin_deduction_ratio),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/publish/preget"),
            post(publish::publish_preget),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/publish/check"),
            get(publish::publish_check),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/publish/badwords"),
            get(publish::publish_badwords),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/publish/capabilities"),
            get(publish::publish_capabilities),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/message/token"),
            post(publish::message_token),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/upload_image"),
            post(upload::upload_item_image),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item/upload_image_url"),
            post(upload::upload_image_url),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/chat/upload_image"),
            post(upload::upload_chat_image),
        )
}

// ─── shared payload structs ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct ItemRefPayload {
    pub(super) uid: String,
    pub(super) item_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct UidQuery {
    pub(super) uid: String,
}

pub(crate) async fn load_location_or_default(state: &AppState, uid: &str) -> LocationContext {
    xianyu_client::load_location(state.storage().accounts(), uid)
        .await
        .unwrap_or_default()
}

pub(super) async fn ensure_market_uid_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid.trim()).await
}

pub(super) async fn ensure_order_belongs_to_uid(
    state: &AppState,
    uid: &str,
    order_id: &str,
) -> Result<(), ResponseError> {
    let found = sqlx::query("SELECT 1 FROM account_orders WHERE uid = ? AND tid = ? LIMIT 1")
        .bind(uid.trim())
        .bind(order_id.trim())
        .fetch_optional(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?
        .is_some();
    if found {
        Ok(())
    } else {
        Err(ResponseError::not_found("订单不存在"))
    }
}

pub(super) async fn ensure_item_belongs_to_uid(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<(), ResponseError> {
    let found = sqlx::query("SELECT 1 FROM account_items WHERE uid = ? AND item_id = ? LIMIT 1")
        .bind(uid.trim())
        .bind(item_id.trim())
        .fetch_optional(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?
        .is_some();
    if found {
        Ok(())
    } else {
        Err(ResponseError::not_found("商品不存在"))
    }
}

// ─── payload / ret helpers ──────────────────────────────────────────────────

pub(super) fn auth_expired_message(raw: &str) -> bool {
    let upper = raw.to_ascii_uppercase();
    upper.contains("SESSION_EXPIRED")
        || upper.contains("USER_NOT_LOGIN")
        || upper.contains("NOT_LOGIN")
        || upper.contains("TOKEN_EXOIRED")
        || upper.contains("TOKEN_EXPIRED")
        || upper.contains("TOKEN_ILLEGAL")
}

fn upstream_data_object(upstream: &Value) -> Map<String, Value> {
    match upstream.get("data") {
        Some(Value::Object(object)) => object.clone(),
        Some(value) => {
            let mut object = Map::new();
            object.insert("rawData".to_string(), value.clone());
            object
        }
        None => Map::new(),
    }
}

pub(super) fn upstream_response_with_api(api: &str, version: &str, upstream: Value) -> Value {
    let mut data = upstream_data_object(&upstream);
    data.insert("api".to_string(), json!(api));
    data.insert("version".to_string(), json!(version));
    json!({
        "ret": upstream.get("ret").cloned().unwrap_or_else(success_ret_value),
        "data": data,
    })
}

pub(super) fn success_message(message: &str) -> Value {
    json!({
        "ret": success_ret(),
        "data": {
            "success": true,
            "message": message,
        }
    })
}

pub(super) fn success_ret() -> [&'static str; 1] {
    ["SUCCESS::调用成功"]
}

pub(super) fn success_ret_value() -> Value {
    json!(success_ret())
}

pub(crate) fn price_to_cents(value: &str) -> String {
    let cents = value
        .trim()
        .parse::<f64>()
        .map(|amount| (amount * 100.0).round() as i64)
        .unwrap_or(0);
    cents.to_string()
}

// ─── value utility functions ────────────────────────────────────────────────

pub(super) fn deep_find_array(root: &Value, keys: &[&str]) -> Option<Value> {
    if keys.is_empty() {
        return None;
    }
    let key = keys[0];
    match root {
        Value::Object(object) => {
            if let Some(value) = object.get(key) {
                return Some(value.clone());
            }
            object
                .values()
                .find_map(|value| deep_find_array(value, keys))
        }
        Value::Array(items) => items.iter().find_map(|value| deep_find_array(value, keys)),
        _ => None,
    }
}

pub(super) fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(if *flag {
            "true".to_string()
        } else {
            "false".to_string()
        }),
        _ => None,
    }
}

pub(super) fn value_to_string_ref(value: &Value) -> String {
    value_to_string(value).unwrap_or_default()
}

pub(super) fn value_to_string_lossy(value: Option<&Value>) -> String {
    value.and_then(value_to_string).unwrap_or_default()
}

pub(super) fn value_is_trueish(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_i64().unwrap_or_default() == 1,
        Value::String(text) => {
            let normalized = text.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true"
        }
        _ => false,
    }
}

pub(super) fn normalize_cent_string(raw: String) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        String::new()
    } else if trimmed.contains('.') {
        price_to_cents(trimmed)
    } else {
        trimmed.to_string()
    }
}

pub(super) fn cent_string_from_value(value: Option<&Value>) -> String {
    normalize_cent_string(value_to_string_lossy(value))
}

pub(super) fn display_price_to_cent_string(value: Option<&Value>) -> String {
    let raw = value_to_string_lossy(value);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        price_to_cents(trimmed)
    }
}

pub(super) fn display_price_string(value: Option<&Value>) -> String {
    let raw = value_to_string_lossy(value);
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "0" {
        return String::new();
    }
    match trimmed.parse::<f64>() {
        Ok(amount) if amount > 0.0 => {
            let normalized = amount.to_string();
            normalized
                .parse::<f64>()
                .map(|n| {
                    if (n.fract()).abs() < f64::EPSILON {
                        format!("{:.0}", n)
                    } else {
                        format!("{:.2}", n)
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                            .to_string()
                    }
                })
                .unwrap_or_else(|_| trimmed.to_string())
        }
        _ => trimmed.to_string(),
    }
}

pub(super) fn bool_from_optional(value: Option<&Value>, default: bool) -> bool {
    value.map(value_is_trueish).unwrap_or(default)
}

pub(super) fn resolve_no_bargain(
    no_bargain: Option<&Value>,
    bargain: Option<&Value>,
    default: bool,
) -> bool {
    if let Some(value) = no_bargain {
        return value_is_trueish(value);
    }
    if let Some(value) = bargain {
        return !value_is_trueish(value);
    }
    default
}

const SIMPLE_ITEM_CATEGORY_KEYWORDS: [&str; 11] = [
    "服务",
    "技能",
    "软件开发",
    "软件资源",
    "定制设计",
    "文案设计",
    "代写",
    "咨询",
    "辅导",
    "翻译",
    "系统安装升级",
];

pub(super) fn infer_simple_item_from_category(category: Option<&Value>) -> bool {
    let Some(category) = category else {
        return false;
    };
    let text = [
        category.get("catName"),
        category.get("channelCatName"),
        category.get("valueName"),
        category.get("text"),
        category.get("name"),
        category.get("level1Name"),
        category.get("level2Name"),
        category.get("level3Name"),
        category.get("level4Name"),
    ]
    .into_iter()
    .filter_map(|value| value.and_then(value_to_string))
    .collect::<Vec<_>>()
    .join(" ");
    if text.trim().is_empty() {
        return false;
    }
    SIMPLE_ITEM_CATEGORY_KEYWORDS
        .iter()
        .any(|keyword| text.contains(keyword))
}

pub(super) fn image_urls_from_value(detail: &Value) -> Vec<String> {
    detail
        .get("imageInfoDOList")
        .or_else(|| detail.get("imageInfos"))
        .or_else(|| detail.get("imageUrls"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|image| match image {
            Value::String(url) => Some(url),
            Value::Object(object) => object
                .get("url")
                .or_else(|| object.get("imageUrl"))
                .and_then(Value::as_str)
                .map(ToString::to_string),
            _ => None,
        })
        .collect()
}

pub(super) fn build_image_info_list(urls: &[String], string_sizes: bool) -> Vec<Value> {
    urls.iter()
        .enumerate()
        .map(|(index, url)| {
            if string_sizes {
                json!({
                    "major": index == 0,
                    "type": 0,
                    "url": url,
                    "widthSize": "0",
                    "heightSize": "0",
                })
            } else {
                // H5 publish/edit 必带 extraInfo（Python _build_publish_payload 行 1410 确认）
                json!({
                    "url": url,
                    "major": index == 0,
                    "type": 0,
                    "widthSize": 0,
                    "heightSize": 0,
                    "extraInfo": {"raw": "true", "isT": "false", "isH": "false"},
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        display_price_string, display_price_to_cent_string, infer_simple_item_from_category,
        resolve_no_bargain,
    };
    use serde_json::json;

    #[test]
    fn display_price_integer_yuan_maps_to_cents() {
        assert_eq!(display_price_to_cent_string(Some(&json!("15"))), "1500");
        assert_eq!(display_price_to_cent_string(Some(&json!("15.00"))), "1500");
        assert_eq!(display_price_to_cent_string(Some(&json!(15))), "1500");
    }

    #[test]
    fn display_price_string_keeps_yuan_not_cents() {
        assert_eq!(display_price_string(Some(&json!("15"))), "15");
        assert_eq!(display_price_string(Some(&json!("15.00"))), "15");
        assert_eq!(display_price_string(Some(&json!("15.50"))), "15.5");
    }

    #[test]
    fn resolve_no_bargain_falls_back_to_inverse_bargain() {
        assert!(resolve_no_bargain(None, Some(&json!(0)), false));
        assert!(!resolve_no_bargain(None, Some(&json!(1)), true));
    }

    #[test]
    fn infer_simple_item_from_category_marks_service_categories() {
        assert!(infer_simple_item_from_category(Some(
            &json!({"catName": "软件开发"})
        )));
        assert!(!infer_simple_item_from_category(Some(
            &json!({"catName": "手机"})
        )));
    }
}

pub(super) fn build_post_fee_dto(
    shipping_type: Option<&str>,
    can_free_shipping: Option<bool>,
    post_price: &str,
    freight_template_id: &str,
    wx_format: bool,
) -> Value {
    let normalized = shipping_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .or_else(|| {
            can_free_shipping.map(|flag| {
                if flag {
                    "free".to_string()
                } else {
                    "fixed".to_string()
                }
            })
        })
        .unwrap_or_else(|| "free".to_string());
    let post_price_cent = price_to_cents(post_price.trim());
    match (wx_format, normalized.as_str()) {
        (true, "self") => json!({
            "onlyTakeSelf": true,
            "postPriceInCent": "",
        }),
        (true, "freight") => json!({
            "supportFreight": true,
            "templateId": freight_template_id.trim(),
            "postPriceInCent": "",
        }),
        (true, "fixed") => json!({
            "postPriceInCent": post_price_cent,
        }),
        (true, _) => json!({
            "postPriceInCent": "",
        }),
        // H5 publish/edit 路径：必须带 idleTemplateId 和 templateId（Python 老版抓包确认）。
        // canFreeShipping=true 同时 supportFreight=true 是闲鱼标准默认（不是矛盾，是"包邮+支持运费模板"）。
        (false, "self") => json!({
            "onlyTakeSelf": true,
            "canFreeShipping": true,
            "supportFreight": false,
            "idleTemplateId": "0",
            "templateId": "0",
            "postPriceInCent": "0",
        }),
        (false, "freight") => json!({
            "onlyTakeSelf": false,
            "canFreeShipping": false,
            "supportFreight": true,
            "idleTemplateId": "0",
            "templateId": freight_template_id.trim(),
            "postPriceInCent": "0",
        }),
        (false, "fixed") => json!({
            "onlyTakeSelf": false,
            "canFreeShipping": false,
            "supportFreight": false,
            "idleTemplateId": "0",
            "templateId": "0",
            "postPriceInCent": post_price_cent,
        }),
        (false, _) => json!({
            "onlyTakeSelf": false,
            "canFreeShipping": true,
            "supportFreight": true,
            "idleTemplateId": "0",
            "templateId": "0",
            "postPriceInCent": "0",
        }),
    }
}

// ─── publish shared helpers (used by publish, publish_kgraph, publish_ai) ────

pub(super) fn category_name_from_value(category: &Value) -> String {
    category
        .get("catName")
        .or_else(|| category.get("channelCatName"))
        .or_else(|| category.get("valueName"))
        .or_else(|| category.get("text"))
        .or_else(|| category.get("name"))
        .map(|value| value_to_string_lossy(Some(value)))
        .unwrap_or_default()
}

pub(super) fn build_image_info_list_h5_full(urls: &[String]) -> Vec<Value> {
    urls.iter()
        .enumerate()
        .map(|(index, url)| {
            json!({
                "url": url,
                "major": index == 0,
                "type": 0,
                "widthSize": 0,
                "heightSize": 0,
                "extraInfo": {
                    "raw": "true",
                    "isT": "false",
                    "isH": "false",
                },
            })
        })
        .collect()
}

pub(super) fn build_h5_publish_labels(cat_info: Option<&Value>) -> Vec<Value> {
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
    vec![json!({
        "channelCateId": channel_cat_id,
        "channelCateName": cat_name,
        "from": "newPublishChoice",
        "isUserClick": "1",
        "labelFrom": "newPublish",
        "labelType": "common",
        "properties": format!("-10000##分类:{}##{}", channel_cat_id, cat_name),
        "propertyId": "-10000",
        "propertyName": "分类",
        "tbCatId": tb_cat_id,
        "text": cat_name,
    })]
}

// ─── publish capability payloads (used by publish + groupon) ────────────────

pub(super) fn publish_capability_post_get_payload(item_id: &str) -> Value {
    let input = json!({
        "bucketId": "16",
        "enterType": "newPublish",
        "flutter": "true",
        "hitNativeDetail": true,
        "isOldFriendly": "false",
        "itemId": item_id,
        "item_id": item_id,
        "needNotPreGet": "true",
        "prefetchRequestKey": format!(
            "{}{}",
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.awesome.detail1.0"),
            unix_millis_now()
        ),
        "publishScene": "mainPublish",
        "sourceId": "main_publish_item",
        "transmitSignal": "1",
        "xDetail": true,
    });
    json!({
        "apiName": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.publish.postGet"),
        "apiVersioin": "1.0",
        "inputJson": input.to_string(),
        "originJson": false,
        "scene": "detailSuccess",
    })
}

pub(super) fn publish_capability_postget_payload(
    item_id: &str,
    location: &LocationContext,
) -> Value {
    let extra = json!({
        "enterType": "newPublish",
        "flutter": true,
        "fromMainPage": true,
        "itemId": item_id,
        "publishScene": "mainPublish",
        "sourceId": "main_publish_item",
        "xDetail": "true",
    });
    let input = json!({
        "itemId": item_id,
        "enterType": "newPublish",
        "bizPageName": "resellPage",
        "scene": "resellPage",
        "extra": extra.to_string(),
    });
    json!({
        "fishAdCode": location.fish_ad_code.clone(),
        "gps": location.gps.clone(),
        "inputJson": input.to_string(),
        "scene": "resellPage",
    })
}

// ─── sync state (shared by items + orders) ──────────────────────────────────

#[derive(Debug, Clone, Default)]
pub(crate) struct SyncOutcome {
    pub ok: bool,
    pub total: i64,
    pub synced_at: f64,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SyncStateRow {
    pub last_synced_at: f64,
    pub last_ok: bool,
    pub last_reason: String,
    pub total_count: i64,
}

pub(crate) async fn read_sync_state(
    pool: &SqlitePool,
    uid: &str,
    kind: &str,
) -> Option<SyncStateRow> {
    sqlx::query(
        r#"
        SELECT last_synced_at, last_ok, last_reason, total_count
        FROM account_sync_state
        WHERE uid = ? AND kind = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .bind(kind)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|row| SyncStateRow {
        last_synced_at: row.get::<f64, _>("last_synced_at"),
        last_ok: row.get::<i64, _>("last_ok") != 0,
        last_reason: row.get::<String, _>("last_reason"),
        total_count: row.get::<i64, _>("total_count"),
    })
}

// ─── database helpers (shared across sub-modules) ───────────────────────────

pub(super) async fn fetch_item(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
        .bind(uid)
        .bind(item_id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(items_sync::item_row_to_value))
}

pub(super) async fn set_item_status(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
    status: &str,
    online: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE account_items SET status = ?, online = ?, updated_at = ? WHERE uid = ? AND item_id = ?",
    )
    .bind(status)
    .bind(if online { 1_i64 } else { 0_i64 })
    .bind(unix_seconds_now())
    .bind(uid)
    .bind(item_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) fn value_str(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

// ─── JSON / string helpers ──────────────────────────────────────────────────

pub(super) fn decode_maybe_json_string(value: &Value) -> Value {
    match value {
        Value::String(text) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| value.clone())
        }
        _ => value.clone(),
    }
}

pub(super) fn json_array_from_text(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Array(Vec::new()))
}

pub(super) fn json_object_from_text(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::Object(Map::new()))
}

// ─── time helpers ───────────────────────────────────────────────────────────

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
