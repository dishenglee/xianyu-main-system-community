#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
import secrets
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


DEFAULT_SYNC_URL = "https://license.gooofish.shop/api/internal/client-release/sync"
DEFAULT_USER_AGENT = "YuguanjiaReleaseMonitor/1.0"


def env_value(*names: str) -> str:
    for name in names:
        value = os.environ.get(name, "").strip()
        if value:
            return value
    return ""


def canonical_message(
    method: str,
    path: str,
    body_sha256_hex: str,
    timestamp: int,
    nonce: str,
    client_id: str,
) -> str:
    return "\n".join(
        [
            method.strip().upper(),
            path.strip(),
            body_sha256_hex.strip().lower(),
            str(timestamp),
            nonce.strip(),
            client_id.strip(),
        ]
    )


def sign_message(secret_hex: str, message: str) -> str:
    try:
        secret = bytes.fromhex(secret_hex.strip())
    except ValueError as error:
        raise SystemExit(f"invalid sync secret hex: {error}") from error
    return hmac.new(secret, message.encode("utf-8"), hashlib.sha256).hexdigest()


def policy_body(policy_file: Path) -> bytes:
    payload = json.loads(policy_file.read_text(encoding="utf-8"))
    latest_version = str(payload.get("latestVersion") or "").strip()
    if not latest_version:
        raise SystemExit(f"policy missing latestVersion: {policy_file}")
    return json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def post_policy(
    url: str,
    body: bytes,
    client_id: str,
    key_id: str,
    secret_hex: str,
    user_agent: str,
) -> str:
    parsed = urllib.parse.urlsplit(url)
    path = parsed.path or "/"
    timestamp = int(time.time())
    nonce = secrets.token_hex(16)
    body_sha = hashlib.sha256(body).hexdigest()
    message = canonical_message("POST", path, body_sha, timestamp, nonce, client_id)
    signature = sign_message(secret_hex, message)
    request = urllib.request.Request(
        url,
        data=body,
        method="POST",
        headers={
            "accept": "application/json",
            "content-type": "application/json",
            "user-agent": user_agent,
            "x-control-plane-client": client_id,
            "x-control-plane-key-id": key_id,
            "x-control-plane-ts": str(timestamp),
            "x-control-plane-nonce": nonce,
            "x-control-plane-signature": signature,
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.read().decode("utf-8")
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise SystemExit(f"sync failed: HTTP {error.code}: {detail}") from error
    except urllib.error.URLError as error:
        raise SystemExit(f"sync failed: {error}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description="Sync client release policy to license service.")
    parser.add_argument("--policy-file", required=True, type=Path)
    parser.add_argument("--url", default=env_value("YUGUANJIA_LICENSE_SYNC_URL") or DEFAULT_SYNC_URL)
    parser.add_argument(
        "--client-id",
        default=env_value("YUGUANJIA_LICENSE_SYNC_CLIENT_ID", "LICENSE_CONTROL_PLANE_CLIENT_ID")
        or "xianyu-control-plane",
    )
    parser.add_argument(
        "--key-id",
        default=env_value("YUGUANJIA_LICENSE_SYNC_KEY_ID", "LICENSE_CONTROL_PLANE_KEY_ID")
        or "default",
    )
    parser.add_argument(
        "--secret-hex",
        default=env_value("YUGUANJIA_LICENSE_SYNC_SECRET_HEX", "LICENSE_CONTROL_PLANE_SHARED_SECRET_HEX"),
    )
    parser.add_argument(
        "--user-agent",
        default=env_value("YUGUANJIA_LICENSE_SYNC_USER_AGENT") or DEFAULT_USER_AGENT,
    )
    parser.add_argument("--skip-without-secret", action="store_true")
    args = parser.parse_args()

    if not args.secret_hex:
        if args.skip_without_secret:
            print("client release policy sync skipped: missing sync secret")
            return 0
        raise SystemExit("missing YUGUANJIA_LICENSE_SYNC_SECRET_HEX")

    body = policy_body(args.policy_file)
    response = post_policy(
        args.url,
        body,
        args.client_id,
        args.key_id,
        args.secret_hex,
        args.user_agent,
    )
    print(response)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
