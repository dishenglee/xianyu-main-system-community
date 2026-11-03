use axum::{
    extract::{Extension, Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use futures_util::{stream, StreamExt};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
};
use url::Url;

use crate::{
    models::auth::RequestIdentity,
    routes::{
        accounts::shared::{
            payload_ret_success, successful_upstream_h5_request, successful_upstream_wx_request,
        },
        error::{ApiResult, ResponseError},
        tenant::{ensure_account_uid_accessible, owner_username_filter},
    },
    state::AppState,
};

const PUBLIC_PROFILE_FISH_AD_CODE: &str = "330110";
const PUBLIC_PROFILE_GPS: &str = "27.867353,113.164719";
const AWESOME_DETAIL_APP_KEY: &str = "12574478";
const ITEM_RATE_PAGE_SIZE: usize = 20;
const ITEM_RATE_CONCURRENCY: usize = 4;
const ITEM_RATE_ITEMS_PER_PAGE: usize = 20;
const ITEM_RATE_MAX_ITEM_PAGES: u32 = 12;
const ITEM_RATE_MAX_CURSOR_PAGES: usize = 3;
const PEER_MONITOR_ITEM_PAGE_SIZE: usize = 20;
const PEER_MONITOR_RATE_PAGE_SIZE: usize = 20;
const PEER_MONITOR_TIMELINE_PAGE_SIZE: usize = 20;
const PEER_MONITOR_MAX_PAGES: usize = 100;
const PEER_MONITOR_REFRESH_INTERVAL_SECS: f64 = 15.0 * 60.0;
const PEER_MONITOR_REFRESH_STALE_SECS: f64 = 60.0 * 60.0;
const PEER_MONITOR_WORKER_BATCH_SIZE: i64 = 2;
static PEER_LINK_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)(?:https?://|fleamarket://)[^\s]+")
        .expect("peer monitor link pattern is valid")
});

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/summary"),
            get(summary),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/targets"),
            get(list_targets).post(create_target),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/targets/:target_id"),
            get(get_target).patch(update_target).delete(delete_target),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/targets/:target_id/refresh"),
            post(record_refresh),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/candidates/resolve"),
            post(resolve_candidate),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/candidates/resolve-link"),
            post(resolve_link_candidate),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/peer-monitor/candidates/:seller_id/reviews"),
            get(list_public_reviews),
        )
}

#[derive(Debug, Deserialize)]
struct CreateTargetPayload {
    seller_id: String,
    #[serde(default)]
    seller_name: String,
    #[serde(default)]
    seller_avatar: String,
    #[serde(default)]
    source_item: Value,
}

#[derive(Debug, Deserialize)]
struct UpdateTargetPayload {
    enabled: Option<bool>,
    #[serde(default)]
    status: String,
}

#[derive(Debug, Deserialize)]
struct ResolveCandidatePayload {
    source_uid: String,
    item_id: String,
    #[serde(default)]
    source_item: Value,
}

#[derive(Debug, Deserialize)]
struct ResolveLinkPayload {
    link: String,
}

#[derive(Debug, Deserialize)]
struct PublicReviewQuery {
    #[serde(default)]
    page: Option<u32>,
    #[serde(default)]
    source_uid: Option<String>,
    #[serde(default)]
    rate_type: Option<i64>,
    #[serde(default)]
    extra_tag: Option<String>,
    #[serde(default)]
    fold_flag: Option<i64>,
}

#[derive(Debug)]
enum PeerLinkTarget {
    Item(String),
    Seller(String),
}

#[derive(Debug, Clone)]
struct PublicPeerSnapshot {
    profile: Value,
    items: Vec<Value>,
    timeline: Option<Vec<Value>>,
    reviews: Option<Vec<Value>>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PeerMonitorRefreshBatch {
    pub attempted: usize,
    pub succeeded: usize,
    pub partial: usize,
    pub failed: usize,
}

#[derive(Debug, Clone)]
struct PeerMonitorRefreshOutcome {
    partial: bool,
    item_count: usize,
    timeline_count: usize,
    rate_count: usize,
    detail: String,
}

fn owner(state: &AppState, identity: &RequestIdentity) -> String {
    owner_username_filter(state, identity).unwrap_or_else(|| "__legacy__".to_string())
}

fn unix_timestamp_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs_f64()
}

fn text(row: &sqlx::sqlite::SqliteRow, name: &str) -> String {
    row.try_get::<String, _>(name).unwrap_or_default()
}

fn number(row: &sqlx::sqlite::SqliteRow, name: &str) -> f64 {
    row.try_get::<f64, _>(name).unwrap_or_default()
}

fn json_text(row: &sqlx::sqlite::SqliteRow, name: &str) -> Value {
    serde_json::from_str(&text(row, name)).unwrap_or(Value::Null)
}

fn target_value(row: &sqlx::sqlite::SqliteRow) -> Value {
    let enabled = row.try_get::<i64, _>("enabled").unwrap_or(0) != 0;
    let last_error = text(row, "last_error");
    let profile = json_text(row, "profile_json");
    let seller_name = first_text([
        value_text(profile.get("sellerNick")),
        text(row, "seller_name"),
    ]);
    let seller_avatar = first_text([
        value_text(profile.get("sellerAvatar")),
        text(row, "seller_avatar"),
    ]);
    let last_success_at = number(row, "last_success_at");
    let snapshot_ready = last_success_at > 0.0;
    json!({
        "id": row.try_get::<i64, _>("id").unwrap_or_default(),
        "sellerId": text(row, "seller_id"),
        "sellerName": seller_name, "sellerNick": seller_name,
        "sellerAvatar": seller_avatar,
        "area": value_text(profile.get("area")),
        "sellerSummary": value_text(profile.get("sellerSummary")),
        "sellerLevel": value_text(profile.get("sellerLevel")),
        "shopLevel": value_text(profile.get("shopLevel")),
        "followersCount": value_text(profile.get("followersCount")),
        "followingCount": value_text(profile.get("followingCount")),
        "rateCount": value_text(profile.get("rateCount")),
        "dynamicCount": value_text(profile.get("dynamicCount")),
        "profile": profile,
        "sourceItemId": text(row, "source_item_id"),
        "sourceItem": {
            "itemId": text(row, "source_item_id"),
            "title": text(row, "source_item_title"),
            "price": text(row, "source_item_price"),
            "imageUrl": text(row, "source_item_image_url"),
        },
        "enabled": enabled,
        "status": if !last_error.is_empty() { "error" } else if enabled { "active" } else { "paused" },
        "snapshotState": if snapshot_ready { "ready" } else { "pending" },
        "baselineCreatedAt": number(row, "baseline_created_at"),
        "lastRefreshedAt": number(row, "last_refreshed_at"), "lastRefreshAt": number(row, "last_refreshed_at"),
        "lastSuccessAt": last_success_at,
        "profileObservedAt": number(row, "profile_observed_at"),
        "lastError": last_error,
        "activeItemCount": row.try_get::<i64, _>("item_count").unwrap_or(0),
        "changeCountToday": row.try_get::<i64, _>("change_count").unwrap_or(0),
        "createdAt": number(row, "created_at"),
    })
}

const TARGET_SELECT: &str = "SELECT t.*, (SELECT COUNT(*) FROM peer_monitor_item_snapshots s WHERE s.target_id = t.id AND s.is_active = 1) AS item_count, (SELECT COUNT(*) FROM peer_monitor_events e WHERE e.target_id = t.id AND e.event_type NOT IN ('baseline', 'refresh') AND e.created_at >= unixepoch('now', 'start of day')) AS change_count FROM peer_monitor_targets t";

async fn load_target(
    state: &AppState,
    owner: &str,
    target_id: i64,
) -> Result<sqlx::sqlite::SqliteRow, ResponseError> {
    load_target_from_pool(state.storage().xianyu(), owner, target_id).await
}

async fn load_target_from_pool(
    pool: &SqlitePool,
    owner: &str,
    target_id: i64,
) -> Result<sqlx::sqlite::SqliteRow, ResponseError> {
    sqlx::query(&format!(
        "{TARGET_SELECT} WHERE t.id = ? AND t.owner_username = ?"
    ))
    .bind(target_id)
    .bind(owner)
    .fetch_optional(pool)
    .await
    .map_err(ResponseError::internal)?
    .ok_or_else(|| ResponseError::not_found("同行监控目标不存在"))
}

async fn ensure_baseline_event(pool: &SqlitePool, target_id: i64) -> Result<bool, ResponseError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM peer_monitor_events WHERE target_id = ? AND event_type = 'baseline'",
    )
    .bind(target_id)
    .fetch_one(pool)
    .await
    .map_err(ResponseError::internal)?;
    if exists != 0 {
        return Ok(false);
    }

    sqlx::query("INSERT INTO peer_monitor_events (target_id, event_type, title, summary, entity_type) VALUES (?, 'baseline', '已建立账号监控基线', '首次完整成功采集同行主页、公开宝贝、动态和评价；后续成功采集才会生成变化事件。', 'account')")
        .bind(target_id)
        .execute(pool)
        .await
        .map_err(ResponseError::internal)?;
    Ok(true)
}

async fn list_targets(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = owner(&state, &identity);
    let rows = sqlx::query(&format!(
        "{TARGET_SELECT} WHERE t.owner_username = ? ORDER BY t.updated_at DESC"
    ))
    .bind(&owner)
    .fetch_all(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    let active_count = rows
        .iter()
        .filter(|row| row.try_get::<i64, _>("enabled").unwrap_or(0) != 0)
        .count();
    let event_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM peer_monitor_events e JOIN peer_monitor_targets t ON t.id = e.target_id WHERE t.owner_username = ? AND e.event_type != 'baseline'",
    ).bind(&owner).fetch_one(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let failed_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM peer_monitor_targets WHERE owner_username = ? AND last_error != ''",
    )
    .bind(&owner)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    Ok(Json(json!({
        "targets": rows.iter().map(target_value).collect::<Vec<_>>(),
        "summary": { "total": rows.len(), "active": active_count, "changes": event_count, "abnormal": failed_count }
    })))
}

async fn summary(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let owner = owner(&state, &identity);
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM peer_monitor_targets WHERE owner_username = ?")
            .bind(&owner)
            .fetch_one(state.storage().xianyu())
            .await
            .map_err(ResponseError::internal)?;
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM peer_monitor_targets WHERE owner_username = ? AND enabled = 1",
    )
    .bind(&owner)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    let changes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM peer_monitor_events e JOIN peer_monitor_targets t ON t.id = e.target_id WHERE t.owner_username = ? AND e.event_type NOT IN ('baseline', 'refresh') AND e.created_at >= unixepoch('now', 'start of day')").bind(&owner).fetch_one(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let abnormal: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM peer_monitor_targets WHERE owner_username = ? AND last_error != ''",
    )
    .bind(&owner)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    Ok(Json(
        json!({ "targetCount": total, "activeCount": active, "changesToday": changes, "errorCount": abnormal }),
    ))
}

async fn create_target(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CreateTargetPayload>,
) -> ApiResult {
    let seller_id = payload.seller_id.trim();
    if seller_id.is_empty() {
        return Err(ResponseError::bad_request("缺少可用于监控的同行标识"));
    }
    let owner = owner(&state, &identity);
    let source_item_id = payload
        .source_item
        .get("itemId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    sqlx::query(
        "INSERT INTO peer_monitor_targets (owner_username, seller_id, seller_name, seller_avatar, source_item_id, source_item_title, source_item_price, source_item_image_url) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(owner_username, seller_id) DO UPDATE SET seller_name = CASE WHEN excluded.seller_name != '' THEN excluded.seller_name ELSE peer_monitor_targets.seller_name END, seller_avatar = CASE WHEN excluded.seller_avatar != '' THEN excluded.seller_avatar ELSE peer_monitor_targets.seller_avatar END, source_item_id = CASE WHEN excluded.source_item_id != '' THEN excluded.source_item_id ELSE peer_monitor_targets.source_item_id END, source_item_title = CASE WHEN excluded.source_item_title != '' THEN excluded.source_item_title ELSE peer_monitor_targets.source_item_title END, source_item_price = CASE WHEN excluded.source_item_price != '' THEN excluded.source_item_price ELSE peer_monitor_targets.source_item_price END, source_item_image_url = CASE WHEN excluded.source_item_image_url != '' THEN excluded.source_item_image_url ELSE peer_monitor_targets.source_item_image_url END, updated_at = unixepoch('subsec')",
    ).bind(&owner).bind(seller_id).bind(payload.seller_name.trim()).bind(payload.seller_avatar.trim()).bind(&source_item_id)
        .bind(payload.source_item.get("title").and_then(Value::as_str).unwrap_or_default().trim())
        .bind(payload.source_item.get("price").and_then(Value::as_str).unwrap_or_default().trim())
        .bind(payload.source_item.get("picUrl").and_then(Value::as_str).unwrap_or_default().trim())
        .execute(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let target = sqlx::query(&format!(
        "{TARGET_SELECT} WHERE t.owner_username = ? AND t.seller_id = ?"
    ))
    .bind(&owner)
    .bind(seller_id)
    .fetch_one(state.storage().xianyu())
    .await
    .map_err(ResponseError::internal)?;
    let created = number(&target, "last_success_at") <= 0.0;
    Ok(Json(
        json!({ "target": target_value(&target), "created": created }),
    ))
}

async fn get_target(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(target_id): Path<i64>,
) -> ApiResult {
    let owner = owner(&state, &identity);
    let target = load_target(&state, &owner, target_id).await?;
    let items = sqlx::query("SELECT item_id, title, price, image_url, status, is_active, first_observed_at, last_observed_at, observed_at FROM peer_monitor_item_snapshots WHERE target_id = ? AND is_active = 1 ORDER BY last_observed_at DESC, observed_at DESC")
        .bind(target_id).fetch_all(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let events = sqlx::query("SELECT id, event_type, title, summary, entity_type, entity_id, previous_json, current_json, created_at FROM peer_monitor_events WHERE target_id = ? ORDER BY created_at DESC LIMIT 100")
        .bind(target_id).fetch_all(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let timeline = sqlx::query("SELECT event_key, item_id, trade_id, event_type, event_date, title, price, image_url, first_observed_at, last_observed_at FROM peer_monitor_timeline_snapshots WHERE target_id = ? ORDER BY last_observed_at DESC LIMIT 100")
        .bind(target_id).fetch_all(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let reviews = sqlx::query("SELECT rate_id, trade_id, item_id, item_title, item_price, item_image_url, rater_nick, feedback, created_at_text, rate_json, first_observed_at, last_observed_at FROM peer_monitor_rate_snapshots WHERE target_id = ? ORDER BY last_observed_at DESC LIMIT 100")
        .bind(target_id).fetch_all(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let runs = sqlx::query("SELECT id, status, detail, trigger, started_at, finished_at, profile_count, item_count, timeline_count, rate_count, created_at FROM peer_monitor_runs WHERE target_id = ? ORDER BY created_at DESC LIMIT 30")
        .bind(target_id).fetch_all(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    let mut response = target_value(&target);
    let response_object = response.as_object_mut().expect("target JSON object");
    response_object.insert("target".to_string(), target_value(&target));
    response_object.insert("items".to_string(), Value::Array(items.iter().map(|row| json!({"itemId": text(row, "item_id"), "title": text(row, "title"), "price": text(row, "price"), "imageUrl": text(row, "image_url"), "status": text(row, "status"), "firstObservedAt": number(row, "first_observed_at"), "observedAt": number(row, "last_observed_at")})).collect::<Vec<_>>()));
    response_object.insert("events".to_string(), Value::Array(events.iter().map(|row| json!({"id": row.try_get::<i64, _>("id").unwrap_or_default(), "type": text(row, "event_type"), "title": text(row, "title"), "summary": text(row, "summary"), "entityType": text(row, "entity_type"), "entityId": text(row, "entity_id"), "previous": json_text(row, "previous_json"), "current": json_text(row, "current_json"), "createdAt": number(row, "created_at")})).collect::<Vec<_>>()));
    response_object.insert("timeline".to_string(), Value::Array(timeline.iter().map(|row| json!({"eventKey": text(row, "event_key"), "itemId": text(row, "item_id"), "tradeId": text(row, "trade_id"), "status": text(row, "event_type"), "statusLabel": timeline_status_label(&text(row, "event_type")), "date": text(row, "event_date"), "title": text(row, "title"), "price": text(row, "price"), "picUrl": text(row, "image_url"), "firstObservedAt": number(row, "first_observed_at"), "observedAt": number(row, "last_observed_at")})).collect::<Vec<_>>()));
    response_object.insert(
        "reviews".to_string(),
        Value::Array(
            reviews
                .iter()
                .map(|row| {
                    let mut rate = json_text(row, "rate_json");
                    if !rate.is_object() {
                        rate = json!({});
                    }
                    rate["id"] = json!(text(row, "rate_id"));
                    rate["tradeId"] = json!(text(row, "trade_id"));
                    rate["itemId"] = json!(text(row, "item_id"));
                    rate["itemTitle"] = json!(text(row, "item_title"));
                    rate["itemPrice"] = json!(text(row, "item_price"));
                    rate["itemPic"] = json!(text(row, "item_image_url"));
                    rate["raterNick"] = json!(text(row, "rater_nick"));
                    rate["feedback"] = json!(text(row, "feedback"));
                    rate["createdAt"] = json!(text(row, "created_at_text"));
                    rate["observedAt"] = json!(number(row, "last_observed_at"));
                    rate
                })
                .collect::<Vec<_>>(),
        ),
    );
    response_object.insert("runs".to_string(), Value::Array(runs.iter().map(|row| json!({"id": row.try_get::<i64, _>("id").unwrap_or_default(), "status": text(row, "status"), "detail": text(row, "detail"), "trigger": text(row, "trigger"), "startedAt": number(row, "started_at"), "finishedAt": number(row, "finished_at"), "profileCount": row.try_get::<i64, _>("profile_count").unwrap_or_default(), "itemCount": row.try_get::<i64, _>("item_count").unwrap_or_default(), "timelineCount": row.try_get::<i64, _>("timeline_count").unwrap_or_default(), "rateCount": row.try_get::<i64, _>("rate_count").unwrap_or_default(), "createdAt": number(row, "created_at")})).collect::<Vec<_>>()));
    Ok(Json(response))
}

async fn update_target(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(target_id): Path<i64>,
    Json(payload): Json<UpdateTargetPayload>,
) -> ApiResult {
    let owner = owner(&state, &identity);
    let _ = load_target(&state, &owner, target_id).await?;
    let enabled = payload.enabled.or_else(|| match payload.status.as_str() {
        "active" => Some(true),
        "paused" => Some(false),
        _ => None,
    });
    if let Some(enabled) = enabled {
        sqlx::query("UPDATE peer_monitor_targets SET enabled = ?, updated_at = unixepoch('subsec') WHERE id = ? AND owner_username = ?")
            .bind(if enabled { 1_i64 } else { 0_i64 }).bind(target_id).bind(&owner).execute(state.storage().xianyu()).await.map_err(ResponseError::internal)?;
    }
    let updated = load_target(&state, &owner, target_id).await?;
    Ok(Json(json!({ "target": target_value(&updated) })))
}

fn find_text(value: &Value, keys: &[&str]) -> String {
    match value {
        Value::Object(object) => {
            for key in keys {
                if let Some(value) = object.get(*key) {
                    let text = find_text(value, &[]);
                    if is_meaningful_detail_text(&text) {
                        return text;
                    }
                }
            }
            for child in object.values() {
                let found = find_text(child, keys);
                if is_meaningful_detail_text(&found) {
                    return found;
                }
            }
            String::new()
        }
        Value::Array(values) => values
            .iter()
            .map(|value| find_text(value, keys))
            .find(|text| is_meaningful_detail_text(text))
            .unwrap_or_default(),
        Value::String(text) => {
            let text = text.trim();
            if text.starts_with('{') || text.starts_with('[') {
                if let Ok(decoded) = serde_json::from_str::<Value>(text) {
                    return find_text(&decoded, keys);
                }
            }
            text.to_string()
        }
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

// Detail payloads include many optional fields initialized to 0. Those are not
// usable seller or item values, and must not mask the populated nested field.
fn is_meaningful_detail_text(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "null" | "undefined" | "none"
    )
}

fn detail_points_to_requester(seller_id: &str, source_uid: &str) -> bool {
    !seller_id.trim().is_empty() && seller_id.trim() == source_uid.trim()
}

fn search_candidate(item_id: &str, source_item: &Value) -> Option<Value> {
    let seller_id = find_text(
        source_item,
        &["sellerId", "seller_id", "sellerUserId", "userId", "user_id"],
    );
    if !is_meaningful_detail_text(&seller_id) {
        return None;
    }

    Some(json!({
        "itemId": item_id,
        "sellerId": seller_id,
        "sellerNick": find_text(source_item, &["sellerNick", "userNick", "userNickName", "nickname"]),
        "sellerAvatar": find_text(source_item, &["sellerAvatar", "userAvatar", "avatar", "userAvatarUrl"]),
        "title": find_text(source_item, &["title", "itemTitle"]),
        "price": find_text(source_item, &["soldPrice", "price", "displayPrice"]),
        "picUrl": find_text(source_item, &["picUrl", "imageUrl", "mainPic"]),
        "area": find_text(source_item, &["area", "location", "city"]),
    }))
}

fn value_text(value: Option<&Value>) -> String {
    value.map(|value| find_text(value, &[])).unwrap_or_default()
}

fn first_text(values: impl IntoIterator<Item = String>) -> String {
    values
        .into_iter()
        .find(|value| is_meaningful_detail_text(value))
        .unwrap_or_default()
}

fn public_item_cards(data: &Value) -> Vec<Value> {
    data.get("cardList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|card| card.get("cardData"))
        .filter_map(|card| {
            let detail = card.get("detailParams").unwrap_or(card);
            let item_id = first_text([
                value_text(card.get("id")),
                value_text(detail.get("itemId")),
            ]);
            (!item_id.is_empty()).then(|| {
                json!({
                    "itemId": item_id,
                    "title": first_text([value_text(card.get("title")), value_text(detail.get("title"))]),
                    "price": first_text([value_text(card.get("priceInfo").and_then(|value| value.get("price"))), value_text(detail.get("soldPrice"))]),
                    "picUrl": value_text(card.get("picInfo").and_then(|value| value.get("picUrl"))),
                    "status": value_text(card.get("itemStatus")),
                })
            })
        })
        .collect()
}

fn public_timeline_cards(data: &Value) -> Vec<Value> {
    data.get("cardList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|card| value_text(card.get("cardType")) == "dayNode")
        .flat_map(|day_node| {
            let day = value_text(day_node.get("ds"));
            let month = value_text(day_node.get("secondaryDesc"));
            day_node
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|group| group.get("items").and_then(Value::as_array))
                .flatten()
                .filter_map(move |item| {
                    let event = timeline_event(item)?;
                    let detail = item.get("detailParams").unwrap_or(item);
                    let item_id = first_text([
                        value_text(item.get("itemId")),
                        value_text(detail.get("itemId")),
                    ]);
                    let title = first_text([
                        value_text(item.get("title")),
                        value_text(detail.get("title")),
                        value_text(item.get("itemTitle")),
                    ]);
                    let pic_url = first_text([
                        value_text(item.get("mainPic")),
                        value_text(item.get("picUrl")),
                        value_text(detail.get("picUrl")),
                    ]);
                    // A timeline can contain ordinary text-only cards as well
                    // as item events. Only expose entries that actually point
                    // at an item, so a generic status description never turns
                    // into a fictitious product activity.
                    if item_id.is_empty() && title.is_empty() && pic_url.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "tradeId": first_text([value_text(item.get("tradeId")), item_id.clone()]),
                        "itemId": item_id,
                        "title": title,
                        "price": first_text([value_text(item.get("price")), value_text(detail.get("price")), value_text(item.get("soldPrice"))]),
                        "date": day.clone(),
                        "month": month.clone(),
                        "picUrl": pic_url,
                        "status": event.status,
                        "statusLabel": event.label,
                        "itemStatus": value_text(item.get("itemStatus")),
                        "cardType": value_text(item.get("cardType")),
                    }))
                })
        })
        .collect()
}

struct TimelineEvent {
    status: &'static str,
    label: &'static str,
}

fn timeline_event(item: &Value) -> Option<TimelineEvent> {
    let source = [
        value_text(item.get("cardType")),
        value_text(item.get("eventType")),
        value_text(item.get("actionType")),
        value_text(item.get("status")),
        value_text(item.get("statusDesc")),
        value_text(item.get("desc")),
    ]
    .join(" ")
    .to_ascii_lowercase();

    if source.contains("sold") || source.contains("售出") || source.contains("成交") {
        return Some(TimelineEvent {
            status: "sold",
            label: "售出",
        });
    }
    if source.contains("off_shelf")
        || source.contains("offshelf")
        || source.contains("down_shelf")
        || source.contains("downshelf")
        || source.contains("下架")
    {
        return Some(TimelineEvent {
            status: "off_shelf",
            label: "下架",
        });
    }
    if source.contains("on_shelf")
        || source.contains("onshelf")
        || source.contains("up_shelf")
        || source.contains("upshelf")
        || source.contains("publish")
        || source.contains("listed")
        || source.contains("上架")
        || source.contains("发布")
    {
        return Some(TimelineEvent {
            status: "listed",
            label: "上架",
        });
    }
    None
}

fn timeline_visibility(data: &Value) -> &'static str {
    match value_text(data.get("privacyDays")).as_str() {
        "-1" => "hidden",
        value if !value.is_empty() && value != "0" => "recent_only",
        _ => "visible",
    }
}

fn timeline_event_key(event: &Value) -> String {
    [
        value_text(event.get("status")),
        value_text(event.get("tradeId")),
        value_text(event.get("itemId")),
        value_text(event.get("date")),
        value_text(event.get("title")),
    ]
    .join("|")
}

fn timeline_status_label(status: &str) -> &'static str {
    match status {
        "listed" => "上架",
        "sold" => "售出",
        "off_shelf" => "下架",
        _ => "动态",
    }
}

fn is_stable_public_id(value: &str) -> bool {
    let value = value.trim();
    (6..=20).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn clean_link_candidate(value: &str) -> &str {
    value.trim_matches(|character: char| {
        matches!(
            character,
            '"' | '\''
                | ','
                | '，'
                | '。'
                | ';'
                | '；'
                | '!'
                | '！'
                | '、'
                | '）'
                | ')'
                | '】'
                | ']'
                | '}'
        )
    })
}

fn link_candidate_from_text(value: &str) -> Option<&str> {
    let text = value.trim();
    if text.starts_with("https://")
        || text.starts_with("http://")
        || text.starts_with("fleamarket://")
    {
        return Some(clean_link_candidate(text));
    }
    PEER_LINK_REGEX
        .find(text)
        .map(|matched| clean_link_candidate(matched.as_str()))
}

fn known_public_link_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    host == "m.tb.cn"
        || host == "taobao.com"
        || host.ends_with(".taobao.com")
        || host == "goofish.com"
        || host.ends_with(".goofish.com")
}

fn numeric_query_value(url: &Url, names: &[&str]) -> Option<String> {
    url.query_pairs().find_map(|(name, value)| {
        names
            .iter()
            .any(|expected| name.eq_ignore_ascii_case(expected))
            .then(|| value.into_owned())
            .filter(|value| is_stable_public_id(value))
    })
}

fn parse_peer_link_target(value: &str) -> Option<PeerLinkTarget> {
    let url = Url::parse(value).ok()?;
    let scheme = url.scheme();
    if scheme != "fleamarket" {
        let host = url.host_str()?;
        if !known_public_link_host(host) {
            return None;
        }
    }

    let path = url.path().to_ascii_lowercase();
    let is_personal_share = path.contains("personal")
        || path.contains("userhome")
        || (path.contains("sharexy")
            && url.query_pairs().any(|(name, value)| {
                name.eq_ignore_ascii_case("bft") && value.eq_ignore_ascii_case("personal")
            }));
    if is_personal_share {
        if let Some(user_id) =
            numeric_query_value(&url, &["userId", "user_id", "userid", "sellerId"])
        {
            return Some(PeerLinkTarget::Seller(user_id));
        }
    }
    numeric_query_value(&url, &["itemId", "item_id", "id", "targetId"]).map(PeerLinkTarget::Item)
}

async fn resolve_peer_link_target(raw_link: &str) -> Result<PeerLinkTarget, String> {
    let link = link_candidate_from_text(raw_link)
        .ok_or_else(|| "请粘贴完整的闲鱼商品链接或同行主页链接".to_string())?;
    if let Some(target) = parse_peer_link_target(link) {
        return Ok(target);
    }

    let mut current = Url::parse(link).map_err(|_| "链接格式无法识别".to_string())?;
    if !matches!(current.scheme(), "http" | "https")
        || !current.host_str().is_some_and(known_public_link_host)
    {
        return Err("仅支持闲鱼或淘宝分享链接".to_string());
    }

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| error.to_string())?;
    for _ in 0..5 {
        let response = client
            .get(current.clone())
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
            )
            .send()
            .await
            .map_err(|error| format!("分享链接无法打开：{error}"))?;
        if let Some(target) = parse_peer_link_target(response.url().as_str()) {
            return Ok(target);
        }
        if !response.status().is_redirection() {
            let body = response
                .text()
                .await
                .map_err(|error| format!("分享链接内容无法读取：{error}"))?;
            if let Some(target) = PEER_LINK_REGEX
                .find_iter(&body)
                .map(|matched| clean_link_candidate(matched.as_str()))
                .find_map(parse_peer_link_target)
            {
                return Ok(target);
            }
            break;
        }
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| "分享链接未返回目标地址".to_string())?;
        current = current
            .join(location)
            .map_err(|_| "分享链接跳转地址无效".to_string())?;
        if !matches!(current.scheme(), "http" | "https")
            || !current.host_str().is_some_and(known_public_link_host)
        {
            return Err("分享链接跳转到了不支持的地址".to_string());
        }
    }
    Err("未能从链接中识别商品或同行主页".to_string())
}

async fn public_h5_request(
    state: &AppState,
    api: &str,
    version: &str,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    let payload = crate::xianyu_client::anonymous_public_h5_request(
        state.config(),
        api,
        version,
        &data,
        extra_params,
    )
    .await?;
    if payload_ret_success(&payload) {
        Ok(payload)
    } else {
        Err("public profile request was not successful".to_string())
    }
}

async fn public_awesome_detail(
    state: &AppState,
    _source_uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    let payload = crate::xianyu_client::anonymous_public_h5_request_with_app_key_and_profile(
        state.config(),
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.awesome.detail"),
        "1.0",
        &json!({ "itemId": item_id }),
        vec![
            ("accountSite".to_string(), "xianyu".to_string()),
            ("prefetch_key".to_string(), format!("detail_{item_id}")),
        ],
        AWESOME_DETAIL_APP_KEY,
        crate::xianyu_client::H5RequestProfile::AwesomeDetailMobile,
    )
    .await?;
    if payload_ret_success(&payload) {
        Ok(payload)
    } else {
        Err("public item detail request was not successful".to_string())
    }
}

fn public_seller_id(detail_data: &Value) -> String {
    first_text([
        value_text(
            detail_data
                .get("sellerDO")
                .and_then(|value| value.get("sellerId")),
        ),
        value_text(
            detail_data
                .get("sellerDO")
                .and_then(|value| value.get("userId")),
        ),
        value_text(
            detail_data
                .get("seller")
                .and_then(|value| value.get("userId")),
        ),
        value_text(
            detail_data
                .get("baseInfo")
                .and_then(|value| value.get("kcUserId")),
        ),
    ])
}

fn seller_level(profile_data: &Value) -> String {
    let tags = profile_data
        .get("module")
        .and_then(|module| {
            module
                .get("base")
                .and_then(|value| value.get("ylzTags"))
                .or_else(|| {
                    module
                        .get("platform")
                        .and_then(|value| value.get("ylzTags"))
                })
        })
        .and_then(Value::as_array);
    let Some(tags) = tags else {
        return String::new();
    };
    tags.iter()
        .find(|tag| {
            tag.get("attributes")
                .and_then(|value| value.get("role"))
                .and_then(Value::as_str)
                == Some("seller")
        })
        .or_else(|| tags.first())
        .map(|tag| value_text(tag.get("text")))
        .unwrap_or_default()
}

fn merge_public_profile_head(candidate: &mut Value, profile_data: &Value, source_item: &Value) {
    let module = profile_data.get("module").unwrap_or(&Value::Null);
    let base = module.get("base").unwrap_or(&Value::Null);
    let nav = module.get("nav").unwrap_or(&Value::Null);
    let social = module.get("social").unwrap_or(&Value::Null);
    let tabs = module.get("tabs").unwrap_or(&Value::Null);
    let quality = module.get("quality").unwrap_or(&Value::Null);
    let identity = module.get("identity").unwrap_or(&Value::Null);
    let introduction = module.get("introduction").unwrap_or(&Value::Null);
    let background = module.get("background").unwrap_or(&Value::Null);
    // The App and public H5 variants place shop under different parents.
    let shop = module
        .get("shop")
        .or_else(|| base.get("shop"))
        .unwrap_or(&Value::Null);

    candidate["sellerNick"] = json!(first_text([
        value_text(base.get("displayName")),
        value_text(nav.get("displayNick")),
        value_text(source_item.get("sellerNick")),
    ]));
    candidate["sellerAvatar"] = json!(first_text([
        value_text(base.get("avatar").and_then(|value| value.get("avatar"))),
        value_text(base.get("avatar")),
        value_text(source_item.get("sellerAvatar")),
    ]));
    candidate["area"] = json!(first_text([
        value_text(base.get("ipLocation")),
        value_text(quality.get("ipRegionAddress")),
        value_text(quality.get("residence")),
        value_text(nav.get("summary")),
        value_text(source_item.get("area")),
    ]));
    candidate["ipRegion"] = json!(value_text(quality.get("ipRegionAddress")));
    candidate["residence"] = json!(value_text(quality.get("residence")));
    candidate["followersCount"] = json!(value_text(social.get("followers")));
    candidate["followingCount"] = json!(value_text(social.get("following")));
    candidate["itemCount"] = json!(value_text(
        tabs.get("item").and_then(|value| value.get("number"))
    ));
    candidate["rateCount"] = json!(value_text(
        tabs.get("rate").and_then(|value| value.get("number"))
    ));
    candidate["dynamicCount"] = json!(value_text(
        tabs.get("action").and_then(|value| value.get("number"))
    ));
    candidate["sellerLevel"] = json!(seller_level(profile_data));
    candidate["shopLevel"] = json!(value_text(shop.get("level")));
    candidate["shopScore"] = json!(value_text(shop.get("score")));
    candidate["praiseRatio"] = json!(value_text(shop.get("praiseRatio")));
    candidate["reviewNum"] = json!(value_text(shop.get("reviewNum")));
    candidate["businessQuality"] = json!(value_text(
        shop.get("businessQuality")
            .and_then(|value| value.get("name"))
    ));
    candidate["sellerSummary"] = json!(first_text([
        value_text(base.get("introduction")),
        value_text(introduction.get("introduction")),
        value_text(identity.get("shopDesc")),
    ]));
    candidate["shopDescription"] = json!(value_text(identity.get("shopDesc")));
    candidate["visitTime"] = json!(value_text(base.get("visitTime")));
    candidate["postCount"] = json!(value_text(nav.get("postCount")));
    // Public profile headers use several real background shapes. Prefer the
    // explicitly self-selected image, then retain the platform-provided
    // background variants rather than substituting the seller avatar.
    candidate["profileBackgroundUrl"] = json!(first_text([
        if value_text(background.get("bgSetting")) == "self" {
            value_text(
                background
                    .get("image")
                    .and_then(|value| value.get("data"))
                    .and_then(|value| value.get("url")),
            )
        } else {
            String::new()
        },
        value_text(background.get("bgUrl")),
        value_text(background.get("asset").and_then(|value| value.get("bgUrl")),),
        value_text(
            background
                .get("default")
                .and_then(|value| value.get("bgUrl")),
        ),
    ]));

    let mut profile_tags = tag_texts(
        module
            .get("impression")
            .and_then(|value| value.get("items")),
    );
    let shop_description = value_text(identity.get("shopDesc"));
    if !shop_description.is_empty() && !profile_tags.contains(&shop_description) {
        profile_tags.insert(0, shop_description);
    }
    if !profile_tags.is_empty() {
        candidate["profileTags"] = json!(profile_tags);
    }
}

async fn load_public_profile_candidate(
    state: &AppState,
    source_uids: &[String],
    item_id: &str,
    source_item: &Value,
) -> Result<Value, String> {
    let detail = public_awesome_detail(state, "", item_id).await?;
    let detail_data = detail.get("data").unwrap_or(&Value::Null);
    let seller_id = public_seller_id(detail_data);
    if seller_id.is_empty() || !seller_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("public item detail did not return a numeric seller id".to_string());
    }

    let mut candidate =
        load_public_profile_by_seller_id(state, &seller_id, Some(item_id), source_item).await?;
    for source_uid in source_uids {
        let Ok(payload) = public_weixin_detail(state, source_uid, item_id).await else {
            continue;
        };
        let data = payload.get("data").unwrap_or(&Value::Null);
        if !wx_detail_matches_public_item(data, item_id, &seller_id) {
            continue;
        }
        merge_wx_seller_enhancement(&mut candidate, data);
        break;
    }
    Ok(candidate)
}

async fn public_weixin_detail(
    state: &AppState,
    source_uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    successful_upstream_wx_request(
        state,
        source_uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.detail"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "itemId": item_id,
        }),
    )
    .await
}

fn wx_detail_matches_public_item(data: &Value, item_id: &str, seller_id: &str) -> bool {
    let returned_item_id = first_text([
        value_text(data.get("itemId")),
        value_text(
            data.get(crate::sealed_protocol::sealed_str!("itemDO"))
                .and_then(|item| item.get("itemId")),
        ),
        value_text(
            data.get("trackParams")
                .and_then(|track| track.get("itemId")),
        ),
    ]);
    if !returned_item_id.is_empty() && returned_item_id != item_id {
        return false;
    }

    let returned_seller_id = first_text([
        value_text(
            data.get("sellerDO")
                .and_then(|seller| seller.get("sellerId")),
        ),
        value_text(data.get("sellerId")),
        value_text(
            data.get("trackParams")
                .and_then(|track| track.get("sellerId")),
        ),
    ]);
    !returned_seller_id.bytes().all(|byte| byte.is_ascii_digit())
        || returned_seller_id.is_empty()
        || returned_seller_id == seller_id
}

fn tag_texts(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|tag| value_text(tag.get("text")))
        .filter(|text| !text.is_empty())
        .collect()
}

fn set_candidate_text_if_missing(candidate: &mut Value, key: &str, value: String) {
    if value.is_empty() || !value_text(candidate.get(key)).is_empty() {
        return;
    }
    candidate[key] = json!(value);
}

fn merge_wx_seller_enhancement(candidate: &mut Value, data: &Value) {
    let seller = data.get("sellerDO").unwrap_or(&Value::Null);
    set_candidate_text_if_missing(
        candidate,
        "replyRatio24h",
        value_text(seller.get("replyRatio24h")),
    );
    set_candidate_text_if_missing(
        candidate,
        "replyInterval",
        value_text(seller.get("replyInterval")),
    );
    set_candidate_text_if_missing(
        candidate,
        "registeredDays",
        value_text(seller.get("userRegDay")),
    );
    set_candidate_text_if_missing(
        candidate,
        "registerTime",
        value_text(seller.get("registerTime")),
    );
    set_candidate_text_if_missing(
        candidate,
        "soldCount",
        value_text(seller.get("hasSoldNumInteger")),
    );
    set_candidate_text_if_missing(
        candidate,
        "praiseRatio",
        value_text(seller.get("newGoodRatioRate")),
    );
    set_candidate_text_if_missing(
        candidate,
        "creditLevel",
        value_text(
            seller
                .get("zhimaLevelInfo")
                .and_then(|credit| credit.get("levelName")),
        ),
    );
    set_candidate_text_if_missing(
        candidate,
        "sellerSummary",
        value_text(seller.get("signature")),
    );
    let verification_tags = tag_texts(seller.get("identityTags"));
    if !verification_tags.is_empty() {
        candidate["verificationTags"] = json!(verification_tags);
    }
    let seller_tags = tag_texts(seller.get("sellerInfoTags"));
    if !seller_tags.is_empty() {
        candidate["sellerTags"] = json!(seller_tags);
    }
    if let Some(zhima_auth) = seller.get("zhimaAuth").and_then(Value::as_bool) {
        candidate["zhimaAuth"] = json!(zhima_auth);
    }
    candidate["detailEnhancementSource"] = json!("wx_mini_program");
}

async fn load_public_profile_by_seller_id(
    state: &AppState,
    seller_id: &str,
    item_id: Option<&str>,
    source_item: &Value,
) -> Result<Value, String> {
    if !is_stable_public_id(seller_id) {
        return Err("public profile link did not contain a numeric user id".to_string());
    }

    let profile = public_h5_request(
        state,
        crate::sealed_protocol::sealed_str!("mtop.idle.web.user.page.head"),
        "1.0",
        json!({
            "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
            "gps": PUBLIC_PROFILE_GPS,
            "userId": seller_id,
        }),
        Vec::new(),
    )
    .await?;
    let items = public_h5_request(
        state,
        crate::sealed_protocol::sealed_str!("mtop.idle.web.xyh.item.list"),
        "1.0",
        json!({
            "needGroupInfo": true,
            "pageNumber": 1,
            "pageSize": 20,
            "userId": seller_id,
        }),
        Vec::new(),
    )
    .await?;
    // Timeline is public data, but upstream rejects an account-bound session
    // for some sellers. Fetch it in a fresh anonymous H5 session and let a
    // timeline-only failure leave the verified profile usable.
    let timeline_payload = json!({
        "endDate": chrono::Local::now().format("%Y%m%d").to_string(),
        "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
        "gps": PUBLIC_PROFILE_GPS,
        "pageSize": 20,
        "type": "",
        "userId": seller_id,
    });
    let timeline = crate::xianyu_client::anonymous_public_h5_request(
        state.config(),
        crate::sealed_protocol::sealed_str!("mtop.idle.user.timeline.list"),
        "1.0",
        &timeline_payload,
        Vec::new(),
    )
    .await
    .ok()
    .filter(payload_ret_success);

    let mut candidate = profile_candidate_from_head(
        seller_id,
        profile.get("data").unwrap_or(&Value::Null),
        item_id,
        source_item,
    );
    candidate["sellerItems"] = json!(public_item_cards(items.get("data").unwrap_or(&Value::Null)));
    let timeline_data = timeline
        .as_ref()
        .and_then(|payload| payload.get("data"))
        .unwrap_or(&Value::Null);
    candidate["timelineVisibility"] = json!(timeline_visibility(timeline_data));
    candidate["timelinePrivacyDays"] = json!(value_text(timeline_data.get("privacyDays")));
    candidate["timelineItems"] = json!(public_timeline_cards(timeline_data));
    Ok(candidate)
}

fn profile_candidate_from_head(
    seller_id: &str,
    profile_data: &Value,
    item_id: Option<&str>,
    source_item: &Value,
) -> Value {
    let mut candidate = item_id
        .and_then(|id| search_candidate(id, source_item))
        .unwrap_or_else(|| {
            item_id
                .map(|id| json!({ "itemId": id }))
                .unwrap_or_else(|| json!({}))
        });
    candidate["sellerId"] = json!(seller_id);
    candidate["profileUserId"] = candidate["sellerId"].clone();
    candidate["profileUrl"] = json!(format!(
        "{}{}",
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/personal?userId="),
        seller_id
    ));
    merge_public_profile_head(&mut candidate, profile_data, source_item);
    candidate
}

async fn fetch_all_public_item_cards(
    state: &AppState,
    seller_id: &str,
) -> Result<Vec<Value>, String> {
    let mut cards = Vec::new();
    let mut seen = HashSet::new();
    for page_number in 1..=PEER_MONITOR_MAX_PAGES {
        let payload = public_h5_request(
            state,
            crate::sealed_protocol::sealed_str!("mtop.idle.web.xyh.item.list"),
            "1.0",
            json!({
                "needGroupInfo": true,
                "pageNumber": page_number,
                "pageSize": PEER_MONITOR_ITEM_PAGE_SIZE,
                "userId": seller_id,
            }),
            Vec::new(),
        )
        .await?;
        let data = payload.get("data").unwrap_or(&Value::Null);
        for card in public_item_cards(data) {
            let item_id = value_text(card.get("itemId"));
            if !item_id.is_empty() && seen.insert(item_id) {
                cards.push(card);
            }
        }
        if !data
            .get("nextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(cards);
        }
    }
    Err("同行公开宝贝分页超过安全上限，未写入不完整快照".to_string())
}

async fn fetch_all_public_timeline_cards(
    state: &AppState,
    seller_id: &str,
) -> Result<Vec<Value>, String> {
    let mut cards = Vec::new();
    let mut seen_events = HashSet::new();
    let mut seen_dates = HashSet::new();
    let mut end_date = chrono::Local::now().format("%Y%m%d").to_string();
    for _ in 0..PEER_MONITOR_MAX_PAGES {
        let payload = crate::xianyu_client::anonymous_public_h5_request(
            state.config(),
            crate::sealed_protocol::sealed_str!("mtop.idle.user.timeline.list"),
            "1.0",
            &json!({
                "endDate": end_date,
                "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
                "gps": PUBLIC_PROFILE_GPS,
                "pageSize": PEER_MONITOR_TIMELINE_PAGE_SIZE,
                "type": "",
                "userId": seller_id,
            }),
            Vec::new(),
        )
        .await?;
        if !payload_ret_success(&payload) {
            return Err("同行公开动态请求未成功".to_string());
        }
        let data = payload.get("data").unwrap_or(&Value::Null);
        for card in public_timeline_cards(data) {
            let key = timeline_event_key(&card);
            if seen_events.insert(key) {
                cards.push(card);
            }
        }
        if !data
            .get("nextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(cards);
        }
        let next_end_date = value_text(data.get("endDate"));
        if next_end_date.is_empty() || !seen_dates.insert(next_end_date.clone()) {
            return Err("同行公开动态分页游标无效，未写入不完整快照".to_string());
        }
        end_date = next_end_date;
    }
    Err("同行公开动态分页超过安全上限，未写入不完整快照".to_string())
}

async fn fetch_all_public_reviews(state: &AppState, seller_id: &str) -> Result<Vec<Value>, String> {
    let mut reviews = Vec::new();
    let mut seen = HashSet::new();
    for page_number in 1..=PEER_MONITOR_MAX_PAGES {
        let payload = public_h5_request(
            state,
            crate::sealed_protocol::sealed_str!("mtop.idle.web.trade.rate.list"),
            "1.0",
            json!({
                "rateType": 0,
                "ratedUid": seller_id,
                "raterType": 0,
                "rowsPerPage": PEER_MONITOR_RATE_PAGE_SIZE,
                "pageNumber": page_number,
                "foldFlag": 0,
                "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
                "extraTag": "",
            }),
            Vec::new(),
        )
        .await?;
        let data = payload.get("data").unwrap_or(&Value::Null);
        for review in public_review_cards(data) {
            let rate_id = value_text(review.get("id"));
            if !rate_id.is_empty() && seen.insert(rate_id) {
                reviews.push(review);
            }
        }
        if !data
            .get("nextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(reviews);
        }
    }
    Err("同行公开评价分页超过安全上限，未写入不完整快照".to_string())
}

async fn collect_public_peer_snapshot(
    state: &AppState,
    seller_id: &str,
    review_source_uid: Option<&str>,
) -> Result<PublicPeerSnapshot, String> {
    let profile_payload = public_h5_request(
        state,
        crate::sealed_protocol::sealed_str!("mtop.idle.web.user.page.head"),
        "1.0",
        json!({
            "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
            "gps": PUBLIC_PROFILE_GPS,
            "userId": seller_id,
        }),
        Vec::new(),
    )
    .await?;
    let mut profile = profile_candidate_from_head(
        seller_id,
        profile_payload.get("data").unwrap_or(&Value::Null),
        None,
        &Value::Null,
    );
    let items = fetch_all_public_item_cards(state, seller_id).await?;
    profile["itemCount"] = json!(items.len());

    let mut warnings = Vec::new();
    let timeline = match fetch_all_public_timeline_cards(state, seller_id).await {
        Ok(timeline) => Some(timeline),
        Err(error) => {
            warnings.push(format!("动态未更新：{error}"));
            None
        }
    };
    let reviews = match fetch_all_public_reviews(state, seller_id).await {
        Ok(seller_reviews) => {
            // The seller-wide list is complete.  Enrich only through the
            // verified mini-program feed; it never guesses an item from a
            // transaction ID or from the current public item list.
            let (item_reviews, _) = public_item_rate_reviews_for_page(
                state,
                review_source_uid,
                seller_id,
                &seller_reviews,
            )
            .await;
            Some(enrich_public_reviews_with_item_rates(
                seller_reviews,
                &item_reviews,
            ))
        }
        Err(error) => {
            warnings.push(format!("评价未更新：{error}"));
            None
        }
    };

    Ok(PublicPeerSnapshot {
        profile,
        items,
        timeline,
        reviews,
        warnings,
    })
}

async fn load_candidate_detail(
    state: &AppState,
    source_uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    let h5_payload = json!({ "itemId": item_id });

    // Keep peer monitoring on the agreed H5 + mini-program chain. The App
    // detail transport has been observed echoing the logged-in account into
    // seller fields and is intentionally excluded here.
    if let Ok(payload) = successful_upstream_h5_request(
        state,
        source_uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
        "1.0",
        h5_payload,
        Vec::new(),
    )
    .await
    {
        return Ok(payload);
    }
    successful_upstream_wx_request(
        state,
        source_uid,
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.weixin.detail"),
        "1.0",
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.78",
            "itemId": item_id,
        }),
    )
    .await
}

async fn resolve_candidate(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ResolveCandidatePayload>,
) -> ApiResult {
    let source_uid = payload.source_uid.trim();
    let item_id = payload.item_id.trim();
    if source_uid.is_empty() || item_id.is_empty() {
        return Err(ResponseError::bad_request("缺少搜索账号或商品标识"));
    }
    let owner = owner_username_filter(&state, &identity);
    crate::routes::tenant::ensure_account_uid_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        source_uid,
    )
    .await?;
    let source_uids = active_public_source_uids(
        state.storage().accounts(),
        owner.as_deref(),
        Some(source_uid),
    )
    .await
    .map_err(ResponseError::internal)?;
    // `awesome.detail` exposes the numeric user ID used by public seller
    // profiles. Unlike `pc.detail`, it does not echo the requesting account
    // into the seller fields, so always prefer it over the search-card ID.
    match load_public_profile_candidate(&state, &source_uids, item_id, &payload.source_item).await {
        Ok(candidate) => {
            return Ok(Json(json!({
                "candidate": candidate,
                "detailLimited": false,
            })));
        }
        Err(_) => {}
    }

    // Keep the seller fields displayed in the originating search card as a
    // non-authoritative fallback. They must never be replaced by `pc.detail`,
    // which has been observed returning the requesting account as the seller.
    if let Some(candidate) = search_candidate(item_id, &payload.source_item) {
        return Ok(Json(json!({
            "candidate": candidate,
            "detailLimited": true,
        })));
    }
    let upstream = load_candidate_detail(&state, source_uid, item_id)
        .await
        .map_err(|_| {
            ResponseError::service_unavailable(
                "同行公开资料暂时无法读取，请更换已登录的搜索账号后重试",
            )
        })?;
    // `pc.detail` returns the seller ID as either a JSON string or number,
    // depending on the account and item. Do not reject a numeric stable ID.
    let data = upstream.get("data").cloned().unwrap_or(Value::Null);
    let seller_id = find_text(
        &data,
        &[
            "sellerId",
            "seller_id",
            "sellerUserId",
            "ownerId",
            "owner_id",
            "userId",
            "user_id",
        ],
    );
    if seller_id.is_empty() {
        return Err(ResponseError::bad_request(
            "平台未返回稳定同行标识，暂不能加入监控",
        ));
    }
    // The PC/App detail API can return the requesting account as every seller
    // field. Treat that as insufficient instead of overwriting the verified
    // seller data that the search result already supplied to the page.
    if detail_points_to_requester(&seller_id, source_uid) {
        return Ok(Json(json!({
            "candidate": {},
            "detailLimited": true,
        })));
    }
    Ok(Json(json!({ "candidate": {
        "itemId": item_id, "sellerId": seller_id,
        "sellerNick": find_text(&data, &["sellerNick", "userNick", "nick", "nickname"]),
        "sellerAvatar": find_text(&data, &["sellerAvatar", "userAvatar", "avatar", "logo"]),
        "title": find_text(&data, &["title", "itemTitle"]),
        "price": find_text(&data, &["soldPrice", "price", "displayPrice"]),
        "picUrl": find_text(&data, &["picUrl", "imageUrl", "mainPic"]),
    }})))
}

async fn resolve_link_candidate(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ResolveLinkPayload>,
) -> ApiResult {
    let source_uids = active_public_source_uids(
        state.storage().accounts(),
        owner_username_filter(&state, &identity).as_deref(),
        None,
    )
    .await
    .map_err(ResponseError::internal)?;
    let target = resolve_peer_link_target(payload.link.trim())
        .await
        .map_err(|error| ResponseError::bad_request(&error))?;
    let (candidate, input_kind) = match target {
        PeerLinkTarget::Item(item_id) => (
            load_public_profile_candidate(&state, &source_uids, &item_id, &Value::Null)
                .await
                .map_err(|_| {
                    ResponseError::service_unavailable("商品公开资料暂时无法读取，请稍后重试")
                })?,
            "item",
        ),
        PeerLinkTarget::Seller(seller_id) => (
            load_public_profile_by_seller_id(&state, &seller_id, None, &Value::Null)
                .await
                .map_err(|_| {
                    ResponseError::service_unavailable("同行公开主页暂时无法读取，请稍后重试")
                })?,
            "seller",
        ),
    };
    Ok(Json(json!({
        "candidate": candidate,
        "inputKind": input_kind,
    })))
}

async fn list_public_reviews(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(seller_id): Path<String>,
    Query(query): Query<PublicReviewQuery>,
) -> ApiResult {
    if !is_stable_public_id(&seller_id) {
        return Err(ResponseError::bad_request("同行主页标识无效"));
    }
    let page_number = query.page.unwrap_or(1).clamp(1, 50);
    let rate_type = query.rate_type.unwrap_or(0);
    let extra_tag = query.extra_tag.unwrap_or_default();
    if extra_tag.chars().count() > 80 {
        return Err(ResponseError::bad_request("评价筛选条件无效"));
    }
    let fold_flag = query.fold_flag.unwrap_or(0).clamp(0, 1);
    let review_source_uids =
        public_review_source_uids(&state, &identity, query.source_uid.as_deref()).await?;

    // Seller-wide lists are the complete, correctly paginated review source.
    // Product-scoped feeds only enrich matching records with their real item;
    // they must never replace or truncate the seller-wide list.
    let state_for_reviews = state.clone();
    let seller_id_for_reviews = seller_id.clone();
    if let Some(upstream) = first_successful_source(&review_source_uids, move |source_uid| {
        let state = state_for_reviews.clone();
        let seller_id = seller_id_for_reviews.clone();
        async move {
            successful_upstream_wx_request(
                &state,
                &source_uid,
                crate::sealed_protocol::sealed_str!("mtop.taobao.idle.rate.list"),
                "6.0",
                json!({
                    "rateType": rate_type,
                    "raterType": 0,
                    "rowsPerPage": 20,
                    "pageNumber": page_number,
                    "foldFlag": fold_flag,
                    "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
                    "gps": PUBLIC_PROFILE_GPS,
                    "ratedUid": seller_id,
                }),
            )
            .await
            .ok()
            .map(|payload| json!({ "sourceUid": source_uid, "payload": payload }))
        }
    })
    .await
    {
        let data = upstream
            .get("payload")
            .and_then(|payload| payload.get("data"))
            .unwrap_or(&Value::Null);
        let seller_reviews = public_review_cards(data);
        let source_uid = value_text(upstream.get("sourceUid"));
        let (item_reviews, _) = public_item_rate_reviews_for_page(
            &state,
            Some(source_uid.as_str()),
            &seller_id,
            &seller_reviews,
        )
        .await;
        let reviews = enrich_public_reviews_with_item_rates(seller_reviews, &item_reviews);
        return Ok(Json(json!({
            "reviews": reviews,
            "tabs": public_review_tabs(data),
            "hasNext": data.get("nextPage").and_then(Value::as_bool).unwrap_or(false),
            "page": page_number,
            "source": "wx_seller_rate",
        })));
    }

    let payload = public_h5_request(
        &state,
        crate::sealed_protocol::sealed_str!("mtop.idle.web.trade.rate.list"),
        "1.0",
        json!({
            "rateType": rate_type,
            "ratedUid": seller_id,
            "raterType": 0,
            "rowsPerPage": 20,
            "pageNumber": page_number,
            "foldFlag": fold_flag,
            "fishAdCode": PUBLIC_PROFILE_FISH_AD_CODE,
            "extraTag": extra_tag,
        }),
        Vec::new(),
    )
    .await;
    let Ok(payload) = payload else {
        let (item_reviews, item_has_next) = public_item_rate_reviews_for_page(
            &state,
            review_source_uids.first().map(String::as_str),
            &seller_id,
            &[],
        )
        .await;
        if !item_reviews.is_empty() {
            return Ok(Json(json!({
                "reviews": item_reviews,
                "tabs": [],
                "hasNext": item_has_next,
                "page": page_number,
                "source": "item_rate",
            })));
        }
        return Err(ResponseError::service_unavailable(
            "同行公开评价暂时无法读取，请稍后重试",
        ));
    };
    let data = payload.get("data").unwrap_or(&Value::Null);
    let seller_reviews = public_review_cards(data);
    let (item_reviews, _) = public_item_rate_reviews_for_page(
        &state,
        review_source_uids.first().map(String::as_str),
        &seller_id,
        &seller_reviews,
    )
    .await;
    let reviews = enrich_public_reviews_with_item_rates(seller_reviews, &item_reviews);
    Ok(Json(json!({
        "reviews": reviews,
        "tabs": public_review_tabs(data),
        "hasNext": data.get("nextPage").and_then(Value::as_bool).unwrap_or(false),
        "page": page_number,
        "source": "seller_rate_fallback",
    })))
}

async fn public_item_rate_reviews_for_page(
    state: &AppState,
    source_uid: Option<&str>,
    seller_id: &str,
    seller_reviews: &[Value],
) -> (Vec<Value>, bool) {
    let unresolved_keys = unresolved_review_match_keys(seller_reviews);
    let mut item_has_next = false;
    let mut all_item_reviews = Vec::new();

    // The seller review list only exposes review/transaction IDs. The item
    // review feed exposes the actual product card. Read the seller's public
    // item pages until all missing review keys are joined, not merely the
    // first page of currently visible goods.
    for item_page in 1..=ITEM_RATE_MAX_ITEM_PAGES {
        let Ok(items_payload) = public_h5_request(
            state,
            crate::sealed_protocol::sealed_str!("mtop.idle.web.xyh.item.list"),
            "1.0",
            json!({
                "needGroupInfo": true,
                "pageNumber": item_page,
                "pageSize": ITEM_RATE_ITEMS_PER_PAGE,
                "userId": seller_id,
            }),
            Vec::new(),
        )
        .await
        else {
            break;
        };
        let items_data = items_payload.get("data").unwrap_or(&Value::Null);
        item_has_next = items_data
            .get("nextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let items = public_item_cards(items_data);
        if items.is_empty() {
            break;
        }

        let state_for_requests = state.clone();
        let seller_id_for_requests = seller_id.to_string();
        let source_uid_for_requests = source_uid.unwrap_or_default().to_string();
        let item_reviews =
            stream::iter(items.into_iter().map(move |item| {
                let state = state_for_requests.clone();
                let seller_id = seller_id_for_requests.clone();
                let source_uid = source_uid_for_requests.clone();
                async move {
                    public_item_rate_reviews(&state, source_uid.as_str(), &seller_id, item).await
                }
            }))
            .buffer_unordered(ITEM_RATE_CONCURRENCY)
            .filter_map(|result| async move { result.ok() })
            .flat_map(stream::iter)
            .collect::<Vec<_>>()
            .await;
        all_item_reviews.extend(item_reviews);

        if !unresolved_keys.is_empty()
            && unresolved_review_match_keys_have_item(&unresolved_keys, &all_item_reviews)
        {
            item_has_next = false;
            break;
        }
        if unresolved_keys.is_empty() || !item_has_next {
            break;
        }
    }

    (deduplicate_item_reviews(all_item_reviews), item_has_next)
}

async fn public_review_source_uids(
    state: &AppState,
    identity: &RequestIdentity,
    requested_uid: Option<&str>,
) -> Result<Vec<String>, ResponseError> {
    let owner_username = owner_username_filter(state, identity);
    let requested_uid = requested_uid.map(str::trim).filter(|uid| !uid.is_empty());
    if let Some(uid) = requested_uid {
        ensure_account_uid_accessible(state.storage().accounts(), owner_username.as_deref(), uid)
            .await?;
    }

    active_public_source_uids(
        state.storage().accounts(),
        owner_username.as_deref(),
        requested_uid,
    )
    .await
    .map_err(ResponseError::internal)
}

async fn active_public_source_uids(
    pool: &SqlitePool,
    owner_username: Option<&str>,
    preferred_uid: Option<&str>,
) -> Result<Vec<String>, sqlx::Error> {
    let preferred_uid = preferred_uid.unwrap_or_default();
    match owner_username {
        Some(owner_username) => {
            sqlx::query_scalar::<_, String>(
                "SELECT uid FROM accounts WHERE status = 'active' AND owner_username = ? AND TRIM(uid) <> '' ORDER BY CASE WHEN uid = ? THEN 0 ELSE 1 END, last_session_ok_at DESC, updated_at DESC, uid ASC",
            )
            .bind(owner_username)
            .bind(preferred_uid)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_scalar::<_, String>(
                "SELECT uid FROM accounts WHERE status = 'active' AND TRIM(uid) <> '' ORDER BY CASE WHEN uid = ? THEN 0 ELSE 1 END, last_session_ok_at DESC, updated_at DESC, uid ASC",
            )
            .bind(preferred_uid)
            .fetch_all(pool)
            .await
        }
    }
}

async fn first_successful_source<T, F, Fut>(source_uids: &[String], mut attempt: F) -> Option<T>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Option<T>>,
{
    for source_uid in source_uids {
        if let Some(result) = attempt(source_uid.clone()).await {
            return Some(result);
        }
    }
    None
}

async fn public_item_rate_reviews(
    state: &AppState,
    source_uid: &str,
    seller_id: &str,
    item: Value,
) -> Result<Vec<Value>, String> {
    let item_id = value_text(item.get("itemId"));
    if item_id.is_empty() {
        return Ok(Vec::new());
    }
    let mut all_reviews = Vec::new();
    let mut next_token = String::new();
    let mut seen_tokens = std::collections::HashSet::new();

    for _ in 0..ITEM_RATE_MAX_CURSOR_PAGES {
        let mut request_data = json!({
            "itemId": item_id,
            "sellerId": seller_id,
            "pageSize": ITEM_RATE_PAGE_SIZE,
            "source": "idledetail",
            "needSellerInfo": "true",
        });
        if !next_token.is_empty() {
            request_data["nextToken"] = json!(next_token);
        }
        let payload = public_item_rate_payload(state, source_uid, request_data).await?;
        if !payload_ret_success(&payload) {
            return Err("public item rate request was not successful".to_string());
        }
        let data = payload.get("data").unwrap_or(&Value::Null);
        all_reviews.extend(public_item_rate_cards(data, &item));
        let next_cursor = public_item_rate_next_cursor(data);
        if next_cursor.is_empty() || !seen_tokens.insert(next_cursor.clone()) {
            break;
        }
        next_token = next_cursor;
    }
    Ok(all_reviews)
}

async fn public_item_rate_payload(
    state: &AppState,
    source_uid: &str,
    request_data: Value,
) -> Result<Value, String> {
    // This is the small-program item-review feed. It has the exact
    // `orderId/rateId -> itemId` relation needed to join a seller-wide review
    // to its product. If the local source account's mini-program session is
    // stale, use the same public H5 contract rather than substituting a
    // guessed item.
    if !source_uid.trim().is_empty() {
        if let Ok(payload) = successful_upstream_wx_request(
            state,
            source_uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ecology.rate.serve.feeds"),
            "1.0",
            request_data.clone(),
        )
        .await
        {
            // A successful MTOP envelope is not enough here. Some stale
            // mini-program sessions return an empty data object, which has
            // no review list to join with the seller-wide rates. In that
            // case use the verified H5 variant of this exact feed instead of
            // treating the empty envelope as an authoritative empty result.
            if public_item_rate_payload_has_list(&payload) {
                return Ok(payload);
            }
        }
    }
    crate::xianyu_client::anonymous_public_h5_request_with_app_key_and_profile(
        state.config(),
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ecology.rate.serve.feeds"),
        "1.0",
        &request_data,
        vec![("accountSite".to_string(), "xianyu".to_string())],
        AWESOME_DETAIL_APP_KEY,
        crate::xianyu_client::H5RequestProfile::AwesomeDetailMobile,
    )
    .await
}

fn public_item_rate_payload_has_list(payload: &Value) -> bool {
    let data = payload.get("data").unwrap_or(&Value::Null);
    [
        "result",
        "idleRateServeDTOList",
        "rateList",
        "feeds",
        "list",
    ]
    .iter()
    .any(|key| data.get(*key).is_some_and(Value::is_array))
}

fn public_item_rate_next_cursor(data: &Value) -> String {
    first_text([
        value_text(data.get("lastCursor")),
        value_text(data.get("nextToken")),
    ])
}

fn unresolved_review_match_keys(reviews: &[Value]) -> std::collections::HashSet<String> {
    reviews
        .iter()
        .filter(|review| !review_has_item(review))
        .flat_map(review_match_keys)
        .collect()
}

fn review_match_keys(review: &Value) -> Vec<String> {
    let rate_id = value_text(review.get("id"));
    let trade_id = value_text(review.get("tradeId"));
    let order_id = value_text(review.get("orderId"));
    let mut keys = Vec::new();
    if !rate_id.is_empty() {
        keys.push(format!("rate:{rate_id}"));
    }
    // `rate.list` calls this value tradeId while the product review feed
    // calls the same value orderId. Keep the namespaces explicit, but add
    // the verified aliases so a transaction-only record can still be joined.
    for value in [trade_id, order_id] {
        if !value.is_empty() {
            keys.push(format!("trade:{value}"));
            keys.push(format!("order:{value}"));
        }
    }
    keys
}

fn unresolved_review_match_keys_have_item(
    unresolved_keys: &std::collections::HashSet<String>,
    item_reviews: &[Value],
) -> bool {
    let found_keys = item_reviews
        .iter()
        .flat_map(review_match_keys)
        .collect::<std::collections::HashSet<_>>();
    unresolved_keys.iter().all(|key| found_keys.contains(key))
}

fn public_item_rate_cards(data: &Value, item: &Value) -> Vec<Value> {
    let fallback_item_id = value_text(item.get("itemId"));
    data.get("result")
        .or_else(|| data.get("idleRateServeDTOList"))
        .or_else(|| data.get("rateList"))
        .or_else(|| data.get("feeds"))
        .or_else(|| data.get("list"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let rate = row
                .get("idleRateServeDTO")
                .or_else(|| row.get("rateServeDTO"))
                .unwrap_or(row);
            let rate_id = first_text([
                value_text(rate.get("rateId")),
                value_text(row.get("rateId")),
            ]);
            let order_id = first_text([
                value_text(rate.get("orderId")),
                value_text(row.get("orderId")),
            ]);
            if rate_id.is_empty() && order_id.is_empty() {
                return None;
            }
            let pictures = row
                .get("images")
                .or_else(|| row.get("pictCdnUrlList"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|image| first_text([value_text(image.get("url")), value_text(Some(image))]))
                .filter(|image| !image.is_empty())
                .collect::<Vec<_>>();
            Some(json!({
                "id": first_text([rate_id, order_id]),
                "orderId": first_text([value_text(rate.get("orderId")), value_text(row.get("orderId"))]),
                "rate": rate.get("rate").and_then(Value::as_i64).unwrap_or(-1),
                "feedback": first_text([value_text(row.get("content")), value_text(row.get("feedback")), value_text(rate.get("feedback")), value_text(rate.get("content"))]),
                "createdAt": first_text([value_text(row.get("gmtCreateStr")), value_text(row.get("publishTime")), value_text(rate.get("gmtCreateStr")), value_text(rate.get("gmtCreate"))]),
                "raterNick": first_text([value_text(row.get("raterNick")), value_text(row.get("raterUserNick")), value_text(rate.get("raterUserNick")), value_text(rate.get("raterNick")), value_text(rate.get("buyerNick"))]),
                "raterAvatar": first_text([value_text(row.get("raterHeadImg")), value_text(row.get("avatar")), value_text(rate.get("raterHeadImg")), value_text(rate.get("raterAvatar"))]),
                "tags": rate.get("rateTagList").and_then(Value::as_array).into_iter().flatten().map(|tag| value_text(tag.get("text"))).filter(|tag| !tag.is_empty()).collect::<Vec<_>>(),
                "pictures": pictures,
                "itemId": first_text([value_text(rate.get("itemId")), value_text(row.get("itemId")), fallback_item_id.clone()]),
                "itemTitle": first_text([value_text(rate.get("itemTitle")), value_text(row.get("itemTitle")), value_text(rate.get("title")), value_text(row.get("title")), value_text(item.get("title"))]),
                "itemPrice": first_text([value_text(rate.get("itemPrice")), value_text(row.get("itemPrice")), value_text(rate.get("price")), value_text(row.get("price")), value_text(item.get("price"))]),
                "itemPic": value_text(item.get("picUrl")),
                "source": "item_rate",
            }))
        })
        .collect()
}

fn deduplicate_item_reviews(reviews: Vec<Value>) -> Vec<Value> {
    let mut seen = std::collections::HashSet::new();
    reviews
        .into_iter()
        .filter(|review| {
            let key = first_text([
                value_text(review.get("orderId")),
                value_text(review.get("id")),
            ]);
            !key.is_empty() && seen.insert(key)
        })
        .collect()
}

fn enrich_public_reviews_with_item_rates(
    reviews: Vec<Value>,
    item_reviews: &[Value],
) -> Vec<Value> {
    let mut item_by_review_id = std::collections::HashMap::new();
    let mut item_by_order_id = std::collections::HashMap::new();
    for item_review in item_reviews {
        let review_id = value_text(item_review.get("id"));
        if !review_id.is_empty() {
            item_by_review_id.entry(review_id).or_insert(item_review);
        }
        let order_id = value_text(item_review.get("orderId"));
        if !order_id.is_empty() {
            item_by_order_id.entry(order_id).or_insert(item_review);
        }
    }

    reviews
        .into_iter()
        .map(|mut review| {
            let review_id = value_text(review.get("id"));
            let trade_id = value_text(review.get("tradeId"));
            let matching_item = item_by_review_id
                .get(&review_id)
                .copied()
                .or_else(|| item_by_order_id.get(&trade_id).copied());
            let Some(item_review) = matching_item else {
                return review;
            };
            for key in ["itemId", "itemTitle", "itemPrice", "itemPic"] {
                if value_text(review.get(key)).is_empty() {
                    let value = item_review.get(key).cloned().unwrap_or(Value::Null);
                    if !value_text(Some(&value)).is_empty() {
                        review[key] = value;
                    }
                }
            }
            review
        })
        .collect()
}

fn review_has_item(review: &Value) -> bool {
    ["itemId", "itemTitle", "itemPic"]
        .iter()
        .any(|key| is_meaningful_detail_text(&value_text(review.get(*key))))
}

fn public_review_tabs(data: &Value) -> Vec<Value> {
    data.get("rateTabDOList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|tab| {
            json!({
                "type": tab.get("tabType").and_then(Value::as_i64).unwrap_or_default(),
                "tag": value_text(tab.get("tabId")),
                "label": value_text(tab.get("tabName")),
                "count": value_text(tab.get("tabCount")),
            })
        })
        .filter(|tab| !value_text(tab.get("label")).is_empty())
        .collect()
}

fn public_review_cards(data: &Value) -> Vec<Value> {
    data.get("cardList")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|card| card.get("cardData").unwrap_or(card))
        .map(|card| {
            let tags = card
                .get("rateTagList")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|tag| value_text(tag.get("text")))
                .filter(|tag| !tag.is_empty())
                .collect::<Vec<_>>();
            let pictures = card
                .get("pictCdnUrlList")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|image| value_text(Some(image)))
                .filter(|image| !image.is_empty())
                .collect::<Vec<_>>();
            let item_id = value_text(card.get("itemId"));
            let trade_id = value_text(card.get("tradeId"));
            let rated_uid = value_text(card.get("ratedUid"));
            let biz_code = value_text(card.get("bizCode"));
            json!({
                "id": value_text(card.get("rateId")),
                "rate": card.get("rate").and_then(Value::as_i64).unwrap_or(-1),
                "feedback": value_text(card.get("feedback")),
                "createdAt": first_text([value_text(card.get("gmtCreateStr")), value_text(card.get("gmtCreate")), value_text(card.get("timeDesc"))]),
                "raterNick": value_text(card.get("raterUserNick")),
                "raterAvatar": value_text(card.get("raterHeadImg")),
                "ipAddress": value_text(card.get("ipAddress")),
                "tags": tags,
                "pictures": pictures,
                "tradeId": trade_id,
                "ratedUid": rated_uid,
                "bizCode": biz_code,
                "itemId": if item_id == "0" { String::new() } else { item_id },
                "itemTitle": first_text([
                    value_text(card.get("itemTitle")),
                    value_text(card.get("tradeInfo").and_then(|trade| trade.get("itemTitle"))),
                    value_text(card.get("tradeInfo").and_then(|trade| trade.get("title"))),
                ]),
                "itemPrice": value_text(card.get("itemPrice")),
                "tradeInfo": card.get("tradeInfo").cloned().unwrap_or(Value::Null),
                "appendReview": card.get("appendRateList").and_then(Value::as_array).and_then(|items| items.first()).map(|append| json!({
                    "label": value_text(append.get("desc")),
                    "feedback": value_text(append.get("feedback")),
                    "pictures": append.get("picUrls").and_then(Value::as_array).into_iter().flatten().map(|image| value_text(Some(image))).filter(|image| !image.is_empty()).collect::<Vec<_>>(),
                })),
                "reply": card.get("replyList").and_then(Value::as_array).and_then(|items| items.first()).map(|reply| json!({
                    "feedback": value_text(reply.get("feedback")),
                })),
            })
        })
        .filter(|review| !value_text(review.get("id")).is_empty())
        .collect()
}

async fn insert_peer_monitor_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    target_id: i64,
    event_type: &str,
    title: &str,
    summary: &str,
    entity_type: &str,
    entity_id: &str,
    previous: &Value,
    current: &Value,
) -> Result<(), String> {
    sqlx::query("INSERT INTO peer_monitor_events (target_id, event_type, title, summary, entity_type, entity_id, previous_json, current_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(target_id)
        .bind(event_type)
        .bind(title)
        .bind(summary)
        .bind(entity_type)
        .bind(entity_id)
        .bind(serde_json::to_string(previous).unwrap_or_default())
        .bind(serde_json::to_string(current).unwrap_or_default())
        .execute(&mut **tx)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn profile_change_labels(previous: &Value, current: &Value) -> Vec<&'static str> {
    [
        ("sellerNick", "昵称"),
        ("sellerAvatar", "头像"),
        ("sellerSummary", "个人简介"),
        ("sellerLevel", "卖家等级"),
        ("shopLevel", "店铺等级"),
        ("followersCount", "粉丝数"),
        ("followingCount", "关注数"),
        ("itemCount", "公开宝贝数"),
        ("rateCount", "评价数"),
        ("dynamicCount", "动态数"),
        ("area", "地区"),
    ]
    .into_iter()
    .filter_map(|(key, label)| {
        let old = previous.get(key).cloned().unwrap_or(Value::Null);
        let new = current.get(key).cloned().unwrap_or(Value::Null);
        (!new.is_null() && old != new).then_some(label)
    })
    .collect()
}

fn monitor_item_value(item: &Value) -> Value {
    json!({
        "itemId": value_text(item.get("itemId")),
        "title": value_text(item.get("title")),
        "price": value_text(item.get("price")),
        "imageUrl": value_text(item.get("picUrl")),
        "status": value_text(item.get("status")),
    })
}

fn target_profile_json(row: &sqlx::sqlite::SqliteRow) -> Value {
    let value = json_text(row, "profile_json");
    value
        .is_object()
        .then_some(value)
        .unwrap_or_else(|| json!({}))
}

async fn persist_public_peer_snapshot(
    pool: &SqlitePool,
    target: &sqlx::sqlite::SqliteRow,
    run_id: i64,
    snapshot: PublicPeerSnapshot,
) -> Result<PeerMonitorRefreshOutcome, String> {
    let target_id: i64 = target.try_get("id").map_err(|error| error.to_string())?;
    let now = unix_timestamp_seconds();
    let has_baseline = number(target, "last_success_at") > 0.0;
    let previous_profile = target_profile_json(target);
    let profile_json =
        serde_json::to_string(&snapshot.profile).map_err(|error| error.to_string())?;
    let profile_changes = has_baseline
        .then(|| profile_change_labels(&previous_profile, &snapshot.profile))
        .unwrap_or_default();
    let mut tx = pool.begin().await.map_err(|error| error.to_string())?;

    if !has_baseline {
        // Pre-account-monitoring versions stored only the source product in
        // this table. It must not become a fake historical listing.
        sqlx::query("DELETE FROM peer_monitor_item_snapshots WHERE target_id = ?")
            .bind(target_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
    }

    let existing_items = sqlx::query("SELECT item_id, title, price, image_url, status, is_active FROM peer_monitor_item_snapshots WHERE target_id = ?")
        .bind(target_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    let mut previous_items = HashMap::new();
    for row in existing_items {
        previous_items.insert(
            text(&row, "item_id"),
            json!({
                "itemId": text(&row, "item_id"),
                "title": text(&row, "title"),
                "price": text(&row, "price"),
                "imageUrl": text(&row, "image_url"),
                "status": text(&row, "status"),
                "isActive": row.try_get::<i64, _>("is_active").unwrap_or(0) != 0,
            }),
        );
    }
    let mut current_item_ids = HashSet::new();
    for item in &snapshot.items {
        let item_value = monitor_item_value(item);
        let item_id = value_text(item_value.get("itemId"));
        if item_id.is_empty() || !current_item_ids.insert(item_id.clone()) {
            continue;
        }
        let previous = previous_items.get(&item_id).cloned();
        if has_baseline {
            match previous.as_ref() {
                None => {
                    insert_peer_monitor_event(
                        &mut tx,
                        target_id,
                        "item_listed",
                        &format!("上架了商品：{}", value_text(item_value.get("title"))),
                        "公开宝贝列表出现了该商品。",
                        "item",
                        &item_id,
                        &Value::Null,
                        &item_value,
                    )
                    .await?;
                }
                Some(previous)
                    if !previous
                        .get("isActive")
                        .and_then(Value::as_bool)
                        .unwrap_or(false) =>
                {
                    insert_peer_monitor_event(
                        &mut tx,
                        target_id,
                        "item_relisted",
                        &format!("重新上架了商品：{}", value_text(item_value.get("title"))),
                        "此前已下架的公开宝贝再次出现。",
                        "item",
                        &item_id,
                        previous,
                        &item_value,
                    )
                    .await?;
                }
                Some(previous)
                    if !value_text(previous.get("price")).is_empty()
                        && !value_text(item_value.get("price")).is_empty()
                        && value_text(previous.get("price"))
                            != value_text(item_value.get("price")) =>
                {
                    insert_peer_monitor_event(
                        &mut tx,
                        target_id,
                        "price_changed",
                        &format!("调整了价格：{}", value_text(item_value.get("title"))),
                        &format!(
                            "价格 {} → {}",
                            value_text(previous.get("price")),
                            value_text(item_value.get("price"))
                        ),
                        "item",
                        &item_id,
                        previous,
                        &item_value,
                    )
                    .await?;
                }
                _ => {}
            }
        }
        sqlx::query("INSERT INTO peer_monitor_item_snapshots (target_id, item_id, title, price, image_url, status, is_active, first_observed_at, last_observed_at, observed_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?, ?, ?) ON CONFLICT(target_id, item_id) DO UPDATE SET title = excluded.title, price = excluded.price, image_url = excluded.image_url, status = excluded.status, is_active = 1, first_observed_at = CASE WHEN peer_monitor_item_snapshots.first_observed_at > 0 THEN peer_monitor_item_snapshots.first_observed_at ELSE excluded.first_observed_at END, last_observed_at = excluded.last_observed_at, observed_at = excluded.observed_at")
            .bind(target_id)
            .bind(&item_id)
            .bind(value_text(item_value.get("title")))
            .bind(value_text(item_value.get("price")))
            .bind(value_text(item_value.get("imageUrl")))
            .bind(value_text(item_value.get("status")))
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
    }
    if has_baseline {
        for (item_id, previous) in &previous_items {
            if previous
                .get("isActive")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                && !current_item_ids.contains(item_id)
            {
                insert_peer_monitor_event(
                    &mut tx,
                    target_id,
                    "item_off_shelf",
                    &format!("下架了商品：{}", value_text(previous.get("title"))),
                    "该商品不再出现在完整的公开宝贝列表中。",
                    "item",
                    item_id,
                    previous,
                    &Value::Null,
                )
                .await?;
                sqlx::query("UPDATE peer_monitor_item_snapshots SET is_active = 0, status = 'off_shelf', last_observed_at = ?, observed_at = ? WHERE target_id = ? AND item_id = ?")
                    .bind(now)
                    .bind(now)
                    .bind(target_id)
                    .bind(item_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }

    if !profile_changes.is_empty() {
        insert_peer_monitor_event(
            &mut tx,
            target_id,
            "profile_changed",
            "同行主页资料有变化",
            &format!("已更新：{}。", profile_changes.join("、")),
            "profile",
            &text(target, "seller_id"),
            &previous_profile,
            &snapshot.profile,
        )
        .await?;
    }

    let mut timeline_count = 0;
    if let Some(timeline) = snapshot.timeline.as_ref() {
        let known_timeline_keys = sqlx::query_scalar::<_, String>(
            "SELECT event_key FROM peer_monitor_timeline_snapshots WHERE target_id = ?",
        )
        .bind(target_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashSet<_>>();
        timeline_count = timeline.len();
        for event in timeline {
            let key = timeline_event_key(event);
            if has_baseline && !known_timeline_keys.contains(&key) {
                let status = value_text(event.get("status"));
                insert_peer_monitor_event(
                    &mut tx,
                    target_id,
                    &format!("timeline_{status}"),
                    &format!("公开动态：{}", timeline_status_label(&status)),
                    &value_text(event.get("title")),
                    "timeline",
                    &key,
                    &Value::Null,
                    event,
                )
                .await?;
            }
            sqlx::query("INSERT INTO peer_monitor_timeline_snapshots (target_id, event_key, item_id, trade_id, event_type, event_date, title, price, image_url, first_observed_at, last_observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(target_id, event_key) DO UPDATE SET item_id = excluded.item_id, trade_id = excluded.trade_id, event_type = excluded.event_type, event_date = excluded.event_date, title = excluded.title, price = excluded.price, image_url = excluded.image_url, last_observed_at = excluded.last_observed_at")
                .bind(target_id)
                .bind(key)
                .bind(value_text(event.get("itemId")))
                .bind(value_text(event.get("tradeId")))
                .bind(value_text(event.get("status")))
                .bind(value_text(event.get("date")))
                .bind(value_text(event.get("title")))
                .bind(value_text(event.get("price")))
                .bind(value_text(event.get("picUrl")))
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|error| error.to_string())?;
        }
    }

    let mut rate_count = 0;
    if let Some(reviews) = snapshot.reviews.as_ref() {
        let known_rate_ids = sqlx::query_scalar::<_, String>(
            "SELECT rate_id FROM peer_monitor_rate_snapshots WHERE target_id = ?",
        )
        .bind(target_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .collect::<HashSet<_>>();
        rate_count = reviews.len();
        for review in reviews {
            let rate_id = value_text(review.get("id"));
            if rate_id.is_empty() {
                continue;
            }
            if has_baseline && !known_rate_ids.contains(&rate_id) {
                insert_peer_monitor_event(
                    &mut tx,
                    target_id,
                    "new_review",
                    "收到新的公开评价",
                    &first_text([
                        value_text(review.get("itemTitle")),
                        value_text(review.get("feedback")),
                    ]),
                    "review",
                    &rate_id,
                    &Value::Null,
                    review,
                )
                .await?;
            }
            sqlx::query("INSERT INTO peer_monitor_rate_snapshots (target_id, rate_id, trade_id, item_id, item_title, item_price, item_image_url, rater_nick, feedback, created_at_text, rate_json, first_observed_at, last_observed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(target_id, rate_id) DO UPDATE SET trade_id = excluded.trade_id, item_id = excluded.item_id, item_title = excluded.item_title, item_price = excluded.item_price, item_image_url = excluded.item_image_url, rater_nick = excluded.rater_nick, feedback = excluded.feedback, created_at_text = excluded.created_at_text, rate_json = excluded.rate_json, last_observed_at = excluded.last_observed_at")
                .bind(target_id)
                .bind(&rate_id)
                .bind(value_text(review.get("tradeId")))
                .bind(value_text(review.get("itemId")))
                .bind(value_text(review.get("itemTitle")))
                .bind(value_text(review.get("itemPrice")))
                .bind(value_text(review.get("itemPic")))
                .bind(value_text(review.get("raterNick")))
                .bind(value_text(review.get("feedback")))
                .bind(value_text(review.get("createdAt")))
                .bind(serde_json::to_string(review).unwrap_or_default())
                .bind(now)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|error| error.to_string())?;
        }
    }

    sqlx::query("INSERT INTO peer_monitor_profile_snapshots (target_id, profile_json, observed_at) VALUES (?, ?, ?)")
        .bind(target_id)
        .bind(&profile_json)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    if !has_baseline {
        sqlx::query("UPDATE peer_monitor_events SET title = '已建立账号监控基线', summary = '首次完整成功采集同行主页、公开宝贝、动态和评价；后续成功采集才会生成变化事件。', entity_type = 'account' WHERE target_id = ? AND event_type = 'baseline'")
            .bind(target_id)
            .execute(&mut *tx)
            .await
            .map_err(|error| error.to_string())?;
        ensure_baseline_event_in_tx(&mut tx, target_id).await?;
    }
    let partial = !snapshot.warnings.is_empty();
    let detail = if partial {
        format!(
            "已采集主页与 {} 件公开宝贝；{}",
            snapshot.items.len(),
            snapshot.warnings.join("；")
        )
    } else {
        format!(
            "已采集主页、{} 件公开宝贝、{} 条动态、{} 条评价。",
            snapshot.items.len(),
            timeline_count,
            rate_count
        )
    };
    sqlx::query("UPDATE peer_monitor_targets SET seller_name = ?, seller_avatar = ?, profile_json = ?, profile_observed_at = ?, last_refreshed_at = ?, last_success_at = ?, refresh_started_at = NULL, last_error = '', updated_at = ? WHERE id = ?")
        .bind(value_text(snapshot.profile.get("sellerNick")))
        .bind(value_text(snapshot.profile.get("sellerAvatar")))
        .bind(profile_json)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(target_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query("UPDATE peer_monitor_runs SET status = ?, detail = ?, finished_at = ?, profile_count = 1, item_count = ?, timeline_count = ?, rate_count = ? WHERE id = ?")
        .bind(if partial { "partial" } else { "success" })
        .bind(&detail)
        .bind(now)
        .bind(snapshot.items.len() as i64)
        .bind(timeline_count as i64)
        .bind(rate_count as i64)
        .bind(run_id)
        .execute(&mut *tx)
        .await
        .map_err(|error| error.to_string())?;
    tx.commit().await.map_err(|error| error.to_string())?;
    Ok(PeerMonitorRefreshOutcome {
        partial,
        item_count: snapshot.items.len(),
        timeline_count,
        rate_count,
        detail,
    })
}

async fn ensure_baseline_event_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    target_id: i64,
) -> Result<(), String> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM peer_monitor_events WHERE target_id = ? AND event_type = 'baseline'",
    )
    .bind(target_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| error.to_string())?;
    if exists == 0 {
        insert_peer_monitor_event(
            tx,
            target_id,
            "baseline",
            "已建立账号监控基线",
            "首次完整成功采集同行主页、公开宝贝、动态和评价；后续成功采集才会生成变化事件。",
            "account",
            "",
            &Value::Null,
            &Value::Null,
        )
        .await?;
    }
    Ok(())
}

fn public_refresh_failure_detail(_error: &str) -> &'static str {
    "同行公开数据采集失败，未写入不完整快照；请稍后重试。"
}

async fn refresh_peer_monitor_target(
    state: &AppState,
    target_id: i64,
    trigger: &str,
) -> Result<PeerMonitorRefreshOutcome, String> {
    let pool = state.storage().xianyu();
    let target = sqlx::query("SELECT * FROM peer_monitor_targets WHERE id = ?")
        .bind(target_id)
        .fetch_optional(pool)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "同行监控目标不存在".to_string())?;
    let now = unix_timestamp_seconds();
    let started = sqlx::query("UPDATE peer_monitor_targets SET refresh_started_at = ?, updated_at = ? WHERE id = ? AND (refresh_started_at IS NULL OR refresh_started_at < ?)")
        .bind(now)
        .bind(now)
        .bind(target_id)
        .bind(now - PEER_MONITOR_REFRESH_STALE_SECS)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    if started.rows_affected() == 0 {
        return Err("该同行正在采集，请稍后重新读取数据".to_string());
    }
    let run_id = sqlx::query("INSERT INTO peer_monitor_runs (target_id, status, detail, trigger, started_at) VALUES (?, 'running', '正在采集同行主页、公开宝贝、动态和评价。', ?, ?)")
        .bind(target_id)
        .bind(trigger)
        .bind(now)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?
        .last_insert_rowid();
    let seller_id = text(&target, "seller_id");
    let owner_username = text(&target, "owner_username");
    let review_source_uid =
        active_public_source_uids(state.storage().accounts(), Some(&owner_username), None)
            .await
            .ok()
            .and_then(|uids| uids.into_iter().next());
    let snapshot =
        collect_public_peer_snapshot(state, &seller_id, review_source_uid.as_deref()).await;
    match snapshot {
        Ok(snapshot) => persist_public_peer_snapshot(pool, &target, run_id, snapshot).await,
        Err(error) => {
            let detail = public_refresh_failure_detail(&error);
            let finished_at = unix_timestamp_seconds();
            sqlx::query("UPDATE peer_monitor_targets SET refresh_started_at = NULL, last_refreshed_at = ?, last_error = ?, updated_at = ? WHERE id = ?")
                .bind(finished_at)
                .bind(detail)
                .bind(finished_at)
                .bind(target_id)
                .execute(pool)
                .await
                .map_err(|update_error| update_error.to_string())?;
            sqlx::query("UPDATE peer_monitor_runs SET status = 'failed', detail = ?, finished_at = ? WHERE id = ?")
                .bind(detail)
                .bind(finished_at)
                .bind(run_id)
                .execute(pool)
                .await
                .map_err(|update_error| update_error.to_string())?;
            Err(error)
        }
    }
}

pub(crate) async fn refresh_due_peer_monitor_targets(
    state: &AppState,
) -> Result<PeerMonitorRefreshBatch, String> {
    let due_before = unix_timestamp_seconds() - PEER_MONITOR_REFRESH_INTERVAL_SECS;
    let target_ids = sqlx::query_scalar::<_, i64>("SELECT id FROM peer_monitor_targets WHERE enabled = 1 AND (refresh_started_at IS NULL OR refresh_started_at < ?) AND (last_refreshed_at IS NULL OR last_refreshed_at <= ?) ORDER BY COALESCE(last_refreshed_at, 0) ASC LIMIT ?")
        .bind(unix_timestamp_seconds() - PEER_MONITOR_REFRESH_STALE_SECS)
        .bind(due_before)
        .bind(PEER_MONITOR_WORKER_BATCH_SIZE)
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(|error| error.to_string())?;
    let mut batch = PeerMonitorRefreshBatch::default();
    for target_id in target_ids {
        batch.attempted += 1;
        match refresh_peer_monitor_target(state, target_id, "scheduled").await {
            Ok(outcome) if outcome.partial => batch.partial += 1,
            Ok(_) => batch.succeeded += 1,
            Err(_) => batch.failed += 1,
        }
    }
    Ok(batch)
}

async fn record_refresh(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(target_id): Path<i64>,
) -> ApiResult {
    let owner = owner(&state, &identity);
    let _ = load_target(&state, &owner, target_id).await?;
    let outcome = refresh_peer_monitor_target(&state, target_id, "manual")
        .await
        .map_err(|_| {
            ResponseError::service_unavailable(
                "同行公开数据采集失败，未写入不完整快照；请稍后重试。",
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "status": if outcome.partial { "partial" } else { "success" },
        "detail": outcome.detail,
        "itemCount": outcome.item_count,
        "timelineCount": outcome.timeline_count,
        "rateCount": outcome.rate_count,
    })))
}

async fn delete_target(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(target_id): Path<i64>,
) -> ApiResult {
    let owner = owner(&state, &identity);
    let result =
        sqlx::query("DELETE FROM peer_monitor_targets WHERE id = ? AND owner_username = ?")
            .bind(target_id)
            .bind(&owner)
            .execute(state.storage().xianyu())
            .await
            .map_err(ResponseError::internal)?;
    if result.rows_affected() == 0 {
        return Err(ResponseError::not_found("同行监控目标不存在"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::{ensure_baseline_event, load_target_from_pool};
    use sqlx::{sqlite::SqlitePoolOptions, Row, SqlitePool};
    use std::sync::{Arc, Mutex};

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory sqlite");
        sqlx::raw_sql(include_str!(
            "../../migrations/xianyu/0020_peer_monitor.sql"
        ))
        .execute(&pool)
        .await
        .expect("peer monitor schema");
        sqlx::raw_sql(include_str!(
            "../../migrations/xianyu/0022_peer_monitor_account_snapshots.sql"
        ))
        .execute(&pool)
        .await
        .expect("peer monitor account snapshot schema");
        pool
    }

    async fn setup_accounts_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("memory accounts sqlite");
        sqlx::query(
            "CREATE TABLE accounts (uid TEXT PRIMARY KEY, owner_username TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'active', last_session_ok_at REAL NOT NULL DEFAULT 0, updated_at REAL NOT NULL DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .expect("accounts schema");
        pool
    }

    async fn insert_account(
        pool: &SqlitePool,
        uid: &str,
        owner: &str,
        status: &str,
        last_session_ok_at: f64,
        updated_at: f64,
    ) {
        sqlx::query("INSERT INTO accounts (uid, owner_username, status, last_session_ok_at, updated_at) VALUES (?, ?, ?, ?, ?)")
            .bind(uid)
            .bind(owner)
            .bind(status)
            .bind(last_session_ok_at)
            .bind(updated_at)
            .execute(pool)
            .await
            .expect("insert account");
    }

    async fn insert_target(pool: &SqlitePool, owner: &str, seller_id: &str) -> i64 {
        sqlx::query("INSERT INTO peer_monitor_targets (owner_username, seller_id) VALUES (?, ?)")
            .bind(owner)
            .bind(seller_id)
            .execute(pool)
            .await
            .expect("insert target")
            .last_insert_rowid()
    }

    async fn insert_running_monitor_run(pool: &SqlitePool, target_id: i64) -> i64 {
        sqlx::query("INSERT INTO peer_monitor_runs (target_id, status, detail, trigger, started_at) VALUES (?, 'running', '', 'test', 1)")
            .bind(target_id)
            .execute(pool)
            .await
            .expect("insert running monitor run")
            .last_insert_rowid()
    }

    #[tokio::test]
    async fn targets_are_owner_scoped_and_deduplicated_per_owner() {
        let pool = setup_pool().await;
        let alice_target = insert_target(&pool, "alice", "seller-1").await;
        let _bob_target = insert_target(&pool, "bob", "seller-1").await;

        sqlx::query("INSERT INTO peer_monitor_targets (owner_username, seller_id) VALUES (?, ?) ON CONFLICT(owner_username, seller_id) DO UPDATE SET updated_at = unixepoch('subsec')")
            .bind("alice")
            .bind("seller-1")
            .execute(&pool)
            .await
            .expect("deduplicate target");

        let alice_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM peer_monitor_targets WHERE owner_username = ?",
        )
        .bind("alice")
        .fetch_one(&pool)
        .await
        .expect("alice count");
        assert_eq!(alice_count, 1);
        assert!(load_target_from_pool(&pool, "alice", alice_target)
            .await
            .is_ok());
        assert!(load_target_from_pool(&pool, "bob", alice_target)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn baseline_is_created_once_and_pause_state_is_persistent() {
        let pool = setup_pool().await;
        let target_id = insert_target(&pool, "alice", "seller-1").await;

        assert!(ensure_baseline_event(&pool, target_id)
            .await
            .expect("first baseline"));
        assert!(!ensure_baseline_event(&pool, target_id)
            .await
            .expect("second baseline"));
        let baseline_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM peer_monitor_events WHERE target_id = ? AND event_type = 'baseline'")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .expect("baseline count");
        assert_eq!(baseline_count, 1);

        sqlx::query(
            "UPDATE peer_monitor_targets SET enabled = 0 WHERE id = ? AND owner_username = ?",
        )
        .bind(target_id)
        .bind("alice")
        .execute(&pool)
        .await
        .expect("pause target");
        let row = load_target_from_pool(&pool, "alice", target_id)
            .await
            .expect("paused target");
        assert_eq!(row.get::<i64, _>("enabled"), 0);

        sqlx::query(
            "UPDATE peer_monitor_targets SET enabled = 1 WHERE id = ? AND owner_username = ?",
        )
        .bind(target_id)
        .bind("alice")
        .execute(&pool)
        .await
        .expect("resume target");
        let row = load_target_from_pool(&pool, "alice", target_id)
            .await
            .expect("resumed target");
        assert_eq!(row.get::<i64, _>("enabled"), 1);
    }

    #[tokio::test]
    async fn public_source_accounts_are_owner_scoped_and_prefer_the_requested_active_uid() {
        let pool = setup_accounts_pool().await;
        insert_account(&pool, "alice-new", "alice", "active", 30.0, 30.0).await;
        insert_account(&pool, "alice-requested", "alice", "active", 10.0, 10.0).await;
        insert_account(&pool, "alice-expired", "alice", "expired", 99.0, 99.0).await;
        insert_account(&pool, "bob-new", "bob", "active", 100.0, 100.0).await;

        let uids = super::active_public_source_uids(&pool, Some("alice"), Some("alice-requested"))
            .await
            .expect("owner sources");

        assert_eq!(uids, vec!["alice-requested", "alice-new"]);
    }

    #[tokio::test]
    async fn public_source_accounts_use_recent_success_order_without_a_preference() {
        let pool = setup_accounts_pool().await;
        insert_account(&pool, "older", "alice", "active", 10.0, 100.0).await;
        insert_account(&pool, "newer", "alice", "active", 20.0, 20.0).await;

        let uids = super::active_public_source_uids(&pool, Some("alice"), None)
            .await
            .expect("ordered sources");

        assert_eq!(uids, vec!["newer", "older"]);
    }

    #[tokio::test]
    async fn source_attempts_continue_after_failure_and_stop_at_first_success() {
        let attempted = Arc::new(Mutex::new(Vec::new()));
        let result = super::first_successful_source(
            &[
                "stale".to_string(),
                "working".to_string(),
                "unused".to_string(),
            ],
            {
                let attempted = Arc::clone(&attempted);
                move |uid| {
                    let attempted = Arc::clone(&attempted);
                    async move {
                        attempted.lock().expect("attempt log").push(uid.clone());
                        (uid == "working").then_some(uid)
                    }
                }
            },
        )
        .await;

        assert_eq!(result.as_deref(), Some("working"));
        assert_eq!(
            attempted.lock().expect("attempt log").as_slice(),
            ["stale", "working"]
        );
    }

    #[tokio::test]
    async fn complete_account_snapshots_build_a_baseline_then_emit_real_diffs() {
        let pool = setup_pool().await;
        let target_id = insert_target(&pool, "alice", "1041111931").await;
        let target = sqlx::query("SELECT * FROM peer_monitor_targets WHERE id = ?")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .expect("target row");
        let first_run = insert_running_monitor_run(&pool, target_id).await;
        let first = super::PublicPeerSnapshot {
            profile: serde_json::json!({
                "sellerId": "1041111931",
                "sellerNick": "同行甲",
                "sellerAvatar": "https://example.invalid/avatar.jpg",
                "followersCount": "10",
                "itemCount": 1
            }),
            items: vec![serde_json::json!({
                "itemId": "item-1",
                "title": "首轮商品",
                "price": "10",
                "picUrl": "https://example.invalid/item-1.jpg",
                "status": "on_sale"
            })],
            timeline: Some(vec![serde_json::json!({
                "status": "listed",
                "tradeId": "item-1",
                "itemId": "item-1",
                "date": "20260731",
                "title": "首轮商品",
                "price": "10"
            })]),
            reviews: Some(vec![serde_json::json!({
                "id": "rate-1",
                "feedback": "首轮评价",
                "raterNick": "买家甲"
            })]),
            warnings: Vec::new(),
        };

        let first_outcome = super::persist_public_peer_snapshot(&pool, &target, first_run, first)
            .await
            .expect("persist first snapshot");
        assert!(!first_outcome.partial);
        let first_changes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM peer_monitor_events WHERE target_id = ? AND event_type != 'baseline'")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .expect("first changes");
        assert_eq!(
            first_changes, 0,
            "the first complete snapshot is baseline only"
        );

        let updated_target = sqlx::query("SELECT * FROM peer_monitor_targets WHERE id = ?")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .expect("updated target row");
        let second_run = insert_running_monitor_run(&pool, target_id).await;
        let second = super::PublicPeerSnapshot {
            profile: serde_json::json!({
                "sellerId": "1041111931",
                "sellerNick": "同行甲",
                "sellerAvatar": "https://example.invalid/avatar.jpg",
                "followersCount": "12",
                "itemCount": 2
            }),
            items: vec![
                serde_json::json!({
                    "itemId": "item-1",
                    "title": "首轮商品",
                    "price": "12",
                    "picUrl": "https://example.invalid/item-1.jpg",
                    "status": "on_sale"
                }),
                serde_json::json!({
                    "itemId": "item-2",
                    "title": "新增商品",
                    "price": "20",
                    "picUrl": "https://example.invalid/item-2.jpg",
                    "status": "on_sale"
                }),
            ],
            timeline: Some(vec![
                serde_json::json!({
                    "status": "listed",
                    "tradeId": "item-1",
                    "itemId": "item-1",
                    "date": "20260731",
                    "title": "首轮商品",
                    "price": "10"
                }),
                serde_json::json!({
                    "status": "sold",
                    "tradeId": "trade-2",
                    "itemId": "item-2",
                    "date": "20260801",
                    "title": "新增商品",
                    "price": "20"
                }),
            ]),
            reviews: Some(vec![
                serde_json::json!({ "id": "rate-1", "feedback": "首轮评价", "raterNick": "买家甲" }),
                serde_json::json!({ "id": "rate-2", "feedback": "新增评价", "raterNick": "买家乙", "itemTitle": "新增商品" }),
            ]),
            warnings: Vec::new(),
        };

        super::persist_public_peer_snapshot(&pool, &updated_target, second_run, second)
            .await
            .expect("persist second snapshot");
        let event_types = sqlx::query_scalar::<_, String>(
            "SELECT event_type FROM peer_monitor_events WHERE target_id = ?",
        )
        .bind(target_id)
        .fetch_all(&pool)
        .await
        .expect("event types");
        assert!(event_types.contains(&"profile_changed".to_string()));
        assert!(event_types.contains(&"price_changed".to_string()));
        assert!(event_types.contains(&"item_listed".to_string()));
        assert!(event_types.contains(&"timeline_sold".to_string()));
        assert!(event_types.contains(&"new_review".to_string()));
        let active_items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM peer_monitor_item_snapshots WHERE target_id = ? AND is_active = 1")
            .bind(target_id)
            .fetch_one(&pool)
            .await
            .expect("active item count");
        assert_eq!(active_items, 2);
    }

    #[test]
    fn detail_fields_accept_numeric_ids_and_encoded_data() {
        let data = serde_json::json!({
            "itemDO": { "userId": 17295534829_u64, "title": "同行商品" }
        });
        assert_eq!(
            super::find_text(&data, &["sellerId", "userId"]),
            "17295534829"
        );

        let encoded = serde_json::json!(r#"{"sellerId":17295534829,"sellerNick":"同行卖家"}"#);
        assert_eq!(super::find_text(&encoded, &["sellerId"]), "17295534829");
    }

    #[test]
    fn detail_fields_skip_platform_placeholder_values() {
        let data = serde_json::json!({
            "sellerId": 0,
            "userNick": "0",
            "itemDO": {
                "sellerId": "actual-seller-id",
                "userNick": "真实卖家"
            }
        });

        assert_eq!(super::find_text(&data, &["sellerId"]), "actual-seller-id");
        assert_eq!(super::find_text(&data, &["userNick"]), "真实卖家");
    }

    #[test]
    fn detail_fields_do_not_accept_requesting_account_as_a_peer() {
        assert!(super::detail_points_to_requester(
            "2215560740704",
            "2215560740704"
        ));
        assert!(!super::detail_points_to_requester(
            "other-seller",
            "2215560740704"
        ));
    }

    #[test]
    fn search_candidate_preserves_public_seller_identity() {
        let source_item = serde_json::json!({
            "itemId": "1046497410748",
            "sellerId": "5OGumB+NAwjMj+Ef/KUtcw==",
            "sellerNick": "小柚资源库",
            "title": "TXT 小说合集",
            "price": "0.10",
            "area": "广东"
        });

        let candidate =
            super::search_candidate("1046497410748", &source_item).expect("search candidate");
        assert_eq!(candidate["sellerId"], "5OGumB+NAwjMj+Ef/KUtcw==");
        assert_eq!(candidate["sellerNick"], "小柚资源库");
        assert_eq!(candidate["area"], "广东");
    }

    #[test]
    fn public_detail_uses_numeric_seller_id_for_profile() {
        let detail = serde_json::json!({
            "sellerDO": { "sellerId": 1041111931_u64 },
            "baseInfo": { "kcUserId": "ignored-fallback" }
        });

        assert_eq!(super::public_seller_id(&detail), "1041111931");
    }

    #[test]
    fn wx_detail_enhancement_keeps_h5_identity_and_adds_only_real_seller_metrics() {
        let mut candidate = serde_json::json!({
            "sellerId": "1041111931",
            "sellerNick": "H5 昵称",
            "sellerAvatar": "https://example.invalid/h5-avatar.jpg",
            "praiseRatio": "98%"
        });
        let wx_data = serde_json::json!({
            "itemDO": { "itemId": "1046497410748" },
            "sellerDO": {
                "sellerId": "1041111931",
                "nick": "微信昵称不应覆盖",
                "portraitUrl": "https://example.invalid/wx-avatar.jpg",
                "replyRatio24h": "99%",
                "replyInterval": "1分钟",
                "userRegDay": 3018,
                "hasSoldNumInteger": 12063,
                "newGoodRatioRate": "99%",
                "registerTime": 1524623403000_i64,
                "zhimaAuth": true,
                "zhimaLevelInfo": { "levelName": "信用极好" },
                "identityTags": [{ "text": "芝麻信用已授权" }],
                "sellerInfoTags": [{ "text": "小说收藏爱好者" }]
            }
        });

        assert!(super::wx_detail_matches_public_item(
            &wx_data,
            "1046497410748",
            "1041111931"
        ));
        super::merge_wx_seller_enhancement(&mut candidate, &wx_data);

        assert_eq!(candidate["sellerNick"], "H5 昵称");
        assert_eq!(
            candidate["sellerAvatar"],
            "https://example.invalid/h5-avatar.jpg"
        );
        assert_eq!(candidate["praiseRatio"], "98%");
        assert_eq!(candidate["replyRatio24h"], "99%");
        assert_eq!(candidate["replyInterval"], "1分钟");
        assert_eq!(candidate["registeredDays"], "3018");
        assert_eq!(candidate["soldCount"], "12063");
        assert_eq!(candidate["creditLevel"], "信用极好");
        assert_eq!(candidate["verificationTags"][0], "芝麻信用已授权");
        assert_eq!(candidate["sellerTags"][0], "小说收藏爱好者");
        assert_eq!(candidate["detailEnhancementSource"], "wx_mini_program");
    }

    #[test]
    fn wx_detail_enhancement_rejects_a_different_item_or_numeric_seller() {
        assert!(!super::wx_detail_matches_public_item(
            &serde_json::json!({
                "itemDO": { "itemId": "other-item" },
                "sellerDO": { "sellerId": "1041111931" }
            }),
            "1046497410748",
            "1041111931"
        ));
        assert!(!super::wx_detail_matches_public_item(
            &serde_json::json!({
                "itemDO": { "itemId": "1046497410748" },
                "sellerDO": { "sellerId": "9999999999" }
            }),
            "1046497410748",
            "1041111931"
        ));
    }

    #[test]
    fn peer_link_parser_distinguishes_item_and_seller_links() {
        let seller = super::parse_peer_link_target(crate::sealed_protocol::sealed_str!(
            "https://www.goofish.com/personal?userId=1041111931"
        ))
        .expect("seller link parses");
        assert!(matches!(seller, super::PeerLinkTarget::Seller(id) if id == "1041111931"));

        let item =
            super::parse_peer_link_target("fleamarket://awesome_detail?itemId=1046497410748")
                .expect("item link parses");
        assert!(matches!(item, super::PeerLinkTarget::Item(id) if id == "1046497410748"));

        let shared_home = super::parse_peer_link_target(crate::sealed_protocol::sealed_str!(
            "https://pages.goofish.com/sharexy?userid=2222142708100&bft=personal"
        ))
        .expect("shared seller page parses");
        assert!(matches!(shared_home, super::PeerLinkTarget::Seller(id) if id == "2222142708100"));

        assert!(
            super::parse_peer_link_target("https://example.com/item?id=1046497410748").is_none()
        );
    }

    #[test]
    fn public_review_cards_keep_real_feedback_and_item_fields_when_exposed() {
        let data = serde_json::json!({
            "rateTabDOList": [{ "tabType": 1, "tabName": "好评", "tabCount": 12 }],
            "cardList": [{ "cardData": {
                "rateId": 42,
                "rate": 1,
                "feedback": "发货很快，推荐购买",
                "gmtCreateStr": "2026-07-30",
                "raterUserNick": "买家A",
                "raterHeadImg": "https://example.invalid/avatar.jpg",
                "rateTagList": [{ "text": "买家" }],
                "pictCdnUrlList": ["https://example.invalid/review.jpg"],
                "itemId": 1001,
                "itemTitle": "真实商品",
                "itemPrice": "12.00",
                "tradeId": "trade/+1=",
                "ratedUid": "rated/+2=",
                "bizCode": "order",
                "appendRateList": [{ "desc": "7天后追评", "feedback": "依然很好", "picUrls": [] }],
                "replyList": [{ "feedback": "谢谢支持" }]
            }}]
        });

        let reviews = super::public_review_cards(&data);
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0]["feedback"], "发货很快，推荐购买");
        assert_eq!(reviews[0]["itemId"], "1001");
        assert_eq!(reviews[0]["itemTitle"], "真实商品");
        assert_eq!(reviews[0]["tradeId"], "trade/+1=");
        assert_eq!(reviews[0]["ratedUid"], "rated/+2=");
        assert_eq!(reviews[0]["bizCode"], "order");
        assert_eq!(reviews[0]["appendReview"]["feedback"], "依然很好");
        assert_eq!(reviews[0]["reply"]["feedback"], "谢谢支持");
        assert_eq!(super::public_review_tabs(&data)[0]["label"], "好评");
    }

    #[test]
    fn item_rate_cards_are_bound_to_the_requested_product() {
        let data = serde_json::json!({
            "idleRateServeDTOList": [{ "idleRateServeDTO": {
                "orderId": "order-1",
                "rateId": "rate-2",
                "feedback": "包装仔细，正品",
                "gmtCreateStr": "2026-07-30",
                "itemId": "818580654762"
            }}]
        });
        let item = serde_json::json!({
            "itemId": "818580654762",
            "title": "MacBook Pro 14 寸保护壳",
            "price": "39.00",
            "picUrl": "https://example.invalid/item.jpg"
        });

        let reviews = super::public_item_rate_cards(&data, &item);
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0]["orderId"], "order-1");
        assert_eq!(reviews[0]["itemId"], "818580654762");
        assert_eq!(reviews[0]["itemTitle"], "MacBook Pro 14 寸保护壳");
        assert_eq!(reviews[0]["itemPrice"], "39.00");
        assert_eq!(reviews[0]["itemPic"], "https://example.invalid/item.jpg");
        assert_eq!(reviews[0]["source"], "item_rate");
    }

    #[test]
    fn item_rate_cards_parse_the_captured_result_shape_inline() {
        let data = serde_json::json!({
            "result": [{
                "content": "非常快，很专业很满意",
                "gmtCreateStr": "1天前购买服务",
                "raterNick": "小***",
                "raterHeadImg": "https://example.invalid/buyer.jpg",
                "images": [{ "url": "https://example.invalid/review.jpg" }],
                "idleRateServeDTO": {
                    "orderId": "3306272991674005751",
                    "rateId": "40042572708",
                    "itemId": "1054338118413"
                }
            }]
        });
        let item = serde_json::json!({
            "itemId": "1054338118413",
            "title": "macOS 服务商品",
            "price": "88.00",
            "picUrl": "https://example.invalid/item.jpg"
        });

        let reviews = super::public_item_rate_cards(&data, &item);
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0]["feedback"], "非常快，很专业很满意");
        assert_eq!(reviews[0]["raterNick"], "小***");
        assert_eq!(reviews[0]["createdAt"], "1天前购买服务");
        assert_eq!(reviews[0]["itemTitle"], "macOS 服务商品");
        assert_eq!(
            reviews[0]["pictures"][0],
            "https://example.invalid/review.jpg"
        );
    }

    #[test]
    fn item_rate_cards_deduplicate_by_order_before_rate_id() {
        let reviews = vec![
            serde_json::json!({ "id": "rate-1", "orderId": "order-1" }),
            serde_json::json!({ "id": "rate-2", "orderId": "order-1" }),
            serde_json::json!({ "id": "rate-3", "orderId": "" }),
        ];

        let reviews = super::deduplicate_item_reviews(reviews);
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0]["id"], "rate-1");
        assert_eq!(reviews[1]["id"], "rate-3");
    }

    #[test]
    fn item_rate_cursor_uses_the_small_program_last_cursor() {
        let payload = serde_json::json!({ "lastCursor": "next-rate-page" });
        assert_eq!(
            super::public_item_rate_next_cursor(&payload),
            "next-rate-page"
        );
    }

    #[test]
    fn item_rate_payload_requires_a_recognizable_review_list_before_skipping_h5_fallback() {
        assert!(!super::public_item_rate_payload_has_list(
            &serde_json::json!({
                "data": {}
            })
        ));
        assert!(super::public_item_rate_payload_has_list(
            &serde_json::json!({
                "data": { "result": [] }
            })
        ));
        assert!(super::public_item_rate_payload_has_list(
            &serde_json::json!({
                "data": { "idleRateServeDTOList": [] }
            })
        ));
    }

    #[test]
    fn item_rate_transaction_only_key_matches_the_small_program_order_key() {
        let seller_reviews = vec![serde_json::json!({
            "id": "",
            "tradeId": "order-9",
            "itemId": ""
        })];
        let unresolved = super::unresolved_review_match_keys(&seller_reviews);
        let item_reviews = vec![serde_json::json!({
            "id": "rate-9",
            "orderId": "order-9",
            "itemId": "9001",
            "itemTitle": "交易号对应商品"
        })];

        assert!(super::unresolved_review_match_keys_have_item(
            &unresolved,
            &item_reviews
        ));
    }

    #[test]
    fn item_rate_attribution_only_finishes_when_each_missing_review_has_a_real_key_match() {
        let seller_reviews = vec![
            serde_json::json!({ "id": "rate-1", "tradeId": "order-1", "itemId": "" }),
            serde_json::json!({ "id": "rate-2", "tradeId": "order-2", "itemId": "" }),
        ];
        let unresolved = super::unresolved_review_match_keys(&seller_reviews);
        assert!(unresolved.contains("rate:rate-1"));
        assert!(unresolved.contains("trade:order-2"));

        let one_item_review = vec![serde_json::json!({
            "id": "rate-1",
            "orderId": "order-1",
            "itemId": "1001",
            "itemTitle": "真实商品一"
        })];
        assert!(!super::unresolved_review_match_keys_have_item(
            &unresolved,
            &one_item_review
        ));

        let item_reviews = vec![
            one_item_review[0].clone(),
            serde_json::json!({
                "id": "rate-2",
                "orderId": "order-2",
                "itemId": "1002",
                "itemTitle": "真实商品二"
            }),
        ];
        assert!(super::unresolved_review_match_keys_have_item(
            &unresolved,
            &item_reviews
        ));
    }

    #[test]
    fn seller_reviews_keep_their_order_and_gain_only_verified_item_fields() {
        let seller_reviews = vec![
            serde_json::json!({
                "id": "rate-1",
                "feedback": "第一条账号评价",
                "raterNick": "买家甲",
                "createdAt": "2026-07-30",
                "itemId": "",
                "itemTitle": ""
            }),
            serde_json::json!({
                "id": "rate-2",
                "feedback": "第二条账号评价",
                "raterNick": "买家乙",
                "createdAt": "2026-07-29",
                "itemId": "",
                "itemTitle": ""
            }),
        ];
        let item_reviews = vec![serde_json::json!({
            "id": "rate-2",
            "orderId": "order-2",
            "itemId": "1054338118413",
            "itemTitle": "真实关联商品",
            "itemPrice": "88.00",
            "itemPic": "https://example.invalid/item.jpg"
        })];

        let reviews = super::enrich_public_reviews_with_item_rates(seller_reviews, &item_reviews);

        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0]["feedback"], "第一条账号评价");
        assert_eq!(reviews[0]["itemTitle"], "");
        assert_eq!(reviews[1]["feedback"], "第二条账号评价");
        assert_eq!(reviews[1]["itemId"], "1054338118413");
        assert_eq!(reviews[1]["itemTitle"], "真实关联商品");
        assert_eq!(reviews[1]["itemPrice"], "88.00");
    }

    #[test]
    fn seller_reviews_can_match_a_verified_item_by_trade_order_id() {
        let seller_reviews = vec![serde_json::json!({
            "id": "different-rate-id",
            "tradeId": "order-9",
            "feedback": "按交易号匹配"
        })];
        let item_reviews = vec![serde_json::json!({
            "id": "item-rate-id",
            "orderId": "order-9",
            "itemId": "9001",
            "itemTitle": "交易号对应商品"
        })];

        let reviews = super::enrich_public_reviews_with_item_rates(seller_reviews, &item_reviews);

        assert_eq!(reviews[0]["itemId"], "9001");
        assert_eq!(reviews[0]["itemTitle"], "交易号对应商品");
    }

    #[test]
    fn public_item_cards_parse_the_public_item_list() {
        let data = serde_json::json!({
            "cardList": [{
                "cardData": {
                    "id": "1046497410748",
                    "title": "超17万本 TXT 小说合集",
                    "priceInfo": { "price": "0.10" },
                    "picInfo": { "picUrl": "https://example.invalid/item.jpg" },
                    "itemStatus": 0
                }
            }]
        });

        let cards = super::public_item_cards(&data);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["itemId"], "1046497410748");
        assert_eq!(cards[0]["price"], "0.10");
    }

    #[test]
    fn public_timeline_cards_parse_sold_item_events() {
        let data = serde_json::json!({
            "cardList": [{
                "cardType": "dayNode",
                "ds": "20260729",
                "secondaryDesc": "7月",
                "items": [{ "items": [{
                    "cardType": "soldItem",
                    "tradeId": "trade-1",
                    "itemId": "item-1",
                    "title": "已售商品",
                    "price": "12.00",
                    "mainPic": "https://example.invalid/sold.jpg"
                }]}]
            }]
        });

        let cards = super::public_timeline_cards(&data);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0]["tradeId"], "trade-1");
        assert_eq!(cards[0]["date"], "20260729");
        assert_eq!(cards[0]["price"], "12.00");
        assert_eq!(cards[0]["status"], "sold");
        assert_eq!(cards[0]["statusLabel"], "售出");
    }

    #[test]
    fn public_timeline_cards_keep_real_listing_and_off_shelf_events() {
        let data = serde_json::json!({
            "cardList": [{
                "cardType": "dayNode",
                "ds": "20260730",
                "items": [{ "items": [
                    {
                        "cardType": "onShelfItem",
                        "itemId": "listed-1",
                        "title": "刚上架的商品",
                        "price": "88.00",
                        "mainPic": "https://example.invalid/listed.jpg"
                    },
                    {
                        "cardType": "offShelfItem",
                        "itemId": "off-1",
                        "title": "刚下架的商品",
                        "price": "66.00",
                        "mainPic": "https://example.invalid/off.jpg"
                    },
                    {
                        "cardType": "textNotice",
                        "desc": "平台活动通知"
                    }
                ] }]
            }]
        });

        let cards = super::public_timeline_cards(&data);

        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0]["status"], "listed");
        assert_eq!(cards[0]["statusLabel"], "上架");
        assert_eq!(cards[1]["status"], "off_shelf");
        assert_eq!(cards[1]["statusLabel"], "下架");
    }

    #[test]
    fn timeline_visibility_respects_the_seller_privacy_setting() {
        assert_eq!(
            super::timeline_visibility(&serde_json::json!({ "privacyDays": -1 })),
            "hidden"
        );
        assert_eq!(
            super::timeline_visibility(&serde_json::json!({ "privacyDays": 30 })),
            "recent_only"
        );
        assert_eq!(
            super::timeline_visibility(&serde_json::json!({ "privacyDays": 0 })),
            "visible"
        );
    }

    #[test]
    fn public_profile_reads_seller_credit_tag() {
        let profile = serde_json::json!({
            "module": {
                "base": {
                    "ylzTags": [
                        { "attributes": { "role": "buyer" }, "text": "买家信用良好" },
                        { "attributes": { "role": "seller" }, "text": "卖家信用极好" }
                    ]
                }
            }
        });

        assert_eq!(super::seller_level(&profile), "卖家信用极好");
    }

    #[test]
    fn public_profile_maps_the_real_home_header_fields() {
        let profile = serde_json::json!({
            "module": {
                "nav": { "summary": "杭州。", "postCount": 22 },
                "social": { "followers": "165", "following": "12" },
                "tabs": {
                    "item": { "number": 202 },
                    "rate": { "number": "1975" },
                    "action": { "number": "90+" }
                },
                "quality": { "ipRegionAddress": "浙江省", "residence": "杭州" },
                "background": {
                    "bgSetting": "self",
                    "image": { "data": { "url": "https://img.example.test/profile.jpg" } },
                    "bgUrl": "https://img.example.test/default.jpg"
                },
                "identity": { "shopDesc": "macOS 开发者" },
                "introduction": { "introduction": "一直在做值得长期使用的工具。" },
                "impression": {
                    "items": [
                        { "text": "爽快大气" },
                        { "text": "喜欢游戏" }
                    ]
                },
                "platform": {
                    "ylzTags": [{ "attributes": { "role": "seller" }, "text": "卖家信用极好" }]
                },
                "base": {
                    "displayName": "MacApp_SenZ",
                    "avatar": { "avatar": "https://img.example.test/avatar.jpg" },
                    "visitTime": "12小时前来过",
                    "shop": { "level": "L7", "score": 233, "praiseRatio": 99, "reviewNum": 1936 }
                }
            }
        });
        let mut candidate = serde_json::json!({});

        super::merge_public_profile_head(&mut candidate, &profile, &serde_json::Value::Null);

        assert_eq!(candidate["sellerNick"], "MacApp_SenZ");
        assert_eq!(
            candidate["sellerAvatar"],
            "https://img.example.test/avatar.jpg"
        );
        assert_eq!(
            candidate["profileBackgroundUrl"],
            "https://img.example.test/profile.jpg"
        );
        assert_eq!(candidate["sellerSummary"], "一直在做值得长期使用的工具。");
        assert_eq!(candidate["followersCount"], "165");
        assert_eq!(candidate["followingCount"], "12");
        assert_eq!(candidate["dynamicCount"], "90+");
        assert_eq!(candidate["area"], "浙江省");
        assert_eq!(candidate["shopLevel"], "L7");
        assert_eq!(candidate["sellerLevel"], "卖家信用极好");
        assert_eq!(
            candidate["profileTags"],
            serde_json::json!(["macOS 开发者", "爽快大气", "喜欢游戏"])
        );
    }

    #[test]
    fn public_profile_reads_h5_base_introduction_and_platform_background() {
        let profile = serde_json::json!({
            "module": {
                "base": {
                    "displayName": "MacApp_SenZ",
                    "introduction": "一直在被模仿请大家注意甄别，别上当受骗。"
                },
                "background": {
                    "bgSetting": "default",
                    "bgUrl": "https://img.example.test/platform.jpg",
                    "default": { "bgUrl": "https://img.example.test/platform-default.jpg" }
                }
            }
        });
        let mut candidate = serde_json::json!({});

        super::merge_public_profile_head(&mut candidate, &profile, &serde_json::Value::Null);

        assert_eq!(
            candidate["sellerSummary"],
            "一直在被模仿请大家注意甄别，别上当受骗。"
        );
        assert_eq!(
            candidate["profileBackgroundUrl"],
            "https://img.example.test/platform.jpg"
        );
    }

    #[test]
    fn public_profile_background_falls_back_to_asset_then_default() {
        let profile = serde_json::json!({
            "module": {
                "background": {
                    "asset": { "bgUrl": "https://img.example.test/asset.jpg" },
                    "default": { "bgUrl": "https://img.example.test/default.jpg" }
                }
            }
        });
        let mut candidate = serde_json::json!({});

        super::merge_public_profile_head(&mut candidate, &profile, &serde_json::Value::Null);

        assert_eq!(
            candidate["profileBackgroundUrl"],
            "https://img.example.test/asset.jpg"
        );

        let default_only_profile = serde_json::json!({
            "module": {
                "background": {
                    "default": { "bgUrl": "https://img.example.test/default-only.jpg" }
                }
            }
        });
        let mut default_only_candidate = serde_json::json!({});
        super::merge_public_profile_head(
            &mut default_only_candidate,
            &default_only_profile,
            &serde_json::Value::Null,
        );

        assert_eq!(
            default_only_candidate["profileBackgroundUrl"],
            "https://img.example.test/default-only.jpg"
        );
    }
}
