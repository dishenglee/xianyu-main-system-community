use std::{
    collections::{BTreeMap, HashMap},
    env,
    sync::Arc,
};

use axum::{
    extract::{Extension, Path, State},
    Json as ResponseJson,
};
use base64::{
    engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};
use qrcode::{render::svg, QrCode};
use reqwest::{
    cookie::{CookieStore, Jar},
    header::{COOKIE, SET_COOKIE, USER_AGENT},
    Client, Url,
};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256, Sha512};
use tracing::warn;

use crate::{
    control_plane_auth::random_nonce_hex,
    models::auth::RequestIdentity,
    protocol_catalog::opcodes,
    routes::tenant::has_unscoped_legacy_access,
    state::AppState,
};

use super::{
    cloud_binding::{claim_binding, ClaimBindingError},
    qr_sessions, resolve_account_protocol, unix_millis_now, unix_seconds_now, ApiResult,
    QrSessionState, ResponseError, QR_BAXIA_VERSION, QR_DEFAULT_LOGIN_BIZ_PARAMS,
    QR_DEFAULT_PASSPORT_HOST, QR_DEFAULT_RENDER_REFERER,
};

const OWNER_CONFLICT_MESSAGE: &str = "无法保存闲鱼账号，请检查本机数据库状态后重试。";

fn qr_passport_host() -> String {
    env::var("XIANYU_QR_PASSPORT_HOST")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| QR_DEFAULT_PASSPORT_HOST().to_string())
}

fn qr_local_ttl_secs() -> Option<f64> {
    env::var("XIANYU_QR_LOCAL_TTL_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| *value >= 1.0)
}

fn qr_age_seconds(session: &QrSessionState) -> i64 {
    (unix_seconds_now() - session.created_at).max(0.0).floor() as i64
}

fn qr_expires_in_seconds(session: &QrSessionState) -> i64 {
    let Some(ttl_secs) = qr_local_ttl_secs() else {
        return 0;
    };
    (ttl_secs - (unix_seconds_now() - session.created_at))
        .max(0.0)
        .ceil() as i64
}

fn qr_origin() -> Result<String, String> {
    let url = Url::parse(&qr_passport_host()).map_err(|error| error.to_string())?;
    let host = url
        .host_str()
        .ok_or_else(|| "passport host missing host".to_string())?;
    let port = url
        .port()
        .map(|value| format!(":{value}"))
        .unwrap_or_default();
    Ok(format!("{}://{}{}", url.scheme(), host, port))
}

fn qr_default_user_agent() -> &'static str {
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36"
}

fn qr_request_headers(referer: &str, content_type: Option<&str>) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("accept-language", "zh-CN,zh;q=0.9,en;q=0.8".to_string()),
        ("accept", "application/json, text/plain, */*".to_string()),
        ("user-agent", qr_default_user_agent().to_string()),
        ("referer", referer.to_string()),
        ("origin", qr_origin().unwrap_or_else(|_| qr_passport_host())),
    ];
    if let Some(content_type) = content_type {
        headers.push(("content-type", content_type.to_string()));
    }
    headers
}

fn json_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => {
            if *value {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn extract_meta_content(html: &str, meta_name: &str) -> String {
    let marker = format!(r#"<meta name="{meta_name}" content=""#);
    let Some(start) = html.find(&marker) else {
        return String::new();
    };
    let remainder = &html[start + marker.len()..];
    remainder
        .find('"')
        .map(|end| remainder[..end].to_string())
        .unwrap_or_default()
}

fn extract_default_view(html: &str) -> String {
    let marker = r#""defaultView":"#;
    let Some(start) = html.find(marker) else {
        return "sms".to_string();
    };
    let remainder = &html[start + marker.len()..];
    if let Some(stripped) = remainder.strip_prefix('"') {
        stripped
            .find('"')
            .map(|end| stripped[..end].to_string())
            .unwrap_or_else(|| "sms".to_string())
    } else {
        "sms".to_string()
    }
}

fn extract_json_assignment(html: &str, marker: &str) -> Option<String> {
    let start_marker = html.find(marker)?;
    let mut start = None;
    for (offset, ch) in html[start_marker..].char_indices() {
        if ch == '{' {
            start = Some(start_marker + offset);
            break;
        }
    }
    let start = start?;
    let mut depth = 0_i64;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, ch) in html[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(html[start..start + offset + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn infer_nav_platform(user_agent: &str) -> String {
    let ua = user_agent.to_ascii_lowercase();
    if ua.contains("macintosh") || ua.contains("mac os x") {
        "MacIntel".to_string()
    } else if ua.contains("linux") {
        "Linux x86_64".to_string()
    } else {
        "Win32".to_string()
    }
}

fn sanitize_baxia_value(value: impl AsRef<str>) -> String {
    let text = value.as_ref().trim();
    if text.is_empty()
        || matches!(
            text.to_ascii_lowercase().as_str(),
            "default_not_value" | "default_not_fun" | "default_not_loaded" | "null" | "undefined"
        )
    {
        String::new()
    } else {
        text.to_string()
    }
}

fn build_baxia_seed(session: &QrSessionState) -> String {
    format!(
        "{}|{}|{}|{}",
        session.account_uid,
        session.params.get("umidToken").cloned().unwrap_or_default(),
        session
            .params
            .get("_csrf_token")
            .cloned()
            .unwrap_or_default(),
        session.params.get("ck").cloned().unwrap_or_default(),
    )
}

fn fallback_bx_umidtoken(session: &QrSessionState, form_data: &HashMap<String, String>) -> String {
    for candidate in [
        form_data.get("bx-umidtoken"),
        form_data.get("bx_umidtoken"),
        form_data.get("umidToken"),
        session.cookies.get("bx-umidtoken"),
        session.cookies.get("umidToken"),
        session.cookies.get("cna"),
    ]
    .into_iter()
    .flatten()
    {
        let token = sanitize_baxia_value(candidate);
        if !token.is_empty() {
            return token;
        }
    }
    let digest = Sha256::digest(build_baxia_seed(session).as_bytes());
    URL_SAFE_NO_PAD.encode(&digest[..24])
}

fn fallback_bx_et(session: &QrSessionState, form_data: &HashMap<String, String>) -> String {
    for candidate in [
        form_data.get("bx_et"),
        form_data.get("bx-et"),
        session.cookies.get("bx_et"),
    ]
    .into_iter()
    .flatten()
    {
        let token = sanitize_baxia_value(candidate);
        if !token.is_empty() {
            return token;
        }
    }
    let digest = Sha512::digest(build_baxia_seed(session).as_bytes());
    format!("{}.", URL_SAFE_NO_PAD.encode(digest))
}

fn fallback_x_pipu2(session: &QrSessionState, form_data: &HashMap<String, String>) -> String {
    for candidate in [
        form_data.get("x-pipu2"),
        form_data.get("x_pipu2"),
        session.cookies.get("x-pipu2"),
    ]
    .into_iter()
    .flatten()
    {
        let token = sanitize_baxia_value(candidate);
        if !token.is_empty() {
            return token;
        }
    }
    let seed = format!("{:x}", md5::compute(build_baxia_seed(session).as_bytes()));
    format!("{{nbug`{}~{}}}", &seed[..20], &seed[12..])
}

fn build_qr_svg_data_url(content: &str) -> Result<String, String> {
    let code = QrCode::new(content.as_bytes()).map_err(|error| error.to_string())?;
    let svg = code
        .render::<svg::Color<'_>>()
        .min_dimensions(240, 240)
        .quiet_zone(true)
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        BASE64_STANDARD.encode(svg.as_bytes())
    ))
}

fn build_reqwest_client(jar: Arc<Jar>) -> Result<Client, String> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_provider(jar)
        .build()
        .map_err(|error| error.to_string())
}

fn jar_cookie_map(jar: &Jar, url: &Url) -> BTreeMap<String, String> {
    let mut cookies = BTreeMap::new();
    let header = jar
        .cookies(url)
        .and_then(|value| value.to_str().ok().map(|text| text.to_string()));
    if let Some(header) = header {
        for part in header.split(';') {
            let trimmed = part.trim();
            if let Some((name, value)) = trimmed.split_once('=') {
                cookies.insert(name.to_string(), value.to_string());
            }
        }
    }
    cookies
}

async fn fetch_login_session(session_id: &str) -> Result<QrSessionState, String> {
    let passport_host = qr_passport_host();
    let passport_url = Url::parse(&passport_host).map_err(|error| error.to_string())?;
    let jar = Arc::new(Jar::default());
    let client = build_reqwest_client(jar.clone())?;
    let params = vec![
        ("lang", "zh_cn".to_string()),
        ("appName", "xianyu".to_string()),
        ("appEntrance", "web".to_string()),
        ("styleType", "vertical".to_string()),
        ("bizParams", String::new()),
        ("notLoadSsoView", "false".to_string()),
        ("notKeepLogin", "false".to_string()),
        ("isMobile", "false".to_string()),
        ("qrCodeFirst", "false".to_string()),
        ("site", "77".to_string()),
        ("stie", "77".to_string()),
        ("rnd", format!("{}", unix_seconds_now())),
    ];
    let mut request = client
        .get(
            passport_url
                .join("mini_login.htm")
                .map_err(|error| error.to_string())?,
        )
        .query(&params);
    for (key, value) in qr_request_headers(QR_DEFAULT_RENDER_REFERER(), None) {
        request = request.header(key, value);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let final_url = response.url().clone();
    let html = response.text().await.map_err(|error| error.to_string())?;
    let view_data_json = extract_json_assignment(&html, "window.viewData")
        .ok_or_else(|| "获取登录参数失败".to_string())?;
    let view_data =
        serde_json::from_str::<Value>(&view_data_json).map_err(|error| error.to_string())?;
    let login_form = view_data
        .get(crate::sealed_protocol::sealed_str!("loginFormData"))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| "登录参数缺失".to_string())?;

    let mut params = login_form
        .into_iter()
        .map(|(key, value)| (key, json_value_to_string(&value)))
        .collect::<HashMap<_, _>>();
    params.insert(
        "bizParams".to_string(),
        QR_DEFAULT_LOGIN_BIZ_PARAMS().to_string(),
    );
    params.insert("umidTag".to_string(), "SERVER".to_string());
    params.insert(
        "pageTraceId".to_string(),
        extract_meta_content(&html, "eagleeye-trace"),
    );
    params.insert("defaultView".to_string(), extract_default_view(&html));
    params.insert(
        "documentReferer".to_string(),
        QR_DEFAULT_RENDER_REFERER().to_string(),
    );
    params.entry("deviceId".to_string()).or_default();
    params.entry("ua".to_string()).or_default();
    params.insert("_mini_login_url".to_string(), final_url.to_string());

    let cookies = jar_cookie_map(jar.as_ref(), &passport_url);
    Ok(QrSessionState {
        status: "waiting".to_string(),
        message: String::new(),
        owner_username: String::new(),
        license_auth_token: String::new(),
        owner_conflict: false,
        created_at: unix_seconds_now(),
        account_uid: session_id.to_string(),
        account_nick: String::new(),
        qr_code_url: String::new(),
        qr_content: String::new(),
        cookies,
        params,
        verification_url: String::new(),
        imported: false,
    })
}

fn build_qr_generate_params(params: &HashMap<String, String>) -> Vec<(String, String)> {
    [
        "appName",
        "appEntrance",
        "_csrf_token",
        "umidToken",
        "hsiz",
        "bizParams",
        "mainPage",
        "isMobile",
        "lang",
        "returnUrl",
        "fromSite",
        "umidTag",
    ]
    .into_iter()
    .filter_map(|key| {
        params
            .get(key)
            .cloned()
            .map(|value| (key.to_string(), value))
    })
    .collect()
}

async fn generate_qr_session(session_id: &str) -> Result<QrSessionState, String> {
    let mut session = fetch_login_session(session_id).await?;
    let passport_host = qr_passport_host();
    let passport_url = Url::parse(&passport_host).map_err(|error| error.to_string())?;
    let jar = Arc::new(Jar::default());
    for (name, value) in &session.cookies {
        jar.add_cookie_str(&format!("{name}={value}; Path=/"), &passport_url);
    }
    let client = build_reqwest_client(jar.clone())?;
    let mut request = client
        .get(
            passport_url
                .join("newlogin/qrcode/generate.do")
                .map_err(|error| error.to_string())?,
        )
        .query(&build_qr_generate_params(&session.params));
    for (key, value) in qr_request_headers(QR_DEFAULT_RENDER_REFERER(), None) {
        request = request.header(key, value);
    }
    let payload = request
        .send()
        .await
        .map_err(|error| error.to_string())?
        .json::<Value>()
        .await
        .map_err(|error| error.to_string())?;
    session.cookies = jar_cookie_map(jar.as_ref(), &passport_url);

    let content = payload
        .get("content")
        .and_then(Value::as_object)
        .ok_or_else(|| "获取登录二维码失败".to_string())?;
    if !content
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("获取登录二维码失败".to_string());
    }
    let data = content
        .get("data")
        .and_then(Value::as_object)
        .ok_or_else(|| "二维码数据缺失".to_string())?;
    let qr_content = data
        .get("codeContent")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "二维码内容缺失".to_string())?;
    session.params.insert(
        "t".to_string(),
        data.get("t").map(json_value_to_string).unwrap_or_default(),
    );
    session.params.insert(
        "ck".to_string(),
        data.get("ck").map(json_value_to_string).unwrap_or_default(),
    );
    session.qr_content = qr_content.to_string();
    session.qr_code_url = build_qr_svg_data_url(qr_content)?;
    Ok(session)
}

fn build_scan_status_form_data(session: &QrSessionState) -> HashMap<String, String> {
    let mut form_data = session
        .params
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "_mini_login_url"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<HashMap<_, _>>();
    form_data.entry("ua".to_string()).or_default();
    form_data.insert("navlanguage".to_string(), "zh-CN".to_string());
    form_data.insert(
        "navUserAgent".to_string(),
        qr_default_user_agent().to_string(),
    );
    form_data.insert(
        "navPlatform".to_string(),
        infer_nav_platform(qr_default_user_agent()),
    );
    form_data.insert("isIframe".to_string(), "true".to_string());
    form_data.insert(
        "documentReferer".to_string(),
        QR_DEFAULT_RENDER_REFERER().to_string(),
    );
    form_data.insert(
        "bizParams".to_string(),
        QR_DEFAULT_LOGIN_BIZ_PARAMS().to_string(),
    );
    form_data
        .entry("defaultView".to_string())
        .or_insert_with(|| "sms".to_string());
    form_data.entry("deviceId".to_string()).or_default();
    form_data.entry("pageTraceId".to_string()).or_default();
    form_data.entry("bx-ua".to_string()).or_default();
    form_data.insert(
        "bx-umidtoken".to_string(),
        fallback_bx_umidtoken(session, &form_data),
    );
    form_data.insert("bx_et".to_string(), fallback_bx_et(session, &form_data));
    form_data.insert("x-pipu2".to_string(), fallback_x_pipu2(session, &form_data));
    form_data
}

fn cookie_header_value(cookies: &BTreeMap<String, String>) -> String {
    cookies
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn merge_set_cookie_headers(
    cookies: &mut BTreeMap<String, String>,
    headers: &reqwest::header::HeaderMap,
) {
    for value in headers.get_all(SET_COOKIE) {
        if let Ok(raw) = value.to_str() {
            if let Some((name, rest)) = raw.split_once('=') {
                let value = rest.split(';').next().unwrap_or_default();
                if !name.trim().is_empty() {
                    cookies.insert(name.trim().to_string(), value.to_string());
                }
            }
        }
    }
}

fn h5_sign(token: &str, timestamp: i64, app_key: &str, data: &str) -> String {
    format!(
        "{:x}",
        md5::compute(format!("{token}&{timestamp}&{app_key}&{data}").as_bytes())
    )
}

async fn bootstrap_h5tk(
    state: &AppState,
    mut cookies: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    cookies.remove("_m_h5_tk");
    cookies.remove("_m_h5_tk_enc");
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| error.to_string())?;
    let protocol = resolve_account_protocol(state, opcodes::ACCOUNT_USER_SIMPLE).await?;
    let api = protocol.api();
    let version = protocol.version();
    let data = "{}";
    let base = format!("{}/{}/{version}/", state.config().xianyu.h5_gateway, api);
    let first_ts = unix_millis_now();
    let first_query = vec![
        ("jsv", "2.7.2".to_string()),
        ("appKey", state.config().xianyu.h5_app_key.clone()),
        ("t", first_ts.to_string()),
        (
            "sign",
            h5_sign("", first_ts, &state.config().xianyu.h5_app_key, data),
        ),
        ("api", api.to_string()),
        ("v", version.to_string()),
        ("type", "json".to_string()),
        ("dataType", "json".to_string()),
        ("timeout", "20000".to_string()),
        ("data", data.to_string()),
    ];
    let first = client
        .get(&base)
        .query(&first_query)
        .header(USER_AGENT, qr_default_user_agent())
        .header(COOKIE, cookie_header_value(&cookies))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    merge_set_cookie_headers(&mut cookies, first.headers());
    let token = cookies
        .get("_m_h5_tk")
        .and_then(|value| value.split('_').next())
        .unwrap_or_default()
        .to_string();
    if token.is_empty() {
        return Ok(cookies);
    }

    let second_ts = unix_millis_now();
    let second_query = vec![
        ("jsv", "2.7.2".to_string()),
        ("appKey", state.config().xianyu.h5_app_key.clone()),
        ("t", second_ts.to_string()),
        (
            "sign",
            h5_sign(&token, second_ts, &state.config().xianyu.h5_app_key, data),
        ),
        ("api", api.to_string()),
        ("v", version.to_string()),
        ("type", "json".to_string()),
        ("dataType", "json".to_string()),
        ("timeout", "20000".to_string()),
        ("data", data.to_string()),
    ];
    let second = client
        .get(&base)
        .query(&second_query)
        .header(USER_AGENT, qr_default_user_agent())
        .header(COOKIE, cookie_header_value(&cookies))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    merge_set_cookie_headers(&mut cookies, second.headers());
    Ok(cookies)
}

async fn fetch_user_info(
    state: &AppState,
    cookies: &BTreeMap<String, String>,
) -> Result<(String, String), String> {
    let h5_token = cookies.get("_m_h5_tk").cloned().unwrap_or_default();
    let token = h5_token.split('_').next().unwrap_or_default();
    if token.is_empty() {
        return Ok((String::new(), String::new()));
    }
    let mut data = json!({
        "utdid": "h5_client",
        "platform": "ios",
        "miniAppVersion": "0.3.77",
    });
    if let Some(uid) = cookies.get("unb").filter(|value| !value.is_empty()) {
        data["userId"] = Value::String(uid.clone());
    }
    let data_str = serde_json::to_string(&data).map_err(|error| error.to_string())?;
    let timestamp = unix_millis_now();
    let protocol = resolve_account_protocol(state, opcodes::ACCOUNT_WX_USER_PAGE_HEAD).await?;
    let api = protocol.api();
    let version = protocol.version();
    let query = vec![
        ("jsv", "2.7.2".to_string()),
        ("appKey", state.config().xianyu.h5_app_key.clone()),
        ("t", timestamp.to_string()),
        (
            "sign",
            h5_sign(
                token,
                timestamp,
                &state.config().xianyu.h5_app_key,
                &data_str,
            ),
        ),
        ("api", api.to_string()),
        ("v", version.to_string()),
        ("type", "json".to_string()),
        ("dataType", "json".to_string()),
        ("timeout", "20000".to_string()),
        ("data", data_str),
    ];
    let url = format!("{}/{}/{version}/", state.config().xianyu.h5_gateway, api);
    let payload = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .query(&query)
        .header(USER_AGENT, qr_default_user_agent())
        .header(COOKIE, cookie_header_value(cookies))
        .send()
        .await
        .map_err(|error| error.to_string())?
        .json::<Value>()
        .await
        .map_err(|error| error.to_string())?;
    let base = payload
        .get("data")
        .and_then(|value| value.get("module"))
        .and_then(|value| value.get("base"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let avatar = base
        .get("avatar")
        .and_then(Value::as_object)
        .and_then(|value| value.get("avatar"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let nick = base
        .get("displayName")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((nick, avatar))
}

async fn upsert_account_from_qr(
    state: &AppState,
    owner_username: &str,
    session: &mut QrSessionState,
) -> Result<(), QrImportError> {
    if session.imported {
        return Ok(());
    }
    let mocked_passport = qr_passport_host() != QR_DEFAULT_PASSPORT_HOST();
    let mut cookies = session.cookies.clone();
    if !mocked_passport && !cookies.contains_key("_m_h5_tk") {
        let existing_cookies = cookies.clone();
        cookies = bootstrap_h5tk(state, cookies)
            .await
            .unwrap_or(existing_cookies);
    }
    let (mut nick, avatar) = if mocked_passport {
        (String::new(), String::new())
    } else {
        fetch_user_info(state, &cookies).await.unwrap_or_default()
    };
    let uid = cookies.get("unb").cloned().unwrap_or_default();
    if uid.is_empty() {
        return Ok(());
    }
    session.account_uid = uid.clone();
    if nick.trim().is_empty() {
        nick = cookies
            .get("tracknick")
            .or_else(|| cookies.get("lgc"))
            .or_else(|| cookies.get("_nk_"))
            .cloned()
            .unwrap_or_else(|| uid.clone());
    }
    let now = unix_seconds_now();
    let extra = if avatar.is_empty() {
        json!({})
    } else {
        json!({ "avatar": avatar })
    };
    match claim_binding(state, &session.license_auth_token, &uid, &nick).await {
        Ok(()) => {}
        Err(ClaimBindingError::OwnerConflict(message)) => {
            return Err(QrImportError::OwnerConflict(message));
        }
        Err(ClaimBindingError::Other(error)) => {
            return Err(QrImportError::Other(error.detail().to_string()));
        }
    }
    let result = sqlx::query(
        r#"
        INSERT INTO accounts (
            uid, owner_username, nick, token, sid, cookies, browser_cookie_jar, status, created_at, updated_at, bound_at,
            cookie_refreshed_at, last_session_check_at, last_session_ok_at, first_session_fail_at,
            last_session_status, last_session_reason, extra, security_score, security_score_updated_at,
            restriction_end, coin_balance, violation_count, fish_power, total_earned,
            fish_shop_enabled, fish_shop_level, seller_30d_sold
        )
        VALUES (?, ?, ?, ?, ?, ?, '[]', ?, ?, ?, ?, ?, ?, ?, 0, 'ok', '', ?, 0, 0, '', 0, 0, 0, 0, 0, '', '')
        ON CONFLICT(uid) DO UPDATE SET
            owner_username = excluded.owner_username,
            nick = excluded.nick,
            token = excluded.token,
            sid = excluded.sid,
            cookies = excluded.cookies,
            status = excluded.status,
            updated_at = excluded.updated_at,
            bound_at = CASE
                WHEN accounts.owner_username = excluded.owner_username
                    THEN COALESCE(NULLIF(accounts.bound_at, 0), excluded.bound_at)
                ELSE excluded.bound_at
            END,
            cookie_refreshed_at = excluded.cookie_refreshed_at,
            last_session_check_at = excluded.last_session_check_at,
            last_session_ok_at = excluded.last_session_ok_at,
            last_session_status = excluded.last_session_status,
            last_session_reason = excluded.last_session_reason,
            extra = excluded.extra
        WHERE ? = '' OR accounts.owner_username = ? OR ? = 1
        "#,
    )
    .bind(&uid)
    .bind(owner_username)
    .bind(&nick)
    .bind(cookies.get("_m_h5_tk").cloned().unwrap_or_default())
    .bind(
        cookies
            .get("sid")
            .or_else(|| cookies.get("cookie2"))
            .cloned()
            .unwrap_or_default(),
    )
    .bind(serde_json::to_string(&cookies).map_err(|error| error.to_string())?)
    .bind(if cookies.contains_key("_m_h5_tk") { "active" } else { "expired" })
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(now)
    .bind(serde_json::to_string(&extra).map_err(|error| error.to_string())?)
    .bind(owner_username)
    .bind(owner_username)
    .bind(1_i64)
    .execute(state.storage().accounts())
    .await
    .map_err(|error| error.to_string())?;
    if result.rows_affected() == 0 {
        return Err(QrImportError::OwnerConflict(
            OWNER_CONFLICT_MESSAGE.to_string(),
        ));
    }
    session.account_uid = uid;
    session.account_nick = nick;
    session.cookies = cookies;
    session.imported = true;
    Ok(())
}

#[derive(Debug)]
enum QrImportError {
    OwnerConflict(String),
    Other(String),
}

impl QrImportError {
    fn message(&self) -> &str {
        match self {
            Self::OwnerConflict(message) | Self::Other(message) => message,
        }
    }
}

impl From<String> for QrImportError {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

#[derive(Debug)]
enum QrPollError {
    Internal(String),
}

impl QrPollError {
    fn internal(error: impl std::fmt::Display) -> Self {
        Self::Internal(error.to_string())
    }
}

async fn poll_qr_status(
    state: &AppState,
    owner_username: &str,
    session: &mut QrSessionState,
) -> Result<(), QrPollError> {
    if matches!(
        session.status.as_str(),
        "success" | "expired" | "cancelled" | "verification_required" | "owner_conflict"
    ) {
        return Ok(());
    }
    if let Some(ttl_secs) = qr_local_ttl_secs() {
        if unix_seconds_now() - session.created_at >= ttl_secs {
            session.status = "expired".to_string();
            session.message = "二维码已过期，请重新生成".to_string();
            return Ok(());
        }
    }

    let passport_host = qr_passport_host();
    let passport_url = Url::parse(&passport_host).map_err(QrPollError::internal)?;
    let jar = Arc::new(Jar::default());
    for (name, value) in &session.cookies {
        jar.add_cookie_str(&format!("{name}={value}; Path=/"), &passport_url);
    }
    let client = build_reqwest_client(jar.clone()).map_err(QrPollError::Internal)?;
    let mut request = client.post(
        passport_url
            .join("newlogin/qrcode/query.do")
            .map_err(QrPollError::internal)?,
    );
    for (key, value) in qr_request_headers(
        session
            .params
            .get("_mini_login_url")
            .map(String::as_str)
            .unwrap_or(QR_DEFAULT_RENDER_REFERER()),
        Some("application/x-www-form-urlencoded"),
    ) {
        request = request.header(key, value);
    }
    request = request.header("bx-v", QR_BAXIA_VERSION);
    let payload = request
        .form(&build_scan_status_form_data(session))
        .send()
        .await
        .map_err(QrPollError::internal)?
        .json::<Value>()
        .await
        .map_err(QrPollError::internal)?;
    session.cookies = jar_cookie_map(jar.as_ref(), &passport_url);
    let status = payload
        .get("content")
        .and_then(|value| value.get("data"))
        .and_then(|value| value.get("qrCodeStatus"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    match status {
        "NEW" => {
            session.status = "waiting".to_string();
        }
        "SCANED" => {
            session.status = "scanned".to_string();
        }
        "CONFIRMED" => {
            let verification_required = payload
                .get("content")
                .and_then(|value| value.get("data"))
                .and_then(|value| value.get("iframeRedirect"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if verification_required {
                session.status = "verification_required".to_string();
                session.message = "账号被风控，需要手机验证".to_string();
                session.verification_url = payload
                    .get("content")
                    .and_then(|value| value.get("data"))
                    .and_then(|value| value.get("iframeRedirectUrl"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            } else if let Err(error) = upsert_account_from_qr(state, owner_username, session).await
            {
                let error_code = match &error {
                    QrImportError::OwnerConflict(_) => "owner_conflict",
                    QrImportError::Other(_) => "import_error",
                };
                warn!(
                    target: "client.telemetry.action",
                    stream = "runtime",
                    module = "accounts-qr",
                    action = "import_qr_account",
                    outcome = "failed",
                    username = %owner_username,
                    account_uid = %session.account_uid,
                    error_code,
                    error = %error.message(),
                    retryable = matches!(&error, QrImportError::Other(_)),
                    "qr account import failed"
                );
                match error {
                    QrImportError::OwnerConflict(message) => {
                        session.status = "owner_conflict".to_string();
                        session.owner_conflict = true;
                        session.message = message;
                        return Ok(());
                    }
                    QrImportError::Other(message) => {
                        session.status = "import_error".to_string();
                        session.message = format!("账号已确认，但导入失败：{message}");
                    }
                }
            } else {
                session.status = "success".to_string();
                session.message.clear();
            }
        }
        "EXPIRED" => {
            session.status = "expired".to_string();
            session.message = "二维码已过期，请重新生成".to_string();
        }
        "" => {
            session.message = "扫码状态查询失败".to_string();
        }
        _ => {
            session.status = "cancelled".to_string();
        }
    }
    Ok(())
}

pub(super) async fn qr_generate(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    let session_id = new_qr_session_id();
    let mut session = generate_qr_session(&session_id)
        .await
        .map_err(ResponseError::internal_message)?;
    session.owner_username = qr_owner_username(&state, &identity);
    let qr_code_url = session.qr_code_url.clone();
    let created_at = session.created_at;
    let expires_in = qr_expires_in_seconds(&session);
    qr_sessions()
        .lock()
        .unwrap()
        .insert(session_id.clone(), session);

    Ok(ResponseJson(json!({
        "success": true,
        "session_id": session_id,
        "qr_code_url": qr_code_url,
        "status": "waiting",
        "created_at": created_at,
        "age_seconds": 0,
        "expires_in": expires_in,
    })))
}

/// Crate-internal helper for the WeChat command router (`/添加账号`).
///
/// Same shape as `qr_generate` but returns the raw QR content (the
/// `goofish.com://login?...` URL the official client wants to render) so
/// the caller can re-render the QR as a PNG and ship it to a non-web
/// surface. Session is stashed under the returned `session_id`; downstream
/// pollers can call `qr_status` against the same id to watch login state.
pub(crate) async fn start_qr_session_for_command(
    state: &AppState,
    owner_username: &str,
) -> Result<(String, String), String> {
    let session_id = new_qr_session_id();
    let mut session = generate_qr_session(&session_id).await?;
    session.owner_username = owner_username.trim().to_string();
    let qr_content = session.qr_content.clone();
    qr_sessions()
        .lock()
        .unwrap()
        .insert(session_id.clone(), session);
    Ok((session_id, qr_content))
}

pub(super) async fn qr_status(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(session_id): Path<String>,
) -> ApiResult {
    let mut session = {
        let sessions = qr_sessions().lock().unwrap();
        sessions
            .get(&session_id)
            .cloned()
            .ok_or_else(|| ResponseError::not_found("会话不存在"))?
    };
    let owner_username = qr_owner_username(&state, &identity);
    ensure_qr_session_access(&state, &identity, &owner_username, &session)?;
    let poll_result = poll_qr_status(&state, &owner_username, &mut session).await;
    qr_sessions()
        .lock()
        .unwrap()
        .insert(session_id, session.clone());
    if let Err(error) = poll_result {
        return match error {
            QrPollError::Internal(message) => Err(ResponseError::internal_message(message)),
        };
    }

    Ok(ResponseJson(json!({
        "status": session.status,
        "message": session.message,
        "account_uid": session.account_uid,
        "account_nick": session.account_nick,
        "owner_conflict": session.owner_conflict,
        "can_apply_unbind": session.owner_conflict && !owner_username.trim().is_empty(),
        "verification_url": session.verification_url,
        "created_at": session.created_at,
        "age_seconds": qr_age_seconds(&session),
        "expires_in": qr_expires_in_seconds(&session),
    })))
}

fn qr_owner_username(_state: &AppState, identity: &RequestIdentity) -> String {
    identity.username.trim().to_string()
}

fn ensure_qr_session_access(
    _state: &AppState,
    identity: &RequestIdentity,
    owner_username: &str,
    session: &QrSessionState,
) -> Result<(), ResponseError> {
    if has_unscoped_legacy_access(identity) {
        return Ok(());
    }
    if !session.owner_username.is_empty() && session.owner_username == owner_username {
        return Ok(());
    }
    Err(ResponseError::forbidden("无权访问该扫码会话"))
}

fn new_qr_session_id() -> String {
    format!("qr-{}", random_nonce_hex())
}

#[cfg(test)]
mod tests {
    use super::{ensure_qr_session_access, qr_owner_username, QrSessionState};
    use crate::{models::auth::RequestIdentity, state::AppState};

    #[tokio::test]
    async fn local_identity_can_access_qr_session_from_any_legacy_owner() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        std::env::set_var("APP_DATA_DIR", temp_dir.path());
        let state = AppState::bootstrap(crate::config::AppConfig::from_env().unwrap())
            .await
            .unwrap();
        let identity =
            RequestIdentity::new("local-app".to_string(), "admin".to_string(), true, true);
        let session = QrSessionState {
            status: "waiting".to_string(),
            message: String::new(),
            owner_username: "former-user".to_string(),
            license_auth_token: String::new(),
            owner_conflict: false,
            created_at: 0.0,
            account_uid: String::new(),
            account_nick: String::new(),
            qr_code_url: String::new(),
            qr_content: String::new(),
            cookies: Default::default(),
            params: Default::default(),
            verification_url: String::new(),
            imported: false,
        };

        assert_eq!(qr_owner_username(&state, &identity), "local-app");
        assert!(ensure_qr_session_access(&state, &identity, "local-app", &session).is_ok());
        std::env::remove_var("APP_DATA_DIR");
    }
}
