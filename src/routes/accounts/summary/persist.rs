use serde_json::{json, Map, Value};
use sqlx::Row;
use tracing::{info, warn};

use crate::{models::business::ReplyMode, state::AppState, xianyu_client::load_location};

use super::super::{
    parse_json_array, parse_json_object, unix_millis_now, unix_seconds_now, ResponseError,
    BOOTSTRAP_REFRESH_INTERVAL,
};
use super::helpers::{
    account_row_has_business_signal, coerce_i64, extract_fish_power_from_adapter_payload,
    extract_fish_shop_info_from_user_head, extract_total_earned_from_adapter_payload, fill_bill,
    fill_positive_number, fill_score_records, fill_seller_30d, fill_violations, has_amount_signal,
    has_trustworthy_security_score, normalize_security_score_total, summary_has_business_signal,
};
use super::upstream::{
    account_coin_balance_upstream, account_security_score_records_upstream,
    account_security_score_upstream, account_seller_data_upstream, account_trade_bill_upstream,
    account_user_head_upstream, account_user_page_adapter_upstream, account_violations_upstream,
};

pub(super) fn restore_summary_from_baseline(
    summary: &mut Value,
    cached: Option<&Value>,
    row: &sqlx::sqlite::SqliteRow,
) {
    let Some(summary_object) = summary.as_object_mut() else {
        return;
    };
    let cached_object = cached.and_then(Value::as_object);

    if !has_trustworthy_security_score(summary_object.get("securityScore")) {
        if let Some(cached_score) = cached_object.and_then(|cached| cached.get("securityScore")) {
            if has_trustworthy_security_score(Some(cached_score)) {
                summary_object.insert("securityScore".to_string(), cached_score.clone());
            }
        }
        if !has_trustworthy_security_score(summary_object.get("securityScore"))
            && row.get::<i64, _>("security_score") >= 0
        {
            let mut security = summary_object
                .get("securityScore")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            security.insert(
                "totalScore".to_string(),
                Value::from(row.get::<i64, _>("security_score")),
            );
            let restriction_end = row.get::<String, _>("restriction_end");
            if !restriction_end.trim().is_empty() {
                security.insert("restrictionEnd".to_string(), Value::String(restriction_end));
            }
            summary_object.insert("securityScore".to_string(), Value::Object(security));
        }
    }

    if coerce_i64(summary_object.get("coinBalance")) <= 0 {
        let cached_coin = cached_object
            .and_then(|cached| cached.get("coinBalance"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if cached_coin > 0 {
            summary_object.insert("coinBalance".to_string(), Value::from(cached_coin));
        } else {
            fill_positive_number(
                summary_object,
                "coinBalance",
                row.get::<i64, _>("coin_balance"),
            );
        }
    }

    if coerce_i64(summary_object.get("fishPower")) <= 0 {
        let cached_fish = cached_object
            .and_then(|cached| cached.get("fishPower"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if cached_fish > 0 {
            summary_object.insert("fishPower".to_string(), Value::from(cached_fish));
        } else {
            fill_positive_number(summary_object, "fishPower", row.get::<i64, _>("fish_power"));
        }
    }

    let current_violation_total = summary_object
        .get("violations")
        .and_then(Value::as_object)
        .and_then(|violations| violations.get("totalCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if current_violation_total <= 0 {
        let cached_violations = cached_object
            .and_then(|cached| cached.get("violations"))
            .cloned();
        if cached_violations
            .as_ref()
            .and_then(Value::as_object)
            .and_then(|violations| violations.get("totalCount"))
            .and_then(Value::as_i64)
            .unwrap_or(0)
            > 0
        {
            summary_object.insert(
                "violations".to_string(),
                cached_violations.unwrap_or_else(|| json!({})),
            );
        } else {
            fill_violations(summary_object, row.get::<i64, _>("violation_count"));
        }
    }

    let bill_has_signal = summary_object
        .get("bill")
        .and_then(Value::as_object)
        .map(|bill| {
            has_amount_signal(bill.get("monthAmount"))
                || coerce_i64(bill.get("monthOrderCount")) > 0
        })
        .unwrap_or(false);
    if !bill_has_signal {
        if let Some(cached_bill) = cached_object
            .and_then(|cached| cached.get("bill"))
            .filter(|bill| bill.is_object())
        {
            summary_object.insert("bill".to_string(), cached_bill.clone());
        } else {
            fill_bill(summary_object);
        }
    }

    let seller_present = summary_object
        .get("seller30d")
        .and_then(Value::as_object)
        .map(|seller| !seller.is_empty())
        .unwrap_or(false);
    if !seller_present {
        if let Some(cached_seller) = cached_object
            .and_then(|cached| cached.get("seller30d"))
            .filter(|seller| seller.is_object())
        {
            summary_object.insert("seller30d".to_string(), cached_seller.clone());
        } else {
            fill_seller_30d(summary_object, row.get::<String, _>("seller_30d_sold"));
        }
    }

    if !summary_object.contains_key("scoreRecords") {
        fill_score_records(summary_object);
    }

    let current_total_earned = summary_object
        .get("totalEarned")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if current_total_earned <= 0.0 {
        if let Some(cached_total_earned) = cached_object
            .and_then(|cached| cached.get("totalEarned"))
            .and_then(Value::as_f64)
            .filter(|value| *value > 0.0)
        {
            summary_object.insert("totalEarned".to_string(), Value::from(cached_total_earned));
        } else {
            let total_earned = row.get::<f64, _>("total_earned");
            if total_earned > 0.0 {
                summary_object.insert("totalEarned".to_string(), Value::from(total_earned));
            }
        }
    }

    if !summary_object.contains_key("fishShopEnabled") {
        if let Some(cached_enabled) = cached_object
            .and_then(|cached| cached.get("fishShopEnabled"))
            .and_then(Value::as_bool)
        {
            summary_object.insert("fishShopEnabled".to_string(), Value::Bool(cached_enabled));
        } else if row.get::<i64, _>("fish_shop_enabled") > 0 {
            summary_object.insert("fishShopEnabled".to_string(), Value::Bool(true));
        }
    }

    if !summary_object.contains_key("fishShopLevel") {
        if let Some(cached_level) = cached_object
            .and_then(|cached| cached.get("fishShopLevel"))
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            summary_object.insert("fishShopLevel".to_string(), Value::String(cached_level));
        } else {
            let fish_shop_level = row.get::<String, _>("fish_shop_level");
            if !fish_shop_level.trim().is_empty() {
                summary_object.insert("fishShopLevel".to_string(), Value::String(fish_shop_level));
            }
        }
    }
}

pub(super) async fn build_account_summary_live(state: &AppState, uid: &str) -> Value {
    let mut result = json!({ "uid": uid });

    if let Ok(payload) = account_security_score_upstream(state, uid).await {
        if let Some(data) = payload
            .get("data")
            .and_then(|value| value.get("data"))
            .and_then(Value::as_object)
        {
            let last_change = data
                .get("lastChangedScoreRecord")
                .and_then(Value::as_object)
                .map(|record| {
                    json!({
                        "changeTypeName": record.get("changeTypeName").cloned().unwrap_or_else(|| Value::String(String::new())),
                        "realChangeScore": record.get("realChangeScore").cloned().unwrap_or_else(|| Value::from(0)),
                        "decrease": record.get("decrease").cloned().unwrap_or_else(|| Value::Bool(false)),
                        "scoreAfter": record.get("scoreAfter").cloned().unwrap_or_else(|| Value::from(0)),
                        "scoreBefore": record.get("scoreBefore").cloned().unwrap_or_else(|| Value::from(0)),
                        "gmtCreate": record.get("gmtCreate").cloned().unwrap_or_else(|| Value::String(String::new())),
                    })
                });
            let mut security = Map::new();
            if let Some(score) = normalize_security_score_total(data.get("totalScore")) {
                security.insert("totalScore".to_string(), Value::from(score));
            }
            if let Some(gmt_modified) = data.get("gmtModified").and_then(Value::as_str) {
                security.insert(
                    "gmtModified".to_string(),
                    Value::String(gmt_modified.to_string()),
                );
            }
            if let Some(last_change) = last_change {
                security.insert("lastChange".to_string(), last_change);
            }
            if let Some(object) = result.as_object_mut() {
                object.insert("securityScore".to_string(), Value::Object(security));
            }
        }
    }

    if let Ok(payload) = account_security_score_records_upstream(state, uid, 1, 200).await {
        let records = payload
            .get("data")
            .and_then(|value| value.get("data"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .take(5)
            .map(|record| {
                let object = record.as_object().cloned().unwrap_or_default();
                json!({
                    "changeTypeName": object.get("changeTypeName").cloned().unwrap_or_else(|| Value::String(String::new())),
                    "decrease": object.get("decrease").cloned().unwrap_or_else(|| Value::Bool(false)),
                    "realChangeScore": object.get("realChangeScore").cloned().unwrap_or_else(|| Value::from(0)),
                    "scoreAfter": object.get("scoreAfter").cloned().unwrap_or_else(|| Value::from(0)),
                    "scoreBefore": object.get("scoreBefore").cloned().unwrap_or_else(|| Value::from(0)),
                    "gmtCreate": object.get("gmtCreate").cloned().unwrap_or_else(|| Value::String(String::new())),
                })
            })
            .collect::<Vec<_>>();
        if let Some(object) = result.as_object_mut() {
            object.insert("scoreRecords".to_string(), Value::Array(records));
        }
    }

    if let Ok(payload) = account_coin_balance_upstream(state, uid).await {
        if let Some(object) = result.as_object_mut() {
            // myIdleCoin 上游是字符串 "14873"（抓包确认），as_i64() 直接 None;
            // 兼容 number | string | "14,873" 三种形态。
            let coin_val = payload
                .get("data")
                .and_then(|value| value.get("myIdleCoin"));
            let coin = coin_val
                .and_then(Value::as_i64)
                .or_else(|| {
                    coin_val
                        .and_then(Value::as_str)
                        .map(|s| s.replace([',', ' '], ""))
                        .and_then(|s| s.parse::<i64>().ok())
                })
                .unwrap_or(0);
            object.insert("coinBalance".to_string(), Value::from(coin));
        }
    }

    if let Ok(payload) = account_violations_upstream(state, uid, 20, true).await {
        if let Some(data) = payload.get("data").and_then(Value::as_object) {
            if let Some(object) = result.as_object_mut() {
                object.insert(
                    "violations".to_string(),
                    json!({
                        "totalCount": data.get("totalCount").cloned().unwrap_or_else(|| Value::from(0)),
                        "hasNext": data.get("hasNext").cloned().unwrap_or_else(|| Value::Bool(false)),
                        "list": data.get("data").cloned().unwrap_or_else(|| Value::Array(Vec::new())),
                    }),
                );
            }
        }
    }

    if let Ok(payload) = account_user_head_upstream(state, uid).await {
        if let Some((enabled, level)) = extract_fish_shop_info_from_user_head(&payload) {
            if let Some(object) = result.as_object_mut() {
                object.insert("fishShopEnabled".to_string(), Value::Bool(enabled));
                if let Some(level) = level {
                    object.insert("fishShopLevel".to_string(), Value::String(level));
                }
            }
        }
    }

    match account_user_page_adapter_upstream(state, uid).await {
        Ok(payload) => {
            if let Some(object) = result.as_object_mut() {
                if let Some(fish_power) = extract_fish_power_from_adapter_payload(&payload) {
                    object.insert("fishPower".to_string(), Value::from(fish_power));
                }
                let extracted = extract_total_earned_from_adapter_payload(&payload);
                info!(uid = %uid, extracted_total_earned = ?extracted, "account_user_page_adapter parsed");
                if let Some(total_earned) = extracted {
                    object.insert("totalEarned".to_string(), Value::from(total_earned));
                }
            }
        }
        Err(error) => {
            warn!(uid = %uid, error = %error, "account_user_page_adapter upstream failed");
        }
    }

    if let Ok(payload) = account_trade_bill_upstream(state, uid).await {
        let data = payload
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "bill".to_string(),
                json!({
                    "monthAmount": data.get("monthAmount").cloned().unwrap_or_else(|| Value::String("0.00".to_string())),
                    "monthOrderCount": data.get("monthOrderCount").cloned().unwrap_or_else(|| Value::from(0)),
                }),
            );
        }
    }

    if let Ok(payload) = account_seller_data_upstream(state, uid, "recent30d").await {
        let items = payload
            .get("data")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("graphBannerBenchData"))
            .and_then(|value| value.get("bannerDataList"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seller = Map::new();
        for item in items {
            let Some(name) = item.get("name").and_then(Value::as_str) else {
                continue;
            };
            seller.insert(
                name.to_string(),
                json!({
                    "value": item.get("dataStr").cloned().unwrap_or_else(|| Value::String("0".to_string())),
                    "cycle": item.get("cycle").cloned().unwrap_or_else(|| Value::String(String::new())),
                    "ratio": item.get("ratioFormat").cloned().unwrap_or_else(|| Value::String(String::new())),
                }),
            );
        }
        if let Some(object) = result.as_object_mut() {
            object.insert("seller30d".to_string(), Value::Object(seller));
        }
    }

    result
}

pub(super) async fn persist_account_summary_metrics(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    summary: &Value,
) -> Result<(), ResponseError> {
    let security_score = summary
        .get("securityScore")
        .and_then(Value::as_object)
        .and_then(|score| normalize_security_score_total(score.get("totalScore")));
    // 优先从 violations.list 中扫描 punishEndTime > now 的处罚，取最大者作为生效中处罚到期时间。
    // 闲鱼的 securityScore.restrictionEnd 字段经常缺失/为旧值，violation.list 是权威来源。
    let now_ms = unix_millis_now();
    let mut max_active_end: i64 = 0;
    if let Some(vlist) = summary
        .get("violations")
        .and_then(Value::as_object)
        .and_then(|v| v.get("list"))
        .and_then(Value::as_array)
    {
        for v in vlist {
            let end = v.get("punishEndTime").and_then(Value::as_i64).unwrap_or(0);
            if end > now_ms && end > max_active_end {
                max_active_end = end;
            }
        }
    }
    let restriction_end = if max_active_end > 0 {
        // 存成 yyyy-MM-dd 格式（CST/UTC+8）。手算不引 chrono：
        // days = (ms / 1000 + 8h) / 86400, 然后 days→year/month/day（Howard Hinnant 算法）
        let days_since_epoch = (max_active_end / 1000 + 8 * 3600) / 86400;
        let z = days_since_epoch + 719468;
        let era = z.div_euclid(146097);
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = (yoe as i64) + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
        let year = (y + if m <= 2 { 1 } else { 0 }) as i64;
        format!("{:04}-{:02}-{:02}", year, m, d)
    } else {
        // 回退到 securityScore.restrictionEnd（少数情况闲鱼仅在这下发）
        summary
            .get("securityScore")
            .and_then(Value::as_object)
            .and_then(|score| score.get("restrictionEnd"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let coin_balance = coerce_i64(summary.get("coinBalance"));
    let violation_count = summary
        .get("violations")
        .and_then(Value::as_object)
        .and_then(|violations| violations.get("totalCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let fish_power = coerce_i64(summary.get("fishPower"));
    let total_earned = summary
        .get("totalEarned")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let fish_shop_enabled = summary
        .get("fishShopEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let fish_shop_level = summary
        .get("fishShopLevel")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let seller_30d_sold = summary
        .get("seller30d")
        .and_then(Value::as_object)
        .and_then(|seller| seller.get("已卖出"))
        .and_then(Value::as_object)
        .and_then(|sold| sold.get("value"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let now = unix_seconds_now();

    sqlx::query(
        r#"
        UPDATE accounts
        SET security_score = COALESCE(?, security_score),
            security_score_updated_at = CASE WHEN ? IS NULL THEN security_score_updated_at ELSE ? END,
            restriction_end = CASE WHEN ? = '' THEN restriction_end ELSE ? END,
            coin_balance = ?,
            violation_count = ?,
            fish_power = ?,
            total_earned = CASE WHEN ? <= 0 THEN total_earned ELSE ? END,
            fish_shop_enabled = CASE WHEN ? THEN 1 ELSE fish_shop_enabled END,
            fish_shop_level = CASE WHEN ? = '' THEN fish_shop_level ELSE ? END,
            seller_30d_sold = CASE WHEN ? = '' THEN seller_30d_sold ELSE ? END,
            updated_at = ?
        WHERE uid = ?
        "#,
    )
    .bind(security_score)
    .bind(security_score)
    .bind(now)
    .bind(&restriction_end)
    .bind(&restriction_end)
    .bind(coin_balance)
    .bind(violation_count)
    .bind(fish_power)
    .bind(total_earned)
    .bind(total_earned)
    .bind(fish_shop_enabled)
    .bind(&fish_shop_level)
    .bind(&fish_shop_level)
    .bind(&seller_30d_sold)
    .bind(&seller_30d_sold)
    .bind(now)
    .bind(uid)
    .execute(pool)
    .await
    .map_err(ResponseError::internal)?;

    Ok(())
}

pub(crate) async fn load_account_summary(
    state: AppState,
    uid: &str,
    force: bool,
) -> Result<Value, ResponseError> {
    let row = load_account_row(state.storage().accounts(), uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let extra = parse_json_object(row.get::<String, _>("extra").as_str());
    let cached = extra
        .get("summary_cache")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if cached.is_object() && !force && summary_has_business_signal(&cached) {
        return Ok(cached);
    }

    let mut summary = build_account_summary_live(&state, uid).await;
    if !summary.is_object() {
        summary = json!({ "uid": uid });
    }

    restore_summary_from_baseline(&mut summary, Some(&cached), &row);
    summary
        .as_object_mut()
        .ok_or_else(|| ResponseError::internal_message("invalid summary payload"))?
        .insert("uid".to_string(), Value::String(uid.to_string()));

    if !summary_has_business_signal(&summary) && account_row_has_business_signal(&row) {
        restore_summary_from_baseline(&mut summary, Some(&cached), &row);
    }

    let cached_at = unix_seconds_now();
    if let Some(object) = summary.as_object_mut() {
        object.insert("_cached_at".to_string(), Value::from(cached_at));
    }

    let mut next_extra = extra.clone();
    next_extra.insert("summary_cache".to_string(), summary.clone());
    persist_account_extra(state.storage().accounts(), uid, &next_extra).await?;
    persist_account_summary_metrics(state.storage().accounts(), uid, &summary).await?;

    Ok(summary)
}

pub(crate) async fn load_account_row(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
) -> Result<Option<sqlx::sqlite::SqliteRow>, sqlx::Error> {
    sqlx::query(
        r#"
        SELECT uid, nick, token, sid, cookies, browser_cookie_jar, status, created_at, updated_at, bound_at,
               cookie_refreshed_at, last_session_check_at, last_session_ok_at, first_session_fail_at,
               last_session_status, last_session_reason, extra, security_score, security_score_updated_at,
               restriction_end, coin_balance, violation_count, fish_power, total_earned,
               fish_shop_enabled, fish_shop_level, seller_30d_sold
        FROM accounts
        WHERE uid = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .fetch_optional(pool)
    .await
}

pub(crate) async fn account_extra_map(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
) -> Result<Option<Map<String, Value>>, sqlx::Error> {
    let row = sqlx::query("SELECT extra FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|row| parse_json_object(row.get::<String, _>("extra").as_str())))
}

pub(crate) async fn persist_account_extra(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    uid: &str,
    extra: &Map<String, Value>,
) -> Result<(), ResponseError> {
    let serialized = serde_json::to_string(extra).map_err(ResponseError::internal)?;
    sqlx::query("UPDATE accounts SET extra = ?, updated_at = ? WHERE uid = ?")
        .bind(serialized)
        .bind(unix_seconds_now())
        .bind(uid)
        .execute(pool)
        .await
        .map_err(ResponseError::internal)?;
    Ok(())
}

pub(crate) fn account_row_to_value(row: sqlx::sqlite::SqliteRow) -> Value {
    let cookies = parse_json_object(row.get::<String, _>("cookies").as_str());
    let browser_cookie_jar = parse_json_array(row.get::<String, _>("browser_cookie_jar").as_str());
    let extra = parse_json_object(row.get::<String, _>("extra").as_str());
    let summary_cached_at = extra
        .get("summary_cache")
        .and_then(Value::as_object)
        .and_then(|summary| summary.get("_cached_at"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let avatar = extra
        .get("avatar")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let profile_cache = extra
        .get("editable_profile_cache")
        .and_then(Value::as_object)
        .or_else(|| extra.get("summary_cache").and_then(Value::as_object));
    let display_nick = profile_cache
        .and_then(|profile| {
            [
                "displayNick",
                "displayName",
                "snsNick",
                "nick",
                "nickName",
                "nickname",
            ]
            .iter()
            .find_map(|key| {
                profile
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
            })
        })
        .unwrap_or_default();
    let location = extra.get("location").cloned().unwrap_or_else(|| json!({}));
    let reply_mode = account_reply_mode_from_extra(&extra);
    let auto_reply = reply_mode.auto_reply_enabled();
    let auto_ship = extra
        .get("autoShip")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let ai_enabled = reply_mode.ai_enabled();
    let auto_recall_item_replies = extra
        .get("autoRecallItemReplies")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let total_earned = row.get::<f64, _>("total_earned");
    let fish_shop_enabled = row.get::<i64, _>("fish_shop_enabled") > 0;
    let fish_shop_level = row.get::<String, _>("fish_shop_level");
    let cookie_refreshed_at = row.get::<f64, _>("cookie_refreshed_at");
    let age = if cookie_refreshed_at > 0.0 {
        (unix_seconds_now() - cookie_refreshed_at).max(0.0) as i64
    } else {
        0
    };
    let next_refresh_in = if cookie_refreshed_at > 0.0 {
        (BOOTSTRAP_REFRESH_INTERVAL - age).max(0)
    } else {
        0
    };

    let raw_status = row.get::<String, _>("status");
    let restriction_end_str = row.get::<String, _>("restriction_end");
    // 如果当前有生效中的禁发处罚（restriction_end 是未来日期），把 status 派生为 'restricted'。
    // restriction_end 现存格式 yyyy-MM-dd（写入逻辑见 persist_account_summary_metrics）。
    // 用日期字符串字典序比较是安全的（左对齐定长格式）。
    let today = {
        let secs = unix_seconds_now() as i64 + 8 * 3600;
        let days_since_epoch = secs.div_euclid(86400);
        let z = days_since_epoch + 719468;
        let era = z.div_euclid(146097);
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = (yoe as i64) + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
        let year = (y + if m <= 2 { 1 } else { 0 }) as i64;
        format!("{:04}-{:02}-{:02}", year, m, d)
    };
    let derived_status = if !restriction_end_str.trim().is_empty()
        && restriction_end_str.as_str() >= today.as_str()
        && raw_status == "active"
    {
        "restricted".to_string()
    } else {
        raw_status.clone()
    };
    let bound_at = row.get::<f64, _>("bound_at");
    let added_at = if bound_at > 0.0 {
        Value::from(bound_at)
    } else {
        Value::Null
    };

    json!({
        "uid": row.get::<String, _>("uid"),
        "nick": row.get::<String, _>("nick"),
        "displayNick": display_nick,
        "snsNick": display_nick,
        "avatar": avatar,
        "status": derived_status,
        "raw_status": raw_status,
        "has_h5_tk": cookies.contains_key("_m_h5_tk") || !row.get::<String, _>("token").trim().is_empty(),
        "cookie_count": cookies.len(),
        "browser_cookie_count": browser_cookie_jar.len(),
        "last_session_check_at": row.get::<f64, _>("last_session_check_at"),
        "last_session_ok_at": row.get::<f64, _>("last_session_ok_at"),
        "first_session_fail_at": row.get::<f64, _>("first_session_fail_at"),
        "last_session_status": row.get::<String, _>("last_session_status"),
        "last_session_reason": row.get::<String, _>("last_session_reason"),
        "created_at": row.get::<f64, _>("created_at"),
        "bound_at": bound_at,
        "added_at": added_at,
        "addedAt": added_at,
        "updated_at": row.get::<f64, _>("updated_at"),
        "cookie_refreshed_at": cookie_refreshed_at,
        "next_refresh_in": next_refresh_in,
        "location": location,
        "replyMode": reply_mode,
        "autoReply": auto_reply,
        "autoShip": auto_ship,
        "aiEnabled": ai_enabled,
        "autoRecallItemReplies": auto_recall_item_replies,
        "auto_recall_item_replies": auto_recall_item_replies,
        "security_score": row.get::<i64, _>("security_score"),
        "security_score_updated_at": row.get::<f64, _>("security_score_updated_at"),
        "restriction_end": row.get::<String, _>("restriction_end"),
        "coin_balance": row.get::<i64, _>("coin_balance"),
        "violation_count": row.get::<i64, _>("violation_count"),
        "fish_power": row.get::<i64, _>("fish_power"),
        "seller_30d_sold": row.get::<String, _>("seller_30d_sold"),
        "summaryCachedAt": summary_cached_at,
        "totalEarned": total_earned,
        "fishShopEnabled": fish_shop_enabled,
        "fishShopLevel": fish_shop_level,
    })
}

fn account_reply_mode_from_extra(extra: &Map<String, Value>) -> ReplyMode {
    if let Some(raw) = extra.get("replyMode").and_then(Value::as_str) {
        return ReplyMode::from_account_raw(raw);
    }
    if extra
        .get("aiEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ReplyMode::Ai;
    }
    if extra
        .get("autoReply")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ReplyMode::Default;
    }
    ReplyMode::Off
}

pub(crate) async fn account_row_to_value_with_publish_location(
    accounts: &sqlx::Pool<sqlx::Sqlite>,
    row: sqlx::sqlite::SqliteRow,
) -> Value {
    let uid = row.get::<String, _>("uid");
    let mut value = account_row_to_value(row);
    if let Ok(location) = load_location(accounts, &uid).await {
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "location".to_string(),
                json!({
                    "fishAdCode": location.fish_ad_code,
                    "gps": location.gps,
                    "latitude": location.latitude,
                    "longitude": location.longitude,
                    "name": location.name,
                    "address": location.address,
                    "province": location.province,
                    "city": location.city,
                    "area": location.area,
                    "divisionId": location.fish_ad_code,
                }),
            );
        }
    }
    value
}
