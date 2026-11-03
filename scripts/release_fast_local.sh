#!/usr/bin/env bash
#
# Fast local release entrypoint.
#
# This uses the normal production release scripts and only adds local environment
# detection so repeated builds reuse this machine's warm Cargo/Gradle/Android caches.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

usage() {
  cat >&2 <<'EOF'
usage: ./scripts/release_fast_local.sh <target>

targets:
  check       print the detected local fast-release environment
  doctor      verify local macOS, Windows and Android release prerequisites
  mac         build macOS universal release
  win         build Windows NSIS release via cargo-xwin
  android     build Android release APK
  desktop     build macOS universal, then Windows
  all         build macOS universal, Windows, then Android
  all-parallel
              build macOS, Windows and Android concurrently in isolated
              workspaces with separate CARGO_TARGET_DIR/static outputs

This builds local release artifacts. It does not require product-license keys.
EOF
}

target="${1:-check}"
case "$target" in
  check|doctor|mac|win|android|desktop|all|all-parallel|parallel) ;;
  -h|--help|help) usage; exit 0 ;;
  *) usage; exit 2 ;;
esac

# shellcheck disable=SC1091
source "$REPO_ROOT/scripts/local_fast_release_env.sh"

if [[ "$target" == "check" ]]; then
  "$REPO_ROOT/scripts/local_fast_release_env.sh" --check
  exit 0
fi

"$REPO_ROOT/scripts/check_release_version_consistency.py"

doctor() {
  local ok=1

  check_cmd() {
    local name="$1"
    local cmd="$2"
    if command -v "$cmd" >/dev/null 2>&1; then
      echo "ok: $name -> $(command -v "$cmd")"
    else
      echo "missing: $name ($cmd)" >&2
      ok=0
    fi
  }

  check_path() {
    local name="$1"
    local path="$2"
    if [[ -e "$path" ]]; then
      echo "ok: $name -> $path"
    else
      echo "missing: $name -> $path" >&2
      ok=0
    fi
  }

  check_any_path() {
    local name="$1"
    shift
    local path
    for path in "$@"; do
      if [[ -e "$path" ]]; then
        echo "ok: $name -> $path"
        return 0
      fi
    done
    echo "missing: $name -> $*" >&2
    ok=0
  }

  check_android_ndk_clang() {
    local ndk_dir="${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
    local clang_path=""
    if [[ -n "$ndk_dir" && -d "$ndk_dir/toolchains/llvm/prebuilt" ]]; then
      local candidate
      for candidate in "$ndk_dir"/toolchains/llvm/prebuilt/*/bin/aarch64-linux-android24-clang; do
        if [[ -x "$candidate" ]]; then
          clang_path="$candidate"
          break
        fi
      done
    fi
    if [[ -z "$clang_path" ]]; then
      echo "missing: Android NDK clang -> aarch64-linux-android24-clang" >&2
      ok=0
      return 0
    fi
    if "$clang_path" --version >/dev/null 2>&1; then
      echo "ok: Android NDK clang executes -> $clang_path"
    else
      echo "missing: Android NDK clang cannot execute -> $clang_path" >&2
      echo "hint: on arm64 Linux hosts, install qemu-user-static, binfmt-support, and amd64 runtime libraries for the x86_64 Android NDK toolchain" >&2
      ok=0
    fi
  }

  check_tauri_linux_tray_dependency() {
    [[ "$(uname -s)" == "Linux" ]] || return 0

    check_cmd "pkg-config" pkg-config
    if pkg-config --exists ayatana-appindicator3-0.1 2>/dev/null; then
      echo "ok: Tauri tray appindicator pkg-config -> ayatana-appindicator3-0.1"
      return 0
    fi
    if pkg-config --exists appindicator3-0.1 2>/dev/null; then
      echo "ok: Tauri tray appindicator pkg-config -> appindicator3-0.1"
      return 0
    fi

    echo "missing: Tauri tray appindicator pkg-config -> install libayatana-appindicator3-dev" >&2
    ok=0
  }

  echo "macOS release prerequisites:"
  check_cmd "cargo-tauri" cargo-tauri
  if [[ "$(uname -s)" == "Darwin" ]]; then
    check_cmd "codesign" codesign
    check_cmd "hdiutil" hdiutil
  else
    check_path "osxcross root" "${OSXCROSS_ROOT:-}"
    check_path "osxcross SDK" "${OSXCROSS_SDKROOT:-}"
    check_cmd "aarch64 Apple clang" "${CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER:-aarch64-apple-darwin24.4-clang}"
    check_cmd "x86_64 Apple clang" "${CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER:-x86_64-apple-darwin24.4-clang}"
    check_cmd "Apple lipo" x86_64-apple-darwin24.4-lipo
    check_cmd "rcodesign" rcodesign
    check_path "Linux UDIF maker" "$REPO_ROOT/tools/linux-udif-maker/Cargo.toml"
  fi
  if rustup target list --installed | grep -qx 'aarch64-apple-darwin'; then
    echo "ok: Rust target -> aarch64-apple-darwin"
  else
    echo "missing: Rust target aarch64-apple-darwin" >&2
    ok=0
  fi
  if rustup target list --installed | grep -qx 'x86_64-apple-darwin'; then
    echo "ok: Rust target -> x86_64-apple-darwin"
  else
    echo "missing: Rust target x86_64-apple-darwin" >&2
    ok=0
  fi

  echo
  echo "Windows release prerequisites:"
  check_cmd "cargo-tauri" cargo-tauri
  check_cmd "cargo-xwin" cargo-xwin
  check_cmd "NSIS makensis" makensis
  check_cmd "Windows Authenticode signer" osslsigncode
  check_cmd "OpenSSL" openssl
  if rustup target list --installed | grep -qx 'x86_64-pc-windows-msvc'; then
    echo "ok: Rust target -> x86_64-pc-windows-msvc"
  else
    echo "missing: Rust target x86_64-pc-windows-msvc" >&2
    ok=0
  fi
  check_any_path "cargo-xwin SDK cache" \
    "$HOME/Library/Caches/cargo-xwin/xwin/sdk" \
    "$HOME/.cache/cargo-xwin/xwin/sdk"
  check_any_path "cargo-xwin CRT cache" \
    "$HOME/Library/Caches/cargo-xwin/xwin/crt" \
    "$HOME/.cache/cargo-xwin/xwin/crt"
  check_tauri_linux_tray_dependency

  echo
  echo "Android release prerequisites:"
  check_cmd "cargo-tauri" cargo-tauri
  check_cmd "Java" java
  check_cmd "keytool" keytool
  check_cmd "sdkmanager" sdkmanager
  check_cmd "adb" adb
  check_cmd "apksigner" apksigner
  check_cmd "zipalign" zipalign
  check_path "Android SDK" "${ANDROID_HOME:-}"
  check_path "Android platform-tools" "${ANDROID_HOME:-}/platform-tools"
  check_path "Android build-tools" "${ANDROID_HOME:-}/build-tools"
  check_path "Android platforms" "${ANDROID_HOME:-}/platforms"
  check_path "Android NDK" "${ANDROID_NDK_HOME:-${NDK_HOME:-}}"
  check_android_ndk_clang
  check_path "Tauri Android project" "$REPO_ROOT/src-tauri/gen/android"
  check_path "Android release keystore" "$HOME/.config/yuguanjia-build/android-release.keystore"
  check_path "Android release keystore env" "$HOME/.config/yuguanjia-build/android-keystore.env"
  if rustup target list --installed | grep -qx 'aarch64-linux-android'; then
    echo "ok: Rust target -> aarch64-linux-android"
  else
    echo "missing: Rust target aarch64-linux-android" >&2
    ok=0
  fi

  [[ "$ok" == "1" ]]
}

if [[ "$target" == "doctor" ]]; then
  doctor
  exit $?
fi

run_step() {
  local name="$1"
  shift
  local started=$SECONDS
  echo
  echo "==> $name"
  "$@"
  local elapsed=$((SECONDS - started))
  echo "==> $name complete in ${elapsed}s"
}

show_artifacts() {
  echo
  echo "Artifacts:"
  local roots=()
  local windows_roots=()
  [[ -d "$REPO_ROOT/target" ]] && roots+=("$REPO_ROOT/target")
  [[ -d "$REPO_ROOT/src-tauri/gen/android/app/build/outputs" ]] && roots+=("$REPO_ROOT/src-tauri/gen/android/app/build/outputs")
  [[ -d "$REPO_ROOT/target/x86_64-pc-windows-msvc/release/bundle/nsis" ]] && windows_roots+=("$REPO_ROOT/target/x86_64-pc-windows-msvc/release/bundle/nsis")
  {
    if [[ "${#roots[@]}" -gt 0 ]]; then
      find "${roots[@]}" \
        \( -name '*.dmg' -o -name '*.app' -o -name '*.apk' \) \
        -print 2>/dev/null
    fi
    if [[ "${#windows_roots[@]}" -gt 0 ]]; then
      find "${windows_roots[@]}" -type f -name '*.exe' -print 2>/dev/null
    fi
  } | sort
}

prepare_parallel_workspace() {
  local name="$1"
  local dest="$2"

  if ! command -v rsync >/dev/null 2>&1; then
    echo "error: rsync is required for isolated parallel release workspaces" >&2
    return 2
  fi

  mkdir -p "$dest"
  rsync -a --delete --link-dest="$REPO_ROOT" \
    --exclude='/.git/' \
    --exclude='/target/' \
    --exclude='/static/' \
    --exclude='/.tmp/release-parallel/' \
    --exclude='/frontend-v2/*.tsbuildinfo' \
    --exclude='/src-tauri/gen/android/local.properties' \
    --exclude='/src-tauri/gen/android/.gradle/' \
    --exclude='/src-tauri/gen/android/build/' \
    --exclude='/src-tauri/gen/android/app/build/' \
    "$REPO_ROOT/" "$dest/"

  echo "prepared $name workspace -> $dest"
}

acquire_parallel_lock() {
  local lock_dir="$1"
  mkdir -p "$(dirname "$lock_dir")"
  if mkdir "$lock_dir" 2>/dev/null; then
    printf '%s\n' "$$" >"$lock_dir/pid"
    return 0
  fi

  echo "error: another isolated parallel release appears to be running: $lock_dir" >&2
  if [[ -f "$lock_dir/pid" ]]; then
    echo "lock pid: $(cat "$lock_dir/pid" 2>/dev/null || true)" >&2
  fi
  return 2
}

run_parallel_worker() {
  local name="$1"
  local work_dir="$2"
  local target_dir="$3"
  local log_dir="$4"
  shift 4

  (
    set +e
    local started ended status
    started="$(date +%s)"
    echo "[$name] start $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "[$name] work_dir=$work_dir"
    echo "[$name] CARGO_TARGET_DIR=$target_dir"

    cd "$work_dir"
    status=$?
    if [[ "$status" == "0" ]]; then
      export CARGO_TARGET_DIR="$target_dir"
      export YUGUANJIA_STATIC_DIR="$work_dir/static"
      rm -rf "$YUGUANJIA_STATIC_DIR"
      if [[ "$name" == "android" ]]; then
        rm -rf \
          "$work_dir/src-tauri/gen/android/.gradle" \
          "$work_dir/src-tauri/gen/android/build" \
          "$work_dir/src-tauri/gen/android/app/build"
      fi
      # shellcheck disable=SC1091
      source "$work_dir/scripts/local_fast_release_env.sh"
      status=$?
    fi
    if [[ "$status" == "0" ]]; then
      "$@"
      status=$?
    fi

    ended="$(date +%s)"
    echo "[$name] end $(date -u '+%Y-%m-%dT%H:%M:%SZ') status=$status seconds=$((ended - started))"
    echo "$name status=$status seconds=$((ended - started))" >"$log_dir/$name.status"
    exit "$status"
  ) >"$log_dir/$name.log" 2>&1 &
}

run_parallel_all() {
  local run_id work_root target_root log_dir lock_dir started ended status
  run_id="$(date -u '+%Y%m%d-%H%M%S')"
  work_root="${YUGUANJIA_PARALLEL_WORK_ROOT:-$HOME/.cache/yuguanjia-build/parallel-workspaces/stable}"
  target_root="${YUGUANJIA_PARALLEL_TARGET_ROOT:-$HOME/.cache/yuguanjia-build/parallel-targets}"
  log_dir="${YUGUANJIA_PARALLEL_LOG_DIR:-/tmp/yuguanjia-parallel-isolated-$run_id}"
  lock_dir="${YUGUANJIA_PARALLEL_LOCK_DIR:-$HOME/.cache/yuguanjia-build/parallel-workspaces/.release.lock}"

  mkdir -p "$work_root" "$target_root" "$log_dir"
  acquire_parallel_lock "$lock_dir"
  YUGUANJIA_PARALLEL_ACTIVE_LOCK_DIR="$lock_dir"
  trap 'rm -rf "${YUGUANJIA_PARALLEL_ACTIVE_LOCK_DIR:-}"' EXIT

  echo
  echo "==> isolated parallel release"
  echo "run_id=$run_id"
  echo "work_root=$work_root"
  echo "target_root=$target_root"
  echo "log_dir=$log_dir"

  prepare_parallel_workspace mac "$work_root/mac"
  prepare_parallel_workspace win "$work_root/win"
  prepare_parallel_workspace android "$work_root/android"

  started="$(date +%s)"

  run_parallel_worker mac \
    "$work_root/mac" \
    "$target_root/mac" \
    "$log_dir" \
    "$work_root/mac/scripts/release_desktop.sh" mac-universal
  local mac_pid=$!

  run_parallel_worker win \
    "$work_root/win" \
    "$target_root/win" \
    "$log_dir" \
    "$work_root/win/scripts/release_desktop.sh" win
  local win_pid=$!

  run_parallel_worker android \
    "$work_root/android" \
    "$target_root/android" \
    "$log_dir" \
    "$work_root/android/scripts/release_android.sh" release
  local android_pid=$!

  status=0
  wait "$mac_pid" || status=1
  wait "$win_pid" || status=1
  wait "$android_pid" || status=1

  ended="$(date +%s)"

  echo
  echo "Parallel worker status:"
  cat "$log_dir"/*.status 2>/dev/null | sort || true
  echo "parallel-isolated status=$status seconds=$((ended - started))"
  echo "__PARALLEL_ISOLATED_STATUS=$status"
  echo "__PARALLEL_ISOLATED_SECONDS=$((ended - started))"

  echo
  echo "Artifacts:"
  {
    find "$target_root" "$work_root/android/src-tauri/gen/android/app/build/outputs" \
      \( -name '*.dmg' -o -name '*.app' -o -name '*.apk' \) \
      -print 2>/dev/null || true
    find "$target_root/win" -type f -path '*/bundle/nsis/*.exe' -print 2>/dev/null || true
  } | sort

  return "$status"
}

case "$target" in
  mac)
    run_step "macOS universal release" "$REPO_ROOT/scripts/release_desktop.sh" mac-universal
    ;;
  win)
    run_step "Windows NSIS release" "$REPO_ROOT/scripts/release_desktop.sh" win
    ;;
  android)
    run_step "Android release" "$REPO_ROOT/scripts/release_android.sh" release
    ;;
  desktop)
    run_step "macOS universal release" "$REPO_ROOT/scripts/release_desktop.sh" mac-universal
    run_step "Windows NSIS release" "$REPO_ROOT/scripts/release_desktop.sh" win
    ;;
  all)
    run_step "macOS universal release" "$REPO_ROOT/scripts/release_desktop.sh" mac-universal
    run_step "Windows NSIS release" "$REPO_ROOT/scripts/release_desktop.sh" win
    run_step "Android release" "$REPO_ROOT/scripts/release_android.sh" release
    ;;
  all-parallel|parallel)
    run_parallel_all
    ;;
esac

case "$target" in
  all-parallel|parallel) ;;
  *) show_artifacts ;;
esac
