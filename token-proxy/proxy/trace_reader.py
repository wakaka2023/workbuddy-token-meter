"""读 WorkBuddy 本地 trace 记账内置模型用量（旁路，请求路径零侵入）。

数据源：~/.workbuddy/traces/<pid>/trace_*.json，WorkBuddy 每次 LLM 调用写一个文件。
排除法判断内置模型：powered by 值不匹配自定义黑名单 + toolOutput.model 不以 custom-local:
开头 → 视为内置。自定义模型仍走 token-proxy 记账，两侧写入同一个 _agg/_recent。
增量：state 文件记录已处理文件的 (mtime, size)，只解析新增/修改文件；/stats 惰性触发，
SCAN_TTL 内不重复扫。
"""
import glob
import json
import os
import re
import threading
import time
from datetime import datetime

from .aggregate import _apply_rec
from .config import DATA_DIR, RECENT_KEEP, _agg, _lock, _recent

TRACES_GLOB = os.path.join(
    os.path.expanduser("~"), ".workbuddy", "traces", "*", "trace_*.json"
)
MODELS_JSON = os.path.join(os.path.expanduser("~"), ".workbuddy", "models.json")
STATE_FILE = os.path.join(DATA_DIR, "traces_state.json")
SCAN_TTL = 30.0

_POWERED_RE = re.compile(r"powered by\s+([A-Za-z0-9\.\-_\(\): ]+?)(?:[\r\n\\\"]|$)", re.I)


def _is_builtin_name(powered):
    """模型名形态校验：必须大写/数字开头（排除 'the model named...' 这类提示词残留）。
    自定义模型排除全由 _custom_markers 精确匹配，此处不做家族级排除。"""
    if not powered:
        return False
    return bool(re.match(r"[A-Z0-9]", powered))

_scan_lock = threading.Lock()
_last_scan = 0.0
_state = {}          # path -> [mtime, size]
_dirty = False
# 走 8787 代理的自定义模型 id/name（排除集，避免与 usage.jsonl 重复记账）
_custom_markers = set()
# 自定义模型 name -> provider 显示名（label 用，如 "DeepSeek V4 Flash(B.AI)" -> "B.AI"）
_custom_provider = {}

_PROVIDER_HOST = {
    "api.b.ai": "B.AI",
    "b.ai": "B.AI",
}


def _provider_from_url(url):
    """从自定义模型 url 提取 provider 显示名：映射表优先，兜底取 host 末两级域名大写。"""
    if not url:
        return ""
    host = url.split("://")[-1].split("/")[0].split(":")[0].lower()
    if host in _PROVIDER_HOST:
        return _PROVIDER_HOST[host]
    parts = host.split(".")
    if len(parts) >= 2:
        return parts[-2].upper()
    return host.upper()


def _load_custom_markers():
    """读 models.json：`_custom_markers` 只收集走 8787 代理的模型 id/name 作为排除集
    （直连 api.b.ai 的自定义模型由 trace 统一记账，避免双通道漏记）；
    `_custom_provider` 收集全部自定义模型 name -> provider 名，用于 label 显示。"""
    global _custom_markers, _custom_provider
    markers, provider = set(), {}
    try:
        with open(MODELS_JSON, "r", encoding="utf-8") as f:
            for m in json.load(f):
                url = (m.get("url") or "").strip()
                mid = (m.get("id") or "").strip()
                mname = (m.get("name") or "").strip()
                if mname:
                    provider[mname] = _provider_from_url(url) or "自定义"
                if "127.0.0.1:8787" not in url and "localhost:8787" not in url:
                    continue  # 直连模型，收入 trace 记账
                if mid:
                    markers.add(mid)
                if mname:
                    markers.add(mname)
    except (OSError, json.JSONDecodeError, TypeError):
        pass
    _custom_markers, _custom_provider = markers, provider


def _load_state():
    global _state
    try:
        with open(STATE_FILE, "r", encoding="utf-8") as f:
            raw = json.load(f)
        _state = {k: list(v) for k, v in (raw.get("files") or {}).items()}
    except (OSError, json.JSONDecodeError, ValueError):
        _state = {}


def _save_state():
    try:
        with open(STATE_FILE, "w", encoding="utf-8") as f:
            json.dump({"files": _state}, f, ensure_ascii=False)
    except OSError:
        pass


def _to_local(iso):
    try:
        dt = datetime.fromisoformat(iso.replace("Z", "+00:00"))
        return dt.astimezone().strftime("%Y-%m-%d %H:%M:%S")
    except (ValueError, TypeError):
        return time.strftime("%Y-%m-%d %H:%M:%S")


def _powered_from_tool_input(ti):
    """提取 system prompt 首条消息的 powered by 值。toolInput 可能是 str（JSON 或截断）、
    dict 或裸数组。防止 user 消息里的 "powered by" 文本误匹配：优先解析 JSON 取首条 system
    content；截断无法解析时只搜前 2000 字符（system 永远是首条消息，一定在最前面）。"""
    raw = None
    if isinstance(ti, str):
        # 尝试解析 JSON（dict → get content, list → 首条 system）
        try:
            parsed = json.loads(ti)
        except (json.JSONDecodeError, UnicodeDecodeError):
            parsed = None
        if isinstance(parsed, dict):
            if isinstance(parsed.get("content"), str):
                raw = parsed["content"]
            elif isinstance(parsed.get("messages"), list):
                for m in parsed["messages"]:
                    if isinstance(m, dict) and m.get("role") == "system" and isinstance(m.get("content"), str):
                        raw = m["content"]
                        break
        elif isinstance(parsed, list):
            for m in parsed:
                if isinstance(m, dict) and m.get("role") == "system" and isinstance(m.get("content"), str):
                    raw = m["content"]
                    break
        if raw is None:
            # 截断/无法解析 → 只搜前 2000 字符（system 在开头）
            raw = ti[:2000]
    elif isinstance(ti, dict):
        raw = ti.get("content", "") if isinstance(ti.get("content"), str) else ""
    elif isinstance(ti, list):
        for m in ti:
            if isinstance(m, dict) and m.get("role") == "system" and isinstance(m.get("content"), str):
                raw = m["content"]
                break
    if not raw:
        return None
    m = _POWERED_RE.search(raw)
    return m.group(1).strip() if m else None


def _parse_tool_output(out):
    """解析 toolOutput，返回 (usage, model) 或 (None, None)。"""
    if not out:
        return None, None
    try:
        obj = json.loads(out)
    except (json.JSONDecodeError, UnicodeDecodeError, TypeError):
        return None, None
    if isinstance(obj, dict):
        obj = [obj]
    if not obj or not isinstance(obj[0], dict):
        return None, None
    return obj[0].get("usage") or None, obj[0].get("model") or ""


def _make_rec(sp, powered, usage):
    pdet = usage.get("prompt_tokens_details") or {}
    cdet = usage.get("completion_tokens_details") or {}
    # label 显示具体 provider：powered by 值命中自定义模型 name → 显示其 provider
    # （如 "DeepSeek V4 Flash(B.AI)" → "B.AI"）；否则是真正内置模型 → "内置"（cost 恒 0）
    label = _custom_provider.get(powered, "内置")
    return {
        "ts": _to_local(sp.get("startedAt", "")),
        "model": powered,
        "label": label,
        "prompt_tokens": usage.get("prompt_tokens", 0),
        "completion_tokens": usage.get("completion_tokens", 0),
        "reasoning_tokens": cdet.get("reasoning_tokens", 0) or 0,
        "cache_read_tokens": pdet.get("cached_tokens", 0) or 0,
        "duration_ms": sp.get("duration", 0) or 0,
        "ok": sp.get("status") == "ok",
        "cost": 0,  # 内置模型暂不统计价格
    }


def _scan_files(paths):
    """解析新增文件，返回待聚合 rec 列表；解析成功即更新 state（含无记录的，避免反复重扫）。"""
    global _dirty
    recs = []
    for p in paths:
        try:
            st = os.stat(p)
            key = [st.st_mtime, st.st_size]
        except OSError:
            continue
        if _state.get(p) == key:
            continue
        try:
            with open(p, "r", encoding="utf-8") as f:
                data = json.load(f)
        except (OSError, json.JSONDecodeError):
            continue  # 写入中/损坏：不记 state，下次重试
        for sp in data.get("spans", []):
            if sp.get("type") != "generation":
                continue
            powered = _powered_from_tool_input(sp.get("toolInput", ""))
            if not powered:
                continue
            # 排除法：powered by 值精确匹配自定义模型 id/name → 自定义（不在这里记账）
            if powered in _custom_markers:
                continue
            # 形态校验：排除 system prompt 里非模型名的文本（如 "the model named..."）
            if not _is_builtin_name(powered):
                continue
            usage, out_model = _parse_tool_output(sp.get("toolOutput", ""))
            if not usage:
                continue
            # 直连自定义模型（toolOutput.model 以 custom-local: 开头）已不在排除集，
            # 直接收入 trace 记账，model key 用 powered by 的原值（如 "DeepSeek V4 Flash(B.AI)"）
            recs.append(_make_rec(sp, powered, usage))
        _state[p] = key
        _dirty = True
    return recs


def scan_traces(force=False):
    """惰性增量扫描：SCAN_TTL 内不重复（force 强制重扫，启动全量用）。"""
    global _last_scan, _dirty, _state
    now = time.time()
    if not force and now - _last_scan < SCAN_TTL:
        return
    with _scan_lock:
        if not force and time.time() - _last_scan < SCAN_TTL:
            return
        _last_scan = time.time()
        if force:
            # 强制全量：清空 state，避免复用旧 state 跳过所有文件（聚合只在内存，重启必须重建）
            _state = {}
        if not _custom_markers:
            _load_custom_markers()
        try:
            paths = glob.glob(TRACES_GLOB)
        except OSError:
            return
        recs = _scan_files(paths)
        if recs:
            with _lock:
                for rec in recs:
                    _apply_rec(_agg, rec)
                    _recent.append(rec)
                # 按时间升序重排，保留最近 RECENT_KEEP 条（前端期望 records 正序，末尾=最新）
                _recent.sort(key=lambda r: r.get("ts", ""))
                del _recent[:-RECENT_KEEP]
        if _dirty:
            _save_state()
            _dirty = False


def start_background_scan():
    """启动时后台全量初扫（不阻塞代理启动）。注意：聚合只存内存，重启后必须全量重扫重建，
    不能复用上次的 state 跳过（那会导致重启后内置数据丢失）；state 仅用于会话内增量去重。"""
    threading.Thread(target=lambda: scan_traces(force=True), daemon=True).start()
