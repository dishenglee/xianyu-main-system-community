use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(not(target_os = "android"))]
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
};
use tauri::{App, AppHandle, Manager, Runtime, Window, WindowEvent};
use tokio::{sync::watch, time::timeout};
use tracing::{error, info, warn};

use crate::{data_import, data_transfer};
use xianyu_rust_backend_full::{config::AppConfig, run_server, ServerHandle};

const APP_DATA_DIR_ENV: &str = "APP_DATA_DIR";
const LIVE_RUNTIME_LOCK_DIR_ENV: &str = "XIANYU_LIVE_RUNTIME_LOCK_DIR";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownMode {
    MainWindowClose,
    HostLifecycle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimePolicy {
    pub shutdown_mode: ShutdownMode,
    pub publishes_mobile_bridge: bool,
    pub close_to_tray: bool,
}

#[cfg(target_os = "android")]
const CURRENT_RUNTIME_POLICY: RuntimePolicy = RuntimePolicy {
    shutdown_mode: ShutdownMode::HostLifecycle,
    publishes_mobile_bridge: true,
    close_to_tray: false,
};

#[cfg(not(target_os = "android"))]
const CURRENT_RUNTIME_POLICY: RuntimePolicy = RuntimePolicy {
    shutdown_mode: ShutdownMode::MainWindowClose,
    publishes_mobile_bridge: false,
    close_to_tray: true,
};

pub fn runtime_policy() -> RuntimePolicy {
    CURRENT_RUNTIME_POLICY
}

impl RuntimePolicy {
    pub const fn shutdowns_with_main_window(self) -> bool {
        matches!(self.shutdown_mode, ShutdownMode::MainWindowClose)
    }

    pub const fn shutdown_label(self) -> &'static str {
        match self.shutdown_mode {
            ShutdownMode::MainWindowClose => "main-window-close",
            ShutdownMode::HostLifecycle => "host-lifecycle",
        }
    }
}

pub fn prepare_app_data_dir(data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir).map_err(|error| {
        format!(
            "failed to create app data dir {}: {error}",
            data_dir.display()
        )
    })?;

    match data_transfer::apply_pending_account_data_import(data_dir) {
        Ok(Some(restored)) => {
            info!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "tauri-runtime",
                action = "account_data_import_applied",
                outcome = "success",
                sections = %restored.join("、"),
                "account data import applied before backend startup"
            );
        }
        Ok(None) => {}
        Err(error) => {
            warn!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "tauri-runtime",
                action = "account_data_import_apply_failed",
                outcome = "error",
                detail = %error,
                "failed to apply pending account data import"
            );
        }
    }

    let current_dir = env::current_dir().ok();
    let allow_local_import = data_import::local_data_import_enabled();
    if let Some(report) = data_import::seed_app_data_dir_from_local_sources(
        data_dir,
        current_dir.as_deref(),
        allow_local_import,
    )
    .map_err(|error| {
        format!(
            "failed to seed app data dir {}: {error}",
            data_dir.display()
        )
    })? {
        for copied in report.copied {
            info!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "tauri-runtime",
                action = "seed_app_data_copied",
                outcome = "success",
                "migrated {} -> {}",
                report.source.display(),
                copied.display()
            );
        }
        for skipped in report.skipped {
            info!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "tauri-runtime",
                action = "seed_app_data_skipped",
                outcome = "success",
                path = %skipped.display(),
                "skipped app data seed entry"
            );
        }
    }
    if !allow_local_import {
        info!(
            target: "tauri.runtime",
            stream = "runtime",
            module = "tauri-runtime",
            action = "seed_app_data_disabled",
            outcome = "success",
            "local app data seed disabled; set {}=1 in debug builds to import local data",
            data_import::LOCAL_DATA_IMPORT_ENV
        );
    }

    if env::var(APP_DATA_DIR_ENV)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        unsafe {
            env::set_var(APP_DATA_DIR_ENV, data_dir);
        }
    }
    if env::var(LIVE_RUNTIME_LOCK_DIR_ENV)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true)
    {
        unsafe {
            env::set_var(LIVE_RUNTIME_LOCK_DIR_ENV, data_dir.join("runtime-locks"));
        }
    }

    info!(
        target: "tauri.runtime",
        stream = "runtime",
        module = "tauri-runtime",
        action = "prepare_app_data_dir",
        outcome = "success",
        data_dir = %data_dir.display(),
        "backend data dir ready"
    );
    Ok(())
}

pub fn initialize_app_runtime<R: Runtime + 'static>(
    app: &App<R>,
) -> Result<PathBuf, String> {
    open_main_devtools_if_enabled(app);

    let default_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data dir: {error}"))?;
    let data_dir = resolve_app_data_dir(default_data_dir);

    prepare_app_data_dir(&data_dir)?;
    let policy = runtime_policy();
    info!(
        target: "tauri.runtime",
        stream = "runtime",
        module = "tauri-runtime",
        action = "runtime_policy_loaded",
        outcome = "success",
        shutdown_mode = policy.shutdown_label(),
        mobile_bridge = policy.publishes_mobile_bridge,
        "runtime policy ready"
    );

    Ok(data_dir)
}

#[cfg(not(target_os = "android"))]
struct DesktopTray<R: Runtime>(#[allow(dead_code)] TrayIcon<R>);

#[cfg(not(target_os = "android"))]
pub fn install_desktop_tray<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    if !runtime_policy().close_to_tray {
        return Ok(());
    }

    let show_item = MenuItem::with_id(app, "show_main", "打开鱼管家", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit_app", "退出鱼管家", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;
    let mut builder = TrayIconBuilder::<R>::with_id("main-tray")
        .tooltip("鱼管家正在后台运行")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show_main" => show_main_window(app),
            "quit_app" => {
                if let Some(state) = app.try_state::<crate::BackendState>() {
                    shutdown_embedded_backend(&state.server_handle);
                }
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let tray = builder.build(app)?;
    app.manage(DesktopTray(tray));
    Ok(())
}

#[cfg(target_os = "android")]
pub fn install_desktop_tray<R: Runtime>(_app: &App<R>) -> tauri::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "android"))]
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(error) = window.show() {
            warn!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "desktop-tray",
                action = "show_window_failed",
                outcome = "warn",
                error_code = "show_window_failed",
                detail = %error,
                "failed to show main window"
            );
        }
        if let Err(error) = window.unminimize() {
            warn!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "desktop-tray",
                action = "unminimize_window_failed",
                outcome = "warn",
                error_code = "unminimize_window_failed",
                detail = %error,
                "failed to unminimize main window"
            );
        }
        if let Err(error) = window.set_focus() {
            warn!(
                target: "tauri.runtime",
                stream = "runtime",
                module = "desktop-tray",
                action = "focus_window_failed",
                outcome = "warn",
                error_code = "focus_window_failed",
                detail = %error,
                "failed to focus main window"
            );
        }
    }
}

fn resolve_app_data_dir(default_data_dir: PathBuf) -> PathBuf {
    let Some(value) = env::var(APP_DATA_DIR_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return default_data_dir;
    };

    PathBuf::from(value)
}

pub fn spawn_embedded_backend(
    server_handle_slot: Arc<Mutex<Option<ServerHandle>>>,
    port_tx: watch::Sender<Option<u16>>,
) {
    tauri::async_runtime::spawn(async move {
        let mut config = match AppConfig::from_env() {
            Ok(config) => config,
            Err(error) => {
                error!(
                    target: "tauri.runtime",
                    stream = "runtime",
                    module = "embedded-backend",
                    action = "load_config_failed",
                    outcome = "error",
                    error_code = "backend_config_failed",
                    retryable = false,
                    detail = %error,
                    "failed to load backend config"
                );
                return;
            }
        };
        config.bind_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();

        match run_server(config).await {
            Ok(handle) => {
                let port = handle.bind_addr.port();
                unsafe {
                    env::set_var("XIANYU_LOCAL_BACKEND_PORT", port.to_string());
                }
                if port_tx.send(Some(port)).is_err() {
                    warn!(
                        target: "tauri.runtime",
                        stream = "runtime",
                        module = "embedded-backend",
                        action = "publish_backend_port_dropped",
                        outcome = "warn",
                        error_code = "backend_port_receiver_dropped",
                        retryable = false,
                        "backend port receiver dropped before startup completed"
                    );
                }
                publish_backend_ready(
                    port,
                    xianyu_rust_backend_full::sealed_protocol::sealed_str!("/api/live/ws"),
                );
                *server_handle_slot
                    .lock()
                    .expect("backend server handle mutex poisoned") = Some(handle);
            }
            Err(error) => {
                error!(
                    target: "tauri.runtime",
                    stream = "runtime",
                    module = "embedded-backend",
                    action = "start_failed",
                    outcome = "error",
                    error_code = "embedded_backend_start_failed",
                    retryable = true,
                    detail = %error,
                    "failed to start embedded backend"
                );
            }
        }
    });
}

pub fn shutdown_embedded_backend(server_handle_slot: &Arc<Mutex<Option<ServerHandle>>>) {
    let maybe_handle = server_handle_slot
        .lock()
        .expect("backend server handle mutex poisoned")
        .take();
    if let Some(mut handle) = maybe_handle {
        tauri::async_runtime::block_on(async move {
            if let Err(error) = handle.shutdown().await {
                error!(
                    target: "tauri.runtime",
                    stream = "runtime",
                    module = "embedded-backend",
                    action = "shutdown_request_failed",
                    outcome = "error",
                    error_code = "embedded_backend_shutdown_failed",
                    retryable = true,
                    detail = %error,
                    "failed to request backend shutdown"
                );
            }
            let service_name = handle.service_name.clone();
            if timeout(Duration::from_secs(3), handle.wait_for_shutdown())
                .await
                .is_err()
            {
                warn!(
                    target: "tauri.runtime",
                    stream = "runtime",
                    module = "embedded-backend",
                    action = "shutdown_timeout",
                    outcome = "warn",
                    error_code = "embedded_backend_shutdown_timeout",
                    retryable = true,
                    service_name = %service_name,
                    "backend service did not shut down within 3s"
                );
            }
        });
    }
}

pub fn handle_main_window_close(server_handle_slot: &Arc<Mutex<Option<ServerHandle>>>) {
    if runtime_policy().shutdowns_with_main_window() {
        shutdown_embedded_backend(server_handle_slot);
    }
}

pub fn handle_window_event(
    window: &Window,
    event: &WindowEvent,
    server_handle_slot: &Arc<Mutex<Option<ServerHandle>>>,
) {
    if window.label() != "main" {
        return;
    }

    if let WindowEvent::CloseRequested { api, .. } = event {
        #[cfg(not(target_os = "android"))]
        if runtime_policy().close_to_tray {
            api.prevent_close();
            if let Err(error) = window.hide() {
                warn!(
                    target: "tauri.runtime",
                    stream = "runtime",
                    module = "desktop-tray",
                    action = "hide_window_failed",
                    outcome = "warn",
                    error_code = "hide_window_failed",
                    detail = %error,
                    "failed to hide main window"
                );
            }
            return;
        }
        #[cfg(target_os = "android")]
        let _ = api;
        handle_main_window_close(server_handle_slot);
    }
}

fn open_main_devtools_if_enabled<R: Runtime>(app: &App<R>) {
    #[cfg(all(debug_assertions, desktop, feature = "devtools"))]
    {
        if let Some(window) = app.get_webview_window("main") {
            window.open_devtools();
        }
    }
}

#[cfg(target_os = "android")]
fn publish_backend_ready(port: u16, live_path: &str) {
    use jni::{
        objects::{JClass, JObject, JString, JValue},
        JNIEnv,
    };

    fn with_vm<T>(f: impl FnOnce(&mut JNIEnv) -> Option<T>) -> Option<T> {
        let ctx = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm() as *mut _).ok()? };
        let mut env = vm.attach_current_thread().ok()?;
        f(&mut env)
    }

    info!(
        target: "tauri.runtime",
        stream = "runtime",
        module = "android-bridge",
        action = "publish_backend_ready",
        outcome = "success",
        port = port,
        path_len = live_path.len(),
        "publishing backend to NativeBridge"
    );

    let result = with_vm(|env| {
        let ctx = ndk_context::android_context();
        let context = unsafe { JObject::from_raw(ctx.context() as jni::sys::jobject) };
        let loader = env
            .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])
            .ok()?
            .l()
            .ok()?;
        let fqcn: JString = env
            .new_string("shop.gooofish.yuguanjia.NativeBridge")
            .ok()?;
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
        let cls = JClass::from(cls_obj);
        let jlive_path: JString = env.new_string(live_path).ok()?;
        env.call_static_method(
            &cls,
            "updateBackend",
            "(ILjava/lang/String;)V",
            &[
                JValue::Int(i32::from(port)),
                JValue::Object(&jlive_path),
            ],
        )
        .ok()?;
        Some(())
    });

    if result.is_none() {
        warn!(
            target: "tauri.runtime",
            stream = "runtime",
            module = "android-bridge",
            action = "publish_backend_ready_failed",
            outcome = "warn",
            error_code = "native_bridge_unreachable",
            retryable = true,
            "publish_backend_ready failed to reach NativeBridge"
        );
    } else {
        info!(
            target: "tauri.runtime",
            stream = "runtime",
            module = "android-bridge",
            action = "publish_backend_ready_applied",
            outcome = "success",
            "NativeBridge updated"
        );
    }
}

#[cfg(not(target_os = "android"))]
fn publish_backend_ready(_port: u16, _live_path: &str) {}

#[cfg(test)]
mod tests {
    use super::{
        prepare_app_data_dir, resolve_app_data_dir, runtime_policy,
        ShutdownMode, LIVE_RUNTIME_LOCK_DIR_ENV,
    };
    use std::{path::PathBuf, sync::Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn runtime_policy_matches_current_target() {
        let policy = runtime_policy();

        #[cfg(target_os = "android")]
        {
            assert_eq!(policy.shutdown_mode, ShutdownMode::HostLifecycle);
            assert!(policy.publishes_mobile_bridge);
        }

        #[cfg(not(target_os = "android"))]
        {
            assert_eq!(policy.shutdown_mode, ShutdownMode::MainWindowClose);
            assert!(!policy.publishes_mobile_bridge);
        }
    }

    #[test]
    fn resolve_app_data_dir_prefers_explicit_override() {
        let temp = tempfile::tempdir().unwrap();
        let override_dir = temp.path().join("override");
        let fallback_dir = temp.path().join("fallback");

        unsafe {
            std::env::set_var("APP_DATA_DIR", override_dir.display().to_string());
        }
        let resolved = resolve_app_data_dir(fallback_dir);
        unsafe {
            std::env::remove_var("APP_DATA_DIR");
        }

        assert_eq!(resolved, override_dir);
    }

    #[test]
    fn resolve_app_data_dir_uses_fallback_when_override_missing() {
        let fallback_dir = PathBuf::from("/tmp/fallback");
        unsafe {
            std::env::remove_var("APP_DATA_DIR");
        }
        assert_eq!(resolve_app_data_dir(fallback_dir.clone()), fallback_dir);
    }

    #[test]
    fn prepare_app_data_dir_sets_live_runtime_lock_dir() {
        let temp = tempfile::tempdir().unwrap();
        let old_value = std::env::var(LIVE_RUNTIME_LOCK_DIR_ENV).ok();
        unsafe {
            std::env::remove_var(LIVE_RUNTIME_LOCK_DIR_ENV);
        }

        prepare_app_data_dir(temp.path()).unwrap();

        assert_eq!(
            std::env::var(LIVE_RUNTIME_LOCK_DIR_ENV).unwrap(),
            temp.path().join("runtime-locks").display().to_string()
        );

        unsafe {
            if let Some(value) = old_value {
                std::env::set_var(LIVE_RUNTIME_LOCK_DIR_ENV, value);
            } else {
                std::env::remove_var(LIVE_RUNTIME_LOCK_DIR_ENV);
            }
        }
    }

}
