"""模型配置数据库：当前所有自定义模型的状态登记表（替代原"版本链快照"方案）。

- 每个自定义模型一条记录：渠道、原始直连 url（首次登记快照，永不覆盖）、当前 url、
  路由状态（direct/proxy）、key 引用（渠道/keyId，不存明文——key 明文唯一真源是 config.json）。
- changelog 为轻量操作日志（导入/路由切换/改配置/key 变更），只记事件不存快照。
- 不提供回退（用户要恢复直接重新一键导入即可）；模型登记表即当前状态快照。
- 文件：DATA_DIR/config-ledger.json，不入库（%APPDATA% 数据目录）。
"""
import json
import os
import threading
import time

from .config import DATA_DIR

LEDGER_FILE = os.path.join(DATA_DIR, "config-ledger.json")

_lock = threading.Lock()
_data = {"models": [], "changelog": []}
_loaded = False


def _now():
    return time.strftime("%Y-%m-%d %H:%M:%S")


def _load():
    global _data, _loaded
    if _loaded:
        return
    migrated = False
    try:
        with open(LEDGER_FILE, "r", encoding="utf-8") as f:
            raw = json.load(f)
        if isinstance(raw, dict):
            _data = {
                "models": raw.get("models") or [],
                "changelog": raw.get("changelog") or [],
            }
            # 迁移：旧版本链时代的 changelog 用 source/version 字段，归一化为 detail
            for c in _data["changelog"]:
                if "detail" not in c and "source" in c:
                    c["detail"] = c.get("source", "")
                    c.pop("version", None)
                    migrated = True
            # 清理旧版本链残留字段（versions/currentVersion 不再使用）
            if "versions" in raw or "currentVersion" in raw:
                migrated = True
    except (OSError, json.JSONDecodeError, ValueError):
        pass
    _loaded = True
    if migrated:
        _save()


def _save():
    try:
        with open(LEDGER_FILE, "w", encoding="utf-8") as f:
            json.dump(_data, f, ensure_ascii=False, indent=2)
    except OSError:
        pass


def _append_log(op, detail):
    """内部：追加 changelog（调用方需持锁）。"""
    _data["changelog"].append({"ts": _now(), "op": op, "detail": detail})
    # changelog 控制体积：保留最近 200 条
    del _data["changelog"][:-200]


def log(op, detail):
    """记录一条操作日志（轻量，不存快照）。"""
    with _lock:
        _load()
        _append_log(op, detail)
        _save()


def upsert_model(name, channel, origin_url, current_url, route, key_ref):
    """模型登记表：原始直连 url 永不覆盖（首次登记为准）。
    返回 True=新增 / False=更新。"""
    with _lock:
        _load()
        for m in _data["models"]:
            if m.get("name") == name:
                m["current_url"] = current_url
                m["route"] = route
                if key_ref:
                    m["key_ref"] = key_ref
                if not m.get("origin_url") and origin_url:
                    m["origin_url"] = origin_url
                m["updated_at"] = _now()
                _save()
                return False
        _data["models"].append({
            "name": name, "channel": channel,
            "origin_url": origin_url or current_url,
            "current_url": current_url, "route": route, "key_ref": key_ref,
            "updated_at": _now(),
        })
        _save()
        return True


def models():
    """模型登记表（副本）。"""
    with _lock:
        _load()
        return json.loads(json.dumps(_data["models"]))


def changelog():
    with _lock:
        _load()
        return json.loads(json.dumps(_data["changelog"][-50:]))


def summary():
    with _lock:
        _load()
        return {"models": len(_data["models"]), "logs": len(_data["changelog"])}
