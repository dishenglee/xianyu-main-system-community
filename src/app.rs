use std::{env, path::PathBuf};

use axum::{
    extract::Path,
    http::{header, Request, StatusCode, Uri},
    middleware,
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeFile;
use tower_http::trace::TraceLayer;
use tracing::Level;

use crate::{
    models::auth::RequestIdentity,
    observability::{EventStream, RequestContext, RequestCorrelation},
    routes, state::AppState,
};

pub fn build_app(state: AppState) -> Router {
    let api = routes::public_router()
        .merge(routes::protected_router())
        .route(
            crate::sealed_protocol::sealed_str!("/api/*rest"),
            any(|| async { StatusCode::NOT_FOUND }),
        )
        .with_state(state)
        .layer(middleware::from_fn(local_identity_middleware))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                tracing::span!(
                    Level::INFO,
                    "request",
                    method = %request.method(),
                    uri = %redacted_uri_for_log(request.uri()),
                    version = ?request.version(),
                )
            }),
        )
        .layer(build_cors_layer());

    let static_dir = resolve_static_dir();
    if let Some(dir) = static_dir {
        let index = dir.join("index.html");
        let assets_dir = dir.join("assets");
        let favicon = dir.join("favicon.svg");
        let spa = Router::new()
            .route(
                crate::sealed_protocol::sealed_str!("/assets/*path"),
                get({
                    let assets_dir = assets_dir.clone();
                    move |path| {
                        let assets_dir = assets_dir.clone();
                        async move { serve_static_file(assets_dir, path).await }
                    }
                }),
            )
            .route(
                crate::sealed_protocol::sealed_str!("/favicon.svg"),
                get(move || {
                    let favicon = favicon.clone();
                    async move { serve_known_file(favicon).await }
                }),
            )
            .fallback_service(ServeFile::new(&index));
        api.merge(spa)
    } else {
        api
    }
}

async fn local_identity_middleware(
    mut request: Request<axum::body::Body>,
    next: middleware::Next,
) -> Response {
    let correlation = RequestCorrelation::from_http_headers(request.headers());
    let identity = RequestIdentity::new("local-app".to_string(), "admin".to_string(), true, true)
        .with_correlation(correlation.clone());
    request.extensions_mut().insert(identity);
    request.extensions_mut().insert(
        RequestContext::new(EventStream::Runtime, correlation).with_username("local-app"),
    );
    next.run(request).await
}

fn redacted_uri_for_log(uri: &Uri) -> String {
    if uri.query().is_some() {
        format!("{}?<redacted>", uri.path())
    } else {
        uri.path().to_string()
    }
}

/// The no-auth local API is available only to the packaged Tauri WebView.
fn build_cors_layer() -> CorsLayer {
    use axum::http::{HeaderName, Method};

    let layer = CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            HeaderName::from_static("content-type"),
            HeaderName::from_static("accept"),
            HeaderName::from_static("x-requested-with"),
        ])
        .max_age(std::time::Duration::from_secs(3600));

    layer.allow_origin(AllowOrigin::list([
        header::HeaderValue::from_static("tauri://localhost"),
        header::HeaderValue::from_static("http://tauri.localhost"),
        header::HeaderValue::from_static("https://tauri.localhost"),
    ]))
}

fn resolve_static_dir() -> Option<PathBuf> {
    if let Ok(p) = env::var("FRONTEND_STATIC_DIR") {
        let path = PathBuf::from(p);
        return path.join("index.html").exists().then_some(path);
    }
    for candidate in ["static", "../static", "frontend-v2/dist"] {
        let path = PathBuf::from(candidate);
        if path.join("index.html").exists() {
            return Some(path);
        }
    }
    None
}

async fn serve_static_file(root: PathBuf, Path(path): Path<String>) -> Response {
    if path.is_empty() || path.split('/').any(|segment| segment == "..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    serve_known_file(root.join(path)).await
}

async fn serve_known_file(path: PathBuf) -> Response {
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, content_type_for_path(&path))],
            bytes,
        )
            .into_response(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn content_type_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
    {
        "js" | "mjs" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "json" | "map" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::redacted_uri_for_log;
    use axum::http::Uri;

    #[test]
    fn redacts_query_tokens_from_trace_uri() {
        let uri: Uri = "/api/live/ws?token=secret-token&x=1".parse().unwrap();

        assert_eq!(redacted_uri_for_log(&uri), "/api/live/ws?<redacted>");
    }

    #[test]
    fn preserves_path_without_query() {
        let uri: Uri = crate::sealed_protocol::sealed_str!("/api/accounts")
            .parse()
            .unwrap();

        assert_eq!(
            redacted_uri_for_log(&uri),
            crate::sealed_protocol::sealed_str!("/api/accounts")
        );
    }
}
