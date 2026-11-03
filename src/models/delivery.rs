use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryRunStatus {
    #[default]
    Pending,
    WaitingStock,
    Reserved,
    Consigning,
    Sending,
    Completed,
    Retrying,
    ManualRequired,
    Cancelled,
}

impl DeliveryRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::WaitingStock => "waiting_stock",
            Self::Reserved => "reserved",
            Self::Consigning => "consigning",
            Self::Sending => "sending",
            Self::Completed => "completed",
            Self::Retrying => "retrying",
            Self::ManualRequired => "manual_required",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for DeliveryRunStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DeliveryRunStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "waiting_stock" => Ok(Self::WaitingStock),
            "reserved" => Ok(Self::Reserved),
            "consigning" => Ok(Self::Consigning),
            "sending" => Ok(Self::Sending),
            "completed" => Ok(Self::Completed),
            "retrying" => Ok(Self::Retrying),
            "manual_required" => Ok(Self::ManualRequired),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("unknown delivery run status: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRun {
    pub id: i64,
    pub uid: String,
    pub rule_id: i64,
    pub order_id: String,
    pub buyer_id: String,
    pub item_id: String,
    pub pool_owner_key: String,
    pub pool_scope_snapshot: String,
    pub one_time_pool_tags: Vec<String>,
    pub fixed_pool_tags: Vec<String>,
    pub fixed_message_snapshot: String,
    pub fixed_message_snapshot_captured: bool,
    pub purchase_quantity: Option<i64>,
    pub purchase_quantity_source: String,
    pub required_card_count: i64,
    pub quantity_confirmed_at: i64,
    pub status: DeliveryRunStatus,
    pub attempt_count: i64,
    pub waiting_notice_sent_at: i64,
    pub waiting_notice_claimed_at: i64,
    pub waiting_notice_claim_owner: String,
    pub waiting_notice_claim_expires_at: i64,
    pub admin_notified_at: i64,
    pub consigned_at: i64,
    pub message_sent_at: i64,
    pub delivery_send_claimed_at: i64,
    pub delivery_send_claim_owner: String,
    pub delivery_send_claim_expires_at: i64,
    pub delivery_send_started_at: i64,
    pub lease_owner: String,
    pub lease_expires_at: i64,
    pub message_acknowledged_at: i64,
    pub message_ack_id: String,
    pub message_ack_uuid: String,
    pub completed_at: i64,
    pub cancel_observed_at: i64,
    pub next_retry_at: i64,
    pub last_error_code: String,
    pub last_error_message: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideEffectClaim {
    Acquired,
    ActiveElsewhere,
    ExpiredUncertain,
    AlreadyAcknowledged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelDeliveryRunOutcome {
    CancelledAndReleased,
    ObservedInFlight,
    AlreadyTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryCompletionOutcome {
    Completed,
    AlreadyCompleted,
    AckPersistedButCompletionConflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDeliveryRun {
    pub uid: String,
    pub rule_id: i64,
    pub order_id: String,
    pub buyer_id: String,
    pub item_id: String,
    pub purchase_quantity: Option<i64>,
    pub purchase_quantity_source: String,
    pub required_card_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRunCard {
    pub run_id: i64,
    pub card_id: i64,
    pub position: i64,
    pub content_snapshot: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryAlert {
    pub id: i64,
    pub run_id: i64,
    pub uid: String,
    pub status: String,
    pub title: String,
    pub body: String,
    pub system_notified_at: i64,
    pub external_notified_at: i64,
    pub resolved_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryDiscoveryAlert {
    pub id: i64,
    pub alert_key: String,
    pub alert_type: String,
    pub uid: String,
    pub rule_id: Option<i64>,
    pub order_id: String,
    pub item_id: String,
    pub order_status: String,
    pub order_status_text: String,
    pub status: String,
    pub title: String,
    pub body: String,
    pub occurrence_count: i64,
    pub first_seen_at: i64,
    pub last_seen_at: i64,
    pub resolved_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CardReservation {
    Reserved { cards: Vec<DeliveryRunCard> },
    Insufficient { available: i64, required: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDeliveryConfigurationRequest {
    pub uid: String,
    pub item_id: String,
    #[serde(default = "default_pool_scope")]
    pub pool_scope: String,
    #[serde(default)]
    pub pool_tag: String,
    #[serde(default)]
    pub pool_tags: Vec<String>,
    #[serde(default)]
    pub fixed_pool_tags: Vec<String>,
    #[serde(default)]
    pub fixed_message: String,
    #[serde(default)]
    pub one_time_enabled: bool,
    #[serde(default)]
    pub cards: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeliveryConfigurationRequest {
    pub uid: Option<String>,
    pub item_id: Option<String>,
    pub pool_scope: Option<String>,
    pub pool_tag: Option<String>,
    pub pool_tags: Option<Vec<String>>,
    pub fixed_pool_tags: Option<Vec<String>>,
    pub fixed_message: Option<String>,
    pub one_time_enabled: Option<bool>,
    #[serde(default)]
    pub cards: Vec<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddDeliveryCardsRequest {
    #[serde(default)]
    pub cards: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryConfigurationRecord {
    pub id: i64,
    pub uid: String,
    pub item_id: String,
    pub fixed_message: String,
    pub one_time_enabled: bool,
    pub enabled: bool,
    pub needs_repair: bool,
    pub pool_scope: String,
    #[serde(skip_serializing)]
    pub pool_tag: String,
    pub pool_tags: Vec<String>,
    pub fixed_pool_tags: Vec<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryProduct {
    pub item_id: String,
    pub image: String,
    pub title: String,
    pub price: String,
    pub quantity: Option<i64>,
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryConfiguration {
    pub id: i64,
    pub uid: String,
    pub item_id: String,
    pub fixed_message: String,
    pub one_time_enabled: bool,
    pub enabled: bool,
    pub needs_repair: bool,
    pub pool_scope: String,
    pub pool_tag: String,
    pub pool_tags: Vec<String>,
    pub fixed_pool_tags: Vec<String>,
    pub available_card_count: i64,
    pub product: DeliveryProduct,
    /// 待发货订单的商品没有任何启用自动发货配置时，会附在该账号的
    /// 配置上，供配置页置顶提示并引导人工修复。
    #[serde(default)]
    pub pending_order_mismatches: Vec<PendingDeliveryOrderMismatch>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingDeliveryOrderMismatch {
    pub order_id: String,
    pub item_id: String,
    pub title: String,
    pub status: String,
    pub status_text: String,
}

fn default_pool_scope() -> String {
    "account".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRunSummary {
    pub id: i64,
    pub uid: String,
    pub configuration_id: i64,
    pub order_id: String,
    pub buyer_id: String,
    pub item_id: String,
    pub purchase_quantity: Option<i64>,
    pub required_card_count: i64,
    pub status: String,
    pub attempt_count: i64,
    #[serde(skip_serializing)]
    pub last_error_code: String,
    pub last_error_message: String,
    pub created_at: i64,
    pub updated_at: i64,
}
