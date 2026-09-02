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
# WorkBuddy 自定义模型配置（一键导入 / 路由开关读写）
MODELS_JSON = os.path.join(os.path.expanduser("~"), ".workbuddy", "models.json")

# 运行模式：service（扫描/统计可用，转发停用）→ full（配置 key 后转发启用）
# 环境变量 TOKEN_PROXY_MODE=full 可强制 full（打包版 widget 拉起时按需传）
MODE = "full" if os.environ.get("TOKEN_PROXY_MODE", "").strip().lower() == "full" else "service"
_started_at = time.time()
_forwarded = 0  # 本次会话转发请求数

RETRYABLE = {429, 500, 502, 503, 504}
MAX_ATTEMPTS = 3
BACKOFF = 1.5
FLUSH_INTERVAL = 10
RECENT_KEEP = 200

# 连接复用：模块级 Session
_session = requests.Session()

# 按渠道动态解析转发代理（config.channels[].proxy 字段，用户可在设置面板改）：
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


def _proxy_settings(proxy_mode):
    """按渠道配置的 proxy 字段解析 requests 代理参数。"""
    mode = (proxy_mode or "auto").strip().lower()
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
_config = {"channels": {}, "models": {}}


def _load_config():
    global _config
    try:
        with open(CONFIG_FILE, "r", encoding="utf-8") as f:
            _config = json.load(f)
    except (OSError, json.JSONDecodeError):
        _config = {"channels": {}, "models": {}}
    _config.setdefault("channels", {})
    _config.setdefault("models", {})


def _channel(name):
    """渠道配置行；未登记返回空 dict。"""
    return (_config.get("channels") or {}).get(name) or {}


def _model(model):
    """模型配置行；未登记返回空 dict。若 model 不是 models 字典的键，按 name 字段反向匹配（兼容 WorkBuddy 用显示名请求的场景）。"""
    models = _config.get("models") or {}
    m = models.get(model)
    if m is not None:
        return m
    for mid, mv in models.items():
        if mv.get("name") == model:
            return mv
    return {}


def _resolve_model_id(model):
    """请求 model 名 -> 配置模型键（id）。若请求用 name（显示名）反查命中，返回真正的 id 键，
    转发时据此把 body.model 替换成 id，避免上游收到显示名而 404。未命中返回原 model。"""
    models = _config.get("models") or {}
    if model in models:
        return model
    for mid, mv in models.items():
        if mv.get("name") == model:
            return mid
    return model


def _model_price(model):
    m = _model(model)
    p = m.get("price") or {}
    return p.get("input", 0), p.get("output", 0), p.get("cache_read", 0)


def _model_base(model):
    """模型 base url = 所属渠道的 base。"""
    m = _model(model)
    ch = _channel(m.get("channel") or "")
    return ch.get("base") or ""


# start of host display-name mapping (B.AI etc.)
_PROVIDER_HOST = {
    "api.b.ai": "B.AI",
    "b.ai": "B.AI",
}


def _provider_display(base):
    """从渠道 base url 推导供应商显示名：映射表优先（api.b.ai→B.AI），
    其余返回空（让调用方回退渠道 label）。"""
    if not base:
        return ""
    host = base.split("://")[-1].split("/")[0].split(":")[0].lower()
    return _PROVIDER_HOST.get(host, "")


def _model_label(model):
    """供应商显示名（徽章/统计用）：渠道 base 能映射已知 provider（B.AI）用它；
    否则用渠道 label；全无才回退模型 name。"""
    m = _model(model)
    ch = _channel(m.get("channel") or "")
    disp = _provider_display(ch.get("base") or "")
    if disp:
        return disp
    return ch.get("label") or ch.get("name") or m.get("name") or m.get("label") or model


def _model_proxy(model):
    """模型代理策略 = 所属渠道的 proxy。"""
    m = _model(model)
    ch = _channel(m.get("channel") or "")
    return ch.get("proxy") or "auto"


def _model_defaults(model):
    return _model(model).get("defaults") or {}


def _model_key(model):
    """模型激活 key 明文。

    模型可指定 key id（model.key 字段），未指定时跟随渠道的 activeKey。
    渠道无 key 时返回 None（转发时回退客户端 Authorization）。
    """
    m = _model(model)
    ch_name = m.get("channel") or ""
    ch = _channel(ch_name)
    if not ch:
        return None
    keys = ch.get("keys") or []
    if not keys:
        return None
    # 模型指定 key id
    mk = m.get("key")
    if mk:
        for k in keys:
            if k.get("id") == mk:
                return k.get("key")
    # 渠道激活 key
    ak = ch.get("activeKey")
    if ak:
        for k in keys:
            if k.get("id") == ak:
                return k.get("key")
    return keys[0].get("key")


def _has_any_key():
    """任一渠道配置了真实 key → 可启动 full 模式。"""
    for ch in (_config.get("channels") or {}).values():
        for k in (ch.get("keys") or []):
            if k.get("key") and "REPLACE" not in k["key"]:
                return True
    return False


def mode_status():
    """模式状态（/status 用）。"""
    return {
        "mode": MODE,
        "port": PORT,
        "forwarded": _forwarded,
        "uptime": int(time.time() - _started_at),
        "has_key": _has_any_key(),
    }


def _mask_key(key):
    if not key or len(key) <= 10:
        return key or ""
    return key[:6] + "..." + key[-4:]


def _is_masked_key(key):
    if not key:
        return True
    if "..." in key:
        return True
    return len(key) < 30


def _masked_channels(channels):
    """深拷贝渠道并把各渠道 keys 脱敏。"""
    out = {}
    for cname, ch in (channels or {}).items():
        cp = dict(ch)
        if ch.get("keys"):
            cp["keys"] = [
                {"id": k.get("id"), "name": k.get("name", ""), "key": _mask_key(k.get("key"))}
                for k in ch["keys"]
            ]
        out[cname] = cp
    return out


def _masked_models(models):
    """模型层无 key 明文，直接深拷贝返回。"""
    return json.loads(json.dumps(models or {}))


def _merge_channels_keys(cur_channels, new_channels):
    """PUT /config 合并 channels：keys 脱敏回显按 (渠道,keyId) 找回真实 key。

    返回 (merged, dropped)：dropped 为「掩码 key 且当前配置中找不到真实值」而被丢弃的
    (渠道, keyId) 列表。
    """
    merged = {}
    dropped = []
    for cname, ch in (new_channels or {}).items():
        cur = (cur_channels or {}).get(cname) or {}
        cp = dict(cur)
        for k, v in ch.items():
            if k != "keys":
                cp[k] = v
        cur_keys = {}
        for k in (cur.get("keys") or []):
            cur_keys[k.get("id")] = k
        new_keys = []
        for k in (ch.get("keys") or []):
            kk = dict(k)
            if _is_masked_key(kk.get("key", "")):
                real = cur_keys.get(kk.get("id"), {}).get("key")
                if real:
                    kk["key"] = real
                else:
                    dropped.append((cname, kk.get("id")))
                    continue
            new_keys.append(kk)
        if new_keys:
            cp["keys"] = new_keys
        elif cur.get("keys"):
            cp["keys"] = cur["keys"]
        merged[cname] = cp
    return merged, dropped


def _merge_models_fields(cur_models, new_models):
    """PUT /config 合并 models：模型层无 key 实体，简单字段合并。"""
    merged = {}
    for mid, m in (new_models or {}).items():
        cur = (cur_models or {}).get(mid) or {}
        cp = dict(cur)
        for k, v in m.items():
            cp[k] = v
        merged[mid] = cp
    return merged