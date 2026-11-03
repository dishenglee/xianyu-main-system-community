use axum::{
    extract::{Extension, Json, Query, State},
    routing::{get, post},
    Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    models::auth::RequestIdentity,
    models::comment::{
        CommentCandidateExportPayload, CommentCandidateImportPayload,
        CommentLibraryBatchCreatePayload, CommentLibraryCandidateImportPayload,
        CommentLibraryManualCreatePayload, CommentLibrarySetEnabledPayload,
        CommentLibraryTxtImportPayload, CommentTargetBatchAddPayload,
    },
    routes::error::{ApiResult, ResponseError},
    routes::tenant::{ensure_account_uid_accessible, owner_username_filter},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/comment-targets/batch-add"),
            post(batch_add_comment_targets),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-targets"),
            get(list_comment_targets),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-candidates"),
            get(list_comment_candidates),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-candidates/export"),
            post(export_comment_candidates),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-candidates/import"),
            post(import_comment_candidates),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-library"),
            get(list_comment_library).post(create_comment_library_entry),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-library/batch"),
            post(batch_create_comment_library),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-library/import/txt"),
            post(import_comment_library_from_txt),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-library/import/candidates"),
            post(import_comment_library_from_candidates),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/comment-library/enabled"),
            post(set_comment_library_enabled),
        )
}

#[derive(Debug, Deserialize)]
struct CommentTargetsQuery {
    #[serde(default)]
    status: String,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_targets_page_size")]
    page_size: i64,
}

#[derive(Debug, Deserialize)]
struct CommentCandidatesQuery {
    uid: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    accepted_for_txt: Option<bool>,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_candidates_page_size")]
    page_size: i64,
}

#[derive(Debug, Deserialize)]
struct CommentLibraryQuery {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default = "default_page")]
    page: i64,
    #[serde(default = "default_library_page_size")]
    page_size: i64,
}

async fn batch_add_comment_targets(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentTargetBatchAddPayload>,
) -> ApiResult {
    let owner = comment_owner_scope(&state, &identity);
    let (requested, added, deduplicated, total) = state
        .storage()
        .upsert_comment_targets(&owner, &payload.items)
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "status": "ok",
        "requested": requested,
        "added": added,
        "deduplicated": deduplicated,
        "total": total,
    })))
}

async fn list_comment_targets(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CommentTargetsQuery>,
) -> ApiResult {
    let owner = comment_owner_scope(&state, &identity);
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let items = state
        .storage()
        .list_comment_targets(&owner, &query.status, page, page_size)
        .await
        .map_err(ResponseError::internal)?;
    let total = state
        .storage()
        .count_comment_targets(&owner, &query.status)
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "items": items,
        "pagination": {
            "page": page,
            "page_size": page_size,
            "total": total,
        }
    })))
}

async fn list_comment_candidates(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CommentCandidatesQuery>,
) -> ApiResult {
    validate_uid(&query.uid)?;
    ensure_comment_uid_access(&state, &identity, &query.uid).await?;
    validate_candidate_quality(&query.quality)?;
    validate_candidate_category(&query.category)?;

    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let (items, total, summary) = state
        .storage()
        .list_comment_candidates(
            &query.uid,
            &query.quality,
            &query.category,
            query.accepted_for_txt,
            page,
            page_size,
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "items": items,
        "pagination": {
            "page": page,
            "page_size": page_size,
            "total": total,
        },
        "summary": summary,
    })))
}

async fn export_comment_candidates(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentCandidateExportPayload>,
) -> ApiResult {
    validate_uid(&payload.uid)?;
    ensure_comment_uid_access(&state, &identity, &payload.uid).await?;
    validate_candidate_quality(&payload.quality)?;
    validate_candidate_category(&payload.category)?;

    let summary = state
        .storage()
        .export_comment_candidates(
            &payload.uid,
            &payload.quality,
            &payload.category,
            payload.accepted_for_txt,
            &payload.directory,
            &payload.file_name,
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!(summary)))
}

async fn import_comment_candidates(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentCandidateImportPayload>,
) -> ApiResult {
    validate_uid(&payload.uid)?;
    ensure_comment_uid_access(&state, &identity, &payload.uid).await?;
    validate_candidate_category(&payload.category)?;
    ensure_non_empty(&payload.file_path, "file_path is required")?;

    let summary = state
        .storage()
        .import_comment_candidates(
            &payload.uid,
            &payload.file_path,
            &payload.category,
            &payload.source_api,
            &payload.source_type,
            &payload.item_id,
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!(summary)))
}

async fn list_comment_library(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<CommentLibraryQuery>,
) -> ApiResult {
    let owner = comment_owner_scope(&state, &identity);
    let page = query.page.max(1);
    let page_size = query.page_size.clamp(1, 200);
    let items = state
        .storage()
        .list_comment_library(&owner, query.enabled)
        .await
        .map_err(ResponseError::internal)?;
    let total = items.len() as i64;
    let start = ((page - 1) * page_size) as usize;
    let end = (start + page_size as usize).min(items.len());
    let paged_items = if start >= items.len() {
        Vec::new()
    } else {
        items[start..end].to_vec()
    };
    let enabled_total = state
        .storage()
        .count_comment_library(&owner, Some(true))
        .await
        .map_err(ResponseError::internal)?;
    let disabled_total = state
        .storage()
        .count_comment_library(&owner, Some(false))
        .await
        .map_err(ResponseError::internal)?;
    let usage_count = items.iter().map(|item| item.usage_count).sum::<i64>();
    let success_count = items.iter().map(|item| item.success_count).sum::<i64>();

    Ok(ResponseJson(json!({
        "items": paged_items,
        "pagination": {
            "page": page,
            "page_size": page_size,
            "total": total,
        },
        "summary": {
            "total": total,
            "enabled": enabled_total,
            "disabled": disabled_total,
            "usage_count": usage_count,
            "success_count": success_count,
        }
    })))
}

async fn create_comment_library_entry(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentLibraryManualCreatePayload>,
) -> ApiResult {
    ensure_non_empty(&payload.content, "content cannot be empty")?;
    let owner = comment_owner_scope(&state, &identity);

    let (item, created) = state
        .storage()
        .create_comment_library(&owner, &payload.content, "manual", "manual", "", "", "")
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "item": item,
        "imported": if created { 1 } else { 0 },
        "duplicates": if created { 0 } else { 1 },
        "skipped": 0,
        "error": "",
    })))
}

async fn batch_create_comment_library(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentLibraryBatchCreatePayload>,
) -> ApiResult {
    if payload.contents.is_empty() {
        return Err(ResponseError::bad_request("contents cannot be empty"));
    }

    let owner = comment_owner_scope(&state, &identity);
    let (imported, duplicates, skipped) = state
        .storage()
        .batch_create_comment_library(&owner, &payload.contents)
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "imported": imported,
        "duplicates": duplicates,
        "skipped": skipped,
    })))
}

async fn import_comment_library_from_txt(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentLibraryTxtImportPayload>,
) -> ApiResult {
    ensure_non_empty(&payload.file_path, "file_path is required")?;
    let owner = comment_owner_scope(&state, &identity);

    let summary = state
        .storage()
        .import_comment_library_from_txt(
            &owner,
            &payload.file_path,
            &payload.source_label,
            &payload.source_ref,
            &payload.source_item_id,
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!(summary)))
}

async fn import_comment_library_from_candidates(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentLibraryCandidateImportPayload>,
) -> ApiResult {
    validate_uid(&payload.uid)?;
    ensure_comment_uid_access(&state, &identity, &payload.uid).await?;
    validate_candidate_quality(&payload.quality)?;
    validate_candidate_category(&payload.category)?;

    let owner = comment_owner_scope(&state, &identity);
    let (imported, duplicates, skipped) = state
        .storage()
        .import_comment_library_from_candidates(
            &owner,
            &payload.uid,
            &payload.quality,
            &payload.category,
        )
        .await
        .map_err(ResponseError::internal)?;

    Ok(ResponseJson(json!({
        "imported": imported,
        "duplicates": duplicates,
        "skipped": skipped,
        "error": "",
    })))
}

async fn set_comment_library_enabled(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CommentLibrarySetEnabledPayload>,
) -> ApiResult {
    ensure_non_empty(&payload.content_hash, "content_hash is required")?;
    let owner = comment_owner_scope(&state, &identity);

    let item = state
        .storage()
        .set_comment_library_enabled(&owner, &payload.content_hash, payload.enabled)
        .await
        .map_err(ResponseError::internal)?
        .ok_or_else(|| ResponseError::not_found("comment library row not found"))?;

    Ok(ResponseJson(json!({ "item": item })))
}

fn validate_uid(uid: &str) -> Result<(), ResponseError> {
    ensure_non_empty(uid, "uid is required")
}

fn comment_owner_scope(state: &AppState, identity: &RequestIdentity) -> String {
    owner_username_filter(state, identity).unwrap_or_default()
}

async fn ensure_comment_uid_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid).await
}

fn ensure_non_empty(value: &str, detail: &str) -> Result<(), ResponseError> {
    if value.trim().is_empty() {
        Err(ResponseError::bad_request(detail))
    } else {
        Ok(())
    }
}

fn validate_candidate_quality(raw: &str) -> Result<(), ResponseError> {
    let normalized = raw.trim().to_lowercase();
    if normalized.is_empty() || matches!(normalized.as_str(), "good" | "review" | "bad") {
        Ok(())
    } else {
        Err(ResponseError::bad_request("unsupported quality filter"))
    }
}

fn validate_candidate_category(raw: &str) -> Result<(), ResponseError> {
    if raw.trim().is_empty() || normalize_candidate_category(raw).is_some() {
        Ok(())
    } else {
        Err(ResponseError::bad_request("unsupported category filter"))
    }
}

fn normalize_candidate_category(raw: &str) -> Option<&'static str> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return None;
    }

    const RULES: [(&str, &[&str]); 5] = [
        (
            "物流",
            &["物流", "快递", "发货", "到货", "配送", "送达", "签收"],
        ),
        ("包装", &["包装", "包裹", "盒子", "外观", "封装", "保护"]),
        (
            "商品",
            &[
                "宝贝", "商品", "质量", "成色", "描述", "做工", "材质", "功能", "使用",
            ],
        ),
        ("服务", &["回复", "服务", "态度", "客服", "沟通", "售后"]),
        ("价格", &["价格", "便宜", "划算", "性价比", "值", "优惠"]),
    ];

    for (category, keywords) in RULES {
        if normalized == category || keywords.iter().any(|keyword| normalized.contains(keyword)) {
            return Some(category);
        }
    }
    if normalized == "其他" {
        Some("其他")
    } else {
        None
    }
}

fn default_page() -> i64 {
    1
}

fn default_targets_page_size() -> i64 {
    20
}

fn default_candidates_page_size() -> i64 {
    50
}

fn default_library_page_size() -> i64 {
    50
}
