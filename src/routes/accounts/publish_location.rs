//! Per-account publish location override endpoints.
//!
//! The legacy default (LocationContext::default() — 株洲荷塘区, 430202) used to
//! leak into every publish/groupon/pricing/orders call. These handlers expose
//! a dedicated `publish_locations` table so each uid can carry its own
//! fishAdCode + gps; when no row exists the legacy default is still returned
//! so the existing accounts keep working.
//!
//! Routes:
//!   GET  /api/accounts/:uid/publish_location — current effective value
//!   PUT  /api/accounts/:uid/publish_location — upsert override
//!   DELETE /api/accounts/:uid/publish_location — clear override
//!   GET  /api/location/cities — static fallback city/district catalogue

use axum::{
    extract::{Extension, Path, State},
    routing::get,
    Json, Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    models::auth::RequestIdentity,
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
    storage::publish_locations::PublishLocationRecord,
    xianyu_client::{self, LocationContext},
};

use super::shared::{ApiResult, ResponseError};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/accounts/:uid/publish_location"),
            get(get_publish_location)
                .put(put_publish_location)
                .delete(delete_publish_location),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/location/cities"),
            get(list_cities),
        )
}

#[derive(Debug, Deserialize)]
struct UpsertBody {
    #[serde(default, alias = "fishAdCode", alias = "fish_ad_code")]
    fish_ad_code: Option<String>,
    #[serde(default, alias = "divisionId", alias = "division_id")]
    division_id: Option<String>,
    #[serde(default)]
    gps: Option<String>,
    #[serde(default)]
    latitude: Option<String>,
    #[serde(default)]
    longitude: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    province: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default, alias = "area", alias = "district")]
    district: Option<String>,
}

async fn get_publish_location(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> ApiResult {
    ensure_publish_location_uid_access(&state, &identity, &uid).await?;
    let record = state
        .storage()
        .get_publish_location(&uid)
        .await
        .map_err(ResponseError::internal)?;

    let resolved = xianyu_client::load_location(state.storage().accounts(), &uid)
        .await
        .unwrap_or_default();

    Ok(ResponseJson(json!({
        "uid": uid,
        "overridden": record.is_some(),
        "effective": location_payload(&resolved),
        "record": record.as_ref().map(record_to_value),
    })))
}

async fn put_publish_location(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
    Json(body): Json<UpsertBody>,
) -> ApiResult {
    ensure_publish_location_uid_access(&state, &identity, &uid).await?;
    let fish_ad_code = body
        .fish_ad_code
        .clone()
        .or_else(|| body.division_id.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ResponseError::bad_request("缺少 fishAdCode（divisionId）"))?;

    let latitude = body
        .latitude
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let longitude = body
        .longitude
        .clone()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let gps = match body.gps.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(value) => value.to_string(),
        None => match (latitude.as_deref(), longitude.as_deref()) {
            (Some(lat), Some(lng)) => format!("{lat},{lng}"),
            _ => {
                return Err(ResponseError::bad_request("缺少 gps 或 latitude/longitude"));
            }
        },
    };

    // If lat/lng weren't given but gps was, split the pair so the row carries
    // both representations — saves the consumers a parse step.
    let (latitude, longitude) = match (latitude, longitude) {
        (Some(lat), Some(lng)) => (lat, lng),
        _ => split_gps(&gps),
    };

    let record = PublishLocationRecord {
        uid: uid.clone(),
        fish_ad_code,
        gps,
        latitude,
        longitude,
        name: body
            .name
            .clone()
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
        address: body
            .address
            .clone()
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
        province: body
            .province
            .clone()
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
        city: body
            .city
            .clone()
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
        district: body
            .district
            .clone()
            .map(|value| value.trim().to_string())
            .unwrap_or_default(),
        updated_at: 0,
    };

    state
        .storage()
        .set_publish_location(&record)
        .await
        .map_err(ResponseError::internal)?;

    let resolved = xianyu_client::load_location(state.storage().accounts(), &uid)
        .await
        .unwrap_or_default();

    Ok(ResponseJson(json!({
        "status": "ok",
        "record": record_to_value(&record),
        "effective": location_payload(&resolved),
    })))
}

async fn delete_publish_location(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(uid): Path<String>,
) -> ApiResult {
    ensure_publish_location_uid_access(&state, &identity, &uid).await?;
    state
        .storage()
        .delete_publish_location(&uid)
        .await
        .map_err(ResponseError::internal)?;
    Ok(ResponseJson(json!({"status": "ok"})))
}

async fn ensure_publish_location_uid_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid.trim()).await
}

async fn list_cities() -> ResponseJson<Value> {
    ResponseJson(json!({
        "items": STATIC_CITIES.iter().map(city_to_value).collect::<Vec<_>>(),
    }))
}

fn record_to_value(record: &PublishLocationRecord) -> Value {
    json!({
        "uid": record.uid,
        "fishAdCode": record.fish_ad_code,
        "gps": record.gps,
        "latitude": record.latitude,
        "longitude": record.longitude,
        "name": record.name,
        "address": record.address,
        "province": record.province,
        "city": record.city,
        "district": record.district,
        "updatedAt": record.updated_at,
    })
}

fn location_payload(location: &LocationContext) -> Value {
    json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "latitude": location.latitude,
        "longitude": location.longitude,
        "city": location.city,
        "province": location.province,
        "area": location.area,
    })
}

fn split_gps(gps: &str) -> (String, String) {
    gps.split_once(',')
        .map(|(lat, lng)| (lat.trim().to_string(), lng.trim().to_string()))
        .unwrap_or_default()
}

// ── Static city/district fallback ───────────────────────────────────────────
//
// The upstream `mtop.taobao.idle.division.all.get` endpoint can serve the full
// 全国 division tree but requires an authenticated session. This static list
// covers the major shipping hubs so the modal still works the first time the
// user adds an account (before the live session is healthy).

struct CityEntry {
    province: &'static str,
    city: &'static str,
    district: &'static str,
    ad_code: &'static str,
    lat: &'static str,
    lng: &'static str,
}

const STATIC_CITIES: &[CityEntry] = &[
    CityEntry {
        province: "北京市",
        city: "北京市",
        district: "东城区",
        ad_code: "110101",
        lat: "39.9388",
        lng: "116.4163",
    },
    CityEntry {
        province: "北京市",
        city: "北京市",
        district: "西城区",
        ad_code: "110102",
        lat: "39.9120",
        lng: "116.3661",
    },
    CityEntry {
        province: "北京市",
        city: "北京市",
        district: "海淀区",
        ad_code: "110108",
        lat: "39.9593",
        lng: "116.2981",
    },
    CityEntry {
        province: "北京市",
        city: "北京市",
        district: "朝阳区",
        ad_code: "110105",
        lat: "39.9219",
        lng: "116.4429",
    },
    CityEntry {
        province: "上海市",
        city: "上海市",
        district: "黄浦区",
        ad_code: "310101",
        lat: "31.2304",
        lng: "121.4737",
    },
    CityEntry {
        province: "上海市",
        city: "上海市",
        district: "浦东新区",
        ad_code: "310115",
        lat: "31.2215",
        lng: "121.5444",
    },
    CityEntry {
        province: "上海市",
        city: "上海市",
        district: "徐汇区",
        ad_code: "310104",
        lat: "31.1880",
        lng: "121.4365",
    },
    CityEntry {
        province: "广东省",
        city: "广州市",
        district: "天河区",
        ad_code: "440106",
        lat: "23.1245",
        lng: "113.3613",
    },
    CityEntry {
        province: "广东省",
        city: "广州市",
        district: "越秀区",
        ad_code: "440104",
        lat: "23.1290",
        lng: "113.2664",
    },
    CityEntry {
        province: "广东省",
        city: "深圳市",
        district: "福田区",
        ad_code: "440304",
        lat: "22.5410",
        lng: "114.0530",
    },
    CityEntry {
        province: "广东省",
        city: "深圳市",
        district: "南山区",
        ad_code: "440305",
        lat: "22.5333",
        lng: "113.9304",
    },
    CityEntry {
        province: "广东省",
        city: "深圳市",
        district: "罗湖区",
        ad_code: "440303",
        lat: "22.5482",
        lng: "114.1303",
    },
    CityEntry {
        province: "四川省",
        city: "成都市",
        district: "锦江区",
        ad_code: "510104",
        lat: "30.6586",
        lng: "104.0833",
    },
    CityEntry {
        province: "四川省",
        city: "成都市",
        district: "武侯区",
        ad_code: "510107",
        lat: "30.6420",
        lng: "104.0432",
    },
    CityEntry {
        province: "浙江省",
        city: "杭州市",
        district: "西湖区",
        ad_code: "330106",
        lat: "30.2596",
        lng: "120.1302",
    },
    CityEntry {
        province: "浙江省",
        city: "杭州市",
        district: "上城区",
        ad_code: "330102",
        lat: "30.2422",
        lng: "120.1693",
    },
    CityEntry {
        province: "浙江省",
        city: "宁波市",
        district: "海曙区",
        ad_code: "330203",
        lat: "29.8592",
        lng: "121.5503",
    },
    CityEntry {
        province: "湖北省",
        city: "武汉市",
        district: "江汉区",
        ad_code: "420103",
        lat: "30.6043",
        lng: "114.2716",
    },
    CityEntry {
        province: "湖北省",
        city: "武汉市",
        district: "武昌区",
        ad_code: "420106",
        lat: "30.5540",
        lng: "114.3160",
    },
    CityEntry {
        province: "陕西省",
        city: "西安市",
        district: "雁塔区",
        ad_code: "610113",
        lat: "34.2226",
        lng: "108.9410",
    },
    CityEntry {
        province: "陕西省",
        city: "西安市",
        district: "莲湖区",
        ad_code: "610104",
        lat: "34.2670",
        lng: "108.9400",
    },
    CityEntry {
        province: "江苏省",
        city: "南京市",
        district: "鼓楼区",
        ad_code: "320106",
        lat: "32.0722",
        lng: "118.7690",
    },
    CityEntry {
        province: "江苏省",
        city: "苏州市",
        district: "姑苏区",
        ad_code: "320508",
        lat: "31.3163",
        lng: "120.6195",
    },
    CityEntry {
        province: "重庆市",
        city: "重庆市",
        district: "渝中区",
        ad_code: "500103",
        lat: "29.5547",
        lng: "106.5703",
    },
    CityEntry {
        province: "天津市",
        city: "天津市",
        district: "和平区",
        ad_code: "120101",
        lat: "39.1226",
        lng: "117.2148",
    },
    CityEntry {
        province: "湖南省",
        city: "长沙市",
        district: "岳麓区",
        ad_code: "430104",
        lat: "28.2353",
        lng: "112.9388",
    },
    CityEntry {
        province: "湖南省",
        city: "株洲市",
        district: "荷塘区",
        ad_code: "430202",
        lat: "27.8672",
        lng: "113.1644",
    },
    CityEntry {
        province: "福建省",
        city: "厦门市",
        district: "思明区",
        ad_code: "350203",
        lat: "24.4540",
        lng: "118.0900",
    },
    CityEntry {
        province: "山东省",
        city: "青岛市",
        district: "市南区",
        ad_code: "370202",
        lat: "36.0671",
        lng: "120.3826",
    },
    CityEntry {
        province: "辽宁省",
        city: "沈阳市",
        district: "和平区",
        ad_code: "210102",
        lat: "41.7906",
        lng: "123.4063",
    },
];

fn city_to_value(entry: &CityEntry) -> Value {
    json!({
        "province": entry.province,
        "city": entry.city,
        "district": entry.district,
        "fishAdCode": entry.ad_code,
        "latitude": entry.lat,
        "longitude": entry.lng,
        "gps": format!("{},{}", entry.lat, entry.lng),
    })
}
