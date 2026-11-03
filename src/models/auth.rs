use serde::{Deserialize, Serialize};

use crate::observability::RequestCorrelation;

#[derive(Debug, Clone)]
pub struct AuthSettings {
    pub username: String,
    pub password: String,
    pub jwt_secret: String,
    pub expire_seconds: u64,
    pub legacy_api_token: Option<String>,
    /// Legacy local username/password login. Disabled by default so product
    /// users only authenticate through the remote license account system.
    pub local_admin_login_enabled: bool,
    /// Tauri-issued per-launch bootstrap token. When set, any request whose
    /// bearer/query token equals this value is treated as the local-app identity.
    /// Populated from the `APP_BEARER_TOKEN` env var.
    pub bootstrap_token: Option<String>,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            username: "admin".to_string(),
            password: "admin123".to_string(),
            jwt_secret: String::new(),
            expire_seconds: 24 * 3600,
            legacy_api_token: None,
            local_admin_login_enabled: false,
            bootstrap_token: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClaims {
    pub username: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub legacy: bool,
    pub iat: usize,
    pub exp: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdentity {
    pub username: String,
    pub role: String,
    pub legacy: bool,
    pub bootstrap: bool,
    pub correlation: RequestCorrelation,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseTokenExchangeRequest {
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseUserPublic {
    pub username: String,
    #[serde(default = "default_role")]
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthUser {
    pub username: String,
    pub role: String,
    pub legacy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub expires_in: u64,
    pub user: AuthUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub valid: bool,
    pub user: AuthUser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthErrorResponse {
    pub detail: String,
    pub reason: String,
}

impl From<RequestIdentity> for AuthUser {
    fn from(value: RequestIdentity) -> Self {
        Self {
            username: value.username,
            role: value.role,
            legacy: value.legacy,
        }
    }
}

impl RequestIdentity {
    pub fn new(username: String, role: String, legacy: bool, bootstrap: bool) -> Self {
        Self {
            username,
            role,
            legacy,
            bootstrap,
            correlation: RequestCorrelation::detached(),
        }
    }

    pub fn with_correlation(mut self, correlation: RequestCorrelation) -> Self {
        self.correlation = correlation;
        self
    }
}

fn default_role() -> String {
    "user".to_string()
}
