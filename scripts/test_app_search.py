#!/usr/bin/env python3
"""Test APP search channel and compare with capture data."""
import sqlite3, json, requests, time, sys, os
from urllib.parse import quote

DB = os.path.join(os.path.dirname(__file__), "..", "data", "accounts.db")
SIGN_URL = "https://sign.gooofish.shop/sign"
GATEWAY = "https://acs.m.goofish.com/gw"
CAPTURE_FILE = os.path.join(os.path.dirname(__file__), "..", "完整抓包数据.log")

UTDID = "afLbXwKpMzoDAPE7F1zUWMHl"
TTID = "10003993@fleamarket_android_7.25.70"
APPKEY = "21407387"
APP_VER = "7.25.70"
OAID = "e7140098-5e30-475b-ad27-8249a2ddb977"
UA = "MTOPSDK/3.1.1.7 (Android;13;Google;sdk_gphone64_arm64) DeviceType(Phone)"


def build_search_data(keyword="手机"):
    return json.dumps({
        "keyword": keyword,
        "pageNumber": 1, "page": 1, "rowsPerPage": 10,
        "fromFilter": False, "mainTab": True, "originJson": False,
        "activeSearch": False, "searchTabType": "SEARCH_TAB_MAIN",
        "bizFrom": "home", "searchReqFromPage": "xyHome",
        "apiName": "com.taobao.idlefish.search_implement.protocol.SearchResultReq",
        "disableHierarchicalSort": 0, "smartUIFilter": True,
        "supportFlexFilter": True, "resultListLastIndex": 0,
        "relateResultListLastIndex": 0, "relateResultPageNumber": 1,
        "extraFilterValue": '{"divisionList":[],"excludeMultiPlacesSellers":"0"}',
        "abConfigs": '{"isEnterResultNative":"true","isSearchResultNative":"true"}',
        "forceUseInputKeyword": False, "forceUseTppRepair": False,
        "fromKits": False, "fromLeaf": False, "fromShade": False,
        "fromSuggest": False,
        "searchReqFromActivatePagePart": "searchButton",
        "shadeBucketNum": "-1", "suggestBucketNum": "29",
    }, ensure_ascii=False)


def get_sign(uid, sid, data_str):
    t = str(int(time.time()))
    sb = {
        "api": "mtop.taobao.idlemtopsearch.search",
        "data": data_str, "version": "1.0", "t": t,
        "uid": uid, "sid": sid,
        "utdid": UTDID, "ttid": TTID, "appkey": APPKEY,
        "features": "27", "extdata": "openappkey=DEFAULT_AUTH",
        "lat": "0", "lng": "0", "timeout": 15,
    }
    resp = requests.post(SIGN_URL, json=sb, timeout=20)
    result = resp.json()
    if "error" in result:
        return None, f"sign error: {result['error']}"
    return result, None


def do_search(uid, sid, cookie_str, data_str, sign_result):
    sh = sign_result.get("signHeaders", {})
    rh = sign_result.get("requestHeaders", {})
    x_t = rh.get("x-t", str(int(time.time())))
    now_ms = int(time.time() * 1000)

    hdrs = {
        "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8",
        "User-Agent": quote(UA),
        "x-appkey": APPKEY,
        "x-ttid": quote(TTID),
        "x-app-ver": APP_VER,
        "x-utdid": UTDID,
        "x-uid": uid, "x-sid": sid, "x-t": x_t,
        "x-pv": "6.3", "x-features": "27",
        "x-nettype": "WIFI", "x-nq": "WIFI",
        "x-bx-version": "6.6.231201.33656539",
        "f-refer": "mtop",
        "x-extdata": quote("openappkey=DEFAULT_AUTH"),
        "x-location": "0%2C0",
        "x-custom-cache-uid": uid,
        "x-screen-level": "small", "x-magic_device": "0",
        "first_open": "0", "x-app-conf-v": "0",
        "oaid": OAID,
        "c-launch-info": f"0,0,{now_ms},{now_ms - 2000},2",
        "x-c-traceid": f"null{now_ms}0{now_ms % 100000}",
        "EagleEye-UserData": "spm-cnt=a2170.8011571.0.0&spm-url=a2170.7898022.6148798.1",
        "Cookie": cookie_str,
    }
    for k in ["x-sign", "x-sgext", "x-mini-wua"]:
        if sh.get(k):
            hdrs[k] = quote(sh[k])
    if sh.get("x-umt"):
        hdrs["x-umt"] = quote(sh["x-umt"])
        hdrs["umid"] = sh["x-umt"]

    body = f"data={quote(data_str)}"
    url = f"{GATEWAY}/mtop.taobao.idlemtopsearch.search/1.0/"
    resp = requests.post(url, headers=hdrs, data=body, timeout=20)
    bxpunish = resp.headers.get("bxpunish", "0")
    result = resp.json()
    if bxpunish == "1":
        result["_bxpunish"] = True
    return result, resp.status_code


def load_capture_search_headers():
    """Extract headers from capture data for comparison."""
    if not os.path.exists(CAPTURE_FILE):
        return None
    with open(CAPTURE_FILE) as f:
        for line in f:
            try:
                obj = json.loads(line.strip())
                if (obj.get("type") == "anet_request" and
                    "idlemtopsearch.search/1.0" in obj.get("url", "") and
                    "suggest" not in obj.get("url", "")):
                    return obj.get("headers", {})
            except:
                pass
    return None


def compare_with_capture(our_headers):
    """Compare our headers with capture data."""
    capture_hdrs = load_capture_search_headers()
    if not capture_hdrs:
        return "No capture data found"

    diffs = []
    skip = {"x-sign", "x-sgext", "x-mini-wua", "x-umt", "umid",
            "x-t", "x-uid", "x-sid", "x-c-traceid", "c-launch-info",
            "Cookie", "Content-Length", "x-custom-cache-uid",
            "EagleEye-UserData", "x-falco-id"}

    for k, v in capture_hdrs.items():
        k_lower = k.lower()
        if k in skip:
            continue
        our_v = our_headers.get(k, our_headers.get(k_lower, "MISSING"))
        if our_v == "MISSING":
            diffs.append(f"  MISSING {k}: capture={v}")
        elif quote(str(our_v)) != quote(str(v)) and str(our_v) != str(v):
            diffs.append(f"  DIFF {k}: capture={v} | ours={our_v}")

    return "\n".join(diffs) if diffs else "Headers match capture data"


def main():
    conn = sqlite3.connect(DB)
    rows = conn.execute(
        "SELECT uid, cookies FROM accounts WHERE status = 'active' ORDER BY RANDOM() LIMIT 3"
    ).fetchall()

    if not rows:
        print("FAIL: No active accounts")
        return False

    data_str = build_search_data()
    success = False

    for uid, cookies_raw in rows:
        cookies = json.loads(cookies_raw) if cookies_raw else {}
        sid = cookies.get("cookie2", "")
        cookie_str = "; ".join(f"{k}={v}" for k, v in cookies.items())

        sign_result, err = get_sign(uid, sid, data_str)
        if err:
            print(f"  uid={uid}: {err}")
            continue

        result, status = do_search(uid, sid, cookie_str, data_str, sign_result)
        ret = result.get("ret", [])
        ret_str = ret[0] if ret else "NO_RET"

        if "SUCCESS" in ret_str:
            data = result.get("data", {})
            items = data.get("resultList", []) if isinstance(data, dict) else []
            print(f"  uid={uid}: SUCCESS ({len(items)} items)")
            success = True
            break
        else:
            punished = " [bxpunish=1]" if result.get("_bxpunish") else ""
            print(f"  uid={uid}: HTTP {status} | {ret_str}{punished}")
            if result.get("_bxpunish"):
                print("  >> Device is under BotX anti-bot punishment. CAPTCHA required to clear.")
                print("  >> All accounts will fail — skipping remaining.")
                break

    if not success:
        print("\nDiagnostics — comparing with capture data:")
        print(compare_with_capture({
            "x-appkey": APPKEY,
            "x-ttid": quote(TTID),
            "x-app-ver": APP_VER,
            "x-utdid": UTDID,
            "x-pv": "6.3",
            "x-features": "27",
            "x-nettype": "WIFI",
            "x-nq": "WIFI",
            "x-bx-version": "6.6.231201.33656539",
            "f-refer": "mtop",
            "x-extdata": quote("openappkey=DEFAULT_AUTH"),
            "x-location": "0%2C0",
            "x-screen-level": "small",
            "x-magic_device": "0",
            "first_open": "0",
            "x-app-conf-v": "0",
            "oaid": OAID,
            "user-agent": quote(UA),
            "content-type": "application/x-www-form-urlencoded;charset=UTF-8",
        }))

        # Also test non-search API to verify account validity
        uid, cookies_raw = rows[0]
        cookies = json.loads(cookies_raw) if cookies_raw else {}
        sid = cookies.get("cookie2", "")
        cookie_str = "; ".join(f"{k}={v}" for k, v in cookies.items())
        test_data = json.dumps({"tabFrom": "myIdle", "scene": "myIdlePage",
                                "shopId": "", "userId": uid}, ensure_ascii=False)
        t = str(int(time.time()))
        sb = {
            "api": "mtop.idle.user.page.my.adapter",
            "data": test_data, "version": "1.0", "t": t,
            "uid": uid, "sid": sid,
            "utdid": UTDID, "ttid": TTID, "appkey": APPKEY,
            "features": "27", "extdata": "openappkey=DEFAULT_AUTH",
            "lat": "0", "lng": "0", "timeout": 15,
        }
        sr = requests.post(SIGN_URL, json=sb, timeout=20).json()
        sh = sr.get("signHeaders", {})
        rh = sr.get("requestHeaders", {})
        now_ms = int(time.time() * 1000)
        hdrs = {
            "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8",
            "User-Agent": quote(UA),
            "x-appkey": APPKEY, "x-ttid": quote(TTID),
            "x-app-ver": APP_VER, "x-utdid": UTDID,
            "x-uid": uid, "x-sid": sid,
            "x-t": rh.get("x-t", t),
            "x-pv": "6.3", "x-features": "27",
            "x-nettype": "WIFI", "x-nq": "WIFI",
            "x-bx-version": "6.6.231201.33656539",
            "f-refer": "mtop",
            "x-extdata": quote("openappkey=DEFAULT_AUTH"),
            "x-location": "0%2C0",
            "x-screen-level": "small", "x-magic_device": "0",
            "first_open": "0", "x-app-conf-v": "0",
            "oaid": OAID,
            "c-launch-info": f"0,0,{now_ms},{now_ms - 2000},2",
            "x-c-traceid": f"null{now_ms}0{now_ms % 100000}",
            "Cookie": cookie_str,
        }
        for k in ["x-sign", "x-sgext", "x-mini-wua"]:
            if sh.get(k):
                hdrs[k] = quote(sh[k])
        if sh.get("x-umt"):
            hdrs["x-umt"] = quote(sh["x-umt"])
            hdrs["umid"] = sh["x-umt"]
        body = f"data={quote(test_data)}"
        url = f"{GATEWAY}/mtop.idle.user.page.my.adapter/1.0/"
        resp = requests.post(url, headers=hdrs, data=body, timeout=20)
        r = resp.json()
        print(f"\nNon-search API test (my.adapter): {r.get('ret', ['?'])}")

    return success


if __name__ == "__main__":
    ts = time.strftime("%Y-%m-%d %H:%M:%S")
    print(f"=== APP Search Test @ {ts} ===")
    ok = main()
    print(f"\nResult: {'PASS' if ok else 'FAIL'}")
    sys.exit(0 if ok else 1)
