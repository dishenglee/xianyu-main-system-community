#!/usr/bin/env bash

test_app_guard_state_dir() {
  echo "${XIANYU_TEST_APP_GUARD_STATE_DIR:-$HOME/.config/yuguanjia-build/test-app-single-instance}"
}

test_app_guard_install_path() {
  echo "${XIANYU_TEST_APP_GUARD_INSTALL_PATH:-$HOME/.config/yuguanjia-build/test-app-single-instance-guard.sh}"
}

test_app_guard_state_file() {
  echo "$(test_app_guard_state_dir)/owner.env"
}

test_app_guard_lsof_bin() {
  if [[ -x "/usr/sbin/lsof" ]]; then
    echo "/usr/sbin/lsof"
    return 0
  fi

  command -v lsof 2>/dev/null || true
}

test_app_guard_collect_process_lines() {
  if [[ -n "${XIANYU_TEST_APP_GUARD_PS_FIXTURE:-}" ]]; then
    cat "$XIANYU_TEST_APP_GUARD_PS_FIXTURE"
    return 0
  fi

  ps ax -o pid= -o command=
}

test_app_guard_read_executable_path() {
  local pid="$1"
  local fixture_pid=""
  local fixture_exe=""
  local exe_path=""
  local lsof_bin=""

  if [[ -n "${XIANYU_TEST_APP_GUARD_EXE_CAPTURE:-}" ]]; then
    printf '%s\n' "$pid" >>"$XIANYU_TEST_APP_GUARD_EXE_CAPTURE"
  fi

  if [[ -n "${XIANYU_TEST_APP_GUARD_EXE_FIXTURE:-}" ]]; then
    while IFS=$'\t' read -r fixture_pid fixture_exe; do
      [[ "$fixture_pid" == "$pid" ]] || continue
      printf '%s\n' "$fixture_exe"
      return 0
    done <"$XIANYU_TEST_APP_GUARD_EXE_FIXTURE"
    return 1
  fi

  lsof_bin="$(test_app_guard_lsof_bin)"
  if [[ -n "$lsof_bin" ]]; then
    exe_path="$(
      "$lsof_bin" -Fn -a -d txt -p "$pid" 2>/dev/null | awk '
        /^n/ {
          sub(/^n/, "", $0)
          print
          exit
        }
      '
    )"
    if [[ -n "$exe_path" ]]; then
      printf '%s\n' "$exe_path"
      return 0
    fi
  fi

  exe_path="$(ps -p "$pid" -o comm= 2>/dev/null | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"
  if [[ -n "$exe_path" ]]; then
    printf '%s\n' "$exe_path"
    return 0
  fi

  return 1
}

test_app_guard_collect_processes() {
  local line=""
  local pid=""
  local command=""
  local exe_path=""

  while IFS= read -r line; do
    line="${line#"${line%%[![:space:]]*}"}"
    [[ -n "$line" ]] || continue
    pid="${line%%[[:space:]]*}"
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    command="${line#"$pid"}"
    command="${command#"${command%%[![:space:]]*}"}"
    [[ -n "$command" ]] || continue
    [[ "$command" == *xianyu-tauri* ]] || continue
    if ! exe_path="$(test_app_guard_read_executable_path "$pid")"; then
      echo "error: failed to resolve executable path for candidate pid=$pid" >&2
      echo "error: candidate command: $command" >&2
      return 1
    fi
    [[ "${exe_path##*/}" == "xianyu-tauri" ]] || continue
    printf '%s\t%s\n' "$pid" "$command"
  done < <(test_app_guard_collect_process_lines)
}

test_app_guard_find_pid_for_bin() {
  local app_bin="$1"
  local line=""
  local pid=""
  local command=""
  local exe_path=""
  local matches=()

  while IFS= read -r line; do
    line="${line#"${line%%[![:space:]]*}"}"
    [[ -n "$line" ]] || continue
    pid="${line%%[[:space:]]*}"
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    command="${line#"$pid"}"
    command="${command#"${command%%[![:space:]]*}"}"
    [[ -n "$command" ]] || continue
    [[ "$command" == *xianyu-tauri* ]] || continue
    if ! exe_path="$(test_app_guard_read_executable_path "$pid")"; then
      echo "error: failed to resolve executable path for candidate pid=$pid" >&2
      echo "error: candidate command: $command" >&2
      return 1
    fi
    [[ "$exe_path" == "$app_bin" ]] || continue
    matches+=("$pid")
  done < <(test_app_guard_collect_process_lines)

  if [[ "${#matches[@]}" -ne 1 ]]; then
    echo "error: expected exactly one live process for $app_bin" >&2
    printf '%s\n' "${matches[@]}" >&2
    return 1
  fi

  printf '%s\n' "${matches[0]}"
}

test_app_guard_kill_pid() {
  local pid="$1"

  if [[ -n "${XIANYU_TEST_APP_GUARD_KILL_CAPTURE:-}" ]]; then
    printf '%s\n' "$pid" >>"$XIANYU_TEST_APP_GUARD_KILL_CAPTURE"
    return 0
  fi

  kill "$pid" >/dev/null 2>&1 || true
  sleep 1
  kill -9 "$pid" >/dev/null 2>&1 || true
}

test_app_guard_preflight() {
  local repo_root="$1"
  local restart_flag="$2"
  local rows

  if ! rows="$(test_app_guard_collect_processes)"; then
    return 1
  fi
  [[ -n "$rows" ]] || return 0

  if [[ "$restart_flag" != "1" ]]; then
    echo "error: existing test app instance detected; use --restart to replace it" >&2
    echo "$rows" >&2
    return 1
  fi

  while IFS=$'\t' read -r pid command; do
    [[ -n "$pid" ]] || continue
    test_app_guard_kill_pid "$pid"
  done <<<"$rows"

  if ! rows="$(test_app_guard_collect_processes)"; then
    return 1
  fi
  if [[ -n "$rows" ]]; then
    echo "error: restart cleanup did not remove all existing instances" >&2
    echo "$rows" >&2
    return 1
  fi
}

test_app_guard_postflight() {
  local repo_root="$1"
  local app_pid="$2"
  local app_bin="$3"
  local mode="$4"
  local rows
  local count
  local observed_pid=""
  local observed_bin=""

  if ! rows="$(test_app_guard_collect_processes)"; then
    return 1
  fi
  count="$(printf '%s\n' "$rows" | awk 'NF { count += 1 } END { print count + 0 }')"

  if [[ "$count" -ne 1 ]]; then
    echo "error: expected exactly one xianyu-tauri instance after launch" >&2
    echo "$rows" >&2
    return 1
  fi

  while IFS=$'\t' read -r pid command; do
    [[ -n "$pid" ]] || continue
    observed_pid="$pid"
    break
  done <<<"$rows"

  observed_bin="$(test_app_guard_read_executable_path "$observed_pid")"

  if [[ "$observed_pid" != "$app_pid" || "$observed_bin" != "$app_bin" ]]; then
    echo "error: observed xianyu-tauri instance does not match launched process" >&2
    echo "expected pid=$app_pid bin=$app_bin" >&2
    echo "$rows" >&2
    return 1
  fi

  mkdir -p "$(test_app_guard_state_dir)"
  {
    printf 'TEST_APP_OWNER_REPO_ROOT=%q\n' "$repo_root"
    printf 'TEST_APP_OWNER_PID=%q\n' "$app_pid"
    printf 'TEST_APP_OWNER_BIN=%q\n' "$app_bin"
    printf 'TEST_APP_OWNER_MODE=%q\n' "$mode"
  } >"$(test_app_guard_state_file)"
}
