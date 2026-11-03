use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyItemRecord {
    pub url: String,
    pub label: String,
    pub fail_count: i64,
    pub success_count: i64,
    pub last_used: f64,
    pub last_check: f64,
    pub alive: bool,
    pub avg_latency_ms: f64,
    pub deadline: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProxySnapshot {
    enabled: bool,
    strategy: String,
    proxies: Vec<ProxyItemRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyCheckResult {
    pub alive: bool,
    pub latency_ms: f64,
    pub ip: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProxyStatus {
    pub enabled: bool,
    pub strategy: String,
    pub total: usize,
    pub alive: usize,
}

#[derive(Debug, Clone)]
pub struct ProxyState {
    inner: Arc<Mutex<ProxySnapshot>>,
    config_path: PathBuf,
}

impl ProxyState {
    pub fn bootstrap(data_dir: &Path) -> Self {
        let config_path = data_dir.join("proxy_state.json");
        let snapshot = load_snapshot(&config_path);
        Self {
            inner: Arc::new(Mutex::new(snapshot)),
            config_path,
        }
    }

    pub fn status(&self) -> ProxyStatus {
        let snapshot = self.inner.lock().unwrap();
        ProxyStatus {
            enabled: snapshot.enabled,
            strategy: snapshot.strategy.clone(),
            total: snapshot.proxies.len(),
            alive: snapshot.proxies.iter().filter(|item| item.alive).count(),
        }
    }

    pub fn list_proxies(&self) -> Vec<ProxyItemRecord> {
        self.inner.lock().unwrap().proxies.clone()
    }

    pub fn add_proxy(&self, url: &str, label: &str) -> bool {
        let normalized_url = url.trim();
        if normalized_url.is_empty() {
            return false;
        }

        let mut snapshot = self.inner.lock().unwrap();
        if snapshot
            .proxies
            .iter()
            .any(|item| item.url == normalized_url)
        {
            return false;
        }

        snapshot.proxies.push(ProxyItemRecord {
            url: normalized_url.to_string(),
            label: label.trim().to_string(),
            fail_count: 0,
            success_count: 0,
            last_used: 0.0,
            last_check: 0.0,
            alive: true,
            avg_latency_ms: 0.0,
            deadline: 0.0,
        });
        save_snapshot(&self.config_path, &snapshot);
        true
    }

    pub fn remove_proxy(&self, url: &str) -> bool {
        let mut snapshot = self.inner.lock().unwrap();
        let before = snapshot.proxies.len();
        snapshot.proxies.retain(|item| item.url != url);
        let removed = snapshot.proxies.len() != before;
        if removed {
            save_snapshot(&self.config_path, &snapshot);
        }
        removed
    }

    pub fn set_enabled(&self, enabled: bool) {
        let mut snapshot = self.inner.lock().unwrap();
        snapshot.enabled = enabled;
        save_snapshot(&self.config_path, &snapshot);
    }

    pub fn set_strategy(&self, strategy: &str) {
        let mut snapshot = self.inner.lock().unwrap();
        snapshot.strategy = strategy.to_string();
        save_snapshot(&self.config_path, &snapshot);
    }

    pub fn reset_proxy(&self, url: &str) {
        let mut snapshot = self.inner.lock().unwrap();
        if let Some(proxy) = snapshot.proxies.iter_mut().find(|item| item.url == url) {
            proxy.fail_count = 0;
            proxy.alive = true;
            proxy.last_check = now_seconds();
        }
        save_snapshot(&self.config_path, &snapshot);
    }

    pub fn check_proxy(&self, url: &str) -> ProxyCheckResult {
        let result = simulate_proxy_check(url);
        let mut snapshot = self.inner.lock().unwrap();
        if let Some(proxy) = snapshot.proxies.iter_mut().find(|item| item.url == url) {
            proxy.last_check = now_seconds();
            proxy.last_used = now_seconds();
            proxy.alive = result.alive;
            if result.alive {
                proxy.fail_count = 0;
                proxy.success_count += 1;
                proxy.avg_latency_ms = result.latency_ms;
            } else {
                proxy.fail_count += 1;
            }
        }
        save_snapshot(&self.config_path, &snapshot);
        result
    }

    pub fn check_all(&self) -> Vec<(String, String, ProxyCheckResult)> {
        let urls = self
            .inner
            .lock()
            .unwrap()
            .proxies
            .iter()
            .map(|item| (item.url.clone(), item.label.clone()))
            .collect::<Vec<_>>();

        let mut results = Vec::with_capacity(urls.len());
        for (url, label) in urls {
            let result = self.check_proxy(&url);
            results.push((url, label, result));
        }
        results
    }
}

fn load_snapshot(config_path: &Path) -> ProxySnapshot {
    let defaults = ProxySnapshot {
        enabled: false,
        strategy: "round-robin".to_string(),
        proxies: Vec::new(),
    };

    if !config_path.exists() {
        return defaults;
    }

    fs::read_to_string(config_path)
        .ok()
        .and_then(|content| serde_json::from_str::<ProxySnapshot>(&content).ok())
        .unwrap_or(defaults)
}

fn save_snapshot(config_path: &Path, snapshot: &ProxySnapshot) {
    if let Some(parent) = config_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(snapshot) {
        let _ = fs::write(config_path, content);
    }
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn simulate_proxy_check(url: &str) -> ProxyCheckResult {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return ProxyCheckResult {
            alive: false,
            latency_ms: 0.0,
            ip: String::new(),
            error: "empty proxy url".to_string(),
        };
    }

    let scheme = trimmed.split("://").next().unwrap_or_default();
    if !matches!(scheme, "http" | "https" | "socks5") {
        return ProxyCheckResult {
            alive: false,
            latency_ms: 0.0,
            ip: String::new(),
            error: "unsupported proxy scheme".to_string(),
        };
    }

    if trimmed.contains("offline") || trimmed.contains("invalid") {
        return ProxyCheckResult {
            alive: false,
            latency_ms: 0.0,
            ip: String::new(),
            error: "proxy unreachable".to_string(),
        };
    }

    let authority = trimmed
        .split("://")
        .nth(1)
        .unwrap_or(trimmed)
        .split('@')
        .last()
        .unwrap_or_default()
        .split('/')
        .next()
        .unwrap_or_default();
    let host = authority.split(':').next().unwrap_or_default();
    let latency_ms = 35.0 + (host.len() % 20) as f64;

    ProxyCheckResult {
        alive: true,
        latency_ms,
        ip: if host.is_empty() {
            "simulated".to_string()
        } else {
            host.to_string()
        },
        error: String::new(),
    }
}
