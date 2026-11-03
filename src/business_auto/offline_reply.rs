use serde_json::{json, Value};

use crate::{
    routes::accounts::shared::{first_ret_string, successful_upstream_app_request},
    state::AppState,
    xianyu_client::LocationContext,
};

const NAME: &str = "不在线回复";
const VERSION: &str = "1.0";
#[allow(non_snake_case)]
fn SMART_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idlemessage.profile.smart.list")
}
#[allow(non_snake_case)]
fn SMART_EDIT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idlemessage.profile.smart.edit")
}
#[allow(non_snake_case)]
fn OFFLINE_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idlemessage.profile.offline.config.list")
}
#[allow(non_snake_case)]
fn OFFLINE_EDIT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idlemessage.profile.item.offline.config.edit")
}

#[derive(Debug, Clone)]
pub struct OfflineReplySyncInput {
    pub item_id: String,
    pub enable_offline_reply: Option<bool>,
    pub offline_reply_text: Option<String>,
}

pub async fn load_offline_reply_config(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Value, String> {
    let location = crate::xianyu_client::load_location(state.storage().accounts(), uid)
        .await
        .unwrap_or_default();
    let mut results = serde_json::Map::new();

    results.insert(
        "smartList".to_string(),
        app_call(state, uid, SMART_LIST_API(), base_payload(&location)).await?,
    );
    results.insert(
        "offlineList".to_string(),
        app_call(
            state,
            uid,
            OFFLINE_LIST_API(),
            offline_list_payload(&location, item_id),
        )
        .await?,
    );
    Ok(json!({
        "status": "ok",
        "name": NAME,
        "transport": "app",
        "results": results,
    }))
}

pub async fn sync_offline_reply_config(
    state: &AppState,
    uid: &str,
    input: OfflineReplySyncInput,
) -> Result<Value, String> {
    let location = crate::xianyu_client::load_location(state.storage().accounts(), uid)
        .await
        .unwrap_or_default();
    let item_id = input.item_id.trim();
    let mut results = serde_json::Map::new();

    let account_enable_offline = if item_id.is_empty() {
        input.enable_offline_reply
    } else {
        None
    };

    if let Some(enabled) = account_enable_offline {
        results.insert(
            "smartOpenEdit".to_string(),
            app_call(
                state,
                uid,
                SMART_EDIT_API(),
                smart_open_edit_payload(&location, enabled),
            )
            .await?,
        );
    }

    if let Some(text) = input.offline_reply_text.as_deref() {
        results.insert(
            "smartTextEdit".to_string(),
            app_call(
                state,
                uid,
                SMART_EDIT_API(),
                smart_text_edit_payload(&location, text),
            )
            .await?,
        );
    }

    if let Some(enabled) = input.enable_offline_reply.filter(|_| !item_id.is_empty()) {
        results.insert(
            "offlineItemEdit".to_string(),
            app_call(
                state,
                uid,
                OFFLINE_EDIT_API(),
                offline_edit_payload(&location, item_id, enabled),
            )
            .await?,
        );
    }

    Ok(json!({
        "status": "ok",
        "name": NAME,
        "transport": "app",
        "results": results,
    }))
}

async fn app_call(state: &AppState, uid: &str, api: &str, data: Value) -> Result<Value, String> {
    let payload = successful_upstream_app_request(state, uid, api, VERSION, data).await?;
    Ok(json!({
        "transport": "app",
        "api": api,
        "version": VERSION,
        "ret": first_ret_string(&payload),
        "payload": payload,
    }))
}

fn base_payload(location: &LocationContext) -> Value {
    json!({
        "fishAdCode": location.fish_ad_code,
        "gps": location.gps,
        "latitude": location.latitude,
        "longitude": location.longitude,
    })
}

fn paged_payload(location: &LocationContext, page_size: i64) -> Value {
    let mut payload = base_payload(location);
    payload["currentPage"] = json!(1);
    payload["pageSize"] = json!(page_size);
    payload
}

fn offline_list_payload(location: &LocationContext, item_id: &str) -> Value {
    let mut payload = paged_payload(location, 10);
    payload["itemId"] = json!(item_id.trim());
    payload
}

fn smart_open_edit_payload(location: &LocationContext, enabled: bool) -> Value {
    let mut payload = base_payload(location);
    payload["req"] = json!(json!({
        "id": "offline",
        "open": enabled,
        "useNewStyle": true,
    })
    .to_string());
    payload
}

fn smart_text_edit_payload(location: &LocationContext, text: &str) -> Value {
    let mut payload = base_payload(location);
    payload["req"] = json!(json!({
        "id": "offline",
        "text": text.trim(),
    })
    .to_string());
    payload
}

fn offline_edit_payload(location: &LocationContext, item_id: &str, enabled: bool) -> Value {
    let mut payload = base_payload(location);
    payload["req"] = json!(json!({
        "itemId": item_id.trim(),
        "enableOfflineReply": enabled,
    })
    .to_string());
    payload
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location() -> LocationContext {
        LocationContext {
            fish_ad_code: "430202".to_string(),
            gps: "27.867305,113.164448".to_string(),
            latitude: "27.867305".to_string(),
            longitude: "113.164448".to_string(),
            ..LocationContext::default()
        }
    }

    #[test]
    fn edit_payloads_match_captured_app_shapes() {
        let location = location();
        let smart_open = smart_open_edit_payload(&location, true);
        let smart_open_req: Value =
            serde_json::from_str(smart_open["req"].as_str().unwrap()).unwrap();
        assert_eq!(smart_open_req["id"], "offline");
        assert_eq!(smart_open_req["open"], true);
        assert_eq!(smart_open_req["useNewStyle"], true);

        let smart_text = smart_text_edit_payload(&location, "你好，请描述你的需求或者直接");
        let smart_text_req: Value =
            serde_json::from_str(smart_text["req"].as_str().unwrap()).unwrap();
        assert_eq!(smart_text_req["id"], "offline");
        assert_eq!(smart_text_req["text"], "你好，请描述你的需求或者直接");

        let offline = offline_edit_payload(&location, "1052294015097", true);
        let offline_req: Value = serde_json::from_str(offline["req"].as_str().unwrap()).unwrap();
        assert_eq!(offline_req["itemId"], "1052294015097");
        assert_eq!(offline_req["enableOfflineReply"], true);
    }
}
