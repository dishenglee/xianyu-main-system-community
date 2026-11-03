#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

run_bucket() {
  local label="$1"
  shift
  printf '\n[S06] %s\n' "$label"
  "$@"
}

run_bucket "inherited backend/runtime regression closure" \
  bash scripts/verify-s05-regression-closure.sh

run_bucket "notification page operator proof: runtime/listener/callback visibility + test-send wiring + callback/account context" \
  pnpm --dir frontend-v2 exec vitest run \
    src/features/notifications/runtimePresentation.test.ts \
    src/features/notifications/DesktopNotifications.test.tsx \
    src/features/notifications/MobileNotifications.test.tsx

printf '\n[S06] verification complete: inherited backend/runtime closure and notification-page operator proof both passed through the authoritative slice verifier.\n'
