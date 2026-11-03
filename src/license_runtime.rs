use std::{
    collections::HashMap,
    fs,
    path::Path,
    process::Command,
    sync::{OnceLock, RwLock},
    time::Duration,
};

use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::random;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::control_plane_tls::verify_response_pin;

const LICENSE_PUBKEY_HEX: Option<&str> = option_env!("LICENSE_PUBKEY_HEX");
pub fn client_version_header() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-yuguanjia-client-version")
}
pub fn client_platform_header() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-yuguanjia-client-platform")
}
pub fn default_update_policy_url() -> &'static str {
    crate::sealed_protocol::sealed_str!("https://download.gooofish.shop/api/client/update-policy")
}
fn license_service_host() -> &'static str {
    crate::sealed_protocol::sealed_str!("license.gooofish.shop")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignLicenseContext {
    pub server_url: String,
    pub auth_token: String,
    pub signed_token: String,
    pub license_id: i64,
    pub username: String,
    pub sku: String,
    pub account_limit: i64,
    /// Legacy bootstrap secret. Kept only for fallback registration/bootstrap
    /// and compatibility with older servers that have not enabled device-key
    /// proofs yet.
    pub hmac_secret_hex: String,
    pub device_id: String,
}

pub fn default_account_limit_for_sku(sku: &str) -> i64 {
    match sku.trim().to_ascii_lowercase().as_str() {
        "admin" => 999,
        "trial" => 2,
        "yearly" => 30,
        _ => 10,
    }
}

pub fn normalize_account_limit(sku: &str, account_limit: i64) -> i64 {
    if account_limit > 0 {
        account_limit
    } else {
        default_account_limit_for_sku(sku)
    }
}

const DEVICE_KEY_FILE: &str = "device_ed25519.key";
const DEVICE_KEY_REGISTRATION_MARKER_FILE: &str = "device_key_registered.json";
fn device_key_register_path() -> &'static str {
    crate::sealed_protocol::sealed_str!("/api/licenses/device-key/register")
}
fn protocol_unlock_path() -> &'static str {
    crate::sealed_protocol::sealed_str!("/api/licenses/protocol-unlock")
}
const PROTOCOL_UNLOCK_REFRESH_SKEW_SECS: u64 = 60;
const SHARED_DEVICE_KEY_DIR_ENV: &str = "XIANYU_SHARED_DEVICE_KEY_DIR";
const SHARED_DEVICE_KEY_APP_DIR: &str = "shop.gooofish.yuguanjia.shared";
const SHARED_DEVICE_KEY_SUBDIR: &str = "license-runtime";
fn device_header_public_key() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-public-key")
}
fn device_header_signature() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-signature")
}
fn device_header_timestamp() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-ts")
}
fn device_header_nonce() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-proof-nonce")
}
fn device_header_license_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-id")
}
fn device_header_device_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-license-device-id")
}
fn device_header_key_id() -> &'static str {
    crate::sealed_protocol::sealed_str!("x-device-key-id")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceKeypair {
    signing_key: SigningKey,
}

impl DeviceKeypair {
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.signing_key.verifying_key().to_bytes())
    }

    pub fn sign_message(&self, message: &[u8]) -> String {
        hex::encode(self.signing_key.sign(message).to_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceProof {
    pub public_key_hex: String,
    pub signature_hex: String,
    pub timestamp: i64,
    pub nonce: String,
    pub license_id: i64,
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofMode {
    Device(DeviceProof),
    LegacyLicenseHmac {
        timestamp: i64,
        nonce: String,
        signature_hex: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct DeviceKeyRegistrationMarker {
    license_id: i64,
    device_id: String,
    public_key_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolUnlockKey {
    pub key: [u8; 32],
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProtocolUnlockCacheKey {
    license_id: i64,
    device_id: String,
    protocol_version: String,
    capability: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolUnlockRequest<'a> {
    protocol_version: &'a str,
    capability: &'a str,
    device_id: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProtocolUnlockResponse {
    protocol_key: String,
    expires_at: String,
}

pub fn infer_license_server_url(data_dir: &Path, config_default: &str) -> String {
    if let Some(context) = get_sign_license_context() {
        return context.server_url;
    }

    if let Ok(Some(context)) = load_sign_license_context(data_dir, None, now_unix()) {
        return context.server_url;
    }

    config_default.trim().trim_end_matches('/').to_string()
}

pub fn resolve_update_policy_url(configured: Option<String>) -> String {
    configured
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| !uses_license_service_host(value))
        .unwrap_or_else(|| default_update_policy_url().to_string())
}

fn uses_license_service_host(value: &str) -> bool {
    match url::Url::parse(value.trim()) {
        Ok(url) => url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(license_service_host())),
        Err(_) => value
            .trim()
            .to_ascii_lowercase()
            .contains(license_service_host()),
    }
}

pub fn current_client_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn current_client_platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else {
        "desktop"
    }
}

pub fn apply_client_version_headers(builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    builder
        .header(client_version_header(), current_client_version())
        .header(client_platform_header(), current_client_platform())
}

#[derive(Debug, Deserialize)]
struct PersistedLicenseBundle {
    server_url: String,
    auth_token: String,
    signed_token: String,
    license_id: i64,
    #[serde(default)]
    username: String,
    #[serde(default)]
    sku: String,
    #[serde(default)]
    account_limit: i64,
    hmac_secret_hex: String,
    #[serde(default)]
    expires_unix: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct EmbeddedLicensePayload {
    lid: i64,
    #[serde(default)]
    sku: String,
    exp: u64,
    hs: String,
    #[serde(default)]
    account_limit: i64,
}

fn slot() -> &'static RwLock<Option<SignLicenseContext>> {
    static SLOT: OnceLock<RwLock<Option<SignLicenseContext>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

fn protocol_unlock_slot() -> &'static RwLock<HashMap<ProtocolUnlockCacheKey, ProtocolUnlockKey>> {
    static SLOT: OnceLock<RwLock<HashMap<ProtocolUnlockCacheKey, ProtocolUnlockKey>>> =
        OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn set_sign_license_context(context: Option<SignLicenseContext>) {
    if let Ok(mut guard) = slot().write() {
        *guard = context;
    }
    if let Ok(mut guard) = protocol_unlock_slot().write() {
        guard.clear();
    }
}

pub fn get_sign_license_context() -> Option<SignLicenseContext> {
    slot().read().ok().and_then(|guard| guard.clone())
}

pub fn get_protocol_unlock_key(
    protocol_version: &str,
    capability: &str,
) -> Option<ProtocolUnlockKey> {
    let context = get_sign_license_context()?;
    let cache_key = protocol_unlock_cache_key(&context, protocol_version, capability)?;
    let unlock = protocol_unlock_slot()
        .read()
        .ok()
        .and_then(|guard| guard.get(&cache_key).cloned())?;
    (unlock.expires_at > now_unix().saturating_add(PROTOCOL_UNLOCK_REFRESH_SKEW_SECS))
        .then_some(unlock)
}

pub async fn refresh_protocol_unlock_key(
    client: &reqwest::Client,
    data_dir: &Path,
    protocol_version: &str,
    capability: &str,
) -> Result<ProtocolUnlockKey, String> {
    let context = get_sign_license_context()
        .ok_or_else(|| crate::sealed_protocol::sealed_str!("E_PROTOCOL_CONTEXT").to_string())?;
    let cache_key = protocol_unlock_cache_key(&context, protocol_version, capability)
        .ok_or_else(|| "protocol version and capability are required".to_string())?;
    let refresh_deadline = now_unix().saturating_add(PROTOCOL_UNLOCK_REFRESH_SKEW_SECS);
    if let Some(cached) = protocol_unlock_slot()
        .read()
        .ok()
        .and_then(|guard| guard.get(&cache_key).cloned())
        .filter(|unlock| unlock.expires_at > refresh_deadline)
    {
        return Ok(cached);
    }

    if !ensure_device_key_registered(client, &context.server_url, &context, data_dir).await? {
        return Err(crate::sealed_protocol::sealed_str!("E_PROTOCOL_DEVICE_KEY").to_string());
    }

    let request = ProtocolUnlockRequest {
        protocol_version: protocol_version.trim(),
        capability: capability.trim(),
        device_id: &context.device_id,
    };
    let body = serde_json::to_vec(&request).map_err(|error| {
        format!(
            "{}: {error}",
            crate::sealed_protocol::sealed_str!("E_PROTOCOL_REQUEST_JSON")
        )
    })?;
    let proof = build_device_proof(data_dir, &context, "POST", protocol_unlock_path(), &body)?;
    let response = apply_client_version_headers(
        client
            .post(format!(
                "{}{}",
                context.server_url.trim_end_matches('/'),
                protocol_unlock_path()
            ))
            .bearer_auth(&context.auth_token)
            .header("content-type", "application/json")
            .header(device_header_public_key(), &proof.public_key_hex)
            .header(device_header_signature(), &proof.signature_hex)
            .header(device_header_timestamp(), proof.timestamp.to_string())
            .header(device_header_nonce(), &proof.nonce)
            .header(device_header_key_id(), proof.license_id.to_string())
            .header(device_header_device_id(), &proof.device_id)
            .body(body),
    )
    .send()
    .await
    .map_err(|error| {
        format!(
            "{}: {error}",
            crate::sealed_protocol::sealed_str!("E_PROTOCOL_REQUEST")
        )
    })?;
    let status = response.status();
    let response_body = response.bytes().await.map_err(|error| {
        format!(
            "{}: {error}",
            crate::sealed_protocol::sealed_str!("E_PROTOCOL_RESPONSE_READ")
        )
    })?;
    if !status.is_success() {
        return Err(format!(
            "{} {}: {}",
            crate::sealed_protocol::sealed_str!("E_PROTOCOL_HTTP"),
            status.as_u16(),
            String::from_utf8_lossy(&response_body)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    let payload: ProtocolUnlockResponse =
        serde_json::from_slice(&response_body).map_err(|error| {
            format!(
                "{}: {error}",
                crate::sealed_protocol::sealed_str!("E_PROTOCOL_RESPONSE_JSON")
            )
        })?;
    let key_bytes = hex::decode(payload.protocol_key.trim()).map_err(|error| {
        format!(
            "{}: {error}",
            crate::sealed_protocol::sealed_str!("E_PROTOCOL_KEY_HEX")
        )
    })?;
    let key: [u8; 32] = key_bytes.try_into().map_err(|bytes: Vec<u8>| {
        format!(
            "{} {}",
            crate::sealed_protocol::sealed_str!("E_PROTOCOL_KEY_LENGTH"),
            bytes.len()
        )
    })?;
    let expires_at = parse_rfc3339_unix(&payload.expires_at).ok_or_else(|| {
        crate::sealed_protocol::sealed_str!("E_PROTOCOL_EXPIRY_INVALID").to_string()
    })?;
    if expires_at <= now_unix() {
        return Err(crate::sealed_protocol::sealed_str!("E_PROTOCOL_KEY_EXPIRED").to_string());
    }
    let unlock = ProtocolUnlockKey { key, expires_at };
    protocol_unlock_slot()
        .write()
        .map_err(|_| crate::sealed_protocol::sealed_str!("E_PROTOCOL_CACHE").to_string())?
        .insert(cache_key, unlock.clone());
    Ok(unlock)
}

fn protocol_unlock_cache_key(
    context: &SignLicenseContext,
    protocol_version: &str,
    capability: &str,
) -> Option<ProtocolUnlockCacheKey> {
    let protocol_version = protocol_version.trim();
    let capability = capability.trim();
    if protocol_version.is_empty() || capability.is_empty() {
        return None;
    }
    Some(ProtocolUnlockCacheKey {
        license_id: context.license_id,
        device_id: context.device_id.clone(),
        protocol_version: protocol_version.to_string(),
        capability: capability.to_string(),
    })
}

pub async fn ensure_sign_license_context_from_disk(data_dir: &Path, control_plane_pins: &[String]) {
    if get_sign_license_context().is_some() {
        return;
    }

    match load_sign_license_context(data_dir, None, now_unix()) {
        Ok(Some(context)) => {
            info!(
                server_url = %context.server_url,
                device_id = %context.device_id,
                "sign license context restored from local bundle"
            );
            set_sign_license_context(Some(context));
        }
        Ok(None) => {}
        Err(error) => {
            match restore_legacy_sign_license_context(data_dir, control_plane_pins).await {
                Ok(Some(context)) => {
                    info!(
                        server_url = %context.server_url,
                        device_id = %context.device_id,
                        "sign license context restored by upgrading legacy bundle"
                    );
                    set_sign_license_context(Some(context));
                }
                Ok(None) => {
                    warn!(
                        data_dir = %data_dir.display(),
                        %error,
                        "failed to restore sign license context from local bundle"
                    );
                }
                Err(restore_error) => {
                    warn!(
                        data_dir = %data_dir.display(),
                        %error,
                        restore_error = %restore_error,
                        "failed to restore sign license context from local bundle"
                    );
                }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct LegacyPersistedLicenseBundle {
    server_url: String,
    auth_token: String,
    license_key: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    expires_unix: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct LegacyActivateResponse {
    signed_token: String,
    license: LegacyLicenseRowJson,
    hmac_secret_hex: String,
}

#[derive(Debug, Deserialize)]
struct LegacyLicenseRowJson {
    id: i64,
    sku: String,
    license_key: String,
    issued_at: String,
    expires_at: String,
    #[serde(default)]
    account_limit: i64,
}

#[derive(Debug, Deserialize)]
struct AuthTokenClaims {
    sub: i64,
    #[serde(default)]
    username: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpgradedPersistedLicenseBundle {
    server_url: String,
    auth_token: String,
    license_key: String,
    license_id: i64,
    user_id: i64,
    username: String,
    sku: String,
    account_limit: i64,
    signed_token: String,
    hmac_secret_hex: String,
    issued_at: String,
    expires_at: String,
    expires_unix: u64,
    activated_at_unix: u64,
}

async fn restore_legacy_sign_license_context(
    data_dir: &Path,
    control_plane_pins: &[String],
) -> Result<Option<SignLicenseContext>, String> {
    let bundle_path = data_dir.join("license.json");
    if !bundle_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&bundle_path)
        .map_err(|error| format!("read {}: {error}", bundle_path.display()))?;
    let legacy: LegacyPersistedLicenseBundle = match serde_json::from_str(&raw) {
        Ok(bundle) => bundle,
        Err(_) => return Ok(None),
    };

    let server_url = legacy.server_url.trim().trim_end_matches('/').to_string();
    if server_url.is_empty() {
        return Ok(None);
    }
    let auth_token = legacy.auth_token.trim().to_string();
    let license_key = legacy.license_key.trim().to_string();
    if auth_token.is_empty() || license_key.is_empty() {
        return Ok(None);
    }

    let device_id = resolve_device_id(data_dir, None)?;
    let activate = reactivate_legacy_bundle(
        &server_url,
        &auth_token,
        &license_key,
        &device_id,
        control_plane_pins,
    )
    .await?;
    let auth_claims = parse_auth_token_claims(&auth_token)?;
    let upgraded = build_upgraded_bundle(legacy, activate, auth_claims)?;

    write_upgraded_bundle(&bundle_path, &upgraded)?;

    Ok(Some(SignLicenseContext {
        server_url: upgraded.server_url,
        auth_token: upgraded.auth_token,
        signed_token: upgraded.signed_token,
        license_id: upgraded.license_id,
        username: upgraded.username,
        sku: upgraded.sku,
        account_limit: upgraded.account_limit,
        hmac_secret_hex: upgraded.hmac_secret_hex,
        device_id,
    }))
}

async fn reactivate_legacy_bundle(
    server_url: &str,
    auth_token: &str,
    license_key: &str,
    device_id: &str,
    control_plane_pins: &[String],
) -> Result<LegacyActivateResponse, String> {
    let url = format!(
        "{server_url}{}",
        crate::sealed_protocol::sealed_str!("/api/licenses/activate")
    );
    let client = legacy_activate_client()
        .map_err(|error| format!("activate client init failed: {error}"))?;

    let response = client
        .post(&url)
        .header(client_version_header(), current_client_version())
        .header(client_platform_header(), current_client_platform())
        .bearer_auth(auth_token)
        .json(&serde_json::json!({
            "license_key": license_key,
            "device_id": device_id,
        }))
        .send()
        .await
        .map_err(|error| format!("legacy activate request failed: {error}"))?;
    verify_response_pin(&response, control_plane_pins, "license_service", &url)
        .map_err(|error| format!("legacy activate pin verify failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "legacy activate returned HTTP {}: {}",
            status.as_u16(),
            body.chars().take(300).collect::<String>()
        ));
    }
    response
        .json::<LegacyActivateResponse>()
        .await
        .map_err(|error| format!("legacy activate JSON parse failed: {error}"))
}

fn legacy_activate_client() -> Result<reqwest::Client, reqwest::Error> {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .tls_info(true)
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(8));
    #[cfg(not(test))]
    let builder = builder.https_only(true);
    builder.build()
}

fn parse_auth_token_claims(auth_token: &str) -> Result<AuthTokenClaims, String> {
    let mut parts = auth_token.split('.');
    let _header = parts
        .next()
        .ok_or_else(|| "auth token missing header".to_string())?;
    let payload = parts
        .next()
        .ok_or_else(|| "auth token missing payload".to_string())?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|error| format!("auth token payload b64 decode failed: {error}"))?;
    let claims: AuthTokenClaims = serde_json::from_slice(&payload)
        .map_err(|error| format!("auth token payload JSON parse failed: {error}"))?;
    if claims.sub <= 0 {
        return Err("auth token missing valid sub".to_string());
    }
    Ok(claims)
}

fn build_upgraded_bundle(
    legacy: LegacyPersistedLicenseBundle,
    activate: LegacyActivateResponse,
    auth_claims: AuthTokenClaims,
) -> Result<UpgradedPersistedLicenseBundle, String> {
    let expires_unix = parse_rfc3339_unix(&activate.license.expires_at)
        .or(legacy.expires_unix)
        .ok_or_else(|| "upgraded bundle missing valid expires_unix".to_string())?;
    let username = {
        let auth_username = auth_claims.username.trim();
        if !auth_username.is_empty() {
            auth_username.to_string()
        } else {
            let legacy_username = legacy.username.trim();
            if legacy_username.is_empty() {
                return Err("upgraded bundle missing username".to_string());
            }
            legacy_username.to_string()
        }
    };

    let sku = activate.license.sku;
    let account_limit = normalize_account_limit(&sku, activate.license.account_limit);

    Ok(UpgradedPersistedLicenseBundle {
        server_url: legacy.server_url.trim().trim_end_matches('/').to_string(),
        auth_token: legacy.auth_token.trim().to_string(),
        license_key: activate.license.license_key,
        license_id: activate.license.id,
        user_id: auth_claims.sub,
        username,
        sku,
        account_limit,
        signed_token: activate.signed_token,
        hmac_secret_hex: activate.hmac_secret_hex,
        issued_at: activate.license.issued_at,
        expires_at: activate.license.expires_at,
        expires_unix,
        activated_at_unix: now_unix(),
    })
}

fn write_upgraded_bundle(
    path: &Path,
    bundle: &UpgradedPersistedLicenseBundle,
) -> Result<(), String> {
    let tmp_path = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(bundle)
        .map_err(|error| format!("serialize upgraded bundle: {error}"))?;
    fs::write(&tmp_path, bytes)
        .map_err(|error| format!("write {}: {error}", tmp_path.display()))?;
    fs::rename(&tmp_path, path).map_err(|error| {
        format!(
            "rename {} -> {}: {error}",
            tmp_path.display(),
            path.display()
        )
    })?;
    Ok(())
}

fn parse_rfc3339_unix(value: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|dt| u64::try_from(dt.timestamp()).ok())
}

fn load_sign_license_context(
    data_dir: &Path,
    device_id_override: Option<&str>,
    now_unix: u64,
) -> Result<Option<SignLicenseContext>, String> {
    if let Some(context) = load_sign_license_context_from_env(data_dir, device_id_override)? {
        return Ok(Some(context));
    }

    let bundle_path = data_dir.join("license.json");
    if !bundle_path.exists() {
        return Ok(None);
    }

    let raw = fs::read_to_string(&bundle_path)
        .map_err(|error| format!("read {}: {error}", bundle_path.display()))?;
    let bundle: PersistedLicenseBundle =
        serde_json::from_str(&raw).map_err(|error| format!("parse bundle JSON: {error}"))?;

    let server_url = bundle.server_url.trim().trim_end_matches('/').to_string();
    if server_url.is_empty() {
        return Err(format!("{} missing server_url", bundle_path.display()));
    }

    let auth_token = bundle.auth_token.trim().to_string();
    if auth_token.is_empty() {
        return Err(format!("{} missing auth_token", bundle_path.display()));
    }
    let signed_token = bundle.signed_token.trim();
    if signed_token.is_empty() {
        return Err(format!("{} missing signed_token", bundle_path.display()));
    }
    let claims = trusted_claims_for_persisted_bundle(&bundle, &bundle_path)?;

    if claims.expires_unix <= now_unix {
        return Err(format!(
            "{} expired at {}, current time {now_unix}",
            bundle_path.display(),
            claims.expires_unix
        ));
    }

    let username = resolve_context_username(&bundle.username, &auth_token);
    let device_id = resolve_device_id(data_dir, device_id_override)?;
    Ok(Some(SignLicenseContext {
        server_url,
        auth_token,
        signed_token: signed_token.to_string(),
        license_id: claims.license_id,
        username,
        sku: claims.sku,
        account_limit: claims.account_limit,
        hmac_secret_hex: claims.hmac_secret_hex,
        device_id,
    }))
}

struct TrustedPersistedClaims {
    license_id: i64,
    sku: String,
    account_limit: i64,
    hmac_secret_hex: String,
    expires_unix: u64,
}

fn trusted_claims_for_persisted_bundle(
    bundle: &PersistedLicenseBundle,
    bundle_path: &Path,
) -> Result<TrustedPersistedClaims, String> {
    let payload = trusted_payload_from_token(bundle.signed_token.trim())?;
    if bundle.license_id != payload.lid {
        return Err(format!("{} license_id mismatch", bundle_path.display()));
    }
    let sku = payload.sku.trim().to_string();
    if sku.is_empty() {
        return Err(format!(
            "{} signed token missing sku",
            bundle_path.display()
        ));
    }
    if !bundle.sku.trim().is_empty() && bundle.sku.trim() != sku {
        return Err(format!("{} sku mismatch", bundle_path.display()));
    }
    let account_limit = normalize_account_limit(&sku, payload.account_limit);
    if normalize_account_limit(&sku, bundle.account_limit) != account_limit {
        return Err(format!("{} account_limit mismatch", bundle_path.display()));
    }
    let hmac_secret_hex = payload.hs.trim().to_string();
    if hmac_secret_hex.is_empty() {
        return Err(format!(
            "{} signed token missing hmac_secret",
            bundle_path.display()
        ));
    }
    if bundle.hmac_secret_hex.trim() != hmac_secret_hex {
        return Err(format!("{} hmac_secret mismatch", bundle_path.display()));
    }
    if let Some(expires_unix) = bundle.expires_unix {
        if expires_unix != payload.exp {
            return Err(format!("{} expires_unix mismatch", bundle_path.display()));
        }
    }
    Ok(TrustedPersistedClaims {
        license_id: payload.lid,
        sku,
        account_limit,
        hmac_secret_hex,
        expires_unix: payload.exp,
    })
}

fn resolve_context_username(bundle_username: &str, auth_token: &str) -> String {
    let bundled = bundle_username.trim();
    if !bundled.is_empty() {
        return bundled.to_string();
    }
    parse_auth_token_claims(auth_token)
        .ok()
        .map(|claims| claims.username.trim().to_string())
        .filter(|username| !username.is_empty())
        .unwrap_or_default()
}

fn trusted_payload_from_token(token: &str) -> Result<EmbeddedLicensePayload, String> {
    match verify_signed_token_offline(token) {
        Ok(payload) => Ok(payload),
        #[cfg(test)]
        Err(_) if LICENSE_PUBKEY_HEX.unwrap_or("").trim().is_empty() => {
            parse_unverified_payload(token)
        }
        Err(error) => Err(error),
    }
}

fn verify_signed_token_offline(token: &str) -> Result<EmbeddedLicensePayload, String> {
    let pubkey_hex = LICENSE_PUBKEY_HEX.unwrap_or("").trim();
    if pubkey_hex.is_empty() {
        return Err("license signed token verification failed: no embedded pubkey".to_string());
    }
    let pubkey_bytes =
        hex::decode(pubkey_hex).map_err(|error| format!("license pubkey hex: {error}"))?;
    let pubkey_arr: [u8; 32] = pubkey_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "license pubkey must be 32 bytes".to_string())?;
    let verifier = VerifyingKey::from_bytes(&pubkey_arr)
        .map_err(|error| format!("license pubkey parse: {error}"))?;

    let (body_b64, sig_b64) = token
        .split_once('.')
        .ok_or_else(|| "license signed token format".to_string())?;
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body_b64)
        .map_err(|error| format!("license signed token body b64: {error}"))?;
    let sig = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|error| format!("license signed token sig b64: {error}"))?;
    let sig_arr: [u8; 64] = sig
        .as_slice()
        .try_into()
        .map_err(|_| "license signed token sig must be 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&sig_arr);
    verifier
        .verify(&body, &signature)
        .map_err(|error| format!("license signed token verify: {error}"))?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("license signed token JSON parse: {error}"))
}

fn parse_unverified_payload(token: &str) -> Result<EmbeddedLicensePayload, String> {
    let (body_b64, _) = token
        .split_once('.')
        .ok_or_else(|| "license signed token format".to_string())?;
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body_b64)
        .map_err(|error| format!("license signed token body b64: {error}"))?;
    serde_json::from_slice(&body)
        .map_err(|error| format!("license signed token JSON parse: {error}"))
}

fn load_sign_license_context_from_env(
    data_dir: &Path,
    device_id_override: Option<&str>,
) -> Result<Option<SignLicenseContext>, String> {
    let Some(server_url) = std::env::var("SIGN_LICENSE_SERVER_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let auth_token = std::env::var("SIGN_LICENSE_AUTH_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "SIGN_LICENSE_AUTH_TOKEN is required when SIGN_LICENSE_SERVER_URL is set".to_string()
        })?;
    let signed_token = std::env::var("SIGN_LICENSE_SIGNED_TOKEN")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "SIGN_LICENSE_SIGNED_TOKEN is required when SIGN_LICENSE_SERVER_URL is set".to_string()
        })?;
    let license_id = std::env::var("SIGN_LICENSE_ID")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| {
            "SIGN_LICENSE_ID is required when SIGN_LICENSE_SERVER_URL is set".to_string()
        })?;
    let hmac_secret_hex = std::env::var("SIGN_LICENSE_HMAC_SECRET_HEX")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "SIGN_LICENSE_HMAC_SECRET_HEX is required when SIGN_LICENSE_SERVER_URL is set"
                .to_string()
        })?;
    let sku = std::env::var("SIGN_LICENSE_SKU")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "monthly".to_string());
    let account_limit = std::env::var("SIGN_LICENSE_ACCOUNT_LIMIT")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .map(|value| normalize_account_limit(&sku, value))
        .unwrap_or_else(|| default_account_limit_for_sku(&sku));
    let username = std::env::var("SIGN_LICENSE_USERNAME")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| resolve_context_username("", &auth_token));

    let device_id = resolve_device_id(data_dir, device_id_override)?;
    Ok(Some(SignLicenseContext {
        server_url,
        auth_token,
        signed_token,
        license_id,
        username,
        sku,
        account_limit,
        hmac_secret_hex,
        device_id,
    }))
}

fn resolve_device_id(data_dir: &Path, device_id_override: Option<&str>) -> Result<String, String> {
    if let Some(value) = std::env::var("SIGN_LICENSE_DEVICE_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(value);
    }

    if let Some(value) = device_id_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(value.to_string());
    }

    if let Some(device_id) = discover_macos_device_id() {
        return Ok(device_id);
    }

    load_or_create_fallback_device_id(data_dir)
}

fn discover_macos_device_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("/usr/sbin/ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some(idx) = line.find("\"IOPlatformUUID\"") {
                let tail = &line[idx..];
                if let Some(eq) = tail.find('=') {
                    let raw = tail[eq + 1..].trim();
                    let raw = raw.trim_matches(|c: char| c == '"' || c.is_whitespace());
                    if !raw.is_empty() {
                        return Some(sha256_hex(
                            format!("macos:ioplatform_uuid:{raw}").as_bytes(),
                        ));
                    }
                }
            }
        }
    }
    None
}

fn load_or_create_fallback_device_id(data_dir: &Path) -> Result<String, String> {
    let path = data_dir.join("device_id_fallback.txt");
    if let Ok(raw) = fs::read_to_string(&path) {
        let raw = raw.trim();
        if !raw.is_empty() {
            return Ok(sha256_hex(
                format!("{}:fallback:{raw}", runtime_platform()).as_bytes(),
            ));
        }
    }

    let raw = random::<[u8; 16]>()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    fs::create_dir_all(data_dir)
        .map_err(|error| format!("create {}: {error}", data_dir.display()))?;
    fs::write(&path, &raw).map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(sha256_hex(
        format!("{}:fallback:{raw}", runtime_platform()).as_bytes(),
    ))
}

fn runtime_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "unknown"
    }
}

pub fn canonical_device_message(
    method: &str,
    path: &str,
    body_sha256_hex: &str,
    timestamp: i64,
    nonce: &str,
    license_id: i64,
    device_id: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.trim().to_uppercase(),
        path.trim(),
        body_sha256_hex.trim().to_lowercase(),
        timestamp,
        nonce.trim(),
        license_id,
        device_id.trim(),
    )
}

pub fn load_or_create_device_keypair(data_dir: &Path) -> Result<DeviceKeypair, String> {
    let storage_dir = ensure_device_key_storage_dir_ready(data_dir)?;
    load_or_create_device_keypair_from_storage_dir(data_dir, &storage_dir)
}

fn load_or_create_device_keypair_for_context(
    data_dir: &Path,
    context: &SignLicenseContext,
) -> Result<DeviceKeypair, String> {
    let storage_dir = ensure_device_key_storage_dir_ready_for_context(data_dir, context)?;
    load_or_create_device_keypair_from_storage_dir(data_dir, &storage_dir)
}

fn load_or_create_device_keypair_from_storage_dir(
    data_dir: &Path,
    storage_dir: &Path,
) -> Result<DeviceKeypair, String> {
    let path = storage_dir.join(DEVICE_KEY_FILE);
    if let Ok(raw) = fs::read_to_string(&path) {
        let raw = raw.trim();
        let bytes =
            hex::decode(raw).map_err(|error| format!("decode {}: {error}", path.display()))?;
        if bytes.len() != 32 {
            return Err(format!(
                "{} expected 32 byte seed, got {} bytes",
                path.display(),
                bytes.len()
            ));
        }
        let mut seed = [0u8; 32];
        seed.copy_from_slice(&bytes);
        return Ok(DeviceKeypair {
            signing_key: SigningKey::from_bytes(&seed),
        });
    }

    fs::create_dir_all(data_dir)
        .map_err(|error| format!("create {}: {error}", data_dir.display()))?;
    let seed = random::<[u8; 32]>();
    fs::write(&path, hex::encode(seed))
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(DeviceKeypair {
        signing_key: SigningKey::from_bytes(&seed),
    })
}

fn shared_device_key_dir_override_from_env() -> Option<std::path::PathBuf> {
    std::env::var_os(SHARED_DEVICE_KEY_DIR_ENV)
        .map(std::path::PathBuf::from)
        .filter(|value| !value.as_os_str().is_empty())
}

fn default_shared_device_key_storage_dir() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            std::path::PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(SHARED_DEVICE_KEY_APP_DIR)
                .join(SHARED_DEVICE_KEY_SUBDIR),
        )
    }

    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
        Some(
            std::path::PathBuf::from(base)
                .join(SHARED_DEVICE_KEY_APP_DIR)
                .join(SHARED_DEVICE_KEY_SUBDIR),
        )
    }

    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "macos"),
        not(target_os = "windows")
    ))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Some(
                std::path::PathBuf::from(xdg)
                    .join(SHARED_DEVICE_KEY_APP_DIR)
                    .join(SHARED_DEVICE_KEY_SUBDIR),
            );
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Some(
                std::path::PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join(SHARED_DEVICE_KEY_APP_DIR)
                    .join(SHARED_DEVICE_KEY_SUBDIR),
            );
        }
        None
    }

    #[cfg(target_os = "android")]
    None
}

fn resolve_device_key_storage_dir_with_override(
    data_dir: &Path,
    override_dir: Option<std::path::PathBuf>,
) -> std::path::PathBuf {
    override_dir
        .or_else(default_shared_device_key_storage_dir)
        .unwrap_or_else(|| data_dir.to_path_buf())
}

fn resolve_device_key_storage_dir(data_dir: &Path) -> std::path::PathBuf {
    let override_dir = shared_device_key_dir_override_from_env();
    if cfg!(test) && override_dir.is_none() {
        return data_dir.to_path_buf();
    }
    resolve_device_key_storage_dir_with_override(data_dir, override_dir)
}

fn load_device_key_registration_marker_from_dir(
    storage_dir: &Path,
) -> Option<DeviceKeyRegistrationMarker> {
    let path = storage_dir.join(DEVICE_KEY_REGISTRATION_MARKER_FILE);
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<DeviceKeyRegistrationMarker>(&raw).ok()
}

fn local_device_artifacts_match_context(data_dir: &Path, context: &SignLicenseContext) -> bool {
    let marker = load_device_key_registration_marker_from_dir(data_dir);
    let key_exists = data_dir.join(DEVICE_KEY_FILE).exists();
    matches!(
        marker,
        Some(DeviceKeyRegistrationMarker {
            license_id,
            device_id,
            public_key_hex: _,
        }) if key_exists && license_id == context.license_id && device_id == context.device_id
    )
}

fn resolve_device_key_storage_dir_for_context(
    data_dir: &Path,
    context: &SignLicenseContext,
) -> std::path::PathBuf {
    let override_dir = shared_device_key_dir_override_from_env();
    if cfg!(test) && override_dir.is_none() {
        return data_dir.to_path_buf();
    }

    let storage_dir = resolve_device_key_storage_dir_with_override(data_dir, override_dir);
    if storage_dir != data_dir && local_device_artifacts_match_context(data_dir, context) {
        return data_dir.to_path_buf();
    }
    storage_dir
}

fn migrate_device_key_artifact_if_missing(
    data_dir: &Path,
    storage_dir: &Path,
    file_name: &str,
) -> Result<(), String> {
    let source = data_dir.join(file_name);
    let destination = storage_dir.join(file_name);
    if !source.exists() || destination.exists() {
        return Ok(());
    }
    fs::copy(&source, &destination).map_err(|error| {
        format!(
            "copy {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn ensure_device_key_storage_dir_ready_at(
    data_dir: &Path,
    storage_dir: &Path,
) -> Result<(), String> {
    fs::create_dir_all(storage_dir)
        .map_err(|error| format!("create {}: {error}", storage_dir.display()))?;
    if storage_dir != data_dir {
        migrate_device_key_artifact_if_missing(data_dir, storage_dir, DEVICE_KEY_FILE)?;
        migrate_device_key_artifact_if_missing(
            data_dir,
            storage_dir,
            DEVICE_KEY_REGISTRATION_MARKER_FILE,
        )?;
    }
    Ok(())
}

fn ensure_device_key_storage_dir_ready(data_dir: &Path) -> Result<std::path::PathBuf, String> {
    let storage_dir = resolve_device_key_storage_dir(data_dir);
    ensure_device_key_storage_dir_ready_at(data_dir, &storage_dir)?;
    Ok(storage_dir)
}

fn ensure_device_key_storage_dir_ready_for_context(
    data_dir: &Path,
    context: &SignLicenseContext,
) -> Result<std::path::PathBuf, String> {
    let storage_dir = resolve_device_key_storage_dir_for_context(data_dir, context);
    ensure_device_key_storage_dir_ready_at(data_dir, &storage_dir)?;
    Ok(storage_dir)
}

fn device_key_registration_marker_path(data_dir: &Path) -> Result<std::path::PathBuf, String> {
    Ok(ensure_device_key_storage_dir_ready(data_dir)?.join(DEVICE_KEY_REGISTRATION_MARKER_FILE))
}

fn device_key_registration_marker_path_for_context(
    data_dir: &Path,
    context: &SignLicenseContext,
) -> Result<std::path::PathBuf, String> {
    Ok(
        ensure_device_key_storage_dir_ready_for_context(data_dir, context)?
            .join(DEVICE_KEY_REGISTRATION_MARKER_FILE),
    )
}

fn is_device_key_registered(
    data_dir: &Path,
    context: &SignLicenseContext,
    public_key_hex: &str,
) -> bool {
    let Ok(path) = device_key_registration_marker_path_for_context(data_dir, context) else {
        return false;
    };
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<DeviceKeyRegistrationMarker>(&raw) else {
        return false;
    };
    marker.license_id == context.license_id
        && marker.device_id == context.device_id
        && marker.public_key_hex == public_key_hex
}

fn persist_device_key_registration_marker(
    data_dir: &Path,
    context: &SignLicenseContext,
    public_key_hex: &str,
) -> Result<(), String> {
    fs::create_dir_all(data_dir)
        .map_err(|error| format!("create {}: {error}", data_dir.display()))?;
    let marker = DeviceKeyRegistrationMarker {
        license_id: context.license_id,
        device_id: context.device_id.clone(),
        public_key_hex: public_key_hex.to_string(),
    };
    let bytes = serde_json::to_vec_pretty(&marker)
        .map_err(|error| format!("serialize device registration marker: {error}"))?;
    let path = device_key_registration_marker_path_for_context(data_dir, context)?;
    fs::write(&path, bytes).map_err(|error| format!("write {}: {error}", path.display()))
}

pub fn build_device_proof(
    data_dir: &Path,
    context: &SignLicenseContext,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<DeviceProof, String> {
    let keypair = load_or_create_device_keypair_for_context(data_dir, context)?;
    let timestamp = crate::control_plane_auth::unix_now_secs();
    let nonce = crate::control_plane_auth::random_nonce_hex();
    let body_sha256_hex = sha256_hex(body);
    let message = canonical_device_message(
        method,
        path,
        &body_sha256_hex,
        timestamp,
        &nonce,
        context.license_id,
        &context.device_id,
    );
    Ok(DeviceProof {
        public_key_hex: keypair.public_key_hex(),
        signature_hex: keypair.sign_message(message.as_bytes()),
        timestamp,
        nonce,
        license_id: context.license_id,
        device_id: context.device_id.clone(),
    })
}

pub fn build_legacy_license_proof(
    context: &SignLicenseContext,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<(i64, String, String), String> {
    let timestamp = crate::control_plane_auth::unix_now_secs();
    let nonce = crate::control_plane_auth::random_nonce_hex();
    let body_sha = sha256_hex(body);
    let message = crate::control_plane_auth::canonical_message(
        method,
        path,
        &body_sha,
        timestamp,
        &nonce,
        &context.device_id,
    );
    let signature_hex =
        crate::control_plane_auth::sign_message_hex(&context.hmac_secret_hex, &message)?;
    Ok((timestamp, nonce, signature_hex))
}

fn is_registration_unavailable_status(status: reqwest::StatusCode, body: &str) -> bool {
    matches!(
        status,
        reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::NOT_IMPLEMENTED
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
    ) || body.to_ascii_lowercase().contains("device key")
        || body.to_ascii_lowercase().contains("not available")
        || body.to_ascii_lowercase().contains("not implemented")
}

fn is_device_key_registration_conflict(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::CONFLICT
        && body
            .to_ascii_lowercase()
            .contains("device public key already registered")
}

pub async fn ensure_device_key_registered(
    client: &reqwest::Client,
    license_server_url: &str,
    context: &SignLicenseContext,
    data_dir: &Path,
) -> Result<bool, String> {
    let keypair = load_or_create_device_keypair_for_context(data_dir, context)?;
    let public_key_hex = keypair.public_key_hex();
    if is_device_key_registered(data_dir, context, &public_key_hex) {
        return Ok(true);
    }

    let body = serde_json::json!({
        "device_id": context.device_id,
        "license_id": context.license_id,
        "device_public_key_hex": public_key_hex,
    });
    let body_bytes = serde_json::to_vec(&body)
        .map_err(|error| format!("device key registration JSON: {error}"))?;
    let (timestamp, nonce, signature_hex) =
        build_legacy_license_proof(context, "POST", device_key_register_path(), &body_bytes)?;
    let response = client
        .post(format!(
            "{}{}",
            license_server_url.trim_end_matches('/'),
            device_key_register_path()
        ))
        .header(client_version_header(), current_client_version())
        .header(client_platform_header(), current_client_platform())
        .bearer_auth(&context.auth_token)
        .header(device_header_license_id(), context.license_id.to_string())
        .header(device_header_device_id(), &context.device_id)
        .header(
            crate::sealed_protocol::sealed_str!("x-license-proof-ts"),
            timestamp.to_string(),
        )
        .header(
            crate::sealed_protocol::sealed_str!("x-license-proof-nonce"),
            &nonce,
        )
        .header(
            crate::sealed_protocol::sealed_str!("x-license-proof-signature"),
            signature_hex,
        )
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("device key registration request failed: {error}"))?;

    let status = response.status();
    let response_body = response
        .text()
        .await
        .map_err(|error| format!("device key registration read failed: {error}"))?;
    if !status.is_success() {
        if is_registration_unavailable_status(status, &response_body) {
            return Ok(false);
        }
        if is_device_key_registration_conflict(status, &response_body) {
            warn!(
                license_id = context.license_id,
                device_id = %context.device_id,
                "device key already registered on server with a different local key"
            );
            return Err(
                "device key registration conflict: 本地授权设备密钥与服务器记录不一致，请退出后重新登录授权账号"
                    .to_string(),
            );
        }
        return Err(format!(
            "device key registration returned HTTP {}: {}",
            status.as_u16(),
            response_body.chars().take(300).collect::<String>()
        ));
    }

    persist_device_key_registration_marker(data_dir, context, &public_key_hex)?;
    Ok(true)
}

pub async fn select_runtime_proof_mode(
    client: &reqwest::Client,
    license_server_url: &str,
    context: &SignLicenseContext,
    data_dir: &Path,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<ProofMode, String> {
    if ensure_device_key_registered(client, license_server_url, context, data_dir).await? {
        return Ok(ProofMode::Device(build_device_proof(
            data_dir, context, method, path, body,
        )?));
    }
    let (timestamp, nonce, signature_hex) =
        build_legacy_license_proof(context, method, path, body)?;
    Ok(ProofMode::LegacyLicenseHmac {
        timestamp,
        nonce,
        signature_hex,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Bytes, http::HeaderMap, routing::post, Json, Router};
    use reqwest::StatusCode;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    };
    use tempfile::tempdir;
    use tokio::net::TcpListener;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn sign_license_context_round_trip() {
        let original = get_sign_license_context();
        let expected = SignLicenseContext {
            server_url: "https://license.example.com".into(),
            auth_token: "auth-token".into(),
            signed_token: "signed-token".into(),
            license_id: 42,
            username: "test-user".into(),
            sku: "monthly".into(),
            account_limit: 10,
            hmac_secret_hex: "11".repeat(32),
            device_id: "device-1".into(),
        };
        set_sign_license_context(Some(expected.clone()));
        assert_eq!(get_sign_license_context(), Some(expected));
        set_sign_license_context(original);
    }

    #[test]
    fn loads_sign_license_context_from_bundle_file() {
        let dir = tempdir().unwrap();
        let signed_token = test_signed_token(serde_json::json!({
            "lid": 9,
            "uid": 7,
            "sku": "monthly",
            "iat": 1_700_000_000_u64,
            "exp": 4_102_444_800_u64,
            "hs": "1111111111111111111111111111111111111111111111111111111111111111",
            "account_limit": 10,
        }));
        std::fs::write(
            dir.path().join("license.json"),
            format!(
                r#"{{
              "server_url": "https://license.example.com/",
              "auth_token": "user-jwt-token",
              "signed_token": "{signed_token}",
              "license_id": 9,
              "sku": "monthly",
              "account_limit": 10,
              "hmac_secret_hex": "1111111111111111111111111111111111111111111111111111111111111111",
              "expires_unix": 4102444800
            }}"#
            ),
        )
        .unwrap();

        let context = load_sign_license_context(dir.path(), Some("device-1"), 1_700_000_000)
            .unwrap()
            .unwrap();

        assert_eq!(context.server_url, "https://license.example.com");
        assert_eq!(context.auth_token, "user-jwt-token");
        assert_eq!(context.signed_token, signed_token);
        assert_eq!(context.license_id, 9);
        assert_eq!(context.sku, "monthly");
        assert_eq!(context.account_limit, 10);
        assert_eq!(
            context.hmac_secret_hex,
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert_eq!(context.device_id, "device-1");
    }

    #[test]
    fn rejects_expired_bundle_file() {
        let dir = tempdir().unwrap();
        let signed_token = test_signed_token(serde_json::json!({
            "lid": 9,
            "uid": 7,
            "sku": "monthly",
            "iat": 1_u64,
            "exp": 100_u64,
            "hs": "1111111111111111111111111111111111111111111111111111111111111111",
            "account_limit": 10,
        }));
        std::fs::write(
            dir.path().join("license.json"),
            format!(
                r#"{{
              "server_url": "https://license.example.com",
              "auth_token": "user-jwt-token",
              "signed_token": "{signed_token}",
              "license_id": 9,
              "sku": "monthly",
              "account_limit": 10,
              "hmac_secret_hex": "1111111111111111111111111111111111111111111111111111111111111111",
              "expires_unix": 100
            }}"#
            ),
        )
        .unwrap();

        let error = load_sign_license_context(dir.path(), Some("device-1"), 200).unwrap_err();

        assert!(error.contains("expired"));
    }

    #[test]
    fn rejects_tampered_bundle_claims_when_restoring_sign_context() {
        let dir = tempdir().unwrap();
        let signed_token = test_signed_token(serde_json::json!({
            "lid": 9,
            "uid": 7,
            "sku": "monthly",
            "iat": 1_700_000_000_u64,
            "exp": 4_102_444_800_u64,
            "hs": "1111111111111111111111111111111111111111111111111111111111111111",
            "account_limit": 10,
        }));
        std::fs::write(
            dir.path().join("license.json"),
            format!(
                r#"{{
              "server_url": "https://license.example.com",
              "auth_token": "user-jwt-token",
              "signed_token": "{signed_token}",
              "license_id": 99,
              "sku": "yearly",
              "account_limit": 999,
              "hmac_secret_hex": "2222222222222222222222222222222222222222222222222222222222222222",
              "expires_unix": 4102444800
            }}"#
            ),
        )
        .unwrap();

        let error =
            load_sign_license_context(dir.path(), Some("device-1"), 1_700_000_000).unwrap_err();

        assert!(error.contains("license_id mismatch"));
    }

    #[tokio::test]
    async fn upgrades_legacy_bundle_via_activate_and_persists_complete_shape() {
        let dir = tempdir().unwrap();
        let auth_token = build_test_auth_token(7, "mac-tester");
        std::fs::write(
            dir.path().join("license.json"),
            format!(
                r#"{{
                  "server_url": "https://placeholder.invalid",
                  "auth_token": "{auth_token}",
                  "expires_unix": 1811035476,
                  "license_key": "C8VW-E2UE-2Q5A-L96V",
                  "username": "mac-tester"
                }}"#
            ),
        )
        .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}");
        std::fs::write(
            dir.path().join("license.json"),
            format!(
                r#"{{
                  "server_url": "{server_url}",
                  "auth_token": "{auth_token}",
                  "expires_unix": 1811035476,
                  "license_key": "C8VW-E2UE-2Q5A-L96V",
                  "username": "mac-tester"
                }}"#
            ),
        )
        .unwrap();

        let app = Router::new().route(
            crate::sealed_protocol::sealed_str!("/api/licenses/activate"),
            post(|| async move {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "signed_token": "signed-license-token",
                        "license": {
                            "id": 19,
                            "sku": "trial",
                            "license_key": "C8VW-E2UE-2Q5A-L96V",
                            "issued_at": "2026-05-27T01:02:03+00:00",
                            "expires_at": "2027-05-27T01:02:03+00:00"
                        },
                        "hmac_secret_hex": "11".repeat(32)
                    })),
                )
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let context = restore_legacy_sign_license_context(dir.path(), &[])
            .await
            .unwrap()
            .unwrap();

        server.abort();

        assert_eq!(context.server_url, server_url);
        assert_eq!(context.license_id, 19);
        assert_eq!(context.signed_token, "signed-license-token");

        let upgraded = std::fs::read_to_string(dir.path().join("license.json")).unwrap();
        let upgraded: UpgradedPersistedLicenseBundle = serde_json::from_str(&upgraded).unwrap();
        assert_eq!(upgraded.license_id, 19);
        assert_eq!(upgraded.user_id, 7);
        assert_eq!(upgraded.username, "mac-tester");
        assert_eq!(upgraded.sku, "trial");
        assert_eq!(upgraded.signed_token, "signed-license-token");
        assert_eq!(upgraded.hmac_secret_hex, "11".repeat(32));
    }

    fn test_signed_token(payload: serde_json::Value) -> String {
        let body = serde_json::to_vec(&payload).unwrap();
        let body_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(body);
        format!(
            "{body_b64}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0_u8; 64])
        )
    }

    fn build_test_auth_token(user_id: i64, username: &str) -> String {
        let header = serde_json::json!({ "alg": "HS256", "typ": "JWT" }).to_string();
        let payload = serde_json::json!({
            "sub": user_id,
            "username": username,
            "role": "user",
            "iat": 1,
            "exp": 4_102_444_800u64
        })
        .to_string();
        format!(
            "{}.{}.sig",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header),
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload)
        )
    }

    #[test]
    fn device_key_persists_across_loads() {
        let dir = tempdir().unwrap();
        let first = load_or_create_device_keypair(dir.path()).unwrap();
        let second = load_or_create_device_keypair(dir.path()).unwrap();

        assert_eq!(first.public_key_hex(), second.public_key_hex());
        assert_eq!(
            first.sign_message(b"device-proof"),
            second.sign_message(b"device-proof")
        );
    }

    #[test]
    fn resolve_device_key_storage_dir_prefers_explicit_override() {
        let data_dir = Path::new("/tmp/runtime-data");
        let override_dir = std::path::PathBuf::from("/tmp/shared-device-keys");

        let resolved =
            resolve_device_key_storage_dir_with_override(data_dir, Some(override_dir.clone()));

        assert_eq!(resolved, override_dir);
    }

    #[test]
    fn ensure_device_key_storage_dir_ready_copies_legacy_artifacts_into_shared_dir() {
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("app-data");
        let shared_dir = temp.path().join("shared-device-keys");
        fs::create_dir_all(&data_dir).unwrap();

        let key_seed = "11".repeat(32);
        let marker = serde_json::json!({
            "license_id": 42,
            "device_id": "device-1",
            "public_key_hex": "22".repeat(32),
        });

        fs::write(data_dir.join(DEVICE_KEY_FILE), &key_seed).unwrap();
        fs::write(
            data_dir.join(DEVICE_KEY_REGISTRATION_MARKER_FILE),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();

        ensure_device_key_storage_dir_ready_at(&data_dir, &shared_dir).unwrap();

        assert_eq!(
            fs::read_to_string(shared_dir.join(DEVICE_KEY_FILE)).unwrap(),
            key_seed
        );
        let copied_marker: serde_json::Value = serde_json::from_slice(
            &fs::read(shared_dir.join(DEVICE_KEY_REGISTRATION_MARKER_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(copied_marker, marker);
    }

    #[tokio::test]
    async fn select_runtime_proof_mode_prefers_local_device_artifacts_over_shared_dir() {
        let _guard = env_lock().lock().unwrap();
        let temp = tempdir().unwrap();
        let data_dir = temp.path().join("app-data");
        let shared_dir = temp.path().join("shared-device-keys");
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&shared_dir).unwrap();

        let local_seed = "11".repeat(32);
        let shared_seed = "22".repeat(32);
        fs::write(data_dir.join(DEVICE_KEY_FILE), &local_seed).unwrap();
        fs::write(shared_dir.join(DEVICE_KEY_FILE), &shared_seed).unwrap();

        let context = SignLicenseContext {
            server_url: "https://unused.example.com".into(),
            auth_token: "auth-token".into(),
            signed_token: "signed-token".into(),
            license_id: 35,
            username: "test-user".into(),
            sku: "monthly".into(),
            account_limit: 10,
            hmac_secret_hex: "11".repeat(32),
            device_id: "current-device".into(),
        };

        let local_public_key_hex = load_or_create_device_keypair(&data_dir)
            .unwrap()
            .public_key_hex();
        let shared_public_key_hex = load_or_create_device_keypair(&shared_dir)
            .unwrap()
            .public_key_hex();

        fs::write(
            data_dir.join(DEVICE_KEY_REGISTRATION_MARKER_FILE),
            serde_json::to_vec(&serde_json::json!({
                "license_id": context.license_id,
                "device_id": context.device_id,
                "public_key_hex": local_public_key_hex,
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            shared_dir.join(DEVICE_KEY_REGISTRATION_MARKER_FILE),
            serde_json::to_vec(&serde_json::json!({
                "license_id": 7,
                "device_id": "device-1",
                "public_key_hex": shared_public_key_hex,
            }))
            .unwrap(),
        )
        .unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}");
        let app = Router::new().route(
            device_key_register_path(),
            post(|| async move {
                (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "detail": "device public key already registered",
                        "reason": "conflict",
                    })),
                )
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let previous_override = std::env::var_os(SHARED_DEVICE_KEY_DIR_ENV);
        std::env::set_var(SHARED_DEVICE_KEY_DIR_ENV, &shared_dir);

        let proof_mode = select_runtime_proof_mode(
            &reqwest::Client::new(),
            &server_url,
            &context,
            &data_dir,
            "POST",
            crate::sealed_protocol::sealed_str!("/api/nodes/incidents"),
            br#"{"hello":"world"}"#,
        )
        .await
        .unwrap();

        match previous_override {
            Some(value) => std::env::set_var(SHARED_DEVICE_KEY_DIR_ENV, value),
            None => std::env::remove_var(SHARED_DEVICE_KEY_DIR_ENV),
        }
        server.abort();

        match proof_mode {
            ProofMode::Device(proof) => {
                assert_eq!(proof.public_key_hex, local_public_key_hex);
                assert_eq!(proof.license_id, context.license_id);
                assert_eq!(proof.device_id, context.device_id);
            }
            other => panic!("expected device proof, got {other:?}"),
        }
    }

    #[test]
    fn canonical_device_message_is_stable() {
        let message = canonical_device_message(
            "post",
            "/sign",
            &"ab".repeat(32),
            123,
            "nonce-1",
            42,
            "device-1",
        );
        assert_eq!(
            message,
            format!(
                "POST\n/sign\n{}\n123\nnonce-1\n42\ndevice-1",
                "ab".repeat(32)
            )
        );
    }

    #[tokio::test]
    async fn device_key_registration_uses_server_field_name() {
        let dir = tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}");
        let app = Router::new().route(
            device_key_register_path(),
            post(|Json(body): Json<serde_json::Value>| async move {
                let has_new_field = body
                    .get("device_public_key_hex")
                    .and_then(|value| value.as_str())
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false);
                let has_old_field = body.get("public_key_hex").is_some();
                if has_new_field && !has_old_field {
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({ "registered": true })),
                    )
                } else {
                    (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        Json(serde_json::json!({
                            "detail": "missing field `device_public_key_hex`"
                        })),
                    )
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let context = SignLicenseContext {
            server_url: server_url.clone(),
            auth_token: "auth-token".into(),
            signed_token: "signed-token".into(),
            license_id: 42,
            username: "test-user".into(),
            sku: "monthly".into(),
            account_limit: 10,
            hmac_secret_hex: "11".repeat(32),
            device_id: "device-1".into(),
        };

        let registered = ensure_device_key_registered(&client, &server_url, &context, dir.path())
            .await
            .unwrap();

        server.abort();
        assert!(registered);
    }

    #[tokio::test]
    async fn device_key_registration_conflict_blocks_legacy_runtime_fallback() {
        let dir = tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{addr}");
        let app = Router::new().route(
            device_key_register_path(),
            post(|| async move {
                (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "detail": "device public key already registered",
                        "reason": "conflict",
                    })),
                )
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let context = SignLicenseContext {
            server_url: server_url.clone(),
            auth_token: "auth-token".into(),
            signed_token: "signed-token".into(),
            license_id: 42,
            username: "test-user".into(),
            sku: "monthly".into(),
            account_limit: 10,
            hmac_secret_hex: "11".repeat(32),
            device_id: "device-1".into(),
        };

        let error = select_runtime_proof_mode(
            &client,
            &server_url,
            &context,
            dir.path(),
            "POST",
            crate::sealed_protocol::sealed_str!("/api/licenses/runtime-challenge"),
            br#"{"device_id":"device-1"}"#,
        )
        .await
        .unwrap_err();

        server.abort();
        assert!(error.contains("device key registration conflict"));
    }

    #[tokio::test]
    async fn refresh_protocol_unlock_key_uses_registered_device_proof_and_caches_key() {
        let _guard = env_lock().lock().unwrap();
        let dir = tempdir().unwrap();
        let seed = [23_u8; 32];
        fs::write(dir.path().join(DEVICE_KEY_FILE), hex::encode(seed)).unwrap();
        let signing_key = SigningKey::from_bytes(&seed);
        let public_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
        let context = SignLicenseContext {
            server_url: String::new(),
            auth_token: "user-jwt-token".into(),
            signed_token: "signed-token".into(),
            license_id: 42,
            username: "test-user".into(),
            sku: "monthly".into(),
            account_limit: 10,
            hmac_secret_hex: "11".repeat(32),
            device_id: "device-protocol-1".into(),
        };
        fs::write(
            dir.path().join(DEVICE_KEY_REGISTRATION_MARKER_FILE),
            serde_json::to_vec(&serde_json::json!({
                "license_id": context.license_id,
                "device_id": context.device_id,
                "public_key_hex": public_key_hex,
            }))
            .unwrap(),
        )
        .unwrap();

        let captured = Arc::new(Mutex::new(None));
        let captured_for_handler = Arc::clone(&captured);
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_for_handler = Arc::clone(&request_count);
        let app = Router::new().route(
            protocol_unlock_path(),
            post(move |headers: HeaderMap, body: Bytes| {
                let captured = Arc::clone(&captured_for_handler);
                let request_count = Arc::clone(&request_count_for_handler);
                async move {
                    request_count.fetch_add(1, Ordering::SeqCst);
                    *captured.lock().unwrap() = Some((headers, body));
                    Json(serde_json::json!({
                        "protocolVersion": "2026-08-08",
                        "capability": "orders",
                        "protocolKey": "ab".repeat(32),
                        "expiresAt": "2099-01-01T00:00:00Z",
                        "wrappedKey": null,
                    }))
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let original = get_sign_license_context();
        set_sign_license_context(Some(SignLicenseContext {
            server_url,
            ..context.clone()
        }));
        let cache_key = protocol_unlock_cache_key(&context, "2026-08-08", "orders").unwrap();
        protocol_unlock_slot().write().unwrap().insert(
            cache_key,
            ProtocolUnlockKey {
                key: [0xcd; 32],
                expires_at: now_unix() + 30,
            },
        );
        assert!(get_protocol_unlock_key("2026-08-08", "orders").is_none());

        let unlocked = refresh_protocol_unlock_key(
            &reqwest::Client::new(),
            dir.path(),
            "2026-08-08",
            "orders",
        )
        .await
        .unwrap();
        let repeated = refresh_protocol_unlock_key(
            &reqwest::Client::new(),
            dir.path(),
            "2026-08-08",
            "orders",
        )
        .await
        .unwrap();
        let cached = get_protocol_unlock_key("2026-08-08", "orders").unwrap();

        set_sign_license_context(original);
        server.abort();

        assert_eq!(unlocked.key, [0xab; 32]);
        assert_eq!(repeated, unlocked);
        assert_eq!(cached, unlocked);
        assert_eq!(request_count.load(Ordering::SeqCst), 1);
        let (headers, body) = captured.lock().unwrap().take().unwrap();
        assert_eq!(
            headers.get("authorization").unwrap(),
            "Bearer user-jwt-token"
        );
        assert_eq!(
            headers.get(client_version_header()).unwrap(),
            current_client_version()
        );
        assert_eq!(
            headers
                .get(device_header_public_key())
                .unwrap()
                .to_str()
                .unwrap(),
            public_key_hex.as_str()
        );
        assert!(headers.get(device_header_signature()).is_some());
        assert!(headers.get(device_header_timestamp()).is_some());
        assert!(headers.get(device_header_nonce()).is_some());
        assert_eq!(headers.get(device_header_key_id()).unwrap(), "42");
        let timestamp = headers
            .get(device_header_timestamp())
            .unwrap()
            .to_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let nonce = headers
            .get(device_header_nonce())
            .unwrap()
            .to_str()
            .unwrap();
        let signature_bytes = hex::decode(
            headers
                .get(device_header_signature())
                .unwrap()
                .to_str()
                .unwrap(),
        )
        .unwrap();
        let signature = Signature::from_bytes(signature_bytes.as_slice().try_into().unwrap());
        let signed_message = canonical_device_message(
            "POST",
            protocol_unlock_path(),
            &sha256_hex(&body),
            timestamp,
            nonce,
            context.license_id,
            &context.device_id,
        );
        signing_key
            .verifying_key()
            .verify(signed_message.as_bytes(), &signature)
            .expect("proof signs the exact serialized JSON bytes");
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["protocolVersion"], "2026-08-08");
        assert_eq!(body["capability"], "orders");
        assert_eq!(body["deviceId"], "device-protocol-1");
    }
}
