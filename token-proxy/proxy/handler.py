"""HTTP 处理：/stats /config /keys 管理端点 + /chat/completions 透明转发。"""
import json
import time
from http.server import BaseHTTPRequestHandler

import requests

from . import config, ledger, route_switch, trace_cache
from .aggregate import add_failed, add_usage, extract_usage, log_event
from .config import (
    BACKOFF,
    CONFIG_FILE,
    MAX_ATTEMPTS,
    PORT,
    RETRYABLE,
    _agg,
    _channel,
    _lock,
    _mask_key,
    _masked_channels,
    _masked_models,
    _merge_channels_keys,
    _merge_models_fields,
    _model_base,
    _model_defaults,
    _model_key,
    _model_label,
    _model_price,
    _model_proxy,
    _proxy_settings,
    _recent,
    _resolve_model_id,
    _session,
    mode_status,
)
from .import_wb import import_workbuddy


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
        if p in ("/status", "/status/"):
            self._send_json(200, mode_status())
        elif p in ("/scan/progress", "/scan/progress/"):
            self._send_json(200, trace_cache.progress())
        elif p in ("/models", "/models/"):
            self._send_json(200, {"models": route_switch.list_models()})
        elif p in ("/config/ledger", "/config/ledger/"):
            self._send_json(200, {
                "models": ledger.models(),
                "changelog": ledger.changelog(),
                "summary": ledger.summary(),
            })
        elif p in ("/stats", "/stats/"):
            trace_cache.maybe_scan()  # 惰性增量扫 trace（读缓存记账），30s 内不重复
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
            cfg["channels"] = _masked_channels(cfg.get("channels"))
            cfg["models"] = _masked_models(cfg.get("models"))
            log_event(
                f"[CFG] GET /config: channels={len(cfg.get('channels') or {})} "
                f"models={len(cfg.get('models') or {})}"
            )
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
        # 空保护：提交的 channels 与 models 均为空，且当前配置非空时拒绝覆盖，
        # 防止前端状态丢失/读取失败时把真实配置清空
        has_channels = isinstance(new.get("channels"), dict) and bool(new["channels"])
        has_models = isinstance(new.get("models"), dict) and bool(new["models"])
        incoming_empty = not has_channels and not has_models
        if incoming_empty and (config._config.get("channels") or config._config.get("models")):
            self._send_json(400, {
                "error": "refusing to overwrite non-empty config with empty payload",
                "hint": "当前配置有内容，而提交的 channels/models 均为空，已拒绝写入",
            })
            return
        with _lock:
            _dropped = []
            if isinstance(new.get("channels"), dict):
                _merged_c, _dropped_c = _merge_channels_keys(
                    config._config.get("channels", {}), new["channels"]
                )
                config._config["channels"] = _merged_c
                _dropped += _dropped_c
            if isinstance(new.get("models"), dict):
                config._config["models"] = _merge_models_fields(
                    config._config.get("models", {}), new["models"]
                )
        # 注意：log_event 内部会再拿 _lock（非重入锁），必须在锁外调用，否则死锁
        if _dropped:
            log_event(
                "[CFG] PUT /config: dropped masked keys with no real source: "
                + ", ".join(f"{c}/{kid}" for c, kid in _dropped)
            )
        try:
            with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                json.dump(config._config, f, ensure_ascii=False, indent=2)
        except OSError as e:
            log_event(f"[ERR] PUT /config write failed: {e}")
            self._send_json(500, {"error": str(e)})
            return
        log_event(
            f"[CFG] PUT /config: channels={len(config._config.get('channels', {}))} "
            f"models={len(config._config.get('models', {}))}"
        )
        if config._has_any_key() and config.MODE != "full":
            config.MODE = "full"
            log_event("[MODE] -> full (config has keys)")
        ledger.log("manual_edit", "user PUT /config")
        self._send_json(200, {
            "ok": True,
            "channels": len(config._config.get("channels", {})),
            "models": len(config._config.get("models", {})),
        })

    def _read_body(self):
        """读取并解析 JSON body；失败返回 None。"""
        length = int(self.headers.get("Content-Length", 0))
        try:
            return json.loads(self.rfile.read(length).decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError, ValueError):
            return None

    def _handle_scan(self):
        """POST /scan：触发扫描。body {"force": true} 时清缓存全量重建。"""
        body = self._read_body() or {}
        force = bool(body.get("force"))
        if force:
            trace_cache.scan_cached_async(force=True)
            self._send_json(200, {"ok": True, "started": "full rebuild"})
        else:
            n = trace_cache.scan_cached_async()
            self._send_json(200, {"ok": True, "started": "incremental"})

    def _handle_import(self):
        """POST /import/workbuddy：一键导入 WorkBuddy 自定义模型。"""
        ok, summary, _ = import_workbuddy()
        if ok:
            log_event(f"[IMP] workbuddy import: {summary}")
        self._send_json(200 if ok else 500, {"ok": ok, **summary})

    def _handle_route(self):
        """POST /models/{name}/route：body {"route": "proxy"|"direct"}。"""
        from urllib.parse import unquote
        name = unquote(self.path.rstrip("/").split("/")[-2])
        body = self._read_body() or {}
        route = body.get("route")
        ok, msg, url = route_switch.switch_route(name, route)
        log_event(f"[ROUTE] {name} -> {route}: {msg}")
        self._send_json(200 if ok else 400, {"ok": ok, "message": msg, "url": url})

    def _handle_fetch_models(self):
        """POST /channels/{name}/fetch_models：用渠道激活 key 请求 {base}/models，
        拉取该渠道可用模型列表并缓存（成功写回 config.json，供前端添加模型时勾选）。

        注意：上游网络请求在锁外执行，避免持锁等待拖累其他请求。
        """
        from urllib.parse import unquote
        name = unquote(self.path.rstrip("/").split("/")[-2])
        with _lock:
            ch = dict(config._channel(name) or {})
        if not ch:
            self._send_json(404, {"ok": False, "error": f"channel '{name}' not found"})
            return
        base = ch.get("base") or ""
        if not base:
            self._send_json(400, {"ok": False, "error": "channel has no base url"})
            return
        keys = ch.get("keys") or []
        if not keys:
            self._send_json(400, {
                "ok": False,
                "error": "channel has no api key",
                "hint": "请先在渠道配置中添加 API Key，再拉取模型列表",
            })
            return
        ak = ch.get("activeKey")
        active = next((k.get("key") for k in keys if k.get("id") == ak), None)
        if not active:
            active = keys[0].get("key")
        upstream = base.rstrip("/") + "/models"
        auth = f"Bearer {active}"
        proxy_mode = ch.get("proxy") or "auto"
        try:
            resp = _session.get(
                upstream, headers={
                    "Authorization": auth,
                    "Accept-Encoding": "identity",
                },
                timeout=8, proxies=_proxy_settings(proxy_mode),
            )
        except requests.RequestException as e:
            log_event(f"[ERR] {name} fetch_models RequestException: {e}")
            self._send_json(502, {
                "ok": False,
                "error": "network error",
                "hint": f"拉取失败：{e}。该渠道可能不支持 /models，请手动填写模型名",
            })
            return
        if resp.status_code >= 400:
            body_preview = resp.content.decode("utf-8", "replace")[:200]
            log_event(f"[ERR] {name} fetch_models HTTP {resp.status_code}: {body_preview}")
            self._send_json(502, {
                "ok": False,
                "error": f"upstream HTTP {resp.status_code}",
                "hint": "该渠道可能不支持 /models 端点，请手动填写模型名",
            })
            return
        try:
            payload = resp.json()
            ids = [m.get("id") for m in payload.get("data", []) if m.get("id")]
        except (ValueError, AttributeError):
            self._send_json(502, {
                "ok": False,
                "error": "unexpected response format",
                "hint": "该渠道返回格式非 OpenAI 标准，请手动填写模型名",
            })
            return
        cached_at = time.strftime("%Y-%m-%d %H:%M:%S")
        with _lock:
            cur_ch = config._config.setdefault("channels", {}).get(name)
            if cur_ch is not None:
                cur_ch["availableModels"] = ids
                cur_ch["fetchedAt"] = cached_at
                try:
                    with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                        json.dump(config._config, f, ensure_ascii=False, indent=2)
                except OSError as e:
                    log_event(f"[ERR] fetch_models write failed: {e}")
        log_event(f"[MODELS] {name} fetch_models: {len(ids)} models (cached at {cached_at})")
        self._send_json(200, {
            "ok": True,
            "channel": name,
            "models": ids,
            "count": len(ids),
            "cached_at": cached_at,
        })

    def _handle_mode_start(self):
        """POST /mode/start：切 full 模式（需已配置真实 key）。"""
        if not config._has_any_key():
            self._send_json(400, {
                "ok": False,
                "error": "no_api_key",
                "message": "尚未配置 API Key，请先在设置中添加",
            })
            return
        config.MODE = "full"
        log_event("[MODE] -> full (forwarding enabled)")
        self._send_json(200, {"ok": True, **mode_status()})

    def _handle_mode_stop(self):
        """POST /mode/stop：切回 service 模式（停止转发，不影响扫描/统计）。"""
        config.MODE = "service"
        log_event("[MODE] -> service (forwarding disabled)")
        self._send_json(200, {"ok": True, **mode_status()})

    def _handle_keys(self):
        """POST /keys：管理某个渠道的 API key（set|add|del），成功后写回 config.json。
        body: {channel, action, keyId?, name?, key?}。有真实 key 时自动切 full 模式。"""
        length = int(self.headers.get("Content-Length", 0))
        try:
            req = json.loads(self.rfile.read(length).decode("utf-8"))
        except (json.JSONDecodeError, UnicodeDecodeError):
            self._send_json(400, {"error": "invalid json body"})
            return
        channel = req.get("channel")
        action = req.get("action")
        if not channel or action not in ("set", "add", "del"):
            self._send_json(400, {"error": "channel & action(set|add|del) required"})
            return
        with _lock:
            cfg_channels = config._config.setdefault("channels", {})
            ch = cfg_channels.get(channel)
            if ch is None:
                self._send_json(404, {"error": f"channel '{channel}' not found in config"})
                return
            keys = ch.setdefault("keys", [])
            key_id = req.get("keyId")
            if action == "set":
                if not any(k.get("id") == key_id for k in keys):
                    self._send_json(400, {"error": f"keyId '{key_id}' not found"})
                    return
                ch["activeKey"] = key_id
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
                if not ch.get("activeKey"):
                    ch["activeKey"] = new_id
                key_id = new_id
            elif action == "del":
                if not any(k.get("id") == key_id for k in keys):
                    self._send_json(400, {"error": f"keyId '{key_id}' not found"})
                    return
                keys[:] = [k for k in keys if k.get("id") != key_id]
                if ch.get("activeKey") == key_id:
                    ch["activeKey"] = keys[0]["id"] if keys else None
            try:
                with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                    json.dump(config._config, f, ensure_ascii=False, indent=2)
            except OSError as e:
                log_event(f"[ERR] /keys write failed: {e}")
                self._send_json(500, {"error": str(e)})
                return
        masked = _masked_channels({channel: ch}).get(channel, {}).get("keys", [])
        log_event(f"[KEY] {channel} {action} key={key_id} active={ch.get('activeKey')} total={len(keys)}")
        if config._has_any_key() and config.MODE != "full":
            config.MODE = "full"
            log_event("[MODE] -> full (first key configured)")
        ledger.log("keys_change", f"{channel} {action} {key_id}")
        self._send_json(200, {
            "ok": True,
            "channel": channel,
            "action": action,
            "keyId": key_id,
            "activeKey": ch.get("activeKey"),
            "keys": masked,
        })

    def do_POST(self):
        path = self.path.split("?")[0]
        if path in ("/keys", "/keys/"):
            self._handle_keys()
            return
        if path in ("/scan", "/scan/"):
            self._handle_scan()
            return
        if path in ("/import/workbuddy", "/import/workbuddy/"):
            self._handle_import()
            return
        if path.startswith("/models/") and path.rstrip("/").endswith("/route"):
            self._handle_route()
            return
        if path.startswith("/channels/") and path.rstrip("/").endswith("/fetch_models"):
            self._handle_fetch_models()
            return
        if path in ("/mode/start", "/mode/start/"):
            self._handle_mode_start()
            return
        if path in ("/mode/stop", "/mode/stop/"):
            self._handle_mode_stop()
            return
        if not path.endswith("/chat/completions"):
            self.send_response(404)
            self.end_headers()
            return
        # 双模式门禁：service 模式下转发停用（扫描/统计/配置接口不受影响）
        if config.MODE != "full":
            msg = json.dumps({
                "error": {
                    "type": "proxy_not_enabled",
                    "message": "代理未启用：请在设置中配置 API Key 并启动代理",
                }
            }).encode("utf-8")
            self.send_response(503)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)
            return
        config._forwarded += 1

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

        # 若请求用 name（显示名）命中配置，转发时把 body.model 重写为真实 id，
        # 避免上游只认 id 而 404
        real_id = _resolve_model_id(model)
        if real_id != model:
            log_event(f"[ALIAS] {model} -> {real_id} (display name to model id)")
            body["model"] = real_id
            model = real_id

        # 路由：模型名查 config.models 行，base 即上游地址（无第二跳）
        base = _model_base(model)
        if not base:
            msg = json.dumps({
                "error": {
                    "type": "model_not_configured",
                    "message": f"模型 '{model}' 未配置：请在设置 → 模型 中添加 base 地址后重试",
                }
            }).encode("utf-8")
            self.send_response(404)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)
            return
        upstream = base.rstrip("/") + "/chat/completions"
        label = _model_label(model)

        log_event(f"[REQ] {label}/{model} stream={stream} prompt_chars={len(raw)}")
        if stream and "stream_options" not in body:
            body["stream_options"] = {"include_usage": True}
        for k, v in _model_defaults(model).items():
            body.setdefault(k, v)

        # 模型级 key 覆盖：该模型配置了 keys 时用激活 key 覆盖客户端 Authorization；
        # 未配置则透传客户端自己带的 key（如 WorkBuddy 直连自定义模型）
        active = _model_key(model)
        auth = f"Bearer {active}" if active else self.headers.get("Authorization", "")
        if active:
            log_event(f"[KEY] {label}/{model} using model key {_mask_key(active)}")
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
                    stream=stream, timeout=300, proxies=_proxy_settings(_model_proxy(model)),
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
