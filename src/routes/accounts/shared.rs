//! Shared utilities for the `accounts` module family.
//!
//! Split out of the original `accounts.rs` monolith as part of the module
//! refactor: parseable/formattable helpers, serde defaults, the in-process
//! QR session registry live here so the rest of the submodules (summary, qr,
//! messages, live, wx_live) can share them without circular references.

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Mutex, OnceLock},
};

use serde_json::{Map, Value};

use crate::{
    cookie_refresh::{self, BusinessTransport},
    mtop_errors::classify_mtop_payload,
    protocol_runtime::resolve_protocol,
    state::AppState,
    xianyu_client,
};

pub(crate) async fn resolve_account_protocol(
    state: &AppState,
    opcode: u32,
) -> Result<crate::protocol_catalog::ProtocolEntry, String> {
    resolve_protocol(state, opcode).await
}

pub(crate) use crate::routes::error::{ApiResult, ResponseError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportKind {
    H5,
    Wx,
    App,
}

impl TransportKind {
    pub(crate) const fn tag(self) -> &'static str {
        match self {
            Self::H5 => "h5",
            Self::Wx => "wx",
            Self::App => "app",
        }
    }

    const fn business_transport(self) -> BusinessTransport {
        match self {
            Self::H5 => BusinessTransport::H5,
            Self::Wx => BusinessTransport::Wx,
            Self::App => BusinessTransport::App,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportSelector {
    Auto,
    H5,
    Wx,
    App,
}

impl TransportSelector {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "" | "auto" => Some(Self::Auto),
            "pc" | "h5" => Some(Self::H5),
            "wx" => Some(Self::Wx),
            "app" => Some(Self::App),
            _ => None,
        }
    }

    pub(crate) const fn fallback_chain(self) -> &'static [TransportKind] {
        match self {
            Self::Auto => &[TransportKind::App, TransportKind::H5, TransportKind::Wx],
            Self::H5 => &[TransportKind::H5],
            Self::Wx => &[TransportKind::Wx],
            Self::App => &[TransportKind::App],
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TransportRequest {
    pub(crate) transport: TransportKind,
    pub(crate) api: String,
    pub(crate) version: String,
    pub(crate) data: Value,
    pub(crate) extra_params: Vec<(String, String)>,
}

impl TransportRequest {
    pub(crate) fn new(
        transport: TransportKind,
        api: impl Into<String>,
        version: impl Into<String>,
        data: Value,
    ) -> Self {
        Self {
            transport,
            api: api.into(),
            version: version.into(),
            data,
            extra_params: Vec::new(),
        }
    }

    pub(crate) fn with_extra_params(mut self, extra_params: Vec<(String, String)>) -> Self {
        self.extra_params = extra_params;
        self
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TransportResponse {
    pub(crate) transport: TransportKind,
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TransportRequestOptions {
    pub(crate) record_response: bool,
    pub(crate) require_success: bool,
}

async fn send_transport_request_once(
    state: &AppState,
    uid: &str,
    request: &TransportRequest,
    transport: TransportKind,
) -> Result<Value, String> {
    super::runtime_access::ensure_account_can_call_upstream(state, None, uid)
        .await
        .map_err(|error| error.to_string())?;
    match transport {
        TransportKind::H5 => {
            xianyu_client::h5_request(
                state.config(),
                state.storage().accounts(),
                uid,
                &request.api,
                &request.version,
                &request.data,
                request.extra_params.clone(),
            )
            .await
        }
        TransportKind::Wx => {
            xianyu_client::wx_request(
                state.config(),
                state.storage().accounts(),
                uid,
                &request.api,
                &request.version,
                &request.data,
            )
            .await
        }
        TransportKind::App => {
            xianyu_client::app_request(
                state.config(),
                state.storage().accounts(),
                uid,
                &request.api,
                &request.version,
                &request.data,
            )
            .await
        }
    }
}

pub(crate) async fn request_transport(
    state: &AppState,
    uid: &str,
    request: &TransportRequest,
    options: TransportRequestOptions,
) -> Result<TransportResponse, String> {
    let transport = if request.transport == TransportKind::App
        && crate::license_runtime::get_sign_license_context().is_none()
    {
        tracing::debug!(
            uid = %uid,
            api = %request.api,
            "App signing context unavailable; trying the equivalent H5 transport"
        );
        TransportKind::H5
    } else {
        request.transport
    };
    let first = send_transport_request_once(state, uid, request, transport).await?;
    let payload = if classify_mtop_payload(&first).is_auth_recoverable()
        && cookie_refresh::try_repair_business_session(
            state.config(),
            state.storage().accounts(),
            uid,
            &request.api,
            transport.business_transport(),
        )
        .await
    {
        send_transport_request_once(state, uid, request, transport).await?
    } else {
        first
    };
    if options.record_response {
        state.record_mtop_response(&request.api, &payload);
    }
    if options.require_success && !payload_ret_success(&payload) {
        return Err(format!(
            "non-success upstream ret: {}",
            first_ret_string(&payload)
        ));
    }
    Ok(TransportResponse { transport, payload })
}

pub(crate) fn first_ret_string(payload: &Value) -> String {
    payload
        .get("ret")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .or_else(|| payload.get("ret").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn payload_ret_success(payload: &Value) -> bool {
    first_ret_string(payload)
        .to_ascii_uppercase()
        .contains("SUCCESS")
}

pub(crate) async fn upstream_h5_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    request_transport(
        state,
        uid,
        &TransportRequest::new(TransportKind::H5, api, version, data)
            .with_extra_params(extra_params),
        TransportRequestOptions::default(),
    )
    .await
    .map(|response| response.payload)
}

pub(crate) async fn successful_upstream_h5_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    request_transport(
        state,
        uid,
        &TransportRequest::new(TransportKind::H5, api, version, data)
            .with_extra_params(extra_params),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await
    .map(|response| response.payload)
}

pub(crate) async fn upstream_wx_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
) -> Result<Value, String> {
    request_transport(
        state,
        uid,
        &TransportRequest::new(TransportKind::Wx, api, version, data),
        TransportRequestOptions::default(),
    )
    .await
    .map(|response| response.payload)
}

pub(crate) async fn successful_upstream_wx_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
) -> Result<Value, String> {
    request_transport(
        state,
        uid,
        &TransportRequest::new(TransportKind::Wx, api, version, data),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await
    .map(|response| response.payload)
}

pub(crate) async fn upstream_app_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
) -> Result<Value, String> {
    request_transport(
        state,
        uid,
        &TransportRequest::new(TransportKind::App, api, version, data),
        TransportRequestOptions::default(),
    )
    .await
    .map(|response| response.payload)
}

pub(crate) async fn successful_upstream_app_request(
    state: &AppState,
    uid: &str,
    api: &str,
    version: &str,
    data: Value,
) -> Result<Value, String> {
    request_transport(
        state,
        uid,
        &TransportRequest::new(TransportKind::App, api, version, data),
        TransportRequestOptions {
            record_response: false,
            require_success: true,
        },
    )
    .await
    .map(|response| response.payload)
}

pub(crate) async fn upstream_protocol_h5_request(
    state: &AppState,
    uid: &str,
    opcode: u32,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    let entry = resolve_account_protocol(state, opcode).await?;
    upstream_h5_request(state, uid, entry.api(), entry.version(), data, extra_params).await
}

pub(crate) async fn successful_upstream_protocol_h5_request(
    state: &AppState,
    uid: &str,
    opcode: u32,
    data: Value,
    extra_params: Vec<(String, String)>,
) -> Result<Value, String> {
    let entry = resolve_account_protocol(state, opcode).await?;
    successful_upstream_h5_request(state, uid, entry.api(), entry.version(), data, extra_params)
        .await
}

pub(crate) async fn successful_upstream_protocol_wx_request(
    state: &AppState,
    uid: &str,
    opcode: u32,
    data: Value,
) -> Result<Value, String> {
    let entry = resolve_account_protocol(state, opcode).await?;
    successful_upstream_wx_request(state, uid, entry.api(), entry.version(), data).await
}

pub(crate) async fn successful_upstream_protocol_app_request(
    state: &AppState,
    uid: &str,
    opcode: u32,
    data: Value,
) -> Result<Value, String> {
    let entry = resolve_account_protocol(state, opcode).await?;
    successful_upstream_app_request(state, uid, entry.api(), entry.version(), data).await
}

pub(super) fn parse_json_object(raw: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

pub(super) fn parse_json_array(raw: &str) -> Vec<Value> {
    serde_json::from_str::<Vec<Value>>(raw).unwrap_or_default()
}

pub(super) fn unix_seconds_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs_f64()
}

pub(super) fn unix_millis_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis() as i64
}

pub(super) fn default_page() -> i64 {
    1
}

pub(super) fn default_page_size_20() -> i64 {
    20
}

pub(super) fn default_page_size_30() -> i64 {
    30
}

pub(super) fn default_page_size_50() -> i64 {
    50
}

pub(super) fn default_recent_30d() -> String {
    "recent30d".to_string()
}

pub(super) fn default_session_type() -> i64 {
    1
}

pub(super) fn default_session_types() -> String {
    "[1,3,6]".to_string()
}

#[derive(Debug, Clone)]
pub(super) struct QrSessionState {
    pub status: String,
    pub message: String,
    pub owner_username: String,
    pub license_auth_token: String,
    pub owner_conflict: bool,
    pub created_at: f64,
    pub account_uid: String,
    pub account_nick: String,
    pub qr_code_url: String,
    pub qr_content: String,
    pub cookies: BTreeMap<String, String>,
    pub params: HashMap<String, String>,
    pub verification_url: String,
    pub imported: bool,
}

pub(super) fn qr_sessions() -> &'static Mutex<HashMap<String, QrSessionState>> {
    static STORE: OnceLock<Mutex<HashMap<String, QrSessionState>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::{first_ret_string, payload_ret_success, TransportKind, TransportSelector};
    use serde_json::json;

    #[test]
    fn transport_selector_accepts_expected_aliases() {
        assert_eq!(TransportSelector::parse(""), Some(TransportSelector::Auto));
        assert_eq!(
            TransportSelector::parse("auto"),
            Some(TransportSelector::Auto)
        );
        assert_eq!(TransportSelector::parse("pc"), Some(TransportSelector::H5));
        assert_eq!(TransportSelector::parse("h5"), Some(TransportSelector::H5));
        assert_eq!(TransportSelector::parse("wx"), Some(TransportSelector::Wx));
        assert_eq!(
            TransportSelector::parse("app"),
            Some(TransportSelector::App)
        );
        assert_eq!(TransportSelector::parse("unknown"), None);
    }

    #[test]
    fn transport_selector_prefers_app_then_h5_then_wx_for_auto() {
        assert_eq!(
            TransportSelector::Auto.fallback_chain(),
            &[TransportKind::App, TransportKind::H5, TransportKind::Wx]
        );
        assert_eq!(TransportSelector::H5.fallback_chain(), &[TransportKind::H5]);
    }

    #[test]
    fn ret_helpers_support_array_and_scalar_shapes() {
        assert_eq!(
            first_ret_string(&json!({"ret": ["SUCCESS::调用成功"]})),
            "SUCCESS::调用成功"
        );
        assert_eq!(
            first_ret_string(&json!({"ret": "FAIL_SYS_TOKEN_EXPIRED"})),
            "FAIL_SYS_TOKEN_EXPIRED"
        );
        assert!(payload_ret_success(&json!({"ret": ["SUCCESS::调用成功"]})));
        assert!(!payload_ret_success(
            &json!({"ret": "FAIL_SYS_TOKEN_EXPIRED"})
        ));
    }
}
