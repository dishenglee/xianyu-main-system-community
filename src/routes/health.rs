use axum::{extract::State, response::Json, Extension};
use serde_json::{json, Value};

use crate::{
    models::{auth::RequestIdentity, runtime::HealthStatus},
    routes::{error::ResponseError, tenant::ensure_admin_or_legacy},
    state::AppState,
};

pub(crate) async fn health(State(state): State<AppState>) -> Json<HealthStatus> {
    Json(
        state
            .runtime()
            .health_status(state.config().service_name.clone()),
    )
}

/// MTOP upstream health dashboard. Returns per-API success/error breakdown
/// plus a recent-anomaly ring so we can spot "Taobao changed something" in
/// near real-time without combing through logs.
pub(crate) async fn mtop_health(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> Result<Json<Value>, ResponseError> {
    ensure_admin_or_legacy(&state, &identity)?;
    let metrics = state.mtop_metrics().snapshot();
    let (buffered, capacity, subscribers) = state.live_events().pressure();
    Ok(Json(json!({
        "mtop": metrics,
        "liveEvents": {
            "buffered": buffered,
            "capacity": capacity,
            "subscribers": subscribers,
            "pressurePct": if capacity == 0 { 0 } else { buffered * 100 / capacity },
        },
    })))
}
