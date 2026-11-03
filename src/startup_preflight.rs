use std::time::Duration;

use reqwest::StatusCode;

use crate::{
    config::AppConfig,
    control_plane_tls::verify_response_pin,
    license_runtime::apply_client_version_headers,
    models::runtime::{StartupDependencyProbe, StartupDependencyStatus},
    state::AppState,
};

const HEALTH_TIMEOUT: Duration = Duration::from_secs(4);
const HEALTH_RETRY_ATTEMPTS: usize = 3;
const HEALTH_RETRY_DELAY: Duration = Duration::from_millis(300);

fn control_plane_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .tls_info(true)
        .timeout(HEALTH_TIMEOUT)
        .connect_timeout(Duration::from_secs(4))
        .build()
}

fn apply_sign_service_access_headers(
    builder: reqwest::RequestBuilder,
    config: &AppConfig,
) -> reqwest::RequestBuilder {
    let builder = apply_client_version_headers(builder);
    let client_id = config.xianyu.sign_cf_access_client_id.trim();
    let client_secret = config.xianyu.sign_cf_access_client_secret.trim();
    if client_id.is_empty() || client_secret.is_empty() {
        return builder;
    }
    builder
        .header("CF-Access-Client-Id", client_id)
        .header("CF-Access-Client-Secret", client_secret)
}

pub async fn run_startup_dependency_checks(config: &AppConfig) -> StartupDependencyStatus {
    let client = match control_plane_client() {
        Ok(client) => client,
        Err(error) => {
            return StartupDependencyStatus {
                startup_blocked: false,
                summary: format!("本地程序可继续运行；App 签名通道探测客户端初始化失败: {error}"),
                sign_service: StartupDependencyProbe {
                    name: "sign_service".to_string(),
                    url: format!("{}/health", trim_slash(&config.xianyu.sign_service_url)),
                    critical: false,
                    detail: "probe client unavailable".to_string(),
                    checked_at: checked_at_now(),
                    ..StartupDependencyProbe::default()
                },
                license_service: community_disabled_probe("license_service"),
                version_policy: community_disabled_probe("version_policy"),
            };
        }
    };

    let sign_url = format!("{}/health", trim_slash(&config.xianyu.sign_service_url));
    let pins = &config.xianyu.control_plane_cert_pins;
    let mut sign_service =
        probe_health_with_retries(&client, config, "sign_service", &sign_url, pins, true).await;
    sign_service.critical = false;
    let summary = if sign_service.ok {
        "本地功能可用；签名服务探测成功（App 通道仍需外部授权上下文）".to_string()
    } else {
        format!(
            "本地功能可继续使用；App 签名通道暂不可用: {}",
            sign_service.detail
        )
    };

    StartupDependencyStatus {
        startup_blocked: false,
        summary,
        sign_service,
        license_service: community_disabled_probe("license_service"),
        version_policy: community_disabled_probe("version_policy"),
    }
}

fn community_disabled_probe(name: &str) -> StartupDependencyProbe {
    StartupDependencyProbe {
        name: name.to_string(),
        ok: false,
        critical: false,
        detail: "community edition: not required".to_string(),
        checked_at: checked_at_now(),
        ..StartupDependencyProbe::default()
    }
}

pub fn spawn_startup_dependency_watchdog(state: AppState) {
    tokio::spawn(async move {
        loop {
            let status = run_startup_dependency_checks(state.config()).await;
            state.runtime().update_startup_dependencies(status);
            tokio::time::sleep(Duration::from_secs(20)).await;
        }
    });
}

async fn probe_health_with_retries(
    client: &reqwest::Client,
    config: &AppConfig,
    name: &str,
    url: &str,
    pins: &[String],
    apply_sign_access_headers: bool,
) -> StartupDependencyProbe {
    let mut last_probe = None;
    for attempt in 0..HEALTH_RETRY_ATTEMPTS {
        let probe = probe_health(client, config, name, url, pins, apply_sign_access_headers).await;
        if probe.ok {
            return probe;
        }
        last_probe = Some(probe);
        if attempt + 1 < HEALTH_RETRY_ATTEMPTS {
            tokio::time::sleep(HEALTH_RETRY_DELAY).await;
        }
    }
    last_probe.unwrap_or_else(|| StartupDependencyProbe {
        name: name.to_string(),
        url: url.to_string(),
        ok: false,
        critical: true,
        status_code: None,
        detail: "health probe did not run".to_string(),
        checked_at: checked_at_now(),
    })
}

async fn probe_health(
    client: &reqwest::Client,
    config: &AppConfig,
    name: &str,
    url: &str,
    pins: &[String],
    apply_sign_access_headers: bool,
) -> StartupDependencyProbe {
    let checked_at = checked_at_now();
    let request = apply_client_version_headers(client.get(url));
    let request = if apply_sign_access_headers {
        apply_sign_service_access_headers(request, config)
    } else {
        request
    };
    match request.send().await {
        Ok(response) => {
            if let Err(error) = verify_response_pin(&response, pins, name, url) {
                return StartupDependencyProbe {
                    name: name.to_string(),
                    url: url.to_string(),
                    ok: false,
                    critical: true,
                    status_code: None,
                    detail: error,
                    checked_at,
                };
            }
            let status = response.status();
            if status == StatusCode::OK {
                StartupDependencyProbe {
                    name: name.to_string(),
                    url: url.to_string(),
                    ok: true,
                    critical: true,
                    status_code: Some(status.as_u16()),
                    detail: "ok".to_string(),
                    checked_at,
                }
            } else {
                let body = response
                    .text()
                    .await
                    .unwrap_or_default()
                    .chars()
                    .take(160)
                    .collect::<String>();
                StartupDependencyProbe {
                    name: name.to_string(),
                    url: url.to_string(),
                    ok: false,
                    critical: true,
                    status_code: Some(status.as_u16()),
                    detail: if body.is_empty() {
                        format!("HTTP {}", status.as_u16())
                    } else {
                        format!("HTTP {}: {}", status.as_u16(), body)
                    },
                    checked_at,
                }
            }
        }
        Err(error) => StartupDependencyProbe {
            name: name.to_string(),
            url: url.to_string(),
            ok: false,
            critical: true,
            status_code: None,
            detail: error.to_string(),
            checked_at,
        },
    }
}

fn trim_slash(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn checked_at_now() -> String {
    format!("{:?}", std::time::SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use axum::{http::StatusCode as HttpStatusCode, response::IntoResponse, routing::get, Router};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::net::TcpListener;

    #[test]
    fn optional_dependencies_do_not_block_community_app() {
        let status = StartupDependencyStatus {
            startup_blocked: false,
            summary:
                "本地功能可继续使用；App 签名通道暂不可用: timeout；App 通道仍需外部授权上下文"
                    .to_string(),
            sign_service: StartupDependencyProbe {
                name: "sign_service".to_string(),
                url: "https://sign.example.com/health".to_string(),
                ok: false,
                critical: false,
                status_code: None,
                detail: "timeout".to_string(),
                checked_at: "now".to_string(),
            },
            license_service: community_disabled_probe("license_service"),
            version_policy: community_disabled_probe("version_policy"),
        };

        assert!(!status.startup_blocked);
        assert!(status.summary.contains("App 通道仍需外部授权上下文"));
        assert!(!status.sign_service.critical);
        assert!(!status.license_service.critical);
    }

    #[tokio::test]
    async fn health_probe_retries_transient_failures() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let app = Router::new().route(
            crate::sealed_protocol::sealed_str!("/health"),
            get({
                let attempts = attempts.clone();
                move || {
                    let attempts = attempts.clone();
                    async move {
                        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                            (HttpStatusCode::SERVICE_UNAVAILABLE, "warming up").into_response()
                        } else {
                            (HttpStatusCode::OK, "ok").into_response()
                        }
                    }
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/health", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::builder().build().unwrap();

        let probe = probe_health_with_retries(
            &client,
            &AppConfig::default(),
            "sign_service",
            &url,
            &[],
            false,
        )
        .await;

        server.abort();
        assert!(probe.ok);
        assert_eq!(probe.status_code, Some(200));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
}
