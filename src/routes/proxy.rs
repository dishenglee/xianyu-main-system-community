use axum::{
    extract::{Extension, Json, State},
    routing::{get, post},
    Json as ResponseJson, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    models::auth::RequestIdentity,
    routes::error::{ApiResult, ResponseError},
    routes::tenant::ensure_admin_or_legacy,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/status"),
            get(proxy_status),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/list"),
            get(proxy_list),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/add"),
            post(add_proxy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/batch_add"),
            post(batch_add_proxy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/remove"),
            post(remove_proxy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/set_enabled"),
            post(set_enabled),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/set_strategy"),
            post(set_strategy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/reset"),
            post(reset_proxy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/check"),
            post(check_proxy),
        )
        .route(
            crate::sealed_protocol::sealed_str!("/proxy/check_all"),
            post(check_all_proxies),
        )
}

#[derive(Debug, Deserialize)]
struct AddProxyRequest {
    url: String,
    #[serde(default)]
    label: String,
}

#[derive(Debug, Deserialize)]
struct BatchAddProxyRequest {
    proxies: Vec<AddProxyRequest>,
}

#[derive(Debug, Deserialize)]
struct RemoveProxyRequest {
    url: String,
}

#[derive(Debug, Deserialize)]
struct SetEnabledRequest {
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SetStrategyRequest {
    strategy: String,
}

#[derive(Debug, Deserialize)]
struct ResetProxyRequest {
    url: String,
}

#[derive(Debug, Deserialize)]
struct CheckProxyRequest {
    url: String,
}

async fn proxy_status(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    Ok(ResponseJson(json!({
        "data": state.proxy().status(),
    })))
}

async fn proxy_list(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let status = state.proxy().status();
    Ok(ResponseJson(json!({
        "data": {
            "proxies": state.proxy().list_proxies(),
            "enabled": status.enabled,
            "strategy": status.strategy,
            "total": status.total,
            "alive": status.alive,
        }
    })))
}

async fn add_proxy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<AddProxyRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    if !state.proxy().add_proxy(&payload.url, &payload.label) {
        return Err(ResponseError::bad_request("代理已存在或地址为空"));
    }

    Ok(ResponseJson(json!({
        "data": {
            "success": true,
            "message": "代理已添加",
        }
    })))
}

async fn batch_add_proxy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<BatchAddProxyRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let mut added = 0;
    for proxy in &payload.proxies {
        if state.proxy().add_proxy(&proxy.url, &proxy.label) {
            added += 1;
        }
    }

    Ok(ResponseJson(json!({
        "data": {
            "added": added,
            "total": payload.proxies.len(),
        }
    })))
}

async fn remove_proxy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<RemoveProxyRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    if !state.proxy().remove_proxy(&payload.url) {
        return Err(ResponseError::not_found("代理不存在"));
    }

    Ok(ResponseJson(json!({
        "data": {
            "success": true,
            "message": "代理已删除",
        }
    })))
}

async fn set_enabled(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<SetEnabledRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    state.proxy().set_enabled(payload.enabled);
    Ok(ResponseJson(json!({
        "data": {
            "enabled": payload.enabled,
        }
    })))
}

async fn set_strategy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<SetStrategyRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    if !matches!(
        payload.strategy.as_str(),
        "round-robin" | "random" | "latency"
    ) {
        return Err(ResponseError::bad_request(
            "不支持的策略，可选: round-robin / random / latency",
        ));
    }

    state.proxy().set_strategy(&payload.strategy);
    Ok(ResponseJson(json!({
        "data": {
            "strategy": payload.strategy,
        }
    })))
}

async fn reset_proxy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<ResetProxyRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    state.proxy().reset_proxy(&payload.url);
    Ok(ResponseJson(json!({
        "data": {
            "success": true,
        }
    })))
}

async fn check_proxy(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
    Json(payload): Json<CheckProxyRequest>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    Ok(ResponseJson(json!({
        "data": state.proxy().check_proxy(&payload.url),
    })))
}

async fn check_all_proxies(
    State(state): State<AppState>,
    Extension(identity): Extension<RequestIdentity>,
) -> ApiResult {
    ensure_admin_or_legacy(&state, &identity)?;
    let results = state
        .proxy()
        .check_all()
        .into_iter()
        .map(|(url, label, result)| {
            json!({
                "url": url,
                "label": label,
                "alive": result.alive,
                "latency_ms": result.latency_ms,
                "ip": result.ip,
                "error": result.error,
            })
        })
        .collect::<Vec<_>>();
    let status = state.proxy().status();

    Ok(ResponseJson(json!({
        "data": {
            "results": results,
            "enabled": status.enabled,
            "strategy": status.strategy,
            "total": status.total,
            "alive": status.alive,
        }
    })))
}
