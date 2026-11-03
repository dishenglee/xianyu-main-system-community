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

use super::common::{authorize_uid, empty_object, request_fishad_h5, UidQuery};

#[allow(non_snake_case)]
fn BOOTSTRAP_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.init.user.site.info")
}
#[allow(non_snake_case)]
fn HOME_API() -> &'static str {
    crate::sealed_protocol::sealed_str!("mtop.taobao.idle.ad.home")
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/bootstrap"),
            get(bootstrap),
        )
        .route(crate::sealed_protocol::sealed_str!("/home"), get(home))
}

pub(super) async fn bootstrap(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(&state, &query.uid, BOOTSTRAP_API(), "1.0", empty_object())
        .await
        .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn home(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(&state, &query.uid, HOME_API(), "1.0", empty_object())
        .await
        .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}
