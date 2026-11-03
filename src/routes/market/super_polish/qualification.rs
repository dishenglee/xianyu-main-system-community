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
    authorize_uid, empty_object, request_fishad_h5, request_json_from_params, ParamsQuery, UidQuery,
};

fn qualification_categories_api() -> &'static str {
    crate::sealed_protocol::sealed_str!(
        "mtop.taobao.idle.ad.qualification.allowAuditCategories.query"
    )
}
fn qualification_records_api() -> &'static str {
    crate::sealed_protocol::sealed_str!(
        "mtop.taobao.idle.ad.qualification.submittedRecords.batchQuery"
    )
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/qualification/categories"),
            get(categories),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/qualification/records"),
            get(records),
        )
}

pub(super) async fn categories(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<UidQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        qualification_categories_api(),
        "1.0",
        empty_object(),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}

pub(super) async fn records(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Query(query): Query<ParamsQuery>,
) -> ApiResult {
    authorize_uid(&state, &identity, &query.uid).await?;
    let upstream = request_fishad_h5(
        &state,
        &query.uid,
        qualification_records_api(),
        "1.0",
        request_json_from_params(query.params),
    )
    .await
    .map_err(ResponseError::service_unavailable)?;
    Ok(ResponseJson(upstream))
}
