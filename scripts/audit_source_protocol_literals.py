#!/usr/bin/env python3
"""Reject plaintext MTOP literals in production Rust runtime sources."""

from __future__ import annotations

import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCE_ROOTS = (ROOT / "src", ROOT / "src-tauri" / "src")
MTOP_LITERAL = re.compile(r'"mtop\.(?:taobao|alibaba|idle|wx)\.[^"\\]+"', re.I)
SEALED_PREFIX = re.compile(r"sealed_str!\s*\(\s*$", re.S)
PLAIN_ROUTE_LITERAL = re.compile(r"\.(?:route|nest)\(\s*\"/")


def production_rust_files() -> list[pathlib.Path]:
    files: list[pathlib.Path] = []
    for source_root in SOURCE_ROOTS:
        for path in source_root.rglob("*.rs"):
            if source_root.name == "src" and source_root.parent == ROOT and "bin" in path.relative_to(source_root).parts:
                continue
            files.append(path)
    return sorted(files)


def main() -> int:
    findings: list[tuple[str, pathlib.Path, int]] = []
    for path in production_rust_files():
        text = path.read_text(encoding="utf-8")
        for match in MTOP_LITERAL.finditer(text):
            prefix = text[max(0, match.start() - 160) : match.start()]
            if not SEALED_PREFIX.search(prefix):
                findings.append(
                    ("plaintext protocol literal", path, text.count("\n", 0, match.start()) + 1)
                )
        for match in PLAIN_ROUTE_LITERAL.finditer(text):
            findings.append(
                ("plaintext route registration", path, text.count("\n", 0, match.start()) + 1)
            )

    if findings:
        for description, path, line in findings:
            print(f"{description}: {path.relative_to(ROOT)}:{line}")
        return 1

    print("source protocol literal audit passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
