//! Library entry point shared between desktop and mobile (Android) targets.
//!
//! - Desktop (`main.rs`) calls `run()` directly.
//! - Android links the `cdylib` produced by this crate and Tauri's
//!   `#[mobile_entry_point]` macro generates the JNI glue so the Android
//!   host can invoke `run()` from Kotlin.
//!
//! The Tauri shell now starts the embedded backend on both desktop and
//! Android so the React app can keep talking to the same loopback API surface
//! everywhere.

mod bundle;
mod data_import;
mod data_transfer;
mod host_runtime;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(not(target_os = "android"))]
use std::process::Command;

use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "android"))]
use tauri::UserAttentionType;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::UpdaterExt;
use tokio::{sync::watch, time::timeout};
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*};

use xianyu_rust_backend_full::ServerHandle;

use crate::bundle::{decrypt_bundle, encrypt_bundle, BundleEnvelope};
use crate::data_transfer::{
    AccountDataExportResult, AccountDataImportInspection, AccountDataRestoreResult,
    AccountDataTransferPreview,
};
use xianyu_rust_backend_full::control_plane_tls::{parse_pin_set, verify_response_pin};
use xianyu_rust_backend_full::license_runtime::{
    apply_client_version_headers, resolve_update_policy_url,
};

fn default_download_page_url() -> &'static str {
    xianyu_rust_backend_full::sealed_protocol::sealed_str!("https://download.gooofish.shop/")
}

pub struct BackendState {
    server_handle: Arc<Mutex<Option<ServerHandle>>>,
    port_rx: watch::Receiver<Option<u16>>,
    app_data_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct AppVersionInfo {
    current_version: String,
    update_configured: bool,
}

#[derive(Debug, Clone, Serialize)]
struct AppUpdateCheckResult {
    configured: bool,
    policy_configured: bool,
    current_version: String,
    available: bool,
    version: Option<String>,
    body: Option<String>,
    date: Option<String>,
    minimum_version: Option<String>,
    blocked: bool,
    block_reason: Option<String>,
    download_url: Option<String>,
    download_page_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct AppUpdateInstallResult {
    mode: String,
    restart_required: bool,
    message: String,
    path: Option<String>,
}

#[derive(Debug, Clone)]
struct UpdaterConfig {
    endpoints: Vec<tauri::Url>,
    pubkey: String,
}

#[derive(Debug, Clone, Default)]
struct UpdatePolicy {
    minimum_version: Option<String>,
    latest_version: Option<String>,
    body: Option<String>,
    required: bool,
    message: Option<String>,
    download_url: Option<String>,
    download_page_url: Option<String>,
    updater_endpoints: Vec<String>,
    updater_pubkey: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PlatformDownloadUrls {
    #[serde(default, alias = "mac", alias = "darwin")]
    macos: Option<String>,
    #[serde(default, alias = "win")]
    windows: Option<String>,
    #[serde(default)]
    android: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RawUpdatePolicy {
    #[serde(
        default,
        alias = "minimum_version",
        alias = "min_version",
        alias = "minVersion"
    )]
    minimum_version: Option<String>,
    #[serde(default, alias = "latest_version")]
    latest_version: Option<String>,
    #[serde(
        default,
        alias = "notes",
        alias = "changelog",
        alias = "releaseNotes",
        alias = "release_notes"
    )]
    body: Option<String>,
    #[serde(
        default,
        alias = "required",
        alias = "force_update",
        alias = "mandatory"
    )]
    required: Option<bool>,
    #[serde(default, alias = "reason", alias = "requiredMessage")]
    message: Option<String>,
    #[serde(
        default,
        alias = "download_url",
        alias = "installerUrl",
        alias = "installer_url"
    )]
    download_url: Option<String>,
    #[serde(default, alias = "platform_downloads", alias = "platformDownloads")]
    downloads: Option<PlatformDownloadUrls>,
    #[serde(default, alias = "download_page_url", alias = "downloadPage")]
    download_page_url: Option<String>,
    #[serde(default, alias = "updater_endpoint", alias = "updaterEndpoint")]
    updater_endpoint: Option<String>,
    #[serde(default, alias = "updater_endpoints", alias = "endpoints")]
    updater_endpoints: Option<Vec<String>>,
    #[serde(
        default,
        alias = "updater_pubkey",
        alias = "pubkey",
        alias = "publicKey"
    )]
    updater_pubkey: Option<String>,
}

#[tauri::command]
async fn get_backend_port(state: State<'_, BackendState>) -> Result<u16, String> {
    let mut port_rx = state.port_rx.clone();

    if let Some(port) = *port_rx.borrow() {
        return Ok(port);
    }

    timeout(Duration::from_secs(30), async move {
        loop {
            if port_rx.changed().await.is_err() {
                return Err(());
            }

            if let Some(port) = *port_rx.borrow_and_update() {
                return Ok(port);
            }
        }
    })
    .await
    .map_err(|_| "backend not ready within 30s".to_string())?
    .map_err(|_| "backend not ready within 30s".to_string())
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
fn set_app_badge_count(app: AppHandle, count: i64) -> Result<(), String> {
    let badge_count = if count > 0 { Some(count) } else { None };
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window
        .set_badge_count(badge_count)
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "android")]
#[tauri::command]
fn set_app_badge_count(_app: AppHandle, _count: i64) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
fn request_app_attention(app: AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window
        .request_user_attention(Some(UserAttentionType::Informational))
        .map_err(|e| e.to_string())
}

#[cfg(target_os = "android")]
#[tauri::command]
fn request_app_attention(_app: AppHandle) -> Result<(), String> {
    Ok(())
}

fn current_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn normalize_version_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 已发布版本按发布时间线排列，用于正确比较跨大版本号跳跃的顺序。
/// 来源：docs/releases/known_versions.txt，编译时嵌入，发新版本只需往该文件加一行。
const KNOWN_RELEASE_VERSIONS_TEXT: &str = include_str!("../../docs/releases/known_versions.txt");

fn known_release_versions() -> Vec<&'static str> {
    KNOWN_RELEASE_VERSIONS_TEXT
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with("#"))
        .collect()
}

fn normalize_version_key(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['+', ' '])
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn parse_version_segments(version: &str) -> Vec<u64> {
    normalize_version_key(version)
        .split('.')
        .map(|segment| {
            segment
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>()
        })
        .map(|digits| digits.parse::<u64>().unwrap_or(0))
        .collect()
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let known = known_release_versions();
    let left_key = normalize_version_key(left);
    let right_key = normalize_version_key(right);
    let left_idx = known.iter().position(|v| *v == left_key);
    let right_idx = known.iter().position(|v| *v == right_key);

    if let (Some(li), Some(ri)) = (left_idx, right_idx) {
        return li.cmp(&ri);
    }

    let left_segments = parse_version_segments(left);
    let right_segments = parse_version_segments(right);
    let max_len = left_segments.len().max(right_segments.len());

    for index in 0..max_len {
        let left_value = *left_segments.get(index).unwrap_or(&0);
        let right_value = *right_segments.get(index).unwrap_or(&0);
        match left_value.cmp(&right_value) {
            Ordering::Equal => continue,
            ordering => return ordering,
        }
    }

    Ordering::Equal
}

fn version_is_lower(current: &str, target: &str) -> bool {
    compare_versions(current, target).is_lt()
}

fn parse_updater_endpoints(raw: &str) -> Vec<tauri::Url> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let values = if let Ok(values) = serde_json::from_str::<Vec<String>>(trimmed) {
        values
    } else {
        trimmed
            .split([',', '\n'])
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()
    };

    values
        .into_iter()
        .filter_map(|value| tauri::Url::parse(value.trim()).ok())
        .collect()
}

fn update_check_result(
    configured: bool,
    current_version: String,
    update: Option<tauri_plugin_updater::Update>,
) -> AppUpdateCheckResult {
    let Some(update) = update else {
        return AppUpdateCheckResult {
            configured,
            policy_configured: false,
            current_version,
            available: false,
            version: None,
            body: None,
            date: None,
            minimum_version: None,
            blocked: false,
            block_reason: None,
            download_url: None,
            download_page_url: None,
        };
    };

    AppUpdateCheckResult {
        configured,
        policy_configured: false,
        current_version,
        available: true,
        version: Some(update.version),
        body: update.body,
        date: update.date.map(|date| date.to_string()),
        minimum_version: None,
        blocked: false,
        block_reason: None,
        download_url: None,
        download_page_url: None,
    }
}

fn resolve_updater_config(policy: Option<&UpdatePolicy>) -> Option<UpdaterConfig> {
    let endpoints_raw = std::env::var("XIANYU_UPDATER_ENDPOINTS")
        .ok()
        .or_else(|| option_env!("XIANYU_UPDATER_ENDPOINTS").map(str::to_string))
        .unwrap_or_default();
    let pubkey = std::env::var("XIANYU_UPDATER_PUBKEY")
        .ok()
        .or_else(|| option_env!("XIANYU_UPDATER_PUBKEY").map(str::to_string))
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut endpoints = parse_updater_endpoints(&endpoints_raw);
    let mut pubkey = normalize_version_text(Some(pubkey));

    if endpoints.is_empty() {
        if let Some(policy) = policy {
            endpoints = policy
                .updater_endpoints
                .iter()
                .filter_map(|value| tauri::Url::parse(value.trim()).ok())
                .collect();
        }
    }

    if pubkey.is_none() {
        pubkey = policy.and_then(|policy| policy.updater_pubkey.clone());
    }

    let Some(pubkey) = pubkey else {
        return None;
    };

    if endpoints.is_empty() {
        return None;
    }

    Some(UpdaterConfig { endpoints, pubkey })
}

fn update_policy_urls() -> Vec<String> {
    let configured = std::env::var("XIANYU_UPDATE_POLICY_URL")
        .ok()
        .or_else(|| option_env!("XIANYU_UPDATE_POLICY_URL").map(str::to_string))
        .map(|value| value.trim().to_string());
    update_policy_urls_from_config(configured)
}

fn update_policy_urls_from_config(configured: Option<String>) -> Vec<String> {
    vec![resolve_update_policy_url(configured)]
}

async fn load_update_policy(_state: &BackendState) -> Result<Option<UpdatePolicy>, String> {
    let urls = update_policy_urls();

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|error| error.to_string())?;

    let mut last_error = None;
    for url in urls {
        match fetch_update_policy(&client, &url).await {
            Ok(policy) => return Ok(Some(policy)),
            Err(error) => last_error = Some(error),
        }
    }

    Err(last_error.unwrap_or_else(|| "更新策略请求失败".to_string()))
}

fn current_download_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "macos"
    }
}

fn platform_download_url(downloads: &PlatformDownloadUrls, platform: &str) -> Option<String> {
    match platform {
        "macos" | "mac" | "darwin" => normalize_version_text(downloads.macos.clone()),
        "windows" | "win" => normalize_version_text(downloads.windows.clone()),
        "android" => normalize_version_text(downloads.android.clone()),
        _ => None,
    }
}

fn raw_download_url(raw: &RawUpdatePolicy) -> Option<String> {
    normalize_version_text(raw.download_url.clone()).or_else(|| {
        raw.downloads
            .as_ref()
            .and_then(|downloads| platform_download_url(downloads, current_download_platform()))
    })
}

fn update_artifact_filename(download_url: &str) -> String {
    tauri::Url::parse(download_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(str::to_string)
        })
        .map(|name| {
            name.chars()
                .map(|ch| match ch {
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                    _ => ch,
                })
                .collect::<String>()
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| match current_download_platform() {
            "windows" => "yuguanjia-update.exe".to_string(),
            "android" => "yuguanjia-update.apk".to_string(),
            _ => "yuguanjia-update.dmg".to_string(),
        })
}

async fn download_update_artifact(download_url: &str, filename: &str) -> Result<PathBuf, String> {
    let url =
        tauri::Url::parse(download_url).map_err(|error| format!("更新下载地址无效：{error}"))?;
    if url.scheme() != "https" {
        return Err("更新下载地址必须使用 HTTPS".to_string());
    }

    let update_dir = std::env::temp_dir().join("yuguanjia-updates");
    tokio::fs::create_dir_all(&update_dir)
        .await
        .map_err(|error| format!("创建更新缓存目录失败：{error}"))?;
    let artifact_path = update_dir.join(filename);

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|error| error.to_string())?;
    let response = apply_client_version_headers(client.get(url))
        .send()
        .await
        .map_err(|error| format!("下载安装包失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载安装包失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取安装包失败：{error}"))?;
    if bytes.is_empty() {
        return Err("下载安装包失败：文件为空".to_string());
    }

    tokio::fs::write(&artifact_path, &bytes)
        .await
        .map_err(|error| format!("保存安装包失败：{error}"))?;
    Ok(artifact_path)
}

#[cfg(target_os = "macos")]
fn open_desktop_update_artifact(path: &std::path::Path) -> Result<(), String> {
    Command::new("open")
        .arg(path)
        .spawn()
        .map_err(|error| format!("打开安装包失败：{error}"))?;
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn windows_update_installer_args() -> &'static [&'static str] {
    &["/S", "/R", "/UPDATE"]
}

#[cfg(target_os = "windows")]
fn open_desktop_update_artifact(path: &std::path::Path) -> Result<(), String> {
    Command::new(path)
        .args(windows_update_installer_args())
        .spawn()
        .map_err(|error| format!("打开安装程序失败：{error}"))?;
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_desktop_update_artifact(_path: &std::path::Path) -> Result<(), String> {
    Err("当前平台暂不支持打开安装包".to_string())
}

async fn install_from_download_policy(
    policy: &UpdatePolicy,
) -> Result<AppUpdateInstallResult, String> {
    let Some(download_url) = policy
        .download_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err("当前版本策略没有提供本平台安装包地址".to_string());
    };
    let filename = update_artifact_filename(download_url);

    #[cfg(target_os = "android")]
    {
        let message = android_update::install_apk_from_url(download_url, &filename)?;
        return Ok(AppUpdateInstallResult {
            mode: "android_package_installer".to_string(),
            restart_required: false,
            message,
            path: None,
        });
    }

    #[cfg(not(target_os = "android"))]
    {
        let path = download_update_artifact(download_url, &filename).await?;
        open_desktop_update_artifact(&path)?;
        Ok(AppUpdateInstallResult {
            mode: "downloaded_installer".to_string(),
            restart_required: false,
            message: if cfg!(target_os = "windows") {
                "安装包已下载，正在后台自动更新。".to_string()
            } else {
                "安装包已下载并打开，请按系统提示完成安装。".to_string()
            },
            path: Some(path.display().to_string()),
        })
    }
}

async fn fetch_update_policy(client: &reqwest::Client, url: &str) -> Result<UpdatePolicy, String> {
    let response = apply_client_version_headers(client.get(url))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let pins = parse_pin_set(&std::env::var("XIANYU_CONTROL_PLANE_CERT_PINS").unwrap_or_default());
    verify_response_pin(&response, &pins, "license_service", url)?;
    if !response.status().is_success() {
        return Err(format!(
            "更新策略请求失败（HTTP {}）",
            response.status().as_u16()
        ));
    }

    let raw = response
        .json::<RawUpdatePolicy>()
        .await
        .map_err(|error| error.to_string())?;
    let mut updater_endpoints = raw.updater_endpoints.clone().unwrap_or_default();
    if let Some(endpoint) = normalize_version_text(raw.updater_endpoint.clone()) {
        updater_endpoints.push(endpoint);
    }
    let download_url = raw_download_url(&raw);

    Ok(UpdatePolicy {
        minimum_version: normalize_version_text(raw.minimum_version),
        latest_version: normalize_version_text(raw.latest_version),
        body: normalize_version_text(raw.body),
        required: raw.required.unwrap_or(false),
        message: normalize_version_text(raw.message),
        download_url,
        download_page_url: normalize_version_text(raw.download_page_url)
            .or_else(|| Some(default_download_page_url().to_string())),
        updater_endpoints,
        updater_pubkey: normalize_version_text(raw.updater_pubkey),
    })
}

fn apply_update_policy(
    mut result: AppUpdateCheckResult,
    policy: Option<UpdatePolicy>,
) -> AppUpdateCheckResult {
    let Some(policy) = policy else {
        return result;
    };

    result.policy_configured = true;
    result.minimum_version = policy.minimum_version.clone();
    result.download_url = policy.download_url.clone();
    result.download_page_url = policy.download_page_url.clone();

    if result.body.is_none() {
        result.body = policy.body.clone();
    }

    if let Some(latest_version) = policy.latest_version.clone() {
        if result.version.is_none() && version_is_lower(&result.current_version, &latest_version) {
            result.available = true;
            result.version = Some(latest_version);
        }
    }

    let required_target = policy
        .minimum_version
        .clone()
        .or(policy.latest_version.clone());

    let blocked = required_target
        .as_deref()
        .map(|version| {
            policy.required && version_is_lower(&result.current_version, version)
                || policy
                    .minimum_version
                    .as_deref()
                    .is_some_and(|minimum| version_is_lower(&result.current_version, minimum))
        })
        .unwrap_or(false);

    if blocked {
        result.blocked = true;
        result.block_reason = Some(policy.message.unwrap_or_else(|| {
            if let Some(minimum_version) = result.minimum_version.clone() {
                format!(
                    "当前客户端版本 v{} 过低，最低可用版本为 v{}，请先更新后再继续使用。",
                    result.current_version, minimum_version
                )
            } else if let Some(target_version) = required_target {
                format!(
                    "当前客户端版本 v{} 已停用，请更新到 v{} 或更高版本后继续使用。",
                    result.current_version, target_version
                )
            } else {
                "当前客户端版本已停用，请先更新后再继续使用。".to_string()
            }
        }));
    }

    result
}

async fn load_available_update(
    app: &tauri::AppHandle,
    policy: Option<&UpdatePolicy>,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let Some(config) = resolve_updater_config(policy) else {
        return Ok(None);
    };

    let updater = app
        .updater_builder()
        .pubkey(config.pubkey)
        .endpoints(config.endpoints)
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?;

    updater.check().await.map_err(|error| error.to_string())
}

#[tauri::command]
fn get_app_version_info() -> AppVersionInfo {
    AppVersionInfo {
        current_version: current_app_version(),
        update_configured: resolve_updater_config(None).is_some(),
    }
}

#[tauri::command]
async fn check_app_update(
    app: tauri::AppHandle,
    state: State<'_, BackendState>,
) -> Result<AppUpdateCheckResult, String> {
    let current_version = current_app_version();
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "license-update",
        action = "check_app_update",
        outcome = "started",
        current_version = %current_version,
        "app update check started"
    );
    let policy = load_update_policy(&state).await?;
    let configured = resolve_updater_config(policy.as_ref()).is_some();
    let update = match load_available_update(&app, policy.as_ref()).await {
        Ok(update) => update,
        Err(error)
            if policy
                .as_ref()
                .and_then(|policy| policy.download_url.as_ref())
                .is_some() =>
        {
            error!(
                target: "tauri.updater",
                error = %error,
                "signed updater check failed; falling back to platform download policy"
            );
            None
        }
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "license-update",
                action = "check_app_update",
                outcome = "failed",
                current_version = %current_version,
                error_code = "check_app_update_failed",
                error = %error,
                "app update check failed"
            );
            return Err(error);
        }
    };
    let result = apply_update_policy(
        update_check_result(configured, current_version, update),
        policy,
    );
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "license-update",
        action = "check_app_update",
        outcome = "success",
        current_version = %result.current_version,
        available = result.available,
        target_version = %result.version.clone().unwrap_or_default(),
        configured = result.configured,
        policy_configured = result.policy_configured,
        blocked = result.blocked,
        "app update check completed"
    );
    Ok(result)
}

#[tauri::command]
async fn install_app_update(
    app: tauri::AppHandle,
    state: State<'_, BackendState>,
    target_version: Option<String>,
) -> Result<AppUpdateInstallResult, String> {
    let expected_version_for_log = target_version
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    info!(
        target: "client.telemetry.action",
        stream = "usage",
        module = "license-update",
        action = "install_app_update",
        outcome = "started",
        target_version = %expected_version_for_log,
        "app update install started"
    );
    let policy = load_update_policy(&state).await.ok().flatten();
    let update = match load_available_update(&app, policy.as_ref()).await {
        Ok(update) => update,
        Err(error)
            if policy
                .as_ref()
                .and_then(|policy| policy.download_url.as_ref())
                .is_some() =>
        {
            error!(
                target: "tauri.updater",
                error = %error,
                "signed updater install path failed before download; falling back to platform installer"
            );
            None
        }
        Err(error) => {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "license-update",
                action = "install_app_update",
                outcome = "failed",
                target_version = %expected_version_for_log,
                error_code = "install_update_check_failed",
                error = %error,
                "app update install check failed"
            );
            return Err(error);
        }
    };

    let expected = target_version
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let (Some(update), Some(expected)) = (update.as_ref(), expected.as_deref()) {
        if update.version != expected {
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "license-update",
                action = "install_app_update",
                outcome = "failed",
                target_version = %expected,
                available_version = %update.version,
                error_code = "update_version_changed",
                "app update install rejected: version changed"
            );
            return Err(format!(
                "检测到的最新版本已变化：当前可安装 {}，期望 {}",
                update.version, expected
            ));
        }
    }

    if let Some(update) = update {
        if let Err(error) = update
            .download_and_install(|_downloaded, _total| {}, || {})
            .await
        {
            let error = error.to_string();
            warn!(
                target: "client.telemetry.action",
                stream = "runtime",
                module = "license-update",
                action = "install_app_update",
                outcome = "failed",
                target_version = %update.version,
                error_code = "tauri_update_install_failed",
                error = %error,
                "tauri updater download/install failed"
            );
            return Err(error);
        }
        let result = AppUpdateInstallResult {
            mode: "tauri_updater".to_string(),
            restart_required: true,
            message: format!("新版本 v{} 已安装，应用即将重启。", update.version),
            path: None,
        };
        info!(
            target: "client.telemetry.action",
            stream = "usage",
            module = "license-update",
            action = "install_app_update",
            outcome = "success",
            target_version = %update.version,
            mode = %result.mode,
            restart_required = result.restart_required,
            "app update install completed"
        );
        return Ok(result);
    }

    let Some(policy) = policy else {
        warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "license-update",
            action = "install_app_update",
            outcome = "failed",
            target_version = %expected_version_for_log,
            error_code = "update_policy_missing",
            "app update install rejected: update policy missing"
        );
        return Err("当前版本未配置更新服务或没有可用更新".to_string());
    };

    if let Some(expected) = expected.as_deref() {
        if let Some(latest_version) = policy.latest_version.as_deref() {
            if latest_version != expected {
                warn!(
                    target: "client.telemetry.action",
                    stream = "runtime",
                    module = "license-update",
                    action = "install_app_update",
                    outcome = "failed",
                    target_version = %expected,
                    available_version = %latest_version,
                    error_code = "update_version_changed",
                    "app update install rejected: policy version changed"
                );
                return Err(format!(
                    "检测到的最新版本已变化：当前可安装 {}，期望 {}",
                    latest_version, expected
                ));
            }
        }
    }

    let result = install_from_download_policy(&policy).await;
    match &result {
        Ok(result) => info!(
            target: "client.telemetry.action",
            stream = "usage",
            module = "license-update",
            action = "install_app_update",
            outcome = "success",
            target_version = %expected_version_for_log,
            mode = %result.mode,
            restart_required = result.restart_required,
            "app update install fallback completed"
        ),
        Err(error) => warn!(
            target: "client.telemetry.action",
            stream = "runtime",
            module = "license-update",
            action = "install_app_update",
            outcome = "failed",
            target_version = %expected_version_for_log,
            error_code = "install_update_failed",
            error = %error,
            "app update install failed"
        ),
    }
    result
}

#[tauri::command]
fn request_app_restart(app: tauri::AppHandle) {
    app.request_restart();
}

#[cfg(target_os = "android")]
mod android_update {
    use jni::{
        objects::{JClass, JObject, JString, JValue},
        JNIEnv,
    };

    const INSTALLER_CLASS: &str = "shop/gooofish/yuguanjia/AppUpdateInstaller";

    pub fn install_apk_from_url(download_url: &str, filename: &str) -> Result<String, String> {
        with_vm(|env| {
            let cls = load_app_class(env, INSTALLER_CLASS)?;
            let jurl: JString = env.new_string(download_url).ok()?;
            let jfilename: JString = env.new_string(filename).ok()?;
            let result = env
                .call_static_method(
                    &cls,
                    "installApkFromUrl",
                    "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&jurl), JValue::Object(&jfilename)],
                )
                .ok()?;
            let obj = result.l().ok()?;
            if obj.is_null() {
                return Some("安装包已下载，已打开系统安装器，请按提示完成安装。".to_string());
            }
            let jstr: JString = obj.into();
            let out = env.get_string(&jstr).ok()?;
            Some(out.to_string_lossy().into_owned())
        })
        .ok_or_else(|| "打开 Android 安装器失败".to_string())
    }

    fn with_vm<T>(f: impl FnOnce(&mut JNIEnv) -> Option<T>) -> Option<T> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm() as *mut _).ok()? };
        let mut env = vm.attach_current_thread().ok()?;
        f(&mut env)
    }

    fn load_app_class<'local>(env: &mut JNIEnv<'local>, class: &str) -> Option<JClass<'local>> {
        if let Ok(cls) = env.find_class(class) {
            return Some(cls);
        }
        if env.exception_check().ok()? {
            let _ = env.exception_clear();
        }

        let ctx = ndk_context::android_context();
        let context = unsafe { JObject::from_raw(ctx.context() as jni::sys::jobject) };
        let loader = env
            .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .ok()?
            .l()
            .ok()?;
        let fqcn = env.new_string(class.replace('/', ".")).ok()?;
        let cls_obj = env
            .call_method(
                &loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&fqcn)],
            )
            .ok()?
            .l()
            .ok()?;
        Some(JClass::from(cls_obj))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_update_policy, compare_versions, platform_download_url,
        update_policy_urls_from_config, version_is_lower, windows_update_installer_args,
        AppUpdateCheckResult, PlatformDownloadUrls, UpdatePolicy,
    };
    use std::cmp::Ordering;
    use xianyu_rust_backend_full::license_runtime::default_update_policy_url;

    #[test]
    fn compare_versions_uses_numeric_segments() {
        assert_eq!(compare_versions("0.1.10", "0.1.2"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "1.0"), Ordering::Equal);
        assert_eq!(compare_versions("2.0.0-beta", "2.0.0"), Ordering::Equal);
    }

    #[test]
    fn version_is_lower_handles_standard_semver() {
        assert!(version_is_lower("0.1.0", "0.2.0"));
        assert!(version_is_lower("1.9.9", "1.10.0"));
        assert!(!version_is_lower("1.10.0", "1.10.0"));
        assert!(!version_is_lower("1.10.1", "1.10.0"));
    }

    #[test]
    fn version_compare_uses_known_release_timeline_for_jump_versions() {
        assert!(version_is_lower("0.1.42", "0.1.5"));
        assert!(!version_is_lower("0.1.5", "0.1.42"));
        assert!(!version_is_lower("v0.1.5", "0.1.42"));
        assert!(!version_is_lower("0.1.5+6047a3e", "0.1.42"));
    }

    #[test]
    fn update_policy_does_not_block_current_jump_release() {
        let result = AppUpdateCheckResult {
            configured: false,
            policy_configured: false,
            current_version: "0.1.5".to_string(),
            available: false,
            version: None,
            body: None,
            date: None,
            minimum_version: None,
            blocked: false,
            block_reason: None,
            download_url: None,
            download_page_url: None,
        };
        let policy = UpdatePolicy {
            minimum_version: Some("0.1.42".to_string()),
            latest_version: Some("0.1.5".to_string()),
            body: None,
            required: true,
            message: Some(
                "当前客户端版本过低，最低可用版本为 v0.1.42，请更新后继续使用。".to_string(),
            ),
            download_url: None,
            download_page_url: Some("https://download.gooofish.shop/".to_string()),
            updater_endpoints: Vec::new(),
            updater_pubkey: None,
        };

        let merged = apply_update_policy(result, Some(policy));
        assert!(!merged.blocked);
        assert!(!merged.available);
    }

    #[test]
    fn update_policy_marks_newer_policy_version_available_with_download_metadata() {
        let result = AppUpdateCheckResult {
            configured: false,
            policy_configured: false,
            current_version: "0.1.0".to_string(),
            available: false,
            version: None,
            body: None,
            date: None,
            minimum_version: None,
            blocked: false,
            block_reason: None,
            download_url: None,
            download_page_url: None,
        };
        let policy = UpdatePolicy {
            minimum_version: None,
            latest_version: Some("0.2.0".to_string()),
            body: Some("1. 修复授权同步\n2. 优化推广页加载".to_string()),
            required: false,
            message: None,
            download_url: Some(
                "https://download.gooofish.shop/releases/yuguanjia-0.2.0.dmg".to_string(),
            ),
            download_page_url: Some("https://download.gooofish.shop/".to_string()),
            updater_endpoints: Vec::new(),
            updater_pubkey: None,
        };

        let merged = apply_update_policy(result, Some(policy));

        assert!(merged.policy_configured);
        assert!(merged.available);
        assert_eq!(merged.version.as_deref(), Some("0.2.0"));
        assert_eq!(
            merged.download_url.as_deref(),
            Some("https://download.gooofish.shop/releases/yuguanjia-0.2.0.dmg")
        );
        assert_eq!(
            merged.body.as_deref(),
            Some("1. 修复授权同步\n2. 优化推广页加载")
        );
    }

    #[test]
    fn update_policy_urls_never_fall_back_to_license_server() {
        let urls = update_policy_urls_from_config(None);

        assert_eq!(urls, vec![default_update_policy_url().to_string()]);
        assert!(urls
            .iter()
            .all(|url| !url.contains("license.gooofish.shop")));
    }

    #[test]
    fn update_policy_urls_reject_license_server_config() {
        let urls = update_policy_urls_from_config(Some(
            "https://license.gooofish.shop/api/client/update-policy".to_string(),
        ));

        assert_eq!(urls, vec![default_update_policy_url().to_string()]);
    }

    #[test]
    fn platform_download_url_selects_the_requested_platform_artifact() {
        let downloads = PlatformDownloadUrls {
            macos: Some("https://download.gooofish.shop/macos.dmg".to_string()),
            windows: Some("https://download.gooofish.shop/windows.exe".to_string()),
            android: Some("https://download.gooofish.shop/android.apk".to_string()),
        };

        assert_eq!(
            platform_download_url(&downloads, "macos").as_deref(),
            Some("https://download.gooofish.shop/macos.dmg")
        );
        assert_eq!(
            platform_download_url(&downloads, "windows").as_deref(),
            Some("https://download.gooofish.shop/windows.exe")
        );
        assert_eq!(
            platform_download_url(&downloads, "android").as_deref(),
            Some("https://download.gooofish.shop/android.apk")
        );
    }

    #[test]
    fn windows_downloaded_installer_uses_silent_update_args() {
        assert_eq!(windows_update_installer_args(), &["/S", "/R", "/UPDATE"]);
    }
}


#[tauri::command]
fn export_account_bundle(plaintext: String) -> Result<BundleEnvelope, String> {
    encrypt_bundle(plaintext.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_account_bundle(url: String) -> Result<String, String> {
    let bytes = decrypt_bundle(&url).map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| format!("bundle is not valid utf-8: {e}"))
}

#[tauri::command]
fn get_account_data_package_preview(
    state: State<'_, BackendState>,
) -> Result<AccountDataTransferPreview, String> {
    Ok(data_transfer::account_data_transfer_preview(
        &state.app_data_dir,
        "local-app",
    ))
}

#[tauri::command]
fn export_account_data_package(
    app: AppHandle,
    state: State<'_, BackendState>,
    sections: Vec<String>,
    output_path: Option<String>,
) -> Result<AccountDataExportResult, String> {
    if let Some(path) = output_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return data_transfer::export_account_data_package_to_path(
            &state.app_data_dir,
            &PathBuf::from(path),
            "local-app",
            &sections,
        );
    }
    let output_dir = app
        .path()
        .download_dir()
        .unwrap_or_else(|_| state.app_data_dir.join("exports"));
    data_transfer::export_account_data_package(
        &state.app_data_dir,
        &output_dir,
        "local-app",
        &sections,
    )
}

#[tauri::command]
fn inspect_account_data_package(
    state: State<'_, BackendState>,
    archive_base64: String,
) -> Result<AccountDataImportInspection, String> {
    data_transfer::inspect_account_data_package(&state.app_data_dir, "local-app", &archive_base64)
}

#[tauri::command]
fn restore_account_data_package(
    app: AppHandle,
    state: State<'_, BackendState>,
    import_token: String,
    sections: Vec<String>,
) -> Result<AccountDataRestoreResult, String> {
    let result = data_transfer::restore_account_data_package(
        &state.app_data_dir,
        "local-app",
        &import_token,
        &sections,
    )?;
    app.request_restart();
    Ok(result)
}

fn init_tracing() {
    let _ = tracing_subscriber::registry()
        .with(xianyu_rust_backend_full::logging::default_env_filter())
        .with(fmt::layer().with_target(true).compact())
        .with(xianyu_rust_backend_full::logging::memory_log_layer())
        .try_init();
}

/// Shared Tauri entry point. Called by `main.rs` on desktop and by the
/// auto-generated mobile entry on Android.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();
    let (port_tx, port_rx) = watch::channel(None);
    let server_handle: Arc<Mutex<Option<ServerHandle>>> = Arc::new(Mutex::new(None));

    let builder = tauri::Builder::default();
    #[cfg(not(target_os = "android"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        host_runtime::show_main_window(app);
    }));

    builder
        .invoke_handler(tauri::generate_handler![
            get_backend_port,
            set_app_badge_count,
            request_app_attention,
            get_app_version_info,
            check_app_update,
            install_app_update,
            request_app_restart,
            get_account_data_package_preview,
            export_account_data_package,
            inspect_account_data_package,
            restore_account_data_package,
            export_account_bundle,
            import_account_bundle
        ])
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup({
            let server_handle = Arc::clone(&server_handle);
            move |app| {
                let data_dir = match host_runtime::initialize_app_runtime(app) {
                    Ok(path) => path,
                    Err(error) => {
                        error!(
                            target: "tauri.runtime",
                            stream = "runtime",
                            module = "tauri-runtime",
                            action = "initialize_app_runtime_failed",
                            outcome = "error",
                            error_code = "initialize_app_runtime_failed",
                            retryable = false,
                            detail = %error,
                            "failed to initialize app runtime"
                        );
                        return Ok(());
                    }
                };

                app.manage(BackendState {
                    server_handle: Arc::clone(&server_handle),
                    port_rx: port_rx.clone(),
                    app_data_dir: data_dir.clone(),
                });

                host_runtime::install_desktop_tray(app)?;

                host_runtime::spawn_embedded_backend(
                    Arc::clone(&server_handle),
                    port_tx.clone(),
                );

                Ok(())
            }
        })
        .on_window_event(|window, event| {
            let state = window.state::<BackendState>();
            host_runtime::handle_window_event(window, event, &state.server_handle);
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
