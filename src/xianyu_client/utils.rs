use std::{
    collections::BTreeMap,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Map, Value};
use sqlx::{Pool, Row, Sqlite};

use super::LocationContext;

const BUILTIN_APP_HEADER_TEMPLATE: &[(&str, &str)] = &[
    (
        "content-type",
        "application/x-www-form-urlencoded;charset=UTF-8",
    ),
    ("accept", "application/json, text/plain, */*"),
    (
        "user-agent",
        "MTOPSDK/3.1.1.7 (Android;13;Google;sdk_gphone64_arm64) DeviceType(Phone)",
    ),
    ("x-appkey", "21407387"),
    ("x-pv", "6.3"),
    ("x-features", "27"),
    ("x-utdid", "afLbXwKpMzoDAPE7F1zUWMHl"),
    ("x-ttid", "10003993%40fleamarket_android_7.25.70"),
    ("x-extdata", "openappkey%3DDEFAULT_AUTH"),
    ("x-location", "0%2C0"),
];

pub(super) fn parse_cookie_map(raw: &str) -> BTreeMap<String, String> {
    serde_json::from_str::<Map<String, Value>>(raw)
        .ok()
        .map(|object| {
            object
                .into_iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key, value.to_string())))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(super) fn merged_cookie_map(
    cookies_raw: &str,
    browser_jar_raw: &str,
) -> BTreeMap<String, String> {
    let mut merged = parse_cookie_map(cookies_raw);
    let jar_values = serde_json::from_str::<Vec<Value>>(browser_jar_raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| entry.as_object().cloned())
        .filter_map(|entry| {
            let name = entry
                .get("name")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            let value = entry.get("value").and_then(Value::as_str)?.to_string();
            let domain = entry
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if name.is_empty()
                || value.is_empty()
                || !domain.contains(crate::sealed_protocol::sealed_str!("goofish.com"))
            {
                return None;
            }
            Some((name, value))
        })
        .collect::<BTreeMap<_, _>>();
    for (key, value) in jar_values {
        merged.entry(key).or_insert(value);
    }
    merged
}

pub(super) fn parse_location(raw: &str) -> LocationContext {
    let extra = serde_json::from_str::<Map<String, Value>>(raw).unwrap_or_default();
    let location = extra
        .get("location")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let default = LocationContext::default();
    let fish_ad_code = location
        .get("fishAdCode")
        .and_then(Value::as_str)
        .unwrap_or(default.fish_ad_code.as_str())
        .to_string();
    let gps = location
        .get("gps")
        .and_then(Value::as_str)
        .unwrap_or(default.gps.as_str())
        .to_string();
    let latitude = location
        .get("latitude")
        .and_then(Value::as_str)
        .unwrap_or(default.latitude.as_str())
        .to_string();
    let longitude = location
        .get("longitude")
        .and_then(Value::as_str)
        .unwrap_or(default.longitude.as_str())
        .to_string();
    let city = location
        .get("city")
        .and_then(Value::as_str)
        .unwrap_or(default.city.as_str())
        .to_string();
    let province = location
        .get("province")
        .and_then(Value::as_str)
        .unwrap_or(default.province.as_str())
        .to_string();
    let area = location
        .get("area")
        .and_then(Value::as_str)
        .unwrap_or(default.area.as_str())
        .to_string();
    LocationContext {
        fish_ad_code,
        gps,
        latitude,
        longitude,
        name: String::new(),
        address: String::new(),
        city,
        province,
        area,
    }
}

pub(super) fn parse_pix(raw: &str) -> (i64, i64) {
    let Some((width, height)) = raw.split_once('x') else {
        return (0, 0);
    };
    let width = width.parse::<i64>().unwrap_or(0);
    let height = height.parse::<i64>().unwrap_or(0);
    (width, height)
}

pub(super) fn trim_trailing_slash(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

pub(super) fn set_header(headers: &mut HeaderMap, name: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Ok(());
    }
    let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| error.to_string())?;
    let header_value = HeaderValue::from_str(value).map_err(|error| error.to_string())?;
    headers.insert(header_name, header_value);
    Ok(())
}

pub(super) fn builtin_app_header_template() -> &'static BTreeMap<String, String> {
    static TEMPLATE: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        BUILTIN_APP_HEADER_TEMPLATE
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<BTreeMap<_, _>>()
    })
}

pub(super) fn app_headers_for_account_or_default<'a>(
    account_headers: Option<&'a BTreeMap<String, String>>,
) -> &'a BTreeMap<String, String> {
    match account_headers {
        Some(headers) => headers,
        None => builtin_app_header_template(),
    }
}

pub(super) fn app_template_header(name: &str) -> String {
    builtin_app_header_template()
        .get(name)
        .cloned()
        .unwrap_or_default()
}

pub(super) fn app_template_lat_lng() -> (String, String) {
    let location = percent_decode_lossy(&app_template_header("x-location"));
    if let Some((lng, lat)) = location.split_once(',') {
        return (lat.to_string(), lng.to_string());
    }
    ("0".to_string(), "0".to_string())
}

pub(super) async fn load_account_app_headers(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<Option<BTreeMap<String, String>>, String> {
    let row = sqlx::query("SELECT extra FROM accounts WHERE uid = ? LIMIT 1")
        .bind(uid)
        .fetch_optional(accounts)
        .await
        .map_err(|error| error.to_string())?;
    let Some(row) = row else {
        return Ok(None);
    };
    let extra = serde_json::from_str::<Map<String, Value>>(&row.get::<String, _>("extra"))
        .unwrap_or_default();
    let Some(headers) = extra
        .get("appRequestHeaders")
        .and_then(Value::as_object)
        .map(|object| {
            object
                .iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect::<BTreeMap<_, _>>()
        })
        .filter(|headers| !headers.is_empty())
    else {
        return Ok(None);
    };
    Ok(Some(headers))
}

pub(super) fn percent_encode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let safe = matches!(
            byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'_'
                | b'.'
                | b'~'
        );
        if safe {
            out.push(byte as char);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    out
}

pub(super) fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn unix_millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

pub(super) fn unix_seconds_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_header_template_falls_back_to_builtin_headers() {
        let template = builtin_app_header_template();
        assert_eq!(
            template.get("x-appkey").map(String::as_str),
            Some("21407387")
        );
        assert_eq!(template.get("x-pv").map(String::as_str), Some("6.3"));
        assert_eq!(
            template.get("user-agent").map(String::as_str),
            Some("MTOPSDK/3.1.1.7 (Android;13;Google;sdk_gphone64_arm64) DeviceType(Phone)")
        );
    }
}
