#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-}"
shift || true

if [[ "$MODE" != "production" && "$MODE" != "debug" ]]; then
  echo "error: first argument must be production or debug" >&2
  exit 2
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FRONTEND_DIR="$REPO_ROOT/frontend-v2"
STATIC_DIR="${YUGUANJIA_STATIC_DIR:-$REPO_ROOT/static}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT/.tmp/dev-app}"
APP_LOG="$LOG_DIR/test-app-$MODE.log"
BUILD_LOG="$LOG_DIR/test-app-build-$MODE.log"
SCREEN_NAME="yuguanjia-test-app-$MODE"
TAURI_BUILD_ROOT="$REPO_ROOT/target/debug/bundle/macos"
TAURI_TEST_APP_BUNDLE="$TAURI_BUILD_ROOT/xianyu-tauri.app"
RESTART=0
SCRIPT_NAME="start_test_app_prod.sh"
GUARD_SOURCE="$REPO_ROOT/scripts/test_app_single_instance_guard.sh"
GUARD_INSTALL_PATH="${XIANYU_TEST_APP_GUARD_INSTALL_PATH:-$HOME/.config/yuguanjia-build/test-app-single-instance-guard.sh}"

if [[ "$MODE" == "debug" ]]; then
  SCRIPT_NAME="start_test_app_debug.sh"
fi

for arg in "$@"; do
  case "$arg" in
    --restart)
      RESTART=1
      ;;
    -h|--help)
      cat <<EOF
用法:
  ./scripts/$SCRIPT_NAME --restart

说明:
  - 测试 App 只允许走 App 直启链路，不允许启动 Vite / localhost 网页预览
  - 会先把 frontend-v2 构建到 static，再执行 tauri build --debug --bundles app
  - 最终直启 target/debug/bundle/macos/xianyu-tauri.app
EOF
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

resolve_existing_account_count() {
  local dir="$1"
  local db="$dir/accounts.db"
  if [[ ! -f "$db" ]]; then
    echo 0
    return 0
  fi
  if ! command -v sqlite3 >/dev/null 2>&1; then
    echo 1
    return 0
  fi
  local count
  count="$(sqlite3 "$db" 'select count(*) from accounts;' 2>/dev/null | tr -d '[:space:]')"
  if [[ "$count" =~ ^[0-9]+$ ]]; then
    echo "$count"
  else
    echo 0
  fi
}

resolve_default_test_app_data_dir() {
  local default_dir="${XIANYU_TEST_APP_DATA_DIR:-$REPO_ROOT/.tmp/test-app-data/$MODE}"
  echo "$default_dir"
}

resolve_seed_source_data_dir() {
  local candidates=(
    "${XIANYU_TEST_APP_SOURCE_DATA_DIR:-}"
    "$HOME/Library/Application Support/shop.gooofish.yuguanjia"
    "$HOME/Library/Application Support/com.disheng.xianyutool"
  )
  local best_dir=""
  local best_count=0
  local dir count
  for dir in "${candidates[@]}"; do
    [[ -z "$dir" ]] && continue
    count="$(resolve_existing_account_count "$dir")"
    if [[ "$count" =~ ^[0-9]+$ ]] && (( count > best_count )); then
      best_count="$count"
      best_dir="$dir"
    fi
  done

  if [[ -n "$best_dir" && "$best_count" -gt 0 ]]; then
    echo "$best_dir"
  fi
}

sync_sqlite_snapshot() {
  local src="$1"
  local dst="$2"
  if [[ ! -f "$src" ]]; then
    return 0
  fi
  mkdir -p "$(dirname "$dst")"
  if command -v sqlite3 >/dev/null 2>&1; then
    sqlite3 "$src" ".backup '$dst'"
  else
    cp -f "$src" "$dst"
  fi
  rm -f "$dst-wal" "$dst-shm"
}

sync_test_app_business_data() {
  local target_dir="$1"
  local source_dir="$2"
  [[ -n "$source_dir" ]] || return 0
  [[ "$source_dir" != "$target_dir" ]] || return 0
  [[ -d "$source_dir" ]] || return 0

  local target_count source_count
  target_count="$(resolve_existing_account_count "$target_dir")"
  source_count="$(resolve_existing_account_count "$source_dir")"
  if ! [[ "$target_count" =~ ^[0-9]+$ && "$source_count" =~ ^[0-9]+$ ]]; then
    return 0
  fi
  if (( source_count <= 0 || target_count > 0 )); then
    return 0
  fi

  echo "-> seeding test app business data from: $source_dir"
  sync_sqlite_snapshot "$source_dir/accounts.db" "$target_dir/accounts.db"
  sync_sqlite_snapshot "$source_dir/xianyu.db" "$target_dir/xianyu.db"
  sync_sqlite_snapshot "$source_dir/live_state.db" "$target_dir/live_state.db"
  sync_sqlite_snapshot "$source_dir/notifications.db" "$target_dir/notifications.db"
}

install_test_app_guard() {
  if [[ ! -f "$GUARD_SOURCE" ]]; then
    echo "error: missing guard source at $GUARD_SOURCE" >&2
    exit 2
  fi

  mkdir -p "$(dirname "$GUARD_INSTALL_PATH")"
  if [[ ! -f "$GUARD_INSTALL_PATH" ]] || ! cmp -s "$GUARD_SOURCE" "$GUARD_INSTALL_PATH"; then
    cp "$GUARD_SOURCE" "$GUARD_INSTALL_PATH"
    chmod +x "$GUARD_INSTALL_PATH"
  fi
}

find_tauri_bin() {
  if [[ -x "$FRONTEND_DIR/node_modules/.bin/tauri" ]]; then
    TAURI_BIN=("$FRONTEND_DIR/node_modules/.bin/tauri")
    return
  fi
  if command -v tauri >/dev/null 2>&1; then
    TAURI_BIN=("tauri")
    return
  fi
  if command -v cargo-tauri >/dev/null 2>&1; then
    TAURI_BIN=("cargo" "tauri")
    return
  fi
  echo "error: tauri cli not found" >&2
  exit 2
}

make_tauri_config() {
  python3 - "$STATIC_DIR" <<'PY'
import json
import sys

print(json.dumps({
    "build": {
        "beforeBuildCommand": None,
        "frontendDist": sys.argv[1],
    }
}, ensure_ascii=False, separators=(",", ":")))
PY
}

kill_matching_processes() {
  local pattern="$1"
  pkill -f "$pattern" >/dev/null 2>&1 || true
}

resolve_app_bin() {
  if [[ ! -d "$TAURI_BUILD_ROOT" ]]; then
    echo "error: missing built macOS app under $TAURI_BUILD_ROOT" >&2
    exit 1
  fi
  if [[ ! -d "$TAURI_TEST_APP_BUNDLE" ]]; then
    echo "error: missing test app bundle at $TAURI_TEST_APP_BUNDLE" >&2
    exit 1
  fi
  local bin_path="$TAURI_TEST_APP_BUNDLE/Contents/MacOS/xianyu-tauri"
  if [[ ! -x "$bin_path" ]]; then
    echo "error: missing app binary at $bin_path" >&2
    exit 1
  fi
  echo "$bin_path"
}

prepare_test_app_bundle() {
  local source_bundle
  source_bundle="$(find "$TAURI_BUILD_ROOT" -maxdepth 1 -type d -name '*.app' ! -name 'xianyu-tauri.app' | head -n 1 || true)"
  if [[ -z "$source_bundle" ]]; then
    if [[ -d "$TAURI_TEST_APP_BUNDLE" ]]; then
      source_bundle="$TAURI_TEST_APP_BUNDLE"
    else
      echo "error: no built macOS app found under $TAURI_BUILD_ROOT" >&2
      exit 1
    fi
  fi

  if [[ "$source_bundle" != "$TAURI_TEST_APP_BUNDLE" ]]; then
    rm -rf "$TAURI_TEST_APP_BUNDLE"
    mv "$source_bundle" "$TAURI_TEST_APP_BUNDLE"
  fi

  local plist="$TAURI_TEST_APP_BUNDLE/Contents/Info.plist"
  if [[ -f "$plist" ]]; then
    plutil -replace CFBundleName -string "xianyu-tauri" "$plist" >/dev/null 2>&1 || \
      plutil -insert CFBundleName -string "xianyu-tauri" "$plist"
    plutil -replace CFBundleDisplayName -string "xianyu-tauri" "$plist" >/dev/null 2>&1 || \
      plutil -insert CFBundleDisplayName -string "xianyu-tauri" "$plist"
  fi

  if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "$TAURI_TEST_APP_BUNDLE" >/dev/null 2>&1 || true
  fi
}

find_tauri_bin
install_test_app_guard
# shellcheck source=/dev/null
source "$GUARD_INSTALL_PATH"

APP_DATA_DIR="${APP_DATA_DIR:-$(resolve_default_test_app_data_dir)}"
SEED_SOURCE_DATA_DIR="$(resolve_seed_source_data_dir)"

mkdir -p "$LOG_DIR" "$APP_DATA_DIR" "$STATIC_DIR"
sync_test_app_business_data "$APP_DATA_DIR" "$SEED_SOURCE_DATA_DIR"

test_app_guard_preflight "$REPO_ROOT" "$RESTART"

if [[ "$RESTART" -eq 1 ]]; then
  screen -S "$SCREEN_NAME" -X quit >/dev/null 2>&1 || true
  kill_matching_processes "$FRONTEND_DIR/node_modules/.bin/vite --host 127.0.0.1"
  kill_matching_processes "@tauri-apps/cli/tauri.js dev"
  kill_matching_processes "cargo run --no-default-features --color always --"
  kill_matching_processes "cargo build --package xianyu-tauri"
  kill_matching_processes "tauri build --debug --bundles app"
  sleep 1
fi

echo "-> building frontend static assets ($MODE)"
VITE_APP_RUNTIME_MODE="$MODE" YUGUANJIA_STATIC_DIR="$STATIC_DIR" npm --prefix "$FRONTEND_DIR" run build 2>&1 | tee "$BUILD_LOG"

echo "-> building tauri test app bundle ($MODE)"
rm -rf "$TAURI_BUILD_ROOT"
TAURI_CONFIG_OVERRIDE="$(make_tauri_config)"
(
  cd "$REPO_ROOT"
  VITE_APP_RUNTIME_MODE="$MODE" "${TAURI_BIN[@]}" build --debug --bundles app -c "$TAURI_CONFIG_OVERRIDE"
) 2>&1 | tee -a "$BUILD_LOG"

prepare_test_app_bundle
APP_BIN="$(resolve_app_bin)"

screen -S "$SCREEN_NAME" -X quit >/dev/null 2>&1 || true
screen -dmS "$SCREEN_NAME" bash -lc "cd \"$REPO_ROOT\" && APP_RUNTIME_MODE=\"$MODE\" VITE_APP_RUNTIME_MODE=\"$MODE\" APP_DATA_DIR=\"$APP_DATA_DIR\" \"$APP_BIN\" > \"$APP_LOG\" 2>&1"

sleep 3

if lsof -iTCP:5173 -sTCP:LISTEN -nP >/dev/null 2>&1 || lsof -iTCP:5188 -sTCP:LISTEN -nP >/dev/null 2>&1; then
  echo "error: test app startup unexpectedly left browser dev chain listening on :5173 or :5188" >&2
  exit 1
fi

if ! APP_PID="$(test_app_guard_find_pid_for_bin "$APP_BIN")"; then
  echo "error: xianyu-tauri did not stay running; see $APP_LOG" >&2
  exit 1
fi

test_app_guard_postflight "$REPO_ROOT" "$APP_PID" "$APP_BIN" "$MODE"

osascript -e 'tell application "System Events" to set frontmost of process "xianyu-tauri" to true' >/dev/null 2>&1 || true

echo "-> test app started: xianyu-tauri"
echo "-> app binary: $APP_BIN"
echo "-> app data dir: $APP_DATA_DIR"
echo "-> build log: $BUILD_LOG"
echo "-> log file: $APP_LOG"
