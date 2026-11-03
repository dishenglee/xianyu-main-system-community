use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{Row, SqlitePool};
use tracing::debug;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::{
    deep_find_array, ensure_item_belongs_to_uid, ensure_market_uid_access, fetch_item,
    load_location_or_default, payload_ret_success, success_message, success_ret,
    successful_upstream_h5_request, unix_seconds_now, upstream_h5_request,
    upstream_response_with_api, value_to_string, value_to_string_lossy, value_to_string_ref,
};

// ── serde defaults ────────────────────────────────────────────────────────────

fn default_coin_ratio_price() -> String {
    "100".to_string()
}

fn default_coin_deduction_ratio() -> String {
    "20".to_string()
}

fn default_coin_deduction_channel() -> String {
    "myPublish".to_string()
}

fn is_not_agree_deduction_error(raw: &str) -> bool {
    raw.contains("NOT_AGREE_DEDUCTION") || raw.contains("未同意抵扣协议")
}

async fn coin_deduction_agree_change(
    state: &AppState,
    uid: &str,
    fish_ad_code: &str,
    gps: &str,
) -> Result<Value, String> {
    let mut data = Map::new();
    data.insert("agreeFlag".to_string(), json!(1));
    if !fish_ad_code.trim().is_empty() {
        data.insert("fishAdCode".to_string(), json!(fish_ad_code));
    }
    if !gps.trim().is_empty() {
        data.insert("gps".to_string(), json!(gps));
    }
    upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.agree.change"),
        "1.0",
        Value::Object(data),
        Vec::new(),
    )
    .await
}

fn is_coin_close_t1_limit_error(raw: &str) -> bool {
    raw.contains("FAIL_BIZ_SINGLE_CLOSE_T1_LIMIT")
        || raw.contains("次日操作关闭")
        || raw.contains("24 小时后才可关闭")
}

// ── request payload structs ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(super) struct BargainPriceRangePayload {
    uid: String,
    #[serde(default)]
    price: String,
}

// helper 直传 number（reducePrice(uid, itemId, amount: number)），原本严格
// String 反序列化会 422，这里两种都接。
#[derive(Debug, Deserialize)]
pub(super) struct ReducePricePayload {
    uid: String,
    item_id: String,
    amount: Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReducePriceFeedsPayload {
    pub(super) uid: String,
    pub(super) item_id: String,
    #[serde(default)]
    pub(super) suggest_price: String,
    #[serde(default)]
    pub(super) price_key: String,
    #[serde(default)]
    pub(super) exts: Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct BatchReducePricePayload {
    uid: String,
    item_ids: Vec<String>,
    amount: Value,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoinDeductionPayload {
    uid: String,
    item_ids: Vec<String>,
    #[serde(default = "default_coin_deduction_ratio")]
    ratio: String,
    #[serde(default = "default_coin_deduction_channel")]
    channel: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoinRatioQuery {
    uid: String,
    #[serde(default = "default_coin_ratio_price")]
    price: String,
    #[serde(default)]
    item_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ItemPropertiesPayload {
    uid: String,
    #[serde(default)]
    item_id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    channel_cat_id: String,
}

// ── helper functions ──────────────────────────────────────────────────────────

pub(super) fn price_to_cents(value: &str) -> String {
    let cents = value
        .trim()
        .parse::<f64>()
        .map(|amount| (amount * 100.0).round() as i64)
        .unwrap_or(0);
    cents.to_string()
}

fn amount_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn price_text_to_cents(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = trimmed.parse::<f64>().ok()?;
    if parsed <= 0.0 {
        return None;
    }
    Some((parsed * 100.0).round() as i64)
}

fn cents_to_price_text(cents: i64) -> String {
    format!("{:.2}", cents as f64 / 100.0)
}

fn markdown_target_price(
    item_price: &str,
    amount: &Value,
) -> Result<(String, String), ResponseError> {
    let current_cents = price_text_to_cents(item_price)
        .ok_or_else(|| ResponseError::bad_request("未找到商品当前售价，无法换算降价后的目标价"))?;
    let amount_str = amount_to_string(amount);
    let markdown_cents = price_text_to_cents(amount_str.trim())
        .ok_or_else(|| ResponseError::bad_request("降价金额无效"))?;
    if markdown_cents >= current_cents {
        return Err(ResponseError::bad_request(format!(
            "降价金额不能大于或等于售价，当前售价 ¥{}",
            cents_to_price_text(current_cents)
        )));
    }
    let target_cents = current_cents - markdown_cents;
    if target_cents <= 0 {
        return Err(ResponseError::bad_request(format!(
            "降价后价格必须大于 0，当前售价 ¥{}",
            cents_to_price_text(current_cents)
        )));
    }
    Ok((cents_to_price_text(target_cents), target_cents.to_string()))
}

fn reduce_feed_exts(payload: &ReducePriceFeedsPayload, item: Option<&Value>) -> Value {
    match &payload.exts {
        Value::Array(items) if !items.is_empty() => Value::Array(items.clone()),
        Value::String(text) if !text.trim().is_empty() => {
            serde_json::from_str(text.trim()).unwrap_or_else(|_| Value::Array(Vec::new()))
        }
        _ => item
            .and_then(|value| deep_find_array(value, &["itemLabelExtList"]))
            .filter(|value| value.as_array().is_some())
            .unwrap_or_else(|| Value::Array(Vec::new())),
    }
}

fn reduce_feed_price_key(exts: &Value) -> String {
    let Some(items) = exts.as_array() else {
        return String::new();
    };
    let mut parts = Vec::new();
    for item in items {
        let object = item.as_object().cloned().unwrap_or_default();
        let property_id = object
            .get("propertyId")
            .map(value_to_string_ref)
            .unwrap_or_default();
        let value_id = object
            .get("valueId")
            .map(value_to_string_ref)
            .unwrap_or_default();
        let channel_cat_id = object
            .get("channelCateId")
            .or_else(|| object.get("channelCatId"))
            .map(value_to_string_ref)
            .unwrap_or_default();
        if property_id == "-10000" && !channel_cat_id.is_empty() {
            parts.push(format!("channel_leaf_cate_id:{channel_cat_id}"));
        } else if !property_id.is_empty() && !value_id.is_empty() {
            parts.push(format!("{property_id}:{value_id}"));
        }
        if parts.len() >= 4 {
            break;
        }
    }
    parts.join(";")
}

fn upstream_data_result_false(payload: &Value) -> bool {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("result"))
        .and_then(Value::as_bool)
        == Some(false)
}

fn upstream_data_message(payload: &Value) -> String {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| {
            data.get("msg")
                .or_else(|| data.get("message"))
                .or_else(|| data.get("errorMsg"))
                .or_else(|| data.get("errorMessage"))
        })
        .map(value_to_string_ref)
        .unwrap_or_default()
}

// ── DB helpers ────────────────────────────────────────────────────────────────

async fn update_item_price(
    pool: &SqlitePool,
    uid: &str,
    item_id: &str,
    price: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE account_items SET price = ?, updated_at = ? WHERE uid = ? AND item_id = ?")
        .bind(price)
        .bind(unix_seconds_now())
        .bind(uid)
        .bind(item_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn update_coin_deduction_state(
    pool: &SqlitePool,
    uid: &str,
    item_ids: &[String],
    coin_open: bool,
    coin_ratio: &str,
) -> Result<(), sqlx::Error> {
    let now = unix_seconds_now();
    for item_id in item_ids {
        if coin_open {
            let result = sqlx::query(
                "UPDATE account_items
                 SET coin_open = 1,
                     coin_ratio = ?,
                     coin_opened_at = CASE
                         WHEN coin_opened_at IS NULL OR coin_opened_at <= 0 THEN ?
                         ELSE coin_opened_at
                     END,
                     updated_at = ?
                 WHERE uid = ? AND item_id = ?",
            )
            .bind(coin_ratio)
            .bind(now)
            .bind(now)
            .bind(uid)
            .bind(item_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                sqlx::query(
                    r#"
                    INSERT INTO account_items (
                        item_id, uid, title, short_title, description, price, orig_price,
                        pic_url, image_urls_json, status, online, coin_open, coin_ratio,
                        coin_opened_at, item_context_json, detail_json, actions_json, created_at, updated_at
                    )
                    VALUES (?, ?, '', '', '', '', '', '', '[]', 'ON_SALE', 1, 1, ?, ?, '{}', '{}', '[]', ?, ?)
                    ON CONFLICT(uid, item_id) DO UPDATE SET
                        coin_open = 1,
                        coin_ratio = excluded.coin_ratio,
                        coin_opened_at = CASE
                            WHEN account_items.coin_opened_at IS NULL OR account_items.coin_opened_at <= 0 THEN excluded.coin_opened_at
                            ELSE account_items.coin_opened_at
                        END,
                        updated_at = excluded.updated_at
                    "#,
                )
                .bind(item_id)
                .bind(uid)
                .bind(coin_ratio)
                .bind(now)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?;
            }
        } else {
            let result = sqlx::query(
                "UPDATE account_items
                 SET coin_open = 0,
                     coin_ratio = '',
                     coin_opened_at = 0,
                     updated_at = ?
                 WHERE uid = ? AND item_id = ?",
            )
            .bind(now)
            .bind(uid)
            .bind(item_id)
            .execute(pool)
            .await?;
            if result.rows_affected() == 0 {
                sqlx::query(
                    r#"
                    INSERT INTO account_items (
                        item_id, uid, title, short_title, description, price, orig_price,
                        pic_url, image_urls_json, status, online, coin_open, coin_ratio,
                        coin_opened_at, item_context_json, detail_json, actions_json, created_at, updated_at
                    )
                    VALUES (?, ?, '', '', '', '', '', '', '[]', 'ON_SALE', 1, 0, '', 0, '{}', '{}', '[]', ?, ?)
                    ON CONFLICT(uid, item_id) DO UPDATE SET
                        coin_open = 0,
                        coin_ratio = '',
                        coin_opened_at = 0,
                        updated_at = excluded.updated_at
                    "#,
                )
                .bind(item_id)
                .bind(uid)
                .bind(now)
                .bind(now)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

async fn warm_coin_deduction_upstreams(state: &AppState, uid: &str, item_ids: &[String]) {
    let location = load_location_or_default(state, uid).await;
    let user_id = uid.parse::<i64>().unwrap_or_default();

    let _ = upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.agree.query"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
        }),
        Vec::new(),
    )
    .await;

    let _ = upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.penalty.query"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "userId": user_id,
        }),
        Vec::new(),
    )
    .await;

    for item_id in item_ids {
        let _ = upstream_h5_request(
            state,
            uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.list"),
            "1.0",
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "deductionType": 0,
                "pageNumber": 1,
                "pageSize": 20,
                "sortType": 0,
                "upItemId": item_id,
            }),
            Vec::new(),
        )
        .await;
    }
}

// ── handlers ──────────────────────────────────────────────────────────────────

pub(super) async fn item_bargain_price_range(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<BargainPriceRangePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let price_fen = price_to_cents(payload.price.trim());
    let input_json = serde_json::to_string(&json!({
        "sourceId": "publishItems",
        "price": price_fen,
    }))
    .unwrap_or_else(|_| "{}".to_string());
    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.idleitem.bargain.price.range.query"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "inputJson": input_json,
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    let data = upstream
        .get("data")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .or_else(|| upstream.get("data").cloned())
        .unwrap_or(Value::Null);
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": data,
    })))
}

pub(super) async fn item_reduce_price(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ReducePricePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let resp =
        run_item_reduce_price_action(&state, &payload.uid, &payload.item_id, &payload.amount)
            .await
            .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(crate) async fn run_item_reduce_price_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    amount: &Value,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let location = load_location_or_default(state, uid).await;
    let item = fetch_item(state.storage().xianyu(), uid, item_id)
        .await
        .map_err(|e| e.to_string())?;
    let item_price = item
        .as_ref()
        .map(|value| value_to_string_lossy(value.get("price")))
        .unwrap_or_default();
    let (_target_price, amount_fen) =
        markdown_target_price(&item_price, amount).map_err(|e| e.to_string())?;
    let upstream = upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.fast.markdown"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "latitude": location.latitude,
            "longitude": location.longitude,
            "itemId": item_id,
            "amount": amount_fen,
            "params": "{}",
        }),
        Vec::new(),
    )
    .await
    .map_err(|e| e.to_string())?;
    state.record_mtop_response(
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.fast.markdown"),
        &upstream,
    );
    if !payload_ret_success(&upstream) {
        return Ok(upstream);
    }
    if upstream_data_result_false(&upstream) {
        let msg = upstream_data_message(&upstream);
        return Ok(json!({
            "api": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.fast.markdown"),
            "ret": [format!("FAIL_BIZ_MARKDOWN::{msg}")],
            "data": upstream.get("data").cloned().unwrap_or(Value::Null),
            "v": "1.0",
        }));
    }
    Ok(upstream)
}

pub(crate) async fn run_item_reduce_price_local_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    amount: &Value,
) -> Result<Value, String> {
    ensure_item_belongs_to_uid(state, uid, item_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let item = fetch_item(state.storage().xianyu(), uid, item_id)
        .await
        .map_err(|e| e.to_string())?;
    let item_price = item
        .as_ref()
        .map(|value| value_to_string_lossy(value.get("price")))
        .unwrap_or_default();
    let (target_price, _) =
        markdown_target_price(&item_price, amount).map_err(|e| e.to_string())?;
    update_item_price(state.storage().xianyu(), uid, item_id, &target_price)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "ret": success_ret(),
        "data": {
            "success": true,
            "message": "降价成功",
            "itemId": item_id,
            "newPrice": target_price,
        }
    }))
}

pub(super) async fn item_reduce_price_feeds(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ReducePriceFeedsPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let item = fetch_item(state.storage().xianyu(), &payload.uid, &payload.item_id)
        .await
        .map_err(ResponseError::internal)?;
    let exts = reduce_feed_exts(&payload, item.as_ref());
    let price_key = if payload.price_key.trim().is_empty() {
        reduce_feed_price_key(&exts)
    } else {
        payload.price_key.trim().to_string()
    };
    let suggest_price = if payload.suggest_price.trim().is_empty() {
        item.as_ref()
            .map(|value| value_to_string_lossy(value.get("price")))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "0".to_string())
    } else {
        payload.suggest_price.trim().to_string()
    };
    let exts_text = serde_json::to_string(&exts).unwrap_or_else(|_| "[]".to_string());
    let api = crate::sealed_protocol::sealed_str!("mtop.taobao.idle.reduce.price.feeds.query");
    let version = "1.0";
    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        api,
        version,
        json!({
            "itemId": payload.item_id,
            "exts": exts_text,
            "priceKey": price_key,
            "suggestPrice": suggest_price,
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream_response_with_api(
        api, version, upstream,
    )))
}

pub(super) async fn items_batch_reduce(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<BatchReducePricePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    for item_id in &payload.item_ids {
        ensure_item_belongs_to_uid(&state, &payload.uid, item_id).await?;
    }
    let location = load_location_or_default(&state, &payload.uid).await;
    let mut success_count = 0_usize;
    let mut fail_count = 0_usize;
    let mut results = Vec::with_capacity(payload.item_ids.len());
    for item_id in &payload.item_ids {
        let item = match fetch_item(state.storage().xianyu(), &payload.uid, item_id).await {
            Ok(value) => value,
            Err(err) => {
                fail_count += 1;
                results.push(json!({
                    "itemId": item_id,
                    "ok": false,
                    "message": err.to_string(),
                }));
                continue;
            }
        };
        let item_price = item
            .as_ref()
            .map(|value| value_to_string_lossy(value.get("price")))
            .unwrap_or_default();
        let amount_fen = match markdown_target_price(&item_price, &payload.amount) {
            Ok((_target_price, target_cents)) => target_cents,
            Err(err) => {
                fail_count += 1;
                results.push(json!({
                    "itemId": item_id,
                    "ok": false,
                    "message": err.to_string(),
                }));
                continue;
            }
        };
        let upstream = upstream_h5_request(
            &state,
            &payload.uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.fast.markdown"),
            "1.0",
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "latitude": location.latitude,
                "longitude": location.longitude,
                "itemId": item_id,
                "amount": amount_fen,
                "params": "{}",
            }),
            Vec::new(),
        )
        .await;
        match upstream {
            Ok(upstream) => {
                state.record_mtop_response(
                    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.fast.markdown"),
                    &upstream,
                );
                if payload_ret_success(&upstream) && !upstream_data_result_false(&upstream) {
                    success_count += 1;
                    results.push(json!({
                        "itemId": item_id,
                        "ok": true,
                        "message": success_message("降价成功")["message"].clone(),
                    }));
                    continue;
                }
                fail_count += 1;
                let msg = if payload_ret_success(&upstream) {
                    upstream_data_message(&upstream)
                } else {
                    upstream
                        .get("ret")
                        .and_then(Value::as_array)
                        .and_then(|values| values.first())
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                };
                results.push(json!({
                    "itemId": item_id,
                    "ok": false,
                    "message": if msg.is_empty() { "降价失败".to_string() } else { msg },
                    "raw": upstream,
                }));
            }
            Err(err_msg) => {
                fail_count += 1;
                results.push(json!({
                    "itemId": item_id,
                    "ok": false,
                    "message": err_msg,
                }));
            }
        }
    }
    let ret: Vec<String> = if fail_count == 0 {
        success_ret()
            .into_iter()
            .map(|value| value.to_string())
            .collect()
    } else {
        vec![format!(
            "FAIL_PARTIAL_BATCH_REDUCE::{success_count} 成功，{fail_count} 失败"
        )]
    };
    Ok(ResponseJson(json!({
        "ret": ret,
        "data": {
            "success": fail_count == 0,
            "updated": success_count,
            "failed": fail_count,
            "results": results,
        }
    })))
}

#[cfg(test)]
mod tests {
    use super::{markdown_target_price, update_coin_deduction_state};
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, Row};

    async fn setup_coin_state_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE account_items (
                item_id TEXT NOT NULL,
                uid TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                short_title TEXT NOT NULL DEFAULT '',
                description TEXT NOT NULL DEFAULT '',
                price TEXT NOT NULL DEFAULT '',
                orig_price TEXT NOT NULL DEFAULT '',
                pic_url TEXT NOT NULL DEFAULT '',
                image_urls_json TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'ON_SALE',
                online INTEGER NOT NULL DEFAULT 1,
                want_count INTEGER NOT NULL DEFAULT 0,
                view_count INTEGER NOT NULL DEFAULT 0,
                exposure_num INTEGER NOT NULL DEFAULT 0,
                coin_open INTEGER NOT NULL DEFAULT 0,
                coin_ratio TEXT NOT NULL DEFAULT '',
                city TEXT NOT NULL DEFAULT '',
                province TEXT NOT NULL DEFAULT '',
                area TEXT NOT NULL DEFAULT '',
                division_id TEXT NOT NULL DEFAULT '',
                latitude TEXT NOT NULL DEFAULT '',
                longitude TEXT NOT NULL DEFAULT '',
                fish_ad_code TEXT NOT NULL DEFAULT '',
                item_context_json TEXT NOT NULL DEFAULT '{}',
                detail_json TEXT NOT NULL DEFAULT '{}',
                created_at REAL NOT NULL DEFAULT 0,
                updated_at REAL NOT NULL DEFAULT 0,
                actions_json TEXT NOT NULL DEFAULT '[]',
                coin_opened_at REAL NOT NULL DEFAULT 0,
                PRIMARY KEY (uid, item_id)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create account_items");
        pool
    }

    #[test]
    fn markdown_target_price_uses_delta_amount_not_final_price() {
        let (target_price, target_cents) =
            markdown_target_price("200.00", &json!(0.1)).expect("target price");
        assert_eq!(target_price, "199.90");
        assert_eq!(target_cents, "19990");
    }

    #[test]
    fn markdown_target_price_rejects_amount_ge_price() {
        let err = markdown_target_price("15.00", &json!(15)).expect_err("should reject");
        assert!(err.to_string().contains("降价金额不能大于或等于售价"));
    }

    #[tokio::test]
    async fn update_coin_deduction_state_inserts_stub_row_for_new_publish_item() {
        let pool = setup_coin_state_pool().await;
        update_coin_deduction_state(&pool, "u1", &[String::from("new-item")], true, "10")
            .await
            .expect("update coin state");

        let row = sqlx::query(
            "SELECT uid, item_id, coin_open, coin_ratio, coin_opened_at FROM account_items WHERE uid = ? AND item_id = ?",
        )
        .bind("u1")
        .bind("new-item")
        .fetch_one(&pool)
        .await
        .expect("inserted row");

        assert_eq!(row.get::<String, _>("uid"), "u1");
        assert_eq!(row.get::<String, _>("item_id"), "new-item");
        assert_eq!(row.get::<i64, _>("coin_open"), 1);
        assert_eq!(row.get::<String, _>("coin_ratio"), "10");
        assert!(row.get::<f64, _>("coin_opened_at") > 0.0);
    }
}

pub(super) async fn coin_deduction_open(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CoinDeductionPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    for item_id in &payload.item_ids {
        ensure_item_belongs_to_uid(&state, &payload.uid, item_id).await?;
    }
    let ratio = if payload.ratio.trim().is_empty() {
        default_coin_deduction_ratio()
    } else {
        payload.ratio.trim().to_string()
    };
    let resp = run_coin_open_action(
        &state,
        &payload.uid,
        payload
            .item_ids
            .first()
            .map(String::as_str)
            .unwrap_or_default(),
        &ratio,
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(crate) async fn run_coin_open_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
    ratio: &str,
) -> Result<Value, String> {
    let payload = CoinDeductionPayload {
        uid: uid.to_string(),
        item_ids: vec![item_id.to_string()],
        ratio: ratio.to_string(),
        channel: default_coin_deduction_channel(),
    };
    for item_id in &payload.item_ids {
        ensure_item_belongs_to_uid(state, &payload.uid, item_id)
            .await
            .map_err(|error| error.detail().to_string())?;
    }
    let location = load_location_or_default(&state, &payload.uid).await;
    let ratio = if payload.ratio.trim().is_empty() {
        default_coin_deduction_ratio()
    } else {
        payload.ratio.trim().to_string()
    };
    let channel = if payload.channel.trim().is_empty() {
        default_coin_deduction_channel()
    } else {
        payload.channel.trim().to_string()
    };
    let item_id_list =
        serde_json::to_string(&payload.item_ids).unwrap_or_else(|_| "[]".to_string());
    let open_data = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "itemIdList": item_id_list.clone(),
        "deductionRatio": ratio.clone(),
        "channel": channel.clone(),
    });
    tracing::info!(
        uid = %payload.uid,
        items = ?payload.item_ids,
        ratio = %ratio,
        channel = %channel,
        "coin_deduction_open: sending request"
    );
    warm_coin_deduction_upstreams(&state, &payload.uid, &payload.item_ids).await;
    let upstream = upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.new.open"),
        "1.0",
        open_data.clone(),
        Vec::new(),
    )
    .await;

    let upstream = match upstream {
        Ok(upstream_payload) if payload_ret_success(&upstream_payload) => upstream_payload,
        Ok(upstream_payload) if is_not_agree_deduction_error(&upstream_payload.to_string()) => {
            tracing::warn!(
                uid = %payload.uid,
                ret = ?upstream_payload.get("ret"),
                "coin_deduction_open: h5 reported not-agree, attempting h5 agree.change then retry"
            );
            let agree_result = upstream_h5_request(
                &state,
                &payload.uid,
                crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.agree.query"),
                "1.0",
                json!({
                    "fishAdCode": location.fish_ad_code,
                    "gps": location.gps,
                }),
                Vec::new(),
            )
            .await;
            match &agree_result {
                Ok(resp) => {
                    let agree_flag = resp
                        .get("data")
                        .and_then(|d| {
                            d.get("agreeFlag")
                                .or_else(|| d.as_object().and_then(|o| o.get("agreeFlag")))
                        })
                        .or_else(|| resp.get("agreeFlag"))
                        .and_then(|v| {
                            v.as_i64()
                                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                        })
                        .unwrap_or(0);
                    tracing::info!(uid = %payload.uid, agree_flag = %agree_flag, "coin deduction agree.query response");
                }
                Err(e) => {
                    tracing::warn!(uid = %payload.uid, error = %e, "coin deduction agree.query failed")
                }
            }
            match coin_deduction_agree_change(
                &state,
                &payload.uid,
                &location.fish_ad_code,
                &location.gps,
            )
            .await
            {
                Ok(resp) => {
                    let result_flag = resp
                        .get("data")
                        .and_then(Value::as_object)
                        .and_then(|data| data.get("result"))
                        .and_then(|value| {
                            value
                                .as_bool()
                                .map(|v| v.to_string())
                                .or_else(|| value.as_str().map(ToString::to_string))
                        })
                        .unwrap_or_default();
                    tracing::info!(
                        uid = %payload.uid,
                        result = %result_flag,
                        ret = ?resp.get("ret"),
                        "coin deduction agree.change response"
                    );
                }
                Err(e) => {
                    tracing::warn!(uid = %payload.uid, error = %e, "coin deduction agree.change failed")
                }
            }
            match upstream_h5_request(
                &state,
                &payload.uid,
                crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.new.open"),
                "1.0",
                open_data,
                Vec::new(),
            )
            .await
            {
                Ok(app_payload) if payload_ret_success(&app_payload) => {
                    tracing::info!(
                        uid = %payload.uid,
                        ret = ?app_payload.get("ret"),
                        "coin_deduction_open: h5 retry succeeded"
                    );
                    app_payload
                }
                Ok(app_payload) => {
                    tracing::warn!(
                        uid = %payload.uid,
                        ret = ?app_payload.get("ret"),
                        "coin_deduction_open: h5 retry still non-success"
                    );
                    return Ok(app_payload);
                }
                Err(app_err) => {
                    tracing::warn!(
                        uid = %payload.uid,
                        error = %app_err,
                        "coin_deduction_open: h5 retry failed"
                    );
                    return Err(app_err);
                }
            }
        }
        Ok(upstream_payload) => return Ok(upstream_payload),
        Err(err_msg) => return Err(err_msg),
    };
    tracing::info!(
        uid = %payload.uid,
        ret = ?upstream.get("ret"),
        "coin_deduction_open: upstream response"
    );

    if payload_ret_success(&upstream) {
        update_coin_deduction_state(
            state.storage().xianyu(),
            &payload.uid,
            &payload.item_ids,
            true,
            &ratio,
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(upstream)
}

pub(super) async fn coin_deduction_close(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CoinDeductionPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    for item_id in &payload.item_ids {
        ensure_item_belongs_to_uid(&state, &payload.uid, item_id).await?;
    }
    let resp = run_coin_close_action(
        &state,
        &payload.uid,
        payload
            .item_ids
            .first()
            .map(String::as_str)
            .unwrap_or_default(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(resp))
}

pub(crate) async fn run_coin_close_action(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    let payload = CoinDeductionPayload {
        uid: uid.to_string(),
        item_ids: vec![item_id.to_string()],
        ratio: String::new(),
        channel: default_coin_deduction_channel(),
    };
    for item_id in &payload.item_ids {
        ensure_item_belongs_to_uid(state, &payload.uid, item_id)
            .await
            .map_err(|error| error.detail().to_string())?;
    }
    let now = unix_seconds_now();
    for item_id in &payload.item_ids {
        if let Some(row) =
            sqlx::query("SELECT coin_opened_at FROM account_items WHERE uid = ? AND item_id = ?")
                .bind(&payload.uid)
                .bind(item_id)
                .fetch_optional(state.storage().xianyu())
                .await
                .map_err(|e| e.to_string())?
        {
            let opened_at = row.try_get::<f64, _>("coin_opened_at").ok().unwrap_or(0.0);
            if opened_at > 0.0 && now - opened_at < 24.0 * 3600.0 {
                return Ok(json!({
                    "api": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.new.close"),
                    "ret": ["FAIL_LOCAL_COIN_CLOSE_LOCKED::开启 24 小时后才可关闭，当前只能修改抵扣比例"],
                    "data": {
                        "canClose": false,
                        "coinOpenedAt": opened_at,
                        "closeAvailableAt": opened_at + 24.0 * 3600.0,
                    },
                    "v": "1.0"
                }));
            }
        }
    }
    let location = load_location_or_default(&state, &payload.uid).await;
    let channel = if payload.channel.trim().is_empty() {
        default_coin_deduction_channel()
    } else {
        payload.channel.trim().to_string()
    };
    let upstream = upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.new.close"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "itemIdList": serde_json::to_string(&payload.item_ids).unwrap_or_else(|_| "[]".to_string()),
            "channel": channel,
        }),
        Vec::new(),
    )
    .await
    .map_err(|e| e.to_string())?;
    state.record_mtop_response(
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.new.close"),
        &upstream,
    );
    if !payload_ret_success(&upstream) && is_coin_close_t1_limit_error(&upstream.to_string()) {
        return Ok(json!({
            "api": crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.new.close"),
            "ret": ["FAIL_LOCAL_COIN_CLOSE_LOCKED::开启 24 小时后才可关闭，当前只能修改抵扣比例"],
            "data": {
                "canClose": false,
            },
            "v": "1.0"
        }));
    }
    if payload_ret_success(&upstream) {
        update_coin_deduction_state(
            state.storage().xianyu(),
            &payload.uid,
            &payload.item_ids,
            false,
            "",
        )
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(upstream)
}

// 类目属性 schema —— **真正的发布/编辑属性接口**：
// `mtop.taobao.idle.kgraph.property.recommend` v=2.0
// 关键：必须传 scene="edit" + title + description + catId + channelCatId + imageInfos +
// currentCardList + selectedList + uniqueCode 等完整入参，闲鱼才会按"编辑当前商品"
// 模式返该类目专属属性 schema（cardList[].cardData = {propertyId, propertyName, valuesList[]}）。
// 简化入参（只 itemId+channelCatId）会被识别成"猜分类"模式返通用列表 → 所有商品都一样。
// Python 老项目 `item_kgraph_recommend` 是这个接口的标准用法。
pub(super) async fn item_properties(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemPropertiesPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_item_belongs_to_uid(&state, &payload.uid, &payload.item_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    // 没传 title/cat 就先调 pc.detail 拿
    let mut title = payload.title.clone();
    let mut channel_cat_id = payload.channel_cat_id.clone();
    let mut cat_id = String::new();
    let mut description = String::new();
    let mut image_infos: Value = Value::Array(Vec::new());

    let detail = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
        "1.0",
        json!({ "itemId": payload.item_id }),
        Vec::new(),
    )
    .await
    .ok()
    .and_then(|v| v.get("data").cloned());
    let mut cpv_labels_value: Value = Value::Array(Vec::new());
    if let Some(d) = detail.as_ref() {
        let item_do = d
            .get(crate::sealed_protocol::sealed_str!("itemDO"))
            .cloned()
            .unwrap_or(Value::Null);
        if title.is_empty() {
            title = value_to_string_lossy(item_do.get("title"));
        }
        if description.is_empty() {
            description = value_to_string_lossy(item_do.get("desc"));
        }
        let cat_dto = item_do.get("itemCatDTO").cloned().unwrap_or(Value::Null);
        if cat_id.is_empty() {
            cat_id = value_to_string_lossy(cat_dto.get("catId"));
        }
        if channel_cat_id.is_empty() {
            channel_cat_id = value_to_string_lossy(cat_dto.get("channelCatId"));
        }
        if let Some(arr) = item_do.get("imageInfos").and_then(Value::as_array) {
            image_infos = Value::Array(arr.clone());
        }
        cpv_labels_value = item_do
            .get("cpvLabels")
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
    }
    debug!(
        target: "market.pricing",
        stream = "runtime",
        module = "market-pricing",
        action = "item_properties_seed",
        outcome = "success",
        account_uid = %payload.uid,
        item_id = %payload.item_id,
        title = %title,
        channel_cat_id = %channel_cat_id,
        cat_id = %cat_id,
        "prepared item properties seed data"
    );

    let unique_code = format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.kgraph.property.recommend"),
        "2.0",
        json!({
            "bizParams": {},
            "catId": cat_id,
            "channelCatId": channel_cat_id,
            "currentCardList": [],
            "description": description,
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "imageInfos": image_infos,
            "latitude": location.latitude,
            "lockCpv": false,
            "longitude": location.longitude,
            "multiSKU": false,
            "publishScene": "mainPublish",
            "scene": "edit",
            "selectedList": [],
            "sourceId": "main_publish_item",
            "title": title,
            "trendySkuShopSingle": false,
            "trendySkuStyleInfo": {},
            "uniqueCode": unique_code,
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;

    // 解析 cardList[].cardData = { propertyId, propertyName, valuesList[].{valueId,valueName,...} }
    let card_list = upstream
        .get("data")
        .and_then(|d| d.get("cardList"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let card_names: Vec<String> = card_list
        .iter()
        .filter_map(|c| {
            c.get("cardData")
                .and_then(|cd| cd.get("propertyName"))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect();
    debug!(
        target: "market.pricing",
        stream = "runtime",
        module = "market-pricing",
        action = "item_properties_kgraph_returned",
        outcome = "success",
        account_uid = %payload.uid,
        item_id = %payload.item_id,
        card_count = card_list.len(),
        card_names = ?card_names,
        "received kgraph property cards"
    );
    let kgraph_data = upstream.get("data").unwrap_or(&serde_json::Value::Null);
    debug!(
        target: "market.pricing",
        stream = "runtime",
        module = "market-pricing",
        action = "item_properties_kgraph_raw_data",
        outcome = "success",
        account_uid = %payload.uid,
        item_id = %payload.item_id,
        data_kind = if kgraph_data.is_object() { "object" } else if kgraph_data.is_array() { "array" } else { "other" },
        data_key_count = kgraph_data.as_object().map(|data| data.len()).unwrap_or(0),
        data_array_len = kgraph_data.as_array().map(|data| data.len()).unwrap_or(0),
        "captured kgraph property data summary"
    );
    let mut cards: Vec<Value> = Vec::new();
    for c in card_list {
        let cd = match c.get("cardData").and_then(Value::as_object) {
            Some(o) => o,
            None => continue,
        };
        let prop_id = cd
            .get("propertyId")
            .and_then(value_to_string)
            .unwrap_or_default();
        let prop_name = cd
            .get("propertyName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if prop_name.is_empty() {
            continue;
        }
        let values = cd
            .get("valuesList")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut out_values = Vec::new();
        for v in values {
            // 分类（propertyId=-10000）的 value 项闲鱼不下发 valueId 字段，要 fallback 到
            // channelCatId / catId，否则前端 chip 高亮判断会因 ""==""=true 全部被选中。
            let value_id = value_to_string_lossy(
                v.get("valueId")
                    .or_else(|| v.get("channelCatId"))
                    .or_else(|| v.get("catId")),
            );
            let value_name = value_to_string_lossy(
                v.get("valueName")
                    .or_else(|| v.get("catName"))
                    .or_else(|| v.get("channelCatName")),
            );
            if value_name.is_empty() || value_id.is_empty() {
                continue;
            }
            out_values.push(json!({
                "valueId": value_id,
                "valueName": value_name,
                "selected": v.get("isClicked").and_then(Value::as_str).unwrap_or("0") == "1"
                    || v.get("isUserClick").and_then(Value::as_str).unwrap_or("0") == "1",
            }));
        }
        if out_values.is_empty() {
            continue;
        }
        cards.push(json!({
            "propertyId": prop_id,
            "propertyName": prop_name,
            "valuesList": out_values,
        }));
    }

    // 合并：商品已有的 cpvLabels 中，propertyId 不在 kgraph 推荐里的，补一张卡
    // （valuesList 仅含商品已选的那个值，至少让用户看到当前已选）。
    if let Some(labels) = cpv_labels_value.as_array() {
        let existing_pids: std::collections::HashSet<String> = cards
            .iter()
            .filter_map(|c| c.get("propertyId").and_then(value_to_string))
            .collect();
        for lbl in labels {
            let pid = value_to_string_lossy(lbl.get("propertyId"));
            let pname = value_to_string_lossy(lbl.get("propertyName"));
            let vid = value_to_string_lossy(lbl.get("valueId"));
            let vname = value_to_string_lossy(lbl.get("valueName"));
            if pid.is_empty() || pname.is_empty() || vid.is_empty() || vname.is_empty() {
                continue;
            }
            if existing_pids.contains(&pid) {
                continue;
            }
            cards.push(json!({
                "propertyId": pid,
                "propertyName": pname,
                "valuesList": [{
                    "valueId": vid,
                    "valueName": vname,
                    "selected": true,
                }],
            }));
        }
    }

    debug!(
        target: "market.pricing",
        stream = "runtime",
        module = "market-pricing",
        action = "item_properties_final_cards",
        outcome = "success",
        account_uid = %payload.uid,
        item_id = %payload.item_id,
        final_card_count = cards.len(),
        "built final property cards after cpv merge"
    );
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "cards": cards,
            "title": title,
            "channelCatId": channel_cat_id,
            "catId": cat_id,
        }
    })))
}

pub(super) async fn coin_deduction_ratio(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinRatioQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    let price = query.price.clone();
    if !query.item_id.trim().is_empty() {
        if let Ok(Some(item)) =
            fetch_item(state.storage().xianyu(), &query.uid, query.item_id.trim()).await
        {
            let cached_ratios = item
                .get("coinRatioList")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !cached_ratios.is_empty() {
                return Ok(ResponseJson(json!({
                    "ret": success_ret(),
                    "data": {
                        "source": "cached_item",
                        "itemId": query.item_id,
                        "coinDeductionInfoList": cached_ratios,
                    }
                })));
            }
        }
    }
    if let Ok(upstream) = successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.idle.coin.deduction.ratio.list"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "price": price.clone(),
            "channel": "publishV2",
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    Ok(ResponseJson(json!({
        "ret": ["FAIL_LOCAL_NO_RATIO_OPTIONS::未获取到该商品可用闲鱼币抵扣比例"],
        "data": {
            "coinDeductionInfoList": [],
            "coinRatioList": [],
        }
    })))
}
