import socket
import sys
import threading
from http.server import ThreadingHTTPServer

from proxy import config
from proxy.aggregate import _flush_worker, _init_state, flush_pending, log_event
from proxy.config import MODE, PORT, _config, _load_config
from proxy.handler import Handler
from proxy import trace_cache


def _port_in_use():
    """8787 已被占用则返回 True（本机另一实例或第三方程序）。"""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.bind(("127.0.0.1", PORT))
        return False
    except OSError:
        return True
    finally:
        s.close()


def _health_alive():
    """对端若应答 /health ok 视为「我们的实例已在跑」，与 widget 的判定一致。"""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(1.0)
    try:
        s.connect(("127.0.0.1", PORT))
        s.sendall(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        data = b""
        while True:
            chunk = s.recv(4096)
            if not chunk:
                break
            data += chunk
        return b'"status": "ok"' in data
    except OSError:
        return False
    finally:
        s.close()


def main():
    # 防多实例堆积：widget 在 sync_proxy 里每次误判「未起来」都会再拉一个进程，
    # 端口被本实例占用的新进程若继续跑就会越积越多。检测到已有实例直接退出。
    if _port_in_use():
        if _health_alive():
            print(f"token-proxy already running on :{PORT}, exiting")
            sys.exit(0)
        print(f"port {PORT} occupied by another process, exiting")
        sys.exit(1)
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
    server.daemon_threads = True
    server.allow_reuse_address = True
    server.serve_forever()


if __name__ == "__main__":
    main()
