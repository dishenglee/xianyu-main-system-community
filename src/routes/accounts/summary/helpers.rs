use serde_json::{Map, Value};

pub(super) fn normalize_security_score_total(value: Option<&Value>) -> Option<i64> {
    let Some(value) = value else {
        return None;
    };
    let score = if let Some(value) = value.as_i64() {
        value
    } else if let Some(value) = value.as_str() {
        value.parse::<i64>().ok()?
    } else {
        return None;
    };
    (score >= 0).then_some(score)
}

pub(super) fn coerce_i64(value: Option<&Value>) -> i64 {
    value.and_then(Value::as_i64).unwrap_or_else(|| {
        value
            .and_then(Value::as_str)
            .and_then(|text| text.parse::<i64>().ok())
            .unwrap_or(0)
    })
}

pub(super) fn parse_first_decimal_from_text(text: &str) -> Option<f64> {
    let mut buf = String::new();
    let mut seen_digit = false;
    let mut seen_dot = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
            seen_digit = true;
            continue;
        }
        if ch == '.' && seen_digit && !seen_dot {
            buf.push(ch);
            seen_dot = true;
            continue;
        }
        if seen_digit {
            break;
        }
    }
    if !seen_digit {
        return None;
    }
    buf.parse::<f64>().ok()
}

pub(super) fn extract_total_earned_from_adapter_payload(payload: &Value) -> Option<f64> {
    let sections = payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("container"))
        .and_then(Value::as_object)
        .and_then(|container| container.get("sections"))
        .and_then(Value::as_array)?;

    for section in sections {
        let Some(item) = section.get("item").and_then(Value::as_object) else {
            continue;
        };
        let Some(top) = item.get("top").and_then(Value::as_object) else {
            continue;
        };
        let Some(ex) = top.get("exContent").and_then(Value::as_object) else {
            continue;
        };
        if let Some(sub_title) = ex.get("subTitle").and_then(Value::as_str) {
            if sub_title.contains("在闲鱼赚了") {
                if let Some(value) = parse_first_decimal_from_text(sub_title) {
                    return Some(value);
                }
            }
        }
        let title = ex.get("title").and_then(Value::as_str).unwrap_or_default();
        if title == "我的交易" && !ex.contains_key("subTitle") {
            let sell_count = item
                .get("sell")
                .and_then(Value::as_object)
                .and_then(|sell| sell.get("exContent"))
                .and_then(Value::as_object)
                .and_then(|sell| sell.get("value"))
                .and_then(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
                })
                .unwrap_or(0);
            let buy_count = item
                .get("buy")
                .and_then(Value::as_object)
                .and_then(|buy| buy.get("exContent"))
                .and_then(Value::as_object)
                .and_then(|buy| buy.get("value"))
                .and_then(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str().and_then(|s| s.parse::<i64>().ok()))
                })
                .unwrap_or(0);
            if sell_count == 0 && buy_count == 0 {
                return Some(0.0);
            }
        }
    }
    None
}

pub(super) fn extract_fish_power_from_adapter_payload(payload: &Value) -> Option<i64> {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("base"))
        .and_then(Value::as_object)
        .and_then(|base| base.get("cs"))
        .and_then(Value::as_object)
        .and_then(|cs| cs.get("score"))
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<i64>().ok()))
        })
}

pub(super) fn has_amount_signal(value: Option<&Value>) -> bool {
    let Some(value) = value else {
        return false;
    };
    let text = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or_default();
    if text.is_empty() {
        return false;
    }
    text.parse::<f64>()
        .map(|amount| amount > 0.0)
        .unwrap_or(false)
}

pub(super) fn has_trustworthy_security_score(payload: Option<&Value>) -> bool {
    let Some(payload) = payload.and_then(Value::as_object) else {
        return false;
    };
    let Some(score) = normalize_security_score_total(payload.get("totalScore")) else {
        return false;
    };
    if score != 0 {
        return true;
    }
    payload
        .get("restrictionEnd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
        || payload.get("lastChange").is_some()
}

pub(super) fn summary_has_business_signal(payload: &Value) -> bool {
    let Some(payload) = payload.as_object() else {
        return false;
    };
    has_trustworthy_security_score(payload.get("securityScore"))
        || coerce_i64(payload.get("coinBalance")) > 0
        || coerce_i64(payload.get("fishPower")) > 0
        || coerce_i64(
            payload
                .get("violations")
                .and_then(Value::as_object)
                .and_then(|violations| violations.get("totalCount")),
        ) > 0
        || payload
            .get("bill")
            .and_then(Value::as_object)
            .map(|bill| {
                has_amount_signal(bill.get("monthAmount"))
                    || coerce_i64(bill.get("monthOrderCount")) > 0
            })
            .unwrap_or(false)
        || payload
            .get("seller30d")
            .and_then(Value::as_object)
            .map(|seller| !seller.is_empty())
            .unwrap_or(false)
        || payload
            .get("totalEarned")
            .and_then(Value::as_f64)
            .map(|value| value > 0.0)
            .unwrap_or(false)
        || payload
            .get("fishShopEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

pub(super) fn account_row_has_business_signal(row: &sqlx::sqlite::SqliteRow) -> bool {
    use sqlx::Row;
    row.get::<i64, _>("security_score") >= 0
        || row.get::<i64, _>("coin_balance") > 0
        || row.get::<i64, _>("fish_power") > 0
        || row.get::<i64, _>("violation_count") > 0
        || !row.get::<String, _>("seller_30d_sold").trim().is_empty()
        || row.get::<f64, _>("total_earned") > 0.0
        || row.get::<i64, _>("fish_shop_enabled") > 0
        || !row.get::<String, _>("fish_shop_level").trim().is_empty()
}

pub(super) fn extract_fish_shop_info_from_user_head(
    payload: &Value,
) -> Option<(bool, Option<String>)> {
    let shop = payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("module"))
        .and_then(Value::as_object)
        .and_then(|module| module.get("base"))
        .and_then(Value::as_object)
        .and_then(|base| base.get("shop"))
        .and_then(Value::as_object)?;

    let enabled = !shop.is_empty();
    let level = shop
        .get("level")
        .and_then(Value::as_str)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with("Lv.") {
                value.to_string()
            } else if let Some(raw) = value.strip_prefix('L') {
                format!("Lv.{raw}")
            } else {
                format!("Lv.{value}")
            }
        })
        .or_else(|| {
            shop.get("levelPic")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|value| {
                    let bytes = value.as_bytes();
                    for idx in 0..bytes.len() {
                        if bytes[idx] != b'L' {
                            continue;
                        }
                        let mut end = idx + 1;
                        while end < bytes.len()
                            && (bytes[end].is_ascii_digit() || bytes[end] == b'.')
                        {
                            end += 1;
                        }
                        if end > idx + 1 {
                            return Some(value[idx..end].to_string());
                        }
                    }
                    None
                })
        })
        .filter(|value| !value.is_empty());

    Some((enabled, level))
}

pub(super) fn fill_positive_number(object: &mut Map<String, Value>, key: &str, value: i64) {
    if object.get(key).and_then(Value::as_i64).unwrap_or(0) <= 0 && value > 0 {
        object.insert(key.to_string(), Value::from(value));
    }
}

pub(super) fn fill_score_records(object: &mut Map<String, Value>) {
    if !object
        .get("scoreRecords")
        .map(Value::is_array)
        .unwrap_or(false)
    {
        object.insert("scoreRecords".to_string(), Value::Array(Vec::new()));
    }
}

pub(super) fn fill_violations(object: &mut Map<String, Value>, total: i64) {
    let violations = object
        .entry("violations".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !violations.is_object() {
        *violations = serde_json::json!({});
    }
    if let Some(violations) = violations.as_object_mut() {
        if violations
            .get("totalCount")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            <= 0
            && total > 0
        {
            violations.insert("totalCount".to_string(), Value::from(total));
        }
        violations
            .entry("hasNext".to_string())
            .or_insert_with(|| Value::Bool(false));
        if !violations.get("list").map(Value::is_array).unwrap_or(false) {
            violations.insert("list".to_string(), Value::Array(Vec::new()));
        }
    }
}

pub(super) fn fill_bill(object: &mut Map<String, Value>) {
    let bill = object
        .entry("bill".to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !bill.is_object() {
        *bill = serde_json::json!({});
    }
    if let Some(bill) = bill.as_object_mut() {
        bill.entry("monthAmount".to_string())
            .or_insert_with(|| Value::String("0.00".to_string()));
        bill.entry("monthOrderCount".to_string())
            .or_insert_with(|| Value::from(0));
    }
}

pub(super) fn fill_seller_30d(object: &mut Map<String, Value>, sold: String) {
    if object
        .get("seller30d")
        .map(Value::is_object)
        .unwrap_or(false)
        || sold.trim().is_empty()
    {
        return;
    }
    object.insert(
        "seller30d".to_string(),
        serde_json::json!({
            "已卖出": {
                "value": sold,
                "cycle": "",
                "ratio": "",
            }
        }),
    );
}
