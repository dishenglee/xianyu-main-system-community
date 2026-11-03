#!/usr/bin/env python3
from __future__ import annotations

import argparse
import html
import json
import re
import shutil
import sys
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.parse import quote


DEFAULT_MINIMUM_VERSION_LAG = 2


@dataclass(frozen=True)
class PublishResult:
    artifacts: dict[str, Path]
    policy_path: Path
    index_path: Path


def artifact_names(version: str) -> dict[str, str]:
    return {
        "macos": f"macos-鱼管家_{version}.dmg",
        "windows": f"windows-鱼管家_{version}.exe",
        "android": f"android-鱼管家_{version}.apk",
    }


def normalize_base_url(value: str) -> str:
    base = value.strip().rstrip("/")
    if not base:
        raise SystemExit("download base url must not be empty")
    return base


def public_url(base_url: str, filename: str) -> str:
    return f"{normalize_base_url(base_url)}/{quote(filename)}"


def load_known_release_versions(path: Path) -> list[str]:
    if not path.exists():
        raise SystemExit(f"missing known versions file: {path}")
    versions = [
        line.strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip() and not line.strip().startswith("#")
    ]
    if not versions:
        raise SystemExit(f"empty known versions file: {path}")
    return versions


REPO_ROOT = Path(__file__).resolve().parent.parent
KNOWN_RELEASE_VERSIONS = load_known_release_versions(
    REPO_ROOT / "docs" / "releases" / "known_versions.txt"
)


def minimum_version_for_release(version: str, lag: int = DEFAULT_MINIMUM_VERSION_LAG) -> str:
    version = version.strip()
    idx = KNOWN_RELEASE_VERSIONS.index(version) if version in KNOWN_RELEASE_VERSIONS else -1
    if idx >= 0:
        min_idx = max(0, idx - lag)
        return KNOWN_RELEASE_VERSIONS[min_idx]
    raise SystemExit(
        f"unsupported release version: {version} — "
        f"add it to KNOWN_RELEASE_VERSIONS in this script and in src-tauri/src/lib.rs"
    )


def resolve_minimum_version(version: str, minimum_version: str, lag: int) -> str:
    value = minimum_version.strip()
    if value and value.lower() != "auto":
        return value
    return minimum_version_for_release(version, lag)


def update_required_message(minimum_version: str) -> str:
    return f"当前客户端版本过低，最低可用版本为 v{minimum_version}，请更新后继续使用。"


def read_release_notes(path: Path | None) -> str:
    if path is None or not path.exists():
        return ""
    return path.read_text(encoding="utf-8").strip()


def beijing_now_label() -> str:
    return datetime.now(timezone(timedelta(hours=8))).strftime("%Y-%m-%d %H:%M:%S")


def render_download_page(policy: dict[str, object], last_updated_at: str = "") -> str:
    latest_version = html.escape(str(policy.get("latestVersion") or ""))
    minimum_version = html.escape(str(policy.get("minimumVersion") or ""))
    message = html.escape(str(policy.get("message") or ""))
    notes = html.escape(str(policy.get("body") or "暂无更新日志。"))
    last_updated_at = html.escape(last_updated_at.strip())
    downloads = policy.get("downloads")
    if not isinstance(downloads, dict):
        downloads = {}

    labels = {
        "macos": "下载 macOS 版",
        "windows": "下载 Windows 版",
        "android": "下载 Android APK",
    }
    links = "\n".join(
        f'<a class="download-link" href="{html.escape(str(downloads.get(platform) or "#"), quote=True)}">{label}</a>'
        for platform, label in labels.items()
    )
    updated_html = (
        f'<span class="meta-row">上次更新时间：{last_updated_at}（北京时间）</span>'
        if last_updated_at
        else ""
    )

    return f"""<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>鱼管家下载</title>
  <style>
    body {{
      margin: 0;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", sans-serif;
      color: #1f2937;
      background: #f6f7f9;
    }}
    main {{
      max-width: 840px;
      margin: 0 auto;
      padding: 44px 20px 56px;
    }}
    h1 {{
      margin: 0 0 10px;
      font-size: 32px;
      line-height: 1.2;
    }}
    .meta {{
      margin: 0 0 24px;
      color: #64748b;
      font-size: 14px;
    }}
    .meta-row {{
      display: block;
      margin-top: 6px;
    }}
    .downloads {{
      display: grid;
      grid-template-columns: repeat(auto-fit, minmax(190px, 1fr));
      gap: 12px;
      margin: 24px 0 28px;
    }}
    .download-link {{
      display: block;
      padding: 14px 16px;
      border-radius: 8px;
      color: #111827;
      background: #ffd84d;
      text-align: center;
      text-decoration: none;
      font-weight: 700;
    }}
    .download-link:hover {{
      background: #ffca1a;
    }}
    section {{
      border: 1px solid #e5e7eb;
      border-radius: 8px;
      background: #fff;
      padding: 20px;
    }}
    h2 {{
      margin: 0 0 14px;
      font-size: 20px;
    }}
    pre {{
      margin: 0;
      white-space: pre-wrap;
      word-break: break-word;
      line-height: 1.7;
      font: 14px/1.7 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      color: #334155;
    }}
    @media (max-width: 520px) {{
      main {{ padding-top: 28px; }}
      h1 {{ font-size: 26px; }}
    }}
  </style>
</head>
<body>
  <main>
    <h1>鱼管家下载</h1>
    <p class="meta">最新版本：v{latest_version}　最低可用版本：v{minimum_version}{updated_html}</p>
    <div class="downloads">
      {links}
    </div>
    <section>
      <h2>更新日志</h2>
      <pre>{notes}</pre>
    </section>
    <p class="meta">{message}</p>
  </main>
</body>
</html>
"""


def publish(
    *,
    artifact_dir: Path,
    output_root: Path,
    version: str,
    minimum_version: str,
    force_update: bool,
    update_message: str,
    base_url: str,
    last_updated_at: str = "",
) -> PublishResult:
    minimum_version = resolve_minimum_version(version, minimum_version, DEFAULT_MINIMUM_VERSION_LAG)
    names = artifact_names(version)
    missing = [name for name in names.values() if not (artifact_dir / name).is_file()]
    if missing:
        raise SystemExit(f"missing required artifact: {', '.join(missing)}")

    output_root.mkdir(parents=True, exist_ok=True)
    for pattern in ("macos-*.dmg", "windows-*.exe", "android-*.apk"):
        for stale in output_root.glob(pattern):
            if stale.is_file():
                stale.unlink()

    copied: dict[str, Path] = {}
    for platform, name in names.items():
        dest = output_root / name
        shutil.copy2(artifact_dir / name, dest)
        copied[platform] = dest

    downloads = {platform: public_url(base_url, path.name) for platform, path in copied.items()}
    policy = {
        "minimumVersion": minimum_version,
        "latestVersion": version,
        "body": update_message,
        "required": force_update,
        "message": update_required_message(minimum_version),
        "downloadPageUrl": f"{normalize_base_url(base_url)}/",
        "downloads": downloads,
    }

    policy_path = output_root / "api/client/update-policy"
    policy_path.parent.mkdir(parents=True, exist_ok=True)
    policy_path.write_text(
        json.dumps(policy, ensure_ascii=False, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )
    index_path = output_root / "index.html"
    index_path.write_text(
        render_download_page(policy, last_updated_at or beijing_now_label()),
        encoding="utf-8",
    )

    return PublishResult(artifacts=copied, policy_path=policy_path, index_path=index_path)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Publish the three public release artifacts.")
    parser.add_argument("--artifact-dir", required=True, type=Path)
    parser.add_argument("--output-root", required=True, type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--minimum-version", default="auto")
    parser.add_argument("--minimum-version-lag", type=int, default=DEFAULT_MINIMUM_VERSION_LAG)
    parser.add_argument("--force-update", choices=["true", "false"], default="false")
    parser.add_argument("--update-message", default="")
    parser.add_argument("--release-notes-file", type=Path)
    parser.add_argument("--base-url", default="https://download.gooofish.shop")
    parser.add_argument(
        "--last-updated-at",
        default="",
        help="下载页展示的上次更新时间，格式建议为北京时间 YYYY-MM-DD HH:MM:SS；留空时自动使用当前北京时间。",
    )
    args = parser.parse_args(argv)

    update_message = args.update_message.strip() or read_release_notes(args.release_notes_file)
    minimum_version = resolve_minimum_version(
        args.version.strip(),
        args.minimum_version.strip(),
        args.minimum_version_lag,
    )
    result = publish(
        artifact_dir=args.artifact_dir,
        output_root=args.output_root,
        version=args.version.strip(),
        minimum_version=minimum_version,
        force_update=args.force_update == "true",
        update_message=update_message,
        base_url=args.base_url,
        last_updated_at=args.last_updated_at,
    )

    for platform, path in result.artifacts.items():
        print(f"{platform}={path}")
    print(f"policy={result.policy_path}")
    print(f"index={result.index_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
