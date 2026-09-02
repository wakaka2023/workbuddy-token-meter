"""模型路由开关：改 WorkBuddy models.json 的模型 url（direct ↔ proxy 互切）。

- proxy：url → http://127.0.0.1:8787/v1（记账走代理 usage.jsonl，trace 排除）
- direct：url → 台账登记表的 origin_url（首次导入时快照，永不覆盖）
- 改前备份 models.json.bak-时间戳（DATA_DIR 下，保留最近 5 份）
- 幂等：已是目标路由时 no change
- 改动记入台账 changelog（op=route_change）；models.json 是 WorkBuddy 活配置，
  改后需重启 WorkBuddy 生效（由接口返回提示）
"""
import glob
import json
import os
import shutil
import time

from . import ledger
from .config import DATA_DIR, MODELS_JSON

PROXY_HOSTS = ("127.0.0.1:8787", "localhost:8787")
PROXY_URL = "http://127.0.0.1:8787/v1"
_BAK_KEEP = 5


def _backup():
    """改前备份 models.json，保留最近 _BAK_KEEP 份。"""
    if not os.path.exists(MODELS_JSON):
        return None
    bak = f"{MODELS_JSON}.bak-{time.strftime('%Y%m%d-%H%M%S')}"
    try:
        shutil.copy(MODELS_JSON, bak)
        olds = sorted(glob.glob(f"{MODELS_JSON}.bak-*"))
        for old in olds[:-_BAK_KEEP]:
            try:
                os.remove(old)
            except OSError:
                pass
        return bak
    except OSError:
        return None


def switch_route(name, target_route):
    """切换模型路由。name 匹配 models.json 的 name 或 id。
    返回 (ok, message, new_url)。"""
    if target_route not in ("proxy", "direct"):
        return False, "target must be 'proxy' or 'direct'", None
    try:
        with open(MODELS_JSON, "r", encoding="utf-8") as f:
            models = json.load(f)
    except (OSError, json.JSONDecodeError):
        return False, f"cannot read {MODELS_JSON}", None
    m = next((x for x in models if (x.get("name") == name or x.get("id") == name)), None)
    if not m:
        return False, f"model '{name}' not found in WorkBuddy models", None
    url = (m.get("url") or "").strip()
    is_proxy = any(h in url for h in PROXY_HOSTS)

    if target_route == "proxy" and not is_proxy:
        _backup()
        m["url"] = PROXY_URL
    elif target_route == "direct" and is_proxy:
        reg = next((r for r in ledger.models() if r.get("name") == name), None)
        origin = reg and reg.get("origin_url")
        if not origin:
            return False, f"no origin_url recorded for '{name}' (import first)", None
        _backup()
        m["url"] = origin
    else:
        return True, f"already {target_route}, no change", url

    try:
        with open(MODELS_JSON, "w", encoding="utf-8") as f:
            json.dump(models, f, ensure_ascii=False, indent=2)
    except OSError as e:
        return False, f"write failed: {e}", None
    ledger.upsert_model(name, None, None, m["url"], target_route, None)
    ledger.log("route_change", f"{name} -> {target_route}")
    return True, f"switched to {target_route}, restart WorkBuddy to take effect", m["url"]


def list_models():
    """WorkBuddy 自定义模型概览（key 脱敏，供模型管理面板）。"""
    try:
        with open(MODELS_JSON, "r", encoding="utf-8") as f:
            models = json.load(f)
    except (OSError, json.JSONDecodeError):
        return []
    reg = {r.get("name"): r for r in ledger.models()}
    out = []
    for m in models:
        name = (m.get("name") or "").strip() or (m.get("id") or "").strip()
        url = (m.get("url") or "").strip()
        route = "proxy" if any(h in url for h in PROXY_HOSTS) else "direct"
        ak = m.get("apiKey") or ""
        out.append({
            "id": m.get("id", ""), "name": name, "url": url, "route": route,
            "has_key": bool(ak),
            "origin_url": (reg.get(name) or {}).get("origin_url"),
        })
    return out
