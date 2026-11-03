#!/usr/bin/env python3
"""Fail when distributable artifacts expose protected upstream protocol details."""

from __future__ import annotations

import argparse
import contextlib
import dataclasses
import json
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from collections.abc import Iterable, Iterator
from typing import BinaryIO


CHUNK_SIZE = 1024 * 1024
OVERLAP_SIZE = 1024
MAX_ARCHIVE_MEMBER_SIZE = 512 * 1024 * 1024


@dataclasses.dataclass(frozen=True)
class Rule:
    rule_id: str
    description: str
    ascii_pattern: re.Pattern[bytes]
    utf16le_pattern: re.Pattern[bytes]


def _compile_rule(
    rule_id: str,
    description: str,
    ascii_regex: bytes,
    utf16le_regex: bytes,
) -> Rule:
    return Rule(
        rule_id=rule_id,
        description=description,
        ascii_pattern=re.compile(ascii_regex, re.IGNORECASE),
        utf16le_pattern=re.compile(utf16le_regex, re.IGNORECASE),
    )


RULES = (
    _compile_rule(
        "control_plane_domain",
        "控制面域名",
        rb"(?:[a-z0-9-]+\.)+gooofish\.shop",
        rb"(?:(?:[a-z0-9-]\x00)+\.)+g\x00o\x00o\x00f\x00i\x00s\x00h\x00\.\x00s\x00h\x00o\x00p\x00",
    ),
    _compile_rule(
        "goofish_gateway",
        "Goofish 上游域名",
        rb"(?:[a-z0-9-]+\.)*goofish\.com",
        rb"(?:(?:[a-z0-9-]\x00)+\.\x00)*g\x00o\x00o\x00f\x00i\x00s\x00h\x00\.\x00c\x00o\x00m\x00",
    ),
    _compile_rule(
        "mtop_api",
        "明文 MTOP API 名称",
        rb"mtop\.(?:taobao|alibaba|idle|wx)\.[a-z0-9][a-z0-9._-]{2,}",
        rb"m\x00t\x00o\x00p\x00\.\x00(?:t\x00a\x00o\x00b\x00a\x00o|a\x00l\x00i\x00b\x00a\x00b\x00a|i\x00d\x00l\x00e|w\x00x)\.\x00(?:[a-z0-9._-]\x00){3,}",
    ),
    _compile_rule(
        "api_path",
        "明文 API 路径",
        rb"/api/[a-z0-9][a-z0-9_./:-]*",
        rb"/\x00a\x00p\x00i\x00/\x00(?:[a-z0-9_./:-]\x00){2,}",
    ),
    _compile_rule(
        "local_api_route",
        "明文本地 API 子路由",
        rb"/(?:accounts/:|account/|messages/(?:pc|send|chat|location|session)/|qr/(?:generate|status)|shop/datacompass|coin/(?:signin|activity|tasks?|promotion)|delivery/(?:runs|discovery-alerts)|operation-jobs/|message-stats/|logs/incidents|tasks/:|peer-monitor/(?:summary|candidates))[a-z0-9_./:-]*",
        rb"/\x00(?:a\x00c\x00c\x00o\x00u\x00n\x00t\x00s\x00/\x00:\x00|a\x00c\x00c\x00o\x00u\x00n\x00t\x00/\x00|m\x00e\x00s\x00s\x00a\x00g\x00e\x00s\x00/\x00(?:p\x00c|s\x00e\x00n\x00d|c\x00h\x00a\x00t|l\x00o\x00c\x00a\x00t\x00i\x00o\x00n|s\x00e\x00s\x00s\x00i\x00o\x00n)\x00/\x00|q\x00r\x00/\x00(?:g\x00e\x00n\x00e\x00r\x00a\x00t\x00e|s\x00t\x00a\x00t\x00u\x00s)|s\x00h\x00o\x00p\x00/\x00d\x00a\x00t\x00a\x00c\x00o\x00m\x00p\x00a\x00s\x00s|c\x00o\x00i\x00n\x00/\x00(?:s\x00i\x00g\x00n\x00i\x00n|a\x00c\x00t\x00i\x00v\x00i\x00t\x00y|t\x00a\x00s\x00k\x00s?|p\x00r\x00o\x00m\x00o\x00t\x00i\x00o\x00n)|d\x00e\x00l\x00i\x00v\x00e\x00r\x00y\x00/\x00(?:r\x00u\x00n\x00s|d\x00i\x00s\x00c\x00o\x00v\x00e\x00r\x00y\x00-\x00a\x00l\x00e\x00r\x00t\x00s)|o\x00p\x00e\x00r\x00a\x00t\x00i\x00o\x00n\x00-\x00j\x00o\x00b\x00s\x00/\x00|m\x00e\x00s\x00s\x00a\x00g\x00e\x00-\x00s\x00t\x00a\x00t\x00s\x00/\x00|l\x00o\x00g\x00s\x00/\x00i\x00n\x00c\x00i\x00d\x00e\x00n\x00t\x00s|t\x00a\x00s\x00k\x00s\x00/\x00:\x00|p\x00e\x00e\x00r\x00-\x00m\x00o\x00n\x00i\x00t\x00o\x00r\x00/\x00(?:s\x00u\x00m\x00m\x00a\x00r\x00y|c\x00a\x00n\x00d\x00i\x00d\x00a\x00t\x00e\x00s))(?:[a-z0-9_./:-]\x00)*",
    ),
    _compile_rule(
        "protocol_header",
        "明文设备/授权协议 Header",
        rb"(?:^|[^a-z0-9])x-(?:device|license|yuguanjia)-[a-z0-9-]+",
        rb"(?:^|[^a-z0-9]\x00)x\x00-\x00(?:d\x00e\x00v\x00i\x00c\x00e|l\x00i\x00c\x00e\x00n\x00s\x00e|y\x00u\x00g\x00u\x00a\x00n\x00j\x00i\x00a)\x00-\x00(?:[a-z0-9-]\x00){2,}",
    ),
    _compile_rule(
        "protocol_field",
        "明文上游请求字段",
        rb"(?:itemDO|contentParams|mainParams|loginFormData)",
        rb"(?:i\x00t\x00e\x00m\x00D\x00O|c\x00o\x00n\x00t\x00e\x00n\x00t\x00P\x00a\x00r\x00a\x00m\x00s|m\x00a\x00i\x00n\x00P\x00a\x00r\x00a\x00m\x00s|l\x00o\x00g\x00i\x00n\x00F\x00o\x00r\x00m\x00D\x00a\x00t\x00a)",
    ),
    _compile_rule(
        "fixed_jwt_secret",
        "固定本地 JWT 密钥标识",
        rb"tauri-local-tmp-secret",
        b"tauri-local-tmp-secret".decode("ascii").encode("utf-16le"),
    ),
    _compile_rule(
        "dev_browser_login",
        "生产浏览器开发登录入口",
        rb"(?:/api/auth/)?dev-browser-login",
        rb"(?:/\x00a\x00p\x00i\x00/\x00a\x00u\x00t\x00h\x00/\x00)?d\x00e\x00v\x00-\x00b\x00r\x00o\x00w\x00s\x00e\x00r\x00-\x00l\x00o\x00g\x00i\x00n\x00",
    ),
)


@dataclasses.dataclass(frozen=True)
class Finding:
    rule_id: str
    description: str
    location: str
    count: int


@dataclasses.dataclass(frozen=True)
class ArtifactReport:
    artifact: str
    findings: tuple[Finding, ...]

    @property
    def total(self) -> int:
        return sum(finding.count for finding in self.findings)


def scan_stream(stream: BinaryIO, location: str) -> list[Finding]:
    counts = {rule.rule_id: 0 for rule in RULES}
    carry = b""

    while True:
        chunk = stream.read(CHUNK_SIZE)
        if not chunk:
            break
        data = carry + chunk
        carry_length = len(carry)
        for rule in RULES:
            for pattern in (rule.ascii_pattern, rule.utf16le_pattern):
                counts[rule.rule_id] += sum(
                    1 for match in pattern.finditer(data) if match.end() > carry_length
                )
        carry = data[-OVERLAP_SIZE:]

    return [
        Finding(rule.rule_id, rule.description, location, counts[rule.rule_id])
        for rule in RULES
        if counts[rule.rule_id]
    ]


def scan_file(path: pathlib.Path, location: str | None = None) -> list[Finding]:
    with path.open("rb") as stream:
        return scan_stream(stream, location or path.name)


def scan_apk(path: pathlib.Path) -> list[Finding]:
    findings: list[Finding] = []
    with zipfile.ZipFile(path) as archive:
        for member in archive.infolist():
            if member.is_dir():
                continue
            if member.file_size > MAX_ARCHIVE_MEMBER_SIZE:
                raise ValueError(
                    f"archive member exceeds scan limit: {member.filename}"
                )
            with archive.open(member) as stream:
                findings.extend(scan_stream(stream, member.filename))
    return findings


def _merge_findings(findings: Iterable[Finding]) -> tuple[Finding, ...]:
    merged: dict[tuple[str, str, str], int] = {}
    for finding in findings:
        key = (finding.rule_id, finding.description, finding.location)
        merged[key] = merged.get(key, 0) + finding.count
    return tuple(
        Finding(rule_id, description, location, count)
        for (rule_id, description, location), count in sorted(merged.items())
    )


@contextlib.contextmanager
def mounted_dmg(path: pathlib.Path) -> Iterator[pathlib.Path]:
    hdiutil = shutil.which("hdiutil")
    if not hdiutil:
        raise RuntimeError("hdiutil is required to scan DMG artifacts")

    with tempfile.TemporaryDirectory(prefix="release-leak-audit-") as temp_dir:
        mountpoint = pathlib.Path(temp_dir) / "mount"
        mountpoint.mkdir()
        attach = subprocess.run(
            [
                hdiutil,
                "attach",
                "-readonly",
                "-nobrowse",
                "-noverify",
                "-mountpoint",
                str(mountpoint),
                str(path),
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        if attach.returncode != 0:
            raise RuntimeError(f"failed to mount DMG: {attach.stderr.strip()}")
        try:
            yield mountpoint
        finally:
            subprocess.run(
                [hdiutil, "detach", str(mountpoint), "-force"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                check=False,
            )


def scan_dmg(path: pathlib.Path) -> list[Finding]:
    findings: list[Finding] = []
    with mounted_dmg(path) as mountpoint:
        for candidate in sorted(mountpoint.rglob("*")):
            if not candidate.is_file() or candidate.is_symlink():
                continue
            relative = candidate.relative_to(mountpoint).as_posix()
            findings.extend(scan_file(candidate, relative))
    return findings


def scan_windows_executable(path: pathlib.Path) -> list[Finding]:
    """Scan a PE and, when available, its NSIS payload without requiring 7z."""
    findings = scan_file(path)
    extractor = shutil.which("7z") or shutil.which("7zz")
    if not extractor:
        return findings

    with tempfile.TemporaryDirectory(prefix="release-leak-exe-") as temp_dir:
        result = subprocess.run(
            [extractor, "x", "-y", f"-o{temp_dir}", str(path)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if result.returncode != 0:
            return findings
        root = pathlib.Path(temp_dir)
        for candidate in sorted(root.rglob("*")):
            if not candidate.is_file() or candidate.is_symlink():
                continue
            relative = candidate.relative_to(root).as_posix()
            findings.extend(scan_file(candidate, f"payload/{relative}"))
    return findings


def scan_artifact(path: pathlib.Path) -> ArtifactReport:
    path = path.resolve()
    if not path.is_file():
        raise FileNotFoundError(f"artifact does not exist: {path}")

    suffix = path.suffix.lower()
    if suffix == ".apk":
        findings = scan_apk(path)
    elif suffix == ".dmg":
        findings = scan_dmg(path)
    elif suffix == ".exe":
        findings = scan_windows_executable(path)
    else:
        findings = scan_file(path)
    return ArtifactReport(str(path), _merge_findings(findings))


def _report_as_dict(reports: Iterable[ArtifactReport]) -> dict[str, object]:
    serialized = []
    total = 0
    for report in reports:
        total += report.total
        serialized.append(
            {
                "artifact": report.artifact,
                "finding_count": report.total,
                "findings": [dataclasses.asdict(finding) for finding in report.findings],
            }
        )
    return {"ok": total == 0, "finding_count": total, "artifacts": serialized}


def render_text(reports: Iterable[ArtifactReport]) -> str:
    lines: list[str] = []
    total = 0
    for report in reports:
        lines.append(f"artifact: {report.artifact}")
        if not report.findings:
            lines.append("  ok: no protected protocol literals found")
            continue
        total += report.total
        by_rule: dict[tuple[str, str], int] = {}
        locations: dict[tuple[str, str], list[str]] = {}
        for finding in report.findings:
            key = (finding.rule_id, finding.description)
            by_rule[key] = by_rule.get(key, 0) + finding.count
            locations.setdefault(key, []).append(finding.location)
        for key, count in sorted(by_rule.items()):
            rule_id, description = key
            member_list = ", ".join(sorted(locations[key]))
            lines.append(f"  FAIL {rule_id}: {description}; count={count}")
            lines.append(f"    locations: {member_list}")
    if total:
        lines.append(f"release protocol leak audit failed: findings={total}")
    else:
        lines.append("release protocol leak audit passed")
    return "\n".join(lines)


def _expand_inputs(inputs: Iterable[pathlib.Path]) -> list[pathlib.Path]:
    expanded: list[pathlib.Path] = []
    for item in inputs:
        if item.is_dir():
            expanded.extend(
                path
                for path in sorted(item.iterdir())
                if path.is_file()
                and path.suffix.lower() in {".apk", ".dmg", ".exe", ".so"}
            )
        else:
            expanded.append(item)
    return expanded


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Audit release binaries without printing matched secrets or protocol data."
    )
    parser.add_argument(
        "artifacts",
        type=pathlib.Path,
        nargs="+",
        help="Mach-O, PE/EXE, Android APK/SO, DMG, or a directory containing them",
    )
    parser.add_argument("--json", action="store_true", help="emit a redacted JSON report")
    args = parser.parse_args(argv)

    paths = _expand_inputs(args.artifacts)
    if not paths:
        print("error: no supported release artifacts found", file=sys.stderr)
        return 2

    try:
        reports = [scan_artifact(path) for path in paths]
    except (OSError, RuntimeError, ValueError, zipfile.BadZipFile) as exc:
        print(f"error: release protocol leak audit could not complete: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps(_report_as_dict(reports), ensure_ascii=False, indent=2))
    else:
        print(render_text(reports))
    return 1 if any(report.findings for report in reports) else 0


if __name__ == "__main__":
    raise SystemExit(main())
