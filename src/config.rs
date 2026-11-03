use std::{
    env,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::models::auth::AuthSettings;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub service_name: String,
    pub bind_addr: SocketAddr,
    pub auth: AuthSettings,
    pub sqlite: SqliteConfig,
    pub xianyu: XianyuConfig,
}

#[derive(Debug, Clone)]
pub struct SqliteConfig {
    pub data_dir: PathBuf,
    pub accounts_db_path: PathBuf,
    pub live_state_db_path: PathBuf,
    pub notifications_db_path: PathBuf,
    pub xianyu_db_path: PathBuf,
    pub max_connections: u32,
}

#[derive(Debug, Clone)]
pub struct XianyuConfig {
    pub h5_gateway: String,
    pub h5_app_key: String,
    pub wx_gateway: String,
    pub wx_app_key: String,
    pub wx_jsv: String,
    pub stream_upload_url: String,
    pub upload_taobao_url: String,
    pub sign_service_url: String,
    pub license_server_url: String,
    pub sign_cf_access_client_id: String,
    pub sign_cf_access_client_secret: String,
    pub control_plane_client_id: String,
    pub control_plane_key_id: String,
    pub control_plane_shared_secret_hex: String,
    pub control_plane_max_skew_seconds: u64,
    pub app_mtop_gateway: String,
    pub control_plane_cert_pins: Vec<String>,
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidBindAddr {
        key: &'static str,
        value: String,
        message: String,
    },
    MissingJwtSecret {
        key: &'static str,
    },
    InvalidSqliteConnections {
        key: &'static str,
        value: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBindAddr {
                key,
                value,
                message,
            } => write!(f, "invalid {} value {:?}: {}", key, value, message),
            Self::MissingJwtSecret { key } => {
                write!(f, "missing JWT secret in environment variable {}", key)
            }
            Self::InvalidSqliteConnections { key, value } => {
                write!(
                    f,
                    "invalid {} value {:?}: expected positive integer",
                    key, value
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            service_name: "xianyu-rust-backend-full".to_string(),
            bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 3000),
            auth: AuthSettings::default(),
            sqlite: SqliteConfig::default(),
            xianyu: XianyuConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let defaults = Self::default();
        let service_name = env::var("APP_SERVICE_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(defaults.service_name);

        let bind_addr = match env::var("APP_BIND_ADDR") {
            Ok(value) => parse_socket_addr("APP_BIND_ADDR", &value)?,
            Err(_) => defaults.bind_addr,
        };
        if !bind_addr.ip().is_loopback() {
            return Err(ConfigError::InvalidBindAddr {
                key: "APP_BIND_ADDR",
                value: bind_addr.to_string(),
                message: "the no-auth app may only bind to a loopback address".to_string(),
            });
        }

        let sqlite = SqliteConfig::from_env(&defaults.sqlite)?;
        let xianyu = XianyuConfig::from_env(&defaults.xianyu);

        Ok(Self {
            service_name,
            bind_addr,
            auth: AuthSettings::default(),
            sqlite,
            xianyu,
        })
    }
}

impl Default for SqliteConfig {
    fn default() -> Self {
        let data_dir = PathBuf::from("data");

        Self {
            accounts_db_path: data_dir.join("accounts.db"),
            live_state_db_path: data_dir.join("live_state.db"),
            notifications_db_path: data_dir.join("notifications.db"),
            xianyu_db_path: data_dir.join("xianyu.db"),
            data_dir,
            max_connections: 5,
        }
    }
}

impl SqliteConfig {
    fn from_env(defaults: &SqliteConfig) -> Result<Self, ConfigError> {
        let data_dir = env::var("APP_DATA_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| defaults.data_dir.clone());
        let max_connections =
            sqlite_max_connections("SQLITE_MAX_CONNECTIONS")?.unwrap_or(defaults.max_connections);

        Ok(Self {
            accounts_db_path: db_path_from_env("ACCOUNTS_DB_PATH", &data_dir, "accounts.db")
                .unwrap_or_else(|| defaults.accounts_db_path.clone()),
            live_state_db_path: db_path_from_env("LIVE_STATE_DB_PATH", &data_dir, "live_state.db")
                .unwrap_or_else(|| defaults.live_state_db_path.clone()),
            notifications_db_path: db_path_from_env(
                "NOTIFICATIONS_DB_PATH",
                &data_dir,
                "notifications.db",
            )
            .unwrap_or_else(|| defaults.notifications_db_path.clone()),
            xianyu_db_path: db_path_from_env("XIANYU_DB_PATH", &data_dir, "xianyu.db")
                .unwrap_or_else(|| defaults.xianyu_db_path.clone()),
            data_dir,
            max_connections,
        })
    }
}

impl Default for XianyuConfig {
    fn default() -> Self {
        Self {
            h5_gateway: crate::sealed_protocol::sealed_str!("https://h5api.m.goofish.com/h5")
                .to_string(),
            h5_app_key: "34839810".to_string(),
            wx_gateway: crate::sealed_protocol::sealed_str!("https://acs.m.goofish.com")
                .to_string(),
            wx_app_key: "12574478".to_string(),
            wx_jsv: "2.4.12".to_string(),
            stream_upload_url: crate::sealed_protocol::sealed_str!(
                "https://stream-upload.goofish.com/api/upload.api"
            )
            .to_string(),
            upload_taobao_url: "https://upload.taobao.com/fileUpload".to_string(),
            sign_service_url: crate::sealed_protocol::sealed_str!("https://sign.gooofish.shop")
                .to_string(),
            license_server_url: crate::sealed_protocol::sealed_str!(
                "https://license.gooofish.shop"
            )
            .to_string(),
            sign_cf_access_client_id: String::new(),
            sign_cf_access_client_secret: String::new(),
            control_plane_client_id: "xianyu-control-plane".to_string(),
            control_plane_key_id: "default".to_string(),
            control_plane_shared_secret_hex: String::new(),
            control_plane_max_skew_seconds: 30,
            app_mtop_gateway: crate::sealed_protocol::sealed_str!("https://acs.m.goofish.com/gw")
                .to_string(),
            control_plane_cert_pins: Vec::new(),
        }
    }
}

impl XianyuConfig {
    fn from_env(defaults: &XianyuConfig) -> Self {
        Self {
            h5_gateway: env_or_default("XIANYU_H5_GATEWAY", &defaults.h5_gateway),
            h5_app_key: env_or_default("XIANYU_H5_APP_KEY", &defaults.h5_app_key),
            wx_gateway: env_or_default("XIANYU_WX_GATEWAY", &defaults.wx_gateway),
            wx_app_key: env_or_default("XIANYU_WX_APP_KEY", &defaults.wx_app_key),
            wx_jsv: env_or_default("XIANYU_WX_JSV", &defaults.wx_jsv),
            stream_upload_url: env_or_default(
                "XIANYU_STREAM_UPLOAD_URL",
                &defaults.stream_upload_url,
            ),
            upload_taobao_url: env_or_default(
                "XIANYU_UPLOAD_TAOBAO_URL",
                &defaults.upload_taobao_url,
            ),
            sign_service_url: env_or_default("XIANYU_SIGN_SERVICE_URL", &defaults.sign_service_url),
            license_server_url: env_or_default(
                "XIANYU_LICENSE_SERVER_URL",
                &defaults.license_server_url,
            ),
            sign_cf_access_client_id: env_or_default(
                "XIANYU_SIGN_CF_ACCESS_CLIENT_ID",
                &defaults.sign_cf_access_client_id,
            ),
            sign_cf_access_client_secret: env_or_default(
                "XIANYU_SIGN_CF_ACCESS_CLIENT_SECRET",
                &defaults.sign_cf_access_client_secret,
            ),
            control_plane_client_id: env_or_default(
                "XIANYU_CONTROL_PLANE_CLIENT_ID",
                &defaults.control_plane_client_id,
            ),
            control_plane_key_id: env_or_default(
                "XIANYU_CONTROL_PLANE_KEY_ID",
                &defaults.control_plane_key_id,
            ),
            control_plane_shared_secret_hex: env_or_default(
                "XIANYU_CONTROL_PLANE_SHARED_SECRET_HEX",
                &defaults.control_plane_shared_secret_hex,
            ),
            control_plane_max_skew_seconds: positive_u64_from_env(
                "XIANYU_CONTROL_PLANE_MAX_SKEW_SECONDS",
            )
            .unwrap_or(defaults.control_plane_max_skew_seconds),
            app_mtop_gateway: env_or_default("XIANYU_APP_MTOP_GATEWAY", &defaults.app_mtop_gateway),
            control_plane_cert_pins: crate::control_plane_tls::parse_pin_set(&env_or_default(
                "XIANYU_CONTROL_PLANE_CERT_PINS",
                "",
            )),
        }
    }
}

fn env_or_default(key: &'static str, default: &str) -> String {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn positive_u64_from_env(key: &'static str) -> Option<u64> {
    env::var(key)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
}

fn parse_socket_addr(key: &'static str, value: &str) -> Result<SocketAddr, ConfigError> {
    SocketAddr::from_str(value).map_err(|err| ConfigError::InvalidBindAddr {
        key,
        value: value.to_string(),
        message: err.to_string(),
    })
}

fn sqlite_max_connections(key: &'static str) -> Result<Option<u32>, ConfigError> {
    let Some(raw) = env::var(key).ok() else {
        return Ok(None);
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = trimmed
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ConfigError::InvalidSqliteConnections {
            key,
            value: raw.clone(),
        })?;

    Ok(Some(parsed))
}

fn db_path_from_env(key: &'static str, data_dir: &Path, filename: &str) -> Option<PathBuf> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(data_dir.join(filename)))
}
