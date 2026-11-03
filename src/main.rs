use tracing_subscriber::{fmt, prelude::*};
use xianyu_rust_backend_full::{config::AppConfig, run_server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let config = AppConfig::from_env()?;
    let handle = run_server(config).await?;
    handle.wait_for_shutdown().await;
    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(xianyu_rust_backend_full::logging::default_env_filter())
        .with(fmt::layer().with_target(true).compact())
        .with(xianyu_rust_backend_full::logging::memory_log_layer())
        .init();
}
