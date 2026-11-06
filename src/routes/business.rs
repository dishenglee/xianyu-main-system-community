use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::StatusCode,
    response::Json as ResponseJson,
    routing::get,
    Router,
};
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashSet;

use crate::{
    models::auth::RequestIdentity,
    models::business::{
        AiConnectionTestPayload, AiConnectionTestResult, AiModelsDiscoverPayload,
        AiModelsDiscoverResult, AiSettingsUpdatePayload, Card, CardUpdatePayload,
        CardsBulkCreatePayload, DeliveryRule, DeliveryRuleCreatePayload, DeliveryRuleUpdatePayload,
        ItemReply, ItemReplyCreatePayload, ItemReplyModeDeletePayload, ItemReplyModeUpsertPayload,
        ItemReplyUpdatePayload, Keyword, KeywordCreatePayload, KeywordUpdatePayload,
        OfflineReplyQuery, OfflineReplySyncPayload, ReplyMode,
    },
    routes::{
        accounts::runtime_access::ensure_account_can_call_upstream,
        accounts::summary::{account_extra_map, persist_account_extra},
        operations::append_operation_log,
        tenant::{account_uid_accessible, owner_username_filter},
    },
    state::AppState,
    storage::{
        is_global_scope_key, public_cookie_id, user_global_scope_key, StorageError, StorageState,
        GLOBAL_AI_SETTINGS_KEY,
    },
    xianyu_client::http::http_client,
};

type ApiResult = Result<ResponseJson<Value>, (StatusCode, ResponseJson<Value>)>;
type ValidationResult = Result<(), (StatusCode, ResponseJson<Value>)>;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/ai-settings/models"),
            axum::routing::post(discover_ai_models),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/ai-settings/test"),
            axum::routing::post(test_ai_settings_connection),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/ai-settings/:cookie_id"),
            get(get_ai_settings).put(update_ai_settings),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/keywords/:keyword_or_cookie_id"),
            get(list_keywords)
                .post(create_keyword)
                .put(update_keyword)
                .delete(delete_keyword),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item-replies/:item_reply_or_cookie_id"),
            get(list_item_replies)
                .post(create_item_reply)
                .put(update_item_reply)
                .delete(delete_item_reply),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/item-reply-modes/:cookie_id"),
            get(list_item_reply_modes)
                .put(upsert_item_reply_mode)
                .delete(delete_item_reply_mode),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/offline-reply/:cookie_id"),
            get(get_offline_reply).put(sync_offline_reply),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/cards/:card_or_cookie_id"),
            get(list_cards).put(update_card).delete(delete_card),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/cards/bulk"),
            axum::routing::post(bulk_create_cards),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery-rules/:rule_or_cookie_id"),
            get(list_delivery_rules)
                .post(create_delivery_rule)
                .put(update_delivery_rule)
                .delete(delete_delivery_rule),
        )
}

async fn get_offline_reply(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Query(query): Query<OfflineReplyQuery>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let payload = crate::business_auto::offline_reply::load_offline_reply_config(
        &state,
        &scoped_cookie_id,
        &query.item_id,
    )
    .await
    .map_err(upstream_error)?;
    Ok(ResponseJson(payload))
}

async fn sync_offline_reply(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<OfflineReplySyncPayload>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let input = crate::business_auto::offline_reply::OfflineReplySyncInput {
        item_id: payload.item_id.clone(),
        enable_offline_reply: payload.enable_offline_reply,
        offline_reply_text: payload.offline_reply_text.clone(),
    };
    let result = crate::business_auto::offline_reply::sync_offline_reply_config(
        &state,
        &scoped_cookie_id,
        input,
    )
    .await
    .map_err(upstream_error)?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "sync_offline_reply",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "offline_reply",
            target_id: payload.item_id.as_str(),
            target_title: "不在线回复",
            status: "success",
            error_code: "",
            message: "同步不在线回复成功",
            params: json!(payload),
            before: json!({}),
            after: result.clone(),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(result))
}

async fn get_ai_settings(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let settings = state
        .storage()
        .get_ai_settings(&scoped_cookie_id)
        .await
        .ok()
        .flatten();

    match settings {
        Some(settings) => Ok(ResponseJson(json!(settings))),
        None => Ok(ResponseJson(json!({}))),
    }
}

async fn update_ai_settings(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<AiSettingsUpdatePayload>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let before = state
        .storage()
        .get_ai_settings(&scoped_cookie_id)
        .await
        .ok()
        .flatten();
    let settings = state
        .storage()
        .upsert_ai_settings(&scoped_cookie_id, &payload)
        .await
        .expect("ai settings persist");

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "update_ai_settings",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "ai_settings",
            target_id: &public_cookie_id(&scoped_cookie_id),
            target_title: "",
            status: "success",
            error_code: "",
            message: "更新 AI 设置成功",
            params: json!(payload),
            before: json!(before),
            after: json!(settings),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "settings": settings,
    })))
}

async fn discover_ai_models(
    Extension(_identity): Extension<RequestIdentity>,
    Json(payload): Json<AiModelsDiscoverPayload>,
) -> ApiResult {
    let api_key = required_trimmed_field(payload.api_key.as_deref(), "请先填写 API Key")?;
    let base_url = required_trimmed_field(payload.base_url.as_deref(), "请先填写 Base URL")?;
    let _api_type = payload.api_type.unwrap_or_else(|| "openai".to_string());

    let result = fetch_ai_models(base_url, api_key)
        .await
        .map_err(ai_probe_error)?;

    Ok(ResponseJson(json!({
        "status": "ok",
        "models": result.models,
        "endpoint": result.endpoint,
    })))
}

async fn test_ai_settings_connection(
    Extension(_identity): Extension<RequestIdentity>,
    Json(payload): Json<AiConnectionTestPayload>,
) -> ApiResult {
    let api_key = required_trimmed_field(payload.api_key.as_deref(), "请先填写 API Key")?;
    let base_url = required_trimmed_field(payload.base_url.as_deref(), "请先填写 Base URL")?;
    let _api_type = payload.api_type.unwrap_or_else(|| "openai".to_string());
    let model_name = payload.model_name.unwrap_or_default().trim().to_string();

    if model_name.is_empty() {
        let result = fetch_ai_models(base_url, api_key)
            .await
            .map_err(ai_probe_error)?;
        return Ok(ResponseJson(json!({
            "status": "ok",
            "ok": true,
            "message": format!("连通成功，已获取 {} 个模型", result.models.len()),
            "endpoint": result.endpoint,
            "modelName": "",
            "models": result.models,
        })));
    }

    let endpoint = normalize_ai_chat_completion_url(base_url);
    let client = http_client().map_err(internal_storage_error)?;
    let response = client
        .post(endpoint.as_str())
        .bearer_auth(api_key)
        .json(&json!({
            "model": model_name,
            "messages": [
                {
                    "role": "user",
                    "content": "请只回复：ok"
                }
            ]
        }))
        .send()
        .await
        .map_err(internal_storage_error)?;
    let status = response.status();
    let body = read_json_or_text(response).await;
    if !status.is_success() {
        return Err(ai_probe_error(format!(
            "测试连接失败：HTTP {} {}",
            status.as_u16(),
            summarize_probe_body(&body),
        )));
    }

    let message = extract_ai_probe_message(&body);
    let result = AiConnectionTestResult {
        ok: true,
        message: if message.is_empty() {
            "连接成功，模型可用".to_string()
        } else {
            format!("连接成功，模型回复：{message}")
        },
        endpoint,
        model_name,
        models: Vec::new(),
    };
    Ok(ResponseJson(json!({
        "status": "ok",
        "ok": result.ok,
        "message": result.message,
        "endpoint": result.endpoint,
        "modelName": result.model_name,
        "models": result.models,
    })))
}

async fn list_keywords(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let keywords = state
        .storage()
        .list_keywords_with_global(&scoped_cookie_id)
        .await
        .expect("keywords list");
    Ok(ResponseJson(json!(keywords)))
}

async fn create_keyword(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<KeywordCreatePayload>,
) -> ApiResult {
    ensure_cookie_path_matches(&cookie_id, payload.cookie_id.as_ref())?;
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;

    let keyword = state
        .storage()
        .create_keyword(&scoped_cookie_id, &payload)
        .await
        .expect("keyword create");

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "create_keyword",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "keyword",
            target_id: &keyword.id.to_string(),
            target_title: &keyword.keyword,
            status: "success",
            error_code: "",
            message: "创建关键词成功",
            params: json!(payload),
            before: json!({}),
            after: json!(keyword),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "keyword": keyword,
    })))
}

async fn update_keyword(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(keyword_id): Path<i64>,
    Json(payload): Json<KeywordUpdatePayload>,
) -> ApiResult {
    let before = ensure_keyword_access(&state, &identity, keyword_id).await?;
    let mut payload = payload;
    if let Some(cookie_id) = payload.cookie_id.clone() {
        payload.cookie_id =
            Some(resolve_business_cookie_scope(&state, &identity, &cookie_id).await?);
    }
    let keyword = state
        .storage()
        .update_keyword(keyword_id, &payload)
        .await
        .expect("keyword update")
        .ok_or_else(|| not_found("关键词不存在"))?;

    maybe_disable_account_auto_reply_if_no_sources(state.storage(), &before.cookie_id).await?;
    if keyword.cookie_id != before.cookie_id {
        maybe_disable_account_auto_reply_if_no_sources(state.storage(), &keyword.cookie_id).await?;
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "update_keyword",
            operator: &keyword.cookie_id,
            trigger_source: "manual",
            uid: &keyword.cookie_id,
            account_nick: "",
            target_type: "keyword",
            target_id: &keyword_id.to_string(),
            target_title: &keyword.keyword,
            status: "success",
            error_code: "",
            message: "更新关键词成功",
            params: json!(payload),
            before: json!(before),
            after: json!(keyword),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "keyword": keyword,
    })))
}

async fn delete_keyword(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(keyword_id): Path<i64>,
) -> ApiResult {
    let before = Some(ensure_keyword_access(&state, &identity, keyword_id).await?);
    let deleted = state
        .storage()
        .delete_keyword(keyword_id)
        .await
        .expect("keyword delete");

    if !deleted {
        return Err(not_found("关键词不存在"));
    }

    if let Some(current) = before.as_ref() {
        maybe_disable_account_auto_reply_if_no_sources(state.storage(), &current.cookie_id).await?;
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "delete_keyword",
            operator: "",
            trigger_source: "manual",
            uid: "",
            account_nick: "",
            target_type: "keyword",
            target_id: &keyword_id.to_string(),
            target_title: "",
            status: "success",
            error_code: "",
            message: "删除关键词成功",
            params: json!({}),
            before: json!(before),
            after: json!({
                "deleted": true,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({ "status": "ok" })))
}

async fn list_item_replies(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let replies = state
        .storage()
        .list_item_replies(&scoped_cookie_id)
        .await
        .expect("item replies list");
    Ok(ResponseJson(json!(replies)))
}

async fn create_item_reply(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<ItemReplyCreatePayload>,
) -> ApiResult {
    ensure_cookie_path_matches(&cookie_id, payload.cookie_id.as_ref())?;
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    ensure_business_item_access(&state, &identity, &scoped_cookie_id, &payload.item_id).await?;

    let item_reply = state
        .storage()
        .create_item_reply(&scoped_cookie_id, &payload)
        .await
        .expect("item reply create");

    maybe_seed_item_reply_mode_from_item_reply(
        state.storage(),
        &scoped_cookie_id,
        payload.item_id.as_str(),
    )
    .await?;

    reset_item_reply_run_history_if_needed(state.storage(), None, Some(&item_reply)).await?;

    reset_default_reply_history_if_needed(
        state.storage(),
        None,
        Some((&scoped_cookie_id, payload.item_id.as_str())),
    )
    .await?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "create_item_reply",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "item_reply",
            target_id: &item_reply.id.to_string(),
            target_title: &item_reply.item_id,
            status: "success",
            error_code: "",
            message: "创建商品自动回复成功",
            params: json!(payload),
            before: json!({}),
            after: json!(item_reply),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "itemReply": item_reply,
    })))
}

async fn update_item_reply(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(item_reply_id): Path<i64>,
    Json(payload): Json<ItemReplyUpdatePayload>,
) -> ApiResult {
    let current = ensure_item_reply_access(&state, &identity, item_reply_id).await?;
    let mut payload = payload;
    let next_cookie_id = payload
        .cookie_id
        .clone()
        .unwrap_or_else(|| current.cookie_id.clone());
    payload.cookie_id =
        Some(resolve_business_cookie_scope(&state, &identity, &next_cookie_id).await?);
    let next_item_id = payload
        .item_id
        .clone()
        .unwrap_or_else(|| current.item_id.clone());
    ensure_business_item_access(
        &state,
        &identity,
        payload.cookie_id.as_deref().unwrap_or_default(),
        &next_item_id,
    )
    .await?;

    let item_reply = state
        .storage()
        .update_item_reply(item_reply_id, &payload)
        .await
        .expect("item reply update")
        .ok_or_else(|| not_found("商品自动回复不存在"))?;

    maybe_cleanup_or_seed_item_reply_mode(state.storage(), &current.cookie_id, &current.item_id)
        .await?;
    maybe_seed_item_reply_mode_from_item_reply(
        state.storage(),
        &item_reply.cookie_id,
        &item_reply.item_id,
    )
    .await?;

    reset_item_reply_run_history_if_needed(state.storage(), Some(&current), Some(&item_reply))
        .await?;

    reset_default_reply_history_if_needed(
        state.storage(),
        Some((current.cookie_id.as_str(), current.item_id.as_str())),
        Some((item_reply.cookie_id.as_str(), item_reply.item_id.as_str())),
    )
    .await?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "update_item_reply",
            operator: &item_reply.cookie_id,
            trigger_source: "manual",
            uid: &item_reply.cookie_id,
            account_nick: "",
            target_type: "item_reply",
            target_id: &item_reply_id.to_string(),
            target_title: &item_reply.item_id,
            status: "success",
            error_code: "",
            message: "更新商品自动回复成功",
            params: json!(payload),
            before: json!(current),
            after: json!(item_reply),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "itemReply": item_reply,
    })))
}

async fn delete_item_reply(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(item_reply_id): Path<i64>,
) -> ApiResult {
    let before = Some(ensure_item_reply_access(&state, &identity, item_reply_id).await?);
    let deleted = state
        .storage()
        .delete_item_reply(item_reply_id)
        .await
        .expect("item reply delete");

    if !deleted {
        return Err(not_found("商品自动回复不存在"));
    }

    if let Some(current) = before.as_ref() {
        reset_item_reply_run_history_if_needed(state.storage(), Some(current), None).await?;
        reset_default_reply_history_if_needed(
            state.storage(),
            Some((current.cookie_id.as_str(), current.item_id.as_str())),
            None,
        )
        .await?;
        maybe_cleanup_or_seed_item_reply_mode(
            state.storage(),
            &current.cookie_id,
            &current.item_id,
        )
        .await?;
        maybe_disable_account_auto_reply_if_no_sources(state.storage(), &current.cookie_id).await?;
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "delete_item_reply",
            operator: "",
            trigger_source: "manual",
            uid: before
                .as_ref()
                .map(|it| it.cookie_id.as_str())
                .unwrap_or(""),
            account_nick: "",
            target_type: "item_reply",
            target_id: &item_reply_id.to_string(),
            target_title: before.as_ref().map(|it| it.item_id.as_str()).unwrap_or(""),
            status: "success",
            error_code: "",
            message: "删除商品自动回复成功",
            params: json!({}),
            before: json!(before),
            after: json!({
                "deleted": true,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({ "status": "ok" })))
}

async fn list_item_reply_modes(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let modes = state
        .storage()
        .list_item_reply_modes(&scoped_cookie_id)
        .await
        .expect("item reply modes list");
    Ok(ResponseJson(json!(modes)))
}

async fn upsert_item_reply_mode(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<ItemReplyModeUpsertPayload>,
) -> ApiResult {
    ensure_cookie_path_matches(&cookie_id, payload.cookie_id.as_ref())?;
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let item_id = payload.item_id.trim();
    if item_id.is_empty() {
        return Err(bad_request("商品 ID 不能为空"));
    }
    ensure_business_item_access(&state, &identity, &scoped_cookie_id, item_id).await?;
    let reply_mode = payload.reply_mode.normalize_item();
    let next_payload = ItemReplyModeUpsertPayload {
        cookie_id: Some(scoped_cookie_id.clone()),
        item_id: item_id.to_string(),
        reply_mode,
    };
    let item_reply_mode = state
        .storage()
        .upsert_item_reply_mode(&scoped_cookie_id, &next_payload)
        .await
        .expect("item reply mode persist");

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "upsert_item_reply_mode",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "item_reply_mode",
            target_id: item_id,
            target_title: item_id,
            status: "success",
            error_code: "",
            message: "更新商品回复模式成功",
            params: json!(next_payload),
            before: json!({}),
            after: json!(item_reply_mode),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "itemReplyMode": item_reply_mode,
    })))
}

async fn delete_item_reply_mode(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<ItemReplyModeDeletePayload>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let item_id = payload.item_id.trim();
    if item_id.is_empty() {
        return Err(bad_request("商品 ID 不能为空"));
    }
    ensure_business_item_access(&state, &identity, &scoped_cookie_id, item_id).await?;
    let deleted = state
        .storage()
        .delete_item_reply_mode(&scoped_cookie_id, item_id)
        .await
        .expect("item reply mode delete");

    if !deleted {
        return Err(not_found("商品回复模式不存在"));
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "delete_item_reply_mode",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "item_reply_mode",
            target_id: item_id,
            target_title: item_id,
            status: "success",
            error_code: "",
            message: "删除商品回复模式成功",
            params: json!(payload),
            before: json!({}),
            after: json!({ "deleted": true }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({ "status": "ok" })))
}

async fn reset_default_reply_history_if_needed(
    storage: &crate::storage::StorageState,
    current: Option<(&str, &str)>,
    next: Option<(&str, &str)>,
) -> Result<(), (StatusCode, ResponseJson<Value>)> {
    let mut cookie_ids = std::collections::BTreeSet::new();

    for (cookie_id, item_id) in current.into_iter().chain(next.into_iter()) {
        if is_global_scope_key(cookie_id) || !item_id.trim().is_empty() {
            continue;
        }
        cookie_ids.insert(cookie_id.to_string());
    }

    for cookie_id in cookie_ids {
        storage
            .clear_default_reply_records(&cookie_id)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ResponseJson(json!({ "detail": error.to_string() })),
                )
            })?;
    }

    Ok(())
}

async fn reset_item_reply_run_history_if_needed(
    storage: &crate::storage::StorageState,
    current: Option<&ItemReply>,
    next: Option<&ItemReply>,
) -> Result<(), (StatusCode, ResponseJson<Value>)> {
    let mut targets = std::collections::BTreeSet::new();

    for reply in current.into_iter().chain(next.into_iter()) {
        if crate::storage::is_global_scope_key(&reply.cookie_id) {
            continue;
        }
        targets.insert((reply.cookie_id.clone(), reply.task_key()));
    }

    for (cookie_id, task_key) in targets {
        storage
            .clear_item_reply_task_runs(&cookie_id, &task_key)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ResponseJson(json!({ "detail": error.to_string() })),
                )
            })?;
    }

    Ok(())
}

async fn maybe_disable_account_auto_reply_if_no_sources(
    storage: &crate::storage::StorageState,
    cookie_id: &str,
) -> Result<(), (StatusCode, ResponseJson<Value>)> {
    if is_global_scope_key(cookie_id) {
        return Ok(());
    }

    let settings = storage
        .get_ai_settings(cookie_id)
        .await
        .map_err(internal_storage_error)?
        .unwrap_or_default();
    let reply_mode = settings.reply_mode.normalize_account();
    if reply_mode == ReplyMode::Off {
        return Ok(());
    }

    let has_default_replies = storage
        .list_item_replies(cookie_id)
        .await
        .map_err(internal_storage_error)?
        .into_iter()
        .any(|reply| {
            reply.enabled && reply.item_id.trim().is_empty() && !reply.reply.trim().is_empty()
        });
    let has_keywords = storage
        .list_keywords(cookie_id)
        .await
        .map_err(internal_storage_error)?
        .into_iter()
        .any(|keyword| {
            keyword.enabled
                && !keyword.keyword.trim().is_empty()
                && !keyword.reply.trim().is_empty()
        });
    let next_reply_mode = match reply_mode {
        ReplyMode::Default if has_default_replies => None,
        ReplyMode::Default if has_keywords => Some(ReplyMode::Keyword),
        ReplyMode::Default => Some(ReplyMode::Off),
        ReplyMode::Keyword if has_keywords => None,
        ReplyMode::Keyword if has_default_replies => Some(ReplyMode::Default),
        ReplyMode::Keyword => Some(ReplyMode::Off),
        ReplyMode::Ai => None,
        _ => None,
    };
    let Some(next_reply_mode) = next_reply_mode else {
        return Ok(());
    };

    let mut extra = account_extra_map(storage.accounts(), cookie_id)
        .await
        .map_err(internal_storage_error)?
        .unwrap_or_default();
    extra.insert(
        "replyMode".to_string(),
        Value::String(next_reply_mode.as_str().to_string()),
    );
    extra.insert(
        "autoReply".to_string(),
        Value::Bool(next_reply_mode.auto_reply_enabled()),
    );
    extra.insert(
        "aiEnabled".to_string(),
        Value::Bool(next_reply_mode.ai_enabled()),
    );
    persist_account_extra(storage.accounts(), cookie_id, &extra)
        .await
        .map_err(internal_response_error)?;

    storage
        .upsert_ai_settings(
            cookie_id,
            &AiSettingsUpdatePayload {
                reply_mode: Some(next_reply_mode),
                ..AiSettingsUpdatePayload::default()
            },
        )
        .await
        .map_err(internal_storage_error)?;

    Ok(())
}

async fn maybe_seed_item_reply_mode_from_item_reply(
    storage: &crate::storage::StorageState,
    cookie_id: &str,
    item_id: &str,
) -> Result<(), (StatusCode, ResponseJson<Value>)> {
    if is_global_scope_key(cookie_id) || item_id.trim().is_empty() {
        return Ok(());
    }

    let existing = storage
        .get_item_reply_mode(cookie_id, item_id)
        .await
        .map_err(internal_storage_error)?;
    if existing.is_some() {
        return Ok(());
    }

    storage
        .upsert_item_reply_mode(
            cookie_id,
            &ItemReplyModeUpsertPayload {
                cookie_id: Some(cookie_id.to_string()),
                item_id: item_id.trim().to_string(),
                reply_mode: ReplyMode::ItemReply,
            },
        )
        .await
        .map_err(internal_storage_error)?;

    Ok(())
}

async fn maybe_cleanup_or_seed_item_reply_mode(
    storage: &crate::storage::StorageState,
    cookie_id: &str,
    item_id: &str,
) -> Result<(), (StatusCode, ResponseJson<Value>)> {
    if is_global_scope_key(cookie_id) || item_id.trim().is_empty() {
        return Ok(());
    }

    let mode = storage
        .get_item_reply_mode(cookie_id, item_id)
        .await
        .map_err(internal_storage_error)?;
    let Some(mode) = mode else {
        return Ok(());
    };

    let has_item_replies = storage
        .list_item_replies(cookie_id)
        .await
        .map_err(internal_storage_error)?
        .into_iter()
        .any(|reply| {
            reply.enabled
                && reply.item_id.trim() == item_id.trim()
                && !reply.reply.trim().is_empty()
        });

    if has_item_replies {
        return Ok(());
    }

    if mode.reply_mode == ReplyMode::ItemReply {
        storage
            .delete_item_reply_mode(cookie_id, item_id)
            .await
            .map_err(internal_storage_error)?;
    }

    Ok(())
}

async fn list_cards(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let cards = state
        .storage()
        .list_cards(&scoped_cookie_id)
        .await
        .expect("cards list");
    Ok(ResponseJson(json!(cards)))
}

async fn bulk_create_cards(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CardsBulkCreatePayload>,
) -> ApiResult {
    let scoped_cookie_id =
        resolve_business_cookie_scope(&state, &identity, &payload.cookie_id).await?;
    if !is_global_scope_key(&scoped_cookie_id) {
        let owner = owner_username_filter(&state, &identity);
        ensure_account_can_call_upstream(&state, owner.as_deref(), &scoped_cookie_id)
            .await
            .map_err(|error| {
                let status = error.status();
                let detail = error
                    .public_detail()
                    .unwrap_or("卡密导入服务暂时不可用，请稍后重试");
                (
                    status,
                    ResponseJson(json!({ "status": "error", "detail": detail })),
                )
            })?;
    }
    let cards = state
        .storage()
        .bulk_create_cards(&scoped_cookie_id, &payload.cards)
        .await
        .map_err(card_write_storage_error)?;
    let pool_tags = payload
        .cards
        .iter()
        .map(|card| card.card_pool_tag.trim())
        .filter(|tag| !tag.is_empty())
        .collect::<HashSet<_>>();
    let pool_scope = if is_global_scope_key(&scoped_cookie_id) {
        "global"
    } else {
        "account"
    };
    for pool_tag in &pool_tags {
        crate::business_auto::delivery::wake_pool(&scoped_cookie_id, pool_scope, pool_tag).await;
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "bulk_create_cards",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "card_pool",
            target_id: &payload.cookie_id,
            target_title: "",
            status: "success",
            error_code: "",
            message: "批量导入卡密成功",
            params: json!({
                "importCount": payload.cards.len(),
                "poolScope": pool_scope,
                "poolCount": pool_tags.len(),
            }),
            before: json!({}),
            after: json!({
                "insertedCount": cards.len(),
            }),
            extra: json!({
                "poolScope": pool_scope,
                "poolCount": pool_tags.len(),
            }),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "cards": cards,
    })))
}

async fn delete_card(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(card_id): Path<i64>,
) -> ApiResult {
    let before = Some(ensure_card_access(&state, &identity, card_id).await?);
    let deleted = state
        .storage()
        .delete_card(card_id)
        .await
        .expect("card delete");

    if !deleted {
        return Err(not_found("卡密不存在"));
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "delete_card",
            operator: "",
            trigger_source: "manual",
            uid: "",
            account_nick: "",
            target_type: "card",
            target_id: &card_id.to_string(),
            target_title: "",
            status: "success",
            error_code: "",
            message: "删除卡密成功",
            params: json!({}),
            before: json!(before),
            after: json!({
                "deleted": true,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({ "status": "ok" })))
}

async fn update_card(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(card_id): Path<i64>,
    Json(payload): Json<CardUpdatePayload>,
) -> ApiResult {
    let before = ensure_card_access(&state, &identity, card_id).await?;

    if payload.card_content.trim().is_empty() {
        return Err(bad_request("cardContent 不能为空"));
    }
    if payload.card_pool_tag.trim().is_empty() {
        return Err(bad_request("cardPoolTag 不能为空"));
    }

    let card = state
        .storage()
        .update_card(card_id, &payload.card_content, &payload.card_pool_tag)
        .await
        .map_err(card_write_storage_error)?
        .ok_or_else(|| not_found("卡密不存在"))?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "update_card",
            operator: "",
            trigger_source: "manual",
            uid: "",
            account_nick: "",
            target_type: "card",
            target_id: &card_id.to_string(),
            target_title: "",
            status: "success",
            error_code: "",
            message: "更新卡密成功",
            params: json!({}),
            before: json!(before),
            after: json!(card),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "card": card,
    })))
}

async fn list_delivery_rules(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
) -> ApiResult {
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    let delivery_rules = state
        .storage()
        .list_delivery_rules_with_global(&scoped_cookie_id)
        .await
        .expect("delivery rules list");
    Ok(ResponseJson(json!(delivery_rules)))
}

async fn create_delivery_rule(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(cookie_id): Path<String>,
    Json(payload): Json<DeliveryRuleCreatePayload>,
) -> ApiResult {
    ensure_cookie_path_matches(&cookie_id, payload.cookie_id.as_ref())?;
    let scoped_cookie_id = resolve_business_cookie_scope(&state, &identity, &cookie_id).await?;
    ensure_delivery_target(payload.keyword.as_str(), payload.item_id.as_str())?;
    ensure_business_item_access(&state, &identity, &scoped_cookie_id, &payload.item_id).await?;
    ensure_card_pool_scope(payload.card_pool_scope.as_str())?;
    ensure_card_pool_exists(
        state.storage(),
        &scoped_cookie_id,
        payload.card_pool_scope.as_str(),
        &payload.card_pool_tag,
    )
    .await?;

    let delivery_rule = state
        .storage()
        .create_delivery_rule(&scoped_cookie_id, &payload)
        .await
        .expect("delivery rule create");

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "create_delivery_rule",
            operator: &public_cookie_id(&scoped_cookie_id),
            trigger_source: "manual",
            uid: &public_cookie_id(&scoped_cookie_id),
            account_nick: "",
            target_type: "delivery_rule",
            target_id: &delivery_rule.id.to_string(),
            target_title: &delivery_rule.keyword,
            status: "success",
            error_code: "",
            message: "创建发货规则成功",
            params: json!(payload),
            before: json!({}),
            after: json!(delivery_rule),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "deliveryRule": delivery_rule,
    })))
}

async fn update_delivery_rule(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(rule_id): Path<i64>,
    Json(payload): Json<DeliveryRuleUpdatePayload>,
) -> ApiResult {
    let current = ensure_delivery_rule_access(&state, &identity, rule_id).await?;
    let mut payload = payload;

    let raw_next_cookie_id = payload
        .cookie_id
        .clone()
        .unwrap_or_else(|| current.cookie_id.clone());
    let next_cookie_id =
        resolve_business_cookie_scope(&state, &identity, &raw_next_cookie_id).await?;
    payload.cookie_id = Some(next_cookie_id.clone());
    let next_keyword = payload
        .keyword
        .clone()
        .unwrap_or_else(|| current.keyword.clone());
    let next_item_id = payload
        .item_id
        .clone()
        .unwrap_or_else(|| current.item_id.clone());
    let next_card_pool_tag = payload
        .card_pool_tag
        .clone()
        .unwrap_or_else(|| current.card_pool_tag.clone());
    let next_card_pool_scope = payload
        .card_pool_scope
        .clone()
        .unwrap_or_else(|| current.card_pool_scope.clone());

    ensure_delivery_target(next_keyword.as_str(), next_item_id.as_str())?;
    ensure_business_item_access(&state, &identity, &next_cookie_id, &next_item_id).await?;
    ensure_card_pool_scope(next_card_pool_scope.as_str())?;
    ensure_card_pool_exists(
        state.storage(),
        &next_cookie_id,
        next_card_pool_scope.as_str(),
        &next_card_pool_tag,
    )
    .await?;

    let delivery_rule = state
        .storage()
        .update_delivery_rule(rule_id, &payload)
        .await
        .expect("delivery rule update")
        .ok_or_else(|| not_found("发货规则不存在"))?;

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "update_delivery_rule",
            operator: &delivery_rule.cookie_id,
            trigger_source: "manual",
            uid: &delivery_rule.cookie_id,
            account_nick: "",
            target_type: "delivery_rule",
            target_id: &rule_id.to_string(),
            target_title: &delivery_rule.keyword,
            status: "success",
            error_code: "",
            message: "更新发货规则成功",
            params: json!(payload),
            before: json!(current),
            after: json!(delivery_rule),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({
        "status": "ok",
        "deliveryRule": delivery_rule,
    })))
}

async fn delete_delivery_rule(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(rule_id): Path<i64>,
) -> ApiResult {
    let before = Some(ensure_delivery_rule_access(&state, &identity, rule_id).await?);
    let deleted = state
        .storage()
        .delete_delivery_rule(rule_id)
        .await
        .expect("delivery rule delete");

    if !deleted {
        return Err(not_found("发货规则不存在"));
    }

    append_operation_log(
        &state,
        crate::storage::NewOperationLog {
            job_id: "",
            module: "config",
            action: "delete_delivery_rule",
            operator: "",
            trigger_source: "manual",
            uid: before
                .as_ref()
                .map(|it| it.cookie_id.as_str())
                .unwrap_or(""),
            account_nick: "",
            target_type: "delivery_rule",
            target_id: &rule_id.to_string(),
            target_title: before.as_ref().map(|it| it.keyword.as_str()).unwrap_or(""),
            status: "success",
            error_code: "",
            message: "删除发货规则成功",
            params: json!({}),
            before: json!(before),
            after: json!({
                "deleted": true,
            }),
            extra: json!({}),
        },
    )
    .await;

    Ok(ResponseJson(json!({ "status": "ok" })))
}

fn ensure_cookie_path_matches(
    cookie_id: &str,
    request_cookie_id: Option<&String>,
) -> ValidationResult {
    if let Some(request_cookie_id) = request_cookie_id {
        if !request_cookie_id.is_empty() && request_cookie_id != cookie_id {
            return Err(bad_request("cookieId 与路径参数不一致"));
        }
    }

    Ok(())
}

fn business_owner_filter(_state: &AppState, identity: &RequestIdentity) -> String {
    identity.username.trim().to_string()
}

async fn resolve_business_cookie_scope(
    state: &AppState,
    identity: &RequestIdentity,
    raw_cookie_id: &str,
) -> Result<String, (StatusCode, ResponseJson<Value>)> {
    let cookie_id = raw_cookie_id.trim();
    if cookie_id.is_empty() {
        return Err(bad_request("cookieId is required"));
    }

    let owner = business_owner_filter(state, identity);
    if owner.is_empty() {
        if is_global_scope_key(cookie_id) {
            return Ok(GLOBAL_AI_SETTINGS_KEY.to_string());
        }
        return Ok(cookie_id.to_string());
    }

    if cookie_id == GLOBAL_AI_SETTINGS_KEY {
        return Ok(user_global_scope_key(&owner));
    }

    if cookie_id != GLOBAL_AI_SETTINGS_KEY && is_global_scope_key(cookie_id) {
        let user_scope = user_global_scope_key(&owner);
        if cookie_id == user_scope {
            return Ok(user_scope);
        }
        return Err(not_found("配置不存在"));
    }

    ensure_business_cookie_access(state, identity, cookie_id).await?;
    Ok(cookie_id.to_string())
}

async fn ensure_business_cookie_access(
    state: &AppState,
    identity: &RequestIdentity,
    cookie_id: &str,
) -> ValidationResult {
    if identity.legacy {
        return Ok(());
    }
    let owner = business_owner_filter(state, identity);

    if account_uid_accessible(state.storage().accounts(), Some(&owner), cookie_id)
        .await
        .expect("account access lookup")
    {
        Ok(())
    } else {
        Err(not_found("账号不存在"))
    }
}

fn ensure_business_scope_access(
    state: &AppState,
    identity: &RequestIdentity,
    cookie_id: &str,
) -> ValidationResult {
    let owner = business_owner_filter(state, identity);
    if owner.is_empty() {
        return if cookie_id == GLOBAL_AI_SETTINGS_KEY {
            Ok(())
        } else {
            Err(not_found("配置不存在"))
        };
    }

    if cookie_id == user_global_scope_key(&owner) {
        return Ok(());
    }

    if is_global_scope_key(cookie_id) {
        return Err(not_found("配置不存在"));
    }

    Err(not_found("配置不存在"))
}

async fn ensure_keyword_access(
    state: &AppState,
    identity: &RequestIdentity,
    keyword_id: i64,
) -> Result<Keyword, (StatusCode, ResponseJson<Value>)> {
    let keyword = state
        .storage()
        .get_keyword(keyword_id)
        .await
        .expect("keyword lookup")
        .ok_or_else(|| not_found("关键词不存在"))?;

    ensure_record_cookie_access(state, identity, &keyword.cookie_id).await?;
    Ok(keyword)
}

async fn ensure_item_reply_access(
    state: &AppState,
    identity: &RequestIdentity,
    item_reply_id: i64,
) -> Result<ItemReply, (StatusCode, ResponseJson<Value>)> {
    let item_reply = state
        .storage()
        .get_item_reply(item_reply_id)
        .await
        .expect("item reply lookup")
        .ok_or_else(|| not_found("商品自动回复不存在"))?;

    ensure_record_cookie_access(state, identity, &item_reply.cookie_id).await?;
    Ok(item_reply)
}

async fn ensure_card_access(
    state: &AppState,
    identity: &RequestIdentity,
    card_id: i64,
) -> Result<Card, (StatusCode, ResponseJson<Value>)> {
    let card = state
        .storage()
        .get_card(card_id)
        .await
        .expect("card lookup")
        .ok_or_else(|| not_found("卡密不存在"))?;

    ensure_record_cookie_access(state, identity, &card.cookie_id).await?;
    Ok(card)
}

async fn ensure_delivery_rule_access(
    state: &AppState,
    identity: &RequestIdentity,
    rule_id: i64,
) -> Result<DeliveryRule, (StatusCode, ResponseJson<Value>)> {
    let rule = state
        .storage()
        .get_delivery_rule(rule_id)
        .await
        .expect("delivery rule lookup")
        .ok_or_else(|| not_found("发货规则不存在"))?;

    ensure_record_cookie_access(state, identity, &rule.cookie_id).await?;
    Ok(rule)
}

async fn ensure_record_cookie_access(
    state: &AppState,
    identity: &RequestIdentity,
    cookie_id: &str,
) -> ValidationResult {
    if is_global_scope_key(cookie_id) {
        return ensure_business_scope_access(state, identity, cookie_id);
    }

    ensure_business_cookie_access(state, identity, cookie_id).await
}

async fn ensure_business_item_access(
    state: &AppState,
    identity: &RequestIdentity,
    scoped_cookie_id: &str,
    item_id: &str,
) -> ValidationResult {
    let item_id = item_id.trim();
    if item_id.is_empty() {
        return Ok(());
    }

    if is_global_scope_key(scoped_cookie_id) {
        return ensure_business_global_item_access(state, identity, item_id).await;
    }

    let found = sqlx::query("SELECT 1 FROM account_items WHERE uid = ? AND item_id = ? LIMIT 1")
        .bind(scoped_cookie_id.trim())
        .bind(item_id)
        .fetch_optional(state.storage().xianyu())
        .await
        .expect("business item access lookup")
        .is_some();
    if found {
        Ok(())
    } else {
        Err(not_found("商品不存在"))
    }
}

async fn ensure_business_global_item_access(
    state: &AppState,
    identity: &RequestIdentity,
    item_id: &str,
) -> ValidationResult {
    let owner = if identity.legacy {
        String::new()
    } else {
        business_owner_filter(state, identity)
    };
    if owner.is_empty() {
        let found = sqlx::query("SELECT 1 FROM account_items WHERE item_id = ? LIMIT 1")
            .bind(item_id.trim())
            .fetch_optional(state.storage().xianyu())
            .await
            .expect("business global item access lookup")
            .is_some();
        return if found {
            Ok(())
        } else {
            Err(not_found("商品不存在"))
        };
    }

    let rows = sqlx::query("SELECT uid FROM account_items WHERE item_id = ?")
        .bind(item_id.trim())
        .fetch_all(state.storage().xianyu())
        .await
        .expect("business global item candidates lookup");
    for row in rows {
        let Ok(uid) = row.try_get::<String, _>("uid") else {
            continue;
        };
        if account_uid_accessible(state.storage().accounts(), Some(&owner), &uid)
            .await
            .expect("business global item owner lookup")
        {
            return Ok(());
        }
    }
    Err(not_found("商品不存在"))
}

fn ensure_delivery_target(keyword: &str, item_id: &str) -> ValidationResult {
    if item_id.trim().is_empty() {
        return Err(bad_request("itemId is required for auto delivery"));
    }

    let _ = keyword;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_detail(result: ValidationResult) -> String {
        let Err((_, ResponseJson(value))) = result else {
            return String::new();
        };
        value
            .get("detail")
            .and_then(|it| it.as_str())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn delivery_target_requires_item_id() {
        assert_eq!(
            error_detail(ensure_delivery_target("激活码", "")),
            "itemId is required for auto delivery"
        );
        assert!(ensure_delivery_target("", "item-1").is_ok());
        assert!(ensure_delivery_target("激活码", "item-1").is_ok());
    }

    #[test]
    fn ai_models_url_prefers_models_endpoint() {
        assert_eq!(
            normalize_ai_models_url("https://api.example.com/v1"),
            "https://api.example.com/v1/models"
        );
        assert_eq!(
            normalize_ai_models_url("https://api.example.com/v1/chat/completions"),
            "https://api.example.com/v1/models"
        );
        assert_eq!(
            normalize_ai_models_url("https://api.example.com/v1/models"),
            "https://api.example.com/v1/models"
        );
    }

    #[test]
    fn extract_ai_model_ids_supports_multiple_payload_shapes() {
        let openai = json!({
            "data": [
                { "id": "gpt-4o-mini" },
                { "id": "gpt-4.1" }
            ]
        });
        assert_eq!(
            extract_ai_model_ids(&openai),
            vec!["gpt-4o-mini".to_string(), "gpt-4.1".to_string()]
        );

        let gemini = json!({
            "models": [
                { "name": "gemini-2.5-flash" },
                { "name": "gemini-2.5-pro" }
            ]
        });
        assert_eq!(
            extract_ai_model_ids(&gemini),
            vec!["gemini-2.5-flash".to_string(), "gemini-2.5-pro".to_string()]
        );
    }
}

async fn ensure_card_pool_exists(
    storage: &StorageState,
    cookie_id: &str,
    card_pool_scope: &str,
    card_pool_tag: &str,
) -> ValidationResult {
    let exists = storage
        .card_pool_exists_scoped(cookie_id, card_pool_scope, card_pool_tag)
        .await
        .map_err(card_scope_validation_error)?;
    if !exists {
        return Err(bad_request("卡池不存在，请先导入对应卡密"));
    }

    Ok(())
}

fn ensure_card_pool_scope(scope: &str) -> ValidationResult {
    let trimmed = scope.trim();
    if trimmed.eq_ignore_ascii_case("account") || trimmed.eq_ignore_ascii_case("global") {
        return Ok(());
    }

    Err(bad_request("cardPoolScope must be account or global"))
}

fn upstream_error(error: String) -> (StatusCode, ResponseJson<Value>) {
    (
        StatusCode::BAD_GATEWAY,
        ResponseJson(json!({ "detail": error })),
    )
}

fn required_trimmed_field<'a>(
    value: Option<&'a str>,
    missing_message: &str,
) -> Result<&'a str, (StatusCode, ResponseJson<Value>)> {
    let trimmed = value.unwrap_or_default().trim();
    if trimmed.is_empty() {
        return Err(bad_request(missing_message));
    }
    Ok(trimmed)
}

async fn fetch_ai_models(base_url: &str, api_key: &str) -> Result<AiModelsDiscoverResult, String> {
    let endpoint = normalize_ai_models_url(base_url);
    let client = http_client()?;
    let response = client
        .get(endpoint.as_str())
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body = read_json_or_text(response).await;
    if !status.is_success() {
        return Err(format!(
            "获取模型列表失败：HTTP {} {}",
            status.as_u16(),
            summarize_probe_body(&body),
        ));
    }

    let models = extract_ai_model_ids(&body);
    if models.is_empty() {
        return Err("模型列表为空，请确认 Base URL 与 API Key 是否正确".to_string());
    }

    Ok(AiModelsDiscoverResult { models, endpoint })
}

fn normalize_ai_models_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/models") {
        return trimmed.to_string();
    }
    if let Some(prefix) = trimmed.strip_suffix("/chat/completions") {
        return format!("{prefix}/models");
    }
    format!("{trimmed}/models")
}

fn normalize_ai_chat_completion_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        return trimmed.to_string();
    }
    format!("{trimmed}/chat/completions")
}

fn extract_ai_model_ids(body: &Value) -> Vec<String> {
    let mut models = Vec::new();
    let mut push_model = |value: &Value| {
        let candidate = value
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| value.get("name").and_then(Value::as_str))
            .unwrap_or_default()
            .trim()
            .to_string();
        if candidate.is_empty() || models.iter().any(|it| it == &candidate) {
            return;
        }
        models.push(candidate);
    };

    if let Some(items) = body.get("data").and_then(Value::as_array) {
        for item in items {
            push_model(item);
        }
    }
    if let Some(items) = body.get("models").and_then(Value::as_array) {
        for item in items {
            push_model(item);
        }
    }
    models
}

fn extract_ai_probe_message(body: &Value) -> String {
    body.get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

async fn read_json_or_text(response: reqwest::Response) -> Value {
    let text = response.text().await.unwrap_or_default();
    serde_json::from_str::<Value>(text.as_str()).unwrap_or_else(|_| json!({ "raw": text }))
}

fn summarize_probe_body(body: &Value) -> String {
    body.get("error")
        .and_then(Value::as_object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .or_else(|| body.get("detail").and_then(Value::as_str))
        .or_else(|| body.get("message").and_then(Value::as_str))
        .or_else(|| body.get("raw").and_then(Value::as_str))
        .unwrap_or("请求未返回可读错误信息")
        .trim()
        .to_string()
}

fn ai_probe_error(error: impl Into<String>) -> (StatusCode, ResponseJson<Value>) {
    (
        StatusCode::BAD_REQUEST,
        ResponseJson(json!({ "detail": error.into() })),
    )
}

fn bad_request(message: &str) -> (StatusCode, ResponseJson<Value>) {
    (
        StatusCode::BAD_REQUEST,
        ResponseJson(json!({ "detail": message })),
    )
}

fn unprocessable(message: &str) -> (StatusCode, ResponseJson<Value>) {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        ResponseJson(json!({ "detail": message })),
    )
}

fn internal_storage_error(error: impl std::fmt::Display) -> (StatusCode, ResponseJson<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        ResponseJson(json!({ "detail": error.to_string() })),
    )
}

fn card_write_storage_error(error: StorageError) -> (StatusCode, ResponseJson<Value>) {
    match error {
        StorageError::DuplicateOneTimeCard { .. } => (
            StatusCode::CONFLICT,
            ResponseJson(json!({ "detail": "同一归属和卡池下已存在相同的一次性卡密" })),
        ),
        StorageError::OwnerScopeMissing { .. } => unprocessable("缺少 owner，不能直接使用全局卡池"),
        StorageError::BusyExhausted { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            ResponseJson(json!({ "detail": "数据正在被其他任务处理，请稍后重试" })),
        ),
        StorageError::Conflict { message } => (
            StatusCode::CONFLICT,
            ResponseJson(json!({ "detail": message })),
        ),
        error => {
            let _ = error;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseJson(json!({ "detail": "卡密导入服务暂时不可用，请稍后重试" })),
            )
        }
    }
}

fn card_scope_validation_error(error: StorageError) -> (StatusCode, ResponseJson<Value>) {
    match error {
        StorageError::OwnerScopeMissing { .. } => unprocessable("缺少 owner，不能直接使用全局卡池"),
        StorageError::BusyExhausted { .. } => (
            StatusCode::SERVICE_UNAVAILABLE,
            ResponseJson(json!({ "detail": "数据正在被其他任务处理，请稍后重试" })),
        ),
        error => {
            let _ = error;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ResponseJson(json!({ "detail": "卡池校验暂时不可用，请稍后重试" })),
            )
        }
    }
}

fn internal_response_error(
    error: crate::routes::error::ResponseError,
) -> (StatusCode, ResponseJson<Value>) {
    let status = error.status();
    let detail = error
        .public_detail()
        .unwrap_or("服务暂时不可用，请稍后重试");
    (
        status,
        ResponseJson(json!({ "status": "error", "detail": detail })),
    )
}

fn not_found(message: &str) -> (StatusCode, ResponseJson<Value>) {
    (
        StatusCode::NOT_FOUND,
        ResponseJson(json!({ "detail": message })),
    )
}
