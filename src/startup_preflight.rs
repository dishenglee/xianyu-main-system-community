use crate::{
    config::AppConfig,
    models::runtime::{StartupDependencyProbe, StartupDependencyStatus},
};

pub async fn run_startup_dependency_checks(_config: &AppConfig) -> StartupDependencyStatus {
    StartupDependencyStatus {
        startup_blocked: false,
        summary: "社区版本地功能不依赖产品授权或签名服务；需要签名的请求仅在具体操作时处理。"
            .to_string(),
        sign_service: community_disabled_probe("sign_service"),
        license_service: community_disabled_probe("license_service"),
        version_policy: community_disabled_probe("version_policy"),
    }
}

fn community_disabled_probe(name: &str) -> StartupDependencyProbe {
    StartupDependencyProbe {
        name: name.to_string(),
        ok: false,
        critical: false,
        detail: "community edition: not required at startup".to_string(),
        checked_at: format!("{:?}", std::time::SystemTime::now()),
        ..StartupDependencyProbe::default()
    }
}

#[cfg(test)]
mod tests {
    use super::run_startup_dependency_checks;
    use crate::config::AppConfig;

    #[tokio::test]
    async fn community_startup_does_not_require_control_plane_services() {
        let mut config = AppConfig::default();
        config.xianyu.sign_service_url = "https://sign.invalid.example".to_string();

        let status = run_startup_dependency_checks(&config).await;

        assert!(!status.startup_blocked);
        assert!(status.summary.contains("不依赖产品授权或签名服务"));
        for probe in [
            status.sign_service,
            status.license_service,
            status.version_policy,
        ] {
            assert!(!probe.ok);
            assert!(!probe.critical);
            assert!(probe.detail.contains("not required at startup"));
        }
    }
}
