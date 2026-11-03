#!/usr/bin/env python3
"""
Probe the H5 service-publish flow through the local xianyu backend.

Default mode is dry-run: it selects an active account with security_score > 80,
checks service type APIs, checks coin sign-in/task APIs, and builds the service
publish payload without publishing. Pass --publish to upload a local image and
create the service listing.
"""

from __future__ import annotations

import argparse
import json
import mimetypes
import os
import random
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_DB = ROOT / "data/accounts.db"
DEFAULT_CAPTURE = Path(
    "tools/xianyu-capture/"
    "output_20260527_frida_live/capture_20260527_171538.jsonl"
)
DEFAULT_OUTPUT_DIR = ROOT / "tmp/h5-service-publish"


DEFAULT_LOCATION = {
    "fish_ad_code": "430202",
    "gps": "27.867133,113.164345",
    "latitude": "27.867133",
    "longitude": "113.164345",
    "province": "湖南省",
    "city": "株洲市",
    "district": "荷塘区",
    "name": "新桂广场新桂都",
    "address": "",
}


@dataclass
class Account:
    uid: str
    nick: str
    security_score: int
    fish_power: int


class HttpClient:
    def __init__(self, base_url: str, token: str = "", timeout: int = 45) -> None:
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.timeout = timeout

    def request(self, method: str, path: str, payload: Any | None = None) -> Any:
        body = None
        headers = {"Accept": "application/json"}
        if payload is not None:
            body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
            headers["Content-Type"] = "application/json"
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        req = urllib.request.Request(
            self.base_url + path,
            data=body,
            headers=headers,
            method=method,
        )
        return self._open_json(req)

    def upload(self, path: str, uid: str, image_path: Path) -> Any:
        boundary = f"----xianyu-test-{uuid.uuid4().hex}"
        content_type = mimetypes.guess_type(str(image_path))[0] or "application/octet-stream"
        file_bytes = image_path.read_bytes()
        parts = [
            (
                f"--{boundary}\r\n"
                'Content-Disposition: form-data; name="uid"\r\n\r\n'
                f"{uid}\r\n"
            ).encode("utf-8"),
            (
                f"--{boundary}\r\n"
                f'Content-Disposition: form-data; name="file"; filename="{image_path.name}"\r\n'
                f"Content-Type: {content_type}\r\n\r\n"
            ).encode("utf-8"),
            file_bytes,
            f"\r\n--{boundary}--\r\n".encode("utf-8"),
        ]
        headers = {
            "Accept": "application/json",
            "Content-Type": f"multipart/form-data; boundary={boundary}",
        }
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        req = urllib.request.Request(
            self.base_url + path,
            data=b"".join(parts),
            headers=headers,
            method="POST",
        )
        return self._open_json(req)

    def _open_json(self, req: urllib.request.Request) -> Any:
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                text = resp.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as exc:
            text = exc.read().decode("utf-8", "replace")
            raise RuntimeError(f"HTTP {exc.code} {req.full_url}: {text[:1000]}") from exc
        except urllib.error.URLError as exc:
            raise RuntimeError(f"request failed {req.full_url}: {exc}") from exc
        try:
            return json.loads(text)
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"non-json response {req.full_url}: {text[:1000]}") from exc


def now_ms() -> int:
    return int(time.time() * 1000)


def ret_success(payload: Any) -> bool:
    if not isinstance(payload, dict):
        return False
    ret = payload.get("ret")
    first = ""
    if isinstance(ret, list) and ret:
        first = str(ret[0])
    elif isinstance(ret, str):
        first = ret
    if "SUCCESS" in first.upper():
        return True
    data = payload.get("data")
    return isinstance(data, dict) and data.get("success") is True


def save_json(output_dir: Path, name: str, payload: Any) -> Path:
    output_dir.mkdir(parents=True, exist_ok=True)
    path = output_dir / f"{name}.json"
    path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
    return path


def load_accounts(db_path: Path) -> list[Account]:
    if not db_path.exists():
        raise FileNotFoundError(f"accounts db not found: {db_path}")
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    try:
        rows = conn.execute(
            """
            SELECT uid, nick, security_score, fish_power
            FROM accounts
            WHERE status = 'active' AND security_score > 80
            ORDER BY security_score DESC, fish_power DESC, updated_at DESC
            """
        ).fetchall()
    finally:
        conn.close()
    return [
        Account(
            uid=str(row["uid"]),
            nick=str(row["nick"] or ""),
            security_score=int(row["security_score"] or 0),
            fish_power=int(row["fish_power"] or 0),
        )
        for row in rows
    ]


def select_account(db_path: Path, uid: str = "") -> Account:
    accounts = load_accounts(db_path)
    if uid:
        for account in accounts:
            if account.uid == uid:
                return account
        raise RuntimeError(f"uid={uid} is not active or security_score <= 80")
    if not accounts:
        raise RuntimeError("no active account with security_score > 80")
    return accounts[0]


def load_location(db_path: Path, uid: str) -> dict[str, str]:
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    try:
        row = conn.execute(
            """
            SELECT fish_ad_code, gps, latitude, longitude, province, city, district,
                   name, address
            FROM publish_locations
            WHERE uid = ?
            LIMIT 1
            """,
            (uid,),
        ).fetchone()
    finally:
        conn.close()
    if not row:
        return dict(DEFAULT_LOCATION)
    result = dict(DEFAULT_LOCATION)
    for key in result:
        if key in row.keys() and row[key]:
            result[key] = str(row[key])
    return result


def load_publish_template(capture_path: Path) -> dict[str, Any]:
    if not capture_path.exists():
        raise FileNotFoundError(f"capture not found: {capture_path}")
    selected: dict[str, Any] | None = None
    with capture_path.open("r", encoding="utf-8") as fh:
        for line in fh:
            try:
                obj = json.loads(line)
            except json.JSONDecodeError:
                continue
            if obj.get("type") != "mtop_request":
                continue
            if obj.get("api") != "mtop.idle.idleitem.publish":
                continue
            data = obj.get("data")
            if isinstance(data, str):
                try:
                    data = json.loads(data)
                except json.JSONDecodeError:
                    continue
            if isinstance(data, dict) and data.get("sourceId") == "skillydy":
                selected = data
    if not selected:
        raise RuntimeError("no skillydy service publish template found in capture")
    return selected


def pick_local_image(paths: list[str]) -> Path:
    candidates: list[Path] = []
    if paths:
        for raw in paths:
            p = Path(raw).expanduser()
            if p.is_file():
                candidates.append(p)
            elif p.is_dir():
                candidates.extend(iter_images(p, limit=80))
    else:
        for folder in [
            Path.home() / "Desktop",
            Path.home() / "Pictures",
            Path.home() / "Downloads",
            ROOT / "docs",
        ]:
            candidates.extend(iter_images(folder, limit=60))
    if not candidates:
        raise RuntimeError("no local image found; pass --image /path/to/image")
    return random.choice(candidates)


def iter_images(folder: Path, limit: int) -> list[Path]:
    if not folder.exists():
        return []
    exts = {".jpg", ".jpeg", ".png", ".webp", ".heic"}
    result: list[Path] = []
    for path in folder.rglob("*"):
        if len(result) >= limit:
            break
        if path.is_file() and path.suffix.lower() in exts and path.stat().st_size > 0:
            result.append(path)
    return result


def login_if_needed(client: HttpClient, args: argparse.Namespace) -> str:
    if args.token:
        return args.token
    env_token = os.environ.get("XIANYU_AUTH_TOKEN") or os.environ.get("APP_BEARER_TOKEN") or ""
    if env_token:
        return env_token
    username = args.username or os.environ.get("ADMIN_USERNAME") or "admin"
    password = args.password or os.environ.get("ADMIN_PASSWORD") or "admin123"
    login_payload = client.request(
        "POST",
        "/api/auth/login",
        {"username": username, "password": password},
    )
    token = str(login_payload.get("token") or "")
    if not token:
        raise RuntimeError(f"login response missing token: {login_payload}")
    return token


def mtop(client: HttpClient, uid: str, api: str, version: str, data: Any, channel: str) -> Any:
    return client.request(
        "POST",
        "/api/mtop",
        {
            "uid": uid,
            "api_name": api,
            "api_version": version,
            "data": data,
            "channel": channel,
        },
    )


def rich_text_desc(text: str) -> str:
    return json.dumps(
        {
            "type": None,
            "children": [
                {
                    "type": "Element",
                    "children": [
                        {
                            "type": "Text",
                            "text": text,
                            "properties": {},
                        }
                    ],
                    "properties": {},
                }
            ],
            "properties": {},
            "version": "1.0",
        },
        ensure_ascii=False,
    )


def build_image_info(url: str, source_path: Path) -> dict[str, Any]:
    return {
        "extraInfo": {"isH": "false", "isT": "false", "raw": "false"},
        "heightSize": 800,
        "imgPath": str(source_path),
        "isQrCode": False,
        "labels": [],
        "longImage": False,
        "major": True,
        "thumbnail": str(source_path),
        "type": 0,
        "url": url,
        "widthSize": 800,
    }


def build_skus(spec_name: str) -> list[dict[str, Any]]:
    rows = [
        ("-1", "基础服务", 990, 99),
        ("-2", "进阶服务", 1990, 99),
    ]
    skus = []
    for order, (value_id, value_text, price, qty) in enumerate(rows):
        skus.append(
            {
                "quantity": qty,
                "priceInCent": price,
                "propertyList": [
                    {
                        "propertyId": "-1",
                        "valueId": value_id,
                        "propertyText": spec_name,
                        "valueText": value_text,
                        "idleValueId": value_id,
                        "idlePropertyId": "-1",
                        "actualValueText": value_text,
                        "enabled": "true",
                        "status": "0",
                        "propertySortOrder": "0",
                        "valueSortOrder": str(order),
                        "string": f"-1:{value_id}",
                    }
                ],
            }
        )
    return skus


def patch_publish_payload(
    template: dict[str, Any],
    *,
    location: dict[str, str],
    image_url: str,
    image_path: Path,
    title: str,
    short_title: str,
    desc: str,
    spec_name: str,
) -> dict[str, Any]:
    payload = json.loads(json.dumps(template, ensure_ascii=False))
    stamp = str(now_ms()) + str(random.randint(100, 999))

    payload["uniqueCode"] = stamp
    payload["sourceId"] = "skillydy"
    payload["bizFrom"] = "skill"
    payload["publishScene"] = "mainPublish"
    payload["simpleItem"] = "true"
    payload["quantity"] = "10000"
    payload["fishAdCode"] = location["fish_ad_code"]
    payload["gps"] = location["gps"]
    payload["latitude"] = location["latitude"]
    payload["longitude"] = location["longitude"]
    payload["imageInfoDOList"] = [build_image_info(image_url, image_path)]
    payload["itemTextDTO"] = {
        "title": title,
        "shortTitle": short_title,
        "desc": desc,
        "richTextDesc": rich_text_desc(desc),
        "titleDescSeparate": False,
        "trackParamsRichTextStyle": "",
    }
    payload["itemSkuList"] = build_skus(spec_name)
    payload["itemPriceDTO"] = {"depositePercent": "0"}
    payload["itemPostFeeDTO"] = {
        "canFreeShipping": False,
        "onlyTakeSelf": False,
        "supportFreight": False,
        "templateId": "0",
    }
    payload["itemAddrDTO"] = {
        "addressType": 5,
        "aoiId": "",
        "aoiName": location.get("name", ""),
        "area": location.get("district", ""),
        "city": location.get("city", ""),
        "divisionId": location["fish_ad_code"],
        "gps": location["gps"],
        "poiId": "",
        "poiName": location.get("name", ""),
        "prov": location.get("province", ""),
    }
    base_params = payload.setdefault("baseParams", {})
    base_params.update(
        {
            "bizFrom": "skill",
            "bizcode": "mainPublish",
            "bucketId": "9",
            "rentMultiDayPackage": "1",
            "rentSmartPrice": "1",
            "scene": "ai_assist_desc_gen",
            "simpleItem": "true",
        }
    )
    return payload


def find_service_names(payload: Any) -> list[dict[str, str]]:
    found: list[dict[str, str]] = []

    def walk(value: Any) -> None:
        if isinstance(value, dict):
            skill_id = value.get("skillId") or value.get("id")
            name = value.get("skillName") or value.get("name") or value.get("title")
            level4 = value.get("level4") or value.get("channelCateId") or value.get("channelCatId")
            if skill_id and name:
                found.append(
                    {
                        "skillId": str(skill_id),
                        "name": str(name),
                        "level4": "" if level4 is None else str(level4),
                    }
                )
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)

    walk(payload)
    dedup: dict[tuple[str, str], dict[str, str]] = {}
    for item in found:
        dedup[(item["skillId"], item["name"])] = item
    return list(dedup.values())


def extract_award_tasks(payload: Any) -> list[dict[str, Any]]:
    data = payload.get("data") if isinstance(payload, dict) else None
    if not isinstance(data, dict):
        data = payload if isinstance(payload, dict) else {}
    tasks = data.get("taskList") if isinstance(data, dict) else []
    result = []
    if not isinstance(tasks, list):
        return result
    for task in tasks:
        if not isinstance(task, dict):
            continue
        material = task.get("materialDTO") if isinstance(task.get("materialDTO"), dict) else {}
        benefit = task.get("taskBenefitDTO") if isinstance(task.get("taskBenefitDTO"), dict) else {}
        title = str(material.get("title") or "")
        status = str(task.get("idleTaskStatus") or "")
        need_benefit = bool(benefit.get("needBenefit"))
        if "发布" in title and (status == "AWARD" or need_benefit):
            result.append(
                {
                    "deliveryId": task.get("deliveryId"),
                    "sceneId": task.get("sceneId") or 496,
                    "title": title,
                    "subTitle": material.get("subTitle"),
                    "status": status,
                    "needBenefit": need_benefit,
                }
            )
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description="Test H5 service publish + coin APIs")
    parser.add_argument("--base-url", default=os.environ.get("XIANYU_BASE_URL", "http://127.0.0.1:3000"))
    parser.add_argument("--db", type=Path, default=DEFAULT_DB)
    parser.add_argument("--capture", type=Path, default=DEFAULT_CAPTURE)
    parser.add_argument("--output-dir", type=Path, default=DEFAULT_OUTPUT_DIR / time.strftime("%Y%m%d-%H%M%S"))
    parser.add_argument("--uid", default="")
    parser.add_argument("--token", default=os.environ.get("XIANYU_AUTH_TOKEN", ""))
    parser.add_argument("--username", default=os.environ.get("ADMIN_USERNAME", ""))
    parser.add_argument("--password", default=os.environ.get("ADMIN_PASSWORD", ""))
    parser.add_argument("--channel", default="h5", choices=["h5", "pc", "wx", "app", "auto"])
    parser.add_argument("--skill-id", default="40145", help="service type to verify; default 软件开发")
    parser.add_argument("--asac", default=os.environ.get("XIANYU_ASAC", "2A23B272CW28KVB60F0O51"))
    parser.add_argument("--image", action="append", default=[])
    parser.add_argument("--publish", action="store_true", help="actually upload image and publish service")
    parser.add_argument("--claim", action="store_true", help="claim publish reward tasks after publish/list query")
    parser.add_argument("--sign", action="store_true", help="perform daily coin sign-in if not signed")
    args = parser.parse_args()

    account = select_account(args.db, args.uid)
    location = load_location(args.db, account.uid)
    image_path = pick_local_image(args.image)

    print(f"account: {account.nick} uid={account.uid} security_score={account.security_score} fish_power={account.fish_power}")
    print(f"location: {location['fish_ad_code']} {location['gps']} {location.get('province','')}{location.get('city','')}{location.get('district','')}")
    print(f"image: {image_path}")
    print(f"output: {args.output_dir}")

    public_client = HttpClient(args.base_url)
    token = login_if_needed(public_client, args)
    client = HttpClient(args.base_url, token=token)

    health = public_client.request("GET", "/health")
    save_json(args.output_dir, "00_health", health)

    service_list_req = {
        "fishAdCode": location["fish_ad_code"],
        "gps": location["gps"],
        "reqFromPage": "itemPublish",
    }
    service_list = mtop(
        client,
        account.uid,
        "mtop.taobao.idle.ecology.skill.map.list",
        "1.0",
        service_list_req,
        args.channel,
    )
    save_json(args.output_dir, "10_skill_map_list", service_list)
    service_names = find_service_names(service_list)
    save_json(args.output_dir, "11_skill_map_list_extracted", service_names[:120])

    backend_req = {
        "fishAdCode": location["fish_ad_code"],
        "gps": location["gps"],
        "reqFromPage": "skillPrePublish",
        "skillId": args.skill_id,
    }
    backend_query = mtop(
        client,
        account.uid,
        "mtop.taobao.idle.ecology.skill.map.backend.query",
        "1.0",
        backend_req,
        args.channel,
    )
    save_json(args.output_dir, "12_skill_backend_query", backend_query)

    sign_home_before = mtop(
        client,
        account.uid,
        "mtop.taobao.idle.play.sign.in.home",
        "1.0",
        {
            "fishAdCode": location["fish_ad_code"],
            "gps": location["gps"],
            "playId": 4,
        },
        args.channel,
    )
    save_json(args.output_dir, "20_sign_home_before", sign_home_before)
    sign_result = None
    sign_data = sign_home_before.get("data") if isinstance(sign_home_before, dict) else {}
    if args.sign and isinstance(sign_data, dict) and not sign_data.get("signedIn"):
        sign_result = mtop(
            client,
            account.uid,
            "mtop.taobao.idle.play.sign.in",
            "1.0",
            {
                "asac": args.asac,
                "fishAdCode": location["fish_ad_code"],
                "gps": location["gps"],
                "playId": 4,
                "usertoken": "",
            },
            args.channel,
        )
        save_json(args.output_dir, "21_sign_result", sign_result)
        sign_home_after = mtop(
            client,
            account.uid,
            "mtop.taobao.idle.play.sign.in.home",
            "1.0",
            {
                "fishAdCode": location["fish_ad_code"],
                "gps": location["gps"],
                "playId": 4,
            },
            args.channel,
        )
        save_json(args.output_dir, "22_sign_home_after", sign_home_after)

    template = load_publish_template(args.capture)
    image_url = template.get("imageInfoDOList", [{}])[0].get("url", "")
    upload_result = None
    if args.publish:
        upload_result = client.upload("/api/item/upload_image", account.uid, image_path)
        save_json(args.output_dir, "30_upload_image", upload_result)
        image_url = str(upload_result.get("url") or "")
        if not image_url:
            raise RuntimeError(f"upload did not return url: {upload_result}")

    title = f"测试服务发布多规格 {time.strftime('%m%d%H%M')}"
    short_title = "测试服务多规格"
    desc = "接口联调用测试服务，验证 H5 发布服务、多规格、签到和闲鱼币任务链路。"
    publish_payload = patch_publish_payload(
        template,
        location=location,
        image_url=image_url,
        image_path=image_path,
        title=title,
        short_title=short_title,
        desc=desc,
        spec_name="服务规格",
    )
    save_json(args.output_dir, "31_publish_payload", publish_payload)

    publish_result = None
    if args.publish:
        publish_result = mtop(
            client,
            account.uid,
            "mtop.idle.idleitem.publish",
            "1.0",
            publish_payload,
            args.channel,
        )
        save_json(args.output_dir, "32_publish_result", publish_result)
        if not ret_success(publish_result):
            print("publish failed; skip claim", file=sys.stderr)

    task_req = {
        "brand": "redmi",
        "fishAdCode": location["fish_ad_code"],
        "gps": location["gps"],
        "isWidgetInstalled": False,
        "osVersion": "13",
        "platform": "Android",
        "sceneId": 496,
    }
    task_list = mtop(
        client,
        account.uid,
        "mtop.taobao.idle.task.queryolivertasklist",
        "2.0",
        task_req,
        args.channel,
    )
    save_json(args.output_dir, "40_task_list", task_list)
    award_tasks = extract_award_tasks(task_list)
    save_json(args.output_dir, "41_award_tasks", award_tasks)

    claim_results = []
    if args.claim and (not args.publish or ret_success(publish_result)):
        for task in award_tasks:
            delivery_id = task.get("deliveryId")
            if delivery_id is None:
                continue
            claim = mtop(
                client,
                account.uid,
                "mtop.taobao.idle.task.getolivertaskbenefit",
                "1.0",
                {
                    "asac": args.asac,
                    "deliveryId": delivery_id,
                    "fishAdCode": location["fish_ad_code"],
                    "gps": location["gps"],
                    "sceneId": task.get("sceneId") or 496,
                },
                args.channel,
            )
            claim_results.append({"task": task, "response": claim})
        save_json(args.output_dir, "42_claim_results", claim_results)

    summary = {
        "account": account.__dict__,
        "location": location,
        "dryRun": not args.publish,
        "image": str(image_path),
        "serviceTypeApiOk": ret_success(service_list),
        "backendQueryOk": ret_success(backend_query),
        "serviceTypesExtracted": len(service_names),
        "signHomeSignedInBefore": (sign_data or {}).get("signedIn") if isinstance(sign_data, dict) else None,
        "signExecuted": sign_result is not None,
        "uploadOk": bool(upload_result and upload_result.get("success")),
        "publishOk": ret_success(publish_result) if publish_result is not None else None,
        "publishedItemId": (publish_result or {}).get("data", {}).get("itemId") if isinstance(publish_result, dict) else None,
        "awardTasks": award_tasks,
        "claimCount": len(claim_results),
        "outputDir": str(args.output_dir),
    }
    save_json(args.output_dir, "99_summary", summary)
    print(json.dumps(summary, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
