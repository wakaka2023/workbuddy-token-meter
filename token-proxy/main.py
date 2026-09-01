import threading
from http.server import ThreadingHTTPServer

from proxy.aggregate import _flush_worker, _init_state, flush_pending, log_event
from proxy.config import PORT, _config, _load_config
from proxy.handler import Handler
from proxy.trace_reader import start_background_scan


def main():
    _load_config()
    _init_state()
    start_background_scan()
    threading.Thread(target=_flush_worker, daemon=True).start()
    log_event(f"token-proxy started on :{PORT}")
    flush_pending()
    print(f"token-proxy listening on http://127.0.0.1:{PORT}")
    for name, p in (_config.get("providers") or {}).items():
        print(f"  provider {name}: proxy={p.get('proxy') or 'auto'}")
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
