#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

run_bucket() {
  local label="$1"
  shift
  printf '\n[S07] %s\n' "$label"
  "$@"
}

fail_if_matches() {
  local label="$1"
  local pattern="$2"
  shift 2
  printf '\n[S07] %s\n' "$label"
  if rg -n "$pattern" "$@"; then
    printf '[S07] failed: obsolete notification implementation names were found.\n' >&2
    exit 1
  fi
}

run_bucket "Rust formatting" \
  cargo fmt --manifest-path Cargo.toml --check

run_bucket "diff whitespace check" \
  git diff --check

run_bucket "release script syntax without Feishu Node resources" \
  bash -c "bash -n scripts/release_desktop.sh scripts/package_macos_dmg_linux.sh && ! rg -n 'sidecars/feishu-bridge|FEISHU_SIDECAR|@larksuiteoapi/node-sdk' src-tauri/tauri.conf.json scripts/release_desktop.sh scripts/package_macos_dmg_linux.sh"

run_bucket "frontend notification surfaces" \
  pnpm --dir frontend-v2 exec vitest run \
    src/features/notifications/channelCatalog.test.ts \
    src/features/notifications/callbackUrls.test.ts \
    src/features/notifications/runtimePresentation.test.ts \
    src/features/notifications/FeishuQrRegistrationModal.test.tsx \
    src/features/notifications/DesktopNotifications.test.tsx \
    src/features/notifications/MobileNotifications.test.tsx

run_bucket "frontend production build" \
  pnpm --dir frontend-v2 run build

run_bucket "Rust Feishu native module unit tests" \
  cargo test --manifest-path Cargo.toml --lib notifications::feishu_native -- --nocapture

run_bucket "Rust Feishu and notification integration matrix" \
  cargo test --manifest-path Cargo.toml \
    --test feishu_native_transport \
    --test feishu_im_reply \
    --test notification_routes \
    --test notifications_dispatcher_integration \
    --test notifications_driver_runtime \
    --test notifications_worker_restart \
    --test bootstrap_smoke \
    --test multi_tenant_isolation \
    -- --nocapture

run_bucket "Feishu business action nonce test inventory" \
  rg -n \
    'bound_sender_item_downshelf_requires_nonce_confirmation_before_action|bound_sender_order_dummy_consign_requires_nonce_confirmation_before_action|bound_sender_remaining_business_mutations_require_nonce_confirmation|business_action_nonce_is_sender_and_owner_scoped|business_action_upstream_failure_without_local_fallback_does_not_mutate_cache|duplicate_feishu_confirm_event_does_not_replay_business_action_or_reply' \
    tests/feishu_im_reply.rs

for required_test in \
  bound_sender_item_downshelf_requires_nonce_confirmation_before_action \
  bound_sender_order_dummy_consign_requires_nonce_confirmation_before_action \
  bound_sender_remaining_business_mutations_require_nonce_confirmation \
  business_action_nonce_is_sender_and_owner_scoped \
  business_action_upstream_failure_without_local_fallback_does_not_mutate_cache \
  duplicate_feishu_confirm_event_does_not_replay_business_action_or_reply
do
  if ! rg -q "async fn ${required_test}\\b|fn ${required_test}\\b" tests/feishu_im_reply.rs; then
    printf '[S07] failed: required Feishu nonce/replay test missing: %s\n' "$required_test" >&2
    exit 1
  fi
done

printf '\n[S07] Feishu business action nonce/replay test inventory is complete and covered by the Rust matrix above.\n'

fail_if_matches "old notification worker/sender implementation scan" \
  'send_webhook_text|send_email_text|send_bark_text|send_telegram|TelegramSendResult|telegram_poll|telegram_replies|NotificationDriverKind::(Telegram|Webhook|Email|Bark)|NotificationChannelType::(Telegram|Webhook|Email|Bark)' \
  src frontend-v2 scripts Cargo.toml migrations README.md \
  -g '!target' -g '!node_modules' -g '!dist' -g '!build' -g '!scripts/verify-s07-feishu-integration.sh'

fail_if_matches "obsolete Feishu Node sidecar implementation scan" \
  '@larksuiteoapi/node-sdk|FEISHU_SIDECAR|sidecars/feishu-bridge|feishu_sidecar|FeishuSidecar' \
  src src-tauri scripts tests Cargo.toml \
  -g '!target' -g '!node_modules' -g '!dist' -g '!build' -g '!scripts/verify-s07-feishu-integration.sh'

fail_if_matches "legacy personal WeChat product-code scan" \
  'weixin_personal' \
  src frontend-v2 \
  -g '!target' -g '!node_modules' -g '!dist' -g '!build' -g '!*.test.ts' -g '!*.test.tsx'

printf '\n[S07] verification complete: current Feishu integration matrix passed. Rust native Feishu transport, menu, item/order detail reads, item/order nonce write actions, and old-channel implementation removal are covered. Release readiness still needs real Feishu/Lark smoke, packaged App smoke, and card/button UI.\n'
