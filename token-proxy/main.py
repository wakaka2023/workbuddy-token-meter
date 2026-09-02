import threading
from http.server import ThreadingHTTPServer

from proxy.aggregate import _flush_worker, _init_state, flush_pending, log_event
from proxy.config import MODE, PORT, _config, _load_config
from proxy.handler import Handler
from proxy import trace_cache


def main():
    _load_config()
    _init_state()
    # trace 缓存：初始化目录/state，然后
    #   缓存存在 → 读缓存秒级重建聚合（替代旧"重启必须全量重扫"）
    #   缓存为空 → 后台全量扫描建缓存（约 8s，不阻塞启动）
    trace_cache.init_cache()
    threading.Thread(target=_flush_worker, daemon=True).start()
    n_cached = trace_cache.aggregate_from_cache()
    if n_cached:
        log_event(f"[CACHE] rebuilt agg from trace-cache: {n_cached} records")
        threading.Thread(target=lambda: trace_cache.scan_cached(), daemon=True).start()
    else:
        threading.Thread(target=lambda: trace_cache.scan_cached(force=True), daemon=True).start()
    log_event(f"token-proxy started on :{PORT} (mode={MODE})")
    flush_pending()
    print(f"token-proxy listening on http://127.0.0.1:{PORT} (mode={MODE})")
    if MODE != "full":
        print("  forwarding disabled (service mode): configure API keys, then POST /mode/start")
    for cname, ch in (_config.get("channels") or {}).items():
        print(f"  channel {cname}: base={ch.get('base') or '-'} keys={len(ch.get('keys') or [])}")
    for mid, m in (_config.get("models") or {}).items():
        print(f"  model {mid}: channel={m.get('channel') or '-'} key={m.get('key') or '-'}")
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
