use axum::{
    extract::{Extension, Path, Query, State},
    routing::{get, post},
    Json, Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::common::{
    authorize_uid, confirm_required, empty_object, request_fishad_h5, request_json_from_params,
    ConfirmParamsPayload, ParamsPayload, ParamsQuery, UidQuery,
};

#[allow(non_snake_case)]
fn CAMPAIGN_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.campaign.search")
}
#[allow(non_snake_case)]
fn PRECHECK_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.campaign.start.before")
}
#[allow(non_snake_case)]
fn ITEM_RECOMMEND_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.item.recommend")
}
#[allow(non_snake_case)]
fn ITEM_SEARCH_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.item.searchV2")
}
#[allow(non_snake_case)]
fn PACKAGE_INFO_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.package.info")
}
#[allow(non_snake_case)]
fn BID_ESTIMATES_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.customize.bid.estimates")
}
#[allow(non_snake_case)]
fn CREATE_CAMPAIGN_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.campaign.startV2")
}
#[allow(non_snake_case)]
fn GROUP_LIST_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.group.list")
}
#[allow(non_snake_case)]
fn EDIT_CAMPAIGN_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.campaign.edit")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CampaignPath {
    campaign_id: String,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns"),
            get(list).post(create),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/precheck"),
            post(precheck),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/items/recommend"),
            get(recommend),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/items/search"),
            post(search_items),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/packages/query"),
            post(query_packages),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/bid-estimates"),
            post(bid_estimates),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/:campaignId/groups"),
            get(groups),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/campaigns/:campaignId/edit"),
            post(edit),
        )
}

pub(super) async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        CAMPAIGN_LIST_API(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn precheck(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5(&state, &payload.uid, PRECHECK_API(), "1.0", empty_object())
        .await
        .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn recommend(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        ITEM_RECOMMEND_API(),
        "1.0",
        empty_object(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn search_items(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        ITEM_SEARCH_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn query_packages(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        PACKAGE_INFO_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn bid_estimates(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        BID_ESTIMATES_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ConfirmParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(ResponseJson(confirm_required()));
    }
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        CREATE_CAMPAIGN_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn groups(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(path): Path<CampaignPath>,
    Query(mut query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    query
        .params
        .entry("campaignId".to_string())
        .or_insert(Value::String(path.campaign_id));
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        GROUP_LIST_API(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn edit(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Path(path): Path<CampaignPath>,
    Json(mut payload): Json<ConfirmParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    if !payload.confirm {
        return Ok(ResponseJson(confirm_required()));
    }
    payload
        .params
        .entry("campaignId".to_string())
        .or_insert(Value::String(path.campaign_id));
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        EDIT_CAMPAIGN_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::internal_message)?;
    Ok(ResponseJson(upstream))
}
