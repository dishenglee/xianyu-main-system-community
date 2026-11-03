//! In-memory MTOP call metrics, keyed by API name.
//!
//! Counts success / error breakdown per API, with a sliding "recent anomaly"
//! flag used by `/api/health/mtop` to surface upstream breakage in near
//! real-time.
//!
//! Deliberately in-memory only: these are operational signals, not business
//! records. Persisting would conflate the two and add SQLite write pressure
//! on a hot path. If we ever want historical MTOP health, wire this up to a
//! Prometheus exporter — the shape is already compatible.

use std::{
    collections::BTreeMap,
    sync::{Mutex, RwLock},
    time::{Duration, Instant},
};

use serde_json::{json, Value};

use crate::mtop_errors::MtopErrorKind;

/// Anomaly entries older than this are pruned from the "recent" view.
const RECENT_ANOMALY_WINDOW: Duration = Duration::from_secs(600);
/// Cap the per-API recent-anomaly ring so a stuck API can't blow memory.
const MAX_RECENT_ANOMALIES_PER_API: usize = 20;

#[derive(Debug, Default, Clone)]
pub struct ApiCounters {
    pub ok: u64,
    pub token_expired: u64,
    pub session_expired: u64,
    pub sign_invalid: u64,
    pub schema_changed: u64,
    pub risk_control: u64,
    pub biz_error: u64,
    pub unknown: u64,
}

impl ApiCounters {
    pub fn total(&self) -> u64 {
        self.ok
            + self.token_expired
            + self.session_expired
            + self.sign_invalid
            + self.schema_changed
            + self.risk_control
            + self.biz_error
            + self.unknown
    }

    pub fn anomaly_total(&self) -> u64 {
        self.sign_invalid + self.schema_changed + self.unknown
    }

    fn record(&mut self, kind: &MtopErrorKind) {
        match kind {
            MtopErrorKind::Ok => self.ok += 1,
            MtopErrorKind::TokenExpired => self.token_expired += 1,
            MtopErrorKind::SessionExpired => self.session_expired += 1,
            MtopErrorKind::SignInvalid => self.sign_invalid += 1,
            MtopErrorKind::SchemaChanged => self.schema_changed += 1,
            MtopErrorKind::RiskControl => self.risk_control += 1,
            MtopErrorKind::BizError(_) => self.biz_error += 1,
            MtopErrorKind::Unknown(_) => self.unknown += 1,
        }
    }

    fn as_json(&self) -> Value {
        json!({
            "total": self.total(),
            "ok": self.ok,
            "token_expired": self.token_expired,
            "session_expired": self.session_expired,
            "sign_invalid": self.sign_invalid,
            "schema_changed": self.schema_changed,
            "risk_control": self.risk_control,
            "biz_error": self.biz_error,
            "unknown": self.unknown,
            "anomaly_total": self.anomaly_total(),
        })
    }
}

#[derive(Debug, Clone)]
struct RecentAnomaly {
    at: Instant,
    kind: String,
    detail: String,
}

#[derive(Debug, Default)]
struct ApiState {
    counters: ApiCounters,
    recent: Vec<RecentAnomaly>,
}

#[derive(Debug, Default)]
pub struct MtopMetrics {
    per_api: RwLock<BTreeMap<String, Mutex<ApiState>>>,
}

impl MtopMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, api: &str, kind: &MtopErrorKind) {
        // Fast path: entry exists, just bump under its per-API mutex.
        if let Some(cell) = self.per_api.read().unwrap().get(api) {
            let mut state = cell.lock().unwrap();
            state.counters.record(kind);
            if kind.is_anomaly() {
                push_anomaly(&mut state.recent, kind);
            }
            return;
        }
        // Slow path: take the write lock once to insert, then record.
        let mut map = self.per_api.write().unwrap();
        let cell = map.entry(api.to_string()).or_default();
        let mut state = cell.lock().unwrap();
        state.counters.record(kind);
        if kind.is_anomaly() {
            push_anomaly(&mut state.recent, kind);
        }
    }

    /// Snapshot for the `/api/health/mtop` dashboard.
    pub fn snapshot(&self) -> Value {
        let map = self.per_api.read().unwrap();
        let mut apis: Vec<Value> = Vec::with_capacity(map.len());
        let mut totals = ApiCounters::default();
        let mut anomaly_apis: Vec<Value> = Vec::new();

        for (name, cell) in map.iter() {
            let mut state = cell.lock().unwrap();
            prune_recent(&mut state.recent);

            totals.ok += state.counters.ok;
            totals.token_expired += state.counters.token_expired;
            totals.session_expired += state.counters.session_expired;
            totals.sign_invalid += state.counters.sign_invalid;
            totals.schema_changed += state.counters.schema_changed;
            totals.risk_control += state.counters.risk_control;
            totals.biz_error += state.counters.biz_error;
            totals.unknown += state.counters.unknown;

            let recent_json: Vec<Value> = state
                .recent
                .iter()
                .rev()
                .take(5)
                .map(|entry| {
                    json!({
                        "kind": entry.kind,
                        "detail": entry.detail,
                        "ageSecs": entry.at.elapsed().as_secs(),
                    })
                })
                .collect();

            let mut api_json = state.counters.as_json();
            api_json["api"] = Value::String(name.clone());
            api_json["recentAnomalies"] = Value::Array(recent_json.clone());

            if state.counters.anomaly_total() > 0 {
                anomaly_apis.push(api_json.clone());
            }
            apis.push(api_json);
        }

        apis.sort_by(|a, b| {
            let an = a.get("anomaly_total").and_then(Value::as_u64).unwrap_or(0);
            let bn = b.get("anomaly_total").and_then(Value::as_u64).unwrap_or(0);
            bn.cmp(&an)
        });

        json!({
            "totals": totals.as_json(),
            "apis": apis,
            "anomalyApis": anomaly_apis,
        })
    }
}

fn push_anomaly(buf: &mut Vec<RecentAnomaly>, kind: &MtopErrorKind) {
    let detail = match kind {
        MtopErrorKind::Unknown(raw) => raw.clone(),
        MtopErrorKind::BizError(raw) => raw.clone(),
        _ => String::new(),
    };
    buf.push(RecentAnomaly {
        at: Instant::now(),
        kind: kind.tag().to_string(),
        detail,
    });
    prune_recent(buf);
}

fn prune_recent(buf: &mut Vec<RecentAnomaly>) {
    buf.retain(|entry| entry.at.elapsed() < RECENT_ANOMALY_WINDOW);
    if buf.len() > MAX_RECENT_ANOMALIES_PER_API {
        let drop = buf.len() - MAX_RECENT_ANOMALIES_PER_API;
        buf.drain(0..drop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_bucket_by_kind() {
        let m = MtopMetrics::new();
        m.record("x.api", &MtopErrorKind::Ok);
        m.record("x.api", &MtopErrorKind::Ok);
        m.record("x.api", &MtopErrorKind::TokenExpired);
        m.record("x.api", &MtopErrorKind::SignInvalid);

        let snap = m.snapshot();
        let totals = &snap["totals"];
        assert_eq!(totals["ok"], 2);
        assert_eq!(totals["token_expired"], 1);
        assert_eq!(totals["sign_invalid"], 1);
        assert_eq!(totals["anomaly_total"], 1);
    }

    #[test]
    fn anomalies_recorded_in_recent_ring() {
        let m = MtopMetrics::new();
        m.record(
            "y.api",
            &MtopErrorKind::Unknown("FAIL_SYS_WHATEVER::??".into()),
        );
        let snap = m.snapshot();
        let api = snap["apis"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["api"] == "y.api")
            .expect("api present");
        let recent = api["recentAnomalies"].as_array().unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0]["kind"], "unknown");
        assert!(recent[0]["detail"]
            .as_str()
            .unwrap()
            .contains("FAIL_SYS_WHATEVER"));
    }

    #[test]
    fn recent_ring_is_capped() {
        let m = MtopMetrics::new();
        for i in 0..50 {
            m.record("z.api", &MtopErrorKind::Unknown(format!("CODE_{i}::boom")));
        }
        let map = m.per_api.read().unwrap();
        let state = map.get("z.api").unwrap().lock().unwrap();
        assert!(state.recent.len() <= MAX_RECENT_ANOMALIES_PER_API);
    }
}
