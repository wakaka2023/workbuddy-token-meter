"""usage 聚合与持久化：内存累加 + 后台线程批量落盘。"""
import json
import os
import time

from .config import (
    FLUSH_INTERVAL,
    LOG_FILE,
    RECENT_KEEP,
    USAGE_FILE,
    USAGE_JSONL,
    _agg,
    _lock,
    _log_buf,
    _model_price,
    _pending,
    _recent,
)


def _apply_rec(agg, rec):
    """把一条 usage 记录累加进聚合 dict（total + by_model + by_day + last_success）。"""
    t = agg["total"]
    t["prompt_tokens"] = t.get("prompt_tokens", 0) + rec["prompt_tokens"]
    t["completion_tokens"] = t.get("completion_tokens", 0) + rec["completion_tokens"]
    t["reasoning_tokens"] = t.get("reasoning_tokens", 0) + rec.get("reasoning_tokens", 0)
    t["cache_read_tokens"] = t.get("cache_read_tokens", 0) + rec.get("cache_read_tokens", 0)
    t["calls"] = t.get("calls", 0) + 1
    m = agg["by_model"].setdefault(rec["model"], {
        "label": rec.get("label", "?"), "prompt_tokens": 0, "completion_tokens": 0,
        "reasoning_tokens": 0, "cache_read_tokens": 0, "calls": 0,
    })
    m["prompt_tokens"] += rec["prompt_tokens"]
    m["completion_tokens"] += rec["completion_tokens"]
    m["reasoning_tokens"] += rec.get("reasoning_tokens", 0)
    m["cache_read_tokens"] += rec.get("cache_read_tokens", 0)
    m["calls"] += 1
    day = (rec.get("ts") or "")[:10]
    if day:
        d = agg["by_day"].setdefault(day, {
            "date": day, "prompt_tokens": 0, "completion_tokens": 0,
            "cache_read_tokens": 0, "calls": 0,
        })
        d["prompt_tokens"] += rec["prompt_tokens"]
        d["completion_tokens"] += rec["completion_tokens"]
        d["cache_read_tokens"] += rec.get("cache_read_tokens", 0)
        d["calls"] += 1
        md = agg.setdefault("by_model_day", {}).setdefault(day, {}).setdefault(rec["model"], {
            "prompt_tokens": 0, "completion_tokens": 0,
            "cache_read_tokens": 0, "calls": 0,
        })
        md["prompt_tokens"] += rec["prompt_tokens"]
        md["completion_tokens"] += rec["completion_tokens"]
        md["cache_read_tokens"] += rec.get("cache_read_tokens", 0)
        md["calls"] += 1
    agg["last_success"][rec["model"]] = rec.get("ts", "")


def _init_state():
    """启动时迁移旧 usage.json → JSONL，再从 JSONL 重建聚合基准。"""
    if os.path.exists(USAGE_FILE) and not os.path.exists(USAGE_JSONL):
        try:
            with open(USAGE_FILE, "r", encoding="utf-8") as f:
                old = json.load(f)
            with open(USAGE_JSONL, "w", encoding="utf-8") as f:
                for rec in old.get("records", []):
                    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        except (OSError, json.JSONDecodeError):
            pass
    if os.path.exists(USAGE_JSONL):
        try:
            with open(USAGE_JSONL, "r", encoding="utf-8") as f:
                for line in f:
                    line = line.strip()
                    if not line:
                        continue
                    try:
                        rec = json.loads(line)
                    except json.JSONDecodeError:
                        continue
                    _apply_rec(_agg, rec)
                    _recent.append(rec)
        except OSError:
            pass
        if len(_recent) > RECENT_KEEP:
            del _recent[:-RECENT_KEEP]


def log_event(msg):
    """日志进内存缓冲，后台线程批量写盘（转发路径无文件 IO）。"""
    line = f"{time.strftime('%Y-%m-%d %H:%M:%S')} {msg}\n"
    with _lock:
        _log_buf.append(line)


def _rec_cost(model, prompt, completion, cache_read):
    inp, outp, crp = _model_price(model)
    return round(
        (prompt or 0) / 1e6 * inp
        + (completion or 0) / 1e6 * outp
        + (cache_read or 0) / 1e6 * crp,
        5,
    )


def add_usage(model, label, prompt, completion, reasoning, cache_read, duration_ms=0, ok=True):
    """转发路径只做内存累加，落盘交给后台线程。"""
    rec = {
        "ts": time.strftime("%Y-%m-%d %H:%M:%S"),
        "model": model, "label": label,
        "prompt_tokens": prompt, "completion_tokens": completion,
        "reasoning_tokens": reasoning or 0, "cache_read_tokens": cache_read or 0,
        "duration_ms": duration_ms or 0, "ok": ok,
        "cost": _rec_cost(model, prompt, completion, cache_read),
    }
    with _lock:
        _apply_rec(_agg, rec)
        _pending.append(rec)
        _recent.append(rec)
        if len(_recent) > RECENT_KEEP:
            del _recent[:-RECENT_KEEP]


def add_failed(model, label, status, error, duration_ms):
    """失败请求只进内存 _recent（不落盘、不聚合），供最近请求列表标红。"""
    rec = {
        "ts": time.strftime("%Y-%m-%d %H:%M:%S"),
        "model": model, "label": label,
        "ok": False, "status": status, "error": (error or "")[:200],
        "duration_ms": duration_ms or 0,
    }
    with _lock:
        _recent.append(rec)
        if len(_recent) > RECENT_KEEP:
            del _recent[:-RECENT_KEEP]


def flush_pending():
    """把内存攒的 usage 与日志一次性追加写盘。"""
    with _lock:
        pending, _pending[:] = _pending[:], []
        logs, _log_buf[:] = _log_buf[:], []
    if pending:
        try:
            with open(USAGE_JSONL, "a", encoding="utf-8") as f:
                for rec in pending:
                    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        except OSError:
            pass
    if logs:
        try:
            with open(LOG_FILE, "a", encoding="utf-8") as f:
                f.write("".join(logs))
        except OSError:
            pass


def _flush_worker():
    while True:
        time.sleep(FLUSH_INTERVAL)
        flush_pending()


def extract_usage(payload):
    """从响应 JSON 提取 token 用量。"""
    u = payload.get("usage") or {}
    if not u:
        return 0, 0, 0, 0
    prompt = u.get("prompt_tokens", 0)
    completion = u.get("completion_tokens", 0)
    details = u.get("completion_tokens_details") or {}
    reasoning = details.get("reasoning_tokens", 0)
    pdetails = u.get("prompt_tokens_details") or {}
    cache_read = pdetails.get("cached_tokens", 0)
    return prompt, completion, reasoning, cache_read
