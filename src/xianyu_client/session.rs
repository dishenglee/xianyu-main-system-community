use std::collections::BTreeMap;

use reqwest::header::{HeaderMap, SET_COOKIE};
use serde_json::Value;
use sqlx::{Pool, Row, Sqlite};

use super::{
    utils::{merged_cookie_map, parse_location, unix_seconds_now},
    LocationContext, COOKIE_KEYS,
};

#[derive(Debug, Clone)]
pub(super) struct AccountSession {
    pub(super) uid: String,
    pub(super) token_column: String,
    pub(super) cookies: BTreeMap<String, String>,
    pub(super) location: LocationContext,
}

pub(super) async fn load_account_session(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<AccountSession, String> {
    let row = sqlx::query(
        r#"
        SELECT uid, token, cookies, browser_cookie_jar, extra
        FROM accounts
        WHERE uid = ?
        LIMIT 1
        "#,
    )
    .bind(uid)
    .fetch_optional(accounts)
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| format!("account {uid} not found"))?;

    let cookies = merged_cookie_map(
        row.get::<String, _>("cookies").as_str(),
        row.get::<String, _>("browser_cookie_jar").as_str(),
    );
    let location = parse_location(row.get::<String, _>("extra").as_str());
    Ok(AccountSession {
        uid: row.get::<String, _>("uid"),
        token_column: row.get::<String, _>("token"),
        cookies,
        location,
    })
}

pub(super) async fn apply_response_cookies(
    accounts: &Pool<Sqlite>,
    session: &mut AccountSession,
    headers: &HeaderMap,
) -> Result<(), String> {
    let mut changed = false;
    for value in headers.get_all(SET_COOKIE).iter() {
        let raw = value.to_str().map_err(|error| error.to_string())?;
        let Some((name, rest)) = raw.split_once('=') else {
            continue;
        };
        if !COOKIE_KEYS.contains(&name) {
            continue;
        }
        let cookie_value = rest.split(';').next().unwrap_or_default().trim();
        if cookie_value.is_empty() {
            continue;
        }
        session
            .cookies
            .insert(name.to_string(), cookie_value.to_string());
        changed = true;
    }
    if changed {
        persist_session(accounts, session).await?;
    }
    Ok(())
}

async fn persist_session(accounts: &Pool<Sqlite>, session: &AccountSession) -> Result<(), String> {
    let token = session
        .cookies
        .get("_m_h5_tk")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| session.token_column.clone());
    let cookies = serde_json::to_string(&session.cookies).map_err(|error| error.to_string())?;
    let now = unix_seconds_now();
    sqlx::query(
        r#"
        UPDATE accounts
        SET token = ?,
            cookies = ?,
            cookie_refreshed_at = ?,
            updated_at = ?
        WHERE uid = ?
        "#,
    )
    .bind(token)
    .bind(cookies)
    .bind(now)
    .bind(now)
    .bind(&session.uid)
    .execute(accounts)
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

pub(super) async fn load_location_from_db(
    accounts: &Pool<Sqlite>,
    uid: &str,
) -> Result<LocationContext, String> {
    // Per-account override stored in publish_locations always wins. We fall
    // back to whatever is encoded in `accounts.extra.location` (legacy path)
    // and finally to LocationContext::default() when neither has a row.
    if let Some(record) = crate::storage::publish_locations::get_publish_location(accounts, uid)
        .await
        .map_err(|error| error.to_string())?
    {
        let LocationContext {
            latitude: default_lat,
            longitude: default_lng,
            ..
        } = LocationContext::default();
        let latitude = if record.latitude.is_empty() {
            extract_latitude(&record.gps).unwrap_or(default_lat)
        } else {
            record.latitude
        };
        let longitude = if record.longitude.is_empty() {
            extract_longitude(&record.gps).unwrap_or(default_lng)
        } else {
            record.longitude
        };
        return Ok(LocationContext {
            fish_ad_code: record.fish_ad_code,
            gps: record.gps,
            latitude,
            longitude,
            name: record.name,
            address: record.address,
            city: record.city,
            province: record.province,
            area: record.district,
        });
    }
    Ok(load_account_session(accounts, uid).await?.location)
}

/// `gps` is encoded as `"<lat>,<lng>"` upstream — pull the lat half when the
/// stored row doesn't carry an explicit latitude column (legacy rows).
fn extract_latitude(gps: &str) -> Option<String> {
    gps.split_once(',').map(|(lat, _)| lat.trim().to_string())
}

fn extract_longitude(gps: &str) -> Option<String> {
    gps.split_once(',').map(|(_, lng)| lng.trim().to_string())
}

/// Used only in tests to verify token extraction logic path.
pub(super) fn extract_sign_token(session: &AccountSession) -> String {
    session
        .cookies
        .get("_m_h5_tk")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| session.token_column.clone())
        .split('_')
        .next()
        .unwrap_or_default()
        .to_string()
}

pub(super) fn is_token_expired(payload: &Value) -> bool {
    let ret = payload
        .get("ret")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let text = if let Some(items) = ret.as_array() {
        items
            .first()
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        ret.as_str().unwrap_or_default().to_string()
    };
    let upper = text.to_ascii_uppercase();
    upper.contains("TOKEN_EXPIRED")
        || upper.contains("TOKEN_EMPTY")
        || upper.contains("FAIL_SYS_TOKEN")
}
