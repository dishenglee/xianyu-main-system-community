#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

printf '\n[S01] notifications_driver_runtime\n'
cargo test --manifest-path Cargo.toml --test notifications_driver_runtime -- --nocapture

printf '\n[S01] notifications_dispatcher_integration\n'
cargo test --manifest-path Cargo.toml --test notifications_dispatcher_integration -- --nocapture

printf '\n[S01] notifications_worker_restart\n'
cargo test --manifest-path Cargo.toml --test notifications_worker_restart -- --nocapture

printf '\n[S01] verification complete: T01 + T02 + T03 covered by driver/runtime, dispatcher integration, and worker restart tests.\n'
