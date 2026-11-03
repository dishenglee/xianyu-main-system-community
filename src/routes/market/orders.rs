use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::{
    auth_expired_message, decode_maybe_json_string, ensure_market_uid_access,
    ensure_order_belongs_to_uid, load_location_or_default, payload_ret_success, price_to_cents,
    read_sync_state, success_message, success_ret, success_ret_value, unix_millis_now,
    unix_seconds_now, upstream_h5_request, value_str, value_to_string_lossy, write_sync_state,
    SyncOutcome, UidQuery,
};

// ---------------------------------------------------------------------------
// Payload / query structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct TradeClosePayload {
    uid: String,
    order_id: String,
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TradeAdjustPayload {
    uid: String,
    order_id: String,
    new_price: String,
    #[serde(default)]
    new_transport_fee: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OrderPayload {
    pub(crate) uid: String,
    pub(crate) biz_order_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrderRedFlowerPayload {
    uid: String,
    biz_order_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrderAdjustPayload {
    uid: String,
    biz_order_id: String,
    #[serde(deserialize_with = "deserialize_string_or_number")]
    modify_fee: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrderClosePayload {
    uid: String,
    biz_order_id: String,
    #[serde(default)]
    close_reason: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrderConsignPayload {
    uid: String,
    biz_order_id: String,
    company_code: String,
    company_name: String,
    mail_no: String,
    #[serde(default)]
    address_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TradeSoldQuery {
    uid: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_status_all")]
    status: String,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct TradeOrdersQuery {
    uid: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_seller_true")]
    seller: bool,
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct SyncPayload {
    uid: String,
    #[serde(default = "default_seller_true")]
    seller: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrderCloseReasonQuery {
    uid: String,
    biz_order_id: String,
}

// ---------------------------------------------------------------------------
// Serde default helpers
// ---------------------------------------------------------------------------

fn default_page() -> i64 {
    1
}

fn default_status_all() -> String {
    "all".to_string()
}

fn default_seller_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

pub(super) async fn sync_orders_route(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<SyncPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let outcome = sync_orders_to_db(&state, &payload.uid, payload.seller)
        .await
        .map_err(ResponseError::internal_message)?;
    let status = if outcome.ok { "ok" } else { "error" };
    tracing::info!(uid = %payload.uid, ok = outcome.ok, status = status, detail = %outcome.detail, "sync_orders_route result");
    Ok(ResponseJson(json!({
        "status": status,
        "ok": outcome.ok,
        "uid": payload.uid,
        "total": outcome.total,
        "synced_at": outcome.synced_at,
        "detail": outcome.detail,
    })))
}

pub(super) async fn trade_close(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<TradeClosePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    // Close must succeed on Xianyu first; never fake-close only the local cache.
    let upstream = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.close.by.seller"),
        "2.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "bizOrderId": payload.order_id.clone(),
            "tid": payload.order_id.clone(),
            "closeReason": payload.reason.trim(),
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| ResponseError::internal_message(format!("关闭订单失败：{error}")))?;
    update_order_status(
        state.storage().xianyu(),
        &payload.uid,
        &payload.order_id,
        "TRADE_CLOSED",
        payload.reason.trim(),
    )
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn trade_adjust_price(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<TradeAdjustPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    // Never fake success with local cache-only updates. Order price must change on Xianyu first.
    let upstream = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.user.adjust.price"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderId": payload.order_id.clone(),
            "modifyFee": price_to_cents(payload.new_price.trim()),
            "newTransportFee": price_to_cents(payload.new_transport_fee.trim()),
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| ResponseError::internal_message(format!("订单改价失败：{error}")))?;
    update_order_price(
        state.storage().xianyu(),
        &payload.uid,
        &payload.order_id,
        payload.new_price.trim(),
    )
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn trade_sold(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<TradeSoldQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    // DB-first: force=true triggers an upstream sync into SQLite, then we always
    // read from SQLite so downstream request latency stays flat and upstream
    // cannot rate-limit us on every page refresh.
    if query.force {
        let _ = sync_sold_orders_to_db(&state, &query.uid, query.page, &query.status).await;
    }
    let mut payload = trade_orders_payload(state.storage().xianyu(), &query.uid, query.force)
        .await
        .map_err(ResponseError::internal)?;
    let sync_state = read_sync_state(state.storage().xianyu(), &query.uid, "orders").await;
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "_last_synced_at".to_string(),
            json!(sync_state.as_ref().map(|s| s.last_synced_at).unwrap_or(0.0)),
        );
        object.insert(
            "_last_sync_ok".to_string(),
            json!(sync_state.as_ref().map(|s| s.last_ok).unwrap_or(false)),
        );
        object.insert(
            "_last_sync_reason".to_string(),
            json!(sync_state
                .as_ref()
                .map(|s| s.last_reason.clone())
                .unwrap_or_default()),
        );
    }
    Ok(ResponseJson(payload))
}

pub(super) async fn order_modify_price_render(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    if let Ok(upstream) = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.order.modify.price.render"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "bizOrderId": payload.biz_order_id.clone(),
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    let order = fetch_order(
        state.storage().xianyu(),
        &payload.uid,
        &payload.biz_order_id,
    )
    .await
    .map_err(ResponseError::internal)?
    .ok_or_else(|| ResponseError::not_found("订单不存在"))?;
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "bizOrderId": payload.biz_order_id,
            "price": order["price"],
            "canModify": true,
        }
    })))
}

pub(super) async fn order_adjust_price(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderAdjustPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let modify_fee_cents = order_adjust_modify_fee_cents(&payload.modify_fee);
    // Upstream success is required. Local DB is only a cache mirror after Xianyu accepts the change.
    let upstream = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.user.adjust.price"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderId": payload.biz_order_id.clone(),
            "modifyFee": modify_fee_cents,
            "newTransportFee": "0",
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| ResponseError::internal_message(format!("订单改价失败：{error}")))?;
    update_order_price(
        state.storage().xianyu(),
        &payload.uid,
        &payload.biz_order_id,
        payload.modify_fee.trim(),
    )
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn order_red_flower(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderRedFlowerPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let order = fetch_order(
        state.storage().xianyu(),
        &payload.uid,
        &payload.biz_order_id,
    )
    .await
    .map_err(ResponseError::internal)?
    .ok_or_else(|| ResponseError::not_found("订单不存在"))?;
    if !red_flower_eligible(&order) {
        return Err(ResponseError::bad_request(
            "仅交易成功且未退款的订单可以求小红花",
        ));
    }
    let existed: i64 = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM order_red_flower_requests WHERE uid=? AND order_id=? AND status='success')")
        .bind(&payload.uid).bind(&payload.biz_order_id).fetch_one(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    if existed != 0 {
        return Ok(ResponseJson(
            json!({"ret": success_ret(), "data": {"success":true,"alreadyRequested":true,"message":"该订单已求过小红花"}}),
        ));
    }
    let duplicate =
        request_red_flower_upstream(&state, &payload.uid, &payload.biz_order_id, true, "manual")
            .await
            .map_err(ResponseError::bad_request)?;
    Ok(ResponseJson(
        json!({"ret":success_ret(),"data":{"success":true,"alreadyRequested":duplicate,"message":if duplicate {"该订单已求过小红花"} else {"求小红花成功"}}}),
    ))
}

fn red_flower_eligible(order: &Value) -> bool {
    let status = value_to_string_lossy(order.get("status")).to_ascii_uppercase();
    let text = value_to_string_lossy(order.get("status_text"));
    !(status.contains("REFUND")
        || status.contains("CLOSE")
        || status.contains("CANCEL")
        || text.contains("退款")
        || text.contains("关闭")
        || text.contains("取消"))
        && (status.contains("TRADE_SUCCESS")
            || status.contains("FINISH")
            || status == "DONE"
            || text.contains("交易成功")
            || text.contains("已完成"))
}

fn red_flower_ret(upstream: &Value) -> String {
    upstream
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|v| v.first())
        .and_then(Value::as_str)
        .or_else(|| upstream.get("ret").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

fn parse_red_flower_data_value(payload: &Value) -> Value {
    payload
        .get("data")
        .and_then(|value| match value {
            Value::String(text) => serde_json::from_str::<Value>(text).ok(),
            _ => Some(value.clone()),
        })
        .unwrap_or(Value::Null)
}

fn normalize_red_flower_error_text(text: &str) -> String {
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

fn red_flower_error_message(upstream: &Value) -> String {
    let ret = red_flower_ret(upstream);
    let normalized_ret = normalize_red_flower_error_text(&ret);
    if !normalized_ret.is_empty() && !normalized_ret.eq_ignore_ascii_case("success") {
        return normalized_ret;
    }

    let data = parse_red_flower_data_value(upstream);
    for value in [
        upstream.get("message"),
        upstream.get("errorMsg"),
        upstream.get("errorMessage"),
        upstream.get("sub_msg"),
        upstream.get("subMsg"),
        upstream.get("detail"),
        data.get("msg"),
        data.get("message"),
        data.get("errorMsg"),
        data.get("errorMessage"),
        data.get("sub_msg"),
        data.get("subMsg"),
        data.get("detail"),
    ] {
        let text = normalize_red_flower_error_text(&value_to_string_lossy(value));
        if !text.is_empty() {
            return text;
        }
    }

    "平台暂未受理求小红花，请稍后重试".to_string()
}

fn red_flower_duplicate(upstream: &Value) -> bool {
    [
        "已送出小红花",
        "已收下",
        "已求过",
        "已赠送",
        "已经送",
        "不能重复",
    ]
    .iter()
    .any(|v| format!("{upstream}").contains(v))
}

fn red_flower_receive_issued(upstream: &Value) -> bool {
    let issued = upstream
        .get("data")
        .and_then(|data| data.get("module"))
        .and_then(|module| module.get("issued"));
    matches!(issued, Some(Value::Bool(true))) || issued.and_then(Value::as_str) == Some("true")
}

async fn upsert_red_flower_request_status(
    pool: &SqlitePool,
    uid: &str,
    order_id: &str,
    status: &str,
    upstream_ret: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO order_red_flower_requests (uid,order_id,status,upstream_ret,requested_at,updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(uid,order_id) DO UPDATE SET status=excluded.status, upstream_ret=excluded.upstream_ret, updated_at=excluded.updated_at")
        .bind(uid)
        .bind(order_id)
        .bind(status)
        .bind(upstream_ret)
        .bind(unix_seconds_now())
        .bind(unix_seconds_now())
        .execute(pool)
        .await
        .map(|_| ())
}

async fn upsert_red_flower_receipt_status(
    pool: &SqlitePool,
    uid: &str,
    order_id: &str,
    status: &str,
    upstream_ret: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO order_red_flower_receipts (uid,order_id,status,upstream_ret,received_at,updated_at) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(uid,order_id) DO UPDATE SET status=excluded.status, upstream_ret=excluded.upstream_ret, updated_at=excluded.updated_at")
        .bind(uid)
        .bind(order_id)
        .bind(status)
        .bind(upstream_ret)
        .bind(unix_seconds_now())
        .bind(unix_seconds_now())
        .execute(pool)
        .await
        .map(|_| ())
}

async fn request_red_flower_upstream(
    state: &AppState,
    uid: &str,
    order_id: &str,
    persist_failure: bool,
    source: &str,
) -> Result<bool, String> {
    let upstream = upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idlemessage.red.flower"),
        "4.0",
        json!({"orderId": order_id, "channel":"list"}),
        vec![("accountSite".to_string(), "xianyu".to_string())],
    )
    .await
    .map_err(|error| {
        tracing::warn!(uid=%uid, order_id=%order_id, source=%source, error=%error, "red flower request failed");
        error
    })?;
    let ret = red_flower_ret(&upstream);
    let duplicate = red_flower_duplicate(&upstream);
    if !payload_ret_success(&upstream) && !duplicate {
        let message = red_flower_error_message(&upstream);
        tracing::warn!(uid=%uid, order_id=%order_id, source=%source, ret=%ret, message=%message, "red flower request was rejected by upstream");
        if persist_failure {
            let _ = upsert_red_flower_request_status(
                state.storage().xianyu(),
                uid,
                order_id,
                "failed",
                &ret,
            )
            .await;
        }
        return Err(message);
    }
    upsert_red_flower_request_status(state.storage().xianyu(), uid, order_id, "success", &ret)
        .await
        .map_err(|error| error.to_string())?;
    Ok(duplicate)
}

pub(crate) async fn auto_receive_red_flower(
    state: &AppState,
    uid: &str,
    order_id: &str,
    source: &str,
) -> Result<bool, String> {
    let existed: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM order_red_flower_receipts WHERE uid=? AND order_id=? AND status='success')",
    )
    .bind(uid)
    .bind(order_id)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    if existed != 0 {
        return Ok(true);
    }

    let location = load_location_or_default(state, uid).await;
    let upstream = upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.red.flower.seller.receive"),
        "1.0",
        json!({
            "asac": "",
            "bizType": "",
            "channel": "",
            "drawModel": 0,
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderNo": order_id,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| {
        tracing::warn!(uid=%uid, order_id=%order_id, source=%source, error=%error, "red flower receive failed");
        error
    })?;

    let ret = red_flower_ret(&upstream);
    if !payload_ret_success(&upstream) {
        let message = red_flower_error_message(&upstream);
        let _ = upsert_red_flower_receipt_status(
            state.storage().xianyu(),
            uid,
            order_id,
            "failed",
            &ret,
        )
        .await;
        return Err(message);
    }

    upsert_red_flower_receipt_status(state.storage().xianyu(), uid, order_id, "success", &ret)
        .await
        .map_err(|error| error.to_string())?;
    Ok(red_flower_receive_issued(&upstream))
}

fn should_auto_request_red_flower(previous: Option<&Value>, current: &Value) -> bool {
    red_flower_eligible(current) && previous.map(red_flower_eligible) != Some(true)
}

async fn auto_request_red_flower_for_orders(state: &AppState, uid: &str, order_ids: &[String]) {
    for order_id in order_ids {
        let attempted: i64 = match sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM order_red_flower_requests WHERE uid=? AND order_id=?)",
        )
        .bind(uid)
        .bind(order_id)
        .fetch_one(state.storage().xianyu())
        .await
        {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(uid=%uid, order_id=%order_id, error=%error, "failed to check auto red flower request state");
                continue;
            }
        };
        if attempted != 0 {
            continue;
        }
        match request_red_flower_upstream(state, uid, order_id, true, "order_sync_auto").await {
            Ok(already_requested) => {
                tracing::info!(uid=%uid, order_id=%order_id, already_requested, "auto red flower request completed");
            }
            Err(error) => {
                tracing::warn!(uid=%uid, order_id=%order_id, error=%error, "auto red flower request failed");
            }
        }
    }
}

pub(crate) async fn run_order_adjust_price_upstream_action(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
    modify_fee: &str,
) -> Result<Value, String> {
    ensure_order_belongs_to_uid(state, uid, biz_order_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let location = load_location_or_default(state, uid).await;
    let upstream = super::successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.user.adjust.price"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderId": biz_order_id.trim(),
            "modifyFee": order_adjust_modify_fee_cents(modify_fee),
            "newTransportFee": "0",
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| error.to_string())?;
    update_order_price(
        state.storage().xianyu(),
        uid,
        biz_order_id,
        modify_fee.trim(),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(upstream)
}

pub(crate) async fn run_order_adjust_price_local_action(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
    modify_fee: &str,
) -> Result<Value, String> {
    ensure_order_belongs_to_uid(state, uid, biz_order_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    update_order_price(
        state.storage().xianyu(),
        uid,
        biz_order_id,
        modify_fee.trim(),
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(success_message("订单改价成功"))
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(text) => Ok(text),
        Value::Number(number) => Ok(number.to_string()),
        _ => Err(serde::de::Error::custom("expected string or number")),
    }
}

fn order_adjust_modify_fee_cents(modify_fee: &str) -> String {
    price_to_cents(modify_fee.trim())
}

pub(super) async fn order_delete(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    // Delete must succeed on Xianyu first; never only remove the local cache row.
    let upstream = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.order.delete"),
        "2.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "bizOrderId": payload.biz_order_id.clone(),
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| ResponseError::internal_message(format!("删除订单失败：{error}")))?;
    sqlx::query("DELETE FROM account_orders WHERE uid = ? AND tid = ?")
        .bind(&payload.uid)
        .bind(&payload.biz_order_id)
        .execute(state.storage().xianyu())
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn order_close_reason(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<OrderCloseReasonQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    ensure_order_belongs_to_uid(&state, &query.uid, &query.biz_order_id).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    if let Ok(upstream) = super::successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.order.close.reason.get"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "bizOrderId": query.biz_order_id,
            "latitude": location.latitude,
            "longitude": location.longitude,
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
            "reasons": ["不想卖了", "信息有误", "买家协商取消"],
        }
    })))
}

pub(super) async fn order_close(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderClosePayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    // Close must succeed on Xianyu first; never fake-close only the local cache.
    let upstream = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.close.by.seller"),
        "2.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "bizOrderId": payload.biz_order_id.clone(),
            "tid": payload.biz_order_id.clone(),
            "closeReason": payload.close_reason.trim(),
            "latitude": location.latitude,
            "longitude": location.longitude,
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| ResponseError::internal_message(format!("关闭订单失败：{error}")))?;
    update_order_status(
        state.storage().xianyu(),
        &payload.uid,
        &payload.biz_order_id,
        "ORDER_CLOSED",
        payload.close_reason.trim(),
    )
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn order_consign_render(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    if let Ok(upstream) = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.logistic.consign.render"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderId": payload.biz_order_id.clone(),
            "pageSource": "idleDelivery",
        }),
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(normalize_consign_render_response(
            &upstream,
            &payload.biz_order_id,
        )));
    }

    let order = fetch_order(
        state.storage().xianyu(),
        &payload.uid,
        &payload.biz_order_id,
    )
    .await
    .map_err(ResponseError::internal)?
    .ok_or_else(|| ResponseError::not_found("订单不存在"))?;
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "orderId": payload.biz_order_id,
            "idleLogisticTypes": ["OFFLINE_CONSIGN", "DUMMY_CONSIGN"],
            "senderAddressInfo": Value::Null,
            "receiverAddressInfo": Value::Null,
            "order": order,
        }
    })))
}

pub(super) async fn order_consign(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderConsignPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let mut upstream_data = json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "orderId": payload.biz_order_id.clone(),
        "cpCode": payload.company_code.trim(),
        "mailNo": payload.mail_no.trim(),
    });
    if !payload.address_id.trim().is_empty() {
        upstream_data["addressId"] = parse_json_number_or_string(payload.address_id.trim());
    }
    if let Ok(upstream) = super::successful_upstream_h5_request(
        &state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.logistic.consign.offline"),
        "1.0",
        upstream_data,
        Vec::new(),
    )
    .await
    {
        return Ok(ResponseJson(upstream));
    }
    sqlx::query(
        r#"
        UPDATE account_orders
        SET status = 'SELLER_CONSIGNED',
            status_text = '已发货',
            express_company_code = ?,
            express_company_name = ?,
            mail_no = ?,
            updated_at = ?
        WHERE uid = ? AND tid = ?
        "#,
    )
    .bind(payload.company_code.trim())
    .bind(payload.company_name.trim())
    .bind(payload.mail_no.trim())
    .bind(unix_seconds_now())
    .bind(&payload.uid)
    .bind(&payload.biz_order_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    Ok(ResponseJson(success_message("发货成功")))
}

pub(crate) async fn run_order_consign_upstream_action(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
    company_code: &str,
    company_name: &str,
    mail_no: &str,
) -> Result<Value, String> {
    ensure_order_belongs_to_uid(state, uid, biz_order_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let location = load_location_or_default(state, uid).await;
    let upstream = super::successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.logistic.consign.offline"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderId": biz_order_id.trim(),
            "cpCode": company_code.trim(),
            "mailNo": mail_no.trim(),
        }),
        Vec::new(),
    )
    .await
    .map_err(|error| error.to_string())?;
    apply_order_consign_local_update(
        state,
        uid,
        biz_order_id,
        company_code,
        company_name,
        mail_no,
    )
    .await?;
    Ok(upstream)
}

pub(crate) async fn run_order_consign_local_action(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
    company_code: &str,
    company_name: &str,
    mail_no: &str,
) -> Result<Value, String> {
    ensure_order_belongs_to_uid(state, uid, biz_order_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    apply_order_consign_local_update(
        state,
        uid,
        biz_order_id,
        company_code,
        company_name,
        mail_no,
    )
    .await?;
    Ok(success_message("发货成功"))
}

async fn apply_order_consign_local_update(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
    company_code: &str,
    company_name: &str,
    mail_no: &str,
) -> Result<(), String> {
    sqlx::query(
        r#"
        UPDATE account_orders
        SET status = 'SELLER_CONSIGNED',
            status_text = '已发货',
            express_company_code = ?,
            express_company_name = ?,
            mail_no = ?,
            updated_at = ?
        WHERE uid = ? AND tid = ?
        "#,
    )
    .bind(company_code.trim())
    .bind(company_name.trim())
    .bind(mail_no.trim())
    .bind(unix_seconds_now())
    .bind(uid.trim())
    .bind(biz_order_id.trim())
    .execute(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) async fn order_consign_dummy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<OrderPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_order_belongs_to_uid(&state, &payload.uid, &payload.biz_order_id).await?;
    let response = run_order_consign_dummy_action(&state, &payload.uid, &payload.biz_order_id)
        .await
        .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(response))
}

pub(crate) async fn order_consign_dummy_confirmed(
    state: &AppState,
    payload: &OrderPayload,
) -> Result<Value, String> {
    let upstream = order_consign_dummy_upstream(state, payload).await?;
    sqlx::query(
        r#"
        UPDATE account_orders
        SET status = 'SELLER_CONSIGNED',
            status_text = '无需快递',
            updated_at = ?
        WHERE uid = ? AND tid = ?
        "#,
    )
    .bind(unix_seconds_now())
    .bind(&payload.uid)
    .bind(&payload.biz_order_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    Ok(upstream)
}

pub(crate) async fn run_order_consign_dummy_action(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
) -> Result<Value, String> {
    let payload = OrderPayload {
        uid: uid.trim().to_string(),
        biz_order_id: biz_order_id.trim().to_string(),
    };
    ensure_order_belongs_to_uid(state, &payload.uid, &payload.biz_order_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    match run_order_consign_dummy_upstream_action(state, &payload.uid, &payload.biz_order_id).await
    {
        Ok(upstream) => return Ok(upstream),
        Err(_) => {}
    }
    sqlx::query(
        r#"
        UPDATE account_orders
        SET status = 'SELLER_CONSIGNED',
            status_text = '无需快递',
            updated_at = ?
        WHERE uid = ? AND tid = ?
        "#,
    )
    .bind(unix_seconds_now())
    .bind(&payload.uid)
    .bind(&payload.biz_order_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    Ok(success_message("无需快递发货成功"))
}

pub(crate) async fn run_order_consign_dummy_upstream_action(
    state: &AppState,
    uid: &str,
    biz_order_id: &str,
) -> Result<Value, String> {
    let payload = OrderPayload {
        uid: uid.trim().to_string(),
        biz_order_id: biz_order_id.trim().to_string(),
    };
    let upstream = order_consign_dummy_upstream(state, &payload).await?;
    sqlx::query(
        r#"
        UPDATE account_orders
        SET status = 'SELLER_CONSIGNED',
            status_text = '无需快递',
            updated_at = ?
        WHERE uid = ? AND tid = ?
        "#,
    )
    .bind(unix_seconds_now())
    .bind(&payload.uid)
    .bind(&payload.biz_order_id)
    .execute(state.storage().xianyu())
    .await
    .map_err(|error| error.to_string())?;
    Ok(upstream)
}

async fn order_consign_dummy_upstream(
    state: &AppState,
    payload: &OrderPayload,
) -> Result<Value, String> {
    ensure_order_belongs_to_uid(state, &payload.uid, &payload.biz_order_id)
        .await
        .map_err(|error| error.detail().to_string())?;
    let location = load_location_or_default(&state, &payload.uid).await;
    super::successful_upstream_h5_request(
        state,
        &payload.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.logistic.consign.dummy"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderId": payload.biz_order_id.clone(),
            "newUnconsign": true,
            "source": "normal",
        }),
        Vec::new(),
    )
    .await
}

pub(super) async fn trade_orders(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<TradeOrdersQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    if query.force {
        let _ = sync_orders_to_db(&state, &query.uid, query.seller).await;
    }
    let mut payload = trade_orders_payload(state.storage().xianyu(), &query.uid, query.force)
        .await
        .map_err(ResponseError::internal)?;
    let sync_state = read_sync_state(state.storage().xianyu(), &query.uid, "orders").await;
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "_last_synced_at".to_string(),
            json!(sync_state.as_ref().map(|s| s.last_synced_at).unwrap_or(0.0)),
        );
        object.insert(
            "_last_sync_ok".to_string(),
            json!(sync_state.as_ref().map(|s| s.last_ok).unwrap_or(false)),
        );
        object.insert(
            "_last_sync_reason".to_string(),
            json!(sync_state
                .as_ref()
                .map(|s| s.last_reason.clone())
                .unwrap_or_default()),
        );
    }
    Ok(ResponseJson(payload))
}

pub(super) async fn trade_bill(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    if let Ok(upstream) = super::successful_upstream_h5_request(
        &state,
        &query.uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.bill.list"),
        "1.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
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
            "billList": [],
        }
    })))
}

// ---------------------------------------------------------------------------
// DB sync helpers
// ---------------------------------------------------------------------------

pub(crate) async fn sync_orders_to_db(
    state: &AppState,
    uid: &str,
    seller: bool,
) -> Result<SyncOutcome, String> {
    let pool = state.storage().xianyu();
    let collected = match fetch_orders_batch(state, uid, seller).await {
        Ok(collected) => collected,
        Err(message) => {
            let now = unix_millis_now() as f64 / 1000.0;
            let _ = write_sync_state(pool, uid, "orders", now, false, &message, 0).await;
            return Ok(SyncOutcome {
                ok: false,
                total: 0,
                synced_at: now,
                detail: message,
            });
        }
    };

    let mut auto_request_order_ids = Vec::new();
    for order in &collected {
        let tid = value_str(order, "tid");
        if tid.is_empty() {
            continue;
        }
        let previous = fetch_order(pool, uid, &tid).await.ok().flatten();
        if should_auto_request_red_flower(previous.as_ref(), order) {
            auto_request_order_ids.push(tid);
        }
    }

    let now = unix_millis_now() as f64 / 1000.0;
    let persist = persist_orders_from_current_batch(pool, uid, &collected, now, None).await;
    if seller && !auto_request_order_ids.is_empty() {
        auto_request_red_flower_for_orders(state, uid, &auto_request_order_ids).await;
    }
    let sync_ok = persist.failed == 0;
    let detail = if sync_ok {
        format!("orders={}", persist.persisted)
    } else {
        format!("orders={},failed={}", persist.persisted, persist.failed)
    };
    let reason = if sync_ok { "ok" } else { "partial_failure" };
    let _ = write_sync_state(pool, uid, "orders", now, sync_ok, reason, persist.persisted).await;
    Ok(SyncOutcome {
        ok: sync_ok,
        total: persist.persisted,
        synced_at: now,
        detail,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoritativeOrderSnapshot {
    pub(crate) status: String,
    pub(crate) status_text: String,
    pub(crate) purchase_quantity: Option<i64>,
    pub(crate) purchase_quantity_source: String,
    pub(crate) refreshed_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthoritativeTargetPersistence {
    MissingFromCurrentBatch,
    Persisted,
    PersistFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OrderSyncPersistOutcome {
    persisted: i64,
    failed: i64,
    target_persistence: Option<AuthoritativeTargetPersistence>,
    target_snapshot: Option<AuthoritativeOrderSnapshot>,
}

pub(crate) async fn refresh_authoritative_order_snapshot(
    state: &AppState,
    uid: &str,
    order_id: &str,
) -> Result<AuthoritativeOrderSnapshot, String> {
    let pool = state.storage().xianyu();
    let collected = match fetch_orders_batch(state, uid, true).await {
        Ok(collected) => collected,
        Err(message) => {
            let now = unix_millis_now() as f64 / 1000.0;
            let _ = write_sync_state(pool, uid, "orders", now, false, &message, 0).await;
            return Err(message);
        }
    };
    let now = unix_millis_now() as f64 / 1000.0;
    let refreshed_at = unix_millis_now();
    let persist = persist_orders_from_current_batch(
        pool,
        uid,
        &collected,
        now,
        Some((order_id, refreshed_at)),
    )
    .await;
    let (sync_ok, reason) = match persist.target_persistence {
        Some(AuthoritativeTargetPersistence::PersistFailed) => (false, "target_persist_failed"),
        Some(AuthoritativeTargetPersistence::MissingFromCurrentBatch) | None => {
            (false, "target_missing")
        }
        Some(AuthoritativeTargetPersistence::Persisted) if persist.failed > 0 => {
            (false, "partial_failure")
        }
        Some(AuthoritativeTargetPersistence::Persisted) => (true, "ok"),
    };
    let _ = write_sync_state(pool, uid, "orders", now, sync_ok, reason, persist.persisted).await;

    match persist.target_persistence {
        Some(AuthoritativeTargetPersistence::Persisted) => persist
            .target_snapshot
            .ok_or_else(|| "订单同步后未确认目标订单已刷新落库，请稍后重试".to_string()),
        Some(AuthoritativeTargetPersistence::MissingFromCurrentBatch) | None => {
            Err("订单同步后未确认目标订单已刷新落库，请稍后重试".to_string())
        }
        Some(AuthoritativeTargetPersistence::PersistFailed) => {
            Err("目标订单刷新落库失败，请稍后重试".to_string())
        }
    }
}

async fn fetch_orders_batch(
    state: &AppState,
    uid: &str,
    seller: bool,
) -> Result<Vec<Value>, String> {
    let location = load_location_or_default(state, uid).await;
    let mut collected: Vec<Value> = Vec::new();
    let mut page = 1_i64;
    loop {
        let raw = super::successful_upstream_h5_request(
            state,
            uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.order.search"),
            "2.0",
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "pageNumber": page,
                "offsetRow": "0",
                "seller": seller,
                "latitude": location.latitude,
                "longitude": location.longitude,
            }),
            Vec::new(),
        )
        .await;
        let Ok(raw) = raw else {
            let mut msg = raw.err().unwrap_or_else(|| "upstream error".to_string());
            if auth_expired_message(&msg) {
                let _ = crate::cookie_refresh::mark_business_session_failure(
                    state.storage().accounts(),
                    uid,
                    &format!("sync_orders_failed: {msg}"),
                )
                .await;
                msg = "会话已过期，已尝试自动恢复但失败，请刷新账号登录态后重试同步".to_string();
            }
            return Err(msg);
        };
        let orders = parse_order_list_from_upstream(&raw);
        if orders.is_empty() {
            break;
        }
        collected.extend(orders);
        let data = raw.get("data").cloned().unwrap_or(Value::Null);
        let has_more = data
            .get("hasMore")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_more {
            break;
        }
        page += 1;
        if page > 50 {
            break;
        }
    }
    Ok(collected)
}

async fn persist_orders_from_current_batch(
    pool: &SqlitePool,
    uid: &str,
    orders: &[Value],
    now: f64,
    authoritative_target: Option<(&str, i64)>,
) -> OrderSyncPersistOutcome {
    let target = authoritative_target
        .map(|(order_id, refreshed_at)| (order_id.trim().to_string(), refreshed_at));
    let mut persisted = 0_i64;
    let mut failed = 0_i64;
    let mut target_seen = false;
    let mut target_persist_failed = false;
    let mut target_snapshot = None;

    for order in orders {
        let tid = value_str(order, "tid");
        let is_target = target
            .as_ref()
            .map(|(order_id, _)| !order_id.is_empty() && tid == *order_id)
            .unwrap_or(false);
        match upsert_account_order(pool, uid, order, now).await {
            Ok(()) => {
                persisted += 1;
                if let Some((_, refreshed_at)) = target.as_ref() {
                    if is_target {
                        target_seen = true;
                        target_snapshot =
                            Some(authoritative_snapshot_from_order(order, *refreshed_at));
                    }
                }
            }
            Err(_) => {
                failed += 1;
                if is_target {
                    target_seen = true;
                    target_persist_failed = true;
                }
            }
        }
    }

    let target_persistence = target.map(|_| {
        if target_persist_failed {
            AuthoritativeTargetPersistence::PersistFailed
        } else if target_seen {
            AuthoritativeTargetPersistence::Persisted
        } else {
            AuthoritativeTargetPersistence::MissingFromCurrentBatch
        }
    });

    OrderSyncPersistOutcome {
        persisted,
        failed,
        target_persistence,
        target_snapshot,
    }
}

fn authoritative_snapshot_from_order(
    order: &Value,
    refreshed_at: i64,
) -> AuthoritativeOrderSnapshot {
    AuthoritativeOrderSnapshot {
        status: value_str(order, "status"),
        status_text: value_str(order, "statusText"),
        purchase_quantity: order.get("purchaseQuantity").and_then(Value::as_i64),
        purchase_quantity_source: value_str(order, "purchaseQuantitySource"),
        refreshed_at,
    }
}

async fn sync_sold_orders_to_db(
    state: &AppState,
    uid: &str,
    page: i64,
    status: &str,
) -> Result<SyncOutcome, String> {
    let location = load_location_or_default(state, uid).await;
    let pool = state.storage().xianyu();
    let raw = super::successful_upstream_h5_request(
        state,
        uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.trade.sold.get"),
        "6.0",
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "orderStatus": status,
            "pageNumber": page,
            "offsetRow": 0,
        }),
        Vec::new(),
    )
    .await;
    let raw = match raw {
        Ok(value) => value,
        Err(mut msg) => {
            if auth_expired_message(&msg) {
                let _ = crate::cookie_refresh::mark_business_session_failure(
                    state.storage().accounts(),
                    uid,
                    &format!("sync_sold_orders_failed: {msg}"),
                )
                .await;
                msg = "会话已过期，已尝试自动恢复但失败，请刷新账号登录态后重试同步".to_string();
            }
            let now = unix_seconds_now();
            let _ = write_sync_state(pool, uid, "orders", now, false, &msg, 0).await;
            return Ok(SyncOutcome {
                ok: false,
                total: 0,
                synced_at: now,
                detail: msg,
            });
        }
    };

    let orders = parse_order_list_from_upstream(&raw);
    let now = unix_seconds_now();
    let mut persisted = 0_i64;
    let mut auto_request_order_ids = Vec::new();
    for order in &orders {
        let tid = value_str(order, "tid");
        let previous = if tid.is_empty() {
            None
        } else {
            fetch_order(pool, uid, &tid).await.ok().flatten()
        };
        if upsert_account_order(pool, uid, order, now).await.is_ok() {
            persisted += 1;
            if should_auto_request_red_flower(previous.as_ref(), order) {
                auto_request_order_ids.push(tid);
            }
        }
    }
    if !auto_request_order_ids.is_empty() {
        auto_request_red_flower_for_orders(state, uid, &auto_request_order_ids).await;
    }
    let _ = write_sync_state(pool, uid, "orders", now, true, "ok", persisted).await;
    Ok(SyncOutcome {
        ok: true,
        total: persisted,
        synced_at: now,
        detail: format!("orders={}", persisted),
    })
}

async fn upsert_account_order(
    pool: &SqlitePool,
    uid: &str,
    order: &Value,
    now: f64,
) -> Result<(), sqlx::Error> {
    let tid = value_str(order, "tid");
    if tid.is_empty() {
        return Ok(());
    }
    sqlx::query(
        r#"
        INSERT INTO account_orders (
            tid, uid, item_id, title, price, pic_url, status, status_text, buyer_id, buyer_nick,
            buyer_avatar, gmt_create, express_company_code, express_company_name, mail_no,
            purchase_quantity, purchase_quantity_source, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, '', '', '', ?, ?, ?, ?)
        ON CONFLICT(uid, tid) DO UPDATE SET
            item_id = CASE WHEN TRIM(excluded.item_id) <> '' THEN excluded.item_id ELSE account_orders.item_id END,
            title = excluded.title,
            price = excluded.price,
            pic_url = excluded.pic_url,
            status = excluded.status,
            status_text = excluded.status_text,
            buyer_id = excluded.buyer_id,
            buyer_nick = excluded.buyer_nick,
            buyer_avatar = excluded.buyer_avatar,
            gmt_create = excluded.gmt_create,
            purchase_quantity = CASE
                WHEN ? = 'absent' THEN account_orders.purchase_quantity
                ELSE excluded.purchase_quantity
            END,
            purchase_quantity_source = CASE
                WHEN ? = 'absent' THEN account_orders.purchase_quantity_source
                ELSE excluded.purchase_quantity_source
            END,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&tid)
    .bind(uid)
    .bind(order_item_id(order))
    .bind(value_str(order, "title"))
    .bind(value_str(order, "price"))
    .bind(value_str(order, "picUrl"))
    .bind(value_str(order, "status"))
    .bind(value_str(order, "statusText"))
    .bind(value_str(order, "buyerId"))
    .bind(value_str(order, "buyerNick"))
    .bind(value_str(order, "buyerAvatar"))
    .bind(value_str(order, "gmtCreate"))
    .bind(order.get("purchaseQuantity").and_then(Value::as_i64))
    .bind(value_str(order, "purchaseQuantitySource"))
    .bind(now)
    .bind(now)
    .bind(value_str(order, "purchaseQuantityState"))
    .bind(value_str(order, "purchaseQuantityState"))
    .execute(pool)
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// Query / transform helpers
// ---------------------------------------------------------------------------

async fn trade_orders_payload(
    pool: &SqlitePool,
    uid: &str,
    force: bool,
) -> Result<Value, sqlx::Error> {
    let rows = sqlx::query(
        r#"
        SELECT *
        FROM account_orders
        WHERE uid = ?
        ORDER BY updated_at DESC, created_at DESC, tid DESC
        "#,
    )
    .bind(uid)
    .fetch_all(pool)
    .await?;
    let order_list = rows.into_iter().map(order_row_to_value).collect::<Vec<_>>();
    Ok(json!({
        "ret": success_ret(),
        "_cached": !force,
        "_cached_at": unix_seconds_now(),
        "data": {
            "orderList": order_list,
            "totalCount": order_list.len(),
            "hasMore": false,
        }
    }))
}

async fn fetch_order(
    pool: &SqlitePool,
    uid: &str,
    tid: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query("SELECT * FROM account_orders WHERE uid = ? AND tid = ?")
        .bind(uid)
        .bind(tid)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(order_row_to_value))
}

async fn update_order_price(
    pool: &SqlitePool,
    uid: &str,
    tid: &str,
    price: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE account_orders SET price = ?, updated_at = ? WHERE uid = ? AND tid = ?")
        .bind(price)
        .bind(unix_seconds_now())
        .bind(uid)
        .bind(tid)
        .execute(pool)
        .await?;
    Ok(())
}

async fn update_order_status(
    pool: &SqlitePool,
    uid: &str,
    tid: &str,
    status: &str,
    status_text: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE account_orders SET status = ?, status_text = ?, updated_at = ? WHERE uid = ? AND tid = ?",
    )
    .bind(status)
    .bind(status_text)
    .bind(unix_seconds_now())
    .bind(uid)
    .bind(tid)
    .execute(pool)
    .await?;
    Ok(())
}

fn order_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    let tid: String = row.get("tid");
    let title: String = row.get("title");
    let price: String = row.get("price");
    let purchase_quantity = row.get::<Option<i64>, _>("purchase_quantity");
    let purchase_quantity_source = row
        .get::<Option<String>, _>("purchase_quantity_source")
        .unwrap_or_default();
    json!({
        "tid": tid.clone(),
        "orderId": tid.clone(),
        "bizOrderId": tid,
        "itemId": row.get::<String, _>("item_id"),
        "title": title.clone(),
        "itemTitle": title,
        "price": price.clone(),
        "totalFee": price,
        "picUrl": row.get::<String, _>("pic_url"),
        "status": row.get::<String, _>("status"),
        "statusText": row.get::<String, _>("status_text"),
        "buyerId": row.get::<String, _>("buyer_id"),
        "buyerNick": row.get::<String, _>("buyer_nick"),
        "buyerAvatar": row.get::<String, _>("buyer_avatar"),
        "gmtCreate": row.get::<String, _>("gmt_create"),
        "purchaseQuantity": purchase_quantity,
        "purchaseQuantitySource": purchase_quantity_source,
    })
}

fn order_item_id(order: &Value) -> String {
    [
        value_str(order, "itemId"),
        value_str(order, "item_id"),
        value_str(order, "auctionId"),
        value_str(order, "auctionItemId"),
        value_str(order, "item_id_str"),
    ]
    .into_iter()
    .find(|value| !value.trim().is_empty())
    .unwrap_or_default()
}

fn parse_json_number_or_string(text: &str) -> Value {
    text.parse::<i64>()
        .map(Value::from)
        .unwrap_or_else(|_| Value::String(text.to_string()))
}

// ---------------------------------------------------------------------------
// Order list parsing from upstream responses
// ---------------------------------------------------------------------------

const PURCHASE_QUANTITY_COMMON_SOURCE: &str = "trade.order.search:candidate:commonData.quantity";
const PURCHASE_QUANTITY_DETAIL_SOURCE: &str =
    "trade.order.search:candidate:content.data.detailInfo.quantity";
const PURCHASE_QUANTITY_CONSISTENT_CARD_SOURCE: &str =
    "trade.order.search:candidate:commonData.quantity+content.data.detailInfo.quantity";
const PURCHASE_QUANTITY_GENERIC_SOURCE: &str = "trade.order.search:candidate:quantity";

fn positive_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().filter(|quantity| *quantity > 0),
        Value::String(text)
            if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            text.parse::<i64>().ok().filter(|quantity| *quantity > 0)
        }
        _ => None,
    }
}

enum CardQuantityCandidate {
    Absent,
    Valid(i64),
    Invalid,
}

#[derive(Clone, Copy)]
enum PurchaseQuantityState {
    Absent,
    Valid,
    Invalid,
}

impl PurchaseQuantityState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
        }
    }
}

fn card_quantity_candidate(object: &serde_json::Map<String, Value>) -> CardQuantityCandidate {
    match object.get("quantity") {
        None => CardQuantityCandidate::Absent,
        Some(value) => positive_integer(value)
            .map(CardQuantityCandidate::Valid)
            .unwrap_or(CardQuantityCandidate::Invalid),
    }
}

fn card_purchase_quantity(
    common: &serde_json::Map<String, Value>,
    detail: &serde_json::Map<String, Value>,
) -> (Option<i64>, &'static str, PurchaseQuantityState) {
    let common_quantity = card_quantity_candidate(common);
    let detail_quantity = card_quantity_candidate(detail);
    match (common_quantity, detail_quantity) {
        (CardQuantityCandidate::Valid(common), CardQuantityCandidate::Valid(detail))
            if common == detail =>
        {
            (
                Some(common),
                PURCHASE_QUANTITY_CONSISTENT_CARD_SOURCE,
                PurchaseQuantityState::Valid,
            )
        }
        (CardQuantityCandidate::Valid(quantity), CardQuantityCandidate::Absent) => (
            Some(quantity),
            PURCHASE_QUANTITY_COMMON_SOURCE,
            PurchaseQuantityState::Valid,
        ),
        (CardQuantityCandidate::Absent, CardQuantityCandidate::Valid(quantity)) => (
            Some(quantity),
            PURCHASE_QUANTITY_DETAIL_SOURCE,
            PurchaseQuantityState::Valid,
        ),
        (CardQuantityCandidate::Absent, CardQuantityCandidate::Absent) => {
            (None, "", PurchaseQuantityState::Absent)
        }
        _ => (None, "", PurchaseQuantityState::Invalid),
    }
}

fn generic_purchase_quantity(
    object: &serde_json::Map<String, Value>,
) -> (Option<i64>, &'static str, PurchaseQuantityState) {
    match object.get("quantity") {
        None => (None, "", PurchaseQuantityState::Absent),
        Some(value) => match positive_integer(value) {
            Some(quantity) => (
                Some(quantity),
                PURCHASE_QUANTITY_GENERIC_SOURCE,
                PurchaseQuantityState::Valid,
            ),
            None => (None, "", PurchaseQuantityState::Invalid),
        },
    }
}

pub(super) fn parse_order_list_from_upstream(raw: &Value) -> Vec<Value> {
    let items = raw
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !items.is_empty() && items[0].get("commonData").is_some() {
        return items
            .into_iter()
            .filter_map(|item| {
                let common = item.get("commonData")?.as_object()?.clone();
                let content_data = item
                    .get("content")
                    .and_then(Value::as_object)
                    .and_then(|content| content.get("data"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let detail = content_data
                    .get("detailInfo")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let price_info = content_data
                    .get("priceInfo")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let head_data = item
                    .get("head")
                    .and_then(Value::as_object)
                    .and_then(|head| head.get("data"))
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let user_info = head_data
                    .get("userInfo")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                let buyer_nick = [
                    user_info.get("userName"),
                    user_info.get("userNick"),
                    user_info.get("nickName"),
                    user_info.get("displayName"),
                    common.get("peerUserNick"),
                    common.get("buyerNick"),
                    head_data.get("peerUserNick"),
                ]
                .into_iter()
                .flatten()
                .map(super::value_to_string_ref)
                .find(|value| !value.is_empty())
                .unwrap_or_default();
                let (purchase_quantity, purchase_quantity_source, purchase_quantity_state) =
                    card_purchase_quantity(&common, &detail);
                Some(json!({
                    "tid": common.get("orderIdStr").or_else(|| common.get("orderId")).map(super::value_to_string_ref).unwrap_or_default(),
                    "itemId": detail.get("auctionId")
                        .or_else(|| detail.get("itemId"))
                        .or_else(|| common.get("itemId"))
                        .map(super::value_to_string_ref)
                        .unwrap_or_default(),
                    "title": detail.get("auctionTitle").and_then(Value::as_str).unwrap_or_default(),
                    "price": price_info.get("price").map(super::value_to_string_ref).unwrap_or_default(),
                    "picUrl": detail.get("auctionPic").and_then(Value::as_str).unwrap_or_default(),
                    "status": common.get("tradeStatusEnum").and_then(Value::as_str).unwrap_or_default(),
                    "statusText": head_data.get("statusViewMsg").and_then(Value::as_str).unwrap_or_default(),
                    "buyerId": common.get("peerUserId").map(super::value_to_string_ref).unwrap_or_default(),
                    "buyerNick": buyer_nick,
                    "buyerAvatar": user_info.get("avatarUrl").or_else(|| user_info.get("avatar")).map(super::value_to_string_ref).unwrap_or_default(),
                    "gmtCreate": head_data.get("createTime").and_then(Value::as_str).unwrap_or_default(),
                    "purchaseQuantity": purchase_quantity,
                    "purchaseQuantitySource": purchase_quantity_source,
                    "purchaseQuantityState": purchase_quantity_state.as_str(),
                }))
            })
            .collect();
    }

    raw.get("data")
        .and_then(Value::as_object)
        .and_then(|data| {
            data.get("orderList")
                .or_else(|| data.get("tradeList"))
                .or_else(|| data.get("resultList"))
                .or_else(|| data.get("orderCards"))
        })
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let object = item.as_object()?.clone();
            let (purchase_quantity, purchase_quantity_source, purchase_quantity_state) =
                generic_purchase_quantity(&object);
            Some(json!({
                "tid": object.get("tid").or_else(|| object.get("id")).or_else(|| object.get("orderId")).map(super::value_to_string_ref).unwrap_or_default(),
                "itemId": object.get("itemId")
                    .or_else(|| object.get("item_id"))
                    .or_else(|| object.get("auctionId"))
                    .or_else(|| object.get("auctionItemId"))
                    .map(super::value_to_string_ref)
                    .unwrap_or_default(),
                "title": object.get("itemTitle").or_else(|| object.get("title")).and_then(Value::as_str).unwrap_or_default(),
                "price": object.get("actualFee").or_else(|| object.get("payment")).or_else(|| object.get("price")).map(super::value_to_string_ref).unwrap_or_default(),
                "picUrl": object.get("itemPicUrl").or_else(|| object.get("picUrl")).or_else(|| object.get("pic")).and_then(Value::as_str).unwrap_or_default(),
                "status": object.get("status").or_else(|| object.get("orderStatus")).and_then(Value::as_str).unwrap_or_default(),
                "statusText": object.get("statusText").or_else(|| object.get("orderStatusText")).or_else(|| object.get("statusStr")).and_then(Value::as_str).unwrap_or_default(),
                "buyerId": object.get("buyerId").or_else(|| object.get("peerUserId")).map(super::value_to_string_ref).unwrap_or_default(),
                "buyerNick": object.get("buyerNick").or_else(|| object.get("buyerName")).or_else(|| object.get("buyer")).and_then(Value::as_str).unwrap_or_default(),
                "buyerAvatar": object.get("buyerAvatar").and_then(Value::as_str).unwrap_or_default(),
                "gmtCreate": object.get("gmtCreate").or_else(|| object.get("created")).and_then(Value::as_str).unwrap_or_default(),
                "purchaseQuantity": purchase_quantity,
                "purchaseQuantitySource": purchase_quantity_source,
                "purchaseQuantityState": purchase_quantity_state.as_str(),
            }))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Consign render normalization helpers
// ---------------------------------------------------------------------------

fn extract_consign_render_data(upstream: &Value) -> Value {
    let outer = upstream
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or(Value::Null);
    let nested = outer
        .get("data")
        .map(decode_maybe_json_string)
        .unwrap_or_else(|| outer.clone());
    nested
}

fn logistic_address_line(address: &Value) -> String {
    let formatted = value_to_string_lossy(address.get("formatFullAddress"));
    if !formatted.is_empty() {
        return formatted;
    }

    let direct = value_to_string_lossy(address.get("address"));
    if !direct.contains("^^^") && !direct.is_empty() {
        return direct;
    }

    [
        value_to_string_lossy(address.get("province")),
        value_to_string_lossy(address.get("city")),
        value_to_string_lossy(address.get("area")),
        value_to_string_lossy(address.get("town")),
        value_to_string_lossy(address.get("addressDetail")),
    ]
    .into_iter()
    .filter(|part| !part.trim().is_empty())
    .collect::<Vec<_>>()
    .join("")
}

fn normalize_logistic_address(address: Option<&Value>) -> Value {
    let Some(address) = address else {
        return Value::Null;
    };
    if !address.is_object() {
        return Value::Null;
    }

    let full_name = value_to_string_lossy(address.get("fullName"));
    let mobile = value_to_string_lossy(address.get("mobile"));
    let full_address = logistic_address_line(address);
    let copy_text = [full_name.as_str(), mobile.as_str(), full_address.as_str()]
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    json!({
        "addressId": value_to_string_lossy(address.get("addressId")),
        "fullName": full_name,
        "mobile": mobile,
        "formatFullAddress": full_address,
        "province": value_to_string_lossy(address.get("province")),
        "city": value_to_string_lossy(address.get("city")),
        "area": value_to_string_lossy(address.get("area")),
        "town": value_to_string_lossy(address.get("town")),
        "addressDetail": value_to_string_lossy(address.get("addressDetail")),
        "copyText": copy_text,
        "raw": address.clone(),
    })
}

fn normalize_consign_render_response(upstream: &Value, fallback_order_id: &str) -> Value {
    let data = extract_consign_render_data(upstream);
    let order_id = value_to_string_lossy(data.get("orderId"));
    json!({
        "ret": upstream.get("ret").cloned().unwrap_or_else(success_ret_value),
        "data": {
            "orderId": if order_id.is_empty() { fallback_order_id.to_string() } else { order_id },
            "idleLogisticTypes": data
                .get("idleLogisticTypes")
                .cloned()
                .unwrap_or_else(|| json!(["OFFLINE_CONSIGN", "DUMMY_CONSIGN"])),
            "senderAddressInfo": normalize_logistic_address(data.get("senderAddressInfo")),
            "receiverAddressInfo": normalize_logistic_address(data.get("receiverAddressInfo")),
            "consignNoticeInfos": data
                .get("consignNoticeInfos")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "raw": data,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{
        order_adjust_modify_fee_cents, order_item_id, order_row_to_value,
        parse_order_list_from_upstream, persist_orders_from_current_batch, red_flower_eligible,
        red_flower_error_message, red_flower_receive_issued, should_auto_request_red_flower,
        upsert_account_order, AuthoritativeTargetPersistence, OrderAdjustPayload,
    };
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};

    async fn setup_orders_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory sqlite");
        sqlx::query(
            r#"
            CREATE TABLE account_orders (
                tid TEXT NOT NULL,
                uid TEXT NOT NULL,
                item_id TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL DEFAULT '',
                price TEXT NOT NULL DEFAULT '',
                pic_url TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT '',
                status_text TEXT NOT NULL DEFAULT '',
                buyer_id TEXT NOT NULL DEFAULT '',
                buyer_nick TEXT NOT NULL DEFAULT '',
                buyer_avatar TEXT NOT NULL DEFAULT '',
                gmt_create TEXT NOT NULL DEFAULT '',
                express_company_code TEXT NOT NULL DEFAULT '',
                express_company_name TEXT NOT NULL DEFAULT '',
                mail_no TEXT NOT NULL DEFAULT '',
                created_at REAL NOT NULL DEFAULT 0,
                updated_at REAL NOT NULL DEFAULT 0,
                purchase_quantity INTEGER,
                purchase_quantity_source TEXT,
                PRIMARY KEY (uid, tid)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("create account_orders");
        pool
    }

    fn current_order(
        tid: &str,
        status: &str,
        status_text: &str,
        purchase_quantity: Option<i64>,
        purchase_quantity_source: &str,
    ) -> serde_json::Value {
        json!({
            "tid": tid,
            "itemId": "item-1",
            "status": status,
            "statusText": status_text,
            "purchaseQuantity": purchase_quantity,
            "purchaseQuantitySource": purchase_quantity_source,
        })
    }

    async fn reject_target_upsert(pool: &SqlitePool, tid: &str) {
        let escaped_tid = tid.replace('\'', "''");
        let insert_trigger = format!(
            r#"
            CREATE TRIGGER reject_target_insert
            BEFORE INSERT ON account_orders
            WHEN NEW.tid = '{escaped_tid}'
            BEGIN
                SELECT RAISE(FAIL, 'reject target upsert');
            END;
            "#
        );
        let update_trigger = format!(
            r#"
            CREATE TRIGGER reject_target_update
            BEFORE UPDATE ON account_orders
            WHEN NEW.tid = '{escaped_tid}'
            BEGIN
                SELECT RAISE(FAIL, 'reject target upsert');
            END;
            "#
        );
        sqlx::raw_sql(&insert_trigger)
            .execute(pool)
            .await
            .expect("create reject_target_insert trigger");
        sqlx::raw_sql(&update_trigger)
            .execute(pool)
            .await
            .expect("create reject_target_update trigger");
    }

    #[test]
    fn order_adjust_payload_accepts_numeric_modify_fee_from_frontend() {
        let payload: OrderAdjustPayload = serde_json::from_value(json!({
            "uid": "uid-1",
            "biz_order_id": "order-1",
            "modify_fee": 67.5
        }))
        .unwrap();

        assert_eq!(payload.modify_fee, "67.5");
    }

    #[test]
    fn order_adjust_modify_fee_uses_cent_units_for_upstream() {
        assert_eq!(order_adjust_modify_fee_cents("67.5"), "6750");
        assert_eq!(order_adjust_modify_fee_cents("66.00"), "6600");
    }

    #[test]
    fn order_item_id_is_normalized_from_trade_order_payloads() {
        let orders = parse_order_list_from_upstream(&json!({
            "data": {
                "orderList": [{
                    "tid": "order-1",
                    "itemId": "item-1",
                    "itemTitle": "自动发货商品"
                }]
            }
        }));

        assert_eq!(order_item_id(&orders[0]), "item-1");
    }

    #[test]
    fn card_purchase_quantity_uses_only_common_and_detail_candidates() {
        let common_only = parse_order_list_from_upstream(&json!({
            "data": {"items": [{
                "commonData": {"orderIdStr": "common", "quantity": "2"},
                "content": {"data": {"detailInfo": {"auctionId": "item-1"}}}
            }]}
        }));
        assert_eq!(common_only[0]["purchaseQuantity"], 2);
        assert_eq!(
            common_only[0]["purchaseQuantitySource"],
            "trade.order.search:candidate:commonData.quantity"
        );

        let detail_only = parse_order_list_from_upstream(&json!({
            "data": {"items": [{
                "commonData": {"orderIdStr": "detail"},
                "content": {"data": {"detailInfo": {
                    "auctionId": "item-2",
                    "quantity": 4
                }}}
            }]}
        }));
        assert_eq!(detail_only[0]["purchaseQuantity"], 4);
        assert_eq!(
            detail_only[0]["purchaseQuantitySource"],
            "trade.order.search:candidate:content.data.detailInfo.quantity"
        );

        let consistent = parse_order_list_from_upstream(&json!({
            "data": {"items": [{
                "commonData": {"orderIdStr": "consistent", "quantity": 5},
                "content": {"data": {"detailInfo": {
                    "auctionId": "item-3",
                    "quantity": "5"
                }}}
            }]}
        }));
        assert_eq!(consistent[0]["purchaseQuantity"], 5);
        assert_eq!(
            consistent[0]["purchaseQuantitySource"],
            "trade.order.search:candidate:commonData.quantity+content.data.detailInfo.quantity"
        );
    }

    #[test]
    fn card_purchase_quantity_conflict_stays_unknown() {
        let orders = parse_order_list_from_upstream(&json!({
            "data": {"items": [{
                "commonData": {"orderIdStr": "conflict", "quantity": 2},
                "content": {"data": {"detailInfo": {
                    "auctionId": "item-1",
                    "quantity": 3
                }}}
            }]}
        }));

        assert_eq!(orders[0]["purchaseQuantity"], serde_json::Value::Null);
        assert_eq!(orders[0]["purchaseQuantitySource"], "");
    }

    #[test]
    fn card_purchase_quantity_rejects_present_invalid_detail_when_common_is_valid() {
        for invalid_detail in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("0"),
            json!("-1"),
            json!("1.5"),
            json!("not-a-number"),
        ] {
            let orders = parse_order_list_from_upstream(&json!({
                "data": {"items": [{
                    "commonData": {"orderIdStr": "invalid-detail", "quantity": 2},
                    "content": {"data": {"detailInfo": {
                        "auctionId": "item-1",
                        "quantity": invalid_detail
                    }}}
                }]}
            }));

            assert_eq!(
                orders[0]["purchaseQuantity"],
                serde_json::Value::Null,
                "invalid detail quantity must reject both candidates: {invalid_detail}"
            );
            assert_eq!(orders[0]["purchaseQuantitySource"], "");
        }
    }

    #[test]
    fn card_purchase_quantity_rejects_present_invalid_common_when_detail_is_valid() {
        for invalid_common in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("0"),
            json!("-1"),
            json!("1.5"),
            json!("not-a-number"),
        ] {
            let orders = parse_order_list_from_upstream(&json!({
                "data": {"items": [{
                    "commonData": {
                        "orderIdStr": "invalid-common",
                        "quantity": invalid_common
                    },
                    "content": {"data": {"detailInfo": {
                        "auctionId": "item-1",
                        "quantity": 2
                    }}}
                }]}
            }));

            assert_eq!(
                orders[0]["purchaseQuantity"],
                serde_json::Value::Null,
                "invalid common quantity must reject both candidates: {invalid_common}"
            );
            assert_eq!(orders[0]["purchaseQuantitySource"], "");
        }
    }

    #[test]
    fn generic_purchase_quantity_ignores_guesses_and_rejects_non_positive_or_fractional_values() {
        let orders = parse_order_list_from_upstream(&json!({
            "data": {"orderList": [
                {"tid": "valid", "quantity": 3, "itemCount": 99, "count": 98, "num": 97, "amount": "88.00"},
                {"tid": "string", "quantity": "4"},
                {"tid": "missing", "itemCount": 99, "count": 98, "num": 97, "amount": 88},
                {"tid": "zero", "quantity": 0},
                {"tid": "negative", "quantity": -1},
                {"tid": "fraction", "quantity": 1.5},
                {"tid": "zero-string", "quantity": "0"},
                {"tid": "fraction-string", "quantity": "1.5"}
            ]}
        }));

        assert_eq!(orders[0]["purchaseQuantity"], 3);
        assert_eq!(
            orders[0]["purchaseQuantitySource"],
            "trade.order.search:candidate:quantity"
        );
        assert_eq!(orders[1]["purchaseQuantity"], 4);
        for order in &orders[2..] {
            assert_eq!(order["purchaseQuantity"], serde_json::Value::Null);
            assert_eq!(order["purchaseQuantitySource"], "");
        }
    }

    #[tokio::test]
    async fn order_upsert_keeps_known_purchase_quantity_when_refresh_omits_it() {
        let pool = setup_orders_pool().await;
        let first = parse_order_list_from_upstream(&json!({
            "data": {"orderList": [{
                "tid": "order-quantity",
                "itemId": "item-quantity",
                "itemTitle": "自动发货商品",
                "quantity": 3
            }]}
        }));
        assert_eq!(first[0]["purchaseQuantityState"], "valid");
        upsert_account_order(&pool, "uid-1", &first[0], 100.0)
            .await
            .expect("insert order quantity");

        let row = sqlx::query("SELECT * FROM account_orders WHERE uid = ? AND tid = ?")
            .bind("uid-1")
            .bind("order-quantity")
            .fetch_one(&pool)
            .await
            .expect("fetch inserted order");
        assert_eq!(row.get::<Option<i64>, _>("purchase_quantity"), Some(3));
        let value = order_row_to_value(row);
        assert_eq!(value["purchaseQuantity"], 3);
        assert_eq!(
            value["purchaseQuantitySource"],
            "trade.order.search:candidate:quantity"
        );

        let missing = parse_order_list_from_upstream(&json!({
            "data": {"orderList": [{
                "tid": "order-quantity",
                "itemId": "item-quantity",
                "itemTitle": "自动发货商品（刷新）",
                "itemCount": 99
            }]}
        }));
        assert_eq!(missing[0]["purchaseQuantityState"], "absent");
        upsert_account_order(&pool, "uid-1", &missing[0], 200.0)
            .await
            .expect("refresh order without quantity");

        let row = sqlx::query("SELECT * FROM account_orders WHERE uid = ? AND tid = ?")
            .bind("uid-1")
            .bind("order-quantity")
            .fetch_one(&pool)
            .await
            .expect("fetch refreshed order");
        assert_eq!(row.get::<Option<i64>, _>("purchase_quantity"), Some(3));
        assert_eq!(
            row.get::<Option<String>, _>("purchase_quantity_source"),
            Some("trade.order.search:candidate:quantity".to_string())
        );
    }

    #[tokio::test]
    async fn order_upsert_clears_known_purchase_quantity_on_invalid_generic_refresh() {
        let pool = setup_orders_pool().await;
        let first = parse_order_list_from_upstream(&json!({
            "data": {"orderList": [{
                "tid": "order-invalid-generic",
                "itemId": "item-quantity",
                "quantity": 3
            }]}
        }));
        upsert_account_order(&pool, "uid-1", &first[0], 100.0)
            .await
            .expect("insert known quantity");

        let invalid = parse_order_list_from_upstream(&json!({
            "data": {"orderList": [{
                "tid": "order-invalid-generic",
                "itemId": "item-quantity",
                "quantity": 0
            }]}
        }));
        assert_eq!(invalid[0]["purchaseQuantityState"], "invalid");
        upsert_account_order(&pool, "uid-1", &invalid[0], 200.0)
            .await
            .expect("persist invalid quantity state");

        let row = sqlx::query(
            "SELECT purchase_quantity, purchase_quantity_source FROM account_orders WHERE uid = ? AND tid = ?",
        )
        .bind("uid-1")
        .bind("order-invalid-generic")
        .fetch_one(&pool)
        .await
        .expect("fetch invalid generic refresh");
        assert_eq!(row.get::<Option<i64>, _>("purchase_quantity"), None);
        assert_eq!(
            row.get::<Option<String>, _>("purchase_quantity_source"),
            Some(String::new())
        );
    }

    #[tokio::test]
    async fn order_upsert_clears_known_purchase_quantity_on_card_conflict() {
        let pool = setup_orders_pool().await;
        let first = parse_order_list_from_upstream(&json!({
            "data": {"orderList": [{
                "tid": "order-card-conflict",
                "itemId": "item-quantity",
                "quantity": 3
            }]}
        }));
        upsert_account_order(&pool, "uid-1", &first[0], 100.0)
            .await
            .expect("insert known quantity");

        let conflict = parse_order_list_from_upstream(&json!({
            "data": {"items": [{
                "commonData": {
                    "orderIdStr": "order-card-conflict",
                    "quantity": 2
                },
                "content": {"data": {"detailInfo": {
                    "auctionId": "item-quantity",
                    "quantity": 3
                }}}
            }]}
        }));
        assert_eq!(conflict[0]["purchaseQuantityState"], "invalid");
        upsert_account_order(&pool, "uid-1", &conflict[0], 200.0)
            .await
            .expect("persist card conflict state");

        let row = sqlx::query(
            "SELECT purchase_quantity, purchase_quantity_source FROM account_orders WHERE uid = ? AND tid = ?",
        )
        .bind("uid-1")
        .bind("order-card-conflict")
        .fetch_one(&pool)
        .await
        .expect("fetch card conflict refresh");
        assert_eq!(row.get::<Option<i64>, _>("purchase_quantity"), None);
        assert_eq!(
            row.get::<Option<String>, _>("purchase_quantity_source"),
            Some(String::new())
        );
    }

    #[tokio::test]
    async fn authoritative_refresh_rejects_stale_cached_order_absent_from_current_upstream_result()
    {
        let pool = setup_orders_pool().await;
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'target-order', 'WAIT_BUYER_PAY', '旧缓存', 2, 'cached', 1, 10)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'other-order', 'SUCCESS', '本次同步命中', 1, 'fresh', 2, 20)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let outcome = persist_orders_from_current_batch(
            &pool,
            "uid-1",
            &[current_order(
                "other-order",
                "SUCCESS",
                "本次同步命中",
                Some(1),
                "fresh",
            )],
            20.0,
            Some(("target-order", 20_000)),
        )
        .await;
        assert_eq!(outcome.persisted, 1);
        assert_eq!(
            outcome.target_persistence,
            Some(AuthoritativeTargetPersistence::MissingFromCurrentBatch)
        );
        assert!(outcome.target_snapshot.is_none());
    }

    #[tokio::test]
    async fn concurrent_syncs_with_same_wall_clock_tick_cannot_share_authority_generation() {
        let pool = setup_orders_pool().await;
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'target-order', 'WAIT_BUYER_PAY', '旧tick目标', 2, 'cached', 1, 42)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'other-order', 'SUCCESS', '并发同tick新订单', 1, 'fresh', 2, 42)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let outcome = persist_orders_from_current_batch(
            &pool,
            "uid-1",
            &[current_order(
                "other-order",
                "SUCCESS",
                "并发同tick新订单",
                Some(1),
                "fresh",
            )],
            42.0,
            Some(("target-order", 42_000)),
        )
        .await;
        assert_eq!(
            outcome.target_persistence,
            Some(AuthoritativeTargetPersistence::MissingFromCurrentBatch)
        );
        assert!(outcome.target_snapshot.is_none());
    }

    #[tokio::test]
    async fn target_upsert_failure_is_reported_even_when_other_orders_persist() {
        let pool = setup_orders_pool().await;
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'target-order', 'WAIT_BUYER_PAY', '旧缓存仍在', 2, 'cached', 1, 99)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'other-order', 'SUCCESS', '其他订单已落库', 1, 'fresh', 2, 99)",
        )
        .execute(&pool)
        .await
        .unwrap();

        reject_target_upsert(&pool, "target-order").await;
        let outcome = persist_orders_from_current_batch(
            &pool,
            "uid-1",
            &[
                current_order(
                    "target-order",
                    "WAIT_SELLER_SEND_GOODS",
                    "当前目标",
                    Some(2),
                    "fresh",
                ),
                current_order("other-order", "SUCCESS", "其他订单已落库", Some(1), "fresh"),
            ],
            99.0,
            Some(("target-order", 99_000)),
        )
        .await;
        assert_eq!(outcome.persisted, 1);
        assert_eq!(outcome.failed, 1);
        assert_eq!(
            outcome.target_persistence,
            Some(AuthoritativeTargetPersistence::PersistFailed)
        );
        assert!(outcome.target_snapshot.is_none());
        let other: String = sqlx::query_scalar(
            "SELECT status_text FROM account_orders WHERE uid = 'uid-1' AND tid = 'other-order'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(other, "其他订单已落库");
    }

    #[tokio::test]
    async fn authoritative_refresh_does_not_return_old_row_when_current_target_upsert_fails() {
        let pool = setup_orders_pool().await;
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'target-order', 'WAIT_BUYER_PAY', '旧缓存', 2, 'cached', 1, 10)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO account_orders (uid, tid, status, status_text, purchase_quantity, purchase_quantity_source, created_at, updated_at) VALUES ('uid-1', 'another-order', 'SUCCESS', '本次同步了别的订单', 1, 'fresh', 2, 30)",
        )
        .execute(&pool)
        .await
        .unwrap();

        reject_target_upsert(&pool, "target-order").await;
        let outcome = persist_orders_from_current_batch(
            &pool,
            "uid-1",
            &[
                current_order(
                    "target-order",
                    "WAIT_SELLER_SEND_GOODS",
                    "当前目标失败",
                    Some(2),
                    "fresh",
                ),
                current_order(
                    "another-order",
                    "SUCCESS",
                    "本次同步了别的订单",
                    Some(1),
                    "fresh",
                ),
            ],
            30.0,
            Some(("target-order", 30_000)),
        )
        .await;
        assert_eq!(
            outcome.target_persistence,
            Some(AuthoritativeTargetPersistence::PersistFailed)
        );
        assert!(outcome.target_snapshot.is_none());
        let target: (String, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT status_text, purchase_quantity, purchase_quantity_source FROM account_orders WHERE uid = 'uid-1' AND tid = 'target-order'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(target.0, "旧缓存");
        assert_eq!(target.1, Some(2));
        assert_eq!(target.2, Some("cached".to_string()));
    }

    #[test]
    fn red_flower_requires_completed_non_refunded_order() {
        assert!(red_flower_eligible(&json!({
            "status": "TRADE_SUCCESS",
            "status_text": "交易成功"
        })));
        assert!(red_flower_eligible(&json!({
            "status": "DONE",
            "status_text": "已完成"
        })));
        assert!(!red_flower_eligible(&json!({
            "status": "TRADE_SUCCESS_REFUND",
            "status_text": "退款成功"
        })));
        assert!(!red_flower_eligible(&json!({
            "status": "TRADE_CLOSED",
            "status_text": "交易关闭"
        })));
        assert!(!red_flower_eligible(&json!({
            "status": "WAIT_SELLER_SEND_GOODS",
            "status_text": "等待卖家发货"
        })));
    }

    #[test]
    fn red_flower_error_message_extracts_useful_upstream_text() {
        let payload = json!({
            "ret": ["FAIL_SYS_USER_VALIDATE::平台暂未受理"],
            "data": {
                "detail": "请到闲鱼 App 完成验证后重试"
            }
        });
        assert_eq!(red_flower_error_message(&payload), "平台暂未受理");

        let payload = json!({
            "data": "{\"detail\":\"请稍后重试\",\"errorMsg\":\"请求受限\"}"
        });
        assert_eq!(red_flower_error_message(&payload), "请求受限");
    }

    #[test]
    fn auto_request_red_flower_only_when_order_newly_becomes_eligible() {
        let eligible = json!({
            "status": "TRADE_SUCCESS",
            "status_text": "交易成功"
        });
        let pending = json!({
            "status": "WAIT_SELLER_SEND_GOODS",
            "status_text": "等待卖家发货"
        });

        assert!(should_auto_request_red_flower(None, &eligible));
        assert!(should_auto_request_red_flower(Some(&pending), &eligible));
        assert!(!should_auto_request_red_flower(Some(&eligible), &eligible));
        assert!(!should_auto_request_red_flower(None, &pending));
    }

    #[test]
    fn red_flower_receive_issued_accepts_bool_and_string_true() {
        assert!(red_flower_receive_issued(&json!({
            "data": {"module": {"issued": true}}
        })));
        assert!(red_flower_receive_issued(&json!({
            "data": {"module": {"issued": "true"}}
        })));
        assert!(!red_flower_receive_issued(&json!({
            "data": {"module": {"issued": false}}
        })));
    }
}
