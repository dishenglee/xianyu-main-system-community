use crate::{state::AppState, routes::error::ResponseError};

#[derive(Debug)]
pub(crate) enum ClaimBindingError {
    OwnerConflict(String),
    Other(ResponseError),
}

#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct ResolvedBinding {
    pub(crate) account_uid: String,
    pub(crate) status: String,
    #[serde(default)]
    pub(crate) owner_username: Option<String>,
    #[serde(default)]
    pub(crate) message: String,
}

pub(super) async fn ensure_license_service_healthy(_state: &AppState) -> Result<(), ResponseError> {
    Ok(())
}

pub(crate) async fn claim_binding(
    _state: &AppState,
    _auth_token: &str,
    _account_uid: &str,
    _account_nick: &str,
) -> Result<(), ClaimBindingError> {
    Ok(())
}

pub(super) async fn release_binding(
    _state: &AppState,
    _auth_token: &str,
    _account_uid: &str,
    _reason: &str,
) -> Result<(), ResponseError> {
    Ok(())
}

pub(crate) async fn resolve_bindings(
    _state: &AppState,
    _auth_token: &str,
    _account_uids: &[String],
) -> Result<Vec<ResolvedBinding>, ResponseError> {
    Ok(Vec::new())
}
