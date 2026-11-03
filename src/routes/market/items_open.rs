use axum::{
    extract::{Extension, Json, State},
    Json as ResponseJson,
};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::time::Duration;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client::LocationContext,
};

use super::items_edit::{build_h5_edit_payload, ItemEditPayload};
use super::{
    bool_from_optional, build_image_info_list, build_post_fee_dto, display_price_to_cent_string,
    ensure_item_belongs_to_uid, ensure_market_uid_access, fetch_item, image_urls_from_value,
    load_location_or_default, payload_ret_success, resolve_no_bargain,
    successful_upstream_h5_request, value_is_trueish, value_to_string_lossy,
};

static REPUBLISH_LOCKS: Lazy<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Struct
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct ItemOpenPayload {
    pub(super) uid: String,
    pub(super) item_id: String,
    #[serde(default)]
    pub(super) input_json: String,
    #[serde(default)]
    pub(super) detail: Value,
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
    pub(super) card_list: Vec<Value>,
    #[serde(default)]
    pub(super) prop_selections: Value,
}

// ---------------------------------------------------------------------------
// Item open payload builder
// ---------------------------------------------------------------------------

fn normalize_item_open_result(mut upstream: Value) -> Value {
    if !payload_ret_success(&upstream) {
        return upstream;
    }
    let item_status = upstream
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("itemStatus"))
        .map(|value| value_to_string_lossy(Some(value)))
        .unwrap_or_default();
    if !item_status.is_empty() && item_status != "0" {
        if let Some(obj) = upstream.as_object_mut() {
            obj.insert(
                "ret".to_string(),
                json!([format!(
                    "FAIL_BIZ_ITEM_REPUBLISH_NOT_ON_SALE::重新发布返回成功，但商品仍处于下架状态（itemStatus={item_status}）"
                )]),
            );
        }
    }
    upstream
}

fn pending_confirmation_result(mut upstream: Value, reason: &str) -> Value {
    if let Some(object) = upstream.as_object_mut() {
        object.insert(
            "ret".to_string(),
            json!([format!(
                "FAIL_BIZ_ITEM_REPUBLISH_PENDING_CONFIRMATION::发布请求已受理，但闲鱼尚未确认商品在售：{reason}"
            )]),
        );
    }
    upstream
}

fn item_is_confirmed_on_sale(item: Option<&Value>) -> bool {
    item.map(|value| {
        value.get("status").and_then(Value::as_str) == Some("ON_SALE")
            && value.get("online").and_then(Value::as_bool) == Some(true)
    })
    .unwrap_or(false)
}

fn normalize_listing_title(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_listing_image(value: &str) -> String {
    let without_suffix = value.trim().split(['?', '#']).next().unwrap_or_default();
    without_suffix
        .strip_prefix("https://")
        .or_else(|| without_suffix.strip_prefix("http://"))
        .unwrap_or(without_suffix)
        .trim_end_matches('/')
        .to_string()
}

fn listing_fingerprint_matches(
    source_title: &str,
    source_image: &str,
    candidate_title: &str,
    candidate_image: &str,
) -> bool {
    let source_title = normalize_listing_title(source_title);
    let source_image = normalize_listing_image(source_image);
    !source_title.is_empty()
        && !source_image.is_empty()
        && source_title == normalize_listing_title(candidate_title)
        && source_image == normalize_listing_image(candidate_image)
}

#[derive(Debug, PartialEq, Eq)]
struct ExistingOnSaleListing {
    item_id: String,
    match_count: usize,
}

async fn find_existing_on_sale_listing(
    pool: &sqlx::SqlitePool,
    uid: &str,
    source_item_id: &str,
) -> Result<Option<ExistingOnSaleListing>, sqlx::Error> {
    let source =
        sqlx::query("SELECT title, pic_url FROM account_items WHERE uid = ? AND item_id = ?")
            .bind(uid)
            .bind(source_item_id)
            .fetch_optional(pool)
            .await?;
    let Some(source) = source else {
        return Ok(None);
    };
    let source_title = source.try_get::<String, _>("title").unwrap_or_default();
    let source_image = source.try_get::<String, _>("pic_url").unwrap_or_default();
    if source_title.trim().is_empty() || source_image.trim().is_empty() {
        return Ok(None);
    }

    let candidates = sqlx::query(
        r#"
        SELECT item_id, title, pic_url
        FROM account_items
        WHERE uid = ? AND item_id <> ? AND status = 'ON_SALE' AND online = 1
        ORDER BY created_at DESC, updated_at DESC, item_id DESC
        "#,
    )
    .bind(uid)
    .bind(source_item_id)
    .fetch_all(pool)
    .await?;

    let mut matched_ids = candidates.into_iter().filter_map(|row| {
        let title = row.try_get::<String, _>("title").unwrap_or_default();
        let image = row.try_get::<String, _>("pic_url").unwrap_or_default();
        listing_fingerprint_matches(&source_title, &source_image, &title, &image)
            .then(|| row.try_get::<String, _>("item_id").unwrap_or_default())
            .filter(|item_id| !item_id.is_empty())
    });
    let Some(item_id) = matched_ids.next() else {
        return Ok(None);
    };
    Ok(Some(ExistingOnSaleListing {
        item_id,
        match_count: 1 + matched_ids.count(),
    }))
}

fn republish_lock(uid: &str, source_item_id: &str) -> Arc<AsyncMutex<()>> {
    let key = format!("{}:{}", uid.trim(), source_item_id.trim());
    let mut locks = REPUBLISH_LOCKS
        .lock()
        .expect("republish lock registry poisoned");
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(AsyncMutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

fn republished_target_id(source_item: &Value) -> Option<String> {
    source_item
        .get("detail")
        .and_then(Value::as_object)
        .and_then(|detail| detail.get("republishedToItemId"))
        .map(|value| value_to_string_lossy(Some(value)))
        .filter(|item_id| !item_id.trim().is_empty())
}

fn already_republished_result(target_item_id: &str) -> Value {
    json!({
        "ret": ["SUCCESS::商品已重新上架"],
        "data": {
            "itemId": target_item_id,
            "itemStatus": "0",
            "alreadyRepublished": true,
        }
    })
}

fn matched_existing_listing_result(target_item_id: &str, match_count: usize) -> Value {
    json!({
        "ret": ["SUCCESS::检测到相同商品已在售，本次未重复发布"],
        "data": {
            "itemId": target_item_id,
            "itemStatus": "0",
            "alreadyRepublished": true,
            "matchedExistingListing": true,
            "existingMatchCount": match_count,
        }
    })
}

async fn existing_republish_result(
    state: &AppState,
    uid: &str,
    source_item_id: &str,
) -> Result<Option<Value>, String> {
    let source = fetch_item(state.storage().xianyu(), uid, source_item_id)
        .await
        .map_err(|error| error.to_string())?;
    let target_item_id = source.as_ref().and_then(republished_target_id);

    let sync = super::sync_items_to_db(state, uid).await?;
    if !sync.ok {
        return Err(format!(
            "当前无法确认账号在售商品状态；为避免重复发布，本次未提交：{}",
            sync.detail
        ));
    }
    if let Some(target_item_id) = target_item_id {
        let target = fetch_item(state.storage().xianyu(), uid, &target_item_id)
            .await
            .map_err(|error| error.to_string())?;
        if item_is_confirmed_on_sale(target.as_ref()) {
            return Ok(Some(already_republished_result(&target_item_id)));
        }
    }

    let existing = find_existing_on_sale_listing(state.storage().xianyu(), uid, source_item_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(existing) = existing {
        mark_republished_source(state, uid, source_item_id, &existing.item_id)
            .await
            .map_err(|error| error.to_string())?;
        tracing::info!(
            target: "market.items_open",
            uid = %uid,
            source_item_id = %source_item_id,
            existing_item_id = %existing.item_id,
            match_count = existing.match_count,
            "republish skipped because matching on-sale listing already exists"
        );
        return Ok(Some(matched_existing_listing_result(
            &existing.item_id,
            existing.match_count,
        )));
    }
    Ok(None)
}

fn confirmation_item_id(upstream: &Value, original_item_id: &str) -> String {
    upstream
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("itemId"))
        .map(|value| value_to_string_lossy(Some(value)))
        .filter(|item_id| !item_id.trim().is_empty())
        .unwrap_or_else(|| original_item_id.to_string())
}

async fn mark_republished_source(
    state: &AppState,
    uid: &str,
    source_item_id: &str,
    published_item_id: &str,
) -> Result<(), sqlx::Error> {
    if source_item_id == published_item_id {
        return Ok(());
    }
    let pool = state.storage().xianyu();
    let row = sqlx::query("SELECT detail_json FROM account_items WHERE uid = ? AND item_id = ?")
        .bind(uid)
        .bind(source_item_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let raw = row.try_get::<String, _>("detail_json").unwrap_or_default();
    let mut detail = super::json_object_from_text(&raw)
        .as_object()
        .cloned()
        .unwrap_or_default();
    detail.insert(
        "republishedToItemId".to_string(),
        Value::String(published_item_id.to_string()),
    );
    sqlx::query("UPDATE account_items SET detail_json = ? WHERE uid = ? AND item_id = ?")
        .bind(Value::Object(detail).to_string())
        .bind(uid)
        .bind(source_item_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn confirm_item_open(state: &AppState, payload: &ItemOpenPayload, upstream: Value) -> Value {
    // publish 的 SUCCESS 只代表请求被受理，management.query 的状态通常会有
    // 1～数秒延迟。立即只查一次会把真实成功误判成失败，也会让前端继续保留
    // 下架缓存。这里只重试“同步 + 查状态”，绝不重复提交 publish。
    const CONFIRM_ATTEMPTS: usize = 3;
    let confirmed_item_id = confirmation_item_id(&upstream, &payload.item_id);
    let mut last_reason = "同步后仍未显示为在售".to_string();
    for attempt in 0..CONFIRM_ATTEMPTS {
        match super::sync_items_to_db(state, &payload.uid).await {
            Ok(sync) if sync.ok => {
                match fetch_item(state.storage().xianyu(), &payload.uid, &confirmed_item_id).await {
                    Ok(item) if item_is_confirmed_on_sale(item.as_ref()) => {
                        if let Err(error) = mark_republished_source(
                            state,
                            &payload.uid,
                            &payload.item_id,
                            &confirmed_item_id,
                        )
                        .await
                        {
                            tracing::warn!(
                                target: "market.items_open",
                                uid = %payload.uid,
                                source_item_id = %payload.item_id,
                                published_item_id = %confirmed_item_id,
                                error = %error,
                                "failed to persist republished item relation"
                            );
                        }
                        return upstream;
                    }
                    Ok(_) => last_reason = "同步后仍未显示为在售".to_string(),
                    Err(_) => last_reason = "无法读取同步后的商品状态".to_string(),
                }
            }
            Ok(sync) => {
                last_reason = if sync.detail.is_empty() {
                    "商品状态同步未完成".to_string()
                } else {
                    sync.detail
                };
            }
            Err(_) => last_reason = "商品状态同步失败".to_string(),
        }

        if attempt + 1 < CONFIRM_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(1_200)).await;
        }
    }
    pending_confirmation_result(upstream, &last_reason)
}

fn edit_payload_from_open(payload: &ItemOpenPayload, detail: Value) -> ItemEditPayload {
    ItemEditPayload {
        uid: payload.uid.clone(),
        item_id: payload.item_id.clone(),
        title: payload.title.clone(),
        short_title: payload.short_title.clone(),
        desc: payload.desc.clone(),
        price: payload.price.clone(),
        orig_price: payload.orig_price.clone(),
        no_bargain: payload.no_bargain,
        can_free_shipping: payload.can_free_shipping,
        shipping_type: payload.shipping_type.clone(),
        post_price: payload.post_price.clone(),
        freight_template_id: payload.freight_template_id.clone(),
        quantity: payload.quantity,
        image_urls: payload.image_urls.clone(),
        detail,
        card_list: payload.card_list.clone(),
        prop_selections: payload.prop_selections.clone(),
    }
}

#[allow(dead_code)]
fn build_item_open_payload(
    item_id: &str,
    detail_data: &Value,
    item_context: &Value,
    location: &LocationContext,
) -> Value {
    // pc.detail 实测返回结构：
    //   data.itemDO.itemCatDTO = {catId, tbCatId, channelCatId, leafId, ...}
    //   data.itemDO.title / desc / soldPrice / originalPrice / quantity / simpleItem / imageInfos
    //   data.sellerDO.city / publishCity（地址不全，需要 item_context 兜底）
    // 类目字段一律从 detail_data 抽，永远不依赖前端 item_context（避免 list
    // 没存类目时上架报 CHANNEL_CAT_ID_PATH_QUERY_ERROR）。
    let item_do = detail_data
        .get(crate::sealed_protocol::sealed_str!("itemDO"))
        .cloned()
        .unwrap_or_else(|| detail_data.clone());
    let context = item_context.as_object().cloned().unwrap_or_default();
    let attrs = context
        .get("attributesMap")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let seller_do = detail_data
        .get("sellerDO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    // 取第一个非空字符串值的辅助闭包
    let first_nonempty = |vals: &[Option<&Value>]| -> String {
        for v in vals.iter().flatten() {
            let s = value_to_string_lossy(Some(v));
            if !s.is_empty() {
                return s;
            }
        }
        String::new()
    };

    // 类目：detail_data.itemDO.itemCatDTO 是真权威源
    let cat_dto = item_do
        .get("itemCatDTO")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let tb_cat_id = first_nonempty(&[
        cat_dto.get("tbCatId"),
        context.get("tbCatId"),
        attrs.get("tbCatId"),
    ]);
    let channel_cat_id = first_nonempty(&[
        cat_dto.get("channelCatId"),
        context.get("channelCatId"),
        attrs.get("cCatId"),
    ]);
    let leaf_id = first_nonempty(&[
        cat_dto.get("leafId"),
        context.get("leafId"),
        attrs.get("lfId"),
        item_do.get("leafId"),
    ]);

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
    let quantity = item_do
        .get("quantity")
        .and_then(Value::as_str)
        .map(String::from)
        .or_else(|| {
            item_do
                .get("quantity")
                .and_then(Value::as_i64)
                .map(|n| n.to_string())
        })
        .unwrap_or_else(|| "1".to_string());
    let simple_item = item_do
        .get("simpleItem")
        .map(value_is_trueish)
        .or_else(|| attrs.get("simItem").map(value_is_trueish))
        .unwrap_or(false);
    let no_bargain = resolve_no_bargain(
        item_do.get("noBargain").or_else(|| attrs.get("noBargain")),
        item_do.get("bargain").or_else(|| attrs.get("bargain")),
        false,
    );

    // 从 detail.cpvLabels 转换 itemLabelExtList — 副业/技能/服务类目要求"预计工期"等
    // 必填属性，空数组会被闲鱼业务层校验拒（"必须选择属性:xxx"）。
    let cpv = item_do
        .get("cpvLabels")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut label_list: Vec<Value> = Vec::new();
    for lbl in cpv {
        let pid = value_to_string_lossy(lbl.get("propertyId"));
        let pname = value_to_string_lossy(lbl.get("propertyName"));
        let vid = value_to_string_lossy(lbl.get("valueId"));
        let vname = value_to_string_lossy(lbl.get("valueName"));
        if pid.is_empty() || vname.is_empty() {
            continue;
        }
        label_list.push(json!({
            "channelCateId": channel_cat_id.clone(),
            "channelCateName": vname.clone(),
            "from": "newPublishChoice",
            "isUserClick": "1",
            "labelFrom": "newPublish",
            "labelType": "common",
            "properties": format!("{pid}##{pname}:{vid}##{vname}"),
            "propertyId": pid,
            "propertyName": pname,
            "tbCatId": tb_cat_id.clone(),
            "text": vname,
        }));
    }

    // 地址：detail_data 不全（只有 sellerDO.city），其余 fallback 到 context / location
    let division_id_ctx =
        first_nonempty(&[context.get("divisionId"), attrs.get("cainiaoDivision")]);
    let division_id = if division_id_ctx.is_empty() {
        location.fish_ad_code.clone()
    } else {
        division_id_ctx
    };
    let city = first_nonempty(&[
        context.get("city"),
        seller_do.get("city"),
        seller_do.get("publishCity"),
    ]);
    let prov = first_nonempty(&[context.get("province"), seller_do.get("province")]);
    let area = first_nonempty(&[context.get("area")]);
    let gps_ctx = first_nonempty(&[context.get("gps")]);
    let gps_str = if gps_ctx.is_empty() {
        location.gps.clone()
    } else {
        gps_ctx
    };
    let address_type = context
        .get("addressType")
        .cloned()
        .unwrap_or_else(|| Value::from(5));

    json!({
        "itemId": item_id,
        "itemTextDTO": {
            "title": title,
            "shortTitle": title.chars().take(20).collect::<String>(),
            "desc": desc,
        },
        "itemPriceDTO": {
            "priceInCent": display_price_to_cent_string(item_do.get("soldPrice").or_else(|| item_do.get("price"))),
            "origPriceInCent": display_price_to_cent_string(item_do.get("originalPrice").or_else(|| item_do.get("origPrice"))),
            "noBargain": no_bargain,
        },
        "itemPostFeeDTO": build_post_fee_dto(
            if bool_from_optional(attrs.get("supportFreight"), false) { Some("freight") } else { None },
            Some(bool_from_optional(attrs.get("freeShipping"), true)),
            "",
            "",
            false,
        ),
        "itemAddrDTO": {
            "divisionId": division_id,
            "city": city,
            "prov": prov,
            "area": area,
            "gps": gps_str,
            "addressType": address_type,
        },
        "itemCatDTO": {
            "tbCatId": tb_cat_id,
            "channelCatId": channel_cat_id,
            "leafId": leaf_id,
        },
        "itemLabelExtList": label_list,
        "imageInfoDOList": build_image_info_list(&image_urls, false),
        "itemTypeStr": "b",
        "itemStatus": "0",
        "simpleItem": if simple_item || bool_from_optional(context.get("simpleItem"), false) { "true" } else { "false" },
        "quantity": quantity,
        "publishScene": "mainPublish",
        "sourceId": "main_publish_item",
        "fishAdCode": location.fish_ad_code,
        "gps": context.get("gps").and_then(Value::as_str).filter(|value| !value.is_empty()).unwrap_or(location.gps.as_str()),
        "latitude": location.latitude,
        "longitude": location.longitude,
    })
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

pub(super) async fn item_open(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemOpenPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let resp = run_item_open_payload_action(&state, &payload)
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(crate) async fn run_item_open_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    _item_context: Value,
) -> Result<Value, String> {
    let payload = ItemOpenPayload {
        uid: uid.to_string(),
        item_id: item_id.to_string(),
        input_json: String::new(),
        detail: Value::Null,
        title: String::new(),
        short_title: String::new(),
        desc: String::new(),
        price: String::new(),
        orig_price: String::new(),
        no_bargain: None,
        can_free_shipping: None,
        shipping_type: None,
        post_price: String::new(),
        freight_template_id: String::new(),
        quantity: None,
        image_urls: None,
        card_list: Vec::new(),
        prop_selections: Value::Null,
    };
    run_item_open_payload_action(state, &payload).await
}

async fn run_item_open_payload_action(
    state: &AppState,
    payload: &ItemOpenPayload,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, &payload.uid, &payload.item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let lock = republish_lock(&payload.uid, &payload.item_id);
    let _guard = lock.lock().await;
    if let Some(result) = existing_republish_result(state, &payload.uid, &payload.item_id).await? {
        return Ok(result);
    }
    let location = load_location_or_default(&state, &payload.uid).await;

    // Path A: 调用方提供 input_json（已构造好的 publish payload）—— 直接打 publish。
    if !payload.input_json.trim().is_empty() {
        let mut input = serde_json::from_str::<Value>(payload.input_json.trim())
            .map_err(|error| format!("input_json 不是合法 JSON: {error}"))?;
        let object = input
            .as_object_mut()
            .ok_or_else(|| "input_json 必须是 JSON 对象".to_string())?;
        if object.get("itemId").is_none() {
            object.insert("itemId".to_string(), Value::String(payload.item_id.clone()));
        }
        if object.get("itemStatus").and_then(Value::as_str) == Some("-2") {
            object.insert("itemStatus".to_string(), Value::String("0".to_string()));
        }
        let upstream = successful_upstream_h5_request(
            &state,
            &payload.uid,
            crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.publish"),
            "1.0",
            input,
            Vec::new(),
        )
        .await
        .map_err(|e| e.to_string())?;
        // 闲鱼返回 SUCCESS 仅表示受理。必须以同步后的商品状态确认真实在售。
        let upstream = normalize_item_open_result(upstream);
        if !payload_ret_success(&upstream) {
            return Ok(upstream);
        }
        return Ok(confirm_item_open(state, payload, upstream).await);
    }

    // 批量、自动任务等调用方可能只带列表摘要。统一补齐与单个编辑弹窗相同的
    // 编辑详情，避免退回到旧的 pc.detail 简化拼装路径而丢失发布必需字段。
    let detail = if payload.detail.is_null() {
        super::items_edit::load_edit_detail_for_action(state, &payload.uid, &payload.item_id)
            .await?
    } else {
        payload.detail.clone()
    };
    let edit_payload = edit_payload_from_open(payload, detail);
    let mut open_payload = build_h5_edit_payload(&edit_payload, &location);
    if let Some(object) = open_payload.as_object_mut() {
        object.insert("itemStatus".to_string(), Value::String("0".to_string()));
    }
    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.publish"),
        "1.0",
        open_payload,
        Vec::new(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let upstream = normalize_item_open_result(upstream);
    if !payload_ret_success(&upstream) {
        return Ok(upstream);
    }
    Ok(confirm_item_open(state, payload, upstream).await)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, Executor};
    use std::sync::Arc;

    use super::{
        already_republished_result, confirmation_item_id, find_existing_on_sale_listing,
        item_is_confirmed_on_sale, listing_fingerprint_matches, pending_confirmation_result,
        republish_lock, republished_target_id, ExistingOnSaleListing,
    };

    #[test]
    fn existing_on_sale_item_with_same_title_and_image_blocks_republish() {
        assert!(listing_fingerprint_matches(
            " 苹果音乐会员兑换码 ",
            "https://img.alicdn.com/item.heic?x-oss-process=image/resize",
            "苹果音乐会员兑换码",
            "http://img.alicdn.com/item.heic",
        ));
    }

    #[test]
    fn same_title_with_different_image_does_not_block_republish() {
        assert!(!listing_fingerprint_matches(
            "苹果音乐会员兑换码",
            "https://img.alicdn.com/first.heic",
            "苹果音乐会员兑换码",
            "https://img.alicdn.com/second.heic",
        ));
    }

    #[tokio::test]
    async fn preflight_finds_all_matching_on_sale_listings_and_chooses_newest() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("create sqlite pool");
        pool.execute(
            r#"
            CREATE TABLE account_items (
                uid TEXT NOT NULL,
                item_id TEXT NOT NULL,
                title TEXT NOT NULL,
                pic_url TEXT NOT NULL,
                status TEXT NOT NULL,
                online INTEGER NOT NULL,
                created_at REAL NOT NULL,
                updated_at REAL NOT NULL
            )
            "#,
        )
        .await
        .expect("create account_items");
        for (item_id, title, image, status, online, created_at) in [
            (
                "source",
                "相同商品",
                "http://img/item.heic",
                "SOLD_OUT",
                0,
                0.0,
            ),
            (
                "live-1",
                "相同商品",
                "https://img/item.heic",
                "ON_SALE",
                1,
                1.0,
            ),
            (
                "live-2",
                "相同商品",
                "http://img/item.heic?resize=1",
                "ON_SALE",
                1,
                2.0,
            ),
            (
                "live-3",
                "相同商品",
                "http://img/item.heic",
                "ON_SALE",
                1,
                3.0,
            ),
            (
                "other-image",
                "相同商品",
                "http://img/other.heic",
                "ON_SALE",
                1,
                4.0,
            ),
            (
                "other-title",
                "其他商品",
                "http://img/item.heic",
                "ON_SALE",
                1,
                5.0,
            ),
        ] {
            sqlx::query("INSERT INTO account_items VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind("uid-1")
                .bind(item_id)
                .bind(title)
                .bind(image)
                .bind(status)
                .bind(online)
                .bind(created_at)
                .bind(created_at)
                .execute(&pool)
                .await
                .expect("insert item");
        }

        assert_eq!(
            find_existing_on_sale_listing(&pool, "uid-1", "source")
                .await
                .expect("find existing listing"),
            Some(ExistingOnSaleListing {
                item_id: "live-3".to_string(),
                match_count: 3,
            })
        );
    }

    #[test]
    fn republished_source_exposes_its_target_item_id() {
        assert_eq!(
            republished_target_id(&json!({
                "detail": { "republishedToItemId": "new-item" }
            })),
            Some("new-item".to_string())
        );
        assert_eq!(republished_target_id(&json!({ "detail": {} })), None);
    }

    #[test]
    fn idempotent_republish_result_is_success_and_returns_existing_target() {
        let result = already_republished_result("new-item");
        assert_eq!(result["ret"][0], "SUCCESS::商品已重新上架");
        assert_eq!(result["data"]["itemId"], "new-item");
        assert_eq!(result["data"]["itemStatus"], "0");
        assert_eq!(result["data"]["alreadyRepublished"], true);
    }

    #[test]
    fn concurrent_republish_requests_for_same_source_share_one_lock() {
        let first = republish_lock("uid-1", "old-item");
        let second = republish_lock("uid-1", "old-item");
        let other = republish_lock("uid-1", "other-item");

        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn republish_confirmation_follows_the_new_item_id_returned_by_xianyu() {
        let upstream = json!({
            "ret": ["SUCCESS::调用成功"],
            "data": { "itemId": "new-item", "itemStatus": "0" }
        });

        assert_eq!(confirmation_item_id(&upstream, "old-item"), "new-item");
    }

    #[test]
    fn republish_confirmation_falls_back_to_original_id_without_a_new_id() {
        assert_eq!(
            confirmation_item_id(&json!({ "ret": ["SUCCESS::调用成功"] }), "old-item"),
            "old-item"
        );
    }

    #[test]
    fn only_online_on_sale_item_confirms_republish() {
        assert!(item_is_confirmed_on_sale(Some(&json!({
            "status": "ON_SALE",
            "online": true,
        }))));
        assert!(!item_is_confirmed_on_sale(Some(&json!({
            "status": "SOLD_OUT",
            "online": false,
        }))));
        assert!(!item_is_confirmed_on_sale(Some(&json!({
            "status": "ON_SALE",
            "online": false,
        }))));
    }

    #[test]
    fn unconfirmed_publish_is_not_reported_as_success() {
        let result = pending_confirmation_result(
            json!({ "ret": ["SUCCESS::调用成功"] }),
            "同步后仍未显示为在售",
        );
        assert_eq!(
            result["ret"][0],
            "FAIL_BIZ_ITEM_REPUBLISH_PENDING_CONFIRMATION::发布请求已受理，但闲鱼尚未确认商品在售：同步后仍未显示为在售"
        );
    }
}
