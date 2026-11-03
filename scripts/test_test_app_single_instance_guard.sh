#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_GUARD="$REPO_ROOT/scripts/test_app_single_instance_guard.sh"
CURRENT_WORKTREE_BINARY="$REPO_ROOT/target/debug/xianyu-tauri"
TMP_ROOT="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_ROOT"
}

source_guard() {
  source "$SOURCE_GUARD" || return $?
}

assert_contains() {
  local file="$1"
  local needle="$2"
  if ! grep -F "$needle" "$file" >/dev/null 2>&1; then
    echo "expected [$needle] in $file" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_not_contains() {
  local file="$1"
  local needle="$2"
  if grep -F "$needle" "$file" >/dev/null 2>&1; then
    echo "did not expect [$needle] in $file" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_line_count() {
  local file="$1"
  local expected="$2"
  local actual

  actual="$(awk 'NF { count += 1 } END { print count + 0 }' "$file")"
  if [[ "$actual" != "$expected" ]]; then
    echo "expected $expected lines in $file, got $actual" >&2
    cat "$file" >&2
    exit 1
  fi
}

assert_equals() {
  local expected="$1"
  local actual="$2"
  local message="$3"

  if [[ "$expected" != "$actual" ]]; then
    echo "$message" >&2
    echo "expected: [$expected]" >&2
    echo "actual:   [$actual]" >&2
    exit 1
  fi
}

write_spaced_path_fixture() {
  local case_dir="$1"
  local repo_root_with_spaces="$case_dir/Repo Root With Spaces"
  local binary_with_spaces="$repo_root_with_spaces/target/debug/Xianyu Test App/xianyu-tauri"

  cat >"$case_dir/ps.txt" <<EOF
1212 screen -dmS xianyu-test-app bash -lc cd "$repo_root_with_spaces" && "$binary_with_spaces" > "$case_dir/app.log" 2>&1
1313 $binary_with_spaces --serve
EOF

  {
    printf '1212\t/usr/bin/screen\n'
    printf '1313\t%s\n' "$binary_with_spaces"
  } >"$case_dir/exe.txt"

  printf '%s\n' "$repo_root_with_spaces" >"$case_dir/repo_root.txt"
  printf '%s\n' "$binary_with_spaces" >"$case_dir/binary.txt"
}

run_conflict_requires_restart_test() {
  local case_dir="$TMP_ROOT/conflict"
  mkdir -p "$case_dir"
  cat >"$case_dir/ps.txt" <<'EOF'
1111 /tmp/xianyu-main-system/target/debug/xianyu-tauri
EOF
  printf '1111\t/tmp/xianyu-main-system/target/debug/xianyu-tauri\n' >"$case_dir/exe.txt"
  source_guard
  local stderr_file="$case_dir/stderr.txt"
  if XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
     XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
     XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
     test_app_guard_preflight "$REPO_ROOT" "0" \
     >"$case_dir/stdout.txt" 2>"$stderr_file"; then
    echo "expected conflict preflight to fail without --restart" >&2
    exit 1
  fi
  assert_contains "$stderr_file" "existing test app instance detected"
  assert_contains "$stderr_file" "1111"
}

run_restart_kills_existing_instance_test() {
  local case_dir="$TMP_ROOT/restart"
  local counter_file="$case_dir/calls.txt"
  mkdir -p "$case_dir"
  : >"$case_dir/killed.txt"
  printf '0\n' >"$counter_file"
  source_guard

  test_app_guard_collect_processes() {
    local call_count
    call_count="$(cat "$counter_file")"
    call_count="$((call_count + 1))"
    printf '%s\n' "$call_count" >"$counter_file"
    if [[ "$call_count" -eq 1 ]]; then
      printf '2222\t%s\n' "$CURRENT_WORKTREE_BINARY"
    fi
  }

  if ! XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
      XIANYU_TEST_APP_GUARD_KILL_CAPTURE="$case_dir/killed.txt" \
      test_app_guard_preflight "$REPO_ROOT" "1"; then
    echo "expected restart preflight to succeed after cleanup removes the existing instance" >&2
    exit 1
  fi
  assert_contains "$case_dir/killed.txt" "2222"
}

run_restart_fails_when_existing_instance_survives_test() {
  local case_dir="$TMP_ROOT/restart-survivor"
  local counter_file="$case_dir/calls.txt"
  local stderr_file="$case_dir/stderr.txt"

  mkdir -p "$case_dir"
  : >"$case_dir/killed.txt"
  printf '0\n' >"$counter_file"

  source_guard

  test_app_guard_collect_processes() {
    local call_count
    call_count="$(cat "$counter_file")"
    call_count="$((call_count + 1))"
    printf '%s\n' "$call_count" >"$counter_file"
    printf '2323\t%s\n' "$CURRENT_WORKTREE_BINARY"
  }

  if XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
     XIANYU_TEST_APP_GUARD_KILL_CAPTURE="$case_dir/killed.txt" \
     test_app_guard_preflight "$REPO_ROOT" "1" \
     >"$case_dir/stdout.txt" 2>"$stderr_file"; then
    echo "expected restart preflight to fail when cleanup leaves an instance behind" >&2
    exit 1
  fi

  assert_contains "$case_dir/killed.txt" "2323"
  assert_contains "$stderr_file" "cleanup did not remove all existing instances"
  assert_contains "$stderr_file" "2323"
}

run_preflight_fails_when_executable_lookup_is_unavailable_test() {
  local case_dir="$TMP_ROOT/unresolved-exe"
  local stderr_file="$case_dir/stderr.txt"

  mkdir -p "$case_dir"
  cat >"$case_dir/ps.txt" <<'EOF'
5151 /tmp/xianyu-worktree/xianyu-main-system/target/debug/xianyu-tauri
EOF
  : >"$case_dir/exe.txt"

  source_guard

  if XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
     XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
     XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
     test_app_guard_preflight "$REPO_ROOT" "0" \
     >"$case_dir/stdout.txt" 2>"$stderr_file"; then
    echo "expected preflight to fail when executable lookup is unavailable" >&2
    exit 1
  fi

  assert_contains "$stderr_file" "failed to resolve executable path for candidate pid=5151"
}

run_collect_processes_ignores_wrapper_rows_test() {
  local case_dir="$TMP_ROOT/collect"
  mkdir -p "$case_dir"
  cat >"$case_dir/ps.txt" <<EOF
4444 /bin/sleep 30
5555 $CURRENT_WORKTREE_BINARY
6666 SCREEN -dmS xianyu-test-app bash -lc cd "$REPO_ROOT" && "$CURRENT_WORKTREE_BINARY" > "$case_dir/app.log" 2>&1
EOF
  {
    printf '5555\t%s\n' "$CURRENT_WORKTREE_BINARY"
    printf '6666\t/usr/bin/SCREEN\n'
  } >"$case_dir/exe.txt"
  : >"$case_dir/exe-capture.txt"
  source_guard
  XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
  XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
  XIANYU_TEST_APP_GUARD_EXE_CAPTURE="$case_dir/exe-capture.txt" \
    test_app_guard_collect_processes >"$case_dir/processes.txt"
  assert_line_count "$case_dir/processes.txt" "1"
  assert_contains "$case_dir/processes.txt" $'5555\t'"$CURRENT_WORKTREE_BINARY"
  assert_not_contains "$case_dir/processes.txt" "6666"
  assert_not_contains "$case_dir/exe-capture.txt" "4444"
  assert_contains "$case_dir/exe-capture.txt" "5555"
  assert_contains "$case_dir/exe-capture.txt" "6666"
}

run_find_pid_for_bin_prefers_real_binary_test() {
  local case_dir="$TMP_ROOT/find-pid"

  mkdir -p "$case_dir"
  cat >"$case_dir/ps.txt" <<EOF
8181 SCREEN -dmS xianyu-test-app bash -lc cd "$REPO_ROOT" && "$CURRENT_WORKTREE_BINARY" > "$case_dir/app.log" 2>&1
8282 $CURRENT_WORKTREE_BINARY --serve
EOF
  {
    printf '8181\t/usr/bin/SCREEN\n'
    printf '8282\t%s\n' "$CURRENT_WORKTREE_BINARY"
  } >"$case_dir/exe.txt"

  source_guard

  XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
  XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
    test_app_guard_find_pid_for_bin "$CURRENT_WORKTREE_BINARY" >"$case_dir/pid.txt"

  assert_contains "$case_dir/pid.txt" "8282"
}

run_postflight_rejects_multiple_instances_test() {
  local case_dir="$TMP_ROOT/postflight"
  mkdir -p "$case_dir"
  printf '3333 %s\n4444 /tmp/other-worktree/xianyu-main-system/target/debug/xianyu-tauri\n' \
    "$CURRENT_WORKTREE_BINARY" >"$case_dir/ps.txt"
  {
    printf '3333\t%s\n' "$CURRENT_WORKTREE_BINARY"
    printf '4444\t/tmp/other-worktree/xianyu-main-system/target/debug/xianyu-tauri\n'
  } >"$case_dir/exe.txt"
  source_guard
  local stderr_file="$case_dir/stderr.txt"
  if XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
     XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
     XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
     test_app_guard_postflight \
       "$REPO_ROOT" \
       "3333" \
       "$CURRENT_WORKTREE_BINARY" \
       "production" >"$case_dir/stdout.txt" 2>"$stderr_file"; then
    echo "expected postflight to fail when two instances exist" >&2
    exit 1
  fi
  assert_contains "$stderr_file" "expected exactly one xianyu-tauri instance"
}

run_postflight_rejects_unexpected_process_test() {
  local case_dir="$TMP_ROOT/postflight-mismatch"
  mkdir -p "$case_dir"
  printf '9999 %s\n' "$CURRENT_WORKTREE_BINARY" >"$case_dir/ps.txt"
  printf '9999\t%s\n' "$CURRENT_WORKTREE_BINARY" >"$case_dir/exe.txt"
  source_guard
  local stderr_file="$case_dir/stderr.txt"
  if XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
     XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
     XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
     test_app_guard_postflight \
       "$REPO_ROOT" \
       "7777" \
       "$CURRENT_WORKTREE_BINARY" \
       "production" >"$case_dir/stdout.txt" 2>"$stderr_file"; then
    echo "expected postflight to fail when observed pid does not match launched process" >&2
    exit 1
  fi
  assert_contains "$stderr_file" "does not match launched process"
}

run_postflight_writes_owner_state_for_expected_process_test() {
  local case_dir="$TMP_ROOT/postflight-success"
  mkdir -p "$case_dir"
  cat >"$case_dir/ps.txt" <<EOF
7777 $CURRENT_WORKTREE_BINARY
8888 bash -lc cd "$REPO_ROOT" && "$CURRENT_WORKTREE_BINARY" > "$case_dir/app.log" 2>&1
EOF
  {
    printf '7777\t%s\n' "$CURRENT_WORKTREE_BINARY"
    printf '8888\t/bin/bash\n'
  } >"$case_dir/exe.txt"
  source_guard
  XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
  XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
  XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
  test_app_guard_postflight \
    "$REPO_ROOT" \
    "7777" \
    "$CURRENT_WORKTREE_BINARY" \
    "production"
  assert_contains "$case_dir/state/owner.env" "TEST_APP_OWNER_REPO_ROOT=$REPO_ROOT"
  assert_contains "$case_dir/state/owner.env" "TEST_APP_OWNER_PID=7777"
  assert_contains "$case_dir/state/owner.env" "TEST_APP_OWNER_BIN=$CURRENT_WORKTREE_BINARY"
  assert_contains "$case_dir/state/owner.env" "TEST_APP_OWNER_MODE=production"
}

run_spaced_path_fixture_round_trip_test() {
  local case_dir="$TMP_ROOT/spaced-paths"
  local repo_root_with_spaces
  local binary_with_spaces

  mkdir -p "$case_dir"
  write_spaced_path_fixture "$case_dir"
  repo_root_with_spaces="$(cat "$case_dir/repo_root.txt")"
  binary_with_spaces="$(cat "$case_dir/binary.txt")"

  source_guard
  XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
  XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
    test_app_guard_collect_processes >"$case_dir/processes.txt"

  assert_line_count "$case_dir/processes.txt" "1"
  assert_contains "$case_dir/processes.txt" $'1313\t'"$binary_with_spaces --serve"
  assert_not_contains "$case_dir/processes.txt" "1212"

  XIANYU_TEST_APP_GUARD_STATE_DIR="$case_dir/state" \
  XIANYU_TEST_APP_GUARD_PS_FIXTURE="$case_dir/ps.txt" \
  XIANYU_TEST_APP_GUARD_EXE_FIXTURE="$case_dir/exe.txt" \
  test_app_guard_postflight \
    "$repo_root_with_spaces" \
    "1313" \
    "$binary_with_spaces" \
    "production"

  unset TEST_APP_OWNER_REPO_ROOT TEST_APP_OWNER_PID TEST_APP_OWNER_BIN TEST_APP_OWNER_MODE
  # shellcheck disable=SC1090
  source "$case_dir/state/owner.env"

  assert_equals "$repo_root_with_spaces" "$TEST_APP_OWNER_REPO_ROOT" "owner repo root did not round-trip through owner.env"
  assert_equals "1313" "$TEST_APP_OWNER_PID" "owner pid did not round-trip through owner.env"
  assert_equals "$binary_with_spaces" "$TEST_APP_OWNER_BIN" "owner binary did not round-trip through owner.env"
  assert_equals "production" "$TEST_APP_OWNER_MODE" "owner mode did not round-trip through owner.env"
}

main() {
  run_conflict_requires_restart_test
  run_restart_kills_existing_instance_test
  run_restart_fails_when_existing_instance_survives_test
  run_preflight_fails_when_executable_lookup_is_unavailable_test
  run_collect_processes_ignores_wrapper_rows_test
  run_find_pid_for_bin_prefers_real_binary_test
  run_postflight_rejects_multiple_instances_test
  run_postflight_rejects_unexpected_process_test
  run_postflight_writes_owner_state_for_expected_process_test
  run_spaced_path_fixture_round_trip_test
  echo "guard tests passed"
}

if main; then
  status=0
else
  status=$?
fi

cleanup
exit "$status"
