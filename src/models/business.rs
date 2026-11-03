use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn default_true() -> bool {
    true
}

fn default_text_kind() -> String {
    "text".to_string()
}

fn default_reply_count() -> i64 {
    1
}

fn default_card_pool_scope() -> String {
    "account".to_string()
}

pub fn normalize_item_reply_count(value: i64) -> i64 {
    value.clamp(1, 999)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ReplyMode {
    #[default]
    Off,
    Default,
    Keyword,
    Ai,
    FollowGlobal,
    ItemReply,
}

impl ReplyMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Default => "default",
            Self::Keyword => "keyword",
            Self::Ai => "ai",
            Self::FollowGlobal => "follow_global",
            Self::ItemReply => "item_reply",
        }
    }

    pub fn normalize_account(self) -> Self {
        match self {
            Self::Default | Self::Keyword | Self::Ai => self,
            _ => Self::Off,
        }
    }

    pub fn normalize_item(self) -> Self {
        match self {
            Self::Default | Self::Keyword | Self::Ai | Self::FollowGlobal | Self::ItemReply => self,
            _ => Self::Off,
        }
    }

    pub fn from_account_raw(raw: &str) -> Self {
        match raw.trim() {
            "default" => Self::Default,
            "keyword" => Self::Keyword,
            "ai" => Self::Ai,
            _ => Self::Off,
        }
    }

    pub fn from_item_raw(raw: &str) -> Self {
        match raw.trim() {
            "default" => Self::Default,
            "keyword" => Self::Keyword,
            "ai" => Self::Ai,
            "follow_global" => Self::FollowGlobal,
            "item_reply" => Self::ItemReply,
            _ => Self::Off,
        }
    }

    pub fn auto_reply_enabled(self) -> bool {
        !matches!(self.normalize_account(), Self::Off)
    }

    pub fn ai_enabled(self) -> bool {
        matches!(self.normalize_account(), Self::Ai)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Keyword {
    pub id: i64,
    pub cookie_id: String,
    pub keyword: String,
    pub reply: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub item_id: String,
    pub enabled: bool,
    #[serde(default)]
    pub task_group_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KeywordCreatePayload {
    pub cookie_id: Option<String>,
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub reply: String,
    #[serde(default = "default_text_kind", rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub task_group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KeywordUpdatePayload {
    pub cookie_id: Option<String>,
    pub keyword: Option<String>,
    pub reply: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub item_id: Option<String>,
    pub enabled: Option<bool>,
    pub task_group_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemReply {
    pub id: i64,
    pub cookie_id: String,
    pub item_id: String,
    pub reply: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    #[serde(default)]
    pub auto_recall: bool,
    #[serde(default)]
    pub task_group_id: String,
    #[serde(default = "default_reply_count")]
    pub reply_count: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemReplyCreatePayload {
    pub cookie_id: Option<String>,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub reply: String,
    #[serde(default = "default_text_kind", rename = "type")]
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_recall: bool,
    #[serde(default)]
    pub task_group_id: String,
    #[serde(default = "default_reply_count")]
    pub reply_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemReplyUpdatePayload {
    pub cookie_id: Option<String>,
    pub item_id: Option<String>,
    pub reply: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub enabled: Option<bool>,
    pub auto_recall: Option<bool>,
    pub task_group_id: Option<String>,
    pub reply_count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemReplyMode {
    pub cookie_id: String,
    pub item_id: String,
    pub reply_mode: ReplyMode,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemReplyModeUpsertPayload {
    pub cookie_id: Option<String>,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub reply_mode: ReplyMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemReplyModeDeletePayload {
    #[serde(default)]
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OfflineReplyQuery {
    #[serde(default)]
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OfflineReplySyncPayload {
    #[serde(default)]
    pub item_id: String,
    pub enable_offline_reply: Option<bool>,
    pub offline_reply_text: Option<String>,
}

impl ItemReply {
    pub fn normalized_reply_count(&self) -> i64 {
        normalize_item_reply_count(self.reply_count)
    }

    pub fn task_key(&self) -> String {
        let task_group_id = self.task_group_id.trim();
        if !task_group_id.is_empty() {
            return format!("task:{task_group_id}");
        }

        format!(
            "legacy:{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.item_id.trim(),
            self.kind.trim(),
            self.reply.trim(),
            if self.auto_recall { "1" } else { "0" }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: i64,
    pub cookie_id: String,
    pub card_key: String,
    pub card_content: String,
    pub card_pool_tag: String,
    pub reusable: bool,
    pub used: bool,
    #[serde(default)]
    pub quarantined: bool,
    pub used_at: i64,
    pub used_order_id: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CardCreateItem {
    #[serde(default)]
    pub card_key: String,
    #[serde(default)]
    pub card_content: String,
    #[serde(default)]
    pub card_pool_tag: String,
    #[serde(default)]
    pub reusable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CardsBulkCreatePayload {
    #[serde(default)]
    pub cookie_id: String,
    #[serde(default)]
    pub cards: Vec<CardCreateItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CardUpdatePayload {
    #[serde(default)]
    pub card_content: String,
    #[serde(default)]
    pub card_pool_tag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRule {
    pub id: i64,
    pub cookie_id: String,
    pub keyword: String,
    pub item_id: String,
    pub card_pool_tag: String,
    #[serde(default = "default_card_pool_scope")]
    pub card_pool_scope: String,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRuleCreatePayload {
    pub cookie_id: Option<String>,
    #[serde(default)]
    pub keyword: String,
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub card_pool_tag: String,
    #[serde(default = "default_card_pool_scope")]
    pub card_pool_scope: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRuleUpdatePayload {
    pub cookie_id: Option<String>,
    pub keyword: Option<String>,
    pub item_id: Option<String>,
    pub card_pool_tag: Option<String>,
    pub card_pool_scope: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub cookie_id: String,
    pub reply_mode: ReplyMode,
    pub ai_enabled: bool,
    pub auto_reply_enabled: bool,
    pub auto_ship_enabled: bool,
    pub api_type: String,
    pub api_key: String,
    pub base_url: String,
    pub model_name: String,
    pub max_bargain_rounds: i64,
    pub max_discount_percent: f64,
    pub max_discount_amount: f64,
    pub custom_prompts: Map<String, Value>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsUpdatePayload {
    pub reply_mode: Option<ReplyMode>,
    pub ai_enabled: Option<bool>,
    pub auto_reply_enabled: Option<bool>,
    pub auto_ship_enabled: Option<bool>,
    pub api_type: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model_name: Option<String>,
    pub max_bargain_rounds: Option<i64>,
    pub max_discount_percent: Option<f64>,
    pub max_discount_amount: Option<f64>,
    pub custom_prompts: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiModelsDiscoverPayload {
    pub api_type: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiModelsDiscoverResult {
    pub models: Vec<String>,
    pub endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiConnectionTestPayload {
    pub api_type: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiConnectionTestResult {
    pub ok: bool,
    pub message: String,
    pub endpoint: String,
    pub model_name: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BusinessOrder {
    pub order_id: String,
    pub cookie_id: String,
    pub item_id: String,
    pub buyer_id: String,
    pub amount: String,
    pub status: String,
    pub last_shipped_at: i64,
    pub last_msg_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiConversation {
    pub id: i64,
    pub cookie_id: String,
    pub chat_id: String,
    pub item_id: String,
    pub peer_uid: String,
    pub role: String,
    pub intent: String,
    pub content: String,
    pub created_at: i64,
}
