"""HTTP 处理：/stats /config /keys 管理端点 + /chat/completions 透明转发。"""
import json
import time
from http.server import BaseHTTPRequestHandler

import requests

from . import config
from .aggregate import add_failed, add_usage, extract_usage, log_event
from .config import (
    BACKOFF,
    CONFIG_FILE,
    DEFAULT_ROUTE,
    MAX_ATTEMPTS,
    PORT,
    RETRYABLE,
    ROUTES,
    _active_key,
    _agg,
    _lock,
    _mask_key,
    _masked_providers,
    _merge_providers_keys,
    _model_defaults,
    _model_price,
    _provider,
    _recent,
    _resolve_proxies,
    _session,
)
from .trace_reader import scan_traces


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass  # 静默访问日志

    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, PUT, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, Authorization")

    def _send_json(self, code, obj):
        body = json.dumps(obj, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self._cors()
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def _cost_of(self, m):
        inp, outp, crp = _model_price(m.get("model", ""))
        return round(
            m.get("prompt_tokens", 0) / 1e6 * inp
            + m.get("completion_tokens", 0) / 1e6 * outp
            + m.get("cache_read_tokens", 0) / 1e6 * crp,
            4,
        )

    def do_GET(self):
        p = self.path.split("?")[0]
        if p in ("/stats", "/stats/"):
            scan_traces()  # 惰性增量扫 trace（内置模型记账），30s 内不重复
            with _lock:
                by_model = {k: dict(v) for k, v in _agg["by_model"].items()}
                total = dict(_agg["total"])
                by_day = sorted(
                    (dict(v) for v in _agg["by_day"].values()),
                    key=lambda x: x["date"],
                )
                by_model_day = {
                    day: {m: dict(v) for m, v in models.items()}
                    for day, models in _agg.get("by_model_day", {}).items()
                }
                last_success = dict(_agg["last_success"])
                records = list(_recent)
            for name, m in by_model.items():
                m["model"] = name
                m["cost"] = self._cost_of(m)
            total["cost"] = round(sum(m["cost"] for m in by_model.values()), 4)
            self._send_json(200, {
                "total": total,
                "by_model": by_model,
                "by_day": by_day,
                "by_model_day": by_model_day,
                "last_success": last_success,
                "records": records,
            })
        elif p in ("/requests", "/requests/"):
            with _lock:
                records = list(_recent)
            self._send_json(200, {"records": records})
        elif p in ("/health", "/health/"):
            with _lock:
                calls = _agg["total"].get("calls", 0)
            self._send_json(200, {
                "status": "ok",
                "ts": time.strftime("%Y-%m-%d %H:%M:%S"),
                "calls": calls,
                "proxy": f"http://127.0.0.1:{PORT}",
            })
        elif p in ("/config", "/config/"):
            with _lock:
                cfg = json.loads(json.dumps(config._config))
            cfg["routes"] = dict(ROUTES)
            cfg["providers"] = _masked_providers(cfg.get("providers"))
            self._send_json(200, cfg)
        else:
            self.send_response(404)
            self._cors()
            self.end_headers()

    def do_PUT(self):
        if self.path.split("?")[0] not in ("/config", "/config/"):
            self.send_response(404)
            self._cors()
            self.end_headers()
            return
        length = int(self.headers.get("Content-Length", 0))
        try:
            new = json.loads(self.rfile.read(length).decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self._send_json(400, {"error": "invalid json body"})
            return
        if not isinstance(new, dict):
            self._send_json(400, {"error": "expect a JSON object"})
            return
        with _lock:
            if isinstance(new.get("models"), dict):
                config._config["models"] = new["models"]
            if isinstance(new.get("providers"), dict):
                # 防脱敏回显覆盖真实 key：疑似脱敏值按 id 从现有配置找回
                config._config["providers"] = _merge_providers_keys(
                    config._config.get("providers", {}), new["providers"]
                )
            if isinstance(new.get("routes"), dict):
                ROUTES.clear()
                ROUTES.update(new["routes"])
        try:
            with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                json.dump(config._config, f, ensure_ascii=False, indent=2)
        except OSError as e:
            log_event(f"[ERR] PUT /config write failed: {e}")
            self._send_json(500, {"error": str(e)})
            return
        log_event(f"[CFG] PUT /config: models={len(config._config.get('models', {}))} "
                  f"providers={len(config._config.get('providers', {}))} routes={len(ROUTES)}")
        self._send_json(200, {
            "ok": True,
            "models": len(config._config.get("models", {})),
            "providers": len(config._config.get("providers", {})),
            "routes": dict(ROUTES),
        })

    def _handle_keys(self):
        """POST /keys：管理 provider 的 API key（set|add|del），成功后写回 config.json。"""
        length = int(self.headers.get("Content-Length", 0))
        try:
            req = json.loads(self.rfile.read(length).decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self._send_json(400, {"error": "invalid json body"})
            return
        provider = req.get("provider")
        action = req.get("action")
        if not provider or action not in ("set", "add", "del"):
            self._send_json(400, {"error": "provider & action(set|add|del) required"})
            return
        with _lock:
            prov = (config._config.get("providers") or {}).get(provider)
            if prov is None:
                self._send_json(404, {"error": f"provider '{provider}' not found"})
                return
            keys = prov.setdefault("keys", [])
            key_id = req.get("keyId")
            if action == "set":
                if not any(k.get("id") == key_id for k in keys):
                    self._send_json(400, {"error": f"keyId '{key_id}' not found"})
                    return
                prov["activeKey"] = key_id
            elif action == "add":
                raw_key = req.get("key", "")
                if not raw_key:
                    self._send_json(400, {"error": "key required"})
                    return
                new_id = f"k{len(keys) + 1}"
                keys.append({
                    "id": new_id,
                    "name": req.get("name") or new_id,
                    "key": raw_key,
                })
                if not prov.get("activeKey"):
                    prov["activeKey"] = new_id
                key_id = new_id
            elif action == "del":
                if not any(k.get("id") == key_id for k in keys):
                    self._send_json(400, {"error": f"keyId '{key_id}' not found"})
                    return
                keys[:] = [k for k in keys if k.get("id") != key_id]
                if prov.get("activeKey") == key_id:
                    prov["activeKey"] = keys[0]["id"] if keys else None
            try:
                with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                    json.dump(config._config, f, ensure_ascii=False, indent=2)
            except OSError as e:
                log_event(f"[ERR] /keys write failed: {e}")
                self._send_json(500, {"error": str(e)})
                return
        log_event(f"[KEY] {provider} {action} key={key_id} active={prov.get('activeKey')} total={len(keys)}")
        self._send_json(200, {
            "ok": True,
            "provider": provider,
            "action": action,
            "keyId": key_id,
            "activeKey": prov.get("activeKey"),
            "keys": _masked_providers({provider: prov}).get(provider, {}).get("keys", []),
        })

    def do_POST(self):
        path = self.path.split("?")[0]
        if path in ("/keys", "/keys/"):
            self._handle_keys()
            return
        if not path.endswith("/chat/completions"):
            self.send_response(404)
            self.end_headers()
            return

        length = int(self.headers.get("Content-Length", 0))
        raw = self.rfile.read(length)
        try:
            body = json.loads(raw.decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self.send_response(400)
            self.end_headers()
            return

        model = body.get("model", "unknown")
        stream = bool(body.get("stream", False))

        # 路由：模型名优先（config.models[model].provider），未登记回退 path 前缀
        provider_name = ((config._config.get("models") or {}).get(model) or {}).get("provider")
        if not provider_name:
            route_key = DEFAULT_ROUTE
            for k in dict(ROUTES):
                if self.path.endswith(k):
                    route_key = k
                    break
            provider_name = ROUTES[route_key]
        prov = _provider(provider_name)
        if prov is None:
            msg = json.dumps({"error": f"provider '{provider_name}' (model {model}) not configured"}).encode("utf-8")
            self.send_response(404)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)
            return
        upstream = prov[0] + "/chat/completions"
        label = prov[1]

        log_event(f"[REQ] {label}/{model} stream={stream} prompt_chars={len(raw)}")
        if stream and "stream_options" not in body:
            body["stream_options"] = {"include_usage": True}
        for k, v in _model_defaults(model).items():
            body.setdefault(k, v)

        # 渠道级 key 覆盖：配置了 keys 时用激活 key 覆盖客户端 Authorization
        active = _active_key(provider_name)
        auth = f"Bearer {active}" if active else self.headers.get("Authorization", "")
        if active:
            log_event(f"[KEY] {provider_name} using active key {_mask_key(active)}")
        # Accept-Encoding: identity 强制未压缩流，否则 stream 模式转发乱码
        headers = {
            "Content-Type": "application/json",
            "Authorization": auth,
            "Accept-Encoding": "identity",
        }

        resp = None
        t0 = time.time()
        for attempt in range(1, MAX_ATTEMPTS + 1):
            try:
                resp = _session.post(
                    upstream, json=body, headers=headers,
                    stream=stream, timeout=300, proxies=_resolve_proxies(provider_name),
                )
            except requests.RequestException as e:
                log_event(f"[ERR] {label}/{model} attempt{attempt} RequestException: {e}")
                if attempt < MAX_ATTEMPTS:
                    time.sleep(BACKOFF)
                    continue
                add_failed(model, label, 502, str(e), int((time.time() - t0) * 1000))
                msg = json.dumps({"error": str(e)}).encode("utf-8")
                self.send_response(502)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(msg)))
                self.end_headers()
                self.wfile.write(msg)
                return

            if resp.status_code < 400:
                break

            try:
                err_body = resp.content.decode("utf-8", "replace")
            except Exception:
                err_body = "<unreadable>"
            log_event(
                f"[ERR] {label}/{model} attempt{attempt} HTTP {resp.status_code}: "
                f"{err_body[:800]}"
            )
            if resp.status_code in RETRYABLE and attempt < MAX_ATTEMPTS:
                time.sleep(BACKOFF)
                continue

            add_failed(model, label, resp.status_code, err_body[:200], int((time.time() - t0) * 1000))
            out = err_body.encode("utf-8")
            self.send_response(resp.status_code)
            self.send_header("Content-Type", resp.headers.get("Content-Type", "application/json"))
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
            return

        log_event(
            f"[OK] {label}/{model} HTTP {resp.status_code} "
            f"{'stream' if stream else 'sync'} {int((time.time()-t0)*1000)}ms"
        )

        if not stream:
            # 提取 usage 用 json 解析；转发按原始字节（上游 identity 未压缩）
            payload = json.loads(resp.content)
            p, c, r, cr = extract_usage(payload)
            if p or c:
                add_usage(model, label, p, c, r, cr, duration_ms=int((time.time() - t0) * 1000))
            out = resp.content
            self.send_response(resp.status_code)
            self.send_header("Content-Type", resp.headers.get("Content-Type", "application/json"))
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
            return

        # 流式：逐行转发 SSE，捕获末尾含 usage 的 chunk
        self.send_response(resp.status_code)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        last_usage = None
        saw_content = False
        interrupted = False
        try:
            for line in resp.iter_lines(decode_unicode=False):
                # SSE 靠空行分隔事件：iter_lines 剥离空行，必须原样补回；
                # 按原始字节转发，避免 UTF-8 中文双编码乱码；
                # 先解析后写：客户端读完即关连接时 flush 抛 BrokenPipeError 不会漏记 usage。
                if line.startswith(b"data:"):
                    chunk = line[len(b"data:"):].strip()
                    if chunk != b"[DONE]":
                        try:
                            obj = json.loads(chunk)
                        except (json.JSONDecodeError, UnicodeDecodeError):
                            obj = None
                        if isinstance(obj, dict):
                            if obj.get("error"):
                                log_event(f"[ERR] {label}/{model} stream error chunk: {str(obj['error'])[:400]}")
                            for ch in (obj.get("choices") or []):
                                d = ch.get("delta") or {}
                                if d.get("content") or d.get("tool_calls") or d.get("reasoning_content"):
                                    saw_content = True
                            if obj.get("usage"):
                                last_usage = obj["usage"]
                self.wfile.write(line)
                self.wfile.write(b"\n")
                self.wfile.flush()
        except (ConnectionResetError, BrokenPipeError):
            # 客户端读完最后一行即关连接属正常收尾，usage 已在上一步解析，不刷堆栈
            interrupted = True
            log_event(f"[WARN] {label}/{model} client disconnected mid-stream")
        # 200 但整条流无有效内容（上游占位/空流）无法重试（已发 200），记录定位
        if not saw_content and not last_usage and not interrupted:
            log_event(f"[ERR] {label}/{model} EMPTY STREAM (HTTP 200, no content chunk)")
        if last_usage:
            p, c, r, cr = extract_usage({"usage": last_usage})
            if p or c:
                add_usage(model, label, p, c, r, cr, duration_ms=int((time.time() - t0) * 1000))
