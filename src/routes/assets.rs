use std::{net::IpAddr, time::Duration};

use axum::{
    body::Body,
    extract::Query,
    http::{
        header::{CACHE_CONTROL, CONTENT_TYPE, REFERER, USER_AGENT},
        Response, StatusCode,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use serde::Deserialize;
use url::Url;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
struct ImageProxyQuery {
    url: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route(
        crate::sealed_protocol::sealed_str!("/assets/image-proxy"),
        get(image_proxy),
    )
}

async fn image_proxy(
    Query(query): Query<ImageProxyQuery>,
) -> Result<Response<Body>, ImageProxyError> {
    let url = validate_image_url(&query.url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|_| ImageProxyError::bad_gateway())?;

    let response = client
        .get(url)
        .header(
            USER_AGENT,
            "Mozilla/5.0 (Linux; Android 13) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120 Mobile Safari/537.36",
        )
        .header(REFERER, crate::sealed_protocol::sealed_str!("https://www.goofish.com/"))
        .header(
            reqwest::header::ACCEPT,
            "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8",
        )
        .send()
        .await
        .map_err(|_| ImageProxyError::bad_gateway())?;

    if !response.status().is_success() {
        return Err(ImageProxyError::bad_gateway());
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(image_content_type);

    let bytes = response
        .bytes()
        .await
        .map_err(|_| ImageProxyError::bad_gateway())?;
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err(ImageProxyError::bad_gateway());
    }

    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CACHE_CONTROL, "public, max-age=86400");
    if let Some(content_type) = content_type {
        builder = builder.header(CONTENT_TYPE, content_type);
    }
    builder
        .body(Body::from(bytes.to_vec()))
        .map_err(|_| ImageProxyError::bad_gateway())
}

fn validate_image_url(raw: &str) -> Result<Url, ImageProxyError> {
    let url = Url::parse(raw.trim()).map_err(|_| ImageProxyError::bad_request())?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err(ImageProxyError::bad_request()),
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") || host == "0.0.0.0" {
        return Err(ImageProxyError::bad_request());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err(ImageProxyError::bad_request());
        }
    }
    if !is_allowed_image_host(&host) {
        return Err(ImageProxyError::bad_request());
    }

    Ok(url)
}

fn is_allowed_image_host(host: &str) -> bool {
    let allowed_suffixes = [
        "alicdn.com",
        "taobaocdn.com",
        "tbcdn.cn",
        crate::sealed_protocol::sealed_str!("goofish.com"),
        "goofishcdn.com",
        "alibabausercontent.com",
    ];
    allowed_suffixes
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified())
        }
        IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

fn image_content_type(value: &str) -> Option<&'static str> {
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("image/jpeg") || lower.starts_with("image/jpg") {
        Some("image/jpeg")
    } else if lower.starts_with("image/png") {
        Some("image/png")
    } else if lower.starts_with("image/webp") {
        Some("image/webp")
    } else if lower.starts_with("image/gif") {
        Some("image/gif")
    } else if lower.starts_with("image/avif") {
        Some("image/avif")
    } else if lower.starts_with("image/svg+xml") {
        Some("image/svg+xml")
    } else {
        None
    }
}

#[derive(Debug)]
struct ImageProxyError {
    status: StatusCode,
}

impl ImageProxyError {
    fn bad_request() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
        }
    }

    fn bad_gateway() -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
        }
    }
}

impl IntoResponse for ImageProxyError {
    fn into_response(self) -> axum::response::Response {
        self.status.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::{image_content_type, validate_image_url};

    #[test]
    fn allows_known_image_cdn_hosts() {
        assert!(validate_image_url("https://gw.alicdn.com/imgextra/example.jpg").is_ok());
        assert!(validate_image_url("https://img.alicdn.com/bao/uploaded/example.webp").is_ok());
        assert!(validate_image_url(crate::sealed_protocol::sealed_str!(
            "https://assets.goofish.com/item.png"
        ))
        .is_ok());
    }

    #[test]
    fn rejects_local_or_unknown_hosts() {
        assert!(validate_image_url("http://127.0.0.1:9000/private.png").is_err());
        assert!(validate_image_url("http://localhost/private.png").is_err());
        assert!(validate_image_url("https://example.com/image.png").is_err());
        assert!(validate_image_url("file:///tmp/image.png").is_err());
    }

    #[test]
    fn only_passes_image_content_types() {
        assert_eq!(
            image_content_type("image/webp; charset=binary"),
            Some("image/webp")
        );
        assert_eq!(image_content_type("text/html"), None);
    }
}
