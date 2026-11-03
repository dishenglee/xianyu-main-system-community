use axum::{
    extract::{Query, State},
    routing::get,
    Extension, Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    protocol_runtime::resolve_protocol,
    routes::accounts::shared::{
        request_transport, TransportKind, TransportRequest, TransportRequestOptions,
    },
    routes::{
        error::ResponseError,
        tenant::{ensure_account_uid_accessible, owner_username_filter},
    },
    state::AppState,
    xianyu_client::{self, LocationContext},
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/location/divisions"),
            get(location_divisions),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/location/division_children"),
            get(location_division_children),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/location/by_gps"),
            get(location_by_gps),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/location/poi_search"),
            get(poi_search),
        )
}

#[derive(Debug, Deserialize)]
struct UidQuery {
    uid: String,
}

#[derive(Debug, Deserialize)]
struct DivisionChildrenQuery {
    uid: String,
    parent_id: i64,
}

#[derive(Debug, Deserialize)]
struct GpsQuery {
    uid: String,
    lat: f64,
    lng: f64,
}

#[derive(Debug, Deserialize)]
struct PoiQuery {
    uid: String,
    keyword: String,
}

async fn load_location_or_default(state: &AppState, uid: &str) -> LocationContext {
    xianyu_client::load_location(state.storage().accounts(), uid)
        .await
        .unwrap_or_default()
}

fn success_array(payload: &Value) -> Value {
    payload
        .get("ret")
        .cloned()
        .unwrap_or_else(|| Value::Array(success_ret().into_iter().map(Value::from).collect()))
}

fn location_common_payload(location: &LocationContext) -> Map<String, Value> {
    Map::from_iter([
        (
            "fishAdCode".to_string(),
            Value::String(location.fish_ad_code.clone()),
        ),
        ("gps".to_string(), Value::String(location.gps.clone())),
        (
            "latitude".to_string(),
            Value::String(location.latitude.clone()),
        ),
        (
            "longitude".to_string(),
            Value::String(location.longitude.clone()),
        ),
        ("utdid".to_string(), Value::String("xianyu-web".to_string())),
        ("platform".to_string(), Value::String("h5".to_string())),
        (
            "miniAppVersion".to_string(),
            Value::String("0.3.77".to_string()),
        ),
    ])
}

async fn location_divisions_upstream(state: &AppState, uid: &str) -> Result<Value, String> {
    let protocol = resolve_protocol(state, opcodes::ACCOUNT_DIVISION_ALL_GET).await?;
    let payload = request_transport(
        state,
        uid,
        &TransportRequest::new(
            TransportKind::H5,
            protocol.api(),
            protocol.version(),
            Value::Object(Map::from_iter([
                ("utdid".to_string(), Value::String("xianyu-web".to_string())),
                ("platform".to_string(), Value::String("h5".to_string())),
                (
                    "miniAppVersion".to_string(),
                    Value::String("0.3.77".to_string()),
                ),
            ])),
        ),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await?
    .payload;
    Ok(json!({
        "ret": success_array(&payload),
        "divisionArea": payload
            .get("data")
            .and_then(|value| value.get("divisionArea"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    }))
}

async fn location_division_children_upstream(
    state: &AppState,
    uid: &str,
    parent_id: i64,
) -> Result<Value, String> {
    let protocol = resolve_protocol(state, opcodes::ACCOUNT_DIVISION_ALL_GET).await?;
    let mut data = Map::from_iter([
        ("utdid".to_string(), Value::String("xianyu-web".to_string())),
        ("platform".to_string(), Value::String("h5".to_string())),
        (
            "miniAppVersion".to_string(),
            Value::String("0.3.77".to_string()),
        ),
    ]);
    if parent_id > 0 {
        data.insert("divisionId".to_string(), Value::from(parent_id));
    }
    let payload = request_transport(
        state,
        uid,
        &TransportRequest::new(
            TransportKind::H5,
            protocol.api(),
            protocol.version(),
            Value::Object(data),
        ),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await?
    .payload;
    Ok(json!({
        "ret": success_array(&payload),
        "items": payload
            .get("data")
            .and_then(|value| value.get("divisionArea"))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new())),
    }))
}

async fn location_by_gps_upstream(
    state: &AppState,
    uid: &str,
    lat: f64,
    lng: f64,
) -> Result<Value, String> {
    let protocol = resolve_protocol(state, opcodes::ACCOUNT_DIVISION_BY_GPS).await?;
    let location = load_location_or_default(state, uid).await;
    let mut data = location_common_payload(&location);
    data.insert("latitude".to_string(), Value::from(lat));
    data.insert("longitude".to_string(), Value::from(lng));
    let payload = request_transport(
        state,
        uid,
        &TransportRequest::new(
            TransportKind::H5,
            protocol.api(),
            protocol.version(),
            Value::Object(data),
        ),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await?
    .payload;
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    Ok(json!({
        "ret": success_array(&payload),
        "prov": value_to_string_lossy(data.get("prov")),
        "city": value_to_string_lossy(data.get("city")),
        "area": value_to_string_lossy(data.get("area")),
        "divisionId": value_to_string_lossy(data.get("divisionId")),
    }))
}

async fn poi_search_upstream(state: &AppState, uid: &str, keyword: &str) -> Result<Value, String> {
    let protocol = resolve_protocol(state, opcodes::ACCOUNT_LOCAL_POI_SEARCH).await?;
    let location = load_location_or_default(state, uid).await;
    let mut data = location_common_payload(&location);
    data.insert("city".to_string(), Value::String(location.city.clone()));
    data.insert("keywords".to_string(), Value::String(keyword.to_string()));
    let payload = request_transport(
        state,
        uid,
        &TransportRequest::new(
            TransportKind::H5,
            protocol.api(),
            protocol.version(),
            Value::Object(data),
        ),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await?
    .payload;
    let items = extract_poi_items(&payload);
    if items.is_empty() {
        return Err("upstream poi search returned no items".to_string());
    }
    Ok(json!({
        "ret": success_array(&payload),
        "items": items,
    }))
}

fn extract_poi_items(payload: &Value) -> Vec<Value> {
    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let candidates = [
        "searchPois",
        "poiList",
        "pois",
        "poiInfos",
        "poiInfoList",
        "items",
        "result",
        "results",
        "list",
    ]
    .into_iter()
    .find_map(|key| data.get(key).and_then(Value::as_array).cloned())
    .or_else(|| {
        ["result", "data", "model"].into_iter().find_map(|key| {
            data.get(key).and_then(Value::as_object).and_then(|object| {
                ["searchPois", "poiList", "pois", "items", "list"]
                    .into_iter()
                    .find_map(|nested| object.get(nested).and_then(Value::as_array).cloned())
            })
        })
    })
    .unwrap_or_default();

    candidates
        .into_iter()
        .filter_map(|candidate| candidate.as_object().cloned())
        .map(|candidate| {
            let location = candidate.get("location").cloned().unwrap_or(Value::Null);
            let (longitude, latitude) = if let Some(raw) = location.as_str() {
                let mut parts = raw.split(',').map(str::trim);
                (
                    parts.next().unwrap_or_default().to_string(),
                    parts.next().unwrap_or_default().to_string(),
                )
            } else if let Some(location) = location.as_object() {
                (
                    value_to_string_lossy(location.get("lng")),
                    value_to_string_lossy(location.get("lat")),
                )
            } else {
                (
                    value_to_string_lossy(
                        candidate.get("lng").or_else(|| candidate.get("longitude")),
                    ),
                    value_to_string_lossy(
                        candidate.get("lat").or_else(|| candidate.get("latitude")),
                    ),
                )
            };

            json!({
                "poi": first_non_empty(&[
                    candidate.get("poi"),
                    candidate.get("name"),
                    candidate.get("poiName"),
                    candidate.get("title"),
                ]),
                "address": first_non_empty(&[
                    candidate.get("address"),
                    candidate.get("addr"),
                    candidate.get("detailAddress"),
                ]),
                "prov": first_non_empty(&[
                    candidate.get("prov"),
                    candidate.get("province"),
                    candidate.get("pname"),
                ]),
                "city": first_non_empty(&[
                    candidate.get("city"),
                    candidate.get("cityName"),
                    candidate.get("cityname"),
                ]),
                "area": first_non_empty(&[
                    candidate.get("area"),
                    candidate.get("district"),
                    candidate.get("districtName"),
                    candidate.get("adname"),
                ]),
                "divisionId": first_non_empty(&[
                    candidate.get("divisionId"),
                    candidate.get("adcode"),
                    candidate.get("districtCode"),
                ]),
                "latitude": latitude,
                "longitude": longitude,
            })
        })
        .collect()
}

fn first_non_empty(values: &[Option<&Value>]) -> String {
    values
        .iter()
        .find_map(|value| {
            let value = value_to_string_lossy(*value);
            (!value.trim().is_empty()).then_some(value)
        })
        .unwrap_or_default()
}

fn value_to_string_lossy(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

async fn location_divisions(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> Result<ResponseJson<Value>, ResponseError> {
    ensure_location_uid_accessible(&state, &identity, &query.uid).await?;
    if let Ok(payload) = location_divisions_upstream(&state, &query.uid).await {
        return Ok(ResponseJson(payload));
    }
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "divisionArea": province_nodes(),
    })))
}

async fn location_division_children(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DivisionChildrenQuery>,
) -> Result<ResponseJson<Value>, ResponseError> {
    ensure_location_uid_accessible(&state, &identity, &query.uid).await?;
    if let Ok(payload) =
        location_division_children_upstream(&state, &query.uid, query.parent_id).await
    {
        return Ok(ResponseJson(payload));
    }
    let items = if query.parent_id <= 0 {
        province_nodes()
    } else {
        division_children(query.parent_id)
    };

    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "items": items,
    })))
}

async fn location_by_gps(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<GpsQuery>,
) -> Result<ResponseJson<Value>, ResponseError> {
    ensure_location_uid_accessible(&state, &identity, &query.uid).await?;
    if let Ok(payload) = location_by_gps_upstream(&state, &query.uid, query.lat, query.lng).await {
        return Ok(ResponseJson(payload));
    }
    let division = nearest_area(query.lat, query.lng);
    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "prov": division.prov,
        "city": division.city,
        "area": division.area,
        "divisionId": division.id.to_string(),
    })))
}

async fn poi_search(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<PoiQuery>,
) -> Result<ResponseJson<Value>, ResponseError> {
    ensure_location_uid_accessible(&state, &identity, &query.uid).await?;
    let keyword = query.keyword.trim();
    if !keyword.is_empty() {
        if let Ok(payload) = poi_search_upstream(&state, &query.uid, keyword).await {
            return Ok(ResponseJson(payload));
        }
    }
    let items = if keyword.is_empty() {
        Vec::new()
    } else {
        search_poi(keyword)
    };

    Ok(ResponseJson(json!({
        "ret": success_ret(),
        "items": items,
    })))
}

async fn ensure_location_uid_accessible(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid.trim()).await
}

#[derive(Clone, Copy)]
struct DivisionNode {
    id: i64,
    parent_id: i64,
    name: &'static str,
    level: &'static str,
    prov: &'static str,
    city: &'static str,
    area: &'static str,
    has_child: bool,
    latitude: f64,
    longitude: f64,
}

#[derive(Clone, Copy)]
struct PoiRecord {
    poi: &'static str,
    address: &'static str,
    prov: &'static str,
    city: &'static str,
    area: &'static str,
    division_id: i64,
    latitude: &'static str,
    longitude: &'static str,
    aliases: &'static [&'static str],
}

const DIVISIONS: &[DivisionNode] = &[
    DivisionNode {
        id: 110000,
        parent_id: 0,
        name: "北京市",
        level: "province",
        prov: "北京市",
        city: "",
        area: "",
        has_child: true,
        latitude: 39.9042,
        longitude: 116.4074,
    },
    DivisionNode {
        id: 110100,
        parent_id: 110000,
        name: "北京市",
        level: "city",
        prov: "北京市",
        city: "北京市",
        area: "",
        has_child: true,
        latitude: 39.9042,
        longitude: 116.4074,
    },
    DivisionNode {
        id: 110108,
        parent_id: 110100,
        name: "海淀区",
        level: "area",
        prov: "北京市",
        city: "北京市",
        area: "海淀区",
        has_child: false,
        latitude: 39.9593,
        longitude: 116.2981,
    },
    DivisionNode {
        id: 310000,
        parent_id: 0,
        name: "上海市",
        level: "province",
        prov: "上海市",
        city: "",
        area: "",
        has_child: true,
        latitude: 31.2304,
        longitude: 121.4737,
    },
    DivisionNode {
        id: 310100,
        parent_id: 310000,
        name: "上海市",
        level: "city",
        prov: "上海市",
        city: "上海市",
        area: "",
        has_child: true,
        latitude: 31.2304,
        longitude: 121.4737,
    },
    DivisionNode {
        id: 310115,
        parent_id: 310100,
        name: "浦东新区",
        level: "area",
        prov: "上海市",
        city: "上海市",
        area: "浦东新区",
        has_child: false,
        latitude: 31.2215,
        longitude: 121.5444,
    },
    DivisionNode {
        id: 320000,
        parent_id: 0,
        name: "江苏省",
        level: "province",
        prov: "江苏省",
        city: "",
        area: "",
        has_child: true,
        latitude: 32.0617,
        longitude: 118.7632,
    },
    DivisionNode {
        id: 320100,
        parent_id: 320000,
        name: "南京市",
        level: "city",
        prov: "江苏省",
        city: "南京市",
        area: "",
        has_child: true,
        latitude: 32.0603,
        longitude: 118.7969,
    },
    DivisionNode {
        id: 320106,
        parent_id: 320100,
        name: "鼓楼区",
        level: "area",
        prov: "江苏省",
        city: "南京市",
        area: "鼓楼区",
        has_child: false,
        latitude: 32.0722,
        longitude: 118.7690,
    },
    DivisionNode {
        id: 330000,
        parent_id: 0,
        name: "浙江省",
        level: "province",
        prov: "浙江省",
        city: "",
        area: "",
        has_child: true,
        latitude: 30.2741,
        longitude: 120.1551,
    },
    DivisionNode {
        id: 330100,
        parent_id: 330000,
        name: "杭州市",
        level: "city",
        prov: "浙江省",
        city: "杭州市",
        area: "",
        has_child: true,
        latitude: 30.2741,
        longitude: 120.1551,
    },
    DivisionNode {
        id: 330106,
        parent_id: 330100,
        name: "西湖区",
        level: "area",
        prov: "浙江省",
        city: "杭州市",
        area: "西湖区",
        has_child: false,
        latitude: 30.2596,
        longitude: 120.1302,
    },
    DivisionNode {
        id: 330200,
        parent_id: 330000,
        name: "宁波市",
        level: "city",
        prov: "浙江省",
        city: "宁波市",
        area: "",
        has_child: true,
        latitude: 29.8683,
        longitude: 121.5440,
    },
    DivisionNode {
        id: 330203,
        parent_id: 330200,
        name: "海曙区",
        level: "area",
        prov: "浙江省",
        city: "宁波市",
        area: "海曙区",
        has_child: false,
        latitude: 29.8592,
        longitude: 121.5503,
    },
    DivisionNode {
        id: 440000,
        parent_id: 0,
        name: "广东省",
        level: "province",
        prov: "广东省",
        city: "",
        area: "",
        has_child: true,
        latitude: 23.1291,
        longitude: 113.2644,
    },
    DivisionNode {
        id: 440300,
        parent_id: 440000,
        name: "深圳市",
        level: "city",
        prov: "广东省",
        city: "深圳市",
        area: "",
        has_child: true,
        latitude: 22.5431,
        longitude: 114.0579,
    },
    DivisionNode {
        id: 440305,
        parent_id: 440300,
        name: "南山区",
        level: "area",
        prov: "广东省",
        city: "深圳市",
        area: "南山区",
        has_child: false,
        latitude: 22.5333,
        longitude: 113.9304,
    },
];

const POIS: &[PoiRecord] = &[
    PoiRecord {
        poi: "西湖",
        address: "浙江省杭州市西湖区龙井路1号",
        prov: "浙江省",
        city: "杭州市",
        area: "西湖区",
        division_id: 330106,
        latitude: "30.2428",
        longitude: "120.1500",
        aliases: &["杭州西湖", "西湖景区"],
    },
    PoiRecord {
        poi: "浙江大学紫金港校区",
        address: "浙江省杭州市西湖区余杭塘路866号",
        prov: "浙江省",
        city: "杭州市",
        area: "西湖区",
        division_id: 330106,
        latitude: "30.3081",
        longitude: "120.0850",
        aliases: &["浙大紫金港", "浙江大学"],
    },
    PoiRecord {
        poi: "陆家嘴",
        address: "上海市浦东新区陆家嘴环路",
        prov: "上海市",
        city: "上海市",
        area: "浦东新区",
        division_id: 310115,
        latitude: "31.2397",
        longitude: "121.4998",
        aliases: &["上海陆家嘴"],
    },
    PoiRecord {
        poi: "深圳湾科技生态园",
        address: "广东省深圳市南山区高新南九道",
        prov: "广东省",
        city: "深圳市",
        area: "南山区",
        division_id: 440305,
        latitude: "22.5316",
        longitude: "113.9458",
        aliases: &["深圳湾", "科技生态园"],
    },
];

fn province_nodes() -> Vec<Value> {
    DIVISIONS
        .iter()
        .filter(|node| node.parent_id == 0)
        .map(node_to_value)
        .collect()
}

fn division_children(parent_id: i64) -> Vec<Value> {
    DIVISIONS
        .iter()
        .filter(|node| node.parent_id == parent_id)
        .map(node_to_value)
        .collect()
}

fn nearest_area(lat: f64, lng: f64) -> DivisionNode {
    DIVISIONS
        .iter()
        .filter(|node| node.level == "area")
        .min_by(|left, right| {
            let left_distance = (left.latitude - lat).powi(2) + (left.longitude - lng).powi(2);
            let right_distance = (right.latitude - lat).powi(2) + (right.longitude - lng).powi(2);
            left_distance
                .partial_cmp(&right_distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied()
        .unwrap_or(DIVISIONS[0])
}

fn search_poi(keyword: &str) -> Vec<Value> {
    let normalized_keyword = keyword.trim();
    let mut items = POIS
        .iter()
        .filter(|poi| {
            poi.poi.contains(normalized_keyword)
                || poi.address.contains(normalized_keyword)
                || poi
                    .aliases
                    .iter()
                    .any(|alias| alias.contains(normalized_keyword))
        })
        .map(poi_to_value)
        .collect::<Vec<_>>();

    if items.is_empty() {
        items = DIVISIONS
            .iter()
            .filter(|node| {
                node.name.contains(normalized_keyword)
                    || node.prov.contains(normalized_keyword)
                    || node.city.contains(normalized_keyword)
                    || node.area.contains(normalized_keyword)
            })
            .filter(|node| node.level == "area")
            .map(|node| {
                json!({
                    "poi": node.name,
                    "address": format!("{}{}{}", node.prov, node.city, node.area),
                    "prov": node.prov,
                    "city": node.city,
                    "area": node.area,
                    "divisionId": node.id.to_string(),
                    "latitude": format!("{:.4}", node.latitude),
                    "longitude": format!("{:.4}", node.longitude),
                })
            })
            .collect();
    }

    items
}

fn node_to_value(node: &DivisionNode) -> Value {
    json!({
        "id": node.id,
        "na": node.name,
        "level": node.level,
        "prov": node.prov,
        "city": node.city,
        "area": node.area,
        "divisionId": node.id.to_string(),
        "hasChild": node.has_child,
    })
}

fn poi_to_value(poi: &PoiRecord) -> Value {
    json!({
        "poi": poi.poi,
        "address": poi.address,
        "prov": poi.prov,
        "city": poi.city,
        "area": poi.area,
        "divisionId": poi.division_id.to_string(),
        "latitude": poi.latitude,
        "longitude": poi.longitude,
    })
}

fn success_ret() -> [&'static str; 1] {
    ["SUCCESS::调用成功"]
}
