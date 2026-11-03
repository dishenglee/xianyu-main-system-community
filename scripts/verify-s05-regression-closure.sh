#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

run_bucket() {
  local label="$1"
  shift
  printf '\n[S05] %s\n' "$label"
  "$@"
}

run_bucket "frontend channel catalog closure contract" \
  pnpm --dir frontend-v2 exec vitest run src/features/notifications/channelCatalog.test.ts

run_bucket "dispatcher continuity: openclaw self-heal + unsupported-channel closure" \
  cargo test --manifest-path Cargo.toml --test notifications_dispatcher_integration -- --nocapture

run_bucket "route/manager continuity: route-triggered restart + QR auto-provision" \
  cargo test --manifest-path Cargo.toml --test notification_routes -- --nocapture

run_bucket "worker restart diagnostics continuity" \
  cargo test --manifest-path Cargo.toml --test notifications_worker_restart -- --nocapture

printf '\n[S05] optional live-env path: skipped by default (no external credentials required); authoritative proof remains local and deterministic.\n'
printf '[S05] verification complete: frontend contract, openclaw continuity, route/worker restart continuity, and unsupported-channel guard coverage all passed through one regression-closure command.\n'
