mod helpers;
mod persist;
mod upstream;

use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    routes::accounts::shared::parse_json_object,
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
};

use super::{
    load_location_or_default, location_payload, payload_ret_success, resolve_account_protocol,
    unix_seconds_now, upstream_protocol_h5_request, AccountSummaryQuery, ApiResult,
    CoinDetailQuery, PageQuery, ResponseError, SellerDataQuery, UidQuery, ViolationsAllQuery,
};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccountProfileUpdateBody {
    #[serde(default)]
    uid: String,
    #[serde(default, alias = "snsNick")]
    sns_nick: Option<String>,
    #[serde(default, alias = "defaultNick")]
    default_nick: Option<Value>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default, alias = "signatureTitle")]
    signature_title: Option<String>,
    #[serde(default)]
    avatar: Option<String>,
    #[serde(default)]
    gender: Option<Value>,
    #[serde(default)]
    birthday: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    university: Option<String>,
}

pub(crate) use persist::{
    account_extra_map, account_row_to_value, account_row_to_value_with_publish_location,
    load_account_row, load_account_summary, persist_account_extra,
};

async fn ensure_summary_uid_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid.trim()).await
}

pub(super) async fn account_summary(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<AccountSummaryQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    let summary = load_account_summary(state.clone(), &query.uid, query.force).await?;
    Ok(ResponseJson(summary))
}

pub(super) async fn account_profile(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<AccountSummaryQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    let summary = load_account_summary(state.clone(), &query.uid, query.force).await?;
    Ok(ResponseJson(summary))
}

pub(super) async fn account_profile_editable(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<AccountSummaryQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;

    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let stored_nick = row.get::<String, _>("nick");
    let extra = parse_json_object(row.get::<String, _>("extra").as_str());
    let cached = editable_profile_cache_from_extra(&extra);
    let stored_avatar = cached
        .get("avatar")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let upstream = upstream::account_user_head_upstream(&state, &query.uid)
        .await
        .ok();
    let source_api = resolve_account_protocol(&state, opcodes::ACCOUNT_USER_PAGE_HEAD)
        .await
        .map_err(ResponseError::service_unavailable)?;

    Ok(ResponseJson(build_editable_profile_payload(
        &query.uid,
        upstream.as_ref(),
        &cached,
        &stored_nick,
        &stored_avatar,
        source_api.api(),
    )))
}

pub(super) async fn account_profile_update(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(body): Json<AccountProfileUpdateBody>,
) -> ApiResult {
    let uid = body.uid.trim();
    if uid.is_empty() {
        return Err(ResponseError::bad_request("uid required"));
    }
    ensure_summary_uid_access(&state, &identity, uid).await?;

    let profile_fields = profile_update_fields(&body);
    if profile_fields.is_empty() {
        return Err(ResponseError::bad_request(
            "at least one editable profile field required",
        ));
    }

    let location = load_location_or_default(&state, uid).await;
    let mut data = location_payload(&location);
    for (key, value) in profile_fields.clone() {
        data.insert(key, value);
    }

    let protocol = resolve_account_protocol(&state, opcodes::ACCOUNT_PROFILE_UPDATE)
        .await
        .map_err(ResponseError::service_unavailable)?;
    let upstream = upstream_protocol_h5_request(
        &state,
        uid,
        opcodes::ACCOUNT_PROFILE_UPDATE,
        Value::Object(data),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;

    if payload_ret_success(&upstream) {
        let mut extra = account_extra_map(state.storage().accounts(), uid)
            .await
            .map_err(ResponseError::internal)?
            .unwrap_or_default();

        if let Some(avatar) = profile_fields.get("avatar").cloned() {
            extra.insert("avatar".to_string(), avatar);
        }

        let mut editable_cache = editable_profile_cache_from_extra(&extra);
        for (key, value) in &profile_fields {
            editable_cache.insert(key.clone(), value.clone());
        }
        extra.insert(
            "editable_profile_cache".to_string(),
            Value::Object(editable_cache),
        );

        let summary_cache = extra
            .entry("summary_cache".to_string())
            .or_insert_with(|| json!({ "uid": uid }));
        if let Some(summary_object) = summary_cache.as_object_mut() {
            for (key, value) in &profile_fields {
                summary_object.insert(key.clone(), value.clone());
            }
        }

        persist_account_extra(state.storage().accounts(), uid, &extra).await?;

        if let Some(nick) = profile_fields
            .get("snsNick")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sqlx::query("UPDATE accounts SET nick = ?, updated_at = ? WHERE uid = ?")
                .bind(nick)
                .bind(unix_seconds_now())
                .bind(uid)
                .execute(state.storage().accounts())
                .await
                .map_err(ResponseError::internal)?;
        }
    }

    let mut response = Map::new();
    response.insert(
        "api".to_string(),
        upstream
            .get("api")
            .cloned()
            .unwrap_or_else(|| Value::String(protocol.api().to_string())),
    );
    response.insert(
        "ret".to_string(),
        upstream
            .get("ret")
            .cloned()
            .unwrap_or_else(|| json!(["FAIL_SYS_EMPTY_RET::上游未返回 ret"])),
    );
    response.insert(
        "data".to_string(),
        upstream.get("data").cloned().unwrap_or_else(|| json!({})),
    );
    response.insert("raw".to_string(), upstream);
    for (key, value) in profile_fields {
        response.insert(key, value);
    }

    Ok(ResponseJson(Value::Object(response)))
}

pub(super) async fn account_security_score(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_security_score_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    let summary = load_account_summary(state, &query.uid, false).await?;
    let payload = summary
        .get("securityScore")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(ResponseJson(json!({ "data": { "data": payload } })))
}

fn profile_update_fields(body: &AccountProfileUpdateBody) -> Map<String, Value> {
    let mut fields = Map::new();
    insert_trimmed(&mut fields, "snsNick", body.sns_nick.as_deref());
    insert_profile_value(&mut fields, "defaultNick", body.default_nick.as_ref());
    insert_trimmed(&mut fields, "signature", body.signature.as_deref());
    insert_trimmed(
        &mut fields,
        "signatureTitle",
        body.signature_title.as_deref(),
    );
    insert_trimmed(&mut fields, "avatar", body.avatar.as_deref());
    insert_profile_value(&mut fields, "gender", body.gender.as_ref());
    insert_trimmed(&mut fields, "birthday", body.birthday.as_deref());
    insert_trimmed(&mut fields, "city", body.city.as_deref());
    insert_trimmed(&mut fields, "university", body.university.as_deref());
    fields
}

fn insert_profile_value(fields: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    match value {
        Some(Value::String(value)) => insert_trimmed(fields, key, Some(value)),
        Some(Value::Bool(value)) => {
            fields.insert(key.to_string(), Value::String(value.to_string()));
        }
        Some(Value::Number(value)) => {
            fields.insert(key.to_string(), Value::String(value.to_string()));
        }
        _ => {}
    }
}

fn insert_trimmed(fields: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim) {
        fields.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn editable_profile_cache_from_extra(extra: &Map<String, Value>) -> Map<String, Value> {
    let mut cache = extra
        .get("editable_profile_cache")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if cache.is_empty() {
        if let Some(summary) = extra.get("summary_cache").and_then(Value::as_object) {
            for key in [
                "snsNick",
                "defaultNick",
                "signature",
                "signatureTitle",
                "avatar",
                "gender",
                "birthday",
                "city",
                "university",
                "likeTags",
                "birthdayInfo",
            ] {
                if let Some(value) = summary.get(key).cloned() {
                    cache.insert(key.to_string(), value);
                }
            }
        }
    }

    if !cache.contains_key("avatar") {
        if let Some(value) = extra.get("avatar").cloned() {
            cache.insert("avatar".to_string(), value);
        }
    }

    cache
}

fn build_editable_profile_payload(
    uid: &str,
    user_head_payload: Option<&Value>,
    cached: &Map<String, Value>,
    stored_nick: &str,
    stored_avatar: &str,
    user_head_api: &str,
) -> Value {
    let head_module = user_head_payload
        .and_then(|payload| payload.get("data"))
        .and_then(Value::as_object)
        .and_then(|data| data.get("module"))
        .and_then(Value::as_object);
    let head_base = head_module
        .and_then(|module| module.get("base"))
        .and_then(Value::as_object);
    let head_nav = head_module
        .and_then(|module| module.get("nav"))
        .and_then(Value::as_object);
    let head_intro = head_module
        .and_then(|module| module.get("introduction"))
        .and_then(Value::as_object);

    let birthday_info = first_non_empty_str([
        head_intro
            .and_then(|introduction| introduction.get("birthdayInfo"))
            .and_then(Value::as_str),
        cached.get("birthdayInfo").and_then(Value::as_str),
    ]);
    let gender = derive_gender_from_birthday_info(Some(birthday_info.as_str()))
        .or_else(|| {
            cached
                .get("gender")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_default();
    let source_api = if user_head_payload.is_some() {
        user_head_api
    } else {
        "local_cache"
    };

    json!({
        "uid": uid,
        "snsNick": first_non_empty_str([
            head_base
                .and_then(|base| base.get("displayName"))
                .and_then(Value::as_str),
            cached.get("snsNick").and_then(Value::as_str),
            Some(stored_nick),
            Some(uid),
        ]),
        "signature": first_non_empty_str([
            head_intro
                .and_then(|introduction| introduction.get("introduction"))
                .and_then(Value::as_str),
            head_nav
                .and_then(|nav| nav.get("summary"))
                .and_then(Value::as_str),
            cached.get("signature").and_then(Value::as_str),
        ]),
        "signatureTitle": first_non_empty_str([
            cached.get("signatureTitle").and_then(Value::as_str),
        ]),
        "avatar": first_non_empty_str([
            head_base
                .and_then(|base| base.get("avatar"))
                .and_then(Value::as_object)
                .and_then(|avatar| avatar.get("avatar"))
                .and_then(Value::as_str),
            cached.get("avatar").and_then(Value::as_str),
            Some(stored_avatar),
        ]),
        "gender": gender,
        "birthday": first_non_empty_str([
            cached.get("birthday").and_then(Value::as_str),
        ]),
        "city": first_non_empty_str([
            cached.get("city").and_then(Value::as_str),
        ]),
        "university": first_non_empty_str([
            cached.get("university").and_then(Value::as_str),
        ]),
        "defaultNick": cached.get("defaultNick").cloned().unwrap_or_else(|| Value::Bool(false)),
        "likeTags": first_non_empty_str([
            head_intro
                .and_then(|introduction| introduction.get("likeTags"))
                .and_then(Value::as_str),
            cached.get("likeTags").and_then(Value::as_str),
        ]),
        "birthdayInfo": birthday_info,
        "_sourceApi": source_api,
    })
}

fn first_non_empty_str<const N: usize>(values: [Option<&str>; N]) -> String {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn derive_gender_from_birthday_info(birthday_info: Option<&str>) -> Option<String> {
    let text = birthday_info?.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("女生") || text.contains("女") {
        return Some("2".to_string());
    }
    if text.contains("男生") || text.contains("男") {
        return Some("1".to_string());
    }
    None
}

pub(super) async fn account_security_score_records(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<PageQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) =
        upstream::account_security_score_records_upstream(&state, &query.uid, query.page, 200).await
    {
        return Ok(ResponseJson(payload));
    }
    let summary = load_account_summary(state, &query.uid, false).await?;
    let payload = summary
        .get("scoreRecords")
        .cloned()
        .unwrap_or_else(|| json!([]));
    Ok(ResponseJson(json!({ "data": { "data": payload } })))
}

pub(super) async fn account_violations(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_violations_upstream(&state, &query.uid, 20, true).await {
        return Ok(ResponseJson(payload));
    }
    let summary = load_account_summary(state, &query.uid, false).await?;
    let violations = summary
        .get("violations")
        .cloned()
        .unwrap_or_else(|| json!({ "totalCount": 0, "hasNext": false, "data": [] }));
    let total_count = violations
        .get("totalCount")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let list = violations
        .get("list")
        .or_else(|| violations.get("data"))
        .cloned()
        .unwrap_or_else(|| json!([]));

    Ok(ResponseJson(json!({
        "data": {
            "totalCount": total_count,
            "hasNext": false,
            "data": list,
        }
    })))
}

pub(super) async fn account_violations_all(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ViolationsAllQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) =
        upstream::account_violations_upstream(&state, &query.uid, query.page_size, false).await
    {
        return Ok(ResponseJson(payload));
    }
    account_violations(
        State(state),
        Extension(identity),
        Query(UidQuery { uid: query.uid }),
    )
    .await
}

pub(super) async fn account_security_detection(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_security_detection_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({ "data": { "data": [] } })))
}

pub(super) async fn account_qualification(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_qualification_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({ "data": { "data": {} } })))
}

pub(super) async fn account_coin_balance(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_coin_balance_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let balance = row.get::<i64, _>("coin_balance");
    Ok(ResponseJson(json!({ "data": { "myIdleCoin": balance } })))
}

pub(super) async fn account_coin_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CoinDetailQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) =
        upstream::account_coin_detail_upstream(&state, &query.uid, query.page, query.page_size)
            .await
    {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(
        json!({ "data": { "userScoreDetailDOList": [] } }),
    ))
}

pub(super) async fn account_seller_data(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SellerDataQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) =
        upstream::account_seller_data_upstream(&state, &query.uid, &query.date_type).await
    {
        return Ok(ResponseJson(payload));
    }
    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let sold = row.get::<String, _>("seller_30d_sold");
    let payload = json!({
        "data": {
            "data": {
                "graphBannerBenchData": {
                    "bannerDataList": if sold.trim().is_empty() {
                        Vec::<serde_json::Value>::new()
                    } else {
                        vec![json!({
                            "name": "已卖出",
                            "dataStr": sold,
                            "cycle": query.date_type,
                            "ratioFormat": "",
                        })]
                    }
                }
            }
        }
    });
    Ok(ResponseJson(payload))
}

pub(super) async fn account_user_page(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_user_page_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    Ok(ResponseJson(json!({
        "data": {
            "module": {
                "base": {
                    "creditScore": {
                        "score": row.get::<i64, _>("fish_power"),
                    }
                },
                "trade": {
                    "buyerOrderCount": 0,
                    "sellerOrderCount": 0,
                    "itemCount": 0,
                }
            }
        }
    })))
}

pub(super) async fn account_user_head(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_user_head_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    Ok(ResponseJson(json!({
        "data": {
            "nick": row.get::<String, _>("nick"),
            "creditLevel": row.get::<i64, _>("security_score"),
        }
    })))
}

pub(super) async fn account_user_check(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_user_check_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    Ok(ResponseJson(json!({
        "data": {
            "uid": row.get::<String, _>("uid"),
            "status": row.get::<String, _>("status"),
        }
    })))
}

pub(super) async fn account_alipay(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::account_alipay_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({ "data": { "bound": false } })))
}

pub(super) async fn coin_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    account_coin_balance(State(state), Extension(identity), Query(query)).await
}

pub(super) async fn coin_account(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    ensure_summary_uid_access(&state, &identity, &query.uid).await?;
    if let Ok(payload) = upstream::coin_account_info_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    let row = load_account_row(state.storage().accounts(), &query.uid)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("账号不存在"))?;
    let balance = row.get::<i64, _>("coin_balance");
    Ok(ResponseJson(json!({
        "balance": balance,
        "totalBalance": balance,
        "coinBalance": balance,
        "availableBalance": balance,
    })))
}

#[cfg(test)]
mod tests {
    use super::helpers::{
        extract_total_earned_from_adapter_payload, parse_first_decimal_from_text,
    };
    use super::persist::restore_summary_from_baseline;
    use serde_json::json;
    use sqlx::{Connection, Executor, Row, SqliteConnection};

    #[test]
    fn parses_total_earned_number_from_plain_text() {
        assert_eq!(
            parse_first_decimal_from_text("在闲鱼赚了34670.78元"),
            Some(34670.78)
        );
        assert_eq!(parse_first_decimal_from_text("赚了0.01元"), Some(0.01));
        assert_eq!(parse_first_decimal_from_text("无金额"), None);
    }

    #[test]
    fn extracts_total_earned_from_adapter_trade_top_subtitle() {
        let payload = json!({
            "data": {
                "container": {
                    "sections": [
                        {
                            "item": {
                                "top": {
                                    "exContent": {
                                        "income": "3.46万",
                                        "subTitle": "在闲鱼赚了34670.78元"
                                    }
                                }
                            }
                        }
                    ]
                }
            }
        });

        assert_eq!(
            extract_total_earned_from_adapter_payload(&payload),
            Some(34670.78)
        );
    }

    #[test]
    fn treats_trade_section_without_subtitle_and_zero_counts_as_zero_earned() {
        let payload = json!({
            "data": {
                "container": {
                    "sections": [
                        {
                            "item": {
                                "top": {
                                    "exContent": {
                                        "title": "我的交易"
                                    }
                                },
                                "sell": {
                                    "exContent": {
                                        "title": "我卖出的",
                                        "value": 0
                                    }
                                },
                                "buy": {
                                    "exContent": {
                                        "title": "我买到的",
                                        "value": 0
                                    }
                                }
                            }
                        }
                    ]
                }
            }
        });

        assert_eq!(
            extract_total_earned_from_adapter_payload(&payload),
            Some(0.0)
        );
    }

    #[test]
    fn editable_profile_prefers_user_head_fields_over_summary_snapshot() {
        let payload = json!({
            "data": {
                "module": {
                    "nav": {
                        "summary": "为你解决毕业的烦恼，",
                        "displayNick": "xy404558342290"
                    },
                    "introduction": {
                        "introduction": "长期做毕业辅导，回复快。",
                        "likeTags": "喜欢宠物、摄影、绿植、音乐",
                        "birthdayInfo": "女生 90后 天蝎座"
                    },
                    "base": {
                        "displayName": "夏朝滑冰木莲",
                        "avatar": {
                            "avatar": "http://img.alicdn.com/avatar-head.jpg"
                        }
                    }
                }
            }
        });

        let cached = json!({
            "signatureTitle": "旧标题",
            "city": "长沙",
            "university": "湖南财政经济学院",
            "gender": "1",
            "birthday": "1998-01-01"
        });

        let profile = super::build_editable_profile_payload(
            "2217329279865",
            Some(&payload),
            cached.as_object().expect("cached object"),
            "专业毕业辅导",
            "http://img.alicdn.com/fallback-avatar.jpg",
            crate::sealed_protocol::sealed_str!("mtop.idle.user.page.head"),
        );

        assert_eq!(
            profile.get("snsNick").and_then(|value| value.as_str()),
            Some("夏朝滑冰木莲")
        );
        assert_eq!(
            profile.get("signature").and_then(|value| value.as_str()),
            Some("长期做毕业辅导，回复快。")
        );
        assert_eq!(
            profile.get("avatar").and_then(|value| value.as_str()),
            Some("http://img.alicdn.com/avatar-head.jpg")
        );
        assert_eq!(
            profile.get("gender").and_then(|value| value.as_str()),
            Some("2")
        );
        assert_eq!(
            profile
                .get("signatureTitle")
                .and_then(|value| value.as_str()),
            Some("旧标题")
        );
        assert_eq!(
            profile.get("city").and_then(|value| value.as_str()),
            Some("长沙")
        );
        assert_eq!(
            profile.get("university").and_then(|value| value.as_str()),
            Some("湖南财政经济学院")
        );
        assert_eq!(
            profile.get("birthday").and_then(|value| value.as_str()),
            Some("1998-01-01")
        );
        assert_eq!(
            profile.get("likeTags").and_then(|value| value.as_str()),
            Some("喜欢宠物、摄影、绿植、音乐")
        );
        assert_eq!(
            profile.get("birthdayInfo").and_then(|value| value.as_str()),
            Some("女生 90后 天蝎座")
        );
        assert_eq!(
            profile.get("_sourceApi").and_then(|value| value.as_str()),
            Some(crate::sealed_protocol::sealed_str!(
                "mtop.idle.user.page.head"
            ))
        );
    }

    #[test]
    fn editable_profile_falls_back_to_cached_and_stored_values_when_user_head_missing() {
        let cached = json!({
            "snsNick": "缓存昵称",
            "signature": "缓存简介",
            "signatureTitle": "缓存标题",
            "avatar": "http://img.alicdn.com/cached-avatar.jpg",
            "gender": "1",
            "city": "株洲"
        });

        let profile = super::build_editable_profile_payload(
            "90000000001",
            None,
            cached.as_object().expect("cached object"),
            "测试用户甲",
            "",
            crate::sealed_protocol::sealed_str!("mtop.idle.user.page.head"),
        );

        assert_eq!(
            profile.get("snsNick").and_then(|value| value.as_str()),
            Some("缓存昵称")
        );
        assert_eq!(
            profile.get("signature").and_then(|value| value.as_str()),
            Some("缓存简介")
        );
        assert_eq!(
            profile
                .get("signatureTitle")
                .and_then(|value| value.as_str()),
            Some("缓存标题")
        );
        assert_eq!(
            profile.get("avatar").and_then(|value| value.as_str()),
            Some("http://img.alicdn.com/cached-avatar.jpg")
        );
        assert_eq!(
            profile.get("gender").and_then(|value| value.as_str()),
            Some("1")
        );
        assert_eq!(
            profile.get("city").and_then(|value| value.as_str()),
            Some("株洲")
        );
        assert_eq!(
            profile.get("_sourceApi").and_then(|value| value.as_str()),
            Some("local_cache")
        );
    }

    #[tokio::test]
    async fn restore_summary_keeps_cached_total_earned_when_live_missing() {
        let mut conn = SqliteConnection::connect(":memory:").await.unwrap();
        conn.execute(
            r#"
            CREATE TABLE accounts (
                uid TEXT PRIMARY KEY,
                nick TEXT NOT NULL DEFAULT '',
                token TEXT NOT NULL DEFAULT '',
                sid TEXT NOT NULL DEFAULT '',
                cookies TEXT NOT NULL DEFAULT '{}',
                browser_cookie_jar TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'active',
                created_at REAL NOT NULL DEFAULT 0,
                updated_at REAL NOT NULL DEFAULT 0,
                bound_at REAL NOT NULL DEFAULT 0,
                cookie_refreshed_at REAL NOT NULL DEFAULT 0,
                last_session_check_at REAL NOT NULL DEFAULT 0,
                last_session_ok_at REAL NOT NULL DEFAULT 0,
                first_session_fail_at REAL NOT NULL DEFAULT 0,
                last_session_status TEXT NOT NULL DEFAULT '',
                last_session_reason TEXT NOT NULL DEFAULT '',
                extra TEXT NOT NULL DEFAULT '{}',
                security_score INTEGER NOT NULL DEFAULT -1,
                security_score_updated_at REAL NOT NULL DEFAULT 0,
                restriction_end TEXT NOT NULL DEFAULT '',
                coin_balance INTEGER NOT NULL DEFAULT 0,
                violation_count INTEGER NOT NULL DEFAULT 0,
                fish_power INTEGER NOT NULL DEFAULT 0,
                total_earned REAL NOT NULL DEFAULT 0,
                fish_shop_enabled INTEGER NOT NULL DEFAULT 0,
                fish_shop_level TEXT NOT NULL DEFAULT '',
                seller_30d_sold TEXT NOT NULL DEFAULT ''
            )
            "#,
        )
        .await
        .unwrap();
        conn.execute("INSERT INTO accounts (uid) VALUES ('u1')")
            .await
            .unwrap();
        let row = sqlx::query("SELECT * FROM accounts WHERE uid = 'u1'")
            .fetch_one(&mut conn)
            .await
            .unwrap();

        let mut summary = json!({ "uid": "u1" });
        let cached = json!({ "totalEarned": 34670.8 });
        restore_summary_from_baseline(&mut summary, Some(&cached), &row);

        assert_eq!(
            summary.get("totalEarned").and_then(|value| value.as_f64()),
            Some(34670.8)
        );
    }

    #[tokio::test]
    async fn account_row_exposes_summary_cache_fields_for_list_rendering() {
        let mut conn = SqliteConnection::connect(":memory:").await.unwrap();
        conn.execute(
            r#"
            CREATE TABLE accounts (
                uid TEXT PRIMARY KEY,
                nick TEXT NOT NULL DEFAULT '',
                token TEXT NOT NULL DEFAULT '',
                sid TEXT NOT NULL DEFAULT '',
                cookies TEXT NOT NULL DEFAULT '{}',
                browser_cookie_jar TEXT NOT NULL DEFAULT '[]',
                status TEXT NOT NULL DEFAULT 'active',
                created_at REAL NOT NULL DEFAULT 0,
                updated_at REAL NOT NULL DEFAULT 0,
                bound_at REAL NOT NULL DEFAULT 0,
                cookie_refreshed_at REAL NOT NULL DEFAULT 0,
                last_session_check_at REAL NOT NULL DEFAULT 0,
                last_session_ok_at REAL NOT NULL DEFAULT 0,
                first_session_fail_at REAL NOT NULL DEFAULT 0,
                last_session_status TEXT NOT NULL DEFAULT '',
                last_session_reason TEXT NOT NULL DEFAULT '',
                extra TEXT NOT NULL DEFAULT '{}',
                security_score INTEGER NOT NULL DEFAULT -1,
                security_score_updated_at REAL NOT NULL DEFAULT 0,
                restriction_end TEXT NOT NULL DEFAULT '',
                coin_balance INTEGER NOT NULL DEFAULT 0,
                violation_count INTEGER NOT NULL DEFAULT 0,
                fish_power INTEGER NOT NULL DEFAULT 0,
                total_earned REAL NOT NULL DEFAULT 0,
                fish_shop_enabled INTEGER NOT NULL DEFAULT 0,
                fish_shop_level TEXT NOT NULL DEFAULT '',
                seller_30d_sold TEXT NOT NULL DEFAULT ''
            )
            "#,
        )
        .await
        .unwrap();
        conn.execute(
            r#"
            INSERT INTO accounts (
                uid, extra, coin_balance, security_score, fish_power, violation_count,
                total_earned, fish_shop_enabled, fish_shop_level, seller_30d_sold
            ) VALUES (
                'u1',
                '{"summary_cache":{"_cached_at":1779550312.04485,"totalEarned":1977.14,"fishShopEnabled":true,"fishShopLevel":"Lv.7"}}',
                51605,
                70,
                788,
                6,
                1977.14,
                1,
                'Lv.7',
                '12'
            )
            "#,
        )
        .await
        .unwrap();

        let row = sqlx::query("SELECT * FROM accounts WHERE uid = 'u1'")
            .fetch_one(&mut conn)
            .await
            .unwrap();

        let value = super::account_row_to_value(row);

        assert_eq!(
            value.get("fishShopEnabled").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            value.get("fishShopLevel").and_then(|v| v.as_str()),
            Some("Lv.7")
        );
        assert_eq!(
            value.get("totalEarned").and_then(|v| v.as_f64()),
            Some(1977.14)
        );
        assert_eq!(
            value.get("summaryCachedAt").and_then(|v| v.as_f64()),
            Some(1779550312.04485)
        );
    }
}
