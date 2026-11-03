#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

printf '\n[S02] notifications_driver_runtime\n'
cargo test --manifest-path Cargo.toml --test notifications_driver_runtime -- --nocapture

printf '\n[S02] notifications_dispatcher_integration\n'
cargo test --manifest-path Cargo.toml --test notifications_dispatcher_integration -- --nocapture

printf '\n[S02] verification complete: retained driver/runtime semantics and dispatcher integration passed; unsupported outbound channel paths are covered by deletion guards.\n'
