use std::collections::BTreeMap;
use std::sync::Arc;

use reqwest::{cookie::Jar, Url};
use serde_json::Value;

use crate::protocol_catalog::{opcodes, resolve_builtin_protocol};

use super::cookies::merge_maps;
use super::http::{build_client, default_headers, jar_cookie_map, mtop_params, seed_jar};
use super::persistence::unix_seconds_now;
use super::{
    passport_has_login_url, passport_login_check_url, GOOFISH_URL, HAVANA_RENEW_THRESHOLD_SECS,
    MTOP_BASE, PASSPORT_MINI_LOGIN_URL, SET_LOGIN_SETTINGS_URL,
};

/// Pure HTTP two-step bootstrap — matches Python `bootstrap_h5tk`.
pub(super) async fn bootstrap_h5tk(
    cookies: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let mut seed = cookies.clone();
    seed.remove("_m_h5_tk");
    seed.remove("_m_h5_tk_enc");

    let jar = Arc::new(Jar::default());
    seed_jar(&jar, &seed);
    let client = build_client(jar.clone())?;

    let protocol = resolve_builtin_protocol(opcodes::ACCOUNT_USER_SIMPLE)
        .map_err(|error| error.to_string())?;
    let api = protocol.api();
    let url = format!("{}/{api}/{}/", MTOP_BASE(), protocol.version());
    let data = "{}";

    // Step 1: empty-token request, server returns fresh _m_h5_tk via Set-Cookie.
    let params_empty = mtop_params(api, data, "");
    let _ = client
        .get(&url)
        .query(&params_empty)
        .headers(default_headers())
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let goofish_url = Url::parse(GOOFISH_URL()).map_err(|error| error.to_string())?;
    let jar_cookies = jar_cookie_map(&jar, &goofish_url);

    let h5tk = match jar_cookies.get("_m_h5_tk") {
        Some(value) if !value.is_empty() => value.clone(),
        _ => return Ok(merge_maps(&seed, &jar_cookies)),
    };

    // Step 2: resign with the new token to confirm the handshake.
    let token = h5tk.split('_').next().unwrap_or_default();
    let params_signed = mtop_params(api, data, token);
    let _ = client
        .get(&url)
        .query(&params_signed)
        .headers(default_headers())
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let final_cookies = jar_cookie_map(&jar, &goofish_url);
    Ok(merge_maps(&seed, &final_cookies))
}

/// Passport re-anchor fallback — matches Python `http_reanchor_session`.
pub(super) async fn http_reanchor_session(
    cookies: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let jar = Arc::new(Jar::default());
    seed_jar(&jar, cookies);
    let client = build_client(jar.clone())?;

    for url in [
        PASSPORT_MINI_LOGIN_URL(),
        passport_login_check_url(),
        GOOFISH_URL(),
    ] {
        let _ = client.get(url).headers(default_headers()).send().await;
    }

    let goofish_url = Url::parse(GOOFISH_URL()).map_err(|error| error.to_string())?;
    let merged = merge_maps(cookies, &jar_cookie_map(&jar, &goofish_url));
    // After re-anchoring, retry bootstrap to pick up a fresh h5tk.
    let refreshed = bootstrap_h5tk(&merged).await?;
    if refreshed.get("_m_h5_tk").is_some() {
        Ok(refreshed)
    } else {
        Ok(merged)
    }
}

pub(super) fn havana_needs_refresh(cookies: &BTreeMap<String, String>) -> bool {
    let Some(_) = cookies.get("havana_lgc2_77") else {
        return true;
    };
    let Some(exp) = cookies.get("havana_lgc_exp") else {
        return true;
    };
    let Ok(exp_ms) = exp.parse::<i64>() else {
        return true;
    };
    let exp_secs = exp_ms / 1000;
    let now = unix_seconds_now() as i64;
    (exp_secs - now) < HAVANA_RENEW_THRESHOLD_SECS
}

pub(super) async fn enable_long_login(
    cookies: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let jar = Arc::new(Jar::default());
    seed_jar(&jar, cookies);
    let client = build_client(jar.clone())?;

    let params = [
        ("fromSite", "77"),
        ("appName", "xianyu"),
        ("bizEntrance", "web"),
    ];
    let _ = client
        .post(SET_LOGIN_SETTINGS_URL())
        .query(&params)
        .headers(default_headers())
        .form(&[("status", "0")])
        .send()
        .await
        .map_err(|error| error.to_string())?;

    let goofish_url = Url::parse(GOOFISH_URL()).map_err(|error| error.to_string())?;
    Ok(merge_maps(cookies, &jar_cookie_map(&jar, &goofish_url)))
}

/// Deep passport resume — matches Python `http_resume_haslogin_session`.
/// Replays the browser's silent re-login handshake:
///   mini_login → parse `loginFormData` → hasLogin.do → login_check → home
/// This is what rescues `unb / sgcookie / cookie2` / `_tb_token_` after they've
/// drifted, which `bootstrap_h5tk` alone can't fix (it only touches `_m_h5_tk`).
/// Called as a last-resort layer when `wx.login.token` keeps returning
/// FAIL_SYS_TOKEN_ILLEGAL even after bootstrap.
pub(super) async fn http_resume_haslogin_session(
    cookies: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    let jar = Arc::new(Jar::default());
    seed_jar(&jar, cookies);
    let client = build_client(jar.clone())?;

    // 1) GET mini_login.htm — 拿到内联的 loginFormData JSON。
    let mini = client
        .get(PASSPORT_MINI_LOGIN_URL())
        .headers(default_headers())
        .send()
        .await
        .map_err(|error| error.to_string())?
        .text()
        .await
        .map_err(|error| error.to_string())?;

    let form_data =
        extract_login_form_data(&mini).ok_or_else(|| "E_PASSPORT_FORM_DATA_MISSING".to_string())?;

    // 2) POST hasLogin.do — 带上 loginFormData 的全部字段 + 4 个默认字段。
    let mut pairs: Vec<(String, String)> = form_data
        .iter()
        .map(|(k, v)| (k.clone(), form_value_as_string(v)))
        .collect();
    for (k, v) in [
        ("umidTag", "SERVER"),
        ("isIframe", "false"),
        ("documentReferer", ""),
        ("defaultView", "hasLogin"),
    ] {
        pairs.push((k.to_string(), v.to_string()));
    }
    let _ = client
        .post(passport_has_login_url())
        .headers(default_headers())
        .form(&pairs)
        .send()
        .await;

    // 3) GET 四个页面让 passport 把刷新后的 unb/sgcookie 写回 jar。
    for url in [
        passport_login_check_url(),
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/"),
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/im"),
        crate::sealed_protocol::sealed_str!("https://www.goofish.com/personal"),
    ] {
        let _ = client.get(url).headers(default_headers()).send().await;
    }

    let goofish_url = Url::parse(GOOFISH_URL()).map_err(|error| error.to_string())?;
    Ok(merge_maps(cookies, &jar_cookie_map(&jar, &goofish_url)))
}

/// Extract the `loginFormData` JSON object from the mini_login HTML.
///
/// Passport serves two shapes depending on the login state:
///   - Fresh / anonymous visitor:   `loginFormData = {...};`  (JS 全局变量)
///   - Already-authenticated user:  `"loginFormData":{...}`   (JSON 内嵌到 window._SATRConfig 之类)
///
/// Python's regex `loginFormData\s*=\s*(\{.*?\})\s*;` only catches the first
/// case, which is why the second case silently fell through in the original
/// port. We look for either `=` or `:` after the needle so both shapes work.
fn extract_login_form_data(html: &str) -> Option<serde_json::Map<String, Value>> {
    let field = crate::sealed_protocol::sealed_str!("loginFormData");
    let anchor = html.find(field)?;
    let tail = &html[anchor..];
    // Skip the needle itself + optional closing quote, then expect `=` or `:`.
    // `"loginFormData":{...}` → separator is `:`.
    // `loginFormData = {...}` → separator is `=`.
    let after_key = &tail[field.len()..];
    let after_key = after_key.trim_start_matches('"');
    let after_key = after_key.trim_start();
    let sep_char = after_key.chars().next()?;
    if sep_char != '=' && sep_char != ':' {
        return None;
    }
    let after_sep = &after_key[sep_char.len_utf8()..];
    let start_rel = after_sep.find('{')?;
    // Scan balanced braces to find the matching `}`.
    let body = &after_sep[start_rel..];
    let bytes = body.as_bytes();
    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escape = false;
    let mut end: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if escape {
            escape = false;
            continue;
        }
        if in_str {
            if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let json_slice = &body[..end];
    let value: Value = serde_json::from_str(json_slice).ok()?;
    value.as_object().cloned()
}

fn form_value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
