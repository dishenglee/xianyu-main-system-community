use serde_json::Value;
use std::collections::BTreeMap;

pub(super) fn parse_cookie_map(raw: &str) -> BTreeMap<String, String> {
    serde_json::from_str::<serde_json::Map<String, Value>>(raw)
        .ok()
        .map(|object| {
            object
                .into_iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                .collect()
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

pub(super) fn merge_maps(
    base: &BTreeMap<String, String>,
    overlay: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut out = base.clone();
    for (k, v) in overlay {
        out.insert(k.clone(), v.clone());
    }
    out
}

pub(super) fn merge_in(base: &mut BTreeMap<String, String>, overlay: BTreeMap<String, String>) {
    for (k, v) in overlay {
        base.insert(k, v);
    }
}

pub(super) fn has_recoverable_login_base(cookies: &BTreeMap<String, String>) -> bool {
    let unb = cookies
        .get("unb")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let cookie2 = cookies
        .get("cookie2")
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    unb && cookie2
}
