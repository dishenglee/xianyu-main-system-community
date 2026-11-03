#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_ROOT="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

# shellcheck source=/dev/null
source "$REPO_ROOT/scripts/test_app_business_data.sh"

assert_eq() {
  local expected="$1"
  local actual="$2"
  local message="$3"
  if [[ "$actual" != "$expected" ]]; then
    echo "assertion failed: $message" >&2
    echo "expected: $expected" >&2
    echo "actual:   $actual" >&2
    exit 1
  fi
}

create_business_source() {
  local data_dir="$1"
  local account_count="$2"
  local index=0

  mkdir -p "$data_dir"
  sqlite3 "$data_dir/accounts.db" 'CREATE TABLE accounts (uid TEXT PRIMARY KEY);'
  for ((index = 1; index <= account_count; index += 1)); do
    sqlite3 "$data_dir/accounts.db" "INSERT INTO accounts (uid) VALUES ('uid-$index');"
  done
  for db_name in xianyu live_state notifications; do
    sqlite3 "$data_dir/$db_name.db" "CREATE TABLE snapshot_marker (value TEXT); INSERT INTO snapshot_marker VALUES ('$db_name-source');"
  done
}

local_data_root="$TMP_ROOT/Application Support"
target_dir="$TMP_ROOT/test-production"
larger_source="$local_data_root/Yuguanjia"
smaller_source="$local_data_root/shop.gooofish.yuguanjia"

create_business_source "$larger_source" 3
create_business_source "$smaller_source" 2
mkdir -p "$target_dir"
sqlite3 "$target_dir/accounts.db" "CREATE TABLE accounts (uid TEXT PRIMARY KEY); INSERT INTO accounts VALUES ('stale-test-account');"
printf '%s\n' 'test-license-must-survive' >"$target_dir/license.json"
printf '%s\n' 'production-license-must-not-copy' >"$larger_source/license.json"

XIANYU_TEST_APP_LOCAL_DATA_ROOT="$local_data_root"
export XIANYU_TEST_APP_LOCAL_DATA_ROOT
resolved_source="$(test_app_resolve_business_data_source production "$target_dir")"
assert_eq "$smaller_source" "$resolved_source" "current production directory must be preferred over a larger legacy directory"

test_app_sync_business_data production "$target_dir" "$resolved_source"
assert_eq "2" "$(test_app_account_count "$target_dir")" "target account count"
assert_eq "test-license-must-survive" "$(cat "$target_dir/license.json")" "test license must not be replaced"
assert_eq "live_state-source" "$(sqlite3 "$target_dir/live_state.db" 'SELECT value FROM snapshot_marker;')" "business database snapshot"

echo "test app business data sync tests passed"
