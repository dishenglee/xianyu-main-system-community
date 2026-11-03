use axum::{
    extract::{Extension, Json, State},
    Json as ResponseJson,
};
use std::cmp::Ordering;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::{Row, SqlitePool};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::{
    auth_expired_message, ensure_market_uid_access, load_location_or_default, resolve_no_bargain,
    successful_upstream_h5_request, unix_seconds_now, value_is_trueish, value_to_string_lossy,
    value_to_string_ref, SyncOutcome,
};

// ---------------------------------------------------------------------------
// Struct
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(super) struct SyncPayload {
    pub(super) uid: String,
    #[serde(default = "default_seller_true")]
    pub(super) seller: bool,
}

fn default_seller_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Parse helpers
// ---------------------------------------------------------------------------

pub(super) fn parse_sold_out_items_from_upstream(raw: &Value) -> Vec<Value> {
    raw.get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("cardList"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|card| {
            let card_data = card.get("cardData").and_then(Value::as_object)?.clone();
            let attrs = card_data
                .get("attributesMap")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            // SOLD_OUT 商品的"下架原因 / 售出状态"字段：闲鱼上游字段名不固定，
            // 把所有候选都尝试取一遍，前端按非空优先级展示。常见取值：
            //   itemStatus = SOLD_OUT / OFF_SHELF / 已售出 / 主动下架 / 违规下架...
            //   offReason / pulloffReason / soldStatusText / saleStatusText
            let off_reason_explicit = value_to_string_lossy(card_data.get("offReason")
                .or_else(|| card_data.get("pulloffReason"))
                .or_else(|| card_data.get("offShelfReason"))
                .or_else(|| card_data.get("soldStatusText"))
                .or_else(|| card_data.get("saleStatusText"))
                .or_else(|| card_data.get("statusText"))
                .or_else(|| card_data.get("reason"))
                .or_else(|| card_data.get("subTitle"))
                .or_else(|| attrs.get("offReason"))
                .or_else(|| attrs.get("pulloffReason"))
                .or_else(|| attrs.get("offShelfReason"))
                .or_else(|| attrs.get("statusText")));
            // 基于真实抓包字段判断下架原因。闲鱼 SOLD_OUT 接口 cardData 实测下发：
            //   headTitle:    "小二下架" / "主动下架" / "已售" / "停售" 等闲鱼标准文案（首选）
            //   headIconUrl:  闲鱼自带的状态图标 URL
            //   itemStatus:   -2=主动下架 / -3=小二下架 / 0=正常（不是已售标识！）
            //   instockByXiaoer = true → 小二违规下架
            //   deleteByXiaoer  = true → 小二删除
            //   itemDeleted     = true → 已删除
            //   attrs.recentSoldTime 存在（毫秒时间戳）→ 真有过成交记录（=已售判定）
            //   outStockTime: 下架时间（字符串 "2026-07-15 18:02:43"）
            //   firstModifiedDiff: 最近修改的相对时间（"10天前"）
            //   attrs.gmtOffShelf / gmtShelf: 上下架精确时间戳
            //   canPolish: 是否还能擦亮（违规下架的也可能能擦）
            //   rePublishText / rePublishUrl: 重新上架按钮文案+跳转
            let head_title = value_to_string_lossy(card_data.get("headTitle"));
            let head_icon_url = value_to_string_lossy(card_data.get("headIconUrl"));
            let item_status = value_to_string_lossy(card_data.get("itemStatus")
                .or_else(|| attrs.get("itemStatus")));
            let instock_by_xiaoer = card_data.get("instockByXiaoer").map(value_is_trueish).unwrap_or(false);
            let delete_by_xiaoer = card_data.get("deleteByXiaoer").map(value_is_trueish).unwrap_or(false);
            let item_deleted = card_data.get("itemDeleted").map(value_is_trueish).unwrap_or(false);
            let recent_sold_time = value_to_string_lossy(attrs.get("recentSoldTime"));
            let re_publish_text = value_to_string_lossy(card_data.get("rePublishText"));
            let re_publish_url = value_to_string_lossy(card_data.get("rePublishUrl"));
            let out_stock_time = value_to_string_lossy(card_data.get("outStockTime"));
            let gmt_off_shelf = value_to_string_lossy(attrs.get("gmtOffShelf"));
            let gmt_shelf = value_to_string_lossy(attrs.get("gmtShelf"));
            let first_modified = value_to_string_lossy(card_data.get("firstModified"));
            let first_modified_diff = value_to_string_lossy(card_data.get("firstModifiedDiff"));
            let can_polish = card_data.get("canPolish").map(value_is_trueish).unwrap_or(false);
            // 已售判定（修正：不再用 itemStatus==-2 误判）：仅当存在真实成交时间戳。
            let is_sold = !recent_sold_time.is_empty();
            // 下架原因文案：严格使用闲鱼自己的 headTitle（"小二下架"/"主动下架"/"已售"等）。
            // 不再自己写"违规下架"等猜测文案 —— "小二下架" 不一定违规（也可能是
            // 系统/政策/活动结束），猜了反而误导用户。headTitle 没值时再 fallback。
            let off_reason = if !head_title.is_empty() { head_title.clone() }
                else if !off_reason_explicit.is_empty() { off_reason_explicit }
                else if item_deleted { "已删除".to_string() }
                else if delete_by_xiaoer { "小二删除".to_string() }
                else if instock_by_xiaoer { "小二处理".to_string() }
                else { "已下架".to_string() };
            Some(json!({
                "itemId": card_data.get("id").or_else(|| card_data.get("longItemId")).and_then(super::value_to_string).unwrap_or_default(),
                "title": card_data.get("title").and_then(Value::as_str).unwrap_or_default(),
                "price": value_to_string_lossy(card_data.get("price")),
                "originalPrice": value_to_string_lossy(card_data.get("originalPrice")),
                "picUrl": card_data.get("picUrl").and_then(Value::as_str).unwrap_or_default(),
                "viewCount": card_data.get("browseCount").cloned().unwrap_or_else(|| Value::from(0)),
                "wantCount": card_data.get("collectNum").cloned().unwrap_or_else(|| Value::from(0)),
                "exposureNum": card_data.get("exposureNum").cloned().unwrap_or_else(|| Value::from(0)),
                "sold": is_sold,
                "itemStatus": item_status,
                "headTitle": head_title,
                "headIconUrl": head_icon_url,
                "offReason": off_reason,
                "instockByXiaoer": instock_by_xiaoer,
                "deleteByXiaoer": delete_by_xiaoer,
                "itemDeleted": item_deleted,
                "canPolish": can_polish,
                "rePublishText": re_publish_text,
                "rePublishUrl": re_publish_url,
                "outStockTime": out_stock_time,
                "gmtOffShelf": gmt_off_shelf,
                "gmtShelf": gmt_shelf,
                "firstModified": first_modified,
                "firstModifiedDiff": first_modified_diff,
                "recentSoldTime": recent_sold_time,
                "online": false,
                "coinOpen": false,
                "coinSuffix": "",
                "coinRatio": "",
                "area": card_data.get("area").and_then(Value::as_str).unwrap_or_default(),
                "city": card_data.get("city").and_then(Value::as_str).unwrap_or_default(),
                "province": card_data.get("province").and_then(Value::as_str).unwrap_or_default(),
                "divisionId": card_data.get("divisionId").map(value_to_string_ref).filter(|v| !v.is_empty()).unwrap_or_else(|| attrs.get("cainiaoDivision").map(value_to_string_ref).unwrap_or_default()),
                "gps": card_data.get("gps").and_then(Value::as_str).unwrap_or_default(),
                "categoryId": value_to_string_lossy(card_data.get("categoryId")),
                "channelCatId": attrs.get("cCatId").map(value_to_string_ref).unwrap_or_default(),
                "tbCatId": attrs.get("tbCatId").map(value_to_string_ref).unwrap_or_default(),
                "leafId": card_data.get("leafId").map(value_to_string_ref).filter(|v| !v.is_empty()).unwrap_or_else(|| attrs.get("lfId").map(value_to_string_ref).unwrap_or_default()),
                "addressType": attrs.get("addressType").cloned().unwrap_or_else(|| Value::from(5)),
                "editorVersion": attrs.get("editorVersion").and_then(Value::as_str).unwrap_or("0.0.1"),
                "publishScene": attrs.get("pScene").and_then(Value::as_str).unwrap_or_default(),
                "uniqueCode": attrs.get("uniqueCode").and_then(Value::as_str).unwrap_or_default(),
                "freeShip": card_data.get("freeShip").cloned().unwrap_or_else(|| Value::Bool(false)),
                "supportFreight": attrs.get("supportFreight").map(value_is_trueish).unwrap_or(false),
                "noBargain": resolve_no_bargain(attrs.get("noBargain"), attrs.get("bargain"), false),
                "simpleItem": attrs.get("simItem").map(value_is_trueish).unwrap_or(false),
                "imageUrls": card_data.get("imageUrls").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                "attributesMap": Value::Object(attrs),
            }))
        })
        .collect()
}

pub(super) fn parse_coin_map(raw: &Value) -> Map<String, Value> {
    let mut map = Map::new();
    for card in raw
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("cardList"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let Some(card_obj) = card.as_object() else {
            continue;
        };
        let item_id = card_obj
            .get("id")
            .map(value_to_string_ref)
            .unwrap_or_default();
        if item_id.is_empty() {
            continue;
        }
        let ratios = card_obj
            .get("coinDeductionInfoList")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let info = card_obj
            .get("coinDeductionInfo")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let active_ratio = ratios
            .iter()
            .find(|ratio| ratio.get("open").and_then(Value::as_bool).unwrap_or(false))
            .and_then(|ratio| ratio.get("ratio"))
            .map(value_to_string_ref)
            .filter(|value| !value.is_empty())
            .or_else(|| {
                info.get("ratio")
                    .or_else(|| info.get("deductionRatio"))
                    .map(value_to_string_ref)
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or_default();
        let ratio_list = ratios
            .iter()
            .filter(|ratio| {
                ratio
                    .get("canOpen")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
            })
            .map(|ratio| {
                json!({
                    "ratio": value_to_string_lossy(ratio.get("ratio")),
                    "description": value_to_string_lossy(ratio.get("description")
                        .or_else(|| ratio.get("title"))
                        .or_else(|| ratio.get("name"))),
                    "price": value_to_string_lossy(ratio.get("price")
                        .or_else(|| ratio.get("discountPrice"))
                        .or_else(|| ratio.get("deductionPrice"))),
                    "coin": value_to_string_lossy(ratio.get("coin")
                        .or_else(|| ratio.get("costCoin"))
                        .or_else(|| ratio.get("coinAmount"))),
                    "open": ratio.get("open").and_then(Value::as_bool).unwrap_or(false),
                    "canOpen": ratio.get("canOpen").and_then(Value::as_bool).unwrap_or(true),
                })
            })
            .collect::<Vec<_>>();
        map.insert(
            item_id,
            json!({
                "coinOpen": info.get("open").and_then(Value::as_bool).unwrap_or(false) || !active_ratio.is_empty(),
                "coinRatio": active_ratio,
                "coinCanOpen": info.get("canOpen").and_then(Value::as_bool).unwrap_or(true),
                "coinRatioList": ratio_list,
            }),
        );
    }
    map
}

pub(super) fn merge_coin_map(items: &mut [Value], coin_map: &Map<String, Value>) {
    for item in items {
        let Some(item_id) = item.get("itemId").and_then(Value::as_str) else {
            continue;
        };
        let Some(coin) = coin_map.get(item_id) else {
            continue;
        };
        let Some(item_object) = item.as_object_mut() else {
            continue;
        };
        let Some(coin_object) = coin.as_object() else {
            continue;
        };
        for (key, value) in coin_object {
            if key == "coinRatio" && value.as_str().unwrap_or_default().trim().is_empty() {
                continue;
            }
            item_object.insert(key.clone(), value.clone());
        }
    }
}

const RECENT_COIN_STATE_PRESERVE_WINDOW_SECS: f64 = 300.0;

// ---------------------------------------------------------------------------
// DB helpers: sync state
// ---------------------------------------------------------------------------

pub(super) async fn write_sync_state(
    pool: &SqlitePool,
    uid: &str,
    kind: &str,
    now: f64,
    ok: bool,
    reason: &str,
    total: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO account_sync_state (uid, kind, last_synced_at, last_ok, last_reason, total_count)
        VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(uid, kind) DO UPDATE SET
            last_synced_at = excluded.last_synced_at,
            last_ok = excluded.last_ok,
            last_reason = excluded.last_reason,
            total_count = excluded.total_count
        "#,
    )
    .bind(uid)
    .bind(kind)
    .bind(now)
    .bind(if ok { 1_i64 } else { 0 })
    .bind(reason)
    .bind(total)
    .execute(pool)
    .await
    .map(|_| ())
}

// ---------------------------------------------------------------------------
// sync_items_to_db — pulls upstream and persists into account_items
// ---------------------------------------------------------------------------

pub(crate) async fn sync_items_to_db(state: &AppState, uid: &str) -> Result<SyncOutcome, String> {
    let location = load_location_or_default(state, uid).await;
    let pool = state.storage().xianyu();
    let mut total = 0_i64;
    let mut warnings: Vec<String> = Vec::new();
    let mut collected_on_sale: Vec<Value> = Vec::new();
    // Raw upstream card snapshots, kept so the post-pagination violation scan
    // can inspect bizMessage / banner.title / auditStatus fields that
    // parse_item_list_from_upstream + parse_sold_out_items_from_upstream don't
    // forward into the parsed Value. Each entry is one raw card (ON_SALE:
    // itemCards[*]; SOLD_OUT: cardList[*].cardData).
    let mut raw_cards_for_scan: Vec<Value> = Vec::new();

    // ON_SALE — paginate until hasMore=false.
    let mut page = 1_i64;
    loop {
        let raw = successful_upstream_h5_request(
            state,
            uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.item.management.query"),
            "1.0",
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "latitude": location.latitude,
                "longitude": location.longitude,
                "auctionTypes": ["b", "d", "a"],
                "newVersion": "2.0",
                "onlineSubTab": 1,
                "pageNumber": page,
                "rowsPerPage": 40,
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
                    &format!("sync_items_failed: {msg}"),
                )
                .await;
                msg = "会话已过期，已尝试自动恢复但失败，请刷新账号登录态后重试同步".to_string();
            }
            let now = unix_seconds_now();
            let _ = write_sync_state(pool, uid, "items", now, false, &msg, total).await;
            return Ok(SyncOutcome {
                ok: false,
                total,
                synced_at: now,
                detail: msg,
            });
        };
        let mut batch = super::items::parse_item_list_from_upstream(&raw);
        // Capture raw ON_SALE cards (cardData not nested — the array is
        // `itemCards[*]` directly) for the violation-detection pass.
        if let Some(items) = raw
            .get("data")
            .and_then(|d| d.get("itemCards"))
            .and_then(Value::as_array)
        {
            for c in items {
                raw_cards_for_scan.push(c.clone());
            }
        }
        if let Ok(coin_raw) = successful_upstream_h5_request(
            state,
            uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.coin.deduction.list"),
            "1.0",
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "deductionType": 1,
                "pageNumber": 1,
                "pageSize": 50,
                "sortType": 0,
            }),
            Vec::new(),
        )
        .await
        {
            let coin_map = parse_coin_map(&coin_raw);
            merge_coin_map(&mut batch, &coin_map);
        }
        if batch.is_empty() {
            break;
        }
        total += batch.len() as i64;
        collected_on_sale.append(&mut batch);

        let has_more = raw
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("nextPage"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_more {
            break;
        }
        page += 1;
        if page > 50 {
            break; // safety bound
        }
    }

    // SOLD_OUT
    let mut collected_sold_out: Vec<Value> = Vec::new();
    let mut page = 1_i64;
    loop {
        let raw = successful_upstream_h5_request(
            state,
            uid,
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.wx.user.pulloff.items"),
            "4.0",
            json!({
                "utdid": "xianyu-replay",
                "platform": "mac",
                "miniAppVersion": "9.9.9",
                "pageNumber": page,
            }),
            Vec::new(),
        )
        .await;
        let raw = match raw {
            Ok(raw) => raw,
            Err(msg) => {
                let warn = if auth_expired_message(&msg) {
                    let _ = crate::cookie_refresh::mark_business_session_failure(
                        state.storage().accounts(),
                        uid,
                        &format!("sync_items_sold_out_failed: {msg}"),
                    )
                    .await;
                    "SOLD_OUT sync failed: 会话已过期，已尝试自动恢复但失败".to_string()
                } else {
                    format!("SOLD_OUT sync failed: {msg}")
                };
                warnings.push(warn);
                break;
            }
        };
        let items = parse_sold_out_items_from_upstream(&raw);
        // Capture SOLD_OUT raw cardData snapshots for violation scan. The
        // pulloff.items endpoint returns cardList[*] each wrapping a cardData
        // object; bizMessage / banner.title / itemStatus live on cardData.
        if let Some(card_list) = raw
            .get("data")
            .and_then(|d| d.get("cardList"))
            .and_then(Value::as_array)
        {
            for c in card_list {
                if let Some(card_data) = c.get("cardData") {
                    raw_cards_for_scan.push(card_data.clone());
                }
            }
        }
        if items.is_empty() {
            break;
        }
        collected_sold_out.extend(items);
        let has_more = raw
            .get("data")
            .and_then(Value::as_object)
            .and_then(|data| data.get("nextPage"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !has_more {
            break;
        }
        page += 1;
        if page > 20 {
            break;
        }
    }

    let now = unix_seconds_now();
    let mut persisted = 0_i64;
    for item in &collected_on_sale {
        if upsert_account_item(pool, uid, item, "ON_SALE", now)
            .await
            .is_ok()
        {
            persisted += 1;
        }
    }
    for item in &collected_sold_out {
        if upsert_account_item(pool, uid, item, "SOLD_OUT", now)
            .await
            .is_ok()
        {
            persisted += 1;
        }
    }

    // Violation scan — run AFTER persistence so the items table reflects the
    // current state, but BEFORE prune (prune only touches account_items, not
    // account_violations, so order is interchangeable in practice). Per-card
    // failures don't fail the sync.
    scan_and_record_violations(state, uid, &raw_cards_for_scan).await;

    // Prune stale rows: upstream is the source of truth. Anything in the cache
    // that wasn't in either fresh batch is either deleted/下架 by the user, taken
    // down by Xianyu, or was wrongly cached in a past sync. Without this, the
    // cache only grows — users see phantom items their phone-app inventory
    // doesn't have. Scope the delete to (uid, ...) so we never touch other
    // accounts' rows.
    let fresh_ids: Vec<String> = collected_on_sale
        .iter()
        .chain(collected_sold_out.iter())
        .filter_map(|item| {
            item.get("itemId").and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
        })
        .filter(|s| !s.is_empty())
        .collect();
    let pruned = if fresh_ids.is_empty() {
        // Upstream truly returned nothing — wipe ALL of this uid's cached items.
        sqlx::query("DELETE FROM account_items WHERE uid = ?1")
            .bind(uid)
            .execute(pool)
            .await
            .map(|r| r.rows_affected() as i64)
            .unwrap_or(0)
    } else {
        let placeholders = std::iter::repeat("?")
            .take(fresh_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "DELETE FROM account_items WHERE uid = ? AND item_id NOT IN ({})",
            placeholders
        );
        let mut q = sqlx::query(&sql).bind(uid);
        for id in &fresh_ids {
            q = q.bind(id);
        }
        q.execute(pool)
            .await
            .map(|r| r.rows_affected() as i64)
            .unwrap_or(0)
    };
    if pruned > 0 {
        tracing::info!(
            uid = %uid,
            pruned = pruned,
            kept_on_sale = collected_on_sale.len(),
            kept_sold_out = collected_sold_out.len(),
            "sync_items pruned stale account_items rows"
        );
    }

    let ok = warnings.is_empty();
    let detail = if ok {
        format!(
            "ON_SALE {}, SOLD_OUT {}",
            collected_on_sale.len(),
            collected_sold_out.len()
        )
    } else {
        format!(
            "ON_SALE {}, SOLD_OUT {}, warnings: {}",
            collected_on_sale.len(),
            collected_sold_out.len(),
            warnings.join(" | ")
        )
    };
    let reason = if ok { "ok".to_string() } else { detail.clone() };
    let _ = write_sync_state(pool, uid, "items", now, ok, &reason, persisted).await;
    Ok(SyncOutcome {
        ok,
        total: persisted,
        synced_at: now,
        detail,
    })
}

// ---------------------------------------------------------------------------
// DB helpers: item persistence
// ---------------------------------------------------------------------------

async fn upsert_account_item(
    pool: &SqlitePool,
    uid: &str,
    item: &Value,
    status: &str,
    now: f64,
) -> Result<(), sqlx::Error> {
    let item_id = super::value_str(item, "itemId");
    if item_id.is_empty() {
        return Ok(());
    }
    let image_urls = item
        .get("imageUrls")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let image_urls_json = serde_json::to_string(&image_urls).unwrap_or_else(|_| "[]".to_string());
    let online = item.get("online").and_then(Value::as_bool).unwrap_or(false);
    let coin_open = item
        .get("coinOpen")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let existing_row = sqlx::query(
        "SELECT coin_open, coin_ratio, coin_opened_at, detail_json FROM account_items WHERE uid = ? AND item_id = ?",
    )
    .bind(uid)
    .bind(&item_id)
    .fetch_optional(pool)
    .await?;
    let existing_coin_open = existing_row
        .as_ref()
        .and_then(|row| row.try_get::<i64, _>("coin_open").ok())
        .unwrap_or(0)
        != 0;
    let existing_coin_opened_at = existing_row
        .as_ref()
        .and_then(|row| row.try_get::<f64, _>("coin_opened_at").ok())
        .unwrap_or(0.0);
    let existing_coin_ratio = existing_row
        .as_ref()
        .and_then(|row| row.try_get::<String, _>("coin_ratio").ok())
        .unwrap_or_default();
    let existing_detail = existing_row
        .as_ref()
        .and_then(|row| row.try_get::<String, _>("detail_json").ok())
        .map(|raw| super::json_object_from_text(&raw))
        .unwrap_or_default();
    let incoming_coin_suffix = super::value_str(item, "coinSuffix");
    let incoming_coin_ratio = super::value_str(item, "coinRatio");
    let has_coin_badge_without_ratio = !incoming_coin_suffix.trim().is_empty()
        && !coin_open
        && existing_coin_open
        && existing_coin_ratio.trim().len() > 0;
    let recent_coin_state_age = now - existing_coin_opened_at;
    let preserve_recent_coin_state = !coin_open
        && incoming_coin_ratio.trim().is_empty()
        && existing_coin_open
        && !existing_coin_ratio.trim().is_empty()
        && existing_coin_opened_at > 0.0
        && recent_coin_state_age >= 0.0
        && recent_coin_state_age <= RECENT_COIN_STATE_PRESERVE_WINDOW_SECS;
    let effective_coin_open =
        coin_open || has_coin_badge_without_ratio || preserve_recent_coin_state;
    let coin_opened_at = if effective_coin_open {
        // 同步列表只负责保留已知的开启时间，不应把"历史上早就开着、但本地没记录时间"
        // 的商品误记成"刚开启"。真实开启动作由 open 接口成功后再写入时间戳。
        if existing_coin_opened_at > 0.0 {
            existing_coin_opened_at
        } else {
            0.0
        }
    } else {
        0.0
    };
    // 商品发布时间：上游 item 顶层带 gmtCreate / createTime 等多个候选字段，
    // 在 detail_json 里上游又不返这些。把候选字段一并夹带进 item_context_json，
    // 前端 deepFindTime 才有东西可翻。空字段过滤掉避免误命中。
    let mut ctx_map = serde_json::Map::new();
    for key in [
        "gmtCreate",
        "gmtCreated",
        "createTime",
        "publishTime",
        "postTime",
        "onShelfTime",
        "onSellTime",
        "onlineTime",
    ] {
        let v = super::value_str(item, key);
        if !v.is_empty() {
            ctx_map.insert(key.to_string(), Value::String(v));
        }
    }
    let item_context_json =
        serde_json::to_string(&Value::Object(ctx_map)).unwrap_or_else(|_| "{}".to_string());
    // 透传 server-driven UI 的 actions[]（含 actionUrl 预签名 token）。
    let actions_value = item
        .get("actions")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let actions_json = serde_json::to_string(&actions_value).unwrap_or_else(|_| "[]".to_string());
    // 把 SOLD_OUT 商品的"下架信息"全字段塞进 detail_json，避免新增 schema 列。
    // 前端从 it.detail.* 读取展示完整下架细节（原因/时间/上架按钮等）。
    // 同时把"上架时需要的类目/地址/选项字段"也存进来 —— 否则 itemUpshelf 调用
    // 后端 build_item_open_payload 时类目为空，闲鱼会返
    // FAIL_BIZ_CHANNEL_CAT_ID_PATH_QUERY_ERROR。
    let mut detail_map = serde_json::Map::new();
    for key in [
        // 下架信息
        "itemStatus",
        "headTitle",
        "headIconUrl",
        "offReason",
        "rePublishText",
        "rePublishUrl",
        "outStockTime",
        "gmtOffShelf",
        "gmtShelf",
        "firstModified",
        "firstModifiedDiff",
        "recentSoldTime",
        "instockByXiaoer",
        "deleteByXiaoer",
        "itemDeleted",
        "canPolish",
        "sold",
        // 类目 + 上架需要的发布参数（list 接口已 normalize 出来，不要再丢）
        "tbCatId",
        "channelCatId",
        "leafId",
        "publishScene",
        "sourceId",
        "addressType",
        "simpleItem",
        "supportFreight",
        "noBargain",
        "freeShip",
        // 闲鱼币抵扣展示/可选比例，存在 detail_json 里供刷新后的前端继续使用。
        "coinSuffix",
        "coinCanOpen",
        "coinRatioList",
        // 2人小刀状态，来自 management.query itemCards[].sellerPromotionVO
        "grouponOpen",
        "grouponPrice",
        "sellerPromotionVO",
    ] {
        if let Some(v) = item.get(key) {
            if !matches!(v, Value::Null) && !(v.as_str().map(|s| s.is_empty()).unwrap_or(false)) {
                detail_map.insert(key.to_string(), v.clone());
            }
        }
    }
    if let Some(republished_to) = existing_detail
        .get("republishedToItemId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        detail_map.insert(
            "republishedToItemId".to_string(),
            Value::String(republished_to.to_string()),
        );
    }
    let incoming_groupon_open = item
        .get("grouponOpen")
        .map(value_is_trueish)
        .unwrap_or(false);
    let incoming_groupon_price = super::value_str(item, "grouponPrice");
    let incoming_promotion_present = item
        .get("sellerPromotionVO")
        .and_then(Value::as_object)
        .map(|object| !object.is_empty())
        .unwrap_or(false);
    if !incoming_promotion_present
        && !incoming_groupon_open
        && incoming_groupon_price.trim().is_empty()
    {
        let existing_groupon_open = existing_detail
            .get("grouponOpen")
            .map(value_is_trueish)
            .unwrap_or(false);
        let existing_groupon_price = existing_detail
            .get("grouponPrice")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if existing_groupon_open || !existing_groupon_price.is_empty() {
            detail_map.insert(
                "grouponOpen".to_string(),
                Value::Bool(existing_groupon_open),
            );
            detail_map.insert(
                "grouponPrice".to_string(),
                Value::String(existing_groupon_price),
            );
            if let Some(existing_promotion) = existing_detail.get("sellerPromotionVO") {
                let non_empty = match existing_promotion {
                    Value::Object(object) => !object.is_empty(),
                    Value::Null => false,
                    _ => true,
                };
                if non_empty {
                    detail_map.insert("sellerPromotionVO".to_string(), existing_promotion.clone());
                }
            }
        }
    }
    let detail_json = if detail_map.is_empty() {
        "{}".to_string()
    } else {
        serde_json::to_string(&Value::Object(detail_map)).unwrap_or_else(|_| "{}".to_string())
    };
    let persisted_coin_ratio = if effective_coin_open && incoming_coin_ratio.trim().is_empty() {
        existing_coin_ratio.clone()
    } else {
        incoming_coin_ratio.clone()
    };
    sqlx::query(
        r#"
        INSERT INTO account_items (
            item_id, uid, title, short_title, description, price, orig_price, pic_url,
            image_urls_json, status, online, want_count, view_count, exposure_num,
            coin_open, coin_ratio, coin_opened_at, city, province, area, division_id, latitude, longitude,
            fish_ad_code, item_context_json, detail_json, actions_json, created_at, updated_at
        )
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(uid, item_id) DO UPDATE SET
            title = excluded.title,
            price = excluded.price,
            orig_price = excluded.orig_price,
            pic_url = excluded.pic_url,
            image_urls_json = excluded.image_urls_json,
            status = excluded.status,
            online = excluded.online,
            want_count = excluded.want_count,
            view_count = excluded.view_count,
            exposure_num = excluded.exposure_num,
            coin_open = excluded.coin_open,
            coin_ratio = CASE
                WHEN excluded.coin_open = 1 AND excluded.coin_ratio = '' THEN account_items.coin_ratio
                ELSE excluded.coin_ratio
            END,
            coin_opened_at = CASE
                WHEN excluded.coin_open = 0 THEN 0
                WHEN account_items.coin_opened_at IS NULL OR account_items.coin_opened_at <= 0 THEN excluded.coin_opened_at
                ELSE account_items.coin_opened_at
            END,
            city = excluded.city,
            province = excluded.province,
            area = excluded.area,
            division_id = excluded.division_id,
            fish_ad_code = excluded.fish_ad_code,
            item_context_json = excluded.item_context_json,
            detail_json = excluded.detail_json,
            actions_json = excluded.actions_json,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&item_id)
    .bind(uid)
    .bind(super::value_str(item, "title"))
    .bind("")
    .bind("")
    .bind(super::value_str(item, "price"))
    .bind(super::value_str(item, "originalPrice"))
    .bind(super::value_str(item, "picUrl"))
    .bind(image_urls_json)
    .bind(status)
    .bind(if online { 1_i64 } else { 0 })
    .bind(item.get("wantCount").and_then(Value::as_i64).unwrap_or(0))
    .bind(item.get("viewCount").and_then(Value::as_i64).unwrap_or(0))
    .bind(
        item.get("exposureNum")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<i64>().ok())
            .or_else(|| item.get("exposureNum").and_then(Value::as_i64))
            .unwrap_or(0),
    )
    .bind(if effective_coin_open { 1_i64 } else { 0 })
    .bind(persisted_coin_ratio)
    .bind(coin_opened_at)
    .bind(super::value_str(item, "city"))
    .bind(super::value_str(item, "province"))
    .bind(super::value_str(item, "area"))
    .bind(super::value_str(item, "divisionId"))
    .bind("")
    .bind("")
    .bind("")
    .bind(item_context_json)
    .bind(detail_json)
    .bind(actions_json)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{item_row_to_value, query_items_by_uid, upsert_account_item};
    use serde_json::json;
    use sqlx::{sqlite::SqlitePoolOptions, Row};

    async fn setup_items_pool() -> sqlx::SqlitePool {
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
                actions_json TEXT NOT NULL DEFAULT '[]',
                created_at REAL NOT NULL DEFAULT 0,
                updated_at REAL NOT NULL DEFAULT 0,
                coin_opened_at REAL NOT NULL DEFAULT 0,
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
        pool
    }

    #[tokio::test]
    async fn upsert_account_item_preserves_existing_coin_ratio_when_sync_returns_open_without_ratio(
    ) {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, price, status, online, coin_open, coin_ratio, coin_opened_at,
                image_urls_json, item_context_json, detail_json, actions_json, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '[]', '{}', '{}', '[]', ?, ?)
            "#,
        )
        .bind("item-1")
        .bind("uid-1")
        .bind("Old item")
        .bind("150.00")
        .bind("ON_SALE")
        .bind(1_i64)
        .bind(1_i64)
        .bind("20")
        .bind(12345.0_f64)
        .bind(100.0_f64)
        .bind(100.0_f64)
        .execute(&pool)
        .await
        .expect("seed row");

        let item = json!({
            "itemId": "item-1",
            "title": "New item",
            "price": "150.00",
            "originalPrice": "300.00",
            "picUrl": "https://example.com/item.jpg",
            "imageUrls": [],
            "online": true,
            "status": "0",
            "viewCount": 0,
            "wantCount": 0,
            "exposureNum": 0,
            "coinOpen": true,
            "coinRatio": "",
            "coinRatioList": [
                { "ratio": "10", "open": false },
                { "ratio": "20", "open": false }
            ],
            "city": "",
            "province": "",
            "area": "",
            "divisionId": "",
            "actions": []
        });

        upsert_account_item(&pool, "uid-1", &item, "ON_SALE", 200.0_f64)
            .await
            .expect("upsert item");

        let row = sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-1")
            .bind("item-1")
            .fetch_one(&pool)
            .await
            .expect("fetch row");

        assert_eq!(row.get::<String, _>("coin_ratio"), "20");
        assert_eq!(row.get::<i64, _>("coin_open"), 1);
        assert_eq!(row.get::<f64, _>("coin_opened_at"), 12345.0_f64);

        let value = item_row_to_value(row);
        assert_eq!(value["coinRatio"], "20");
        assert_eq!(value["coinOpen"], true);
    }

    #[tokio::test]
    async fn upsert_account_item_preserves_existing_coin_open_when_sync_returns_coin_badge_without_ratio(
    ) {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, price, status, online, coin_open, coin_ratio, coin_opened_at,
                image_urls_json, item_context_json, detail_json, actions_json, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '[]', '{}', '{}', '[]', ?, ?)
            "#,
        )
        .bind("item-2")
        .bind("uid-2")
        .bind("Old item")
        .bind("15.00")
        .bind("ON_SALE")
        .bind(1_i64)
        .bind(1_i64)
        .bind("5")
        .bind(8888.0_f64)
        .bind(100.0_f64)
        .bind(100.0_f64)
        .execute(&pool)
        .await
        .expect("seed row");

        let item = json!({
            "itemId": "item-2",
            "title": "New item",
            "price": "15.00",
            "originalPrice": "30.00",
            "picUrl": "https://example.com/item.jpg",
            "imageUrls": [],
            "online": true,
            "status": "0",
            "viewCount": 0,
            "wantCount": 0,
            "exposureNum": 0,
            "coinOpen": false,
            "coinRatio": "",
            "coinSuffix": "+30000闲鱼币",
            "city": "",
            "province": "",
            "area": "",
            "divisionId": "",
            "actions": []
        });

        upsert_account_item(&pool, "uid-2", &item, "ON_SALE", 300.0_f64)
            .await
            .expect("upsert item");

        let row = sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-2")
            .bind("item-2")
            .fetch_one(&pool)
            .await
            .expect("fetch row");

        assert_eq!(row.get::<i64, _>("coin_open"), 1);
        assert_eq!(row.get::<String, _>("coin_ratio"), "5");
        assert_eq!(row.get::<f64, _>("coin_opened_at"), 8888.0_f64);

        let value = item_row_to_value(row);
        assert_eq!(value["coinOpen"], true);
        assert_eq!(value["coinRatio"], "5");
    }

    #[tokio::test]
    async fn upsert_account_item_preserves_recent_coin_state_when_sync_returns_no_coin_signal() {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, price, status, online, coin_open, coin_ratio, coin_opened_at,
                image_urls_json, item_context_json, detail_json, actions_json, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '[]', '{}', '{}', '[]', ?, ?)
            "#,
        )
        .bind("item-3")
        .bind("uid-3")
        .bind("Old item")
        .bind("20.00")
        .bind("ON_SALE")
        .bind(1_i64)
        .bind(1_i64)
        .bind("10")
        .bind(540.0_f64)
        .bind(400.0_f64)
        .bind(500.0_f64)
        .execute(&pool)
        .await
        .expect("seed row");

        let item = json!({
            "itemId": "item-3",
            "title": "New item",
            "price": "20.00",
            "originalPrice": "40.00",
            "picUrl": "https://example.com/item.jpg",
            "imageUrls": [],
            "online": true,
            "status": "0",
            "viewCount": 0,
            "wantCount": 0,
            "exposureNum": 0,
            "coinOpen": false,
            "coinRatio": "",
            "coinSuffix": "",
            "coinRatioList": [],
            "city": "",
            "province": "",
            "area": "",
            "divisionId": "",
            "actions": []
        });

        upsert_account_item(&pool, "uid-3", &item, "ON_SALE", 560.0_f64)
            .await
            .expect("upsert item");

        let row = sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-3")
            .bind("item-3")
            .fetch_one(&pool)
            .await
            .expect("fetch row");

        assert_eq!(row.get::<i64, _>("coin_open"), 1);
        assert_eq!(row.get::<String, _>("coin_ratio"), "10");
        assert_eq!(row.get::<f64, _>("coin_opened_at"), 540.0_f64);

        let value = item_row_to_value(row);
        assert_eq!(value["coinOpen"], true);
        assert_eq!(value["coinRatio"], "10");
    }

    #[tokio::test]
    async fn upsert_account_item_allows_stale_coin_state_to_clear_after_grace_window() {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, price, status, online, coin_open, coin_ratio, coin_opened_at,
                image_urls_json, item_context_json, detail_json, actions_json, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, '[]', '{}', '{}', '[]', ?, ?)
            "#,
        )
        .bind("item-4")
        .bind("uid-4")
        .bind("Old item")
        .bind("20.00")
        .bind("ON_SALE")
        .bind(1_i64)
        .bind(1_i64)
        .bind("10")
        .bind(600.0_f64)
        .bind(100.0_f64)
        .bind(100.0_f64)
        .execute(&pool)
        .await
        .expect("seed row");

        let item = json!({
            "itemId": "item-4",
            "title": "New item",
            "price": "20.00",
            "originalPrice": "40.00",
            "picUrl": "https://example.com/item.jpg",
            "imageUrls": [],
            "online": true,
            "status": "0",
            "viewCount": 0,
            "wantCount": 0,
            "exposureNum": 0,
            "coinOpen": false,
            "coinRatio": "",
            "coinSuffix": "",
            "coinRatioList": [],
            "city": "",
            "province": "",
            "area": "",
            "divisionId": "",
            "actions": []
        });

        upsert_account_item(&pool, "uid-4", &item, "ON_SALE", 1000.0_f64)
            .await
            .expect("upsert item");

        let row = sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-4")
            .bind("item-4")
            .fetch_one(&pool)
            .await
            .expect("fetch row");

        assert_eq!(row.get::<i64, _>("coin_open"), 0);
        assert_eq!(row.get::<String, _>("coin_ratio"), "");
        assert_eq!(row.get::<f64, _>("coin_opened_at"), 0.0_f64);
    }

    #[tokio::test]
    async fn management_sync_preserves_trusted_quantity_and_maps_it_to_api_fields() {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, status, online, quantity, quantity_source,
                quantity_synced_at, image_urls_json, item_context_json, detail_json,
                actions_json, created_at, updated_at
            ) VALUES (?, ?, ?, 'ON_SALE', 1, 7, 'pc.detail:data.itemDO.quantity', 123,
                '[]', '{}', '{}', '[]', 100, 100)
            "#,
        )
        .bind("item-quantity")
        .bind("uid-quantity")
        .bind("Old title")
        .execute(&pool)
        .await
        .expect("seed quantity snapshot");

        let item = json!({
            "itemId": "item-quantity",
            "title": "Management title",
            "price": "10.00",
            "quantity": "1",
            "online": true,
            "imageUrls": [],
            "actions": []
        });
        upsert_account_item(&pool, "uid-quantity", &item, "ON_SALE", 200.0)
            .await
            .expect("sync management item");

        let row = sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-quantity")
            .bind("item-quantity")
            .fetch_one(&pool)
            .await
            .expect("fetch item");
        assert_eq!(row.get::<Option<i64>, _>("quantity"), Some(7));
        assert_eq!(
            row.get::<Option<String>, _>("quantity_source"),
            Some("pc.detail:data.itemDO.quantity".to_string())
        );
        assert_eq!(row.get::<Option<i64>, _>("quantity_synced_at"), Some(123));

        let value = item_row_to_value(row);
        assert_eq!(value["quantity"], 7);
        assert_eq!(value["quantitySource"], "pc.detail:data.itemDO.quantity");
        assert_eq!(value["quantitySyncedAt"], 123);
    }

    #[tokio::test]
    async fn management_sync_leaves_new_item_quantity_unknown() {
        let pool = setup_items_pool().await;
        let item = json!({
            "itemId": "new-item",
            "title": "Management item",
            "price": "10.00",
            "quantity": "1",
            "online": true,
            "imageUrls": [],
            "actions": []
        });

        upsert_account_item(&pool, "uid-new", &item, "ON_SALE", 200.0)
            .await
            .expect("sync new management item");
        let row = sqlx::query("SELECT * FROM account_items WHERE uid = ? AND item_id = ?")
            .bind("uid-new")
            .bind("new-item")
            .fetch_one(&pool)
            .await
            .expect("fetch new item");
        assert_eq!(row.get::<Option<i64>, _>("quantity"), None);

        let value = item_row_to_value(row);
        assert_eq!(value["quantity"], serde_json::Value::Null);
        assert_eq!(value["quantitySource"], "");
        assert_eq!(value["quantitySyncedAt"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn sync_preserves_republished_source_marker_and_queries_hide_it() {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, status, online, image_urls_json, item_context_json,
                detail_json, actions_json, created_at, updated_at
            ) VALUES (?, ?, ?, 'SOLD_OUT', 0, '[]', '{}', ?, '[]', 100, 100)
            "#,
        )
        .bind("old-item")
        .bind("uid-republish")
        .bind("Old listing")
        .bind(r#"{"republishedToItemId":"new-item"}"#)
        .execute(&pool)
        .await
        .expect("seed republished source");

        let upstream_item = json!({
            "itemId": "old-item",
            "title": "Old listing",
            "price": "10.00",
            "online": false,
            "imageUrls": [],
            "actions": [],
            "headTitle": "主动下架"
        });
        upsert_account_item(&pool, "uid-republish", &upstream_item, "SOLD_OUT", 200.0)
            .await
            .expect("sync old listing");

        let row =
            sqlx::query("SELECT detail_json FROM account_items WHERE uid = ? AND item_id = ?")
                .bind("uid-republish")
                .bind("old-item")
                .fetch_one(&pool)
                .await
                .expect("fetch old listing");
        let detail: serde_json::Value =
            serde_json::from_str(&row.get::<String, _>("detail_json")).expect("detail json");
        assert_eq!(detail["republishedToItemId"], "new-item");

        let items = query_items_by_uid(&pool, "uid-republish", "SOLD_OUT", 20, 0)
            .await
            .expect("query sold out items");
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn query_items_by_uid_sorts_by_publish_time_desc_not_item_id() {
        let pool = setup_items_pool().await;
        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, status, online, item_context_json, detail_json, actions_json,
                created_at, updated_at
            ) VALUES (?, ?, ?, 'ON_SALE', 1, ?, '{}', '[]', ?, ?)
            "#,
        )
        .bind("999")
        .bind("uid-sort")
        .bind("older-by-gmt-create")
        .bind(r#"{"gmtCreate":"2026-04-15 10:00:00"}"#)
        .bind(10.0_f64)
        .bind(10.0_f64)
        .execute(&pool)
        .await
        .expect("seed older row");

        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, status, online, item_context_json, detail_json, actions_json,
                created_at, updated_at
            ) VALUES (?, ?, ?, 'ON_SALE', 1, ?, '{}', '[]', ?, ?)
            "#,
        )
        .bind("100")
        .bind("uid-sort")
        .bind("newer-by-gmt-created")
        .bind(r#"{"gmtCreated":"2026-04-16 10:00:00"}"#)
        .bind(20.0_f64)
        .bind(20.0_f64)
        .execute(&pool)
        .await
        .expect("seed newer row");

        sqlx::query(
            r#"
            INSERT INTO account_items (
                item_id, uid, title, status, online, item_context_json, detail_json, actions_json,
                created_at, updated_at
            ) VALUES (?, ?, ?, 'ON_SALE', 1, ?, '{}', '[]', ?, ?)
            "#,
        )
        .bind("050")
        .bind("uid-sort")
        .bind("newest-by-create-time")
        .bind(r#"{"createTime":"2026-04-17 10:00:00"}"#)
        .bind(30.0_f64)
        .bind(30.0_f64)
        .execute(&pool)
        .await
        .expect("seed newest row");

        let items = query_items_by_uid(&pool, "uid-sort", "ALL", 20, 0)
            .await
            .expect("query items");

        let titles: Vec<&str> = items
            .iter()
            .map(|item| item["title"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            titles,
            vec![
                "newest-by-create-time",
                "newer-by-gmt-created",
                "older-by-gmt-create",
            ]
        );
    }
}

// ---------------------------------------------------------------------------
// DB helpers: item query
// ---------------------------------------------------------------------------

pub(super) async fn query_items_by_uid(
    pool: &SqlitePool,
    uid: &str,
    tab: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Value>, sqlx::Error> {
    if limit <= 0 {
        return Ok(Vec::new());
    }

    let rows = if tab.eq_ignore_ascii_case("ALL") {
        sqlx::query(
            r#"
            SELECT *
            FROM account_items
            WHERE uid = ?
            "#,
        )
        .bind(uid)
        .fetch_all(pool)
        .await?
    } else {
        let status = if tab.eq_ignore_ascii_case("SOLD_OUT") {
            "SOLD_OUT"
        } else {
            "ON_SALE"
        };
        sqlx::query(
            r#"
            SELECT *
            FROM account_items
            WHERE uid = ? AND status = ?
            "#,
        )
        .bind(uid)
        .bind(status)
        .fetch_all(pool)
        .await?
    };

    let mut ranked_rows: Vec<_> = rows
        .into_iter()
        .filter(|row| {
            let detail = super::json_object_from_text(&row.get::<String, _>("detail_json"));
            detail
                .get("republishedToItemId")
                .and_then(Value::as_str)
                .map(|value| value.trim().is_empty())
                .unwrap_or(true)
        })
        .map(|row| {
            let publish_ts = item_publish_ts_from_row(&row);
            let created_at = row.get::<f64, _>("created_at");
            let item_id = row.get::<String, _>("item_id");
            (row, publish_ts, created_at, item_id)
        })
        .collect();
    ranked_rows.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(Ordering::Equal)
            .then_with(|| right.2.partial_cmp(&left.2).unwrap_or(Ordering::Equal))
            .then_with(|| right.3.cmp(&left.3))
    });

    let start = offset.max(0) as usize;
    let page_size = limit.max(0) as usize;
    Ok(ranked_rows
        .into_iter()
        .skip(start)
        .take(page_size)
        .map(|(row, _, _, _)| item_row_to_value(row))
        .collect())
}

fn item_publish_ts_from_row(row: &sqlx::sqlite::SqliteRow) -> f64 {
    let created_at = row.get::<f64, _>("created_at");
    let item_context = super::json_object_from_text(&row.get::<String, _>("item_context_json"));
    item_publish_ts_from_context(&item_context).unwrap_or(created_at)
}

fn item_publish_ts_from_context(item_context: &Value) -> Option<f64> {
    for key in [
        "gmtCreate",
        "gmtCreated",
        "createTime",
        "publishTime",
        "postTime",
        "onShelfTime",
        "onSellTime",
        "onlineTime",
    ] {
        let raw = value_to_string_lossy(item_context.get(key));
        let ts = parse_publish_time_to_unix_seconds(&raw);
        if ts.is_some() {
            return ts;
        }
    }
    None
}

fn parse_publish_time_to_unix_seconds(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(ts) = parse_local_datetime_to_unix_seconds(trimmed) {
        return Some(ts);
    }
    let numeric = trimmed.parse::<f64>().ok()?;
    if numeric >= 1_000_000_000_000.0 {
        Some(numeric / 1000.0)
    } else {
        Some(numeric)
    }
}

fn parse_local_datetime_to_unix_seconds(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    if trimmed.len() < 19 {
        return None;
    }
    let year = trimmed.get(0..4)?.parse::<i64>().ok()?;
    let month = trimmed.get(5..7)?.parse::<u32>().ok()?;
    let day = trimmed.get(8..10)?.parse::<u32>().ok()?;
    let hour = trimmed.get(11..13)?.parse::<i64>().ok()?;
    let minute = trimmed.get(14..16)?.parse::<i64>().ok()?;
    let second = trimmed.get(17..19)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second - 8 * 3_600) as f64)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let adjust = if month <= 2 { 1 } else { 0 };
    let y = year - adjust;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month as i64 + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(super) fn item_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    let latitude = row.get::<String, _>("latitude");
    let longitude = row.get::<String, _>("longitude");
    let quantity = row.get::<Option<i64>, _>("quantity");
    let quantity_source = row
        .get::<Option<String>, _>("quantity_source")
        .unwrap_or_default();
    let quantity_synced_at = row.get::<Option<i64>, _>("quantity_synced_at");
    let detail = super::json_object_from_text(&row.get::<String, _>("detail_json"));
    let coin_ratio = row.get::<String, _>("coin_ratio");
    let coin_suffix = detail
        .get("coinSuffix")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let coin_ratio_list = detail
        .get("coinRatioList")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let groupon_open = detail
        .get("grouponOpen")
        .map(value_is_trueish)
        .unwrap_or(false);
    let groupon_price = detail
        .get("grouponPrice")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    json!({
        "uid": row.get::<String, _>("uid"),
        "itemId": row.get::<String, _>("item_id"),
        "title": row.get::<String, _>("title"),
        "shortTitle": row.get::<String, _>("short_title"),
        "desc": row.get::<String, _>("description"),
        "price": row.get::<String, _>("price"),
        "originalPrice": row.get::<String, _>("orig_price"),
        "picUrl": row.get::<String, _>("pic_url"),
        "imageUrls": super::json_array_from_text(&row.get::<String, _>("image_urls_json")),
        "online": row.get::<i64, _>("online") != 0,
        "status": row.get::<String, _>("status"),
        "viewCount": row.get::<i64, _>("view_count"),
        "wantCount": row.get::<i64, _>("want_count"),
        "exposureNum": row.get::<i64, _>("exposure_num"),
        "quantity": quantity,
        "quantitySource": quantity_source,
        "quantitySyncedAt": quantity_synced_at,
        "coinOpen": row.get::<i64, _>("coin_open") != 0,
        "coinRatio": coin_ratio,
        "coinOpenedAt": row.get::<f64, _>("coin_opened_at"),
        "coinSuffix": coin_suffix,
        "coinRatioList": coin_ratio_list,
        "grouponOpen": groupon_open,
        "grouponPrice": groupon_price,
        "city": row.get::<String, _>("city"),
        "province": row.get::<String, _>("province"),
        "area": row.get::<String, _>("area"),
        "divisionId": row.get::<String, _>("division_id"),
        "latitude": latitude,
        "longitude": longitude,
        "gps": if latitude.is_empty() || longitude.is_empty() { String::new() } else { format!("{latitude},{longitude}") },
        "fishAdCode": row.get::<String, _>("fish_ad_code"),
        "itemContext": super::json_object_from_text(&row.get::<String, _>("item_context_json")),
        "detail": detail,
        "actions": super::json_array_from_text(&row.get::<String, _>("actions_json")),
    })
}

// ---------------------------------------------------------------------------
// Route handler
// ---------------------------------------------------------------------------

pub(super) async fn sync_items_route(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<SyncPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let outcome = sync_items_to_db(&state, &payload.uid)
        .await
        .map_err(ResponseError::internal_message)?;
    let status = if outcome.ok { "ok" } else { "error" };
    tracing::info!(uid = %payload.uid, ok = outcome.ok, status = status, detail = %outcome.detail, "sync_items_route result");
    Ok(ResponseJson(json!({
        "status": status,
        "ok": outcome.ok,
        "uid": payload.uid,
        "total": outcome.total,
        "synced_at": outcome.synced_at,
        "detail": outcome.detail,
    })))
}

// ---------------------------------------------------------------------------
// Violation scan — runs after items are persisted; spawns one tokio task per
// newly-inserted violation so the items_sync HTTP handler doesn't block on
// downstream WeChat HTTP latency.
// ---------------------------------------------------------------------------

async fn scan_and_record_violations(state: &AppState, uid: &str, raw_cards: &[Value]) {
    if raw_cards.is_empty() {
        return;
    }
    let detected_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    // Cache the account nick once per sync — every alert reuses it.
    let nick = super::violations::fetch_account_nick(state, uid).await;

    for card in raw_cards {
        let item_id = card
            .get("id")
            .or_else(|| card.get("itemId"))
            .or_else(|| card.get("longItemId"))
            .and_then(|v| {
                v.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .unwrap_or_default();
        if item_id.is_empty() {
            continue;
        }
        let Some(detected) = super::violations::detect_violation(card, &item_id) else {
            continue;
        };
        let item_title = card
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let inserted = match state
            .storage()
            .record_violation(
                uid,
                &item_id,
                &item_title,
                &detected.kind,
                &detected.reason,
                &detected.raw_metadata,
                detected_at_ms,
            )
            .await
        {
            Ok(flag) => flag,
            Err(err) => {
                tracing::warn!(
                    uid = %uid,
                    item_id = %item_id,
                    error = %err,
                    "violation record failed"
                );
                continue;
            }
        };

        if !inserted {
            continue;
        }

        // Look up the row id we just inserted so the dispatcher can mark it
        // notified on success. SQLite's last_insert_rowid() doesn't survive
        // out-of-task contexts, so we re-query by the UNIQUE tuple.
        let row_id = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT id FROM account_violations
            WHERE uid = ? AND item_id = ? AND violation_kind = ?
            "#,
        )
        .bind(uid)
        .bind(&item_id)
        .bind(&detected.kind)
        .fetch_one(state.storage().xianyu())
        .await
        .unwrap_or(0);
        if row_id <= 0 {
            continue;
        }

        tracing::info!(
            uid = %uid,
            item_id = %item_id,
            kind = %detected.kind,
            reason = %detected.reason,
            "violation detected — dispatching WeChat alert"
        );

        let state_clone = state.clone();
        let uid_owned = uid.to_string();
        let nick_owned = nick.clone();
        let item_id_owned = item_id.clone();
        let item_title_owned = item_title.clone();
        let kind_owned = detected.kind.clone();
        let reason_owned = detected.reason.clone();
        tokio::spawn(async move {
            super::violations::dispatch_violation_alert(
                state_clone,
                row_id,
                uid_owned,
                nick_owned,
                item_id_owned,
                item_title_owned,
                kind_owned,
                reason_owned,
            )
            .await;
        });
    }
}
