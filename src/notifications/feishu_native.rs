use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    io::Write,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use prost::Message;
use reqwest::multipart::{Form, Part};
use reqwest::{Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::{
    sync::{oneshot, watch, Mutex},
    task::JoinHandle,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use url::Url;

use super::{feishu_events::FeishuImEvent, sanitize::safe_worker_error_message};

pub const FEISHU_REGISTRATION_DOMAIN: &str = "feishu";
const LARK_REGISTRATION_DOMAIN: &str = "lark";
const FEISHU_ACCOUNTS_HOST: &str = "accounts.feishu.cn";
const LARK_ACCOUNTS_HOST: &str = "accounts.larksuite.com";
const DEFAULT_POLL_INTERVAL_MS: i64 = 2_000;
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_PING_INTERVAL: Duration = Duration::from_secs(120);
const SDK_COMPAT_USER_AGENT: &str = "yuguanjia-feishu-native/1.0";

pub type FeishuNativeCallFuture =
    Pin<Box<dyn Future<Output = Result<FeishuNativeResponse, FeishuNativeError>> + Send>>;
pub type FeishuNativeEventFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
pub type FeishuNativeEventHandler =
    Arc<dyn Fn(FeishuImEvent) -> FeishuNativeEventFuture + Send + Sync + 'static>;

pub trait FeishuNativeTransport: Send + Sync {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture;
}

#[derive(Clone)]
pub struct FeishuNativeRuntime {
    transport: Arc<dyn FeishuNativeTransport>,
}

impl std::fmt::Debug for FeishuNativeRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeishuNativeRuntime")
            .finish_non_exhaustive()
    }
}

impl Default for FeishuNativeRuntime {
    fn default() -> Self {
        Self::native(None)
    }
}

impl FeishuNativeRuntime {
    pub fn native(event_handler: Option<FeishuNativeEventHandler>) -> Self {
        Self::with_transport(Arc::new(ReqwestFeishuNativeTransport::new(event_handler)))
    }

    pub fn with_transport(transport: Arc<dyn FeishuNativeTransport>) -> Self {
        Self { transport }
    }

    pub async fn register_start(
        &self,
        request: RegisterStartRequest,
    ) -> Result<RegisterStartResponse, FeishuNativeError> {
        self.call("register.start", request).await
    }

    pub async fn register_poll(
        &self,
        request: RegisterPollRequest,
    ) -> Result<RegisterPollResponse, FeishuNativeError> {
        self.call("register.poll", request).await
    }

    pub async fn channel_start(
        &self,
        request: ChannelStartRequest,
    ) -> Result<ChannelStartResponse, FeishuNativeError> {
        self.call("channel.start", request).await
    }

    pub async fn channel_stop(
        &self,
        request: ChannelStopRequest,
    ) -> Result<ChannelStopResponse, FeishuNativeError> {
        self.call("channel.stop", request).await
    }

    pub async fn message_send(
        &self,
        request: MessageSendRequest,
    ) -> Result<MessageSendResponse, FeishuNativeError> {
        self.call("message.send", request).await
    }

    pub async fn chat_create(
        &self,
        request: ChatCreateRequest,
    ) -> Result<ChatCreateResponse, FeishuNativeError> {
        self.call("chat.create", request).await
    }

    pub async fn chat_members_add(
        &self,
        request: ChatMembersAddRequest,
    ) -> Result<ChatMembersAddResponse, FeishuNativeError> {
        self.call("chat.members.add", request).await
    }

    pub async fn chat_patch(
        &self,
        request: ChatPatchRequest,
    ) -> Result<ChatPatchResponse, FeishuNativeError> {
        self.call("chat.patch", request).await
    }

    pub async fn runtime_health(&self) -> Result<RuntimeHealthResponse, FeishuNativeError> {
        self.call("runtime.health", json!({})).await
    }

    pub async fn app_scope_status(
        &self,
        request: AppScopeStatusRequest,
    ) -> Result<AppScopeStatusResponse, FeishuNativeError> {
        self.call("app.scope.status", request).await
    }

    pub async fn app_scope_apply(
        &self,
        request: AppScopeApplyRequest,
    ) -> Result<AppScopeApplyResponse, FeishuNativeError> {
        self.call("app.scope.apply", request).await
    }

    async fn call<TParams, TResult>(
        &self,
        method: &str,
        params: TParams,
    ) -> Result<TResult, FeishuNativeError>
    where
        TParams: Serialize,
        TResult: DeserializeOwned,
    {
        let request = FeishuNativeRequest {
            method: method.to_string(),
            params: serde_json::to_value(params)?,
        };
        let response = self.transport.call(request).await?;
        serde_json::from_value(response.result).map_err(FeishuNativeError::from)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeishuNativeRequest {
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeishuNativeResponse {
    pub result: Value,
}

impl FeishuNativeResponse {
    pub fn success(result: Value) -> Self {
        Self { result }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterStartRequest {
    pub domain: String,
}

impl Default for RegisterStartRequest {
    fn default() -> Self {
        Self {
            domain: FEISHU_REGISTRATION_DOMAIN.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterStartResponse {
    pub device_code: String,
    pub qr_url: String,
    pub expires_at_ms: i64,
    pub poll_interval_ms: i64,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPollRequest {
    pub device_code: String,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegisterPollResponse {
    #[serde(default)]
    pub status: String,
    pub device_code: String,
    pub domain: String,
    #[serde(default)]
    pub credentials: Option<FeishuRegistrationCredentials>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeishuRegistrationCredentials {
    #[serde(alias = "client_id")]
    pub app_id: String,
    #[serde(alias = "client_secret")]
    pub app_secret: String,
    pub domain: String,
    #[serde(default)]
    pub operator_open_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelStartRequest {
    pub channel_id: i64,
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_open_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelStartResponse {
    pub status: String,
    pub channel_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelStopRequest {
    pub channel_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelStopResponse {
    pub status: String,
    pub channel_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppScopeStatusRequest {
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppScopeStatusResponse {
    #[serde(default)]
    pub scopes: Vec<AppScopeGrantStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppScopeGrantStatus {
    pub scope_name: String,
    pub grant_status: i32,
    #[serde(default)]
    pub scope_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppScopeApplyRequest {
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppScopeApplyResponse {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageSendRequest {
    pub channel_id: i64,
    pub receive_id: String,
    pub receive_id_type: String,
    #[serde(default = "default_message_type")]
    pub msg_type: String,
    pub content: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_in_thread: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_to_plain_chat: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
}

fn default_message_type() -> String {
    "text".to_string()
}

fn default_member_id_type() -> String {
    "open_id".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageSendResponse {
    pub status: String,
    pub message_id: String,
    #[serde(default)]
    pub fell_back: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCreateRequest {
    pub channel_id: i64,
    pub name: String,
    #[serde(default = "default_member_id_type")]
    pub member_id_type: String,
    #[serde(default)]
    pub member_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatCreateResponse {
    pub status: String,
    pub chat_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMembersAddRequest {
    pub channel_id: i64,
    pub chat_id: String,
    #[serde(default = "default_member_id_type")]
    pub member_id_type: String,
    #[serde(default)]
    pub member_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMembersAddResponse {
    pub status: String,
    pub chat_id: String,
    #[serde(default)]
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatPatchRequest {
    pub channel_id: i64,
    pub chat_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatPatchResponse {
    pub status: String,
    pub chat_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeHealthResponse {
    pub status: String,
    #[serde(default)]
    pub transport: String,
}

#[derive(Debug)]
pub enum FeishuNativeError {
    Unavailable(String),
    Protocol(String),
    Api(String),
    Serde(serde_json::Error),
}

impl FeishuNativeError {
    pub fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl std::fmt::Display for FeishuNativeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(f, "Feishu native transport unavailable: {message}")
            }
            Self::Protocol(message) => {
                write!(f, "Feishu native transport protocol error: {message}")
            }
            Self::Api(message) => write!(f, "Feishu native transport API error: {message}"),
            Self::Serde(error) => write!(f, "Feishu native transport JSON error: {error}"),
        }
    }
}

impl std::error::Error for FeishuNativeError {}

impl From<serde_json::Error> for FeishuNativeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

#[derive(Clone)]
struct ReqwestFeishuNativeTransport {
    http: reqwest::Client,
    event_handler: Option<FeishuNativeEventHandler>,
    channels: Arc<Mutex<HashMap<i64, RunningNativeChannel>>>,
    tokens: Arc<Mutex<HashMap<String, CachedTenantToken>>>,
}

impl ReqwestFeishuNativeTransport {
    fn new(event_handler: Option<FeishuNativeEventHandler>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .no_proxy()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            event_handler,
            channels: Arc::new(Mutex::new(HashMap::new())),
            tokens: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn registration_start(
        &self,
        _request: RegisterStartRequest,
    ) -> Result<RegisterStartResponse, FeishuNativeError> {
        let response = self
            .post_registration_form(
                FEISHU_REGISTRATION_DOMAIN,
                &[
                    ("action", "begin"),
                    ("archetype", "PersonalAgent"),
                    ("auth_method", "client_secret"),
                    ("request_user_info", "open_id tenant_brand"),
                ],
            )
            .await?;
        normalize_registration_start(response, FEISHU_REGISTRATION_DOMAIN)
    }

    async fn registration_poll(
        &self,
        request: RegisterPollRequest,
    ) -> Result<RegisterPollResponse, FeishuNativeError> {
        let device_code = request.device_code.trim();
        if device_code.is_empty() {
            return Err(FeishuNativeError::Protocol(
                "device_code is required".to_string(),
            ));
        }
        let domain = normalize_registration_domain(&request.domain);
        let first = self.poll_once(domain, device_code).await?;
        if domain == FEISHU_REGISTRATION_DOMAIN
            && first.credentials.is_none()
            && first.domain == LARK_REGISTRATION_DOMAIN
        {
            return self.poll_once(LARK_REGISTRATION_DOMAIN, device_code).await;
        }
        Ok(first)
    }

    async fn poll_once(
        &self,
        domain: &str,
        device_code: &str,
    ) -> Result<RegisterPollResponse, FeishuNativeError> {
        let response = self
            .post_registration_form(domain, &[("action", "poll"), ("device_code", device_code)])
            .await?;
        normalize_registration_poll(response, domain, device_code)
    }

    async fn post_registration_form(
        &self,
        domain: &str,
        body: &[(&str, &str)],
    ) -> Result<Value, FeishuNativeError> {
        let url = format!(
            "https://{}/oauth/v1/app/registration",
            accounts_host_for_domain(domain)
        );
        let response = self
            .http
            .post(url)
            .header("content-type", "application/x-www-form-urlencoded")
            .form(body)
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = response.status();
        let payload = response.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if status.is_success() || is_pending_registration_response(&payload) {
            return Ok(payload);
        }
        Err(FeishuNativeError::Api(format!(
            "registration HTTP {}{}",
            status.as_u16(),
            safe_api_code_suffix(&payload)
        )))
    }

    async fn start_channel(
        &self,
        request: ChannelStartRequest,
    ) -> Result<ChannelStartResponse, FeishuNativeError> {
        self.stop_channel(request.channel_id).await?;

        let params = normalize_channel_start_request(request)?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let (ready_tx, ready_rx) = oneshot::channel();
        let worker = self.clone();
        let worker_params = params.clone();
        let handle = tokio::spawn(async move {
            worker
                .run_channel_loop(worker_params, stop_rx, Some(ready_tx))
                .await;
        });

        let ready = timeout(Duration::from_secs(20), ready_rx)
            .await
            .map_err(|_| {
                FeishuNativeError::Unavailable("Feishu WebSocket connect timed out".to_string())
            })?
            .map_err(|_| {
                FeishuNativeError::Unavailable("Feishu WebSocket worker stopped".to_string())
            })?;

        if let Err(error) = ready {
            handle.abort();
            return Err(error);
        }

        self.channels.lock().await.insert(
            params.channel_id,
            RunningNativeChannel {
                params: params.clone(),
                stop_tx,
                handle,
            },
        );

        Ok(ChannelStartResponse {
            status: "started".to_string(),
            channel_id: params.channel_id,
        })
    }

    async fn stop_channel(
        &self,
        channel_id: i64,
    ) -> Result<ChannelStopResponse, FeishuNativeError> {
        if let Some(running) = self.channels.lock().await.remove(&channel_id) {
            let _ = running.stop_tx.send(true);
            running.handle.abort();
        }
        Ok(ChannelStopResponse {
            status: "stopped".to_string(),
            channel_id,
        })
    }

    async fn run_channel_loop(
        &self,
        params: ChannelStartRequest,
        mut stop_rx: watch::Receiver<bool>,
        mut ready_tx: Option<oneshot::Sender<Result<(), FeishuNativeError>>>,
    ) {
        loop {
            if *stop_rx.borrow() {
                return;
            }

            let result = self
                .run_single_ws_session(&params, &mut stop_rx, &mut ready_tx)
                .await;
            if let Err(error) = &result {
                if let Some(tx) = ready_tx.take() {
                    let _ = tx.send(Err(clone_native_error(error)));
                }
            }
            if *stop_rx.borrow() {
                return;
            }
            if let Err(error) = result {
                let safe = safe_worker_error_message(&error.to_string());
                eprintln!(
                    "[feishu.native] channel {} websocket session ended: {safe}",
                    params.channel_id
                );
            }
            sleep(DEFAULT_RECONNECT_INTERVAL).await;
        }
    }

    async fn run_single_ws_session(
        &self,
        params: &ChannelStartRequest,
        stop_rx: &mut watch::Receiver<bool>,
        ready_tx: &mut Option<oneshot::Sender<Result<(), FeishuNativeError>>>,
    ) -> Result<(), FeishuNativeError> {
        let connect_config = self.pull_connect_config(params).await?;
        let service_id = service_id_from_url(&connect_config.url).unwrap_or(0);
        let ping_interval = connect_config
            .client_config
            .ping_interval
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_PING_INTERVAL);
        let (ws, _) = connect_async(&connect_config.url)
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        if let Some(tx) = ready_tx.take() {
            let _ = tx.send(Ok(()));
        }
        let (mut write, mut read) = ws.split();
        let mut ping = tokio::time::interval(ping_interval);
        let mut fragments = WsFragmentCache::default();

        loop {
            tokio::select! {
                changed = stop_rx.changed() => {
                    if changed.is_ok() && *stop_rx.borrow() {
                        return Ok(());
                    }
                }
                _ = ping.tick() => {
                    let frame = FeishuWsFrame::ping(service_id);
                    let bytes = frame.encode_to_vec();
                    write.send(WsMessage::Binary(bytes)).await
                        .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
                }
                message = read.next() => {
                    let Some(message) = message else {
                        return Err(FeishuNativeError::Unavailable("Feishu WebSocket closed".to_string()));
                    };
                    match message.map_err(|err| FeishuNativeError::Unavailable(err.to_string()))? {
                        WsMessage::Binary(bytes) => {
                            if let Some(ack) = self.handle_ws_binary(params, &mut fragments, &bytes).await? {
                                write.send(WsMessage::Binary(ack.encode_to_vec())).await
                                    .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
                            }
                        }
                        WsMessage::Close(_) => {
                            return Err(FeishuNativeError::Unavailable("Feishu WebSocket closed".to_string()));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    async fn handle_ws_binary(
        &self,
        params: &ChannelStartRequest,
        fragments: &mut WsFragmentCache,
        bytes: &[u8],
    ) -> Result<Option<FeishuWsFrame>, FeishuNativeError> {
        let frame = FeishuWsFrame::decode(bytes)
            .map_err(|err| FeishuNativeError::Protocol(err.to_string()))?;
        if frame.method == FrameType::Control as i32 {
            return Ok(None);
        }
        if frame.method != FrameType::Data as i32 {
            return Ok(None);
        }
        let headers = frame.headers_map();
        if headers.get("type").map(String::as_str) != Some("event") {
            return Ok(None);
        }

        let message_id = headers.get("message_id").cloned().unwrap_or_default();
        let sum = headers
            .get("sum")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1);
        let seq = headers
            .get("seq")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let payload = frame.payload.clone().unwrap_or_default();
        let Some(event_payload) = fragments.push(message_id, sum, seq, payload)? else {
            return Ok(None);
        };

        let started = Instant::now();
        let event = normalize_ws_im_event(params, &event_payload);
        if let Some(event) = event {
            if let Some(handler) = &self.event_handler {
                handler(event).await;
            }
        }
        Ok(Some(frame.ack(StatusCode::OK, started.elapsed())))
    }

    async fn pull_connect_config(
        &self,
        params: &ChannelStartRequest,
    ) -> Result<WsConnectConfig, FeishuNativeError> {
        let domain = open_domain(&params.domain);
        let url = format!("{domain}/callback/ws/endpoint");
        let payload = self
            .http
            .post(url)
            .header("locale", "zh")
            .header("user-agent", SDK_COMPAT_USER_AGENT)
            .json(&json!({
                "AppID": params.app_id,
                "AppSecret": params.app_secret,
            }))
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?
            .json::<Value>()
            .await
            .map_err(|err| FeishuNativeError::Protocol(err.to_string()))?;
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            return Err(FeishuNativeError::Api(format!(
                "pullConnectConfig failed: code={code}{}",
                safe_message_suffix(&payload)
            )));
        }
        parse_ws_connect_config(&payload)
    }

    async fn send_message(
        &self,
        request: MessageSendRequest,
    ) -> Result<MessageSendResponse, FeishuNativeError> {
        let credentials = self.message_credentials(&request).await?;
        if let Some(reply_to_message_id) = request.reply_to_message_id.as_deref() {
            let reply_result = self
                .send_reply_message(&credentials, &request, reply_to_message_id)
                .await;
            match reply_result {
                Ok(message_id) => {
                    return Ok(MessageSendResponse {
                        status: "sent".to_string(),
                        message_id,
                        fell_back: false,
                    })
                }
                Err(error) if request.fallback_to_plain_chat.unwrap_or(false) => {
                    let safe = safe_worker_error_message(&error.to_string());
                    eprintln!("[feishu.native] reply failed, falling back to create: {safe}");
                }
                Err(error) => return Err(error),
            }
        }
        let message_id = self.create_message(&credentials, &request).await?;
        Ok(MessageSendResponse {
            status: "sent".to_string(),
            message_id,
            fell_back: request.reply_to_message_id.is_some(),
        })
    }

    async fn create_chat(
        &self,
        request: ChatCreateRequest,
    ) -> Result<ChatCreateResponse, FeishuNativeError> {
        let credentials = self
            .resolve_request_credentials(
                request.channel_id,
                request.app_id.as_deref(),
                request.app_secret.as_deref(),
                request.domain.as_deref(),
            )
            .await?;
        let token = self.tenant_access_token(&credentials).await?;
        let avatar = match self
            .upload_chat_avatar_if_needed(&credentials, request.avatar_url.as_deref())
            .await
        {
            Ok(avatar) => avatar,
            Err(error) => {
                let safe = safe_worker_error_message(&error.to_string());
                eprintln!("[feishu.native] chat.create avatar upload skipped: {safe}");
                None
            }
        };
        let member_id_type = normalized_member_id_type(&request.member_id_type);
        let url = format!(
            "{}/open-apis/im/v1/chats?user_id_type={}",
            open_domain(&credentials.domain),
            urlencoding::encode(&member_id_type)
        );
        let mut body = json!({
            "name": request.name,
            "user_id_list": request.member_ids,
        });
        if let Some(avatar) = avatar {
            body["avatar"] = Value::String(avatar);
        }
        let payload = self
            .send_feishu_json_api(Method::POST, &url, token, &body, "chat.create")
            .await?;
        Ok(ChatCreateResponse {
            status: "created".to_string(),
            chat_id: chat_id_from_payload(&payload).ok_or_else(|| {
                FeishuNativeError::Protocol("chat.create response missing chat_id".to_string())
            })?,
            name: string_from_path(&payload, &["data", "name"])
                .or_else(|| string_from_path(&payload, &["name"]))
                .unwrap_or(request.name),
        })
    }

    async fn add_chat_members(
        &self,
        request: ChatMembersAddRequest,
    ) -> Result<ChatMembersAddResponse, FeishuNativeError> {
        let credentials = self
            .resolve_request_credentials(
                request.channel_id,
                request.app_id.as_deref(),
                request.app_secret.as_deref(),
                request.domain.as_deref(),
            )
            .await?;
        let token = self.tenant_access_token(&credentials).await?;
        let member_id_type = normalized_member_id_type(&request.member_id_type);
        let url = format!(
            "{}/open-apis/im/v1/chats/{}/members?member_id_type={}",
            open_domain(&credentials.domain),
            urlencoding::encode(&request.chat_id),
            urlencoding::encode(&member_id_type)
        );
        let payload = self
            .send_feishu_json_api(
                Method::POST,
                &url,
                token,
                &json!({
                    "id_list": request.member_ids,
                }),
                "chat.members.add",
            )
            .await?;
        Ok(ChatMembersAddResponse {
            status: "ok".to_string(),
            chat_id: chat_id_from_payload(&payload).unwrap_or(request.chat_id),
            member_ids: string_array(
                payload
                    .pointer("/data/member_ids")
                    .or_else(|| payload.pointer("/data/id_list"))
                    .or_else(|| payload.pointer("/data/user_id_list")),
            ),
        })
    }

    async fn patch_chat(
        &self,
        request: ChatPatchRequest,
    ) -> Result<ChatPatchResponse, FeishuNativeError> {
        let credentials = self
            .resolve_request_credentials(
                request.channel_id,
                request.app_id.as_deref(),
                request.app_secret.as_deref(),
                request.domain.as_deref(),
            )
            .await?;
        let token = self.tenant_access_token(&credentials).await?;
        let avatar = match self
            .upload_chat_avatar_if_needed(&credentials, request.avatar_url.as_deref())
            .await
        {
            Ok(avatar) => avatar,
            Err(error) => {
                let safe = safe_worker_error_message(&error.to_string());
                eprintln!("[feishu.native] chat.patch avatar upload skipped: {safe}");
                None
            }
        };
        let url = format!(
            "{}/open-apis/chat/v4/update/",
            open_domain(&credentials.domain)
        );
        let mut body = json!({
            "chat_id": request.chat_id,
            "name": request.name,
        });
        if let Some(avatar) = avatar {
            body["avatar"] = Value::String(avatar);
        }
        let payload = self
            .send_feishu_json_api(Method::POST, &url, token, &body, "chat.patch")
            .await?;
        Ok(ChatPatchResponse {
            status: "ok".to_string(),
            chat_id: chat_id_from_payload(&payload).unwrap_or(request.chat_id),
            name: string_from_path(&payload, &["data", "name"])
                .or_else(|| string_from_path(&payload, &["name"]))
                .unwrap_or(request.name),
        })
    }

    async fn query_app_scope_status(
        &self,
        request: AppScopeStatusRequest,
    ) -> Result<AppScopeStatusResponse, FeishuNativeError> {
        let credentials = ChannelStartRequest {
            channel_id: 0,
            app_id: request.app_id,
            app_secret: request.app_secret,
            domain: normalize_registration_domain(&request.domain).to_string(),
            bot_open_id: None,
            bot_user_id: None,
        };
        let token = self.tenant_access_token(&credentials).await?;
        let url = format!(
            "{}/open-apis/application/v6/scopes",
            open_domain(&credentials.domain)
        );
        let payload = self
            .send_feishu_empty_json_api(Method::GET, &url, token, "app.scope.status")
            .await?;
        let scopes = payload
            .pointer("/data/scopes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(AppScopeStatusResponse {
            scopes: scopes
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    async fn apply_app_scopes(
        &self,
        request: AppScopeApplyRequest,
    ) -> Result<AppScopeApplyResponse, FeishuNativeError> {
        let credentials = ChannelStartRequest {
            channel_id: 0,
            app_id: request.app_id,
            app_secret: request.app_secret,
            domain: normalize_registration_domain(&request.domain).to_string(),
            bot_open_id: None,
            bot_user_id: None,
        };
        let token = self.tenant_access_token(&credentials).await?;
        let url = format!(
            "{}/open-apis/application/v6/scopes/apply",
            open_domain(&credentials.domain)
        );
        self.send_feishu_json_api(Method::POST, &url, token, &json!({}), "app.scope.apply")
            .await?;
        Ok(AppScopeApplyResponse {
            status: "ok".to_string(),
        })
    }

    async fn message_credentials(
        &self,
        request: &MessageSendRequest,
    ) -> Result<ChannelStartRequest, FeishuNativeError> {
        self.resolve_request_credentials(
            request.channel_id,
            request.app_id.as_deref(),
            request.app_secret.as_deref(),
            request.domain.as_deref(),
        )
        .await
    }

    async fn upload_chat_avatar_if_needed(
        &self,
        credentials: &ChannelStartRequest,
        avatar_url: Option<&str>,
    ) -> Result<Option<String>, FeishuNativeError> {
        let avatar_url = avatar_url.unwrap_or("").trim();
        if avatar_url.is_empty() {
            return Ok(None);
        }

        let token = self.tenant_access_token(credentials).await?;
        let response = self
            .http
            .get(avatar_url)
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(FeishuNativeError::Api(format!(
                "avatar download HTTP {}",
                status.as_u16()
            )));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_string();
        let bytes = response
            .bytes()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;

        let url = format!(
            "{}/open-apis/im/v1/images",
            open_domain(&credentials.domain)
        );
        let form = Form::new().text("image_type", "avatar").part(
            "image",
            Part::bytes(bytes.to_vec())
                .file_name("avatar.jpg")
                .mime_str(&content_type)
                .map_err(|err| FeishuNativeError::Protocol(err.to_string()))?,
        );
        let payload = self
            .http
            .post(url)
            .bearer_auth(token)
            .header("user-agent", SDK_COMPAT_USER_AGENT)
            .multipart(form)
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = payload.status();
        let payload = payload
            .json::<Value>()
            .await
            .map_err(|err| FeishuNativeError::Protocol(err.to_string()))?;
        if !status.is_success() {
            return Err(FeishuNativeError::Api(format!(
                "image.create HTTP {}{}",
                status.as_u16(),
                safe_api_code_suffix(&payload)
            )));
        }
        let image_key = string_from_path(&payload, &["data", "image_key"])
            .or_else(|| string_from_path(&payload, &["image_key"]))
            .ok_or_else(|| {
                FeishuNativeError::Protocol("image.create response missing image_key".to_string())
            })?;
        Ok(Some(image_key))
    }

    async fn resolve_request_credentials(
        &self,
        channel_id: i64,
        app_id: Option<&str>,
        app_secret: Option<&str>,
        domain: Option<&str>,
    ) -> Result<ChannelStartRequest, FeishuNativeError> {
        let app_id = app_id.unwrap_or("").trim();
        let app_secret = app_secret.unwrap_or("").trim();
        if !app_id.is_empty() && !app_secret.is_empty() {
            return Ok(ChannelStartRequest {
                channel_id,
                app_id: app_id.to_string(),
                app_secret: app_secret.to_string(),
                domain: domain
                    .map(normalize_registration_domain)
                    .unwrap_or(FEISHU_REGISTRATION_DOMAIN)
                    .to_string(),
                bot_open_id: None,
                bot_user_id: None,
            });
        }
        let channels = self.channels.lock().await;
        channels
            .get(&channel_id)
            .map(|running| running.params.clone())
            .ok_or_else(|| {
                FeishuNativeError::Unavailable(format!(
                    "channel {} is not started and message credentials were not provided",
                    channel_id
                ))
            })
    }

    async fn send_reply_message(
        &self,
        credentials: &ChannelStartRequest,
        request: &MessageSendRequest,
        message_id: &str,
    ) -> Result<String, FeishuNativeError> {
        let token = self.tenant_access_token(credentials).await?;
        let url = format!(
            "{}/open-apis/im/v1/messages/{}/reply",
            open_domain(&credentials.domain),
            urlencoding::encode(message_id)
        );
        let body = json!({
            "msg_type": normalized_message_type(&request.msg_type),
            "content": serde_json::to_string(&request.content)?,
            "reply_in_thread": request.reply_in_thread.unwrap_or(false),
        });
        self.post_feishu_message_api(&url, token, &body).await
    }

    async fn create_message(
        &self,
        credentials: &ChannelStartRequest,
        request: &MessageSendRequest,
    ) -> Result<String, FeishuNativeError> {
        let token = self.tenant_access_token(credentials).await?;
        let url = format!(
            "{}/open-apis/im/v1/messages?receive_id_type={}",
            open_domain(&credentials.domain),
            urlencoding::encode(&request.receive_id_type)
        );
        let body = json!({
            "receive_id": request.receive_id,
            "msg_type": normalized_message_type(&request.msg_type),
            "content": serde_json::to_string(&request.content)?,
        });
        self.post_feishu_message_api(&url, token, &body).await
    }

    async fn post_feishu_message_api(
        &self,
        url: &str,
        token: String,
        body: &Value,
    ) -> Result<String, FeishuNativeError> {
        let payload = self
            .http
            .post(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = payload.status();
        let payload = payload.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(FeishuNativeError::Api(format!(
                "message HTTP {}{}{}",
                status.as_u16(),
                safe_api_code_suffix(&payload),
                safe_message_suffix(&payload)
            )));
        }
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(FeishuNativeError::Api(format!(
                "message API code={code}{}",
                safe_message_suffix(&payload)
            )));
        }
        string_from_path(&payload, &["data", "message_id"])
            .or_else(|| string_from_path(&payload, &["message_id"]))
            .ok_or_else(|| {
                FeishuNativeError::Protocol("message response missing message_id".to_string())
            })
    }

    async fn send_feishu_json_api(
        &self,
        method: Method,
        url: &str,
        token: String,
        body: &Value,
        action: &str,
    ) -> Result<Value, FeishuNativeError> {
        let payload = self
            .http
            .request(method, url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = payload.status();
        let payload = payload.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(FeishuNativeError::Api(format!(
                "{action} HTTP {}{}{}",
                status.as_u16(),
                safe_api_code_suffix(&payload),
                safe_message_suffix(&payload)
            )));
        }
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(FeishuNativeError::Api(format!(
                "{action} API code={code}{}",
                safe_message_suffix(&payload)
            )));
        }
        Ok(payload)
    }

    async fn send_feishu_empty_json_api(
        &self,
        method: Method,
        url: &str,
        token: String,
        action: &str,
    ) -> Result<Value, FeishuNativeError> {
        let payload = self
            .http
            .request(method, url)
            .bearer_auth(token)
            .header("user-agent", SDK_COMPAT_USER_AGENT)
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = payload.status();
        let payload = payload.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(FeishuNativeError::Api(format!(
                "{action} HTTP {}{}{}",
                status.as_u16(),
                safe_api_code_suffix(&payload),
                safe_message_suffix(&payload)
            )));
        }
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(FeishuNativeError::Api(format!(
                "{action} API code={code}{}",
                safe_message_suffix(&payload)
            )));
        }
        Ok(payload)
    }

    async fn tenant_access_token(
        &self,
        credentials: &ChannelStartRequest,
    ) -> Result<String, FeishuNativeError> {
        let key = format!(
            "{}\n{}\n{}",
            normalize_registration_domain(&credentials.domain),
            credentials.app_id,
            credentials.app_secret
        );
        if let Some(token) = self.tokens.lock().await.get(&key) {
            if token.expires_at > Instant::now() {
                return Ok(token.value.clone());
            }
        }

        let url = format!(
            "{}/open-apis/auth/v3/tenant_access_token/internal",
            open_domain(&credentials.domain)
        );
        let payload = self
            .http
            .post(url)
            .json(&json!({
                "app_id": credentials.app_id,
                "app_secret": credentials.app_secret,
            }))
            .send()
            .await
            .map_err(|err| FeishuNativeError::Unavailable(err.to_string()))?;
        let status = payload.status();
        let payload = payload.json::<Value>().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            return Err(FeishuNativeError::Api(format!(
                "tenant token HTTP {}{}",
                status.as_u16(),
                safe_api_code_suffix(&payload)
            )));
        }
        let code = payload.get("code").and_then(Value::as_i64).unwrap_or(0);
        if code != 0 {
            return Err(FeishuNativeError::Api(format!(
                "tenant token API code={code}{}",
                safe_message_suffix(&payload)
            )));
        }
        let token = string_from_path(&payload, &["tenant_access_token"])
            .or_else(|| string_from_path(&payload, &["data", "tenant_access_token"]))
            .ok_or_else(|| {
                FeishuNativeError::Protocol("tenant token response missing token".to_string())
            })?;
        let expire = payload
            .get("expire")
            .or_else(|| payload.pointer("/data/expire"))
            .and_then(Value::as_u64)
            .unwrap_or(7_200);
        let ttl = Duration::from_secs(expire.saturating_sub(180).max(60));
        self.tokens.lock().await.insert(
            key,
            CachedTenantToken {
                value: token.clone(),
                expires_at: Instant::now() + ttl,
            },
        );
        Ok(token)
    }
}

impl FeishuNativeTransport for ReqwestFeishuNativeTransport {
    fn call(&self, request: FeishuNativeRequest) -> FeishuNativeCallFuture {
        let transport = self.clone();
        Box::pin(async move {
            let result = match request.method.as_str() {
                "register.start" => {
                    let params = serde_json::from_value::<RegisterStartRequest>(request.params)?;
                    serde_json::to_value(transport.registration_start(params).await?)?
                }
                "register.poll" => {
                    let params = serde_json::from_value::<RegisterPollRequest>(request.params)?;
                    serde_json::to_value(transport.registration_poll(params).await?)?
                }
                "channel.start" => {
                    let params = serde_json::from_value::<ChannelStartRequest>(request.params)?;
                    serde_json::to_value(transport.start_channel(params).await?)?
                }
                "channel.stop" => {
                    let params = serde_json::from_value::<ChannelStopRequest>(request.params)?;
                    serde_json::to_value(transport.stop_channel(params.channel_id).await?)?
                }
                "message.send" => {
                    let params = serde_json::from_value::<MessageSendRequest>(request.params)?;
                    serde_json::to_value(transport.send_message(params).await?)?
                }
                "chat.create" => {
                    let params = serde_json::from_value::<ChatCreateRequest>(request.params)?;
                    serde_json::to_value(transport.create_chat(params).await?)?
                }
                "chat.members.add" => {
                    let params = serde_json::from_value::<ChatMembersAddRequest>(request.params)?;
                    serde_json::to_value(transport.add_chat_members(params).await?)?
                }
                "chat.patch" => {
                    let params = serde_json::from_value::<ChatPatchRequest>(request.params)?;
                    serde_json::to_value(transport.patch_chat(params).await?)?
                }
                "app.scope.status" => {
                    let params = serde_json::from_value::<AppScopeStatusRequest>(request.params)?;
                    serde_json::to_value(transport.query_app_scope_status(params).await?)?
                }
                "app.scope.apply" => {
                    let params = serde_json::from_value::<AppScopeApplyRequest>(request.params)?;
                    serde_json::to_value(transport.apply_app_scopes(params).await?)?
                }
                "runtime.health" => serde_json::to_value(RuntimeHealthResponse {
                    status: "ok".to_string(),
                    transport: "rust-native".to_string(),
                })?,
                other => {
                    return Err(FeishuNativeError::Protocol(format!(
                        "unsupported Feishu native method: {other}"
                    )))
                }
            };
            Ok(FeishuNativeResponse::success(result))
        })
    }
}

#[derive(Debug)]
struct RunningNativeChannel {
    params: ChannelStartRequest,
    stop_tx: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

#[derive(Clone)]
struct CachedTenantToken {
    value: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, Default)]
struct WsConnectClientConfig {
    ping_interval: Option<u64>,
}

#[derive(Debug, Clone)]
struct WsConnectConfig {
    url: String,
    client_config: WsConnectClientConfig,
}

#[derive(Clone, PartialEq, Message)]
struct FeishuWsHeader {
    #[prost(string, tag = "1")]
    key: String,
    #[prost(string, tag = "2")]
    value: String,
}

#[derive(Clone, PartialEq, Message)]
struct FeishuWsFrame {
    #[prost(uint64, tag = "1")]
    seq_id: u64,
    #[prost(uint64, tag = "2")]
    log_id: u64,
    #[prost(int32, tag = "3")]
    service: i32,
    #[prost(int32, tag = "4")]
    method: i32,
    #[prost(message, repeated, tag = "5")]
    headers: Vec<FeishuWsHeader>,
    #[prost(string, optional, tag = "6")]
    payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    payload_type: Option<String>,
    #[prost(bytes, optional, tag = "8")]
    payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    log_id_new: Option<String>,
}

impl FeishuWsFrame {
    fn ping(service_id: i32) -> Self {
        Self {
            seq_id: 0,
            log_id: 0,
            service: service_id,
            method: FrameType::Control as i32,
            headers: vec![FeishuWsHeader {
                key: "type".to_string(),
                value: "ping".to_string(),
            }],
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        }
    }

    fn ack(&self, status: StatusCode, elapsed: Duration) -> Self {
        let mut headers = self.headers.clone();
        headers.push(FeishuWsHeader {
            key: "biz_rt".to_string(),
            value: elapsed.as_millis().to_string(),
        });
        Self {
            headers,
            payload: Some(json!({ "code": status.as_u16() }).to_string().into_bytes()),
            ..self.clone()
        }
    }

    fn headers_map(&self) -> HashMap<String, String> {
        self.headers
            .iter()
            .map(|header| (header.key.clone(), header.value.clone()))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameType {
    Control = 0,
    Data = 1,
}

#[derive(Debug, Default)]
struct WsFragmentCache {
    inner: HashMap<String, WsFragment>,
}

#[derive(Debug)]
struct WsFragment {
    parts: Vec<Option<Vec<u8>>>,
    created_at: Instant,
}

impl WsFragmentCache {
    fn push(
        &mut self,
        message_id: String,
        sum: usize,
        seq: usize,
        data: Vec<u8>,
    ) -> Result<Option<Value>, FeishuNativeError> {
        self.evict_expired();
        let sum = sum.max(1);
        if seq >= sum {
            return Err(FeishuNativeError::Protocol(
                "Feishu WebSocket frame seq exceeds sum".to_string(),
            ));
        }
        let key = if message_id.trim().is_empty() {
            format!("single-{}", Instant::now().elapsed().as_nanos())
        } else {
            message_id
        };
        let fragment = self.inner.entry(key.clone()).or_insert_with(|| WsFragment {
            parts: vec![None; sum],
            created_at: Instant::now(),
        });
        if fragment.parts.len() != sum {
            fragment.parts.resize(sum, None);
        }
        fragment.parts[seq] = Some(data);
        if !fragment.parts.iter().all(Option::is_some) {
            return Ok(None);
        }
        let bytes = fragment
            .parts
            .iter()
            .flat_map(|part| part.as_ref().into_iter().flatten().copied())
            .collect::<Vec<_>>();
        self.inner.remove(&key);
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(FeishuNativeError::from)
    }

    fn evict_expired(&mut self) {
        let now = Instant::now();
        self.inner.retain(|_, fragment| {
            now.duration_since(fragment.created_at) <= Duration::from_secs(10)
        });
    }
}

fn normalize_channel_start_request(
    request: ChannelStartRequest,
) -> Result<ChannelStartRequest, FeishuNativeError> {
    let app_id = request.app_id.trim();
    let app_secret = request.app_secret.trim();
    if app_id.is_empty() || app_secret.is_empty() {
        return Err(FeishuNativeError::Protocol(
            "missing app_id or app_secret".to_string(),
        ));
    }
    Ok(ChannelStartRequest {
        app_id: app_id.to_string(),
        app_secret: app_secret.to_string(),
        domain: normalize_channel_domain(&request.domain),
        bot_open_id: request
            .bot_open_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        bot_user_id: request
            .bot_user_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        ..request
    })
}

fn normalize_registration_start(
    raw: Value,
    domain: &str,
) -> Result<RegisterStartResponse, FeishuNativeError> {
    let data = object_value(raw.get("data"))
        .unwrap_or_else(|| object_value(Some(&raw)).unwrap_or_default());
    Ok(RegisterStartResponse {
        device_code: required_string(&data, &["device_code", "deviceCode"])?,
        qr_url: required_string(
            &data,
            &[
                "qr_url",
                "qrUrl",
                "qrcode_url",
                "qrcodeUrl",
                "verification_uri_complete",
                "verificationUriComplete",
            ],
        )?,
        expires_at_ms: number_value(&data, &["expires_at_ms", "expiresAtMs", "expire_time_ms"])
            .or_else(|| {
                duration_ms_value(&data, &["expire_in", "expireIn", "expires_in", "expiresIn"])
            })
            .unwrap_or(0),
        poll_interval_ms: number_value(
            &data,
            &["poll_interval_ms", "pollIntervalMs", "interval_ms"],
        )
        .or_else(|| duration_ms_value(&data, &["interval"]))
        .unwrap_or(DEFAULT_POLL_INTERVAL_MS),
        domain: normalize_domain_with_fallback(data.get("domain"), domain).to_string(),
    })
}

fn normalize_registration_poll(
    raw: Value,
    domain: &str,
    device_code: &str,
) -> Result<RegisterPollResponse, FeishuNativeError> {
    let data = object_value(raw.get("data"))
        .unwrap_or_else(|| object_value(Some(&raw)).unwrap_or_default());
    let inferred_domain = if has_lark_tenant_brand(&data) {
        LARK_REGISTRATION_DOMAIN
    } else {
        domain
    };
    let credentials = registration_credentials_from(&data, inferred_domain);
    let status = string_value(data.get("status")).unwrap_or_else(|| {
        if credentials.is_some() {
            "confirmed"
        } else {
            "pending"
        }
        .to_string()
    });
    Ok(RegisterPollResponse {
        status,
        device_code: string_value(data.get("device_code"))
            .or_else(|| string_value(data.get("deviceCode")))
            .unwrap_or_else(|| device_code.to_string()),
        domain: credentials
            .as_ref()
            .map(|value| value.domain.clone())
            .unwrap_or_else(|| {
                normalize_domain_with_fallback(data.get("domain"), inferred_domain).to_string()
            }),
        credentials,
    })
}

fn registration_credentials_from(
    data: &Map<String, Value>,
    fallback_domain: &str,
) -> Option<FeishuRegistrationCredentials> {
    let nested = object_value(data.get("credentials")).unwrap_or_default();
    let source = if nested.is_empty() { data } else { &nested };
    let app_id = string_value(source.get("app_id"))
        .or_else(|| string_value(source.get("appId")))
        .or_else(|| string_value(source.get("client_id")))
        .or_else(|| string_value(source.get("clientId")))?;
    let app_secret = string_value(source.get("app_secret"))
        .or_else(|| string_value(source.get("appSecret")))
        .or_else(|| string_value(source.get("client_secret")))
        .or_else(|| string_value(source.get("clientSecret")))?;
    let user_info = object_value(data.get("user_info"))
        .or_else(|| object_value(data.get("userInfo")))
        .unwrap_or_default();
    Some(FeishuRegistrationCredentials {
        app_id,
        app_secret,
        domain: normalize_domain_with_fallback(source.get("domain"), fallback_domain).to_string(),
        operator_open_id: string_value(user_info.get("open_id"))
            .or_else(|| string_value(user_info.get("openId")))
            .or_else(|| string_value(data.get("operator_open_id")))
            .or_else(|| string_value(data.get("operatorOpenId")))
            .or_else(|| string_value(source.get("operator_open_id")))
            .or_else(|| string_value(source.get("operatorOpenId")))
            .unwrap_or_default(),
    })
}

fn normalize_ws_im_event(params: &ChannelStartRequest, raw: &Value) -> Option<FeishuImEvent> {
    let root = as_map(Some(raw));
    let header = as_map(root.get("header"));
    let event = as_map(root.get("event"));
    let message = object_value(event.get("message"))
        .or_else(|| object_value(root.get("message")))
        .unwrap_or_default();
    if message.is_empty() {
        return normalize_ws_non_message_event(params, raw, &root, &header, &event);
    }
    let sender = object_value(event.get("sender"))
        .or_else(|| object_value(root.get("sender")))
        .unwrap_or_default();
    let sender_id = object_value(sender.get("sender_id"))
        .or_else(|| object_value(root.get("sender_id")))
        .unwrap_or_default();
    let mentions = message
        .get("mentions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| object_value(Some(&value)))
        .collect::<Vec<_>>();
    let bot_open_id = params.bot_open_id.as_deref().unwrap_or("").trim();
    let bot_user_id = params.bot_user_id.as_deref().unwrap_or("").trim();
    let sender_open_id = string_value(sender_id.get("open_id")).unwrap_or_default();
    let sender_user_id = string_value(sender_id.get("user_id")).unwrap_or_default();
    let has_configured_bot = !bot_open_id.is_empty() || !bot_user_id.is_empty();
    let mention_all = mentions.iter().any(is_mention_all);
    let mentioned_bot = mention_all
        || if has_configured_bot {
            mentions.iter().any(|mention| {
                let id = as_map(mention.get("id"));
                (!bot_open_id.is_empty()
                    && string_value(id.get("open_id")).as_deref() == Some(bot_open_id))
                    || (!bot_user_id.is_empty()
                        && string_value(id.get("user_id")).as_deref() == Some(bot_user_id))
            })
        } else {
            !mentions.is_empty()
        };
    let is_from_bot = string_value(sender.get("sender_type"))
        .map(|value| value.eq_ignore_ascii_case("bot"))
        .unwrap_or(false)
        || (!bot_open_id.is_empty() && sender_open_id == bot_open_id)
        || (!bot_user_id.is_empty() && sender_user_id == bot_user_id);
    let mention_names = mentions
        .iter()
        .filter_map(|mention| string_value(mention.get("name")))
        .collect::<Vec<_>>();
    let content = parse_message_content(message.get("content"));

    Some(FeishuImEvent {
        channel_id: params.channel_id,
        event_id: string_value(header.get("event_id"))
            .or_else(|| string_value(event.get("event_id")))
            .or_else(|| string_value(root.get("event_id")))
            .unwrap_or_default(),
        chat_id: string_value(message.get("chat_id")).unwrap_or_default(),
        message_id: string_value(message.get("message_id")).unwrap_or_default(),
        thread_id: string_value(message.get("thread_id"))
            .or_else(|| string_value(message.get("root_id")))
            .or_else(|| string_value(message.get("parent_id")))
            .unwrap_or_default(),
        chat_type: string_value(message.get("chat_type"))
            .or_else(|| string_value(event.get("chat_type")))
            .or_else(|| string_value(root.get("chat_type")))
            .unwrap_or_default(),
        sender_id: sender_open_id
            .clone()
            .or_else_non_empty(sender_user_id)
            .or_else_non_empty(string_value(sender.get("sender_id")).unwrap_or_default())
            .or_else_non_empty(string_value(sender.get("sender_id_str")).unwrap_or_default()),
        sender_name: string_value(sender.get("sender_name"))
            .or_else(|| string_value(sender.get("name")))
            .or_else(|| string_value(event.get("sender_name")))
            .or_else(|| string_value(root.get("sender_name")))
            .unwrap_or_default(),
        text: render_text_with_mentions(&content, &mention_names),
        mentioned_bot,
        mention_all,
        is_from_bot,
        action_form_values: Map::new(),
    })
}

fn normalize_ws_non_message_event(
    params: &ChannelStartRequest,
    raw: &Value,
    root: &Map<String, Value>,
    header: &Map<String, Value>,
    event: &Map<String, Value>,
) -> Option<FeishuImEvent> {
    let event_type = string_value(header.get("event_type"))
        .or_else(|| string_value(event.get("event_type")))
        .or_else(|| string_value(root.get("event_type")))
        .unwrap_or_default();
    let text = if event_type.contains("menu") {
        let key = string_value(event.get("event_key"))
            .or_else(|| string_value(event.get("key")))
            .or_else(|| string_value(root.get("event_key")))
            .unwrap_or_default();
        command_from_menu_key(&key)
    } else {
        command_from_card_action(raw, event)?
    };
    let operator = object_value(event.get("operator"))
        .or_else(|| object_value(root.get("operator")))
        .or_else(|| object_value(event.get("user")))
        .or_else(|| object_value(root.get("user")))
        .unwrap_or_default();
    let operator_id = object_value(operator.get("operator_id"))
        .or_else(|| object_value(operator.get("user_id")))
        .or_else(|| object_value(operator.get("id")))
        .unwrap_or_default();
    let sender_open_id = string_value(operator_id.get("open_id"))
        .or_else(|| string_value(operator.get("open_id")))
        .or_else(|| string_from_path(raw, &["event", "operator", "open_id"]))
        .unwrap_or_default();
    let sender_user_id = string_value(operator_id.get("user_id"))
        .or_else(|| string_value(operator.get("user_id")))
        .unwrap_or_default();
    let sender_id = sender_open_id
        .clone()
        .or_else_non_empty(sender_user_id)
        .or_else_non_empty(string_value(operator.get("operator_id")).unwrap_or_default())
        .or_else_non_empty(string_value(operator.get("id")).unwrap_or_default());

    Some(FeishuImEvent {
        channel_id: params.channel_id,
        event_id: string_value(header.get("event_id"))
            .or_else(|| string_value(event.get("event_id")))
            .or_else(|| string_value(root.get("event_id")))
            .unwrap_or_default(),
        chat_id: string_from_path(raw, &["event", "context", "open_chat_id"])
            .or_else(|| string_from_path(raw, &["event", "context", "chat_id"]))
            .or_else(|| string_from_path(raw, &["event", "open_chat_id"]))
            .or_else(|| string_value(event.get("chat_id")))
            .unwrap_or_default(),
        message_id: string_from_path(raw, &["event", "context", "open_message_id"])
            .or_else(|| string_from_path(raw, &["event", "context", "message_id"]))
            .or_else(|| string_from_path(raw, &["event", "open_message_id"]))
            .or_else(|| string_value(event.get("message_id")))
            .unwrap_or_default(),
        thread_id: String::new(),
        chat_type: string_from_path(raw, &["event", "context", "chat_type"])
            .or_else(|| string_value(event.get("chat_type")))
            .unwrap_or_else(|| "p2p".to_string()),
        sender_id,
        sender_name: string_value(operator.get("name"))
            .or_else(|| string_value(operator.get("sender_name")))
            .or_else(|| string_value(event.get("sender_name")))
            .unwrap_or_default(),
        text,
        mentioned_bot: true,
        mention_all: false,
        is_from_bot: false,
        action_form_values: card_action_form_values(raw, root, event),
    })
}

fn command_from_card_action(raw: &Value, event: &Map<String, Value>) -> Option<String> {
    [
        string_from_path(raw, &["event", "action", "value", "command"]),
        string_from_path(raw, &["event", "action", "value", "text_command"]),
        string_from_path(raw, &["event", "action", "value", "cmd"]),
        string_from_path(raw, &["event", "action", "value", "key"]),
        string_from_path(raw, &["event", "action", "name"]),
        string_from_path(raw, &["event", "action", "action_key"]),
        string_value(event.get("event_key")),
    ]
    .into_iter()
    .flatten()
    .map(|value| command_from_menu_key(&value))
    .next()
}

fn command_from_menu_key(key: &str) -> String {
    let normalized = key.trim();
    if normalized.is_empty() {
        return "菜单".to_string();
    }
    let lower = normalized.to_ascii_lowercase();
    if normalized.contains("订单") || lower.contains("order") {
        "订单列表".to_string()
    } else if normalized.contains("商品")
        || lower.contains("item")
        || lower.contains("goods")
        || lower.contains("product")
    {
        "商品列表".to_string()
    } else if normalized.contains("消息")
        || lower.contains("recent")
        || lower.contains("message")
        || lower == "im"
    {
        "最近消息".to_string()
    } else if normalized.contains("帮助") || lower.contains("help") {
        "帮助".to_string()
    } else if normalized.contains("绑定") || lower.contains("bind") {
        "绑定".to_string()
    } else if normalized.contains("菜单") || lower.contains("menu") {
        "菜单".to_string()
    } else {
        normalized.to_string()
    }
}

fn card_action_form_values(
    raw: &Value,
    root: &Map<String, Value>,
    event: &Map<String, Value>,
) -> Map<String, Value> {
    let action = object_value(event.get("action"))
        .or_else(|| object_value(root.get("action")))
        .unwrap_or_default();
    object_value(action.get("form_value"))
        .or_else(|| object_value(action.get("formValue")))
        .or_else(|| {
            raw.pointer("/event/action/form_value")
                .and_then(|value| object_value(Some(value)))
        })
        .or_else(|| {
            raw.pointer("/event/action/formValue")
                .and_then(|value| object_value(Some(value)))
        })
        .unwrap_or_default()
}

trait NonEmptyString {
    fn or_else_non_empty(self, fallback: String) -> String;
}

impl NonEmptyString for String {
    fn or_else_non_empty(self, fallback: String) -> String {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

fn parse_ws_connect_config(payload: &Value) -> Result<WsConnectConfig, FeishuNativeError> {
    let data = as_map(payload.get("data"));
    let url = string_value(data.get("URL"))
        .or_else(|| string_value(data.get("url")))
        .ok_or_else(|| {
            FeishuNativeError::Protocol("WebSocket endpoint response missing URL".to_string())
        })?;
    let client_config = as_map(data.get("ClientConfig"));
    Ok(WsConnectConfig {
        url,
        client_config: WsConnectClientConfig {
            ping_interval: client_config
                .get("PingInterval")
                .or_else(|| client_config.get("ping_interval"))
                .and_then(Value::as_u64),
        },
    })
}

fn service_id_from_url(value: &str) -> Option<i32> {
    let url = Url::parse(value).ok()?;
    url.query_pairs()
        .find(|(key, _)| key == "service_id")
        .and_then(|(_, value)| value.parse::<i32>().ok())
}

fn normalize_registration_domain(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "lark" | LARK_ACCOUNTS_HOST => LARK_REGISTRATION_DOMAIN,
        _ => FEISHU_REGISTRATION_DOMAIN,
    }
}

fn normalize_channel_domain(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    if looks_like_explicit_base_url(trimmed) {
        return trimmed.to_string();
    }
    normalize_registration_domain(trimmed).to_string()
}

fn normalize_domain_with_fallback<'a>(value: Option<&Value>, fallback: &'a str) -> &'a str {
    let Some(domain) = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return fallback;
    };
    normalize_registration_domain(domain)
}

fn accounts_host_for_domain(domain: &str) -> &'static str {
    if normalize_registration_domain(domain) == LARK_REGISTRATION_DOMAIN {
        LARK_ACCOUNTS_HOST
    } else {
        FEISHU_ACCOUNTS_HOST
    }
}

fn open_domain(domain: &str) -> Cow<'_, str> {
    let trimmed = domain.trim().trim_end_matches('/');
    if looks_like_explicit_base_url(trimmed) {
        return Cow::Owned(trimmed.to_string());
    }
    if normalize_registration_domain(domain) == LARK_REGISTRATION_DOMAIN {
        Cow::Borrowed("https://open.larksuite.com")
    } else {
        Cow::Borrowed("https://open.feishu.cn")
    }
}

pub fn feishu_console_addons_url(app_id: &str, domain: &str, scopes: &[String]) -> Option<String> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    let addons = json!({
        "scopes": {
            "tenant": scopes,
            "user": [],
        }
    });
    let raw = serde_json::to_vec(&addons).ok()?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&raw).ok()?;
    let compressed = encoder.finish().ok()?;
    let encoded = URL_SAFE_NO_PAD.encode(compressed);
    let host = if normalize_registration_domain(domain) == LARK_REGISTRATION_DOMAIN {
        "https://open.larksuite.com"
    } else {
        "https://open.feishu.cn"
    };
    Some(format!(
        "{host}/page/launcher?clientID={}&addons={encoded}",
        urlencoding::encode(app_id)
    ))
}

fn looks_like_explicit_base_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn normalized_message_type(value: &str) -> &'static str {
    if value.trim().eq_ignore_ascii_case("interactive") {
        "interactive"
    } else if value.trim().eq_ignore_ascii_case("post") {
        "post"
    } else {
        "text"
    }
}

fn normalized_member_id_type(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_member_id_type()
    } else {
        trimmed.to_string()
    }
}

fn is_pending_registration_response(value: &Value) -> bool {
    let error = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    error == "authorization_pending" || error == "slow_down"
}

fn has_lark_tenant_brand(data: &Map<String, Value>) -> bool {
    let user_info = object_value(data.get("user_info"))
        .or_else(|| object_value(data.get("userInfo")))
        .unwrap_or_default();
    let has_lark = [user_info.get("tenant_brand"), user_info.get("tenantBrand")]
        .into_iter()
        .filter_map(string_value)
        .any(|value| value.eq_ignore_ascii_case(LARK_REGISTRATION_DOMAIN));
    has_lark
}

fn parse_message_content(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            serde_json::from_str::<Value>(trimmed)
                .ok()
                .map(|value| parse_message_content(Some(&value)))
                .unwrap_or_else(|| trimmed.to_string())
        }
        Some(Value::Object(map)) => string_value(map.get("text")).unwrap_or_default(),
        _ => String::new(),
    }
}

fn render_text_with_mentions(text: &str, names: &[String]) -> String {
    let mut out = String::new();
    let mut rest = text;
    let mut index = 0usize;
    while let Some(start) = rest.find("<at") {
        out.push_str(&rest[..start]);
        let after_start = &rest[start..];
        if let Some(end) = after_start.find("</at>") {
            if let Some(name) = names.get(index) {
                if !name.trim().is_empty() {
                    out.push('@');
                    out.push_str(name.trim());
                }
            }
            index += 1;
            rest = &after_start[end + "</at>".len()..];
        } else {
            out.push_str(after_start);
            rest = "";
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_mention_all(mention: &Map<String, Value>) -> bool {
    let id = as_map(mention.get("id"));
    let mention_all = [
        mention.get("key"),
        mention.get("name"),
        id.get("open_id"),
        id.get("user_id"),
        id.get("union_id"),
    ]
    .into_iter()
    .filter_map(string_value)
    .any(|value| matches!(value.to_ascii_lowercase().as_str(), "@all" | "all"));
    mention_all
}

fn object_value(value: Option<&Value>) -> Option<Map<String, Value>> {
    match value {
        Some(Value::Object(map)) => Some(map.clone()),
        _ => None,
    }
}

fn as_map(value: Option<&Value>) -> Map<String, Value> {
    object_value(value).unwrap_or_default()
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn string_from_path(value: &Value, path: &[&str]) -> Option<String> {
    let mut cursor = value;
    for key in path {
        cursor = cursor.get(*key)?;
    }
    string_value(Some(cursor))
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn required_string(data: &Map<String, Value>, keys: &[&str]) -> Result<String, FeishuNativeError> {
    keys.iter()
        .find_map(|key| string_value(data.get(*key)))
        .ok_or_else(|| {
            FeishuNativeError::Protocol(format!(
                "Feishu registration response missing {}",
                keys.first().copied().unwrap_or("field")
            ))
        })
}

fn number_value(data: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| match data.get(*key) {
        Some(Value::Number(value)) => value.as_i64(),
        Some(Value::String(value)) => value.trim().parse::<i64>().ok(),
        _ => None,
    })
}

fn duration_ms_value(data: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    number_value(data, keys).map(|seconds| seconds * 1_000)
}

fn safe_api_code_suffix(payload: &Value) -> String {
    payload
        .get("error")
        .or_else(|| payload.get("code"))
        .and_then(|value| match value {
            Value::String(text) => Some(text.trim().to_string()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        })
        .filter(|value| !value.is_empty() && value.len() <= 80)
        .map(|value| format!(" ({value})"))
        .unwrap_or_default()
}

fn chat_id_from_payload(payload: &Value) -> Option<String> {
    string_from_path(payload, &["data", "chat_id"])
        .or_else(|| string_from_path(payload, &["data", "open_chat_id"]))
        .or_else(|| string_from_path(payload, &["chat_id"]))
        .or_else(|| string_from_path(payload, &["open_chat_id"]))
}

fn safe_message_suffix(payload: &Value) -> String {
    payload
        .get("msg")
        .or_else(|| payload.get("message"))
        .and_then(Value::as_str)
        .map(safe_worker_error_message)
        .filter(|value| !value.is_empty())
        .map(|value| format!(", msg={value}"))
        .unwrap_or_default()
}

fn clone_native_error(error: &FeishuNativeError) -> FeishuNativeError {
    match error {
        FeishuNativeError::Unavailable(message) => FeishuNativeError::Unavailable(message.clone()),
        FeishuNativeError::Protocol(message) => FeishuNativeError::Protocol(message.clone()),
        FeishuNativeError::Api(message) => FeishuNativeError::Api(message.clone()),
        FeishuNativeError::Serde(error) => FeishuNativeError::Protocol(error.to_string()),
    }
}

pub fn safe_native_error_detail(prefix: &str, detail: &str) -> String {
    let safe = safe_worker_error_message(detail);
    if safe.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}: {safe}")
    }
}

pub fn redact_native_error(error: &FeishuNativeError) -> String {
    safe_worker_error_message(&error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Json, Router};
    use futures_util::StreamExt;
    use serde_json::json;
    use tokio::{net::TcpListener, time::timeout};
    use tokio_tungstenite::accept_async;

    #[test]
    fn ws_frame_roundtrips_ping_protobuf_shape() {
        let frame = FeishuWsFrame::ping(7);
        let bytes = frame.encode_to_vec();
        let decoded = FeishuWsFrame::decode(bytes.as_slice()).unwrap();
        assert_eq!(decoded.service, 7);
        assert_eq!(decoded.method, FrameType::Control as i32);
        assert_eq!(
            decoded.headers_map().get("type").map(String::as_str),
            Some("ping")
        );
    }

    #[test]
    fn native_ws_event_normalizer_accepts_wrapped_and_flattened_event_shapes() {
        let params = ChannelStartRequest {
            channel_id: 9,
            app_id: "cli".to_string(),
            app_secret: "sec".to_string(),
            domain: "feishu".to_string(),
            bot_open_id: Some("ou_bot".to_string()),
            bot_user_id: None,
        };
        let wrapped = json!({
            "schema": "2.0",
            "header": { "event_id": "evt-1", "event_type": "im.message.receive_v1" },
            "event": {
                "sender": {
                    "sender_id": { "open_id": "ou_user" },
                    "sender_type": "user",
                    "sender_name": "Operator"
                },
                "message": {
                    "chat_id": "oc_chat",
                    "message_id": "om_1",
                    "chat_type": "group",
                    "content": "{\"text\":\"<at user_id=\\\"ou_bot\\\"></at> 商品列表\"}",
                    "mentions": [{ "id": { "open_id": "ou_bot" }, "name": "鱼管家" }]
                }
            }
        });
        let flattened = json!({
            "event_id": "evt-2",
            "sender": {
                "sender_id": { "open_id": "ou_user" },
                "sender_type": "user"
            },
            "message": {
                "chat_id": "oc_chat",
                "message_id": "om_2",
                "chat_type": "p2p",
                "content": "{\"text\":\"菜单\"}"
            }
        });

        let wrapped_event = normalize_ws_im_event(&params, &wrapped).unwrap();
        assert_eq!(wrapped_event.event_id, "evt-1");
        assert_eq!(wrapped_event.sender_id, "ou_user");
        assert!(wrapped_event.mentioned_bot);
        assert_eq!(wrapped_event.text, "@鱼管家 商品列表");

        let flat_event = normalize_ws_im_event(&params, &flattened).unwrap();
        assert_eq!(flat_event.event_id, "evt-2");
        assert_eq!(flat_event.message_id, "om_2");
        assert_eq!(flat_event.text, "菜单");
    }

    #[test]
    fn native_ws_event_normalizer_accepts_bot_menu_event() {
        let params = ChannelStartRequest {
            channel_id: 9,
            app_id: "cli".to_string(),
            app_secret: "sec".to_string(),
            domain: "feishu".to_string(),
            bot_open_id: Some("ou_bot".to_string()),
            bot_user_id: None,
        };
        let raw = json!({
            "schema": "2.0",
            "header": { "event_id": "evt-menu", "event_type": "application.bot.menu_v6" },
            "event": {
                "event_key": "items",
                "operator": {
                    "operator_id": { "open_id": "ou_operator", "user_id": "user_operator" },
                    "name": "Operator"
                }
            }
        });

        let event = normalize_ws_im_event(&params, &raw).unwrap();

        assert_eq!(event.event_id, "evt-menu");
        assert_eq!(event.sender_id, "ou_operator");
        assert_eq!(event.text, "商品列表");
        assert!(event.mentioned_bot);
        assert_eq!(event.chat_id, "");
    }

    #[test]
    fn native_ws_event_normalizer_accepts_card_action_command() {
        let params = ChannelStartRequest {
            channel_id: 9,
            app_id: "cli".to_string(),
            app_secret: "sec".to_string(),
            domain: "feishu".to_string(),
            bot_open_id: Some("ou_bot".to_string()),
            bot_user_id: None,
        };
        let raw = json!({
            "schema": "2.0",
            "header": { "event_id": "evt-card", "event_type": "card.action.trigger" },
            "event": {
                "operator": {
                    "operator_id": { "open_id": "ou_operator" },
                    "name": "Operator"
                },
                "context": {
                    "open_chat_id": "oc_card_chat",
                    "open_message_id": "om_card_source"
                },
                "action": {
                    "tag": "button",
                    "value": {
                        "command": "订单列表"
                    }
                }
            }
        });

        let event = normalize_ws_im_event(&params, &raw).unwrap();

        assert_eq!(event.event_id, "evt-card");
        assert_eq!(event.chat_id, "oc_card_chat");
        assert_eq!(event.message_id, "om_card_source");
        assert_eq!(event.sender_id, "ou_operator");
        assert_eq!(event.text, "订单列表");
        assert!(event.mentioned_bot);
    }

    #[test]
    fn native_ws_event_normalizer_keeps_card_form_values_for_reply_box() {
        let params = ChannelStartRequest {
            channel_id: 9,
            app_id: "cli".to_string(),
            app_secret: "sec".to_string(),
            domain: "feishu".to_string(),
            bot_open_id: Some("ou_bot".to_string()),
            bot_user_id: None,
        };
        let event = normalize_ws_im_event(
            &params,
            &json!({
                "header": { "event_id": "evt-card-input", "event_type": "card.action.trigger" },
                "event": {
                    "context": {
                        "open_chat_id": "oc_group_a",
                        "open_message_id": "om_group_card",
                        "chat_type": "group"
                    },
                    "operator": {
                        "operator_id": { "open_id": "ou_operator_1" },
                        "name": "Alice"
                    },
                    "action": {
                        "value": { "command": "group.reply.send" },
                        "form_value": { "reply_text": "你好，我在飞书群里回复" }
                    }
                }
            }),
        )
        .expect("card event should normalize");

        assert_eq!(event.chat_type, "group");
        assert_eq!(event.text, "group.reply.send");
        assert_eq!(
            event
                .action_form_values
                .get("reply_text")
                .and_then(Value::as_str),
            Some("你好，我在飞书群里回复")
        );
    }

    #[test]
    fn normalize_channel_start_request_preserves_explicit_base_url_domains() {
        let params = normalize_channel_start_request(ChannelStartRequest {
            channel_id: 77,
            app_id: "cli".to_string(),
            app_secret: "sec".to_string(),
            domain: "http://127.0.0.1:40123/".to_string(),
            bot_open_id: None,
            bot_user_id: None,
        })
        .expect("normalize request");

        assert_eq!(params.domain, "http://127.0.0.1:40123");
    }

    #[tokio::test]
    async fn reqwest_transport_start_channel_returns_once_websocket_connects() {
        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws_addr = ws_listener.local_addr().unwrap();
        let ws_task = tokio::spawn(async move {
            let (stream, _) = ws_listener.accept().await.unwrap();
            let ws = accept_async(stream).await.unwrap();
            let (_write, mut read) = ws.split();
            while let Some(message) = read.next().await {
                if message.is_err() {
                    break;
                }
            }
        });

        let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http_listener.local_addr().unwrap();
        let ws_url = format!("ws://{ws_addr}/?service_id=7");
        let http_app = Router::new().route(
            crate::sealed_protocol::sealed_str!("/callback/ws/endpoint"),
            post({
                let ws_url = ws_url.clone();
                move || {
                    let ws_url = ws_url.clone();
                    async move {
                        Json(json!({
                            "code": 0,
                            "msg": "",
                            "data": {
                                "URL": ws_url,
                                "ClientConfig": {
                                    "PingInterval": 90
                                }
                            }
                        }))
                    }
                }
            }),
        );
        let http_task = tokio::spawn(async move {
            axum::serve(http_listener, http_app).await.unwrap();
        });

        let transport = ReqwestFeishuNativeTransport::new(None);
        let result = timeout(
            Duration::from_secs(2),
            transport.start_channel(ChannelStartRequest {
                channel_id: 77,
                app_id: "cli".to_string(),
                app_secret: "sec".to_string(),
                domain: format!("http://{http_addr}"),
                bot_open_id: None,
                bot_user_id: None,
            }),
        )
        .await
        .expect("channel start should return once the websocket is connected")
        .expect("channel start result");

        assert_eq!(result.status, "started");
        assert_eq!(result.channel_id, 77);

        transport.stop_channel(77).await.unwrap();
        http_task.abort();
        ws_task.abort();
    }
}
