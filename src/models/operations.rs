use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    Queued,
    Running,
    Success,
    PartialSuccess,
    Failed,
    Skipped,
    Blocked,
}

impl OperationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Success => "success",
            Self::PartialSuccess => "partial_success",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLogEntry {
    pub id: i64,
    pub job_id: String,
    pub module: String,
    pub action: String,
    pub operator: String,
    pub trigger_source: String,
    pub uid: String,
    pub account_nick: String,
    pub target_type: String,
    pub target_id: String,
    pub target_title: String,
    pub status: String,
    pub error_code: String,
    pub message: String,
    pub params: Value,
    pub before: Value,
    pub after: Value,
    pub extra: Value,
    pub created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationJob {
    pub id: String,
    pub kind: String,
    pub module: String,
    pub action: String,
    pub operator: String,
    pub trigger_source: String,
    pub status: String,
    pub params: Value,
    pub summary: Value,
    pub total_count: i64,
    pub success_count: i64,
    pub fail_count: i64,
    pub skipped_count: i64,
    pub started_at: f64,
    pub finished_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationJobItem {
    pub id: i64,
    pub job_id: String,
    pub seq: i64,
    pub uid: String,
    pub account_nick: String,
    pub target_type: String,
    pub target_id: String,
    pub target_title: String,
    pub status: String,
    pub params: Value,
    pub result: Value,
    pub message: String,
    pub error_code: String,
    pub started_at: f64,
    pub finished_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDailyStat {
    pub stat_date: String,
    pub uid: String,
    pub account_nick: String,
    pub notification_message_count: i64,
    pub interaction_message_count: i64,
    pub received_message_count: i64,
    pub default_reply_count: i64,
    pub auto_reply_count: i64,
    pub ai_reply_count: i64,
    pub manual_reply_count: i64,
    pub image_reply_count: i64,
    pub auto_delivery_count: i64,
    pub reply_failure_count: i64,
    pub recall_count: i64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationLogFilter {
    pub module: String,
    pub action: String,
    pub uid: String,
    pub status: String,
    pub keyword: String,
    pub limit: i64,
    pub since: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageDailyStatFilter {
    pub uid: String,
    pub start_date: String,
    pub end_date: String,
    pub limit: i64,
}
