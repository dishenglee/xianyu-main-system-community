use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode, Uri},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{
    decode, encode, errors::ErrorKind, DecodingKey, EncodingKey, Header, Validation,
};

use crate::{
    license_runtime::get_sign_license_context,
    models::auth::{AuthErrorResponse, AuthSettings, RequestIdentity, TokenClaims},
    observability::{EventStream, RequestContext, RequestCorrelation},
    state::AppState,
};

#[derive(Debug)]
pub enum AuthError {
    InvalidToken,
    ExpiredToken,
}

impl AuthError {
    pub fn reason(&self) -> &'static str {
        match self {
            Self::InvalidToken => "invalid-token",
            Self::ExpiredToken => "expired-token",
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for AuthError {}

#[derive(Debug, Clone)]
pub struct AuthRejection {
    status: StatusCode,
    body: AuthErrorResponse,
}

impl AuthRejection {
    pub fn missing_token() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthorized", "missing-token")
    }

    pub fn invalid_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "Invalid credentials",
            "invalid-credentials",
        )
    }

    pub fn invalid_license_token(detail: &str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, detail, "invalid-license-token")
    }

    pub fn license_auth_unavailable(detail: &str) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, detail, "license-auth-unavailable")
    }

    pub fn forbidden(detail: &str, reason: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, detail, reason)
    }

    pub fn local_admin_login_disabled() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "Local admin login is disabled; use the license account system",
            "local-admin-login-disabled",
        )
    }

    pub fn client_session_taken_over() -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "当前设备在线会话已被其他设备接管，请重新登录",
            "client-session-taken-over",
        )
    }

    pub fn from_auth_error(error: AuthError) -> Self {
        let detail = match error {
            AuthError::ExpiredToken => "Token expired",
            AuthError::InvalidToken => "Invalid token",
        };

        Self::new(StatusCode::UNAUTHORIZED, detail, error.reason())
    }

    pub fn auth_state_unavailable() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Auth state unavailable",
            "auth-state-unavailable",
        )
    }

    fn new(status: StatusCode, detail: &str, reason: &str) -> Self {
        Self {
            status,
            body: AuthErrorResponse {
                detail: detail.to_string(),
                reason: reason.to_string(),
            },
        }
    }
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

pub fn encode_access_token(
    settings: &AuthSettings,
    username: &str,
    role: &str,
    legacy: bool,
) -> Result<String, AuthError> {
    let issued_at = unix_timestamp_now();
    let claims = TokenClaims {
        username: normalized_username(username, &settings.username),
        role: normalized_role(role),
        legacy,
        iat: issued_at,
        exp: issued_at + settings.expire_seconds as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(settings.jwt_secret.as_bytes()),
    )
    .map_err(|_| AuthError::InvalidToken)
}

pub fn decode_access_token(settings: &AuthSettings, token: &str) -> Result<TokenClaims, AuthError> {
    decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(settings.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(map_jwt_error)
}

pub fn resolve_request_identity(
    settings: &AuthSettings,
    token: &str,
) -> Result<RequestIdentity, AuthError> {
    if settings
        .legacy_api_token
        .as_deref()
        .is_some_and(|legacy_token| legacy_token == token)
    {
        return Ok(RequestIdentity::new(
            settings.username.clone(),
            "admin".to_string(),
            true,
            false,
        ));
    }

    if settings
        .bootstrap_token
        .as_deref()
        .is_some_and(|bootstrap_token| bootstrap_token == token)
    {
        return Ok(RequestIdentity::new(
            "local-app".to_string(),
            "local".to_string(),
            false,
            true,
        ));
    }

    let claims = decode_access_token(settings, token)?;
    Ok(RequestIdentity::new(
        normalized_username(&claims.username, &settings.username),
        normalized_role(&claims.role),
        claims.legacy,
        false,
    ))
}

pub fn authenticate_credentials(settings: &AuthSettings, username: &str, password: &str) -> bool {
    username.trim() == settings.username && password == settings.password
}

pub fn resolve_bearer_identity(
    settings: &AuthSettings,
    headers: &HeaderMap,
) -> Result<RequestIdentity, AuthRejection> {
    let token = parse_bearer_token(headers)?;
    resolve_request_identity(settings, token).map_err(AuthRejection::from_auth_error)
}

pub fn resolve_query_identity(
    settings: &AuthSettings,
    token: Option<&str>,
) -> Result<RequestIdentity, AuthRejection> {
    let token = token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(AuthRejection::missing_token)?;

    resolve_request_identity(settings, token).map_err(AuthRejection::from_auth_error)
}

/// Best-effort token extraction that accepts either `Authorization: Bearer ...`
/// or `?token=...` query parameter. WebSocket upgrade requests can't set custom
/// headers in the browser, so they rely on the query form.
fn extract_any_token(headers: &HeaderMap, uri: &Uri) -> Result<String, AuthRejection> {
    if let Ok(token) = parse_bearer_token(headers) {
        return Ok(token.to_string());
    }

    if let Some(query) = uri.query() {
        for pair in query.split('&') {
            if let Some(rest) = pair.strip_prefix("token=") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    Err(AuthRejection::missing_token())
}

/// Axum middleware that enforces `Authorization: Bearer <token>` (or `?token=`
/// for WS upgrades) on every protected route. Valid identities include the
/// Tauri-issued bootstrap token, the optional legacy API token, and any JWT
/// issued via the explicit maintenance-only local login — all handled by
/// `resolve_request_identity`.
pub async fn require_bearer_middleware(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, AuthRejection> {
    let settings = &state.config().auth;
    let token = extract_any_token(req.headers(), req.uri())?;
    let correlation = RequestCorrelation::from_http_headers(req.headers());
    let identity = resolve_request_identity(settings, &token)
        .map(|identity| identity.with_correlation(correlation.clone()))
        .map_err(AuthRejection::from_auth_error)?;
    if identity.bootstrap && !bootstrap_route_allowed(req.method(), req.uri().path()) {
        return Err(AuthRejection::forbidden(
            "Bootstrap token is not allowed on this route",
            "bootstrap-token-forbidden",
        ));
    }
    if stale_license_context_route(req.uri().path())
        && !identity.bootstrap
        && get_sign_license_context().is_none()
    {
        return Err(AuthRejection::client_session_taken_over());
    }
    let request_context = RequestContext::new(EventStream::Runtime, correlation)
        .with_username(identity.username.clone());
    req.extensions_mut().insert(identity);
    req.extensions_mut().insert(request_context);
    Ok(next.run(req).await)
}

fn stale_license_context_route(path: &str) -> bool {
    path == crate::sealed_protocol::sealed_str!("/api/live/ws")
        || path.starts_with(crate::sealed_protocol::sealed_str!(
            "/api/messages/pc/sessions"
        ))
}

fn bootstrap_route_allowed(method: &axum::http::Method, path: &str) -> bool {
    (method == axum::http::Method::GET
        && path == crate::sealed_protocol::sealed_str!("/api/auth/verify"))
        || (method == axum::http::Method::GET
            && path == crate::sealed_protocol::sealed_str!("/api/live/ws"))
        || (method == axum::http::Method::POST
            && path == crate::sealed_protocol::sealed_str!("/api/logs/incidents"))
        || (method == axum::http::Method::POST
            && path == crate::sealed_protocol::sealed_str!("/api/logs/client-events"))
}

fn parse_bearer_token(headers: &HeaderMap) -> Result<&str, AuthRejection> {
    let raw = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(AuthRejection::missing_token)?;

    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(AuthRejection::missing_token)?;

    Ok(token)
}

fn normalized_username(username: &str, fallback: &str) -> String {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalized_role(role: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        "admin" => "admin".to_string(),
        _ => "user".to_string(),
    }
}

fn unix_timestamp_now() -> usize {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs() as usize
}

fn map_jwt_error(error: jsonwebtoken::errors::Error) -> AuthError {
    match error.kind() {
        ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
        _ => AuthError::InvalidToken,
    }
}
