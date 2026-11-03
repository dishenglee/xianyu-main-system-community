#!/usr/bin/env bash
#
# Start the desktop App with an explicit runtime mode.
#
# Why this script exists:
#   Running `tauri dev` directly can execute `beforeDevCommand` from the wrong
#   working directory in this repo, making npm look for a mismatched frontend
#   package.json.
#   The stable flow is:
#     1. start Vite from an absolute frontend path
#     2. run Tauri with beforeDevCommand disabled
#
# Usage:
#   ./scripts/start_app_dev.sh --production           # frontend-v2 + production runtime
#   ./scripts/start_app_dev.sh --debug                # frontend-v2 + debug runtime
#   ./scripts/start_app_dev.sh --restart --production # restart and relaunch App
#
set -euo pipefail

if [[ "${XIANYU_ALLOW_BROWSER_DEV_CHAIN:-0}" != "1" ]]; then
  cat >&2 <<'EOF'
error: 浏览器开发链路已默认禁用。

本项目测试 App 只能走 App 直启链路：
  - 生产测试模式：./scripts/start_test_app_prod.sh --restart
  - 调试测试模式：./scripts/start_test_app_debug.sh --restart

如确实要临时调试旧的浏览器/Vite 链路，必须显式设置：
  XIANYU_ALLOW_BROWSER_DEV_CHAIN=1
EOF
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_RUNTIME_MODE="${APP_RUNTIME_MODE:-production}"
FRONTEND_DIR_OVERRIDE="${FRONTEND_DIR:-}"
FRONTEND_PORT_OVERRIDE="${FRONTEND_PORT:-}"
FRONTEND_HOST="${FRONTEND_HOST:-127.0.0.1}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT/.tmp/dev-app}"
TAURI_CONFIG_OVERRIDE=""

RESTART=0
for arg in "$@"; do
  case "$arg" in
    --restart) RESTART=1 ;;
    --legacy)
      ;;
    --prod|--production)
      APP_RUNTIME_MODE="production"
      ;;
    --debug)
      APP_RUNTIME_MODE="debug"
      ;;
    -h|--help)
      sed -n '1,36p' "$0"
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if [[ "${XIANYU_ALLOW_BROWSER_DEV_CHAIN:-0}" != "1" ]]; then
  cat >&2 <<'EOF'
error: browser dev chain is disabled by default for this project
hint: use ./scripts/start_test_app_prod.sh --restart or ./scripts/start_test_app_debug.sh --restart
hint: only set XIANYU_ALLOW_BROWSER_DEV_CHAIN=1 for explicit maintenance work
EOF
  exit 2
fi

DEFAULT_FRONTEND_DIR="$REPO_ROOT/frontend-v2"
DEFAULT_FRONTEND_PORT="5173"

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
  local default_dir="${XIANYU_TEST_APP_DATA_DIR:-$REPO_ROOT/.tmp/test-app-data/$APP_RUNTIME_MODE}"
  echo "$default_dir"
}

resolve_seed_source_data_dir() {
  if [[ "$APP_RUNTIME_MODE" != "production" ]]; then
    return 0
  fi

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
  [[ "$APP_RUNTIME_MODE" == "production" ]] || return 0
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

FRONTEND_DIR="${FRONTEND_DIR_OVERRIDE:-$DEFAULT_FRONTEND_DIR}"
FRONTEND_PORT="${FRONTEND_PORT_OVERRIDE:-$DEFAULT_FRONTEND_PORT}"
VITE_LOG="$LOG_DIR/vite-$FRONTEND_PORT.log"
DEFAULT_TEST_APP_DATA_DIR="$(resolve_default_test_app_data_dir)"
APP_DATA_DIR="${APP_DATA_DIR:-$DEFAULT_TEST_APP_DATA_DIR}"
SEED_SOURCE_DATA_DIR="$(resolve_seed_source_data_dir)"
TAURI_DEV_IDENTIFIER="${TAURI_DEV_IDENTIFIER:-shop.gooofish.yuguanjia.test.$APP_RUNTIME_MODE}"

TAURI_DEV_URL="${TAURI_DEV_URL:-http://$FRONTEND_HOST:$FRONTEND_PORT}"
TAURI_CONFIG_OVERRIDE="$(printf '{"identifier":"%s","build":{"beforeDevCommand":"","devUrl":"%s"}}' "$TAURI_DEV_IDENTIFIER" "$TAURI_DEV_URL")"

mkdir -p "$LOG_DIR"
mkdir -p "$APP_DATA_DIR"
sync_test_app_business_data "$APP_DATA_DIR" "$SEED_SOURCE_DATA_DIR"

pid_on_port() {
  lsof -tiTCP:"$1" -sTCP:LISTEN -nP 2>/dev/null || true
}

collect_pids_by_substring() {
  local needle
  for needle in "$@"; do
    [[ -z "$needle" ]] && continue
    ps ax -o pid= -o command= 2>/dev/null | awk -v n="$needle" 'index($0, n) { print $1 }'
  done | sort -u
}

kill_pids() {
  local label="$1"
  shift
  local pids=()
  local pid
  for pid in "$@"; do
    [[ -n "$pid" ]] && pids+=("$pid")
  done
  if [[ ${#pids[@]} -eq 0 ]]; then
    return 0
  fi
  echo "-> stopping $label: ${pids[*]}"
  kill "${pids[@]}" 2>/dev/null || true
  sleep 1
  local alive=()
  for pid in "${pids[@]}"; do
    if kill -0 "$pid" 2>/dev/null; then
      alive+=("$pid")
    fi
  done
  if [[ ${#alive[@]} -gt 0 ]]; then
    kill -9 "${alive[@]}" 2>/dev/null || true
  fi
}

if [[ "$RESTART" -eq 1 ]]; then
  vite_pids=()
  while IFS= read -r pid; do
    [[ -n "$pid" ]] && vite_pids+=("$pid")
  done < <(pid_on_port "$FRONTEND_PORT")
  kill_pids "Vite on :$FRONTEND_PORT" "${vite_pids[@]:-}"

  tauri_pids=()
  while IFS= read -r pid; do
    [[ -n "$pid" ]] && tauri_pids+=("$pid")
  done < <(
    collect_pids_by_substring \
      "$REPO_ROOT/target/debug/xianyu-tauri" \
      "$REPO_ROOT/target/debug/bundle/macos/鱼管家.app/Contents/MacOS/xianyu-tauri"
  )
  kill_pids "desktop app" "${tauri_pids[@]:-}"

  tauri_cli_pids=()
  while IFS= read -r pid; do
    [[ -n "$pid" ]] && tauri_cli_pids+=("$pid")
  done < <(
    collect_pids_by_substring "@tauri-apps/cli/tauri.js dev"
  )
  kill_pids "tauri dev cli" "${tauri_cli_pids[@]:-}"
fi

if [[ ! -f "$FRONTEND_DIR/package.json" ]]; then
  echo "error: missing $FRONTEND_DIR/package.json" >&2
  exit 2
fi

TAURI_BIN="$FRONTEND_DIR/node_modules/.bin/tauri"
if [[ ! -x "$TAURI_BIN" ]]; then
  echo "error: missing Tauri CLI at $TAURI_BIN" >&2
  echo "       run: npm --prefix \"$FRONTEND_DIR\" install" >&2
  exit 2
fi

VITE_STARTED=0
if [[ -z "$(pid_on_port "$FRONTEND_PORT")" ]]; then
  echo "-> starting Vite ($APP_RUNTIME_MODE): http://$FRONTEND_HOST:$FRONTEND_PORT"
  APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
  VITE_APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
  npm --prefix "$FRONTEND_DIR" run dev -- \
    --host "$FRONTEND_HOST" \
    --port "$FRONTEND_PORT" \
    --strictPort >"$VITE_LOG" 2>&1 &
  VITE_PID=$!
  VITE_STARTED=1
else
  echo "-> Vite already listening on :$FRONTEND_PORT"
fi

cleanup() {
  if [[ "${VITE_STARTED:-0}" -eq 1 && -n "${VITE_PID:-}" ]]; then
    kill "$VITE_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

echo "-> waiting for Vite"
for _ in $(seq 1 80); do
  if curl -fsS "http://$FRONTEND_HOST:$FRONTEND_PORT/" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done

if ! curl -fsS "http://$FRONTEND_HOST:$FRONTEND_PORT/" >/dev/null 2>&1; then
  echo "error: Vite did not become ready; see $VITE_LOG" >&2
  exit 1
fi

echo "-> starting Tauri App ($APP_RUNTIME_MODE); frontend logs: $VITE_LOG"
echo "-> using test app data dir: $APP_DATA_DIR"
echo "-> using test app identifier: $TAURI_DEV_IDENTIFIER"
cd "$REPO_ROOT"
APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
APP_DATA_DIR="$APP_DATA_DIR" \
VITE_APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
"$TAURI_BIN" dev -c "$TAURI_CONFIG_OVERRIDE"
