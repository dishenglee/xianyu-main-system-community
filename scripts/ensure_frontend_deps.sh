#!/usr/bin/env bash
#
# Keep frontend-v2/node_modules in sync with package.json/package-lock.json.
# Release workers reuse stable workspaces, so a new dependency can be present in
# the lockfile while the cached node_modules tree is still missing it.

set -euo pipefail

FRONTEND_DIR="${1:-frontend-v2}"

if [[ ! -d "$FRONTEND_DIR" ]]; then
  echo "error: frontend directory not found: $FRONTEND_DIR" >&2
  exit 2
fi

manifest_hash="$(
  python3 - "$FRONTEND_DIR" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
digest = hashlib.sha256()
for name in ("package.json", "package-lock.json"):
    path = root / name
    if path.exists():
        digest.update(name.encode("utf-8"))
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
print(digest.hexdigest())
PY
)"

marker="$FRONTEND_DIR/node_modules/.yuguanjia-deps.sha256"

if [[ -d "$FRONTEND_DIR/node_modules" && -f "$marker" && "$(cat "$marker" 2>/dev/null || true)" == "$manifest_hash" ]]; then
  echo "→ frontend dependencies already match package lock"
  exit 0
fi

echo "→ installing frontend dependencies"
if [[ -f "$FRONTEND_DIR/package-lock.json" ]]; then
  npm --prefix "$FRONTEND_DIR" ci --prefer-offline --no-audit --no-fund
else
  npm --prefix "$FRONTEND_DIR" install --prefer-offline --no-audit --no-fund
fi

mkdir -p "$(dirname "$marker")"
printf '%s\n' "$manifest_hash" >"$marker"
