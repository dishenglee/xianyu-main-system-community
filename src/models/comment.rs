use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentTarget {
    pub owner_username: String,
    pub item_id: String,
    pub title: String,
    pub price: String,
    pub pic_url: String,
    pub item_url: String,
    pub seller_id: String,
    pub seller_nick: String,
    pub source: String,
    pub status: String,
    pub selected_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentTargetBatchAddPayload {
    pub items: Vec<CommentTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentCandidate {
    pub id: i64,
    pub uid: String,
    pub item_id: String,
    pub source_api: String,
    pub source_type: String,
    pub comment_id: String,
    pub author_id: String,
    pub author_nick: String,
    pub content: String,
    pub content_hash: String,
    pub published_at: String,
    pub category: String,
    pub score: f64,
    pub quality: String,
    pub score_breakdown: Map<String, Value>,
    pub snapshot_time: f64,
    pub accepted_for_txt: bool,
    pub txt_exported_at: f64,
    pub txt_export_path: String,
    pub txt_imported_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentCandidateSummary {
    pub quality: Map<String, Value>,
    pub category: Map<String, Value>,
    pub accepted_for_txt: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentCandidatesList {
    pub items: Vec<CommentCandidate>,
    pub pagination: Map<String, Value>,
    pub summary: CommentCandidateSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentCandidateExportPayload {
    pub uid: String,
    pub quality: String,
    pub category: String,
    pub accepted_for_txt: Option<bool>,
    pub directory: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentCandidateImportPayload {
    pub uid: String,
    pub file_path: String,
    pub category: String,
    pub source_api: String,
    pub source_type: String,
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentLibraryItem {
    pub id: i64,
    pub owner_username: String,
    pub content: String,
    pub content_hash: String,
    pub source: String,
    pub source_type: String,
    pub source_label: String,
    pub source_ref: String,
    pub source_item_id: String,
    pub enabled: bool,
    pub usage_count: i64,
    pub success_count: i64,
    pub last_used_at: f64,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentLibraryListSummary {
    pub total: i64,
    pub enabled: i64,
    pub disabled: i64,
    pub usage_count: i64,
    pub success_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentLibraryList {
    pub items: Vec<CommentLibraryItem>,
    pub pagination: Map<String, Value>,
    pub summary: CommentLibraryListSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentLibraryManualCreatePayload {
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentLibraryBatchCreatePayload {
    pub contents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentLibraryTxtImportPayload {
    pub file_path: String,
    pub source_label: String,
    pub source_ref: String,
    pub source_item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentLibraryCandidateImportPayload {
    pub uid: String,
    pub quality: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CommentLibrarySetEnabledPayload {
    pub content_hash: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentImportSummary {
    pub imported: i64,
    pub duplicates: i64,
    pub skipped: i64,
    pub file_path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommentExportSummary {
    pub exported: i64,
    pub duplicates: i64,
    pub skipped: i64,
    pub file_path: String,
    pub error: String,
}
