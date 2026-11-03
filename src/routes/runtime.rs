use axum::{extract::State, response::Json, Extension};
use serde_json::{json, Value};

use crate::{
    models::{auth::RequestIdentity, runtime::RuntimeInspection},
    routes::{error::ResponseError, tenant::ensure_admin_or_legacy},
    state::AppState,
};

pub(crate) async fn runtime(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<RuntimeInspection>, ResponseError> {
    ensure_admin_or_legacy(&state, &identity)?;
    Ok(Json(
        state
            .runtime()
            .inspection(state.config().service_name.clone()),
    ))
}

pub(crate) async fn runtime_profile(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<Value>, ResponseError> {
    ensure_admin_or_legacy(&state, &identity)?;
    Ok(Json(json!({
        "service": state.config().service_name,
        "version": env!("CARGO_PKG_VERSION"),
        "state_backend": "sqlite",
        "transport_mode": "ws",
    })))
}
