#!/usr/bin/env bash
#
# Release build for the 鱼管家 Tauri desktop app.
#
# What this does:
#   1. Runs `cargo tauri build` with the right target(s).
#      - On non-Windows hosts, Windows release packaging uses `cargo-xwin` +
#        `makensis` and emits an NSIS installer without requiring a Windows host.
#   3. Ad-hoc signs the macOS bundle. We intentionally do NOT notarize —
#      there is no Apple Developer account. End-users first-run Gatekeeper
#      will show "unknown developer"; the README documents
#      `xattr -dr com.apple.quarantine <bundle>` as the escape hatch.
#
# Usage:
#   ./scripts/release_desktop.sh                  # current host target
#   ./scripts/release_desktop.sh mac              # Apple Silicon dmg
#   ./scripts/release_desktop.sh mac-intel        # Intel dmg
#   ./scripts/release_desktop.sh win              # Windows NSIS installer
#
# Env overrides:
#   WINDOWS_CODESIGN_PFX_FILE
#                       — optional Authenticode PFX/P12 certificate for Windows
#                         signing. If absent on non-Windows hosts, a persistent
#                         self-signed test certificate is generated under
#                         ~/.config/yuguanjia-build.
#   WINDOWS_CODESIGN_PFX_PASSWORD
#   WINDOWS_CODESIGN_PFX_PASSWORD_FILE
#                       — password or password file for the Windows signing cert.
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
TARGET_ROOT="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"

"$REPO_ROOT/scripts/check_release_version_consistency.py"

TARGET_ARG="${1:-host}"

# -----------------------------------------------------------------------------
# 1. Validate release version
# -----------------------------------------------------------------------------
ensure_frontend_dependencies() {
  local frontend_dir="${YUGUANJIA_FRONTEND_DIR:-$REPO_ROOT/frontend-v2}"
  "$REPO_ROOT/scripts/ensure_frontend_deps.sh" "$frontend_dir"
}

check_tauri_linux_tray_dependency() {
  [[ "$(uname -s)" == "Linux" ]] || return 0

  if pkg-config --exists ayatana-appindicator3-0.1 2>/dev/null; then
    return 0
  fi
  if pkg-config --exists appindicator3-0.1 2>/dev/null; then
    return 0
  fi

  cat >&2 <<'EOF'
error: missing appindicator development package for Tauri tray-icon bundling on Linux hosts.
hint: install it with: sudo apt-get install -y libayatana-appindicator3-dev
EOF
  exit 2
}

windows_sign_file() {
  local file="$1"
  [[ -f "$file" ]] || return 0

  echo "→ Authenticode signing $file"
  "$REPO_ROOT/scripts/windows_osslsigncode.sh" "$file"
  verify_windows_signature "$file"
}

source_windows_codesign_env() {
  local sign_dir="${WINDOWS_CODESIGN_DIR:-$HOME/.config/yuguanjia-build/windows-codesign}"
  local env_file="${WINDOWS_CODESIGN_ENV_FILE:-$sign_dir/windows-codesign.env}"
  if [[ -f "$env_file" ]]; then
    # shellcheck disable=SC1090
    source "$env_file"
  fi
}

verify_windows_signature() {
  local file="$1"
  [[ -f "$file" ]] || return 0

  source_windows_codesign_env
  if [[ -n "${WINDOWS_CODESIGN_CERT_FILE:-}" && -f "$WINDOWS_CODESIGN_CERT_FILE" ]]; then
    osslsigncode verify -CAfile "$WINDOWS_CODESIGN_CERT_FILE" -in "$file" || true
  else
    osslsigncode verify -in "$file" || true
  fi
}

sign_windows_artifacts() {
  local target_dir="$TARGET_ROOT/x86_64-pc-windows-msvc/release"
  local app_exe="$target_dir/xianyu-tauri.exe"
  windows_sign_file "$app_exe"

  local installer
  while IFS= read -r installer; do
    windows_sign_file "$installer"
  done < <(find "$target_dir/bundle/nsis" -maxdepth 1 -type f -name '*.exe' 2>/dev/null | sort || true)
}

# -----------------------------------------------------------------------------
# 2. Select Tauri target(s)
# -----------------------------------------------------------------------------
TAURI_ARGS=()
case "$TARGET_ARG" in
  host) ;;
  mac)        TAURI_ARGS=(--target aarch64-apple-darwin) ;;
  mac-intel)  TAURI_ARGS=(--target x86_64-apple-darwin) ;;
  mac-universal) TAURI_ARGS=(--target universal-apple-darwin) ;;
  win)        TAURI_ARGS=(--target x86_64-pc-windows-msvc) ;;
  *) echo "unknown target: $TARGET_ARG" >&2; exit 2 ;;
esac

# -----------------------------------------------------------------------------
# 3. Build
# -----------------------------------------------------------------------------
if ! command -v tauri &>/dev/null; then
  if command -v cargo-tauri &>/dev/null; then
    TAURI_BIN=(cargo tauri)
  elif [[ -x "$HOME/.cargo/bin/cargo-tauri" ]]; then
    TAURI_BIN=(cargo tauri)
  else
    echo "→ installing tauri-cli (first run only)"
    cargo install tauri-cli --version '^2' --locked
    TAURI_BIN=(cargo tauri)
  fi
else
  TAURI_BIN=(tauri)
fi

echo "→ cargo tauri build ${TAURI_ARGS[*]:-}"
ensure_frontend_dependencies
make_tauri_config() {
  local include_windows_bundle="${1:-0}"
  local skip_before_build="${2:-0}"
  python3 - "$include_windows_bundle" "$skip_before_build" "$REPO_ROOT" <<'PY'
import json
import os
import sys

include_windows_bundle = sys.argv[1] == "1"
skip_before_build = sys.argv[2] == "1"
repo_root = sys.argv[3]
frontend_dir = os.environ.get("YUGUANJIA_FRONTEND_DIR") or os.path.join(repo_root, "frontend-v2")
static_dir = (
    os.environ.get("YUGUANJIA_STATIC_DIR")
    or os.environ.get("RELEASE_FRONTEND_DIST_DIR")
    or os.path.join(repo_root, "static")
)

config = {}
default_frontend = os.path.join(repo_root, "frontend-v2")
default_static = os.path.join(repo_root, "static")
if skip_before_build:
    config["build"] = {
        "beforeBuildCommand": None,
        "frontendDist": static_dir,
    }
elif frontend_dir != default_frontend or static_dir != default_static:
    config["build"] = {
        "beforeBuildCommand": f"npm --prefix {frontend_dir} run build",
        "frontendDist": static_dir,
    }

if include_windows_bundle:
    config["bundle"] = {
        "targets": ["nsis"],
        "windows": {
            "signCommand": {
                "cmd": os.path.join(repo_root, "scripts", "windows_osslsigncode.sh"),
                "args": ["%1"],
            }
        },
    }

if config:
    print(json.dumps(config, ensure_ascii=False, separators=(",", ":")))
PY
}

TAURI_CONFIG_OVERRIDE="$(make_tauri_config 0)"
WINDOWS_BUNDLE_CONFIG="$(make_tauri_config 1)"
HOST_UNAME="$(uname -s)"
case "$HOST_UNAME" in
  MINGW*|MSYS*|CYGWIN*) IS_WINDOWS_HOST=1 ;;
  *) IS_WINDOWS_HOST=0 ;;
esac

build_macos_universal_parallel_linux() {
  local frontend_dir static_dir config log_dir lipo_bin
  local slice_root arm_target_root intel_target_root
  local arm_log intel_log arm_status intel_status
  local arm_bin intel_bin universal_dir universal_bin

  frontend_dir="${YUGUANJIA_FRONTEND_DIR:-$REPO_ROOT/frontend-v2}"
  static_dir="${YUGUANJIA_STATIC_DIR:-${RELEASE_FRONTEND_DIST_DIR:-$REPO_ROOT/static}}"
  log_dir="${TARGET_ROOT}/mac-universal-parallel-logs"
  slice_root="${TARGET_ROOT}/mac-universal-slices"
  arm_target_root="$slice_root/aarch64"
  intel_target_root="$slice_root/x86_64"
  lipo_bin="${OSXCROSS_LIPO:-x86_64-apple-darwin24.4-lipo}"

  if ! command -v "$lipo_bin" >/dev/null 2>&1; then
    echo "error: Apple lipo not found: $lipo_bin" >&2
    exit 2
  fi

  echo "→ building frontend once for macOS universal"
  mkdir -p "$static_dir" "$log_dir" "$arm_target_root" "$intel_target_root"
  YUGUANJIA_STATIC_DIR="$static_dir" npm --prefix "$frontend_dir" run build

  config="$(YUGUANJIA_STATIC_DIR="$static_dir" make_tauri_config 0 1)"
  arm_log="$log_dir/aarch64-apple-darwin.log"
  intel_log="$log_dir/x86_64-apple-darwin.log"

  echo "→ building macOS universal slices in parallel"
  (
    export CARGO_TARGET_DIR="$arm_target_root"
    "${TAURI_BIN[@]}" build \
      --target aarch64-apple-darwin \
      --no-bundle \
      -c "$config"
  ) >"$arm_log" 2>&1 &
  local arm_pid=$!

  (
    export CARGO_TARGET_DIR="$intel_target_root"
    "${TAURI_BIN[@]}" build \
      --target x86_64-apple-darwin \
      --no-bundle \
      -c "$config"
  ) >"$intel_log" 2>&1 &
  local intel_pid=$!

  arm_status=0
  intel_status=0
  wait "$arm_pid" || arm_status=$?
  wait "$intel_pid" || intel_status=$?

  echo "---- macOS aarch64 build log ----"
  cat "$arm_log"
  echo "---- macOS x86_64 build log ----"
  cat "$intel_log"

  if [[ "$arm_status" != "0" || "$intel_status" != "0" ]]; then
    echo "error: macOS universal slice build failed: aarch64=$arm_status x86_64=$intel_status" >&2
    exit 1
  fi

  arm_bin="$arm_target_root/aarch64-apple-darwin/release/xianyu-tauri"
  intel_bin="$intel_target_root/x86_64-apple-darwin/release/xianyu-tauri"
  universal_dir="$TARGET_ROOT/universal-apple-darwin/release"
  universal_bin="$universal_dir/xianyu-tauri"
  if [[ ! -x "$arm_bin" || ! -x "$intel_bin" ]]; then
    echo "error: expected macOS slice binaries were not produced" >&2
    echo "  aarch64: $arm_bin" >&2
    echo "  x86_64: $intel_bin" >&2
    exit 1
  fi

  mkdir -p "$universal_dir"
  echo "→ lipo macOS slices into universal binary"
  "$lipo_bin" -create "$arm_bin" "$intel_bin" -output "$universal_bin"
  chmod +x "$universal_bin"
}

if [[ "$TARGET_ARG" == "win" && "$IS_WINDOWS_HOST" == "0" ]]; then
  if ! command -v cargo-xwin &>/dev/null; then
    echo "error: cargo-xwin not found. Install with: cargo install cargo-xwin" >&2
    exit 2
  fi
  if ! command -v makensis &>/dev/null; then
    echo "error: makensis not found. Install with: brew install makensis" >&2
    exit 2
  fi
  if [[ -d /opt/homebrew/opt/llvm/bin ]]; then
    export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
  fi
  check_tauri_linux_tray_dependency
  echo "→ using Windows cross-build pipeline: cargo-xwin + NSIS"
  "${TAURI_BIN[@]}" build \
    --runner cargo-xwin \
    --target x86_64-pc-windows-msvc \
    -c "$WINDOWS_BUNDLE_CONFIG"
  windows_sign_file "$TARGET_ROOT/x86_64-pc-windows-msvc/release/xianyu-tauri.exe"
elif [[ "$TARGET_ARG" == "win" ]]; then
  echo "→ using Windows host pipeline: NSIS only"
  "${TAURI_BIN[@]}" build \
    --target x86_64-pc-windows-msvc \
    --no-sign \
    -c "$WINDOWS_BUNDLE_CONFIG"
  sign_windows_artifacts
elif [[ "$TARGET_ARG" == "mac-universal" && "$HOST_UNAME" != "Darwin" ]]; then
  build_macos_universal_parallel_linux
else
  BUILD_ARGS=("${TAURI_ARGS[@]}")
  if [[ "$HOST_UNAME" != "Darwin" ]]; then
    case "$TARGET_ARG" in
      mac|mac-intel|mac-universal)
        BUILD_ARGS+=(--no-bundle)
        ;;
    esac
  fi
  "${TAURI_BIN[@]}" build "${BUILD_ARGS[@]}" \
    ${TAURI_CONFIG_OVERRIDE:+"-c"} ${TAURI_CONFIG_OVERRIDE:+"$TAURI_CONFIG_OVERRIDE"}
fi

if [[ "$HOST_UNAME" != "Darwin" ]]; then
  case "$TARGET_ARG" in
    mac|mac-intel|mac-universal)
      echo "→ packaging macOS app/dmg on Linux"
      "$REPO_ROOT/scripts/package_macos_dmg_linux.sh" "$TARGET_ARG"
      ;;
  esac
fi

# -----------------------------------------------------------------------------
# 4. Ad-hoc sign macOS .app so Gatekeeper shows "unidentified developer" once
#    instead of "damaged and can't be opened".
# -----------------------------------------------------------------------------
if [[ "$(uname -s)" == "Darwin" ]]; then
  BUNDLE_DIR="$TARGET_ROOT"
  APP_PATHS=()
  while IFS= read -r p; do APP_PATHS+=("$p"); done < <(
    find "$BUNDLE_DIR" -type d -name '鱼管家.app' -path '*/bundle/*' 2>/dev/null || true
  )
  for app in "${APP_PATHS[@]:-}"; do
    [[ -z "$app" ]] && continue
    echo "→ ad-hoc codesigning $app"
    codesign --force --deep --sign - "$app"
  done
fi

echo
echo "✓ release build complete"
echo "  Artifacts are under target/.../bundle/"
