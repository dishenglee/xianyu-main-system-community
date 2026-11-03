#!/usr/bin/env bash

test_app_account_count() {
  local data_dir="$1"
  local db_file="$data_dir/accounts.db"
  local count=""

  if [[ ! -f "$db_file" ]] || ! command -v sqlite3 >/dev/null 2>&1; then
    echo 0
    return 0
  fi

  count="$(sqlite3 "$db_file" 'SELECT COUNT(*) FROM accounts;' 2>/dev/null | tr -d '[:space:]')"
  if [[ "$count" =~ ^[0-9]+$ ]]; then
    echo "$count"
  else
    echo 0
  fi
}

test_app_resolve_business_data_source() {
  local runtime_mode="$1"
  local target_dir="$2"
  local explicit_source="${XIANYU_TEST_APP_SOURCE_DATA_DIR:-}"
  local local_data_root="${XIANYU_TEST_APP_LOCAL_DATA_ROOT:-$HOME/Library/Application Support}"

  [[ "$runtime_mode" == "production" ]] || return 0

  if [[ -n "$explicit_source" ]]; then
    if [[ "$explicit_source" == "$target_dir" || "$(test_app_account_count "$explicit_source")" -le 0 ]]; then
      echo "error: XIANYU_TEST_APP_SOURCE_DATA_DIR is not a valid production business data source: $explicit_source" >&2
      return 1
    fi
    printf '%s\n' "$explicit_source"
    return 0
  fi

  # This is the production bundle's canonical data directory. Legacy builds
  # used other directory names that can retain more, but stale, accounts and
  # databases. Prefer the active production directory whenever it is valid.
  local preferred_source="$local_data_root/shop.gooofish.yuguanjia"
  if [[ "$preferred_source" != "$target_dir" ]] && (( $(test_app_account_count "$preferred_source") > 0 )); then
    printf '%s\n' "$preferred_source"
    return 0
  fi

  local candidates=(
    "$local_data_root/Yuguanjia"
    "$local_data_root/com.disheng.xianyutool"
  )
  local best_dir=""
  local best_count=0
  local candidate=""
  local count=0

  for candidate in "${candidates[@]}"; do
    [[ "$candidate" != "$target_dir" ]] || continue
    count="$(test_app_account_count "$candidate")"
    if [[ "$count" =~ ^[0-9]+$ ]] && (( count > best_count )); then
      best_count="$count"
      best_dir="$candidate"
    fi
  done

  if (( best_count > 0 )); then
    printf '%s\n' "$best_dir"
  fi
}

test_app_sqlite_snapshot() {
  local source_db="$1"
  local target_db="$2"
  local snapshot_db="${target_db}.sync-$$"
  local quoted_snapshot="${snapshot_db//\'/\'\'}"
  local integrity=""

  [[ -f "$source_db" ]] || return 0
  mkdir -p "$(dirname "$target_db")"
  rm -f "$snapshot_db" "$snapshot_db-wal" "$snapshot_db-shm"
  sqlite3 "$source_db" ".backup '$quoted_snapshot'"
  integrity="$(sqlite3 "$snapshot_db" 'PRAGMA quick_check;' 2>/dev/null | tr -d '[:space:]')"
  if [[ "$integrity" != "ok" ]]; then
    rm -f "$snapshot_db" "$snapshot_db-wal" "$snapshot_db-shm"
    echo "error: SQLite snapshot integrity check failed for $source_db" >&2
    return 1
  fi

  rm -f "$target_db-wal" "$target_db-shm"
  mv -f "$snapshot_db" "$target_db"
}

test_app_sync_business_data() {
  local runtime_mode="$1"
  local target_dir="$2"
  local source_dir="$3"

  [[ "$runtime_mode" == "production" ]] || return 0
  [[ -n "$source_dir" ]] || return 0
  [[ "$source_dir" != "$target_dir" ]] || return 0
  if ! command -v sqlite3 >/dev/null 2>&1; then
    echo "error: sqlite3 is required to sync production business data into the test App" >&2
    return 1
  fi

  local source_count=""
  local target_count=""
  source_count="$(test_app_account_count "$source_dir")"
  if (( source_count <= 0 )); then
    echo "error: selected production business data source has no accounts: $source_dir" >&2
    return 1
  fi

  echo "-> syncing test app business data from: $source_dir (accounts=$source_count)"
  test_app_sqlite_snapshot "$source_dir/accounts.db" "$target_dir/accounts.db"
  test_app_sqlite_snapshot "$source_dir/xianyu.db" "$target_dir/xianyu.db"
  test_app_sqlite_snapshot "$source_dir/live_state.db" "$target_dir/live_state.db"
  test_app_sqlite_snapshot "$source_dir/notifications.db" "$target_dir/notifications.db"

  target_count="$(test_app_account_count "$target_dir")"
  if [[ "$target_count" != "$source_count" ]]; then
    echo "error: test app account snapshot mismatch: source=$source_count target=$target_count" >&2
    return 1
  fi
  echo "-> test app business data synced: $target_dir (accounts=$target_count)"
}
