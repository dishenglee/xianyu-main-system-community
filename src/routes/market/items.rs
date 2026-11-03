use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::{
    deep_find_array, display_price_string, ensure_item_belongs_to_uid, ensure_market_uid_access,
    fetch_item, load_location_or_default, payload_ret_success, read_sync_state, set_item_status,
    success_message, success_ret, success_ret_value, successful_upstream_h5_request,
    unix_seconds_now, value_to_string, value_to_string_lossy, ItemRefPayload,
};

// ---------------------------------------------------------------------------
// Serde defaults
// ---------------------------------------------------------------------------

fn default_page() -> i64 {
    1
}

fn default_page_size_20() -> i64 {
    20
}

fn default_item_tab() -> String {
    "ON_SALE".to_string()
}

fn default_online_sub_tab() -> i64 {
    1
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct ItemListPayload {
    uid: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size_20")]
    page_size: i64,
    #[serde(default = "default_item_tab")]
    tab: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemManagementLivePayload {
    uid: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size_20")]
    page_size: i64,
    #[serde(default = "default_online_sub_tab")]
    online_sub_tab: i64,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemPolishQuery {
    uid: String,
    #[serde(default)]
    item_id: String,
}

// ---------------------------------------------------------------------------
// Item list helpers
// ---------------------------------------------------------------------------

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

fn extract_share_payload(detail: &Value, item_id: &str) -> Option<Value> {
    let item_do = detail
        .get(crate::sealed_protocol::sealed_str!("itemDO"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let fallback_url = format!(
        "{}{}",
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/item?id="),
        item_id
    );
    let mut share_url = String::new();
    for source in [
        item_do.get("shareData").cloned().unwrap_or(Value::Null),
        detail.get("shareData").cloned().unwrap_or(Value::Null),
    ] {
        let Some(object) = source.as_object() else {
            continue;
        };
        let Some(info_str) = object.get("shareInfoJsonString").and_then(Value::as_str) else {
            continue;
        };
        let Ok(info) = serde_json::from_str::<Value>(info_str) else {
            continue;
        };
        let Some(url) = info.get("url").and_then(Value::as_str) else {
            continue;
        };
        if url.starts_with("http") {
            share_url = url.to_string();
            break;
        }
    }
    if share_url.is_empty() {
        share_url = fallback_url;
    }

    let title = item_do
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .chars()
        .take(60)
        .collect::<String>();
    let price_display = display_price_string(
        item_do
            .get("soldPrice")
            .or_else(|| item_do.get("price"))
            .or_else(|| item_do.get("defaultPrice")),
    );

    let mut parts = vec!["【闲鱼】".to_string()];
    if !title.is_empty() {
        parts.push(format!("「{}」", title));
    }
    if !price_display.is_empty() {
        parts.push(format!("¥{price_display}"));
    }
    parts.push(share_url.clone());

    Some(json!({
        "url": share_url,
        "text": parts.join(" "),
        "title": title,
        "price": if price_display.is_empty() { String::new() } else { format!("¥{price_display}") },
    }))
}

pub(super) fn parse_item_list_from_upstream(raw: &Value) -> Vec<Value> {
    let data_obj = raw.get("data").cloned().unwrap_or(Value::Null);
    let raw_list = data_obj
        .get("itemCards")
        .cloned()
        .or_else(|| deep_find_array(raw, &["itemCards", "itemList", "items"]))
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();

    raw_list
        .into_iter()
        .filter_map(|item| {
            let object = item.as_object()?.clone();
            let image_urls = object
                .get("imageUrls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let pic = object
                .get("picUrl")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .or_else(|| {
                    image_urls
                        .first()
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .unwrap_or_default();
            let bubble = object
                .get("bubbleInfo")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let price_tags = object
                .get("priceTags")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let seller_promotion = object
                .get("sellerPromotionVO")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let coin_suffix = price_tags
                .get("priceSuffixLabelInfoDO")
                .and_then(Value::as_object)
                .and_then(|info| info.get("text"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let coin_ratio = coin_ratio_from_text(&coin_suffix);
            let coin_open_hint = bubble.get("iconTag").and_then(Value::as_str)
                == Some("coinDeduction")
                && !coin_ratio.is_empty();
            let groupon_price =
                super::value_to_string_lossy(seller_promotion.get("activityPriceOrDiscount"));
            let groupon_status = seller_promotion
                .get("promotionStatus")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let groupon_open = !groupon_price.is_empty() && groupon_status != 0;
            // 上游 cardData.itemStatus 是数字：0=正常在售，非 0 通常代表
            // 审核中 / 限时下架 / 系统拦截等特殊状态。我们把它原样塞进 parsed
            // object 两次：
            //   "status"      用于历史兼容（旧前端按 0/非 0 判定 online 状态）；
            //   "itemStatus"  让 items_sync.rs 的 detail_map 能把它写进 detail_json，
            //                  否则审核中标识在落库后被丢掉，前端 ItemCard
            //                  按 detail.itemStatus 判定的"审核中"角标永远不亮。
            let item_status_raw = object.get("itemStatus").cloned().unwrap_or_else(|| Value::from(0));
            Some(json!({
                "itemId": object.get("id").or_else(|| object.get("itemId")).and_then(value_to_string).unwrap_or_default(),
                "title": object.get("title").and_then(Value::as_str).unwrap_or_default(),
                "price": value_to_string_lossy(object.get("price")),
                "originalPrice": value_to_string_lossy(object.get("originalPrice")),
                "picUrl": pic,
                "status": item_status_raw.clone(),
                "itemStatus": item_status_raw,
                "online": object.get("online").cloned().unwrap_or_else(|| Value::Bool(false)),
                "viewCount": object.get("browseCount").cloned().unwrap_or_else(|| Value::from(0)),
                // 闲鱼上游 ON_SALE 的"想要数"实际叫 collectNum，老代码用了 wantNum 永远 miss。
                "wantCount": object.get("wantNum").or_else(|| object.get("collectNum")).cloned().unwrap_or_else(|| Value::from(0)),
                "collectNum": object.get("collectNum").cloned().unwrap_or_else(|| Value::from(0)),
                "exposureNum": value_to_string_lossy(object.get("exposureNum")),
                "city": object.get("city").and_then(Value::as_str).unwrap_or_default(),
                "province": object.get("province").and_then(Value::as_str).unwrap_or_default(),
                "area": object.get("area").and_then(Value::as_str).unwrap_or_default(),
                "divisionId": value_to_string_lossy(object.get("divisionId")),
                "categoryId": value_to_string_lossy(object.get("categoryId")),
                // 闲鱼有时把时间下发为数字毫秒（不是字符串），用 lossy 兼容两种形态。
                "gmtCreated": value_to_string_lossy(object.get("gmtCreated")),
                "gmtCreate": value_to_string_lossy(object.get("gmtCreate").or_else(|| object.get("createTime"))),
                "firstModifiedDiff": object.get("firstModifiedDiff").and_then(Value::as_str).unwrap_or_default(),
                // 服务端下发的可用操作列表（actionType / actionName / actionUrl 加密 token / enable）。
                // 透传到前端做 hover 按钮动态渲染。抓包样本：
                //   actionType 2=粉丝价 3=降价 4=编辑 7=分享 11=下架 13=智能回复 17=2人小刀 22=闲鱼币抵扣
                "actions": object.get("actions").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                "coinOpen": coin_open_hint,
                "coinSuffix": coin_suffix,
                "coinRatio": coin_ratio,
                "grouponOpen": groupon_open,
                "grouponPrice": groupon_price,
                "sellerPromotionVO": if seller_promotion.is_empty() { Value::Null } else { Value::Object(seller_promotion) },
            }))
        })
        .collect()
}

fn coin_ratio_from_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Some(percent_index) = trimmed.find('%') {
        let before_percent = &trimmed[..percent_index];
        if let Some(value) = last_ratio_candidate(before_percent) {
            return value;
        }
    }

    if trimmed.contains('币') || trimmed.contains("抵扣") {
        return first_ratio_candidate(trimmed).unwrap_or_default();
    }

    String::new()
}

fn last_ratio_candidate(text: &str) -> Option<String> {
    let mut last = None;
    for candidate in ratio_candidates(text) {
        last = Some(candidate);
    }
    last
}

fn first_ratio_candidate(text: &str) -> Option<String> {
    ratio_candidates(text).into_iter().next()
}

fn ratio_candidates(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
            continue;
        }
        push_ratio_candidate(&mut out, &mut buf);
    }
    push_ratio_candidate(&mut out, &mut buf);
    out
}

pub(crate) fn extract_pc_detail_quantity(upstream: &Value) -> Option<i64> {
    upstream
        .get("data")?
        .get(crate::sealed_protocol::sealed_str!("itemDO"))?
        .get("quantity")
        .and_then(non_negative_integer)
}

pub(crate) fn extract_edit_detail_quantity(detail: &Value) -> Option<i64> {
    detail
        .get("quantity")
        .or_else(|| {
            detail
                .get(crate::sealed_protocol::sealed_str!("itemDO"))
                .and_then(|item| item.get("quantity"))
        })
        .or_else(|| detail.get("itemInfo").and_then(|item| item.get("quantity")))
        .and_then(non_negative_integer)
}

fn non_negative_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().filter(|quantity| *quantity >= 0),
        Value::String(text)
            if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            text.parse::<i64>().ok()
        }
        _ => None,
    }
}

async fn persist_item_quantity_from_upstream(
    pool: &sqlx::SqlitePool,
    uid: &str,
    item_id: &str,
    upstream: &Value,
    synced_at: i64,
) -> Result<Option<i64>, sqlx::Error> {
    let quantity = extract_pc_detail_quantity(upstream);
    let source = if quantity.is_some() {
        crate::sealed_protocol::sealed_str!("pc.detail:data.itemDO.quantity")
    } else {
        ""
    };
    let result = sqlx::query(
        r#"
        UPDATE account_items
        SET quantity = ?, quantity_source = ?, quantity_synced_at = ?
        WHERE uid = ? AND item_id = ?
        "#,
    )
    .bind(quantity)
    .bind(source)
    .bind(synced_at)
    .bind(uid)
    .bind(item_id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(quantity)
}

async fn persist_item_quantity_snapshot(
    pool: &sqlx::SqlitePool,
    uid: &str,
    item_id: &str,
    quantity: i64,
    source: &str,
    synced_at: i64,
) -> Result<(), sqlx::Error> {
    let result = sqlx::query(
        r#"
        UPDATE account_items
        SET quantity = ?, quantity_source = ?, quantity_synced_at = ?
        WHERE uid = ? AND item_id = ?
        "#,
    )
    .bind(quantity)
    .bind(source)
    .bind(synced_at)
    .bind(uid)
    .bind(item_id)
    .execute(pool)
    .await?;
    if result.rows_affected() != 1 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum RefreshItemQuantityError {
    Upstream(String),
    Persistence(sqlx::Error),
}

pub(crate) async fn refresh_item_quantity_from_upstream(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<(Value, Option<i64>), RefreshItemQuantityError> {
    let upstream = successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
        "1.0",
        json!({
            "itemId": item_id,
        }),
        Vec::new(),
    )
    .await
    .map_err(RefreshItemQuantityError::Upstream)?;
    let quantity = persist_item_quantity_from_upstream(
        state.storage().xianyu(),
        uid,
        item_id,
        &upstream,
        unix_seconds_now() as i64,
    )
    .await
    .map_err(RefreshItemQuantityError::Persistence)?;
    Ok((upstream, quantity))
}

pub(crate) async fn refresh_item_quantity_with_edit_fallback(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Option<i64>, RefreshItemQuantityError> {
    let mut failures = Vec::new();
    for (source, result) in [
        (
            "pc.editdetail:quantity",
            super::items_edit::fetch_pc_backend_edit_detail(state, uid, item_id).await,
        ),
        (
            "wx.editdetail:quantity",
            super::items_edit::fetch_wx_edit_detail(state, uid, item_id).await,
        ),
    ] {
        match result {
            Ok(detail) => {
                if let Some(quantity) = extract_edit_detail_quantity(&detail) {
                    persist_item_quantity_snapshot(
                        state.storage().xianyu(),
                        uid,
                        item_id,
                        quantity,
                        source,
                        unix_seconds_now() as i64,
                    )
                    .await
                    .map_err(RefreshItemQuantityError::Persistence)?;
                    return Ok(Some(quantity));
                }
                failures.push(format!("{source} 未返回库存"));
            }
            Err(error) => failures.push(format!("{source}: {error}")),
        }
    }

    match refresh_item_quantity_from_upstream(state, uid, item_id).await {
        Ok((_, quantity)) => Ok(quantity),
        Err(RefreshItemQuantityError::Upstream(error)) => {
            failures.push(format!("pc.detail: {error}"));
            Err(RefreshItemQuantityError::Upstream(failures.join(" | ")))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_item_list_from_upstream;
    use serde_json::json;

    #[test]
    fn parse_item_list_does_not_mark_coin_open_when_suffix_has_no_ratio() {
        let upstream = json!({
            "data": {
                "itemCards": [{
                    "id": "item-coin-badge-only",
                    "title": "Badge only",
                    "price": "12.00",
                    "online": true,
                    "bubbleInfo": {
                        "iconTag": "coinDeduction"
                    },
                    "priceTags": {
                        "priceSuffixLabelInfoDO": {
                            "text": "+3600币"
                        }
                    }
                }]
            }
        });

        let items = parse_item_list_from_upstream(&upstream);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["coinOpen"], false);
        assert_eq!(items[0]["coinRatio"], "");
    }

    #[test]
    fn parse_item_list_marks_coin_open_when_suffix_has_ratio() {
        let upstream = json!({
            "data": {
                "itemCards": [{
                    "id": "item-coin-open",
                    "title": "Coin open",
                    "price": "12.00",
                    "online": true,
                    "bubbleInfo": {
                        "iconTag": "coinDeduction"
                    },
                    "priceTags": {
                        "priceSuffixLabelInfoDO": {
                            "text": "闲鱼币抵扣20%"
                        }
                    }
                }]
            }
        });

        let items = parse_item_list_from_upstream(&upstream);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["coinOpen"], true);
        assert_eq!(items[0]["coinRatio"], "20");
    }
}

fn push_ratio_candidate(out: &mut Vec<String>, buf: &mut String) {
    let raw = buf.trim_matches('.');
    if raw.is_empty() {
        buf.clear();
        return;
    }
    if let Ok(value) = raw.parse::<f64>() {
        if value > 0.0 && value <= 100.0 {
            let normalized = if value.fract() == 0.0 {
                format!("{}", value as i64)
            } else {
                let mut s = format!("{value:.2}");
                while s.ends_with('0') {
                    s.pop();
                }
                if s.ends_with('.') {
                    s.pop();
                }
                s
            };
            out.push(normalized);
        }
    }
    buf.clear();
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub(super) async fn item_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    match refresh_item_quantity_from_upstream(&state, &payload.uid, &payload.item_id).await {
        Ok((upstream, _)) => return Ok(ResponseJson(upstream)),
        Err(RefreshItemQuantityError::Upstream(error)) => {
            tracing::warn!(
                uid = %payload.uid,
                item_id = %payload.item_id,
                error = %error,
                "商品详情上游刷新失败，回退本地缓存"
            );
        }
        Err(RefreshItemQuantityError::Persistence(error)) => {
            tracing::error!(
                uid = %payload.uid,
                item_id = %payload.item_id,
                error = %error,
                "商品详情上游成功后库存持久化失败"
            );
            return Err(ResponseError::internal_message(
                "商品详情已获取，但库存同步失败，请稍后重试",
            ));
        }
    }
    let item = fetch_item(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("商品不存在"))?;
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": item,
    })))
}

pub(super) async fn item_share_link(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
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
        if let Some(share) = extract_share_payload(
            upstream.get("data").unwrap_or(&Value::Null),
            &payload.item_id,
        ) {
            return Ok(ResponseJson(share));
        }
    }
    let item = fetch_item(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("商品不存在"))?;
    let share_link = format!(
        "{}{}",
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/item?id="),
        payload.item_id
    );
    Ok(ResponseJson(json!({
        "url": share_link,
        "text": format!("【闲鱼】「{}」 {}", item["title"].as_str().unwrap_or("商品"), share_link),
        "title": item["title"].as_str().unwrap_or("商品"),
        "price": item["price"].as_str().unwrap_or(""),
    })))
}

pub(super) async fn list_items(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemListPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let tab = payload.tab.trim().to_ascii_uppercase();
    // Force refresh: sync upstream into DB before reading.
    if payload.force {
        let _ = super::items_sync::sync_items_to_db(&state, &payload.uid).await;
    }

    let page_size = payload.page_size.clamp(1, 100);
    let page = payload.page.max(1);
    let offset = ((page - 1) * page_size).max(0);
    let items = super::items_sync::query_items_by_uid(
        state.storage().xianyu(),
        &payload.uid,
        tab.as_str(),
        page_size,
        offset,
    )
    .await
    .map_err(ResponseError::internal)?;

    let sync_state = read_sync_state(state.storage().xianyu(), &payload.uid, "items").await;
    let total = sync_state
        .as_ref()
        .map(|s| s.total_count)
        .unwrap_or(items.len() as i64);
    let has_more = (offset + items.len() as i64) < total;

    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "_cached": true,
        "_cached_at": unix_seconds_now(),
        "_last_synced_at": sync_state.as_ref().map(|s| s.last_synced_at).unwrap_or(0.0),
        "_last_sync_ok": sync_state.as_ref().map(|s| s.last_ok).unwrap_or(false),
        "_last_sync_reason": sync_state.as_ref().map(|s| s.last_reason.clone()).unwrap_or_default(),
        "data": {
            "itemList": items,
            "totalCount": total,
            "hasMore": has_more,
        }
    })))
}

pub(super) async fn items_management_live(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemManagementLivePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let page = payload.page.max(1);
    let page_size = payload.page_size.clamp(1, 100);
    let api = crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.management.query");
    let version = "1.0";
    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        api,
        version,
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "latitude": location.latitude,
            "longitude": location.longitude,
            "auctionTypes": ["b", "d", "a"],
            "newVersion": "2.0",
            "onlineSubTab": payload.online_sub_tab,
            "pageNumber": page,
            "rowsPerPage": page_size,
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;

    let mut data = upstream_data_object(&upstream);
    data.insert("api".to_string(), json!(api));
    data.insert("version".to_string(), json!(version));
    data.insert("page".to_string(), json!(page));
    data.insert("pageSize".to_string(), json!(page_size));
    data.insert(
        "itemList".to_string(),
        Value::Array(parse_item_list_from_upstream(&upstream)),
    );
    Ok(ResponseJson(json!({
        "ret": upstream.get("ret").cloned().unwrap_or_else(success_ret_value),
        "data": data,
    })))
}

pub(super) async fn item_polish(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ItemPolishQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let payload = run_item_polish_action(&state, &query.uid, query.item_id.trim())
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(payload))
}

pub(crate) async fn run_item_polish_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    if !item_id.trim().is_empty() {
        ensure_item_belongs_to_uid(state, uid, item_id.trim())
            .await
            .map_err(|error| error.to_string())?;
    }
    let location = load_location_or_default(state, uid).await;
    let mut payload = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "latitude": location.latitude,
        "longitude": location.longitude,
        "doTask": true,
    });
    if !item_id.trim().is_empty() {
        if let Some(object) = payload.as_object_mut() {
            object.insert("itemId".to_string(), json!(item_id.trim()));
        }
    }
    if let Ok(payload) = successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.polish"),
        "2.0",
        payload,
        Vec::new(),
    )
    .await
    {
        return Ok(payload);
    }
    Ok(json!({
        "ret": success_ret(),
        "data": {
            "success": true,
            "message": "擦亮成功",
        }
    }))
}

pub(super) async fn item_delete(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let resp = run_item_delete_action(&state, &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(super) async fn item_downshelf(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let resp = run_item_downshelf_action(&state, &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(crate) async fn run_item_delete_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    if let Ok(upstream) = successful_upstream_h5_request(
        state,
        uid,
        "com.taobao.idle.item.delete",
        "1.1",
        json!({
            "itemId": item_id,
        }),
        Vec::new(),
    )
    .await
    {
        let _ = sqlx::query("DELETE FROM account_items WHERE uid = ? AND item_id = ?")
            .bind(uid)
            .bind(item_id)
            .execute(state.storage().xianyu())
            .await;
        return Ok(upstream);
    }
    sqlx::query("DELETE FROM account_items WHERE uid = ? AND item_id = ?")
        .bind(uid)
        .bind(item_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(|e| e.to_string())?;
    Ok(success_message("删除成功"))
}

pub(crate) async fn run_item_downshelf_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    match run_item_downshelf_upstream_action(state, uid, item_id).await {
        Ok(upstream) => return Ok(upstream),
        Err(_) => {}
    }
    set_item_status(state.storage().xianyu(), uid, item_id, "SOLD_OUT", false)
        .await
        .map_err(|e| e.to_string())?;
    Ok(success_message("下架成功"))
}

pub(crate) async fn run_item_downshelf_upstream_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let location = load_location_or_default(state, uid).await;
    let upstream = successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.downshelf"),
        "2.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "latitude": location.latitude,
            "longitude": location.longitude,
            "itemId": item_id,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| error.to_string())?;
    set_item_status(state.storage().xianyu(), uid, item_id, "SOLD_OUT", false)
        .await
        .map_err(|e| e.to_string())?;
    Ok(upstream)
}

pub(super) async fn item_collect(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemRefPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    if let Ok(upstream) = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.collect.item"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "latitude": location.latitude,
            "longitude": location.longitude,
            "itemId": payload.item_id.clone(),
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "success": true,
            "itemId": payload.item_id,
            "uid": payload.uid,
        }
    })))
}

#[cfg(test)]
mod quantity_tests {
    use super::{
        extract_edit_detail_quantity, extract_pc_detail_quantity,
        persist_item_quantity_from_upstream,
    };
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, Row};

    #[test]
    fn pc_detail_quantity_accepts_non_negative_integers_and_digit_strings() {
        assert_eq!(
            extract_pc_detail_quantity(&json!({"data": {"itemDO": {"quantity": 7}}})),
            Some(7)
        );
        assert_eq!(
            extract_pc_detail_quantity(&json!({"data": {"itemDO": {"quantity": "7"}}})),
            Some(7)
        );
        assert_eq!(
            extract_pc_detail_quantity(&json!({"data": {"itemDO": {"quantity": 0}}})),
            Some(0)
        );
    }

    #[test]
    fn pc_detail_quantity_rejects_missing_invalid_and_non_authoritative_values() {
        for upstream in [
            json!({"data": {"itemDO": {}}}),
            json!({"data": {"itemDO": {"quantity": -1}}}),
            json!({"data": {"itemDO": {"quantity": 1.5}}}),
            json!({"data": {"itemDO": {"quantity": "1.5"}}}),
            json!({"data": {"quantity": 9, "itemDO": {}}}),
            json!({"quantity": "1", "data": {"itemDO": {}}}),
        ] {
            assert_eq!(extract_pc_detail_quantity(&upstream), None, "{upstream}");
        }
    }

    #[test]
    fn edit_detail_quantity_supports_pc_and_wx_response_shapes() {
        for (detail, expected) in [
            (json!({"quantity": "8"}), Some(8)),
            (json!({"itemDO": {"quantity": 5}}), Some(5)),
            (json!({"itemInfo": {"quantity": "3"}}), Some(3)),
            (json!({"quantity": "1.5"}), None),
            (json!({"quantity": -1}), None),
        ] {
            assert_eq!(extract_edit_detail_quantity(&detail), expected, "{detail}");
        }
    }

    #[tokio::test]
    async fn successful_detail_without_trusted_quantity_clears_snapshot() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE account_items (
                uid TEXT NOT NULL,
                item_id TEXT NOT NULL,
                quantity INTEGER,
                quantity_source TEXT,
                quantity_synced_at INTEGER,
                PRIMARY KEY (uid, item_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create account_items");
        sqlx::query(
            "INSERT INTO account_items (uid, item_id, quantity, quantity_source, quantity_synced_at) VALUES (?, ?, 7, 'trusted', 100)",
        )
        .bind("uid-1")
        .bind("item-1")
        .execute(&pool)
        .await
        .expect("seed quantity");

        let quantity = persist_item_quantity_from_upstream(
            &pool,
            "uid-1",
            "item-1",
            &json!({"data": {"itemDO": {}, "quantity": 1}}),
            200,
        )
        .await
        .expect("persist unknown quantity");

        assert_eq!(quantity, None);
        let row = sqlx::query(
            "SELECT quantity, quantity_source, quantity_synced_at FROM account_items WHERE uid = ? AND item_id = ?",
        )
        .bind("uid-1")
        .bind("item-1")
        .fetch_one(&pool)
        .await
        .expect("fetch quantity");
        assert_eq!(row.get::<Option<i64>, _>("quantity"), None);
        assert_eq!(
            row.get::<Option<String>, _>("quantity_source"),
            Some(String::new())
        );
        assert_eq!(row.get::<Option<i64>, _>("quantity_synced_at"), Some(200));
    }

    #[tokio::test]
    async fn persist_item_quantity_errors_when_item_row_is_missing() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE account_items (
                uid TEXT NOT NULL,
                item_id TEXT NOT NULL,
                quantity INTEGER,
                quantity_source TEXT,
                quantity_synced_at INTEGER,
                PRIMARY KEY (uid, item_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create account_items");

        let result = persist_item_quantity_from_upstream(
            &pool,
            "uid-missing",
            "item-missing",
            &json!({"data": {"itemDO": {"quantity": 7}}}),
            200,
        )
        .await;

        assert!(
            matches!(result, Err(sqlx::Error::RowNotFound)),
            "zero-row update must fail with RowNotFound, got {result:?}"
        );
    }
}
