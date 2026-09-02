"""trace 扫描缓存：精简记录按天分片落盘 + 增量 upsert + 聚合。

目标：
- 初次全量解析 ~/.workbuddy/traces 的 generation span，只保留统计字段（无请求体/无 key），
  写 DATA_DIR/trace-cache/YYYY-MM-DD.jsonl。
- 后续扫描仅 stat 对比（mtime/size），只解析新增/变更文件；记录键 traceId+spanId 全局唯一
  （实测 500 文件 957 generation 零重复），变更文件重扫不会重复计数（upsert 语义）。
- 启动时 aggregate_from_cache() 读缓存秒级重建 _agg（原方案重启全量重扫 ~8s → <0.5s）。
- 缓存是派生物，可随时"重新全量扫描"重建；用户删原始 trace 不影响已有账本。
"""
import glob
import json
import os
import threading
import time

from .config import DATA_DIR, _agg, _lock, _recent
from .trace_reader import (
    TRACES_GLOB,
    _is_builtin_name,
    _load_custom_markers,
    _parse_tool_output,
    _powered_from_tool_input,
    _to_local,
)

CACHE_DIR = os.path.join(DATA_DIR, "trace-cache")
STATE_FILE = os.path.join(CACHE_DIR, "_state.json")

_scan_lock = threading.Lock()
_state = {}          # trace 文件绝对路径 -> [mtime, size]
_seen_keys = None    # set[(traceId, spanId)]，懒加载自缓存
_dirty = False
_last_scan = 0.0
SCAN_TTL = 30.0
# 进度状态（/scan/progress 用）
_progress = {"running": False, "total": 0, "scanned": 0, "records": 0, "done": True}

RECENT_KEEP = 200


def set_scan_ttl(seconds: float):
    """动态调整 trace 扫描间隔（与前端轮询频率同步，全局统一）。"""
    global SCAN_TTL
    SCAN_TTL = max(1.0, float(seconds))


def _cache_files():
    return glob.glob(os.path.join(CACHE_DIR, "20*.jsonl"))


def _load_seen_keys():
    """从缓存加载全部 (traceId, spanId) 键，用于 upsert 去重。"""
    global _seen_keys
    if _seen_keys is not None:
        return _seen_keys
    keys = set()
    for fp in _cache_files():
        try:
            with open(fp, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        r = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    keys.add((r.get("traceId"), r.get("spanId")))
        except OSError:
            continue
    _seen_keys = keys
    return keys


def _load_state():
    global _state
    try:
        with open(STATE_FILE, "r", encoding="utf-8") as f:
            _state = {k: list(v) for k, v in (json.load(f).get("files") or {}).items()}
    except (OSError, json.JSONDecodeError, ValueError):
        _state = {}


def _save_state():
    try:
        with open(STATE_FILE, "w", encoding="utf-8") as f:
            json.dump({"files": _state}, f, ensure_ascii=False)
    except OSError:
        pass


def _make_cache_rec(sp, powered, usage, fname, provider_map):
    """精简记录：统计字段 + 去重键，不含请求体。ts 转本地时间。
    label = provider 名（如 B.AI）；不在 models.json 的为内置模型 → "内置"。"""
    pdet = usage.get("prompt_tokens_details") or {}
    cdet = usage.get("completion_tokens_details") or {}
    return {
        "ts": _to_local(sp.get("startedAt", "")),
        "model": powered,
        "label": provider_map.get(powered, "内置"),
        "prompt_tokens": usage.get("prompt_tokens", 0),
        "completion_tokens": usage.get("completion_tokens", 0),
        "reasoning_tokens": cdet.get("reasoning_tokens", 0) or 0,
        "cache_read_tokens": pdet.get("cached_tokens", 0) or 0,
        "duration_ms": sp.get("duration", 0) or 0,
        "ok": sp.get("status") == "ok",
        "traceId": sp.get("traceId", ""),
        "spanId": sp.get("spanId", ""),
        "source": fname,
    }


def _append_recs(recs):
    """按天分片追加写缓存。"""
    by_day = {}
    for r in recs:
        by_day.setdefault((r["ts"] or "unknown")[:10], []).append(r)
    for day, day_recs in by_day.items():
        try:
            with open(os.path.join(CACHE_DIR, f"{day}.jsonl"), "a", encoding="utf-8") as f:
                for r in day_recs:
                    f.write(json.dumps(r, ensure_ascii=False) + "\n")
        except OSError:
            pass


def _parse_new_records(paths):
    """解析 stat 有变化的文件，返回新记录（键去重），更新 _state。"""
    global _dirty
    from .trace_reader import _custom_provider
    if not _custom_provider:
        _load_custom_markers()
    provider_map = _custom_provider
    keys = _load_seen_keys()
    recs = []
    scanned = 0
    for p in paths:
        try:
            st = os.stat(p)
        except OSError:
            continue
        key = [st.st_mtime, st.st_size]
        if _state.get(p) == key:
            continue  # 未变化，跳过
        scanned += 1
        _progress["scanned"] = scanned
        try:
            with open(p, "r", encoding="utf-8") as f:
                data = json.load(f)
        except (OSError, json.JSONDecodeError):
            continue  # 写入中/损坏：不记 state，下次重试
        fname = os.path.basename(p)
        file_new = 0
        for sp in data.get("spans", []):
            if sp.get("type") != "generation":
                continue
            powered = _powered_from_tool_input(sp.get("toolInput", ""))
            if not powered or not _is_builtin_name(powered):
                continue
            usage, _ = _parse_tool_output(sp.get("toolOutput", ""))
            if not usage:
                continue
            k = (sp.get("traceId"), sp.get("spanId"))
            if k in keys:
                continue  # upsert 去重：变更文件里已有记录不再计
            keys.add(k)
            recs.append(_make_cache_rec(sp, powered, usage, fname, provider_map))
            file_new += 1
        _state[p] = key
        _dirty = True
        if file_new:
            _progress["records"] += file_new
    return recs


def _apply_new_to_agg(recs):
    """新记录累加进共享 _agg 并补进 _recent（与旧 scan_traces 行为一致）。"""
    from .aggregate import _apply_rec
    if not recs:
        return
    with _lock:
        for r in recs:
            _apply_rec(_agg, r)
            _recent.append(dict(r))
        _recent.sort(key=lambda x: x.get("ts", ""))
        del _recent[:-RECENT_KEEP]


def scan_cached(force=False):
    """增量扫描（force 时清缓存全量重建）。返回新记录数。阻塞调用方，耗时任务请用 scan_cached_async。"""
    global _seen_keys, _dirty
    with _scan_lock:
        from .trace_reader import _custom_provider
        if not _custom_provider:
            _load_custom_markers()
        if force:
            _rebuild_cache()
        try:
            paths = glob.glob(TRACES_GLOB)
        except OSError:
            paths = []
        _progress.update({"running": True, "total": len(paths), "scanned": 0, "records": 0, "done": False})
        recs = _parse_new_records(paths)
        _append_recs(recs)
        if _dirty:
            _save_state()
            _dirty = False
        _progress.update({"running": False, "done": True, "scanned": len(paths)})
        _apply_new_to_agg(recs)
        _last_scan = time.time()
        return len(recs)


def maybe_scan(force=False):
    """惰性增量扫描：SCAN_TTL 内不重复（供 /stats 等只读接口触发）。
    force=True 时跳过 TTL 直接强制扫描。
    注意：不能持有 _scan_lock 调用 scan_cached（Lock 不可重入会死锁），
    这里只在抢 TTL 时短暂持锁，实际扫描交给 scan_cached 自行加锁。"""
    global _last_scan
    now = time.time()
    if not force and now - _last_scan < SCAN_TTL:
        return 0
    with _scan_lock:
        if not force and time.time() - _last_scan < SCAN_TTL:
            return 0
        _last_scan = time.time()
    return scan_cached(force=force)


def _rebuild_cache():
    """清空缓存与 state（保留目录）。"""
    global _state, _seen_keys, _dirty
    _state = {}
    _seen_keys = None
    _dirty = False
    try:
        os.makedirs(CACHE_DIR, exist_ok=True)
        for fp in _cache_files():
            os.remove(fp)
        if os.path.exists(STATE_FILE):
            os.remove(STATE_FILE)
    except OSError:
        pass


def scan_cached_async(force=False):
    """后台线程扫描（不阻塞调用方）。"""
    threading.Thread(target=scan_cached, args=(force,), daemon=True).start()


def aggregate_from_cache():
    """启动时读缓存重建 _agg（秒级，替代旧"重启必须全量重扫"）。返回记录数。"""
    from .aggregate import _apply_rec
    n = 0
    for fp in _cache_files():
        try:
            with open(fp, "r", encoding="utf-8") as f:
                lines = f.readlines()
        except OSError:
            continue
        batch = []
        for line in lines:
            line = line.strip()
            if not line:
                continue
            try:
                r = json.loads(line)
            except json.JSONDecodeError:
                continue
            batch.append(r)
        batch.sort(key=lambda x: x.get("ts", ""))
        with _lock:
            for r in batch:
                _apply_rec(_agg, r)
                _recent.append(r)
                n += 1
            _recent.sort(key=lambda x: x.get("ts", ""))
            del _recent[:-RECENT_KEEP]
    return n


def init_cache():
    """启动初始化：确保目录、加载 state 与自定义模型标记。"""
    os.makedirs(CACHE_DIR, exist_ok=True)
    _load_custom_markers()
    _load_state()


def progress():
    return dict(_progress)
