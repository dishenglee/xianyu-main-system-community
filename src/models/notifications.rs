use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const ALLOWED_NOTIFICATION_TYPES: &[&str] =
    &["wechat", "dingtalk", "feishu", "lark", "openclaw_weixin"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannel {
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    #[serde(default)]
    pub owner_username: String,
    pub config: Map<String, Value>,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
    pub runtime: NotificationRuntime,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotificationRuntime {
    pub channel_id: i64,
    pub status: String,
    pub last_error: String,
    pub last_attempt_at: f64,
    pub last_success_at: f64,
    pub runtime_json: Map<String, Value>,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationBinding {
    pub id: i64,
    pub uid: String,
    pub channel_id: i64,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
    pub channel_name: String,
    pub channel_type: String,
    pub channel_config: Map<String, Value>,
    pub channel_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuUserBinding {
    pub id: i64,
    pub channel_id: i64,
    pub owner_username: String,
    pub sender_id: String,
    pub sender_name: String,
    pub chat_id: String,
    pub chat_type: String,
    pub source: String,
    pub role: String,
    pub joined_via: String,
    pub invited_by_sender_id: String,
    pub invited_by_sender_name: String,
    pub last_message_id: String,
    pub last_event_id: String,
    pub last_seen_at: f64,
    pub enabled: bool,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuCollaboratorInvite {
    pub id: i64,
    pub channel_id: i64,
    pub owner_username: String,
    pub code: String,
    pub created_by_sender_id: String,
    pub created_by_sender_name: String,
    pub status: String,
    pub expires_at: f64,
    pub used_by_sender_id: String,
    pub used_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuImReplyRoute {
    pub id: i64,
    pub channel_id: i64,
    pub owner_username: String,
    pub feishu_message_id: String,
    pub chat_id: String,
    pub uid: String,
    pub session_id: String,
    pub peer_uid: String,
    pub customer_name: String,
    pub item_id: String,
    pub item_title: String,
    pub xianyu_message_id: String,
    pub xianyu_upstream_message_id: String,
    pub created_at: f64,
    pub expires_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuSessionGroupChat {
    pub id: i64,
    pub channel_id: i64,
    pub owner_username: String,
    pub uid: String,
    pub session_id: String,
    pub peer_uid: String,
    pub customer_name: String,
    pub item_id: String,
    pub item_title: String,
    pub item_pic: String,
    pub item_price: String,
    pub feishu_chat_id: String,
    pub feishu_chat_name: String,
    pub feishu_operator_sender_id: String,
    pub status: String,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_message_ts: f64,
    pub closed_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuOperatorActivity {
    pub id: i64,
    pub channel_id: i64,
    pub owner_username: String,
    pub sender_id: String,
    pub sender_name: String,
    pub last_chat_id: String,
    pub last_event_id: String,
    pub last_active_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuPendingAction {
    pub id: i64,
    pub channel_id: i64,
    pub owner_username: String,
    pub sender_id: String,
    pub chat_id: String,
    pub nonce: String,
    pub action_type: String,
    pub uid: String,
    pub target_id: String,
    pub args: Map<String, Value>,
    pub status: String,
    pub source_message_id: String,
    pub created_at: f64,
    pub expires_at: f64,
    pub confirmed_at: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationChannelPayload {
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    pub config: Option<Value>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NotificationChannelUpdatePayload {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub channel_type: Option<String>,
    pub config: Option<Value>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NotificationRuntimePatchPayload {
    pub runtime: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationBindingPayload {
    pub channel_id: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNotificationSettings {
    pub platform_messages_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageNotificationSettingsUpdatePayload {
    pub platform_messages_enabled: bool,
}

pub fn normalize_notification_type(raw: &str) -> Option<String> {
    let mut normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized == "ding_talk" {
        normalized = "dingtalk".to_string();
    }
    // OpenClaw WeChat gateway — accept a few common alias spellings.
    if normalized == "openclaw"
        || normalized == "openclaw-weixin"
        || normalized == "openclaw_wx"
        || normalized == "openclaw-wx"
    {
        normalized = "openclaw_weixin".to_string();
    }
    ALLOWED_NOTIFICATION_TYPES
        .contains(&normalized.as_str())
        .then_some(normalized)
}

pub fn parse_object_value(raw: Option<Value>) -> Result<Map<String, Value>, String> {
    match raw {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(map)) => Ok(map),
        Some(Value::String(text)) => {
            let parsed: Value =
                serde_json::from_str(&text).map_err(|error| format!("配置 JSON 非法: {error}"))?;
            match parsed {
                Value::Object(map) => Ok(map),
                _ => Err("config 必须是 JSON 对象或 JSON 字符串".to_string()),
            }
        }
        _ => Err("config 必须是 JSON 对象或 JSON 字符串".to_string()),
    }
}

pub fn default_enabled() -> bool {
    true
}
