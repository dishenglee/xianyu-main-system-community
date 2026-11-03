use serde_json::{json, Map, Value};
use sqlx::Row;

use crate::{protocol_catalog::opcodes, state::AppState};

use super::super::{
    load_location_or_default, location_payload, resolve_account_protocol,
    successful_upstream_protocol_app_request, successful_upstream_protocol_h5_request,
    successful_upstream_protocol_wx_request, unix_millis_now,
};

pub(super) async fn account_security_score_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("appName".to_string(), Value::String("FM".to_string()));
    data.insert("scoreType".to_string(), Value::from(1));
    data.insert("queryScoreLimit".to_string(), Value::Bool(false));
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_SECURITY_SCORE,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_security_score_records_upstream(
    state: &AppState,
    uid: &str,
    page: i64,
    page_size: i64,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("appName".to_string(), Value::String("FM".to_string()));
    data.insert("scoreType".to_string(), Value::from(1));
    data.insert("pageIndex".to_string(), Value::from(page.max(1)));
    data.insert("pageSize".to_string(), Value::from(page_size.max(1)));
    data.insert(
        "scoreChangeTypeList".to_string(),
        Value::String("[2,1,3,5]".to_string()),
    );
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_SECURITY_SCORE_RECORDS,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_violations_upstream(
    state: &AppState,
    uid: &str,
    page_size: i64,
    in_90_days: bool,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    let now = unix_millis_now();
    data.insert("biz".to_string(), Value::String("FM".to_string()));
    data.insert("pageSize".to_string(), Value::from(page_size.max(1)));
    data.insert("endTime".to_string(), Value::from(now));
    data.insert("in90Days".to_string(), Value::Bool(in_90_days));
    if in_90_days {
        data.insert(
            "beginTime".to_string(),
            Value::from(now - 90 * 86_400_000_i64),
        );
        // 之前传 statusList="0" 会过滤出"已结束"处罚，导致永远拉不到生效中的处罚 →
        // restriction_end 永远算不出来。这里去掉过滤，拉全量再自己按 punishEndTime 派生生效状态。
    }
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_SECURITY_VIOLATIONS,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_security_detection_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("appName".to_string(), Value::String("FM".to_string()));
    data.insert(
        "callback".to_string(),
        Value::String(
            crate::sealed_protocol::sealed_str!(
                "https://h5.m.goofish.com/app/msd/buyer-aqcenter/home.html"
            )
            .to_string(),
        ),
    );
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_SECURITY_DETECTION,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_qualification_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("appName".to_string(), Value::String("FM".to_string()));
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_QUALIFICATION,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_coin_balance_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_COIN_BALANCE,
        Value::Object(location_payload(&location)),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_coin_detail_upstream(
    state: &AppState,
    uid: &str,
    page: i64,
    page_size: i64,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("pageNum".to_string(), Value::from(page.max(1)));
    data.insert("pageSize".to_string(), Value::from(page_size.max(1)));
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_COIN_DETAIL,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn coin_account_info_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert(
        "pointProductCode".to_string(),
        Value::String("IDLE_COIN".to_string()),
    );
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_COIN_BANK,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_seller_data_upstream(
    state: &AppState,
    uid: &str,
    date_type: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    let mut data = location_payload(&location);
    data.insert("dateRange".to_string(), Value::String(String::new()));
    data.insert("dateType".to_string(), Value::String(date_type.to_string()));
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::DATACOMPASS_SELLER_SUMMARY,
        Value::Object(data),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_user_page_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    successful_upstream_protocol_wx_request(
        state,
        uid,
        opcodes::ACCOUNT_WX_USER_PAGE_MY,
        json!({
            "utdid": "h5_client",
            "platform": "ios",
            "miniAppVersion": "0.3.77",
        }),
    )
    .await
}

pub(super) async fn account_user_page_adapter_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let row = sqlx::query("SELECT nick, cookies FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid)
        .fetch_optional(state.storage().accounts())
        .await
        .map_err(|error| error.to_string())?;
    let (stored_nick, cookies) = row
        .map(|row| {
            (
                row.get::<String, _>("nick"),
                row.get::<String, _>("cookies"),
            )
        })
        .unwrap_or_else(|| (String::new(), "{}".to_string()));
    let nick = resolve_account_user_page_adapter_nick(&cookies, &stored_nick, uid);
    let protocol = resolve_account_protocol(state, opcodes::ACCOUNT_USER_PAGE_ADAPTER).await?;
    let data = build_account_user_page_adapter_payload(
        &nick,
        unix_millis_now(),
        protocol.api(),
        protocol.version(),
    );

    successful_upstream_protocol_app_request(state, uid, opcodes::ACCOUNT_USER_PAGE_ADAPTER, data)
        .await
}

fn build_account_user_page_adapter_payload(
    nick: &str,
    now_millis: i64,
    api: &str,
    version: &str,
) -> Value {
    json!({
        "apiName": api,
        "apiVersioin": version,
        "containerParams": json!({
            "xianyu_my_main": {
                "baseCacheTime": now_millis,
                "passParams": {
                    "lastVersion": "v2",
                },
                "realBaseCacheTime": now_millis,
                "requestType": "coldStart",
            }
        })
        .to_string(),
        "mode": "2025",
        "nick": nick,
        "originJson": false,
    })
}

fn resolve_account_user_page_adapter_nick(
    cookies_raw: &str,
    stored_nick: &str,
    uid: &str,
) -> String {
    let cookies = serde_json::from_str::<Map<String, Value>>(cookies_raw).unwrap_or_default();
    for key in ["tracknick", "lgc", "_nk_"] {
        let Some(raw) = cookies.get(key).and_then(Value::as_str) else {
            continue;
        };
        let decoded = decode_cookie_nick(raw);
        if !decoded.is_empty() {
            return decoded;
        }
    }
    let trimmed = stored_nick.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    uid.to_string()
}

fn decode_cookie_nick(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    urlencoding::decode(trimmed)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_account_user_page_adapter_payload, resolve_account_user_page_adapter_nick};

    #[test]
    fn adapter_payload_matches_real_capture_shape() {
        let payload = build_account_user_page_adapter_payload(
            "大碗宽面哎",
            1_775_886_041_246,
            crate::sealed_protocol::sealed_str!("mtop.idle.user.page.my.adapter"),
            "1.0",
        );

        assert_eq!(
            payload["apiName"],
            crate::sealed_protocol::sealed_str!("mtop.idle.user.page.my.adapter")
        );
        assert_eq!(payload["apiVersioin"], "1.0");
        assert_eq!(payload["mode"], "2025");
        assert_eq!(payload["nick"], "大碗宽面哎");
        assert_eq!(payload["originJson"], false);

        let container = payload["containerParams"]
            .as_str()
            .expect("containerParams string");
        let parsed: serde_json::Value =
            serde_json::from_str(container).expect("containerParams JSON");
        assert_eq!(parsed["xianyu_my_main"]["passParams"]["lastVersion"], "v2");
        assert_eq!(parsed["xianyu_my_main"]["requestType"], "coldStart");
        assert_eq!(
            parsed["xianyu_my_main"]["baseCacheTime"],
            1_775_886_041_246_i64
        );
    }

    #[test]
    fn adapter_nick_prefers_percent_encoded_tracknick() {
        let nick = resolve_account_user_page_adapter_nick(
            r#"{"tracknick":"%E5%A4%A7%E7%A2%97%E5%AE%BD%E9%9D%A2%E5%93%8E"}"#,
            "测试用户甲",
            "90000000001",
        );

        assert_eq!(nick, "大碗宽面哎");
    }

    #[test]
    fn adapter_nick_falls_back_to_stored_nick() {
        let nick = resolve_account_user_page_adapter_nick("{}", "测试用户乙", "90000000002");
        assert_eq!(nick, "测试用户乙");
    }
}

pub(super) async fn account_user_head_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    successful_upstream_protocol_app_request(
        state,
        uid,
        opcodes::ACCOUNT_USER_PAGE_HEAD,
        json!({
            "userId": uid,
        }),
    )
    .await
}

pub(super) async fn account_user_check_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_USER_CHECK,
        json!({}),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_alipay_upstream(state: &AppState, uid: &str) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_ALIPAY,
        Value::Object(location_payload(&location)),
        Vec::new(),
    )
    .await
}

pub(super) async fn account_trade_bill_upstream(
    state: &AppState,
    uid: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, uid).await;
    successful_upstream_protocol_h5_request(
        state,
        uid,
        opcodes::ACCOUNT_BILL_LIST,
        Value::Object(location_payload(&location)),
        Vec::new(),
    )
    .await
}
