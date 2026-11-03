use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{
    models::auth::RequestIdentity,
    routes::error::ResponseError,
    state::AppState,
    xianyu_client::{self, H5RequestProfile},
};

use super::super::ensure_market_uid_access;

pub(crate) const LOCAL_CONFIRM_REQUIRED_RET: &str =
    "FAIL_LOCAL_CONFIRM_REQUIRED::该操作会改变账号状态或扣除闲鱼币，需要确认";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UidQuery {
    pub(crate) uid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsQuery {
    pub(crate) uid: String,
    #[serde(flatten)]
    pub(crate) params: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ParamsPayload {
    pub(crate) uid: String,
    #[serde(flatten)]
    pub(crate) params: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmParamsPayload {
    pub(crate) uid: String,
    #[serde(default)]
    pub(crate) confirm: bool,
    #[serde(flatten)]
    pub(crate) params: Map<String, Value>,
}

pub(crate) async fn authorize_uid(
    state: &AppState,
    identity: &RequestIdentity,
    uid: &str,
) -> Result<(), ResponseError> {
    ensure_market_uid_access(state, identity, uid).await
}

pub(crate) fn empty_object() -> Value {
    Value::Object(Map::new())
}

pub(crate) fn wrap_request_json(value: Value) -> Value {
    json!({
        "requestJson": serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
    })
}

pub(crate) fn request_json_from_params(mut params: Map<String, Value>) -> Value {
    if let Some(value) = params.remove("requestJson") {
        let mut request = Map::new();
        request.insert("requestJson".to_string(), value);
        return Value::Object(request);
    }
    if params.len() == 1 && params.contains_key("payload") {
        let payload = params.remove("payload").unwrap_or_else(empty_object);
        return wrap_request_json(payload);
    }
    wrap_request_json(Value::Object(params))
}

pub(crate) fn confirm_required() -> Value {
    json!({
        "ret": [LOCAL_CONFIRM_REQUIRED_RET],
        "data": { "success": false }
    })
}

pub(crate) async fn request_fishad_h5(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
) -> Result<Value, String> {
    request_fishad_h5_with_params(state, uid, api, version, data, vec![]).await
}

pub(crate) async fn request_fishad_h5_with_params(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    xianyu_client::h5_request_with_profile(
        state.config(),
        state.storage().accounts(),
        uid,
        api,
        version,
        &data,
        extra_params,
        H5RequestProfile::FishAd,
    )
    .await
}
