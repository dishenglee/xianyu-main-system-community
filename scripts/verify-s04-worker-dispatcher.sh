#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

printf '\n[S04] worker restart runtime surface\n'
cargo test --manifest-path Cargo.toml --test notifications_worker_restart -- --nocapture

printf '\n[S04] route restart + QR auto-provision manager surface\n'
cargo test --manifest-path Cargo.toml --test notification_routes -- --nocapture

printf '\n[S04] dispatcher openclaw self-heal runtime surface\n'
cargo test --manifest-path Cargo.toml --test notifications_dispatcher_integration -- --nocapture

printf '\n[S04] verification complete: startup scan, route restart, QR auto-provision, and openclaw stale-context self-heal all converge on the unified manager/runtime surface.\n'
