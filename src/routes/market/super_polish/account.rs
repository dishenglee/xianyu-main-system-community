use axum::{
    extract::{Extension, Query, State},
    routing::get,
    Json as ResponseJson, Router,
};

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    state::AppState,
};

use super::common::{
    authorize_uid, empty_object, request_fishad_h5, request_fishad_h5_with_params,
    request_json_from_params, ParamsQuery, UidQuery,
};

#[allow(non_snake_case)]
fn ACCOUNT_INFO_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.account.info")
}
#[allow(non_snake_case)]
fn LOGIN_INFO_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.get.login.info.v2")
}
#[allow(non_snake_case)]
fn ACCOUNT_CARD_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.bp.account.card.query")
}
#[allow(non_snake_case)]
fn UNREAD_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.message.unread.info")
}
#[allow(non_snake_case)]
fn MEMBER_DATA_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.member.data.query")
}
#[allow(non_snake_case)]
fn ACTIVITY_REWARD_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.activity.detail.rendering")
}
#[allow(non_snake_case)]
fn COUPONS_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.ad.bp.coupon.info.query")
}
#[allow(non_snake_case)]
fn COUPON_STATISTICS_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.alibaba.idle.ad.bp.coupon.statistic.query")
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/account/info"),
            get(info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/login-info"),
            get(login_info),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/card"),
            get(card),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/messages/unread"),
            get(unread),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/member-data"),
            get(member_data),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/activity-reward"),
            get(activity_reward),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/coupons"),
            get(coupons),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/account/coupon-statistics"),
            get(coupon_statistics),
        )
}

pub(super) async fn info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        ACCOUNT_INFO_API(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn login_info(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(&state, &query.uid, LOGIN_INFO_API(), "1.0", empty_object())
        .await
        .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn card(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let data = if query.params.is_empty() {
        request_json_from_params(serde_json::Map::from_iter([(
            "scene".to_string(),
            serde_json::Value::String("PC_HOME".to_string()),
        )]))
    } else {
        request_json_from_params(query.params)
    };
    let upstream = request_fishad_h5_with_params(
        &state,
        &query.uid,
        ACCOUNT_CARD_API(),
        "1.0",
        data,
        vec![("needLoginPC".to_string(), "true".to_string())],
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn unread(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(&state, &query.uid, UNREAD_API(), "1.0", empty_object())
        .await
        .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn member_data(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        MEMBER_DATA_API(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn activity_reward(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        ACTIVITY_REWARD_API(),
        "1.0",
        empty_object(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn coupons(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        COUPONS_API(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn coupon_statistics(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        COUPON_STATISTICS_API(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}
