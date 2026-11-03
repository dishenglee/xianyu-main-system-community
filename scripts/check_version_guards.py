#!/usr/bin/env python3
from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

RAW_REQUEST_FILES = (
    "src/routes/auth.rs",
    "src/routes/accounts/mod.rs",
    "src/routes/accounts/cloud_binding.rs",
    "src/startup_preflight.rs",
    "src/telemetry/incidents.rs",
    "src-tauri/src/license.rs",
)

APPROVED_REQUEST_TOKENS = (
    "apply_client_version_headers(",
    "apply_sign_service_access_headers(",
    "apply_control_plane_proof_headers(",
    "apply_runtime_proof_headers(",
    ".header(CLIENT_VERSION_HEADER",
    "http://127.0.0.1:",
    "http://localhost:",
)

REQUIRED_SNIPPETS = {
    "src/license_runtime.rs": (
        'pub const CLIENT_VERSION_HEADER: &str = "x-yuguanjia-client-version";',
        "pub fn current_client_version() -> &'static str {",
        "pub fn apply_client_version_headers(builder: reqwest::RequestBuilder)",
        "pub fn resolve_update_policy_url(configured: Option<String>) -> String {",
        'const LICENSE_SERVICE_HOST: &str = "license.gooofish.shop";',
    ),
    "src/models/runtime.rs": (
        "pub version_policy: StartupDependencyProbe,",
    ),
    "src/state.rs": (
        'name: "version_policy".to_string(),',
    ),
    "src/startup_preflight.rs": (
        "fn version_policy_url() -> String {",
        "resolve_update_policy_url(configured)",
        "let version_policy_url = version_policy_url();",
        "let startup_blocked = !(sign_service.ok && license_service.ok && version_policy.ok);",
        'failed.push(format!("版本检查异常: {}", version_policy.detail));',
    ),
    "src/routes/auth.rs": (
        'let response = apply_client_version_headers(client.get(format!("{base}/api/me")))',
    ),
    "src/routes/accounts/mod.rs": (
        "let response = apply_client_version_headers(client.post(&url))",
        "let response = apply_client_version_headers(client.get(&url))",
    ),
    "src/routes/accounts/cloud_binding.rs": (
        "let response = apply_client_version_headers(client.get(&url))",
        "let response = apply_client_version_headers(client.post(&url))",
    ),
    "src/routes/accounts/qr.rs": (
        "let response = apply_client_version_headers(response)",
    ),
    "src/telemetry/incidents.rs": (
        "let builder = crate::license_runtime::apply_client_version_headers(builder);",
    ),
    "src/xianyu_client/http.rs": (
        "let builder = apply_client_version_headers(builder);",
        "apply_client_version_headers(client.post(&url))",
        "apply_sign_service_access_headers(",
    ),
    "src-tauri/src/license.rs": (
        "let res = apply_client_version_headers(client.get(url)).send().await?;",
        "let mut req = apply_client_version_headers(client.post(url)).json(body);",
        "let mut req = apply_client_version_headers(client.patch(url)).json(body);",
        "let res = apply_client_version_headers(client.delete(url))",
        "client.post(format!(\"{}/api/heartbeats\", bundle.server_url))",
    ),
    "src-tauri/src/lib.rs": (
        "async fn load_update_policy(_state: &BackendState) -> Result<Option<UpdatePolicy>, String> {",
        "vec![resolve_update_policy_url(configured)]",
        "async fn fetch_update_policy(client: &reqwest::Client, url: &str) -> Result<UpdatePolicy, String> {",
        "let response = apply_client_version_headers(client.get(url))",
        "match apply_client_version_headers(client.get(&url)).send().await {",
    ),
    "frontend-v2/src/features/license/api.ts": (
        "version_policy?: StartupDependencyProbe",
    ),
    "frontend-v2/src/features/license/LicenseGate.tsx": (
        "const versionPolicy = backendHealth?.dependencies?.version_policy",
    ),
    "scripts/release_424_parallel.sh": (
        'download_force_update="${YUGUANJIA_DOWNLOAD_FORCE_UPDATE:-false}"',
    ),
    "scripts/install_424_release_monitor.sh": (
        "YUGUANJIA_DOWNLOAD_FORCE_UPDATE='false'",
        'DOWNLOAD_FORCE_UPDATE="${YUGUANJIA_DOWNLOAD_FORCE_UPDATE:-false}"',
    ),
}

REQUEST_RE = re.compile(
    r"(?P<stmt>[^;]*(?:client|response)\s*\.\s*(?:get|post|patch|delete)\s*\([^;]*?\.send\s*\([^;]*?\)[^;]*;)",
    re.S,
)


def read_text(rel_path: str) -> str:
    return (ROOT / rel_path).read_text(encoding="utf-8")


def line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def main() -> int:
    failures: list[str] = []

    for rel_path, snippets in REQUIRED_SNIPPETS.items():
        text = read_text(rel_path)
        for snippet in snippets:
            if snippet not in text:
                failures.append(f"{rel_path}: missing required snippet: {snippet}")

    for rel_path in RAW_REQUEST_FILES:
        text = read_text(rel_path)
        for match in REQUEST_RE.finditer(text):
            stmt = match.group("stmt")
            if any(token in stmt for token in APPROVED_REQUEST_TOKENS):
                continue
            lineno = line_number(text, match.start())
            compact = " ".join(stmt.split())
            failures.append(
                f"{rel_path}:{lineno}: raw control-plane request missing version guard: {compact[:220]}"
            )

    if failures:
        print("version guard check failed:", file=sys.stderr)
        for item in failures:
            print(f"  - {item}", file=sys.stderr)
        return 1

    print("version guard check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
