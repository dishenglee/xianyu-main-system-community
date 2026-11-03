#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FRONTEND_DIR="${FRONTEND_DIR:-$REPO_ROOT/frontend-v2}"
STATIC_DIR="${YUGUANJIA_STATIC_DIR:-$REPO_ROOT/static}"
LOG_DIR="${LOG_DIR:-$REPO_ROOT/.tmp/test-app}"
APP_RUNTIME_MODE="${APP_RUNTIME_MODE:-production}"
RESTART=0
SYNC_BUSINESS_DATA=0
TAURI_CONFIG_OVERRIDE="$(printf '{"build":{"beforeDevCommand":"","devUrl":null,"frontendDist":"%s"}}' "$STATIC_DIR")"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/start_test_app.sh --production [--restart] [--sync-business-data]
  ./scripts/start_test_app.sh --debug [--restart] [--sync-business-data]

Behavior:
  1. build frontend static assets into ./static
  2. build target/debug/xianyu-tauri
  3. launch target/debug/xianyu-tauri directly

Data behavior:
  - preserves the isolated test App data directory by default, including
    settings saved while testing
  - --sync-business-data explicitly replaces isolated business databases with
    fresh snapshots from the local production data directory

Notes:
  - this community build has no product login or license activation gate
EOF
}

for arg in "$@"; do
  case "$arg" in
    --production|--prod)
      APP_RUNTIME_MODE="production"
      ;;
    --debug)
      APP_RUNTIME_MODE="debug"
      ;;
    --restart)
      RESTART=1
      ;;
    --sync-business-data)
      SYNC_BUSINESS_DATA=1
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

# Keep the established test runtime, including its own activated test
# identity. A parent shell can legitimately carry APP_DATA_DIR for another
# App instance, so test startup must never inherit that generic variable.
DEFAULT_APP_DATA_DIR="$HOME/Library/Application Support/shop.gooofish.yuguanjia.test.$APP_RUNTIME_MODE"
APP_DATA_DIR="${XIANYU_TEST_APP_DATA_DIR:-$DEFAULT_APP_DATA_DIR}"
APP_BINARY="$REPO_ROOT/target/debug/xianyu-tauri"
APP_LOG="$LOG_DIR/$APP_RUNTIME_MODE.log"
PID_FILE="$LOG_DIR/$APP_RUNTIME_MODE.pid"
GUARD_SOURCE="$REPO_ROOT/scripts/test_app_single_instance_guard.sh"
GUARD_INSTALL_PATH="${XIANYU_TEST_APP_GUARD_INSTALL_PATH:-$HOME/.config/yuguanjia-build/test-app-single-instance-guard.sh}"

account_count_in_dir() {
  local data_dir="$1"
  local database="$data_dir/accounts.db"
  if [[ ! -f "$database" ]] || ! command -v sqlite3 >/dev/null 2>&1; then
    echo 0
    return 0
  fi
  sqlite3 "$database" 'SELECT COUNT(*) FROM accounts;' 2>/dev/null | tr -d '[:space:]' || echo 0
}

migration_max_in_dir() {
  local migration_dir="$1"
  find "$migration_dir" -maxdepth 1 -type f -name '*.sql' -print 2>/dev/null \
    | sed -E 's#.*/0*([0-9]+)_[^/]+\.sql#\1#' \
    | sort -nr \
    | head -n 1
}

database_migrations_are_compatible() {
  local database="$1"
  local migration_dir="$2"
  [[ -f "$database" ]] || return 1

  local applied available
  applied="$(sqlite3 "$database" 'SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations;' 2>/dev/null || true)"
  available="$(migration_max_in_dir "$migration_dir")"
  [[ "$applied" =~ ^[0-9]+$ && "$available" =~ ^[0-9]+$ ]] || return 1
  (( applied <= available ))
}

seed_source_is_compatible() {
  local data_dir="$1"
  database_migrations_are_compatible "$data_dir/accounts.db" "$REPO_ROOT/migrations/accounts" \
    && database_migrations_are_compatible "$data_dir/xianyu.db" "$REPO_ROOT/migrations/xianyu" \
    && database_migrations_are_compatible "$data_dir/live_state.db" "$REPO_ROOT/migrations/live_state" \
    && database_migrations_are_compatible "$data_dir/notifications.db" "$REPO_ROOT/migrations/notifications"
}

resolve_seed_source_data_dir() {
  local candidates=(
    "${XIANYU_TEST_APP_SOURCE_DATA_DIR:-}"
    "$HOME/Library/Application Support/shop.gooofish.yuguanjia"
    "$HOME/Library/Application Support/Yuguanjia"
    "$HOME/Library/Application Support/com.disheng.xianyutool"
  )
  local data_dir count
  for data_dir in "${candidates[@]}"; do
    [[ -n "$data_dir" && -d "$data_dir" ]] || continue
    seed_source_is_compatible "$data_dir" || continue
    count="$(account_count_in_dir "$data_dir")"
    [[ "$count" =~ ^[0-9]+$ ]] && (( count > 0 )) || continue
    printf '%s\n' "$data_dir"
    return 0
  done
  return 1
}

sync_sqlite_snapshot() {
  local source="$1"
  local destination="$2"
  [[ -f "$source" ]] || return 0
  mkdir -p "$(dirname "$destination")"
  sqlite3 "$source" ".backup '$destination'"
  rm -f "$destination-wal" "$destination-shm"
}

sync_test_app_business_data() {
  local source_dir="$1"
  [[ -n "$source_dir" ]] || return 1
  [[ "$source_dir" != "$APP_DATA_DIR" ]] || return 0

  echo "-> replacing isolated test business data from: $source_dir"
  sync_sqlite_snapshot "$source_dir/accounts.db" "$APP_DATA_DIR/accounts.db"
  sync_sqlite_snapshot "$source_dir/xianyu.db" "$APP_DATA_DIR/xianyu.db"
  sync_sqlite_snapshot "$source_dir/live_state.db" "$APP_DATA_DIR/live_state.db"
  sync_sqlite_snapshot "$source_dir/notifications.db" "$APP_DATA_DIR/notifications.db"
  sync_sqlite_snapshot "$source_dir/message_history.db" "$APP_DATA_DIR/message_history.db"
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

pid_on_port() {
  lsof -tiTCP:"$1" -sTCP:LISTEN -nP 2>/dev/null || true
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

restart_browser_dev_chain() {
  local vite_pids=()
  while IFS= read -r pid; do
    [[ -n "$pid" ]] && vite_pids+=("$pid")
  done < <(printf '%s\n%s\n' "$(pid_on_port 5173)" "$(pid_on_port 5188)" | sort -u)
  kill_pids "browser dev chain" "${vite_pids[@]:-}"
}

mkdir -p "$LOG_DIR"
install_test_app_guard
# shellcheck source=/dev/null
source "$GUARD_INSTALL_PATH"

test_app_guard_preflight "$REPO_ROOT" "$RESTART"

mkdir -p "$APP_DATA_DIR"
if [[ "$SYNC_BUSINESS_DATA" -eq 1 ]]; then
  SEED_SOURCE_DATA_DIR="$(resolve_seed_source_data_dir)"
  if [[ -z "$SEED_SOURCE_DATA_DIR" ]]; then
    echo "error: no migration-compatible local business data source was found" >&2
    exit 1
  fi
  sync_test_app_business_data "$SEED_SOURCE_DATA_DIR"
else
  echo "-> preserving isolated test business data: $APP_DATA_DIR"
fi

if [[ "$RESTART" -eq 1 ]]; then
  restart_browser_dev_chain
fi

if [[ ! -f "$FRONTEND_DIR/package.json" ]]; then
  echo "error: missing $FRONTEND_DIR/package.json" >&2
  exit 2
fi

echo "-> building frontend static ($APP_RUNTIME_MODE)"
APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
VITE_APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
npm --prefix "$FRONTEND_DIR" run build

echo "-> building xianyu-tauri"
cd "$REPO_ROOT"
APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
TAURI_CONFIG="$TAURI_CONFIG_OVERRIDE" \
cargo build --manifest-path src-tauri/Cargo.toml --bin xianyu-tauri --no-default-features

if [[ ! -x "$APP_BINARY" ]]; then
  echo "error: missing built binary at $APP_BINARY" >&2
  exit 1
fi

echo "-> launching test app"
echo "-> runtime mode: $APP_RUNTIME_MODE"
echo "-> app data dir: $APP_DATA_DIR"
echo "-> log file: $APP_LOG"

# On macOS under non-interactive launchers, `nohup ... &` can still die when
# the parent shell session exits. Start the app in a fresh session so the test
# App survives after this script returns.
/usr/bin/perl -MPOSIX -e 'setsid() or die "setsid failed: $!"; exec @ARGV or die "exec failed: $!";' -- \
  /usr/bin/env \
  APP_RUNTIME_MODE="$APP_RUNTIME_MODE" \
  APP_DATA_DIR="$APP_DATA_DIR" \
  YUGUANJIA_OFFLINE_SNAPSHOT="" \
  YUGUANJIA_OFFLINE_SNAPSHOT_FILE="" \
  XIANYU_DISABLE_LIVE_RUNTIME="${XIANYU_DISABLE_LIVE_RUNTIME:-}" \
  "$APP_BINARY" >"$APP_LOG" 2>&1 < /dev/null &

APP_PID=$!
echo "$APP_PID" >"$PID_FILE"
sleep 2

if ! kill -0 "$APP_PID" 2>/dev/null; then
  echo "error: test app exited early; see $APP_LOG" >&2
  tail -n 120 "$APP_LOG" >&2 || true
  exit 1
fi

if [[ -n "$(pid_on_port 5173)" || -n "$(pid_on_port 5188)" ]]; then
  echo "error: test app startup unexpectedly left browser dev chain listening on :5173 or :5188" >&2
  exit 1
fi

test_app_guard_postflight "$REPO_ROOT" "$APP_PID" "$APP_BINARY" "$APP_RUNTIME_MODE"

echo "-> test app started (pid=$APP_PID)"
