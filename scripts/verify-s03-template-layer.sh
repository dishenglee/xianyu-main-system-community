#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

printf '\n[S03] frontend channelCatalog contract\n'
pnpm --dir frontend-v2 exec vitest run src/features/notifications/channelCatalog.test.ts

printf '\n[S03] notifications_driver_runtime\n'
cargo test --manifest-path Cargo.toml --test notifications_driver_runtime -- --nocapture

printf '\n[S03] notifications_dispatcher_integration\n'
cargo test --manifest-path Cargo.toml --test notifications_dispatcher_integration -- --nocapture

printf '\n[S03] verification complete: frontend config shape, retained-channel template negotiation, dispatcher wiring, and runtime regressions passed.\n'
