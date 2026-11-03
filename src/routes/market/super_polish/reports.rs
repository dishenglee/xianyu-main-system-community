use axum::{
    extract::{Extension, State},
    routing::post,
    Json, Json as ResponseJson, Router,
};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::common::{
    authorize_uid, request_fishad_h5, request_fishad_h5_with_params, request_json_from_params,
    ParamsPayload,
};

#[allow(non_snake_case)]
fn ACCOUNT_REPORT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.member.data.query")
}
#[allow(non_snake_case)]
fn GROUP_REPORT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.group.data.query")
}
#[allow(non_snake_case)]
fn ITEM_REPORT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.item.data.batch.query")
}
#[allow(non_snake_case)]
fn CAMPAIGN_REPORT_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.campaign.data.batch.query")
}
// The analyzer captured `spm_cnt` on campaign/item report requests, but not the
// exact value. Reuse a stable browser-style token shape so the backend request
// profile stays closer to the captured report surface.
const REPORT_SPM_CNT: &str = "a21ybx.im.0.0";

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/reports/account"),
            post(account),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/reports/group"),
            post(group),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/reports/item"),
            post(item),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/reports/campaign"),
            post(campaign),
        )
}

pub(super) async fn account(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        ACCOUNT_REPORT_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn group(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &payload.uid,
        GROUP_REPORT_API(),
        "1.0",
        request_json_from_params(payload.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn item(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5_with_params(
        &state,
        &payload.uid,
        ITEM_REPORT_API(),
        "1.0",
        request_json_from_params(payload.params),
        vec![("spm_cnt".to_string(), REPORT_SPM_CNT.to_string())],
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn campaign(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ParamsPayload>,
) -> ApiResult {
    authorize_uid(&state, &identity, &payload.uid).await?;
    let upstream = request_fishad_h5_with_params(
        &state,
        &payload.uid,
        CAMPAIGN_REPORT_API(),
        "1.0",
        request_json_from_params(payload.params),
        vec![
            ("needLoginPC".to_string(), "true".to_string()),
            ("spm_cnt".to_string(), REPORT_SPM_CNT.to_string()),
        ],
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}
