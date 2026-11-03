#!/usr/bin/env python3
from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import publish_download_artifacts as publisher


class PublishDownloadArtifactsTest(unittest.TestCase):
    def test_collects_only_three_release_artifacts_and_writes_download_policy(self) -> None:
        with tempfile.TemporaryDirectory() as source_dir, tempfile.TemporaryDirectory() as output_dir:
            source = Path(source_dir)
            output = Path(output_dir)

            (source / "macos-鱼管家_0.1.2_universal.dmg").write_text("mac", encoding="utf-8")
            (source / "windows-鱼管家_0.1.2_x64-setup.exe").write_text("win", encoding="utf-8")
            (source / "android-app-universal-release.apk").write_text("android", encoding="utf-8")
            (source / "windows-xianyu-tauri.exe").write_text("internal", encoding="utf-8")
            (source / "release.log").write_text("log", encoding="utf-8")

            result = publisher.publish(
                artifact_dir=source,
                output_root=output,
                version="0.1.2",
                minimum_version="0.1.0",
                force_update=False,
                update_message="0.1.2：更新说明",
                base_url="https://download.gooofish.shop",
            )

            self.assertEqual(
                sorted(path.name for path in output.iterdir() if path.is_file()),
                [
                    "android-app-universal-release.apk",
                    "macos-鱼管家_0.1.2_universal.dmg",
                    "windows-鱼管家_0.1.2_x64-setup.exe",
                ],
            )
            self.assertEqual(len(result.artifacts), 3)

            policy = json.loads((output / "api/client/update-policy").read_text(encoding="utf-8"))
            self.assertEqual(policy["latestVersion"], "0.1.2")
            self.assertEqual(policy["minimumVersion"], "0.1.0")
            self.assertFalse(policy["required"])
            self.assertEqual(policy["downloadPageUrl"], "https://download.gooofish.shop/")
            self.assertEqual(
                policy["downloads"],
                {
                    "macos": "https://download.gooofish.shop/macos-%E9%B1%BC%E7%AE%A1%E5%AE%B6_0.1.2_universal.dmg",
                    "windows": "https://download.gooofish.shop/windows-%E9%B1%BC%E7%AE%A1%E5%AE%B6_0.1.2_x64-setup.exe",
                    "android": "https://download.gooofish.shop/android-app-universal-release.apk",
                },
            )

    def test_fails_when_a_required_artifact_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as source_dir, tempfile.TemporaryDirectory() as output_dir:
            source = Path(source_dir)
            output = Path(output_dir)
            (source / "macos-鱼管家_0.1.2_universal.dmg").write_text("mac", encoding="utf-8")

            with self.assertRaisesRegex(SystemExit, "missing required artifact"):
                publisher.publish(
                    artifact_dir=source,
                    output_root=output,
                    version="0.1.2",
                    minimum_version="0.1.0",
                    force_update=False,
                    update_message="0.1.2：更新说明",
                    base_url="https://download.gooofish.shop",
                )


if __name__ == "__main__":
    unittest.main()
