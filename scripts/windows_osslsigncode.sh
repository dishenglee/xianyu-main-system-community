#!/usr/bin/env bash
#
# Tauri Windows signCommand helper.
#
# The Windows bundler calls this for the app exe, NSIS helper DLLs, temporary
# installer image, and final NSIS installer. Keep it standalone because Tauri
# executes signCommand outside the release_desktop.sh shell process.

set -euo pipefail

file="${1:-}"
if [[ -z "$file" ]]; then
  echo "error: missing file to sign" >&2
  exit 2
fi
if [[ ! -f "$file" ]]; then
  echo "error: file not found: $file" >&2
  exit 2
fi

if ! command -v osslsigncode &>/dev/null; then
  echo "error: osslsigncode not found. Install with: sudo apt-get install osslsigncode" >&2
  exit 2
fi
if ! command -v openssl &>/dev/null; then
  echo "error: openssl not found; required for the fallback Windows signing certificate" >&2
  exit 2
fi

sign_dir="${WINDOWS_CODESIGN_DIR:-$HOME/.config/yuguanjia-build/windows-codesign}"
env_file="${WINDOWS_CODESIGN_ENV_FILE:-$sign_dir/windows-codesign.env}"
mkdir -p "$sign_dir"

if [[ -f "$env_file" ]]; then
  # shellcheck disable=SC1090
  source "$env_file"
fi

WINDOWS_CODESIGN_PFX_FILE="${WINDOWS_CODESIGN_PFX_FILE:-${WIN_CODESIGN_PFX_FILE:-}}"
WINDOWS_CODESIGN_PFX_PASSWORD="${WINDOWS_CODESIGN_PFX_PASSWORD:-${WIN_CODESIGN_PFX_PASSWORD:-}}"
WINDOWS_CODESIGN_PFX_PASSWORD_FILE="${WINDOWS_CODESIGN_PFX_PASSWORD_FILE:-${WIN_CODESIGN_PFX_PASSWORD_FILE:-}}"

default_pfx="$sign_dir/yuguanjia-windows-selfsigned.pfx"
default_cert="$sign_dir/yuguanjia-windows-selfsigned.crt"
if [[ -z "$WINDOWS_CODESIGN_PFX_FILE" ]]; then
  WINDOWS_CODESIGN_PFX_FILE="$default_pfx"
  WINDOWS_CODESIGN_PFX_PASSWORD_FILE="$sign_dir/yuguanjia-windows-selfsigned.pass"
  WINDOWS_CODESIGN_CERT_FILE="${WINDOWS_CODESIGN_CERT_FILE:-$default_cert}"
fi
WINDOWS_CODESIGN_CERT_FILE="${WINDOWS_CODESIGN_CERT_FILE:-${WIN_CODESIGN_CERT_FILE:-}}"
if [[ "$WINDOWS_CODESIGN_PFX_FILE" == "$default_pfx" && -z "$WINDOWS_CODESIGN_CERT_FILE" ]]; then
  WINDOWS_CODESIGN_CERT_FILE="$default_cert"
fi

if [[ -n "$WINDOWS_CODESIGN_PFX_PASSWORD_FILE" && -f "$WINDOWS_CODESIGN_PFX_PASSWORD_FILE" ]]; then
  WINDOWS_CODESIGN_PFX_PASSWORD="$(<"$WINDOWS_CODESIGN_PFX_PASSWORD_FILE")"
fi

if [[ ! -f "$WINDOWS_CODESIGN_PFX_FILE" ]]; then
  if [[ -z "$WINDOWS_CODESIGN_PFX_PASSWORD" ]]; then
    WINDOWS_CODESIGN_PFX_PASSWORD="$(openssl rand -hex 18)"
  fi
  if [[ -z "$WINDOWS_CODESIGN_PFX_PASSWORD_FILE" ]]; then
    WINDOWS_CODESIGN_PFX_PASSWORD_FILE="$sign_dir/yuguanjia-windows-codesign.pass"
  fi
  printf '%s' "$WINDOWS_CODESIGN_PFX_PASSWORD" >"$WINDOWS_CODESIGN_PFX_PASSWORD_FILE"
  chmod 600 "$WINDOWS_CODESIGN_PFX_PASSWORD_FILE"

  key_file="$sign_dir/yuguanjia-windows-selfsigned.key"
  cert_file="${WINDOWS_CODESIGN_CERT_FILE:-$default_cert}"
  echo "generating Windows self-signed Authenticode certificate at $WINDOWS_CODESIGN_PFX_FILE"
  openssl req \
    -x509 \
    -newkey rsa:3072 \
    -keyout "$key_file" \
    -out "$cert_file" \
    -sha256 \
    -days 3650 \
    -nodes \
    -subj "/CN=Yuguanjia Windows Code Signing/O=Disheng/C=CN" \
    -addext "keyUsage=digitalSignature" \
    -addext "extendedKeyUsage=codeSigning" >/dev/null 2>&1
  openssl pkcs12 \
    -export \
    -out "$WINDOWS_CODESIGN_PFX_FILE" \
    -inkey "$key_file" \
    -in "$cert_file" \
    -name "Yuguanjia Windows Code Signing" \
    -passout "pass:$WINDOWS_CODESIGN_PFX_PASSWORD" >/dev/null 2>&1
  chmod 600 "$WINDOWS_CODESIGN_PFX_FILE"
fi

if [[ -z "$WINDOWS_CODESIGN_PFX_PASSWORD" ]]; then
  echo "error: missing WINDOWS_CODESIGN_PFX_PASSWORD or WINDOWS_CODESIGN_PFX_PASSWORD_FILE for $WINDOWS_CODESIGN_PFX_FILE" >&2
  exit 2
fi

cat >"$env_file" <<EOF
export WINDOWS_CODESIGN_PFX_FILE='$WINDOWS_CODESIGN_PFX_FILE'
export WINDOWS_CODESIGN_PFX_PASSWORD_FILE='${WINDOWS_CODESIGN_PFX_PASSWORD_FILE:-}'
export WINDOWS_CODESIGN_CERT_FILE='${WINDOWS_CODESIGN_CERT_FILE:-}'
EOF
chmod 600 "$env_file"

signed_file="$file.signed"
args=(
  sign
  -pkcs12 "$WINDOWS_CODESIGN_PFX_FILE"
  -pass "$WINDOWS_CODESIGN_PFX_PASSWORD"
  -h sha256
  -n "鱼管家"
  # Keep the Authenticode issuer URL neutral. The protected control-plane
  # domains must not be copied into distributable certificate metadata.
  -i "${WINDOWS_CODESIGN_APP_URL:-https://yuguanjia.invalid}"
)
if [[ -n "${WINDOWS_CODESIGN_TIMESTAMP_URL:-}" ]]; then
  args+=(-t "$WINDOWS_CODESIGN_TIMESTAMP_URL")
fi
args+=(-in "$file" -out "$signed_file")

rm -f "$signed_file"
osslsigncode "${args[@]}"
mv "$signed_file" "$file"
