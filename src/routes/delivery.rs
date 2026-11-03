use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Extension, Json, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{QueryBuilder, Row, Sqlite};

use crate::{
    models::{
        auth::RequestIdentity,
        delivery::{
            AddDeliveryCardsRequest, DeliveryConfiguration, DeliveryConfigurationRecord,
            DeliveryProduct, SaveDeliveryConfigurationRequest, UpdateDeliveryConfigurationRequest,
        },
    },
    routes::{
        accounts::runtime_access::ensure_account_can_call_upstream,
        error::ResponseError,
        market::items::{refresh_item_quantity_with_edit_fallback, RefreshItemQuantityError},
        tenant::{accessible_account_uids, ensure_account_uid_accessible, owner_username_filter},
    },
    state::AppState,
    storage::StorageError,
};

type ApiResult = Result<Json<Value>, Response>;
const MAX_RUNS_PAGE: i64 = 10_000;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigurationQuery {
    uid: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunsQuery {
    uid: Option<String>,
    configuration_id: Option<i64>,
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveryAlertsQuery {
    uid: Option<String>,
    status: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct RefreshStockQuery {
    uid: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/configurations"),
            get(list_configurations).post(create_configuration),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/configurations/:id"),
            axum::routing::put(update_configuration).delete(delete_configuration),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/configurations/:id/cards"),
            get(list_configuration_cards).post(add_configuration_cards),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/products/:item_id/refresh-stock"),
            post(refresh_product_stock),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/runs"),
            get(list_runs),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/runs/:id/retry"),
            post(retry_run),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/delivery/discovery-alerts"),
            get(list_discovery_alerts),
        )
}

async fn list_configurations(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ConfigurationQuery>,
) -> ApiResult {
    ensure_uid_access(&state, &identity, &query.uid).await?;
    let records = state
        .storage()
        .list_delivery_configuration_records(&query.uid)
        .await
        .map_err(storage_error)?;
    let item_ids = records
        .iter()
        .map(|record| record.item_id.clone())
        .collect::<Vec<_>>();
    let products = load_products(&state, &query.uid, &item_ids).await?;
    let pending_order_mismatches = state
        .storage()
        .pending_delivery_order_mismatches(&query.uid)
        .await
        .map_err(storage_error)?;
    let mut output = Vec::with_capacity(records.len());
    for record in records {
        let product = products
            .get(&record.item_id)
            .cloned()
            .unwrap_or_else(|| missing_product(&record.item_id));
        let available_card_count = state
            .storage()
            .available_delivery_card_count(&record)
            .await
            .map_err(storage_error)?;
        let mut configuration = configuration_from_parts(record, product, available_card_count);
        configuration.pending_order_mismatches = pending_order_mismatches.clone();
        output.push(configuration);
    }
    Ok(Json(json!(output)))
}

async fn create_configuration(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(mut input): Json<SaveDeliveryConfigurationRequest>,
) -> ApiResult {
    input.uid = input.uid.trim().to_string();
    input.item_id = input.item_id.trim().to_string();
    input.fixed_message = input.fixed_message.trim().to_string();
    input.pool_scope = input.pool_scope.trim().to_ascii_lowercase();
    input.pool_tag = input.pool_tag.trim().to_string();
    input.pool_tags = normalize_pool_tags(&input.pool_tags, &input.pool_tag);
    input.fixed_pool_tags = normalize_pool_tags(&input.fixed_pool_tags, "");
    if !input.pool_tags.is_empty() && !input.fixed_pool_tags.is_empty() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "固定卡密池和一次性卡密池不能同时配置",
        ));
    }
    input.one_time_enabled = !input.pool_tags.is_empty();
    ensure_uid_access(&state, &identity, &input.uid).await?;
    ensure_write_access(&state, &identity, &input.uid).await?;
    let product = load_product(&state, &input.uid, &input.item_id)
        .await?
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "商品不属于当前账号"))?;
    if !product.online || product.quantity.unwrap_or(0) <= 0 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "只能为在售且有库存的商品创建自动发货配置",
        ));
    }
    if input.pool_tags.is_empty()
        && input.fixed_pool_tags.is_empty()
        && input.fixed_message.is_empty()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "请至少配置一次性卡密池、固定卡密池或固定说明",
        ));
    }
    if !input.pool_tags.is_empty() {
        ensure_card_pools_ready(&state, &input.uid, &input.pool_scope, &input.pool_tags).await?;
    }
    ensure_fixed_card_pools_ready(
        &state,
        &input.uid,
        &input.pool_scope,
        &input.fixed_pool_tags,
    )
    .await?;
    let record = state
        .storage()
        .create_delivery_configuration(&input, &input.pool_tag, &[])
        .await
        .map_err(configuration_save_error)?;
    let configuration = configuration_dto(&state, record).await?;
    Ok(Json(json!({
        "status": "ok",
        "configuration": configuration,
    })))
}

async fn update_configuration(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<i64>,
    Json(mut input): Json<UpdateDeliveryConfigurationRequest>,
) -> ApiResult {
    let current = configuration_record(&state, id).await?;
    ensure_uid_access(&state, &identity, &current.uid).await?;
    ensure_write_access(&state, &identity, &current.uid).await?;
    let cards = normalize_cards(&input.cards);
    input.fixed_message = input.fixed_message.map(|value| value.trim().to_string());
    input.pool_scope = input
        .pool_scope
        .map(|value| value.trim().to_ascii_lowercase());
    input.pool_tag = input.pool_tag.map(|value| value.trim().to_string());
    input.pool_tags = input
        .pool_tags
        .map(|tags| normalize_pool_tags(&tags, input.pool_tag.as_deref().unwrap_or("")));
    input.fixed_pool_tags = input
        .fixed_pool_tags
        .map(|tags| normalize_pool_tags(&tags, ""));
    let product = load_product(&state, &current.uid, &current.item_id)
        .await?
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "配置商品不存在"))?;
    if !product.online || product.quantity.unwrap_or(0) <= 0 {
        let changes_content = input.fixed_message.is_some()
            || input.one_time_enabled.is_some()
            || input.uid.is_some()
            || input.item_id.is_some();
        if input.enabled == Some(true) || changes_content {
            return Err(api_error(
                StatusCode::CONFLICT,
                "已售罄或下架配置只能暂停、删除或补充卡密",
            ));
        }
    }
    let next_scope = input.pool_scope.as_deref().unwrap_or(&current.pool_scope);
    let next_tags = input.pool_tags.as_deref().unwrap_or(&current.pool_tags);
    let next_fixed_tags = input
        .fixed_pool_tags
        .as_deref()
        .unwrap_or(&current.fixed_pool_tags);
    if !next_tags.is_empty() && !next_fixed_tags.is_empty() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "固定卡密池和一次性卡密池不能同时配置",
        ));
    }
    let next_fixed_message = input
        .fixed_message
        .as_deref()
        .unwrap_or(&current.fixed_message);
    input.one_time_enabled = Some(!next_tags.is_empty());
    if next_tags.is_empty() && next_fixed_tags.is_empty() && next_fixed_message.trim().is_empty() {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "请至少配置一次性卡密池、固定卡密池或固定说明",
        ));
    }
    let changes_pool =
        input.pool_scope.is_some() || input.pool_tag.is_some() || input.one_time_enabled.is_some();
    if !next_tags.is_empty() && (changes_pool || input.enabled == Some(true)) {
        ensure_card_pools_ready(&state, &current.uid, next_scope, next_tags).await?;
    }
    if input.fixed_pool_tags.is_some() || input.enabled == Some(true) {
        ensure_fixed_card_pools_ready(&state, &current.uid, next_scope, next_fixed_tags).await?;
    }
    let record = state
        .storage()
        .update_delivery_configuration(id, &input, &cards)
        .await
        .map_err(configuration_save_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "自动发货配置不存在"))?;
    if !cards.is_empty() {
        crate::business_auto::delivery::wake_pool(
            &record.uid,
            &record.pool_scope,
            &record.pool_tag,
        )
        .await;
    }
    Ok(Json(json!({
        "status": "ok",
        "configuration": configuration_dto(&state, record).await?,
    })))
}

async fn delete_configuration(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<i64>,
) -> ApiResult {
    let current = configuration_record(&state, id).await?;
    ensure_uid_access(&state, &identity, &current.uid).await?;
    ensure_write_access(&state, &identity, &current.uid).await?;
    let deleted = state
        .storage()
        .delete_delivery_configuration(&current)
        .await
        .map_err(storage_error)?;
    if !deleted {
        return Err(api_error(StatusCode::NOT_FOUND, "自动发货配置不存在"));
    }
    Ok(Json(json!({ "status": "ok" })))
}

async fn add_configuration_cards(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<i64>,
    Json(input): Json<AddDeliveryCardsRequest>,
) -> ApiResult {
    let current = configuration_record(&state, id).await?;
    ensure_uid_access(&state, &identity, &current.uid).await?;
    ensure_write_access(&state, &identity, &current.uid).await?;
    let cards = normalize_cards(&input.cards);
    if cards.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "没有可导入的一次性卡密"));
    }
    let inserted = state
        .storage()
        .add_delivery_cards(&current, &cards)
        .await
        .map_err(storage_error)?;
    crate::business_auto::delivery::wake_pool(&current.uid, &current.pool_scope, &current.pool_tag)
        .await;
    Ok(Json(json!({
        "status": "ok",
        "insertedCount": inserted,
        "configuration": configuration_dto(&state, current).await?,
    })))
}

async fn list_configuration_cards(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<i64>,
) -> ApiResult {
    let current = configuration_record(&state, id).await?;
    ensure_uid_access(&state, &identity, &current.uid).await?;
    let pool_cookie_id = state
        .storage()
        .resolve_card_pool_cookie_id(&current.uid, &current.pool_scope)
        .await
        .map_err(storage_error)?;
    let cards = state
        .storage()
        .list_cards(&pool_cookie_id)
        .await
        .map_err(storage_error)?
        .into_iter()
        .filter(|card| card.card_pool_tag == current.pool_tag)
        .collect::<Vec<_>>();
    Ok(Json(json!(cards)))
}

async fn refresh_product_stock(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(item_id): Path<String>,
    Query(query): Query<RefreshStockQuery>,
) -> ApiResult {
    ensure_uid_access(&state, &identity, &query.uid).await?;
    ensure_write_access(&state, &identity, &query.uid).await?;
    if load_product(&state, &query.uid, &item_id).await?.is_none() {
        return Err(api_error(StatusCode::NOT_FOUND, "商品不存在"));
    }
    let quantity = refresh_item_quantity_with_edit_fallback(&state, &query.uid, &item_id)
        .await
        .map_err(|error| match error {
            RefreshItemQuantityError::Upstream(message) => {
                tracing::warn!(uid = %query.uid, item_id = %item_id, error = %message, error_code = "delivery_stock_upstream", "自动发货商品库存上游刷新失败");
                api_error(StatusCode::BAD_GATEWAY, "商品库存刷新失败，请稍后重试")
            }
            RefreshItemQuantityError::Persistence(error) => {
                let _ = error;
                tracing::error!(uid = %query.uid, item_id = %item_id, error_code = "delivery_stock_persistence", "自动发货商品库存持久化失败");
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "商品库存同步失败，请稍后重试",
                )
            }
        })?;
    Ok(Json(json!({
        "status": "ok",
        "itemId": item_id,
        "quantity": quantity,
    })))
}

async fn list_runs(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<RunsQuery>,
) -> ApiResult {
    let page = query.page.unwrap_or(1);
    if !(1..=MAX_RUNS_PAGE).contains(&page) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("page 必须在 1 到 {MAX_RUNS_PAGE} 之间"),
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    let uids = if let Some(uid) = query.uid.as_deref().filter(|uid| !uid.trim().is_empty()) {
        ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid)
            .await
            .map_err(response_error)?;
        vec![uid.trim().to_string()]
    } else {
        accessible_account_uids(state.storage().accounts(), owner.as_deref())
            .await
            .map_err(storage_error)?
            .into_iter()
            .collect()
    };
    if let Some(configuration_id) = query.configuration_id {
        let record = configuration_record(&state, configuration_id).await?;
        if !uids.iter().any(|uid| uid == &record.uid) {
            return Err(api_error(StatusCode::NOT_FOUND, "自动发货配置不存在"));
        }
    }
    let runs = state
        .storage()
        .list_delivery_runs_summary(
            &uids,
            query.configuration_id,
            query.status.as_deref(),
            page,
            query.page_size.unwrap_or(50),
        )
        .await
        .map_err(storage_error)?;
    Ok(Json(json!(runs)))
}

async fn retry_run(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(id): Path<i64>,
) -> ApiResult {
    let run = state
        .storage()
        .delivery_run_summary(id)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "自动发货任务不存在"))?;
    ensure_uid_access(&state, &identity, &run.uid).await?;
    ensure_write_access(&state, &identity, &run.uid).await?;
    let run = state
        .storage()
        .retry_delivery_run(id, &run.uid)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| api_error(StatusCode::CONFLICT, "只有失败或需人工处理的任务可以重试"))?;
    Ok(Json(json!({ "status": "ok", "run": run })))
}

async fn list_discovery_alerts(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<DiscoveryAlertsQuery>,
) -> ApiResult {
    let page = query.page.unwrap_or(1);
    if !(1..=MAX_RUNS_PAGE).contains(&page) {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!("page 必须在 1 到 {MAX_RUNS_PAGE} 之间"),
        ));
    }
    let owner = owner_username_filter(&state, &identity);
    let uids = if let Some(uid) = query.uid.as_deref().filter(|uid| !uid.trim().is_empty()) {
        ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid)
            .await
            .map_err(response_error)?;
        vec![uid.trim().to_string()]
    } else {
        accessible_account_uids(state.storage().accounts(), owner.as_deref())
            .await
            .map_err(storage_error)?
            .into_iter()
            .collect()
    };
    let alerts = state
        .storage()
        .list_delivery_discovery_alerts(
            &uids,
            query.status.as_deref(),
            page,
            query.page_size.unwrap_or(50),
        )
        .await
        .map_err(storage_error)?;
    Ok(Json(json!(alerts)))
}

async fn configuration_dto(
    state: &AppState,
    record: DeliveryConfigurationRecord,
) -> Result<DeliveryConfiguration, Response> {
    let product = load_product(state, &record.uid, &record.item_id)
        .await?
        .unwrap_or_else(|| missing_product(&record.item_id));
    let available_card_count = state
        .storage()
        .available_delivery_card_count(&record)
        .await
        .map_err(storage_error)?;
    Ok(configuration_from_parts(
        record,
        product,
        available_card_count,
    ))
}

fn configuration_from_parts(
    record: DeliveryConfigurationRecord,
    product: DeliveryProduct,
    available_card_count: i64,
) -> DeliveryConfiguration {
    DeliveryConfiguration {
        id: record.id,
        uid: record.uid,
        item_id: record.item_id,
        fixed_message: record.fixed_message,
        one_time_enabled: record.one_time_enabled,
        enabled: record.enabled,
        needs_repair: record.needs_repair,
        pool_scope: record.pool_scope,
        pool_tag: record.pool_tag,
        pool_tags: record.pool_tags,
        fixed_pool_tags: record.fixed_pool_tags,
        available_card_count,
        product,
        pending_order_mismatches: Vec::new(),
        created_at: record.created_at,
    }
}

async fn ensure_fixed_card_pools_ready(
    state: &AppState,
    uid: &str,
    pool_scope: &str,
    pool_tags: &[String],
) -> Result<(), Response> {
    for pool_tag in pool_tags {
        let count = state
            .storage()
            .delivery_card_pool_count(uid, pool_scope, pool_tag, false)
            .await
            .map_err(storage_error)?;
        if count == 0 {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("固定卡密池「{pool_tag}」没有可用固定内容"),
            ));
        }
    }
    Ok(())
}

async fn ensure_card_pools_ready(
    state: &AppState,
    uid: &str,
    pool_scope: &str,
    pool_tags: &[String],
) -> Result<(), Response> {
    if !matches!(pool_scope, "account" | "global") {
        return Err(api_error(StatusCode::BAD_REQUEST, "卡密池范围无效"));
    }
    if pool_tags.is_empty() {
        return Err(api_error(StatusCode::BAD_REQUEST, "请选择卡密池"));
    }
    for pool_tag in pool_tags {
        let count = state
            .storage()
            .delivery_card_pool_count(uid, pool_scope, pool_tag, true)
            .await
            .map_err(storage_error)?;
        if count == 0 {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("卡密池「{pool_tag}」没有可用的一次性卡密"),
            ));
        }
    }
    Ok(())
}

fn normalize_pool_tags(pool_tags: &[String], legacy_pool_tag: &str) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in pool_tags
        .iter()
        .map(|tag| tag.trim())
        .chain(std::iter::once(legacy_pool_tag.trim()))
    {
        if !tag.is_empty() && !normalized.iter().any(|existing| existing == tag) {
            normalized.push(tag.to_string());
        }
    }
    normalized
}

fn missing_product(item_id: &str) -> DeliveryProduct {
    DeliveryProduct {
        item_id: item_id.to_string(),
        image: String::new(),
        title: String::new(),
        price: String::new(),
        quantity: None,
        online: false,
    }
}

async fn load_products(
    state: &AppState,
    uid: &str,
    item_ids: &[String],
) -> Result<HashMap<String, DeliveryProduct>, Response> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let mut builder = QueryBuilder::<Sqlite>::new(
        "SELECT item_id, pic_url, title, price, quantity, online FROM account_items WHERE uid = ",
    );
    builder.push_bind(uid.trim()).push(" AND item_id IN (");
    let mut separated = builder.separated(", ");
    for item_id in item_ids {
        separated.push_bind(item_id);
    }
    separated.push_unseparated(")");
    let rows = builder
        .build()
        .fetch_all(state.storage().xianyu())
        .await
        .map_err(storage_error)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let product = DeliveryProduct {
                item_id: row.get("item_id"),
                image: row.get("pic_url"),
                title: row.get("title"),
                price: row.get("price"),
                quantity: row.get("quantity"),
                online: row.get::<i64, _>("online") != 0,
            };
            (product.item_id.clone(), product)
        })
        .collect())
}

async fn load_product(
    state: &AppState,
    uid: &str,
    item_id: &str,
) -> Result<Option<DeliveryProduct>, Response> {
    let row = sqlx::query(
        r#"
        SELECT item_id, pic_url, title, price, quantity, online
        FROM account_items WHERE uid = ? AND item_id = ? LIMIT 1
        "#,
    )
    .bind(uid.trim())
    .bind(item_id.trim())
    .fetch_optional(state.storage().xianyu())
    .await
    .map_err(storage_error)?;
    Ok(row.map(|row| DeliveryProduct {
        item_id: row.get("item_id"),
        image: row.get("pic_url"),
        title: row.get("title"),
        price: row.get("price"),
        quantity: row.get("quantity"),
        online: row.get::<i64, _>("online") != 0,
    }))
}

async fn configuration_record(
    state: &AppState,
    id: i64,
) -> Result<DeliveryConfigurationRecord, Response> {
    state
        .storage()
        .delivery_configuration_record(id)
        .await
        .map_err(storage_error)?
        .ok_or_else(|| api_error(StatusCode::NOT_FOUND, "自动发货配置不存在"))
}

async fn ensure_uid_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), Response> {
    let owner = owner_username_filter(state, identity);
    ensure_account_uid_accessible(state.storage().accounts(), owner.as_deref(), uid)
        .await
        .map_err(response_error)
}

async fn ensure_write_access(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), Response> {
    let owner = owner_username_filter(state, identity);
    ensure_account_can_call_upstream(state, owner.as_deref(), uid)
        .await
        .map_err(response_error)
}

fn normalize_cards(cards: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for line in cards.iter().flat_map(|value| value.lines()) {
        let content = line.trim();
        if !content.is_empty() && seen.insert(content.to_string()) {
            normalized.push(content.to_string());
        }
    }
    normalized
}

fn storage_error(error: impl Into<StorageFailure>) -> Response {
    safe_storage_error(error.into(), "自动发货服务暂时不可用，请稍后重试")
}

fn configuration_save_error(error: StorageError) -> Response {
    safe_storage_error(
        StorageFailure::Storage(error),
        "自动发货配置保存失败，请稍后重试",
    )
}

fn safe_storage_error(error: StorageFailure, public_message: &str) -> Response {
    match error {
        StorageFailure::Storage(StorageError::Sqlx(sqlx::Error::Database(database)))
            if database
                .message()
                .contains("duplicate delivery configuration") =>
        {
            api_error(StatusCode::CONFLICT, "同一账号和商品只能有一条自动发货配置")
        }
        StorageFailure::Storage(StorageError::Conflict { message })
            if matches!(
                message.as_str(),
                "自动发货配置不能更换账号或商品"
                    | "同一账号和商品只能有一条自动发货配置"
                    | "该全局一次性卡密池已被其他账号绑定，请选择其他卡密池"
                    | "固定卡密池和一次性卡密池不能同时配置"
            ) =>
        {
            api_error(StatusCode::CONFLICT, message)
        }
        StorageFailure::Storage(StorageError::Conflict { message })
            if matches!(
                message.as_str(),
                "所选卡池没有可用的一次性卡密"
                    | "所选卡池没有固定卡密"
                    | "卡密池范围无效"
                    | "请至少配置一次性卡密池、固定卡密池或固定说明"
            ) =>
        {
            api_error(StatusCode::BAD_REQUEST, message)
        }
        StorageFailure::Storage(StorageError::Conflict { message })
            if message == "该任务发送结果不确定，必须先人工确认，不能直接重试" =>
        {
            api_error(StatusCode::CONFLICT, message)
        }
        StorageFailure::Storage(StorageError::BusyExhausted { .. }) => api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "数据正在被其他任务处理，请稍后重试",
        ),
        StorageFailure::Storage(error) => {
            let _ = error;
            tracing::error!(error_code = "delivery_storage", "自动发货存储操作失败");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, public_message)
        }
        StorageFailure::Sqlx(error) => {
            let _ = error;
            tracing::error!(error_code = "delivery_database", "自动发货数据库操作失败");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, public_message)
        }
    }
}

enum StorageFailure {
    Storage(StorageError),
    Sqlx(sqlx::Error),
}

impl From<StorageError> for StorageFailure {
    fn from(value: StorageError) -> Self {
        Self::Storage(value)
    }
}

impl From<sqlx::Error> for StorageFailure {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

fn response_error(error: ResponseError) -> Response {
    let status = error.status();
    if let Some(detail) = error.public_detail() {
        return api_error(status, detail);
    }
    tracing::warn!(
        status = status.as_u16(),
        error_code = "delivery_access",
        "自动发货账号访问校验失败"
    );
    api_error(status, "自动发货服务暂时不可用，请稍后重试")
}

fn api_error(status: StatusCode, detail: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({
            "status": "error",
            "detail": detail.to_string(),
        })),
    )
        .into_response()
}
