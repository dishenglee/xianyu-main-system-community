#!/usr/bin/env bash
#
# Package a signed macOS .app and UDIF .dmg from a macOS target binary on Linux.
#
# Tauri can cross-compile the macOS binary on Linux, but its native app/dmg
# bundlers are host-gated and ignored on Linux. This script fills that gap on
# the 424 packaging host by creating the app bundle layout, signing it with
# rcodesign, and writing a compressed UDIF dmg with tools/linux-udif-maker.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
TARGET_ROOT="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"

TARGET_ARG="${1:-mac-universal}"

case "$TARGET_ARG" in
  mac) TARGET_TRIPLE="aarch64-apple-darwin"; DMG_ARCH="aarch64" ;;
  mac-intel) TARGET_TRIPLE="x86_64-apple-darwin"; DMG_ARCH="x64" ;;
  mac-universal) TARGET_TRIPLE="universal-apple-darwin"; DMG_ARCH="universal" ;;
  *) echo "error: unsupported macOS target: $TARGET_ARG" >&2; exit 2 ;;
esac

if [[ "$(uname -s)" == "Darwin" ]]; then
  echo "error: this Linux fallback should not run on macOS" >&2
  exit 2
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found; source ~/.cargo/env before running this script" >&2
  exit 2
fi

if ! command -v rcodesign >/dev/null 2>&1; then
  echo "error: rcodesign not found. Install with: cargo install apple-codesign --version 0.29.0 --locked --no-default-features" >&2
  exit 2
fi

read_metadata() {
  python3 - <<'PY'
import json
import pathlib
import shlex

conf = json.loads(pathlib.Path("src-tauri/tauri.conf.json").read_text())
bundle = conf.get("bundle") or {}
macos = bundle.get("macOS") or {}

product = conf.get("productName") or "App"
version = conf.get("version") or "0.0.0"
identifier = conf.get("identifier") or "local.app"
category = bundle.get("category") or "Productivity"
copyright_text = bundle.get("copyright") or ""
minimum = macos.get("minimumSystemVersion") or "10.15"

print(f"PRODUCT_NAME={shlex.quote(product)}")
print(f"APP_VERSION={shlex.quote(version)}")
print(f"BUNDLE_IDENTIFIER={shlex.quote(identifier)}")
print(f"APP_CATEGORY={shlex.quote(category)}")
print(f"COPYRIGHT_TEXT={shlex.quote(copyright_text)}")
print(f"MINIMUM_SYSTEM_VERSION={shlex.quote(minimum)}")
PY
}

eval "$(read_metadata)"

BINARY_PATH="$TARGET_ROOT/$TARGET_TRIPLE/release/xianyu-tauri"
if [[ ! -x "$BINARY_PATH" ]]; then
  echo "error: macOS target binary not found or not executable: $BINARY_PATH" >&2
  exit 2
fi

ICON_PATH="$REPO_ROOT/src-tauri/icons/icon.icns"
if [[ ! -f "$ICON_PATH" ]]; then
  echo "error: icon not found: $ICON_PATH" >&2
  exit 2
fi

BUNDLE_ROOT="$TARGET_ROOT/$TARGET_TRIPLE/release/bundle"
APP_PATH="$BUNDLE_ROOT/macos/$PRODUCT_NAME.app"
DMG_DIR="$BUNDLE_ROOT/dmg"
DMG_PATH="$DMG_DIR/${PRODUCT_NAME}_${APP_VERSION}_${DMG_ARCH}.dmg"
STAGE_DIR="$DMG_DIR/linux-stage"
VOLUME_LABEL="${DMG_VOLUME_LABEL_ASCII:-YUGUANJIA}"

rm -rf "$APP_PATH" "$STAGE_DIR"
mkdir -p "$APP_PATH/Contents/MacOS" "$APP_PATH/Contents/Resources" "$DMG_DIR" "$STAGE_DIR"

cp "$BINARY_PATH" "$APP_PATH/Contents/MacOS/xianyu-tauri"
chmod +x "$APP_PATH/Contents/MacOS/xianyu-tauri"
cp "$ICON_PATH" "$APP_PATH/Contents/Resources/icon.icns"

export PRODUCT_NAME APP_VERSION BUNDLE_IDENTIFIER APP_CATEGORY COPYRIGHT_TEXT
export TARGET_TRIPLE
export APP_PATH
export MINIMUM_SYSTEM_VERSION="${MACOSX_DEPLOYMENT_TARGET:-$MINIMUM_SYSTEM_VERSION}"
python3 - <<'PY'
import os
import pathlib
import plistlib

app_path = pathlib.Path(os.environ["APP_PATH"])
plist_path = app_path / "Contents" / "Info.plist"
data = {
    "CFBundleDevelopmentRegion": "English",
    "CFBundleDisplayName": os.environ["PRODUCT_NAME"],
    "CFBundleExecutable": "xianyu-tauri",
    "CFBundleIconFile": "icon.icns",
    "CFBundleIdentifier": os.environ["BUNDLE_IDENTIFIER"],
    "CFBundleInfoDictionaryVersion": "6.0",
    "CFBundleName": os.environ["PRODUCT_NAME"],
    "CFBundlePackageType": "APPL",
    "CFBundleShortVersionString": os.environ["APP_VERSION"],
    "CFBundleVersion": os.environ["APP_VERSION"],
    "CSResourcesFileMapped": True,
    "LSApplicationCategoryType": f"public.app-category.{os.environ['APP_CATEGORY'].lower()}",
    "LSMinimumSystemVersion": os.environ["MINIMUM_SYSTEM_VERSION"],
    "NSHighResolutionCapable": True,
    "NSHumanReadableCopyright": os.environ["COPYRIGHT_TEXT"],
}
plist_path.write_bytes(plistlib.dumps(data, fmt=plistlib.FMT_XML, sort_keys=False))
(app_path / "Contents" / "PkgInfo").write_text("APPL????", encoding="ascii")
PY

sign_with_rcodesign() {
  local path="$1"
  local args=(sign)

  if [[ -n "${MACOS_CODESIGN_P12_FILE:-}" ]]; then
    args+=(--p12-file "$MACOS_CODESIGN_P12_FILE")
    if [[ -n "${MACOS_CODESIGN_P12_PASSWORD_FILE:-}" ]]; then
      args+=(--p12-password-file "$MACOS_CODESIGN_P12_PASSWORD_FILE")
    fi
    if [[ -n "${MACOS_CODESIGN_FLAGS:-}" ]]; then
      args+=(--code-signature-flags "$MACOS_CODESIGN_FLAGS")
    fi
  fi

  args+=("$path")
  rcodesign "${args[@]}"
}

echo "→ signing macOS app with rcodesign"
sign_with_rcodesign "$APP_PATH"

cp -R "$APP_PATH" "$STAGE_DIR/$PRODUCT_NAME.app"
ln -s /Applications "$STAGE_DIR/Applications"

rm -f "$DMG_PATH"
echo "→ creating Linux UDIF dmg"
cargo run --release --manifest-path "$REPO_ROOT/tools/linux-udif-maker/Cargo.toml" -- \
  "$STAGE_DIR" \
  "$DMG_PATH" \
  "$VOLUME_LABEL"

echo "→ signing dmg with rcodesign"
sign_with_rcodesign "$DMG_PATH"

rm -rf "$STAGE_DIR"

echo
echo "✓ Linux macOS dmg packaging complete"
echo "  app = $APP_PATH"
echo "  dmg = $DMG_PATH"
