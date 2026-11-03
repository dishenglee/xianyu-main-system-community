#!/usr/bin/env python3
"""Fail release packaging when the repository version sources disagree."""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys


def cargo_version(path: pathlib.Path) -> str:
    match = re.search(
        r'^version\s*=\s*"([^"]+)"',
        path.read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if not match:
        raise ValueError(f"failed to read package version from {path}")
    return match.group(1)


def latest_known_version(path: pathlib.Path) -> str:
    versions = [
        line.strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]
    if not versions:
        raise ValueError(f"no released versions found in {path}")
    return versions[-1]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--repo-root",
        type=pathlib.Path,
        default=pathlib.Path(__file__).resolve().parent.parent,
    )
    args = parser.parse_args()
    root = args.repo_root.resolve()

    sources = {
        "Cargo.toml": cargo_version(root / "Cargo.toml"),
        "src-tauri/Cargo.toml": cargo_version(root / "src-tauri/Cargo.toml"),
        "src-tauri/tauri.conf.json": str(
            json.loads(
                (root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8")
            ).get("version", "")
        ),
        "docs/releases/known_versions.txt (latest)": latest_known_version(
            root / "docs/releases/known_versions.txt"
        ),
    }

    expected = sources["Cargo.toml"]
    mismatches = {name: value for name, value in sources.items() if value != expected}
    if mismatches:
        print("error: release version sources are inconsistent", file=sys.stderr)
        for name, value in sources.items():
            marker = " !=" if name in mismatches else " =="
            print(f"  {name}: {value}{marker} {expected}", file=sys.stderr)
        return 2

    print(f"ok: release version sources agree on {expected}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"error: release version consistency check failed: {exc}", file=sys.stderr)
        raise SystemExit(2) from exc
