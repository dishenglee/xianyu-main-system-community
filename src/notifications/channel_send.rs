use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hmac::{Hmac, Mac};
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::time::Duration;

use crate::{
    models::notifications::NotificationChannel,
    notifications::{
        driver::{config_string, openclaw_context_token_for_send, send_channel_text},
        openclaw_weixin::IlinkBotClient,
        template::RenderedNotificationContent,
    },
};

pub(crate) async fn send_text_to_channel(
    channel: &NotificationChannel,
    text: &str,
) -> Result<(), String> {
    let _ = send_channel_text(channel, text).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RenderedSendRequest<'a> {
    pub(crate) channel_id: i64,
    pub(crate) config: &'a Map<String, Value>,
    pub(crate) text: &'a str,
    pub(crate) rendered: Option<&'a RenderedNotificationContent>,
}

pub(crate) async fn send_openclaw_text(request: RenderedSendRequest<'_>) -> Result<(), String> {
    let base_url = config_string(request.config, "base_url");
    let bot_token = config_string(request.config, "bot_token");
    let ilink_user_id = config_string(request.config, "ilink_user_id");
    let explicit_to = config_string(request.config, "to_user_id");
    let to_user_id = if explicit_to.is_empty() {
        ilink_user_id
    } else {
        explicit_to
    };
    let bot_agent = {
        let value = config_string(request.config, "bot_agent");
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    };

    if base_url.is_empty() || bot_token.is_empty() || to_user_id.is_empty() {
        return Err("missing base_url / bot_token / to_user_id".to_string());
    }

    let context_token =
        openclaw_context_token_for_send(request.channel_id, request.config, &to_user_id);
    if context_token.trim().is_empty() {
        return Err("missing context_token".to_string());
    }

    let client = IlinkBotClient::new(base_url, bot_token, bot_agent);
    client
        .send_text(
            &to_user_id,
            &context_token,
            select_body(request.rendered, request.text),
        )
        .await
        .map_err(|err| format!("send_text failed: {err}"))
}

pub(crate) async fn send_dingtalk_robot_text(
    request: RenderedSendRequest<'_>,
) -> Result<(), String> {
    let robot_url = config_string(request.config, "robot_url");
    if robot_url.is_empty() {
        return Err("missing robot_url".to_string());
    }

    let secret = config_string(request.config, "secret");
    let target_url = if secret.is_empty() {
        robot_url
    } else {
        append_dingtalk_signature(&robot_url, &secret)?
    };

    post_text_robot(
        "dingtalk",
        &target_url,
        select_body(request.rendered, request.text),
    )
    .await
}

pub(crate) async fn send_enterprise_wechat_robot_text(
    request: RenderedSendRequest<'_>,
) -> Result<(), String> {
    let robot_url = config_string(request.config, "robot_url");
    if robot_url.is_empty() {
        return Err("missing robot_url".to_string());
    }

    post_text_robot(
        "wechat",
        &robot_url,
        select_body(request.rendered, request.text),
    )
    .await
}

async fn post_text_robot(driver: &str, robot_url: &str, text: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|err| format!("{driver} robot client init failed: {err}"))?;
    let response = client
        .post(robot_url)
        .json(&json!({
            "msgtype": "text",
            "text": {
                "content": text
            }
        }))
        .send()
        .await
        .map_err(|err| format!("{driver} robot request failed: {err}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|err| format!("{driver} robot read response failed: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "{driver} robot returned HTTP {}: {}",
            status.as_u16(),
            truncate_response_body(&body)
        ));
    }

    if let Ok(payload) = serde_json::from_str::<Value>(&body) {
        let errcode = payload.get("errcode").and_then(Value::as_i64).unwrap_or(0);
        if errcode != 0 {
            let errmsg = payload
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            return Err(format!(
                "{driver} robot returned errcode={errcode}: {errmsg}"
            ));
        }
    }

    Ok(())
}

fn append_dingtalk_signature(robot_url: &str, secret: &str) -> Result<String, String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| format!("system clock before unix epoch: {err}"))?
        .as_millis()
        .to_string();
    let sign = dingtalk_signature(&timestamp, secret)?;
    let mut url =
        reqwest::Url::parse(robot_url).map_err(|err| format!("invalid robot_url: {err}"))?;
    url.query_pairs_mut()
        .append_pair("timestamp", &timestamp)
        .append_pair("sign", &sign);
    Ok(url.to_string())
}

fn dingtalk_signature(timestamp: &str, secret: &str) -> Result<String, String> {
    type HmacSha256 = Hmac<Sha256>;
    let string_to_sign = format!("{timestamp}\n{secret}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|err| format!("dingtalk sign init failed: {err}"))?;
    mac.update(string_to_sign.as_bytes());
    Ok(BASE64_STANDARD.encode(mac.finalize().into_bytes()))
}

fn truncate_response_body(body: &str) -> String {
    const MAX_LEN: usize = 500;
    let trimmed = body.trim();
    if trimmed.chars().count() <= MAX_LEN {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(MAX_LEN).collect::<String>())
    }
}

fn select_body<'a>(
    rendered: Option<&'a RenderedNotificationContent>,
    fallback_text: &'a str,
) -> &'a str {
    rendered
        .map(|value| value.body.as_str())
        .unwrap_or(fallback_text)
}
