#![recursion_limit = "512"]

pub mod app;
pub mod auth;
pub mod automation_raw_report;
pub mod automation_scheduler;
pub mod business_auto;
pub mod config;
pub mod control_plane_auth;
pub mod control_plane_tls;
pub mod cookie_refresh;
pub mod license_runtime;
pub mod logging;
pub mod models;
pub mod mtop_errors;
pub mod mtop_metrics;
pub mod notifications;
pub mod observability;
pub mod operations_runtime;
pub mod protocol_catalog;
pub mod protocol_runtime;
pub mod proxy_store;
pub mod routes;
pub mod runtime_supervisor;
pub mod sealed_protocol;
pub mod startup_preflight;
pub mod state;
pub mod storage;
pub mod telemetry;
pub mod xianyu_client;

use std::{net::SocketAddr, time::SystemTime};

use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tracing::{error, info};

use crate::{
    app::build_app,
    config::{AppConfig, ConfigError},
    runtime_supervisor::RuntimeSupervisor,
    state::AppState,
    storage::StorageError,
};

#[derive(Debug)]
pub enum BootstrapError {
    Io(std::io::Error),
    Config(ConfigError),
    StateBootstrap(StorageError),
    ServeJoin(tokio::task::JoinError),
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "bootstrap I/O error: {error}"),
            Self::Config(error) => write!(f, "bootstrap config error: {error}"),
            Self::StateBootstrap(error) => write!(f, "bootstrap state error: {error}"),
            Self::ServeJoin(error) => write!(f, "bootstrap serve task join error: {error}"),
        }
    }
}

impl std::error::Error for BootstrapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::StateBootstrap(error) => Some(error),
            Self::ServeJoin(error) => Some(error),
        }
    }
}

impl From<std::io::Error> for BootstrapError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ConfigError> for BootstrapError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

impl From<StorageError> for BootstrapError {
    fn from(value: StorageError) -> Self {
        Self::StateBootstrap(value)
    }
}

impl From<tokio::task::JoinError> for BootstrapError {
    fn from(value: tokio::task::JoinError) -> Self {
        Self::ServeJoin(value)
    }
}

pub struct ServerHandle {
    pub bind_addr: SocketAddr,
    pub service_name: String,
    runtime_supervisor: RuntimeSupervisor,
    shutdown_tx: Option<oneshot::Sender<()>>,
    shutdown_rx: oneshot::Receiver<()>,
    serve_shutdown_tx: Option<oneshot::Sender<()>>,
    serve_handle: JoinHandle<std::io::Result<()>>,
}

impl ServerHandle {
    pub async fn wait_for_shutdown(self) {
        let Self {
            bind_addr: _,
            service_name,
            runtime_supervisor,
            shutdown_tx,
            shutdown_rx,
            mut serve_shutdown_tx,
            serve_handle,
        } = self;

        tokio::pin!(shutdown_rx);

        let signal = wait_for_shutdown_signal(service_name.clone());
        tokio::pin!(signal);

        tokio::select! {
            _ = &mut signal => {}
            _ = &mut shutdown_rx => {}
        }

        drop(shutdown_tx);

        if let Some(tx) = serve_shutdown_tx.take() {
            let _ = tx.send(());
        }

        match serve_handle.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                error!(service = %service_name, %error, "server task exited with error");
            }
            Err(error) => {
                error!(service = %service_name, %error, "server task join failed");
            }
        }

        runtime_supervisor.shutdown_and_wait().await;

        info!(
            service = %service_name,
            terminated_at = ?SystemTime::now(),
            "bootstrap shutdown complete"
        );
    }

    pub async fn shutdown(&mut self) -> Result<(), BootstrapError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        Ok(())
    }
}

async fn wait_for_shutdown_signal(service_name: String) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(signal) => Some(signal),
            Err(error) => {
                error!(service = %service_name, %error, signal = "SIGINT", "failed to install shutdown signal handler");
                None
            }
        };
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => Some(signal),
            Err(error) => {
                error!(service = %service_name, %error, signal = "SIGTERM", "failed to install shutdown signal handler");
                None
            }
        };

        match (interrupt.as_mut(), terminate.as_mut()) {
            (Some(interrupt), Some(terminate)) => {
                tokio::select! {
                    _ = interrupt.recv() => info!(service = %service_name, signal = "SIGINT", "shutdown signal received"),
                    _ = terminate.recv() => info!(service = %service_name, signal = "SIGTERM", "shutdown signal received"),
                }
            }
            (Some(interrupt), None) => {
                let _ = interrupt.recv().await;
                info!(service = %service_name, signal = "SIGINT", "shutdown signal received");
            }
            (None, Some(terminate)) => {
                let _ = terminate.recv().await;
                info!(service = %service_name, signal = "SIGTERM", "shutdown signal received");
            }
            (None, None) => std::future::pending::<()>().await,
        }
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => info!(service = %service_name, signal = "SIGINT", "shutdown signal received"),
            Err(error) => {
                error!(service = %service_name, %error, "failed to install shutdown signal handler");
                std::future::pending::<()>().await;
            }
        }
    }
}

pub async fn run_server(config: AppConfig) -> Result<ServerHandle, BootstrapError> {
    let service_name = config.service_name.clone();
    info!(
        service = %config.service_name,
        addr = %config.bind_addr,
        "bootstrap starting"
    );

    let listener = TcpListener::bind(config.bind_addr).await?;
    let local_addr = listener.local_addr()?;
    let state = AppState::bootstrap(config).await?;
    let runtime_supervisor = state.start_runtime_supervisor();
    spawn_initial_startup_dependency_check(state.clone(), service_name.clone());
    spawn_control_plane_log_sync(state.clone());

    // Spawn listener-backed notification workers through the unified manager
    // lifecycle surface. Detached intentionally — runtime hooks reuse the
    // same manager entrypoint for per-channel restarts.
    tokio::spawn(notifications::manager::start_managed_channel_workers(
        state.clone(),
    ));

    crate::routes::accounts::datacompass_metrics::spawn_daily_metrics_sync_worker(state.clone());

    // Daily system-stats report scheduler — fires once per day at the
    // configured local time and pushes through every bind-all WeChat channel.
    // Detached; see `notifications::daily_report` for the lifecycle contract.
    let _ = notifications::daily_report::spawn_daily_report_scheduler(state.clone());

    let app = build_app(state);

    info!(
        service = %service_name,
        addr = %local_addr,
        "bootstrap listener ready"
    );

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (serve_shutdown_tx, serve_shutdown_rx) = oneshot::channel();
    let serve_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = serve_shutdown_rx.await;
            })
            .await
    });

    Ok(ServerHandle {
        bind_addr: local_addr,
        service_name,
        runtime_supervisor,
        shutdown_tx: Some(shutdown_tx),
        shutdown_rx,
        serve_shutdown_tx: Some(serve_shutdown_tx),
        serve_handle,
    })
}

fn spawn_initial_startup_dependency_check(state: AppState, service_name: String) {
    tokio::spawn(async move {
        let startup_dependencies =
            startup_preflight::run_startup_dependency_checks(state.config()).await;
        state
            .runtime()
            .update_startup_dependencies(startup_dependencies);
        if state.runtime().is_startup_blocked() {
            let _ = state
                .incident_reporter()
                .report_from_state(
                    &state,
                    crate::telemetry::IncidentInput {
                        error_level: "error".to_string(),
                        module: "startup".to_string(),
                        action: "dependency_check".to_string(),
                        route: "/health".to_string(),
                        error_code: "startup_blocked".to_string(),
                        error_message: state
                            .runtime()
                            .startup_block_summary()
                            .unwrap_or_else(|| "startup dependency blocked".to_string()),
                        http_status: None,
                        upstream_ret: String::new(),
                        retryable: true,
                        context_json: serde_json::json!({
                            "dependencies": state.runtime().inspection(service_name).dependencies,
                        }),
                        environment: None,
                    },
                )
                .await;
        }
    });
}

fn spawn_control_plane_log_sync(state: AppState) {
    let worker = crate::telemetry::LogSyncWorker::new(state);
    tokio::spawn(async move {
        let sync_interval = if cfg!(target_os = "android") {
            std::time::Duration::from_secs(120)
        } else {
            std::time::Duration::from_secs(20)
        };
        let mut interval = tokio::time::interval(sync_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if let Err(error) = worker.sync_once().await {
                tracing::debug!(error = %error, "control-plane log sync skipped");
            }
        }
    });
}
extern crate self as xianyu_rust_backend_full;
