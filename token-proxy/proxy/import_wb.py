"""一键导入 WorkBuddy 自定义模型（~/.workbuddy/models.json → token-proxy config.json）。

v0.2.1 渠道+模型双实体：
- 按 url host 归渠道（b.ai → 渠道，aliyun → 渠道），渠道管理 base/代理/key 池
- 模型行引用渠道 + 指定 key（模型级 key 选择），不存 key 明文
- url 指向 8787 代理的模型 → 跳过（已由代理转发，渠道归属由 config 模型行决定）
- 台账登记：origin_url 首次快照永不覆盖，route=direct/proxy
"""
import json
import os

from . import ledger
from .config import CONFIG_FILE, MODELS_JSON

PROXY_HOSTS = ("127.0.0.1:8787", "localhost:8787")


def channel_from_url(url):
    """url -> (渠道名, base_url)。host 末两级小写做渠道名；base = scheme://host[:port]。"""
    if "://" not in url:
        return None, None
    scheme, rest = url.split("://", 1)
    host_port = rest.split("/")[0]
    host = host_port.split(":")[0].lower()
    parts = host.split(".")
    name = ".".join(parts[-2:]) if len(parts) >= 2 else host
    return name, f"{scheme}://{host_port}"


def _read_workbuddy_models():
    try:
        with open(MODELS_JSON, "r", encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, json.JSONDecodeError, ValueError):
        return None
    return data if isinstance(data, list) else None


def _read_cfg():
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            return json.load(f)
    except (OSError, json.JSONDecodeError):
        return {"channels": {}, "models": {}}


def _find_key_in_pool(keys, raw_key):
    """在 key 池中按值查找，返回 (key_id, found)。"""
    for k in keys:
        if k.get("key") == raw_key:
            return k.get("id"), True
    return None, False


def import_workbuddy():
    """执行导入。返回 (ok, summary, registry_updates)。summary 含导入/跳过计数。"""
    models = _read_workbuddy_models()
    if models is None:
        return False, {"error": f"cannot read {MODELS_JSON}"}, []
    if not models:
        return True, {"imported": 0, "skipped": 0, "channels": {},
                      "message": "WorkBuddy 中未配置自定义模型"}, []

    cfg = _read_cfg()
    cfg.setdefault("channels", {})
    cfg.setdefault("models", {})
    cfg_channels = cfg["channels"]
    cfg_models = cfg["models"]
    reg = {r.get("name"): r for r in ledger.models()}

    registry = []
    imported = skipped = 0
    channels_summary = {}
    for m in models:
        url = (m.get("url") or "").strip()
        mid = (m.get("id") or "").strip()
        mname = (m.get("name") or "").strip() or mid
        raw_key = (m.get("apiKey") or "").strip()
        if not url or not mname:
            continue

        is_proxy = any(h in url for h in PROXY_HOSTS)

        if is_proxy:
            # 走代理的模型：渠道归属由 config 模型行决定，跳过渠道创建
            registry.append({
                "name": mname, "mid": mid, "route": "proxy",
                "channel": None, "key_ref": None,
            })
            skipped += 1
            continue

        ch_name, ch_base = channel_from_url(url)
        if not ch_name:
            continue

        # 渠道行：创建或更新
        ch = cfg_channels.setdefault(ch_name, {})
        ch.setdefault("base", ch_base)
        ch.setdefault("label", ch_name)
        ch.setdefault("proxy", "auto")
        keys = ch.setdefault("keys", [])
        key_id = None
        if raw_key:
            existing_id, found = _find_key_in_pool(keys, raw_key)
            if found:
                key_id = existing_id
            else:
                key_id = f"k{len(keys) + 1}"
                keys.append({"id": key_id, "name": ch_name, "key": raw_key})
                ch.setdefault("activeKey", key_id)

        # 模型行
        row = cfg_models.setdefault(mid, {})
        row["channel"] = ch_name
        row["name"] = mname
        if key_id and row.get("key") != key_id:
            row["key"] = key_id
        row.setdefault("price", row.get("price") or {})

        # 台账
        origin = (reg.get(mname) or {}).get("origin_url") or url
        if key_id:
            key_ref = f"{ch_name}/{key_id}"
        else:
            key_ref = None
        ledger.upsert_model(mname, ch_name, origin, url, "direct", key_ref)
        registry.append({
            "name": mname, "mid": mid, "route": "direct",
            "channel": ch_name, "key_ref": key_ref,
        })
        imported += 1
        channels_summary[ch_name] = channels_summary.get(ch_name, 0) + 1

    # 清理：如果导入后某渠道空了（所有模型都被删了），保留渠道行（用户可手动删）

    try:
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            json.dump(cfg, f, ensure_ascii=False, indent=2)
    except OSError as e:
        return False, {"error": str(e)}, registry

    ledger.log("import", f"imported {imported}, skipped {skipped}, channels {len(channels_summary)}")
    return True, {
        "imported": imported,
        "skipped": skipped,
        "channels": channels_summary,
    }, registry