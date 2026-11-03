#!/usr/bin/env bash
#
# Local release environment bootstrap.
#
# This script is intentionally sourceable:
#   source ./scripts/local_fast_release_env.sh
#
# It does not build anything. It only exports the machine-local paths that make
# repeated local release builds reuse the warm caches already present on this machine.

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  set -euo pipefail
  _YUGUANJIA_FAST_ENV_DIRECT=1
else
  _YUGUANJIA_FAST_ENV_DIRECT=0
fi

_yuguanjia_fast_env_fail() {
  echo "error: $*" >&2
  if [[ "$_YUGUANJIA_FAST_ENV_DIRECT" == "1" ]]; then
    exit 2
  fi
  return 2
}

_yuguanjia_fast_env_prepend_path() {
  local dir="$1"
  [[ -d "$dir" ]] || return 0
  case ":$PATH:" in
    *":$dir:"*) ;;
    *) export PATH="$dir:$PATH" ;;
  esac
}

_yuguanjia_fast_env_latest_child_dir() {
  local parent="$1"
  [[ -d "$parent" ]] || return 1
  find "$parent" -maxdepth 1 -mindepth 1 -type d | sort | tail -n 1
}

_yuguanjia_fast_env_first_existing_file() {
  local candidate
  for candidate in "$@"; do
    if [[ -e "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

_yuguanjia_fast_env_main() {
  local script_dir repo_root
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  repo_root="$(cd "$script_dir/.." && pwd)"

  export YUGUANJIA_REPO_ROOT="$repo_root"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo_root/target}"

  local android_sdk_candidates=(
    "${ANDROID_HOME:-}"
    "${ANDROID_SDK_ROOT:-}"
    "$HOME/Library/Android/sdk"
    "$HOME/android-sdk"
    "$HOME/Android/Sdk"
    "/opt/android-sdk"
    "/opt/homebrew/share/android-commandlinetools"
  )
  local sdk=""
  for candidate in "${android_sdk_candidates[@]}"; do
    if [[ -n "$candidate" && -d "$candidate/platform-tools" && -d "$candidate/platforms" ]]; then
      sdk="$candidate"
      break
    fi
  done
  if [[ -n "$sdk" ]]; then
    export ANDROID_HOME="$sdk"
    export ANDROID_SDK_ROOT="$sdk"
    _yuguanjia_fast_env_prepend_path "$sdk/platform-tools"
    _yuguanjia_fast_env_prepend_path "$sdk/cmdline-tools/latest/bin"
    local build_tools
    build_tools="$(_yuguanjia_fast_env_latest_child_dir "$sdk/build-tools" || true)"
    if [[ -n "$build_tools" ]]; then
      _yuguanjia_fast_env_prepend_path "$build_tools"
    fi

    if [[ -z "${ANDROID_NDK_HOME:-}${NDK_HOME:-}" ]]; then
      local ndk
      ndk="$(_yuguanjia_fast_env_latest_child_dir "$sdk/ndk" || true)"
      if [[ -n "$ndk" ]]; then
        export ANDROID_NDK_HOME="$ndk"
        export NDK_HOME="$ndk"
      fi
    elif [[ -n "${ANDROID_NDK_HOME:-}" ]]; then
      export NDK_HOME="${NDK_HOME:-$ANDROID_NDK_HOME}"
    else
      export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$NDK_HOME}"
    fi
  fi

  if [[ -z "${JAVA_HOME:-}" ]] && command -v /usr/libexec/java_home >/dev/null 2>&1; then
    local java_home
    java_home="$(/usr/libexec/java_home -v 17 2>/dev/null || /usr/libexec/java_home 2>/dev/null || true)"
    if [[ -n "$java_home" ]]; then
      export JAVA_HOME="$java_home"
      _yuguanjia_fast_env_prepend_path "$JAVA_HOME/bin"
    fi
  fi

  if [[ -z "${JAVA_HOME:-}" && -d /usr/lib/jvm/java-17-openjdk-arm64 ]]; then
    export JAVA_HOME=/usr/lib/jvm/java-17-openjdk-arm64
    _yuguanjia_fast_env_prepend_path "$JAVA_HOME/bin"
  fi

  local osxcross_root="${OSXCROSS_ROOT:-$HOME/osxcross}"
  local osxcross_bin="$osxcross_root/target/bin"
  if [[ -d "$osxcross_bin" ]]; then
    export OSXCROSS_ROOT="$osxcross_root"
    _yuguanjia_fast_env_prepend_path "$osxcross_bin"
    export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-13.0}"

    local osxcross_sdk=""
    osxcross_sdk="$(_yuguanjia_fast_env_first_existing_file \
      "${OSXCROSS_SDKROOT:-}" \
      "$osxcross_root/target/SDK/MacOSX13.1.sdk" \
      "$osxcross_root/target/SDK/MacOSX15.4.sdk" \
      "$osxcross_root/target/SDK/MacOSX.sdk" || true)"
    if [[ -n "$osxcross_sdk" ]]; then
      export OSXCROSS_SDKROOT="$osxcross_sdk"
      export SDKROOT="$osxcross_sdk"
    fi

    export CC_aarch64_apple_darwin="${CC_aarch64_apple_darwin:-aarch64-apple-darwin24.4-clang}"
    export CXX_aarch64_apple_darwin="${CXX_aarch64_apple_darwin:-aarch64-apple-darwin24.4-clang++}"
    export AR_aarch64_apple_darwin="${AR_aarch64_apple_darwin:-aarch64-apple-darwin24.4-ar}"
    export CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER="${CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER:-aarch64-apple-darwin24.4-clang}"

    export CC_x86_64_apple_darwin="${CC_x86_64_apple_darwin:-x86_64-apple-darwin24.4-clang}"
    export CXX_x86_64_apple_darwin="${CXX_x86_64_apple_darwin:-x86_64-apple-darwin24.4-clang++}"
    export AR_x86_64_apple_darwin="${AR_x86_64_apple_darwin:-x86_64-apple-darwin24.4-ar}"
    export CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER="${CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER:-x86_64-apple-darwin24.4-clang}"
  fi

  export ANDROID_TAURI_TARGETS="${ANDROID_TAURI_TARGETS:-aarch64}"
  if [[ "${1:-}" == "--check" || "$_YUGUANJIA_FAST_ENV_DIRECT" == "1" ]]; then
    echo "YUGUANJIA_REPO_ROOT=$YUGUANJIA_REPO_ROOT"
    echo "CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
    echo "ANDROID_HOME=${ANDROID_HOME:-}"
    echo "ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-}"
    echo "JAVA_HOME=${JAVA_HOME:-}"
    echo "ANDROID_TAURI_TARGETS=$ANDROID_TAURI_TARGETS"
    echo "apksigner=$(command -v apksigner || true)"
    echo "zipalign=$(command -v zipalign || true)"
    echo "OSXCROSS_ROOT=${OSXCROSS_ROOT:-}"
    echo "OSXCROSS_SDKROOT=${OSXCROSS_SDKROOT:-}"
    echo "MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-}"
    echo "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=${CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER:-}"
    echo "CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=${CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER:-}"
    echo "cargo-tauri=$(command -v cargo-tauri || true)"
    echo "cargo-xwin=$(command -v cargo-xwin || true)"
    echo "makensis=$(command -v makensis || true)"
    echo "osslsigncode=$(command -v osslsigncode || true)"
    echo "sdkmanager=$(command -v sdkmanager || true)"
  fi
}

_yuguanjia_fast_env_main "${1:-}"
