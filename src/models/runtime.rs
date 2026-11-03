use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInspection {
    pub service: String,
    pub version: String,
    pub state_backend: String,
    pub auth_configured: bool,
    pub ready: bool,
    pub started_at: String,
    pub startup_count: u64,
    pub databases: Vec<DatabaseStatus>,
    pub managers: Vec<RuntimeWorkerStatus>,
    pub cache_sync: CacheSyncRuntimeStatus,
    pub delete_runtime: DeleteRuntimeStatus,
    pub dependencies: StartupDependencyStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub ready: bool,
    pub service: String,
    pub state_backend: String,
    pub started_at: String,
    pub startup_blocked: bool,
    pub dependencies: StartupDependencyStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StartupDependencyStatus {
    pub startup_blocked: bool,
    pub summary: String,
    pub sign_service: StartupDependencyProbe,
    pub license_service: StartupDependencyProbe,
    pub version_policy: StartupDependencyProbe,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StartupDependencyProbe {
    pub name: String,
    pub url: String,
    pub ok: bool,
    pub critical: bool,
    pub status_code: Option<u16>,
    pub detail: String,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseStatus {
    pub name: String,
    pub path: String,
    pub migrations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkerStatus {
    pub name: String,
    pub status: String,
    pub tick_count: u64,
    pub last_tick_at: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheSyncRuntimeStatus {
    pub status: String,
    pub cached_uids: Vec<String>,
    pub total_entries: u64,
    pub last_sync_at: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeleteRuntimeStatus {
    pub running_job_ids: Vec<i64>,
    pub running_count: u64,
    pub pending_count: u64,
    pub retryable_count: u64,
    pub persisted_running_count: u64,
    pub runtime_ready: bool,
    pub last_poll_at: f64,
    pub last_runtime_error: String,
    pub last_recovered_count: u64,
    pub claim_batch_size: u64,
    pub action_capacity: u64,
}
