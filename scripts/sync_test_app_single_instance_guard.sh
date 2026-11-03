#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FILES=(
  "scripts/test_app_single_instance_guard.sh"
  "scripts/start_test_app.sh"
  "scripts/start_test_app_internal.sh"
  "AGENTS.md"
)

git -C "$REPO_ROOT" worktree list --porcelain | awk '/^worktree / { print substr($0, 10) }' | while IFS= read -r worktree_path; do
  [[ -n "$worktree_path" ]] || continue
  [[ "$worktree_path" != "$REPO_ROOT" ]] || continue
  for rel in "${FILES[@]}"; do
    src="$REPO_ROOT/$rel"
    dst="$worktree_path/$rel"
    [[ -f "$src" ]] || continue
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
  done
  echo "synced: $worktree_path"
done
