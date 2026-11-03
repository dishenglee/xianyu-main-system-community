use axum::{
    extract::{Extension, Json, Query, State},
    Json as ResponseJson,
};
use rand::seq::IndexedRandom;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sqlx::Row;

use crate::{
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    protocol_runtime::resolve_protocol,
    routes::accounts::shared::{first_ret_string, TransportKind, TransportSelector},
    routes::error::{ApiResult, ResponseError},
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
};

use super::{
    load_location_or_default, success_ret, successful_upstream_app_request,
    successful_upstream_h5_request, successful_upstream_wx_request, UidQuery,
};

async fn request_search_transport(
    state: &AppState,
    uid: &str,
    transport: TransportKind,
    pc_payload: &Value,
    wx_payload: &Value,
    app_payload: &Value,
) -> Result<Value, String> {
    match transport {
        TransportKind::App => {
            let protocol = resolve_protocol(state, opcodes::SEARCH_APP).await?;
            successful_upstream_app_request(
                state,
                uid,
                protocol.api(),
                protocol.version(),
                app_payload.clone(),
            )
            .await
        }
        TransportKind::H5 => {
            let protocol = resolve_protocol(state, opcodes::SEARCH_PC).await?;
            successful_upstream_h5_request(
                state,
                uid,
                protocol.api(),
                protocol.version(),
                pc_payload.clone(),
                Vec::new(),
            )
            .await
        }
        TransportKind::Wx => {
            let protocol = resolve_protocol(state, opcodes::SEARCH_WX).await?;
            successful_upstream_wx_request(
                state,
                uid,
                protocol.api(),
                protocol.version(),
                wx_payload.clone(),
            )
            .await
        }
    }
}

async fn pick_random_active_uid(state: &AppState, owner_username: Option<&str>) -> Option<String> {
    let rows = sqlx::query(
        "SELECT uid FROM accounts WHERE status = 'active' AND (? IS NULL OR owner_username = ?)",
    )
    .bind(owner_username)
    .bind(owner_username)
    .fetch_all(state.storage().accounts())
    .await
    .ok()?;
    let uids: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>("uid").ok())
        .collect();
    uids.choose(&mut rand::rng()).cloned()
}

#[derive(Debug, Deserialize)]
pub(super) struct SearchPayload {
    uid: String,
    keyword: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_page_size_50")]
    page_size: i64,
    #[serde(default)]
    source_mode: String,
    #[serde(default)]
    sort_field: String,
    #[serde(default)]
    sort_value: String,
    #[serde(default)]
    prop_value_str: Value,
    // 附近搜索字段——前端 LocationPicker 选完 POI 把 latitude/longitude 传过来，
    // distance 是搜索半径（米，例 "5000"）。三者都非空才在 PC payload 里塞 gps。
    #[serde(default)]
    latitude: String,
    #[serde(default)]
    longitude: String,
    #[serde(default)]
    distance: String,
    #[serde(default)]
    region_filter: Value,
}

fn default_page() -> i64 {
    1
}

fn default_page_size_50() -> i64 {
    50
}

fn search_has_items(payload: &Value) -> bool {
    payload
        .get("data")
        .and_then(Value::as_object)
        .and_then(|data| data.get("resultList"))
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false)
}

fn search_has_dynamic_filters(payload: &Value) -> bool {
    let data = payload.get("data").and_then(Value::as_object);
    let smart_has_values = data
        .and_then(|object| object.get("smartUiFilterPanelDO"))
        .and_then(Value::as_object)
        .and_then(|object| object.get("filterPanelDOList"))
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    let flex_has_values = data
        .and_then(|object| object.get("flexFilter"))
        .and_then(Value::as_object)
        .and_then(|object| object.get("data"))
        .and_then(Value::as_array)
        .map(|items| !items.is_empty())
        .unwrap_or(false);
    smart_has_values || flex_has_values
}

fn merge_filter_metadata(mut base: Value, filter_source: &Value) -> Value {
    let Some(base_data) = base.get_mut("data").and_then(Value::as_object_mut) else {
        return base;
    };
    let Some(filter_data) = filter_source.get("data").and_then(Value::as_object) else {
        return base;
    };
    if !base_data.contains_key("flexFilter") {
        if let Some(flex) = filter_data.get("flexFilter") {
            base_data.insert("flexFilter".to_string(), flex.clone());
        }
    }
    let base_smart_empty = base_data
        .get("smartUiFilterPanelDO")
        .and_then(Value::as_object)
        .and_then(|object| object.get("filterPanelDOList"))
        .and_then(Value::as_array)
        .map(|items| items.is_empty())
        .unwrap_or(true);
    if base_smart_empty {
        if let Some(smart) = filter_data.get("smartUiFilterPanelDO") {
            base_data.insert("smartUiFilterPanelDO".to_string(), smart.clone());
        }
    }
    base
}

fn decorate_search_result(
    payload: &Value,
    requested: &str,
    source: &str,
    attempted: Vec<&str>,
    switch_suggested: bool,
    recommended: &str,
) -> Value {
    let mut object = payload.as_object().cloned().unwrap_or_default();
    object.insert("source".to_string(), Value::String(source.to_string()));
    object.insert(
        "meta".to_string(),
        json!({
            "requested_source_mode": requested,
            "actual_source": source,
            "attempted_sources": attempted,
            "switch_source_suggested": switch_suggested,
            "recommended_source_mode": recommended,
            "has_items": search_has_items(payload),
            "ret": first_ret_string(payload),
        }),
    );
    Value::Object(object)
}

fn normalize_pc_prop_value(value: &Value) -> Value {
    match value {
        Value::Null => json!({}),
        Value::Object(_) => value.clone(),
        Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                json!({})
            } else if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                if parsed.is_object() {
                    parsed
                } else if let Some(text) = parsed.as_str() {
                    json!({ "searchFilter": text })
                } else {
                    json!({})
                }
            } else {
                json!({ "searchFilter": trimmed })
            }
        }
        _ => json!({}),
    }
}

fn build_search_pc_payload(payload: &SearchPayload) -> Value {
    let normalized_prop = normalize_pc_prop_value(&payload.prop_value_str);
    let has_filters = !payload.sort_field.trim().is_empty()
        || !payload.sort_value.trim().is_empty()
        || normalized_prop
            .as_object()
            .map(|object| !object.is_empty())
            .unwrap_or(false);
    // 附近搜索：lat/lng 都非空时填 gps + customGps + customDistance + userPositionJson。
    // 闲鱼 PC 搜索的 GPS 字段格式 = "lng,lat"（经纬反过来），跟前端 LocationPicker
    // 返的 latitude/longitude 字段语义对齐——我们这里组装时只需拼一下。
    let has_gps = !payload.latitude.trim().is_empty() && !payload.longitude.trim().is_empty();
    let gps_str = if has_gps {
        format!("{},{}", payload.longitude.trim(), payload.latitude.trim())
    } else {
        String::new()
    };
    let distance_str = if has_gps && !payload.distance.trim().is_empty() {
        payload.distance.trim().to_string()
    } else {
        String::new()
    };
    let user_position_json = if has_gps {
        serde_json::to_string(&json!({
            "latitude": payload.latitude,
            "longitude": payload.longitude,
        }))
        .unwrap_or_else(|_| "{}".to_string())
    } else {
        "{}".to_string()
    };
    json!({
        "pageNumber": payload.page.max(1),
        "keyword": payload.keyword,
        "fromFilter": has_filters || has_gps,
        "rowsPerPage": payload.page_size.clamp(1, 50),
        "sortValue": payload.sort_value,
        "sortField": payload.sort_field,
        "customDistance": distance_str,
        "gps": gps_str.clone(),
        "propValueStr": normalized_prop,
        "customGps": gps_str,
        "searchReqFromPage": "pcSearch",
        "extraFilterValue": "{}",
        "userPositionJson": user_position_json,
    })
}

fn build_search_wx_payload(payload: &SearchPayload) -> Value {
    let prop = match &payload.prop_value_str {
        Value::String(raw) => raw.trim().to_string(),
        Value::Object(object) => serde_json::to_string(object).unwrap_or_else(|_| "{}".to_string()),
        _ => String::new(),
    };
    let has_filters = !payload.sort_field.trim().is_empty() || !prop.is_empty();
    let mut object = Map::new();
    object.insert("utdid".to_string(), Value::String("h5_client".to_string()));
    object.insert("platform".to_string(), Value::String("ios".to_string()));
    object.insert(
        "miniAppVersion".to_string(),
        Value::String("0.3.78".to_string()),
    );
    object.insert(
        "keyword".to_string(),
        Value::String(payload.keyword.clone()),
    );
    object.insert("pageNumber".to_string(), Value::from(payload.page.max(1)));
    object.insert(
        "rowsPerPage".to_string(),
        Value::from(payload.page_size.clamp(1, 50)),
    );
    object.insert(
        "sortField".to_string(),
        Value::String(payload.sort_field.clone()),
    );
    object.insert(
        "sortValue".to_string(),
        Value::String(payload.sort_value.clone()),
    );
    object.insert("fromFilter".to_string(), Value::Bool(has_filters));
    object.insert("bizFrom".to_string(), Value::String("miniHome".to_string()));
    if has_filters {
        object.insert("fromCombo".to_string(), Value::String("Filter".to_string()));
    }
    if !prop.is_empty() {
        object.insert("propValueStr".to_string(), Value::String(prop));
    }
    Value::Object(object)
}

fn build_search_app_payload(payload: &SearchPayload) -> Value {
    let prop = match &payload.prop_value_str {
        Value::String(raw) => raw.trim().to_string(),
        Value::Object(object) => serde_json::to_string(object).unwrap_or_else(|_| "{}".to_string()),
        _ => String::new(),
    };
    let region_filter = payload
        .region_filter
        .as_object()
        .cloned()
        .unwrap_or_default();
    let has_region_filter = !region_filter.is_empty();
    let has_filters =
        !payload.sort_field.trim().is_empty() || !prop.is_empty() || has_region_filter;
    let mut object = Map::new();
    object.insert(
        "keyword".to_string(),
        Value::String(payload.keyword.clone()),
    );
    object.insert("pageNumber".to_string(), Value::from(payload.page.max(1)));
    object.insert("page".to_string(), Value::from(payload.page.max(1)));
    object.insert(
        "rowsPerPage".to_string(),
        Value::from(payload.page_size.clamp(1, 50)),
    );
    object.insert("fromFilter".to_string(), Value::Bool(has_filters));
    object.insert("mainTab".to_string(), Value::Bool(true));
    object.insert("originJson".to_string(), Value::Bool(false));
    object.insert("activeSearch".to_string(), Value::Bool(false));
    object.insert(
        "searchTabType".to_string(),
        Value::String("SEARCH_TAB_MAIN".to_string()),
    );
    object.insert("bizFrom".to_string(), Value::String("home".to_string()));
    object.insert(
        "searchReqFromPage".to_string(),
        Value::String("xyHome".to_string()),
    );
    object.insert(
        "apiName".to_string(),
        Value::String("com.taobao.idlefish.search_implement.protocol.SearchResultReq".to_string()),
    );
    object.insert("disableHierarchicalSort".to_string(), Value::from(0));
    object.insert("smartUIFilter".to_string(), Value::Bool(true));
    object.insert("supportFlexFilter".to_string(), Value::Bool(true));
    object.insert("resultListLastIndex".to_string(), Value::from(0));
    object.insert("relateResultListLastIndex".to_string(), Value::from(0));
    object.insert("relateResultPageNumber".to_string(), Value::from(1));
    let has_gps = !payload.latitude.trim().is_empty() && !payload.longitude.trim().is_empty();
    let fish_ad_code = region_filter
        .get("fishAdCode")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let region_division_id = region_filter
        .get("divisionId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let region_province = region_filter
        .get("province")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let region_city = region_filter
        .get("city")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let region_district = region_filter
        .get("district")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let extra_filter_value = region_filter
        .get("extraFilterValue")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if has_region_filter {
                let mut region_object = Map::new();
                if !region_filter
                    .get("distance")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .is_empty()
                {
                    let poi_name = region_filter
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    let display_name = [region_city.clone(), poi_name.clone()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("·");
                    let poi_gps = format!(
                        "{},{}",
                        payload.latitude.trim(),
                        payload.longitude.trim()
                    );
                    region_object.insert(
                        "poiList".to_string(),
                        json!([{
                            "area": region_district,
                            "city": region_city,
                            "displayName": display_name,
                            "divisionId": if !region_division_id.is_empty() { region_division_id.clone() } else { fish_ad_code.clone() },
                            "gps": poi_gps,
                            "poiName": poi_name,
                            "prov": region_province,
                        }]),
                    );
                } else {
                    let mut division = Map::new();
                    if !region_city.is_empty() {
                        division.insert("city".to_string(), Value::String(region_city.clone()));
                    }
                    if !region_province.is_empty() {
                        division.insert("province".to_string(), Value::String(region_province.clone()));
                    }
                    if !region_district.is_empty() {
                        division.insert("area".to_string(), Value::String(region_district.clone()));
                    }
                    region_object.insert("divisionList".to_string(), Value::Array(vec![Value::Object(division)]));
                }
                serde_json::to_string(&Value::Object(region_object))
                    .unwrap_or_else(|_| r#"{"divisionList":[]}"#.to_string())
            } else {
                r#"{"divisionList":[],"excludeMultiPlacesSellers":"0"}"#.to_string()
            }
        });
    object.insert(
        "extraFilterValue".to_string(),
        Value::String(extra_filter_value),
    );
    if has_gps || has_region_filter {
        let latitude = region_filter
            .get("latitude")
            .and_then(Value::as_str)
            .unwrap_or(payload.latitude.trim());
        let longitude = region_filter
            .get("longitude")
            .and_then(Value::as_str)
            .unwrap_or(payload.longitude.trim());
        if !fish_ad_code.is_empty() {
            object.insert("fishAdCode".to_string(), Value::String(fish_ad_code));
        }
        if !latitude.is_empty() {
            object.insert("latitude".to_string(), Value::String(latitude.to_string()));
        }
        if !longitude.is_empty() {
            object.insert(
                "longitude".to_string(),
                Value::String(longitude.to_string()),
            );
        }
        if !latitude.is_empty() && !longitude.is_empty() {
            let gps_str = format!("{latitude},{longitude}");
            object.insert("gps".to_string(), Value::String(gps_str.clone()));
            let custom_gps = region_filter
                .get("customGps")
                .and_then(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .unwrap_or(&gps_str);
            object.insert(
                "customGps".to_string(),
                Value::String(custom_gps.to_string()),
            );
        }
        if let Some(distance) = region_filter
            .get("customDistance")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                region_filter
                    .get("distance")
                    .and_then(Value::as_str)
                    .filter(|v| !v.trim().is_empty())
            })
            .or_else(|| (!payload.distance.trim().is_empty()).then_some(payload.distance.trim()))
        {
            object.insert(
                "customDistance".to_string(),
                Value::String(distance.to_string()),
            );
        }
        let user_position_json = region_filter
            .get("userPositionJson")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                serde_json::to_string(&json!({
                    "city": region_city,
                    "district": region_district,
                    "province": region_province,
                }))
                .unwrap_or_else(|_| "{}".to_string())
            });
        if !user_position_json.trim().is_empty() {
            object.insert(
                "userPositionJson".to_string(),
                Value::String(user_position_json),
            );
        }
    }
    object.insert(
        "abConfigs".to_string(),
        Value::String(
            r#"{"isEnterResultNative":"true","isSearchResultNative":"true"}"#.to_string(),
        ),
    );
    object.insert("forceUseInputKeyword".to_string(), Value::Bool(false));
    object.insert("forceUseTppRepair".to_string(), Value::Bool(false));
    object.insert("fromKits".to_string(), Value::Bool(false));
    object.insert("fromLeaf".to_string(), Value::Bool(false));
    object.insert("fromShade".to_string(), Value::Bool(false));
    object.insert("fromSuggest".to_string(), Value::Bool(false));
    object.insert(
        "searchReqFromActivatePagePart".to_string(),
        Value::String("searchButton".to_string()),
    );
    object.insert(
        "shadeBucketNum".to_string(),
        Value::String("-1".to_string()),
    );
    object.insert(
        "suggestBucketNum".to_string(),
        Value::String("29".to_string()),
    );
    if has_filters {
        if !payload.sort_field.trim().is_empty() {
            object.insert(
                "sortField".to_string(),
                Value::String(payload.sort_field.clone()),
            );
            object.insert(
                "sortValue".to_string(),
                Value::String(payload.sort_value.clone()),
            );
        }
        if !prop.is_empty() {
            object.insert("propValueStr".to_string(), Value::String(prop));
        }
    }
    Value::Object(object)
}

pub(super) async fn search(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<SearchPayload>,
) -> ApiResult {
    let selector = TransportSelector::parse(&payload.source_mode)
        .ok_or_else(|| ResponseError::bad_request("source_mode 仅支持 auto / app / pc / wx"))?;
    let requested_mode = match selector {
        TransportSelector::Auto => "auto",
        TransportSelector::H5 => "pc",
        TransportSelector::Wx => "wx",
        TransportSelector::App => "app",
    };

    let owner = owner_username_filter(&state, &identity);
    let uid = if payload.uid.trim().is_empty() {
        pick_random_active_uid(&state, owner.as_deref())
            .await
            .ok_or_else(|| ResponseError::bad_request("没有可用的活跃账号"))?
    } else {
        ensure_account_uid_accessible(
            state.storage().accounts(),
            owner.as_deref(),
            payload.uid.trim(),
        )
        .await?;
        payload.uid.trim().to_string()
    };

    let pc_payload = build_search_pc_payload(&payload);
    let wx_payload = build_search_wx_payload(&payload);
    let app_payload = build_search_app_payload(&payload);
    let attempted = selector
        .fallback_chain()
        .iter()
        .map(|transport| match transport {
            TransportKind::App => "app_native",
            TransportKind::H5 => "h5_pc",
            TransportKind::Wx => "wx_miniprogram",
        })
        .collect::<Vec<_>>();
    let mut last_error = None;

    for transport in selector.fallback_chain() {
        let result = match request_search_transport(
            &state,
            &uid,
            *transport,
            &pc_payload,
            &wx_payload,
            &app_payload,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                last_error = Some((*transport, error));
                continue;
            }
        };
        let result =
            if matches!(transport, TransportKind::H5) && !search_has_dynamic_filters(&result) {
                if let Ok(filter_payload) = request_search_transport(
                    &state,
                    &uid,
                    TransportKind::Wx,
                    &pc_payload,
                    &wx_payload,
                    &app_payload,
                )
                .await
                {
                    merge_filter_metadata(result, &filter_payload)
                } else {
                    result
                }
            } else {
                result
            };
        let has_items = search_has_items(&result);
        let chosen = match transport {
            TransportKind::App => "app_native",
            TransportKind::H5 => "h5_pc",
            TransportKind::Wx => "wx_miniprogram",
        };
        let fallback_hint = match transport {
            TransportKind::App if !has_items => "pc",
            TransportKind::H5 if !has_items => "wx",
            TransportKind::Wx if !has_items && selector != TransportSelector::Auto => "pc",
            TransportKind::Wx if !has_items => "app",
            _ => "",
        };

        if selector != TransportSelector::Auto
            || has_items
            || matches!(transport, TransportKind::Wx)
        {
            return Ok(ResponseJson(decorate_search_result(
                &result,
                requested_mode,
                chosen,
                attempted.clone(),
                !has_items,
                fallback_hint,
            )));
        }
    }

    if let Some((transport, error)) = last_error {
        let label = match transport {
            TransportKind::App => "APP搜索通道失败",
            TransportKind::H5 => "PC搜索通道失败",
            TransportKind::Wx => "微信搜索通道失败",
        };
        return Err(ResponseError::bad_request(&format!("{label}: {error}")));
    }

    Err(ResponseError::bad_request("所有搜索通道均失败，请稍后重试"))
}

pub(super) async fn search_shade(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ResponseJson<Value> {
    let owner = owner_username_filter(&state, &identity);
    if ensure_account_uid_accessible(
        state.storage().accounts(),
        owner.as_deref(),
        query.uid.trim(),
    )
    .await
    .is_err()
    {
        return ResponseJson(json!({
            "ret": ["FAIL_SYS_FORBIDDEN::账号不存在"],
            "data": {}
        }));
    }
    let location = load_location_or_default(&state, &query.uid).await;
    if let Ok(protocol) = resolve_protocol(&state, opcodes::SEARCH_SHADE).await {
        if let Ok(payload) = successful_upstream_h5_request(
            &state,
            &query.uid,
            protocol.api(),
            protocol.version(),
            json!({
                "fishAdCode": location.fish_ad_code,
                "gps": location.gps,
                "latitude": location.latitude,
                "longitude": location.longitude,
                "fromNewMainTab": true,
            }),
            Vec::new(),
        )
        .await
        {
            return ResponseJson(payload);
        }
    }
    ResponseJson(json!({
        "ret": success_ret(),
        "data": {
            "shadeWords": ["MacBook", "显示器", "人体工学椅", "镜头", "露营装备"],
        }
    }))
}
