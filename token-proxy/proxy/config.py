"""路径/常量/连接与共享状态。"""
import json
import os
import shutil
import socket
import sys
import threading
import time

import requests

PORT = 8787
# widget 拉起 exe 时用 TOKEN_PROXY_DATA_DIR 指向 %APPDATA%；未设置回退 exe/脚本目录
DATA_DIR = os.environ.get("TOKEN_PROXY_DATA_DIR", "").strip() or (
    os.path.dirname(sys.executable) if getattr(sys, "frozen", False)
    else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
)
os.makedirs(DATA_DIR, exist_ok=True)
# 首次运行：数据目录缺 config.json 时从 exe 同目录复制默认配置
if getattr(sys, "frozen", False) and not os.path.exists(os.path.join(DATA_DIR, "config.json")):
    bundled = os.path.join(os.path.dirname(sys.executable), "config.json")
    if os.path.exists(bundled):
        shutil.copy(bundled, os.path.join(DATA_DIR, "config.json"))
USAGE_JSONL = os.path.join(DATA_DIR, "usage.jsonl")
USAGE_FILE = os.path.join(DATA_DIR, "usage.json")  # 旧格式，启动时迁移一次
LOG_FILE = os.path.join(DATA_DIR, "proxy.log")
CONFIG_FILE = os.path.join(DATA_DIR, "config.json")

# 路径前缀 -> provider 名（base/label 运行时从 _config 读，支持热加载）
ROUTES = {
    "/v1/chat/completions": "b.ai",
    "/aliyun/chat/completions": "aliyun",
}
DEFAULT_ROUTE = "/v1/chat/completions"
RETRYABLE = {429, 500, 502, 503, 504}
MAX_ATTEMPTS = 3
BACKOFF = 1.5
FLUSH_INTERVAL = 10
RECENT_KEEP = 200

# 连接复用：模块级 Session
_session = requests.Session()

# 按渠道动态解析转发代理（config.providers[].proxy 字段，用户可在设置面板改）：
#   auto   -> 本机 7897 探测(结果缓存 30s)：在就走 VPN，不在就直连
#   direct -> 强制直连（显式禁用一切代理）
#   http(s)://host:port -> 强制走指定代理
# 本机回环 connect 端口在/不在均即时返回，探测开销微秒级，不拖慢请求
DEFAULT_PROXY = "http://127.0.0.1:7897"
_PROBE_CACHE = {}  # port -> {"ts": float, "up": bool}
_PROBE_TTL = 30.0


def _port_up(port, host="127.0.0.1", timeout=0.2):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(timeout)
    try:
        s.connect((host, port))
        return True
    except OSError:
        return False
    finally:
        s.close()


def _probe(port):
    now = time.time()
    c = _PROBE_CACHE.get(port)
    if c and now - c["ts"] < _PROBE_TTL:
        return c["up"]
    up = _port_up(port)
    _PROBE_CACHE[port] = {"ts": now, "up": up}
    return up


def _resolve_proxies(provider_name):
    p = (_config.get("providers") or {}).get(provider_name) or {}
    mode = (p.get("proxy") or "auto").strip().lower()
    if mode == "direct":
        return {"http": None, "https": None}
    if mode.startswith(("http://", "https://")):
        return {"http": mode, "https": mode}
    # auto：7897 探测 -> 直连（旧配置里的 env 视为 auto）
    if _probe(7897):
        return {"http": DEFAULT_PROXY, "https": DEFAULT_PROXY}
    return {"http": None, "https": None}

_lock = threading.Lock()
_agg = {"total": {}, "by_model": {}, "by_day": {}, "by_model_day": {}, "last_success": {}}
_recent = []
_pending = []
_log_buf = []
_config = {"models": {}, "providers": {}}


def _load_config():
    global _config
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            _config = json.load(f)
    except (OSError, json.JSONDecodeError):
        _config = {"models": {}, "providers": {}}


def _model_price(model):
    m = (_config.get("models") or {}).get(model) or {}
    p = m.get("price") or {}
    return p.get("input", 0), p.get("output", 0), p.get("cache_read", 0)


def _provider(name):
    p = (_config.get("providers") or {}).get(name) or {}
    if not p.get("base"):
        return None
    return p["base"], p.get("label", name)


def _model_defaults(model):
    m = (_config.get("models") or {}).get(model) or {}
    return m.get("defaults") or {}


def _active_key(name):
    p = (_config.get("providers") or {}).get(name) or {}
    keys = p.get("keys") or []
    if not keys:
        return None
    ak = p.get("activeKey")
    for k in keys:
        if k.get("id") == ak:
            return k.get("key")
    return keys[0].get("key")


def _mask_key(key):
    if not key or len(key) <= 10:
        return key or ""
    return key[:6] + "..." + key[-4:]


def _masked_providers(providers):
    """深拷贝 providers 并把 key 脱敏，防止完整 key 泄露到前端/日志。"""
    out = {}
    for name, p in (providers or {}).items():
        cp = dict(p)
        if p.get("keys"):
            cp["keys"] = [
                {"id": k.get("id"), "name": k.get("name", ""), "key": _mask_key(k.get("key"))}
                for k in p["keys"]
            ]
        out[name] = cp
    return out


def _is_masked_key(key):
    if not key:
        return True
    if "..." in key:
        return True
    return len(key) < 30


def _merge_providers_keys(cur_providers, new_providers):
    """PUT /config 合并 providers：保留未传字段（如 proxy），keys 脱敏回显按 id 找回真实 key。"""
    merged = {}
    for name, p in (new_providers or {}).items():
        cur = (cur_providers or {}).get(name) or {}
        cp = dict(cur)
        for k, v in p.items():
            if k != "keys":
                cp[k] = v
        cur_keys = {}
        for k in (cur.get("keys") or []):
            cur_keys[k.get("id")] = k
        new_keys = []
        for k in (p.get("keys") or []):
            kk = dict(k)
            if _is_masked_key(kk.get("key", "")):
                real = cur_keys.get(kk.get("id"), {}).get("key")
                if real:
                    kk["key"] = real
            new_keys.append(kk)
        if new_keys:
            cp["keys"] = new_keys
        merged[name] = cp
    return merged
