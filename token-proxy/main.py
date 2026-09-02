import threading
from http.server import ThreadingHTTPServer

from proxy import config
from proxy.aggregate import _flush_worker, _init_state, flush_pending, log_event
from proxy.config import MODE, PORT, _config, _load_config
from proxy.handler import Handler
from proxy import trace_cache


def main():
    _load_config()
    _init_state()
    threading.Thread(target=_flush_worker, daemon=True).start()
    if not config.LEAN:
        # v0.2.4 起统计迁到 widget 本地（Rust 引擎直读 trace）；仅非 lean（手动起代理）
        # 时保留 trace 缓存扫描能力，避免与 widget 引擎双写同一缓存目录。
        trace_cache.init_cache()
        n_cached = trace_cache.aggregate_from_cache()
        if n_cached:
            log_event(f"[CACHE] rebuilt agg from trace-cache: {n_cached} records")
            threading.Thread(target=lambda: trace_cache.scan_cached(), daemon=True).start()
        else:
            threading.Thread(target=lambda: trace_cache.scan_cached(force=True), daemon=True).start()
    log_event(f"token-proxy started on :{PORT} (mode={MODE}{', lean' if config.LEAN else ''})")
    flush_pending()
    print(f"token-proxy listening on http://127.0.0.1:{PORT} (mode={MODE})")
    if config.LEAN:
        print("  lean mode: forwarding only (stats/scan handled by widget)")
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
