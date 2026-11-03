use std::collections::BTreeMap;
use std::sync::Arc;

use reqwest::{
    cookie::{CookieStore, Jar},
    header::{HeaderMap, HeaderValue, REFERER, USER_AGENT},
    Client, Url,
};

use super::{APPKEY, BLOCKED_COOKIE_PREFIXES, GOOFISH_URL, UA};

pub(super) fn build_client(jar: Arc<Jar>) -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_provider(jar)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())
}

pub(super) fn seed_jar(jar: &Jar, cookies: &BTreeMap<String, String>) {
    let url = Url::parse(GOOFISH_URL()).expect("static url parses");
    for (name, value) in cookies {
        if is_blocked_cookie(name) {
            continue;
        }
        let cookie_domain = crate::sealed_protocol::sealed_str!(".goofish.com");
        let cookie = format!("{name}={value}; Domain={cookie_domain}; Path=/");
        jar.add_cookie_str(&cookie, &url);
    }
}

pub(super) fn jar_cookie_map(jar: &Jar, url: &Url) -> BTreeMap<String, String> {
    let mut cookies = BTreeMap::new();
    let header = jar
        .cookies(url)
        .and_then(|value| value.to_str().ok().map(|text| text.to_string()));
    if let Some(header) = header {
        for part in header.split(';') {
            let trimmed = part.trim();
            if let Some((name, value)) = trimmed.split_once('=') {
                if is_blocked_cookie(name) {
                    continue;
                }
                cookies.insert(name.to_string(), value.to_string());
            }
        }
    }
    cookies
}

pub(super) fn is_blocked_cookie(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    BLOCKED_COOKIE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

pub(super) fn default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(UA));
    headers.insert(
        REFERER,
        HeaderValue::from_static(crate::sealed_protocol::sealed_str!(
            "https://www.goofish.com/"
        )),
    );
    headers
}

pub(super) fn mtop_params(api: &str, data: &str, token: &str) -> Vec<(String, String)> {
    let t = unix_millis_now().to_string();
    let sign = format!(
        "{:x}",
        md5::compute(format!("{}&{}&{}&{}", token, t, APPKEY, data))
    );
    vec![
        ("jsv".to_string(), "2.7.2".to_string()),
        ("appKey".to_string(), APPKEY.to_string()),
        ("t".to_string(), t),
        ("sign".to_string(), sign),
        ("v".to_string(), "1.0".to_string()),
        ("type".to_string(), "originaljson".to_string()),
        ("dataType".to_string(), "json".to_string()),
        ("api".to_string(), api.to_string()),
        ("data".to_string(), data.to_string()),
    ]
}

fn unix_millis_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}
