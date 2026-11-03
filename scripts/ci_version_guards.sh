#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

python3 scripts/check_version_guards.py

cargo test --lib startup_preflight::tests::version_policy_blocks_lower_versions
cargo test --lib startup_preflight::tests::version_policy_honors_force_update_latest_version
cargo test --lib state::tests::runtime_health_reports_blocked_startup_dependencies

pnpm -C frontend-v2 exec vitest run src/features/license/LicenseGate.test.tsx
