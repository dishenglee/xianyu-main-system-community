use axum::extract::{Extension, Json, Query, State};
use axum::Json as ResponseJson;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    models::auth::RequestIdentity,
    routes::accounts::runtime_access::{
        ensure_account_can_call_upstream, ACCOUNT_DELETE_ONLY_MESSAGE,
    },
    routes::error::{ApiResult, ResponseError},
    state::AppState,
    xianyu_client::{self, LocationContext},
};

use super::publish::{AwesomeDetailPayload, ItemImportPayload, SellerRecommendQuery};
use super::{
    display_price_string, ensure_market_uid_access, image_urls_from_value,
    load_location_or_default, resolve_no_bargain, successful_upstream_h5_request,
    upstream_response_with_api, value_is_trueish,
};

// ─── import helper functions ──────────────────────────────────────────────────

static URL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"https?://[^\s]+").expect("valid import url regex"));

fn short_text_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)[..12].to_string()
}

fn extract_item_id_from_text(text: &str) -> Option<String> {
    for marker in ["itemId=", "id="] {
        if let Some(index) = text.find(marker) {
            let value = text[index + marker.len()..]
                .chars()
                .take_while(|char| char.is_ascii_digit())
                .collect::<String>();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn extract_url_candidates(text: &str) -> Vec<String> {
    URL_REGEX
        .find_iter(text)
        .map(|matched| {
            matched.as_str().trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\''
                        | ','
                        | '，'
                        | '.'
                        | '。'
                        | ';'
                        | '；'
                        | '!'
                        | '！'
                        | '?'
                        | '？'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '<'
                        | '>'
                        | '【'
                        | '】'
                        | '（'
                        | '）'
                )
            })
        })
        .filter(|trimmed| trimmed.starts_with("http://") || trimmed.starts_with("https://"))
        .map(str::to_string)
        .collect()
}

fn text_looks_like_import_url(value: &str) -> bool {
    let text = value.trim();
    text.starts_with("http://")
        || text.starts_with("https://")
        || text.contains(crate::sealed_protocol::sealed_str!("goofish.com/item"))
        || text.contains("m.tb.cn/")
}

fn import_detail_is_usable(response: &Value, raw_text: &str) -> bool {
    let title = response
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if title.is_empty() || text_looks_like_import_url(title) {
        return false;
    }
    let desc = response
        .get("desc")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let urls = extract_url_candidates(raw_text);
    if text_looks_like_import_url(desc)
        && (desc == raw_text.trim() || urls.iter().any(|url| url == desc))
    {
        return false;
    }
    true
}

async fn resolve_item_id_from_links(text: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .ok()?;

    for url in extract_url_candidates(text) {
        if let Some(item_id) = extract_item_id_from_text(&url) {
            return Some(item_id);
        }

        let response = client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
            )
            .send()
            .await
            .ok()?;
        let final_url = response.url().to_string();
        if let Some(item_id) = extract_item_id_from_text(&final_url) {
            return Some(item_id);
        }
        let body = response.text().await.ok()?;
        if let Some(item_id) = extract_item_id_from_text(&body) {
            return Some(item_id);
        }
    }

    None
}

async fn fetch_import_detail(
    state: &AppState,
    uid: &str,
    item_id: &str,
    raw_text: &str,
) -> Result<Value, String> {
    let mut errors = Vec::new();
    for fetch_uid in import_detail_fetch_uids(state, uid).await {
        match fetch_import_detail_with_uid(state, &fetch_uid, item_id, raw_text).await {
            Ok(data) => {
                if fetch_uid != uid.trim() {
                    tracing::info!(
                        uid = %uid,
                        fetch_uid = %fetch_uid,
                        item_id = %item_id,
                        "import link detail fetched through same-owner fallback account"
                    );
                }
                return Ok(data);
            }
            Err(error) => errors.push(format!("{fetch_uid}: {error}")),
        }
    }
    Err(errors
        .last()
        .cloned()
        .unwrap_or_else(|| "import detail unavailable".to_string()))
}

async fn import_detail_fetch_uids(state: &AppState, uid: &str) -> Vec<String> {
    let uid = uid.trim();
    if uid.is_empty() {
        return Vec::new();
    }
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT owner_username FROM accounts WHERE uid = ? AND status != 'disabled' LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(state.storage().accounts())
    .await
    .ok()
    .flatten()
    .unwrap_or_default();
    let owner = owner.trim().to_string();
    if owner.is_empty() {
        return vec![uid.to_string()];
    }
    let mut uids = sqlx::query_scalar::<_, String>(
        r#"
        SELECT uid
        FROM accounts
        WHERE owner_username = ?
          AND status = 'active'
        ORDER BY
          CASE WHEN uid = ? THEN 0 ELSE 1 END,
          updated_at DESC,
          bound_at DESC,
          uid ASC
        LIMIT 8
        "#,
    )
    .bind(&owner)
    .bind(uid)
    .fetch_all(state.storage().accounts())
    .await
    .unwrap_or_default();
    if !uids.iter().any(|candidate| candidate == uid) {
        uids.insert(0, uid.to_string());
    }
    uids.dedup();
    uids
}

async fn fetch_import_detail_with_uid(
    state: &AppState,
    fetch_uid: &str,
    item_id: &str,
    raw_text: &str,
) -> Result<Value, String> {
    let location = load_location_or_default(state, fetch_uid).await;
    let attempts = [
        (
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.pc.detail"),
            "1.0",
            json!({ "itemId": item_id }),
        ),
        (
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.awesome.detail"),
            "1.0",
            awesome_detail_payload(item_id, &location.city, &location, true),
        ),
        (
            crate::sealed_protocol::sealed_str!("mtop.taobao.idle.awesome.detail.unit"),
            "1.0",
            awesome_detail_payload(item_id, &location.city, &location, false),
        ),
    ];
    let mut last_error = String::new();
    for (api, version, payload) in attempts {
        match successful_upstream_h5_request(state, fetch_uid, api, version, payload, Vec::new())
            .await
        {
            Ok(upstream) => {
                let data = upstream.get("data").unwrap_or(&Value::Null);
                let response = build_import_link_response(data, item_id);
                if import_detail_is_usable(&response, raw_text) {
                    return Ok(data.clone());
                }
                last_error = format!("{api} returned unusable import detail");
            }
            Err(error) => {
                last_error = format!("{api}: {error}");
            }
        }
    }
    Err(last_error)
}

fn import_image_filename(item_id: &str, index: usize, url: &str) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let extension = path
        .rsplit('.')
        .next()
        .filter(|ext| {
            !ext.contains('/')
                && !ext.is_empty()
                && ext.chars().all(|ch| ch.is_ascii_alphanumeric())
                && ext.len() <= 5
        })
        .unwrap_or("jpg");
    format!("import_{item_id}_{index}.{extension}")
}

fn guess_image_content_type(filename: &str) -> &'static str {
    match filename
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        _ => "image/jpeg",
    }
}

async fn reupload_import_image_urls(
    state: &AppState,
    uid: &str,
    item_id: &str,
    urls: &[String],
) -> Vec<String> {
    if urls.is_empty() {
        return Vec::new();
    }

    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
    {
        Ok(client) => client,
        Err(_) => return urls.to_vec(),
    };

    let mut results = Vec::new();
    for (index, url) in urls.iter().take(9).enumerate() {
        let uploaded = match client
            .get(url)
            .header(
                reqwest::header::USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36",
            )
            .send()
            .await
        {
            Ok(response) => match response.bytes().await {
                Ok(bytes) if !bytes.is_empty() => {
                    let filename = import_image_filename(item_id, index, url);
                    let content_type = guess_image_content_type(&filename);
                    xianyu_client::stream_upload(
                        state.config(),
                        state.storage().accounts(),
                        uid,
                        &filename,
                        content_type,
                        bytes.to_vec(),
                        false,
                    )
                    .await
                    .map(|result| result.url)
                    .ok()
                }
                _ => None,
            },
            Err(_) => None,
        };
        results.push(uploaded.unwrap_or_else(|| url.clone()));
    }

    results
}

fn clean_import_price_text(text: &str) -> String {
    text.trim()
        .trim_start_matches('¥')
        .trim_start_matches('￥')
        .replace(',', "")
        .trim()
        .to_string()
}

fn value_string(value: Option<&Value>) -> String {
    value
        .and_then(|value| match value {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

fn first_non_empty(values: impl IntoIterator<Item = String>) -> String {
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .unwrap_or_default()
}

fn flow_components(flow_data: &Value) -> Vec<&Value> {
    flow_data
        .get("body")
        .and_then(|body| body.get("sections"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|section| {
            section
                .get("components")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .collect()
}

fn flow_component_data_string(components: &[&Value], key: &str) -> String {
    first_non_empty(
        components
            .iter()
            .map(|component| value_string(component.get("data").and_then(|data| data.get(key)))),
    )
}

fn flow_component_req_param_string(components: &[&Value], key: &str) -> String {
    first_non_empty(components.iter().map(|component| {
        value_string(
            component
                .get("data")
                .and_then(|data| data.get("reqParams"))
                .and_then(|params| params.get(key)),
        )
    }))
}

fn flow_component_price_note_string(components: &[&Value], key: &str) -> String {
    first_non_empty(components.iter().map(|component| {
        value_string(
            component
                .get("data")
                .and_then(|data| data.get("priceWithNote"))
                .and_then(|note| note.get(key)),
        )
    }))
}

fn flow_share_info_json(flow_data: &Value) -> Option<Value> {
    let share_info = flow_data
        .get("context")
        .and_then(|context| context.get("shareInfo"))
        .and_then(Value::as_str)?;
    let share_info: Value = serde_json::from_str(share_info).ok()?;
    let nested = share_info
        .get("shareInfoJsonString")
        .and_then(Value::as_str)?;
    serde_json::from_str(nested).ok()
}

fn flow_share_main_params(flow_data: &Value) -> Option<Value> {
    flow_share_info_json(flow_data)?
        .get(crate::sealed_protocol::sealed_str!("contentParams"))?
        .get(crate::sealed_protocol::sealed_str!("mainParams"))
        .cloned()
}

fn collect_image_urls_from_array(array: Option<&Vec<Value>>, keys: &[&str]) -> Vec<String> {
    array
        .into_iter()
        .flatten()
        .filter_map(|item| match item {
            Value::String(url) => Some(url.trim().to_string()),
            Value::Object(object) => keys.iter().find_map(|key| {
                object
                    .get(*key)
                    .and_then(Value::as_str)
                    .map(|url| url.trim().to_string())
            }),
            _ => None,
        })
        .filter(|url| !url.is_empty())
        .collect()
}

fn flow_image_urls(flow_data: &Value, components: &[&Value]) -> Vec<String> {
    let mut urls = Vec::new();
    for component in components {
        urls.extend(collect_image_urls_from_array(
            component
                .get("data")
                .and_then(|data| data.get("mediaInfos"))
                .and_then(Value::as_array),
            &["url", "imageUrl", "image", "picUrl"],
        ));
    }
    if urls.is_empty() {
        if let Some(main_params) = flow_share_main_params(flow_data) {
            urls.extend(collect_image_urls_from_array(
                main_params.get("images").and_then(Value::as_array),
                &["image", "url", "imageUrl", "picUrl"],
            ));
        }
    }
    if urls.is_empty() {
        if let Some(main_pic) = flow_data
            .get("trackParams")
            .and_then(|track| track.get("mainPic"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
        {
            urls.push(main_pic.to_string());
        }
    }
    urls
}

fn first_line_or_empty(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_string()
}

fn flow_data_import_item(flow_data: &Value, item_id: &str) -> Option<Value> {
    let components = flow_components(flow_data);
    let share_main_params = flow_share_main_params(flow_data);
    let desc = first_non_empty([
        value_string(
            flow_data
                .get("context")
                .and_then(|context| context.get("desc")),
        ),
        flow_component_data_string(&components, "desc"),
        share_main_params
            .as_ref()
            .map(|params| value_string(params.get("content")))
            .unwrap_or_default(),
    ]);
    let title = first_non_empty([
        flow_component_req_param_string(&components, "title"),
        flow_component_data_string(&components, "title"),
        value_string(
            flow_data
                .get("context")
                .and_then(|context| context.get("title")),
        ),
        first_line_or_empty(&desc),
    ]);
    if title.trim().is_empty() && desc.trim().is_empty() {
        return None;
    }
    let price = first_non_empty([
        flow_component_data_string(&components, "price"),
        flow_component_price_note_string(&components, "originalPriceText"),
        share_main_params
            .as_ref()
            .and_then(|params| params.get("extra"))
            .map(|extra| value_string(extra.get("soldPrice")))
            .unwrap_or_default(),
    ]);
    let orig_price = first_non_empty([
        flow_component_price_note_string(&components, "strikethroughText"),
        flow_component_price_note_string(&components, "marketPriceText"),
    ]);
    Some(json!({
        "itemId": item_id,
        "title": title,
        "desc": desc,
        "price": clean_import_price_text(&price),
        "origPrice": clean_import_price_text(&orig_price),
        "imageUrls": flow_image_urls(flow_data, &components),
        "simpleItem": false,
        "noBargain": false,
    }))
}

fn import_item_do_from_raw(raw: &Value, item_id: &str) -> Value {
    if let Some(item_do) = raw.get(crate::sealed_protocol::sealed_str!("itemDO")) {
        return item_do.clone();
    }
    if let Some(flow_data) = raw.get("flowData") {
        if let Some(item) = flow_data_import_item(flow_data, item_id) {
            return item;
        }
    }
    raw.clone()
}

fn build_import_link_response(raw: &Value, item_id: &str) -> Value {
    let item_do = import_item_do_from_raw(raw, item_id);
    let simple_item = item_do
        .get("simpleItem")
        .map(value_is_trueish)
        .unwrap_or(false);
    let no_bargain = resolve_no_bargain(item_do.get("noBargain"), item_do.get("bargain"), false);
    json!({
        "itemId": item_id,
        "title": item_do.get("title").and_then(Value::as_str).unwrap_or_default(),
        "desc": item_do.get("desc").or_else(|| item_do.get("description")).and_then(Value::as_str).unwrap_or_default(),
        "price": display_price_string(item_do.get("soldPrice").or_else(|| item_do.get("price")).or_else(|| item_do.get("defaultPrice"))),
        "origPrice": display_price_string(item_do.get("originalPrice").or_else(|| item_do.get("origPrice"))),
        "imageUrls": image_urls_from_value(&item_do),
        "simpleItem": simple_item,
        "noBargain": no_bargain,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_import_link_response, extract_item_id_from_text, extract_url_candidates,
        import_detail_is_usable,
    };
    use serde_json::json;

    #[test]
    fn build_import_link_response_defaults_flags_to_false_when_missing() {
        let payload = build_import_link_response(
            &json!({
                "itemDO": {
                    "title": "测试商品",
                    "desc": "测试描述"
                }
            }),
            "item-1",
        );

        assert_eq!(payload["simpleItem"], false);
        assert_eq!(payload["noBargain"], false);
    }

    #[test]
    fn build_import_link_response_falls_back_to_inverse_bargain_flag() {
        let payload = build_import_link_response(
            &json!({
                "itemDO": {
                    "title": "测试商品",
                    "desc": "测试描述",
                    "bargain": 0
                }
            }),
            "item-1",
        );

        assert_eq!(payload["noBargain"], true);
    }

    #[test]
    fn build_import_link_response_reads_awesome_flow_data() {
        let payload = build_import_link_response(
            &json!({
                "flowData": {
                    "context": {
                        "desc": "pentair/滨特尔净水器龙头\n美国进口净水器原装"
                    },
                    "trackParams": {
                        "mainPic": "https://img.example.com/fallback.jpg"
                    },
                    "body": {
                        "sections": [{
                            "components": [
                                {
                                    "data": {
                                        "price": "120",
                                        "priceWithNote": {
                                            "strikethroughText": "¥126"
                                        }
                                    }
                                },
                                {
                                    "data": {
                                        "mediaInfos": [
                                            { "url": "https://img.example.com/1.heic" },
                                            { "url": "https://img.example.com/2.heic" }
                                        ]
                                    }
                                },
                                {
                                    "data": {
                                        "reqParams": {
                                            "title": "pentair/滨特尔净水器龙头"
                                        }
                                    }
                                }
                            ]
                        }]
                    }
                }
            }),
            "897040648270",
        );

        assert_eq!(payload["title"], "pentair/滨特尔净水器龙头");
        assert_eq!(
            payload["desc"],
            "pentair/滨特尔净水器龙头\n美国进口净水器原装"
        );
        assert_eq!(payload["price"], "120");
        assert_eq!(payload["origPrice"], "126");
        assert_eq!(
            payload["imageUrls"],
            json!([
                "https://img.example.com/1.heic",
                "https://img.example.com/2.heic"
            ])
        );
        assert!(import_detail_is_usable(
            &payload,
            crate::sealed_protocol::sealed_str!("https://www.goofish.com/item?id=897040648270")
        ));
    }

    #[test]
    fn extracts_item_id_from_goofish_url_with_extra_params() {
        assert_eq!(
            extract_item_id_from_text(crate::sealed_protocol::sealed_str!(
                "https://www.goofish.com/item?spm=a21ybx.home&id=897040648270&categoryId=126854861"
            ),),
            Some("897040648270".to_string()),
        );
    }

    #[test]
    fn extracts_short_link_from_xianyu_share_text() {
        let urls = extract_url_candidates(
            "【闲鱼】https://m.tb.cn/h.RDFa8gI?tk=naU8gm0PBck tG-#22>lD 「快来捡漏」",
        );
        assert_eq!(
            urls,
            vec!["https://m.tb.cn/h.RDFa8gI?tk=naU8gm0PBck".to_string()],
        );
    }

    #[test]
    fn rejects_import_detail_that_echoes_the_link_as_title() {
        let payload = json!({
            "title": crate::sealed_protocol::sealed_str!("https://www.goofish.com/item?id=897040648270"),
            "desc": crate::sealed_protocol::sealed_str!("https://www.goofish.com/item?id=897040648270"),
            "price": "99.00"
        });
        assert!(!import_detail_is_usable(
            &payload,
            crate::sealed_protocol::sealed_str!("https://www.goofish.com/item?id=897040648270"),
        ));
    }
}

pub(super) fn awesome_detail_payload(
    item_id: &str,
    city: &str,
    location: &LocationContext,
    include_enter_type: bool,
) -> Value {
    use serde_json::Map;
    let city = if city.trim().is_empty() {
        location.city.trim()
    } else {
        city.trim()
    };
    let mut object = Map::new();
    object.insert(
        "abInfo".to_string(),
        json!("{\"bucketId\":\"16\",\"options\":\"OldDetail|Skill_New_Render|Plan_A|BottomBar_Right_Chat|Share_B|Image_Hight_Quality\"}"),
    );
    object.insert(
        "apiName".to_string(),
        json!("com.taobao.idlefish.detail.business.model.protocol.DetailRequest"),
    );
    object.insert("city".to_string(), json!(city));
    object.insert(
        "extraParam".to_string(),
        json!("{\"transmitSignal\":\"1\"}"),
    );
    object.insert("flowVersion".to_string(), json!("8.0"));
    object.insert("hitNativeDetail".to_string(), json!(true));
    object.insert("isBundleGroup".to_string(), json!(false));
    object.insert("isOld".to_string(), json!(false));
    object.insert("itemId".to_string(), json!(item_id));
    object.insert("needSimpleDetail".to_string(), json!(false));
    object.insert("originJson".to_string(), json!(false));
    if include_enter_type {
        object.insert("enterType".to_string(), json!("newPublish"));
    }
    Value::Object(object)
}

// ─── import / resell handlers ─────────────────────────────────────────────────

pub(super) async fn item_import_link(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ItemImportPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    ensure_account_can_call_upstream(&state, None, &payload.uid).await?;
    let text = payload.text.trim();
    let item_id = if let Some(item_id) = extract_item_id_from_text(text) {
        item_id
    } else {
        resolve_item_id_from_links(text).await.ok_or_else(|| {
            ResponseError::bad_request("未能从文本中识别商品 ID，请粘贴完整的闲鱼商品链接或口令")
        })?
    };
    let data = fetch_import_detail(&state, &payload.uid, &item_id, text)
        .await
        .map_err(|error| {
            let url_count = URL_REGEX.find_iter(text).count();
            tracing::warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "market-publish-import",
                action = "import_link_detail",
                outcome = "failed",
                uid = %payload.uid,
                account_uid = %payload.uid,
                item_id = %item_id,
                text_len = text.chars().count(),
                url_count,
                text_hash = %short_text_hash(text),
                contains_goofish = text.contains(crate::sealed_protocol::sealed_str!("goofish.com")),
                contains_taobao_shortlink = text.contains("m.tb.cn"),
                error_code = "import_link_detail_failed",
                retryable = !error.contains(ACCOUNT_DELETE_ONLY_MESSAGE),
                error = %error,
                "import link detail fetch failed"
            );
            if error.contains(ACCOUNT_DELETE_ONLY_MESSAGE) {
                ResponseError::forbidden(ACCOUNT_DELETE_ONLY_MESSAGE)
            } else {
                ResponseError::service_unavailable("导入商品详情失败，请稍后重试")
            }
        })?;
    let item_do = import_item_do_from_raw(&data, &item_id);
    let image_urls = reupload_import_image_urls(
        &state,
        &payload.uid,
        &item_id,
        &image_urls_from_value(&item_do),
    )
    .await;
    let mut response = build_import_link_response(&data, &item_id);
    if let Some(object) = response.as_object_mut() {
        object.insert(
            "imageUrls".to_string(),
            Value::Array(image_urls.into_iter().map(Value::from).collect()),
        );
    }
    Ok(ResponseJson(response))
}

pub(super) async fn item_seller_recommend(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SellerRecommendQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let api =
        crate::sealed_protocol::sealed_str!("mtop.taobao.idle.activity.sell.seller.recommend");
    let version = "2.0";
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        api,
        version,
        json!({
            "pageNumber": query.page.max(1),
            "pageSize": query.page_size.clamp(1, 50),
            "scene": "toBuyMatchInWofabude",
            "sceneId": 0,
            "sourceId": "tobuy_wofabude",
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream_response_with_api(
        api, version, upstream,
    )))
}

pub(super) async fn item_resell_feeds(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<SellerRecommendQuery>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &query.uid).await?;
    let location = load_location_or_default(&state, &query.uid).await;
    let api = crate::sealed_protocol::sealed_str!("mtop.taobao.idle.post.pub.resell.feeds");
    let version = "1.0";
    let upstream = successful_upstream_h5_request(
        &state,
        &query.uid,
        api,
        version,
        json!({
            "fishAdCode": location.fish_ad_code,
            "gps": location.gps,
            "inputJson": json!({ "pageNumber": query.page.max(1) }).to_string(),
        }),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream_response_with_api(
        api, version, upstream,
    )))
}

pub(super) async fn item_awesome_detail(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<AwesomeDetailPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let api = crate::sealed_protocol::sealed_str!("mtop.taobao.idle.awesome.detail");
    let version = "1.0";
    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        api,
        version,
        awesome_detail_payload(&payload.item_id, &payload.city, &location, true),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream_response_with_api(
        api, version, upstream,
    )))
}

pub(super) async fn item_awesome_detail_unit(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<AwesomeDetailPayload>,
) -> ApiResult {
    ensure_market_uid_access(&state, &identity, &payload.uid).await?;
    let location = load_location_or_default(&state, &payload.uid).await;
    let api = crate::sealed_protocol::sealed_str!("mtop.taobao.idle.awesome.detail.unit");
    let version = "1.0";
    let upstream = successful_upstream_h5_request(
        &state,
        &payload.uid,
        api,
        version,
        awesome_detail_payload(&payload.item_id, &payload.city, &location, false),
        Vec::new(),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream_response_with_api(
        api, version, upstream,
    )))
}
