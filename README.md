# workbuddy-token-meter

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)]()
[![Platform](https://img.shields.io/badge/platform-Windows-0078d6.svg)]()
[![Python](https://img.shields.io/badge/python-3.10+-3776AB.svg)]()
[![License](https://img.shields.io/badge/license-MIT-green.svg)]()

A lightweight **desktop floating widget + local proxy** that tracks token usage of
[WorkBuddy](https://www.workbuddy.cn) LLM requests in real time. See at a glance how
many tokens each conversation costs — per model, per day, or per provider — without
touching WorkBuddy internals.

---

## Features

- 🖥️ **Desktop floating widget** — frameless, transparent, always-on-top mini window
  with live stats: total calls, tokens, cost, per-model breakdown and a daily trend chart.
- 🔀 **Dual-channel accounting** — requests routed through the local proxy are metered
  from upstream `usage` responses (`usage.jsonl`); built-in models and *directly-connected*
  custom models are metered by a passive reader over WorkBuddy's local trace files.
  No hooks, no injection, zero impact on response latency.
- 🏷️ **Provider-aware labels** — custom models are tagged with their real provider name
  (e.g. `B.AI`, resolved from `models.json`), built-in models are tagged `built-in`.
- 📊 **Rich aggregation** — totals, per-model, per-day, per-model-per-day and the latest
  `N` records, served through a single `/stats` endpoint.
- 🔌 **OpenAI-compatible local proxy** — transparent forwarding with sync + SSE streaming,
  multi-provider routing (`b.ai` / `aliyun`), multi-key rotation, automatic retry with
  backoff on retryable errors, and per-provider proxy resolution (auto-probe local VPN).
- 🔐 **Safe by default** — API keys are masked in every response and never written to
  logs; the repository only ships a sanitized `config.example.json`.

## Architecture

```
┌─────────────────────┐   POST /v1/chat/completions   ┌─────────────────────────────┐
│      WorkBuddy      │ ───────────────────────────▶ │ token-proxy  (Python, :8787) │
│  custom model base  │                               │  · route & forward upstream  │
│ http://127.0.0.1:8787/v1                            │  · meter usage → usage.jsonl │
└─────────────────────┘                               └──────────────┬──────────────┘
                                                                      │
┌─────────────────────┐   passive read (side-channel) ┌──────────────▼──────────────┐
│ WorkBuddy trace log │ ───────────────────────────▶ │ trace_reader (in-process)    │
│ ~/.workbuddy/traces/│                               │  · built-in & direct models │
│   */trace_*.json    │                               │  · incremental scan, 30s TTL │
└─────────────────────┘                               └──────────────┬──────────────┘
                                                                      │ GET /stats
                                                              ┌───────▼────────┐
                                                              │  token-widget  │
                                                              │ (Tauri 2 + React)│
                                                              └────────────────┘
```

Two independent metering paths converge into one in-memory aggregate, so every request
is counted exactly once — regardless of whether it went through the proxy or connected
directly to the upstream provider.

## Quick Start (dev mode)

> Prerequisites: Python 3.10+, Node.js 20+, Rust toolchain (for the widget).

```bash
# 1. backend — configure and start the proxy
cd token-proxy
cp config.example.json config.json   # then fill in your API keys
pip install requests
python main.py                       # listening on http://127.0.0.1:8787

# 2. point WorkBuddy at the proxy
#    In WorkBuddy → Settings → custom model, set base URL:
#    http://127.0.0.1:8787/v1

# 3. frontend — run the floating widget
cd ../token-widget
npm install
npm run tauri dev
```

On Windows you can also double-click `token-proxy/start_dev.bat` to run the proxy
with a visible console window.

## HTTP API

| Method | Path                  | Description                                          |
|--------|-----------------------|------------------------------------------------------|
| GET    | `/stats`              | Full aggregation (total / by_model / by_day / by_model_day / recent) |
| GET    | `/requests`           | Recent records only                                  |
| GET    | `/health`             | Health check with current call count                 |
| GET    | `/config`             | Current config (API keys masked)                     |
| PUT    | `/config`             | Update models / providers / routes (keys merged, masked values restored by id) |
| POST   | `/keys`               | Manage provider keys: `set` (activate) / `add` / `del` |
| POST   | `/v1/chat/completions`| Transparent OpenAI-compatible forwarding (sync + SSE) |

## Data & Storage

- **Data directory** — `%APPDATA%\com.tauri-app.token-widget\token-proxy`
  (override with `TOKEN_PROXY_DATA_DIR`; falls back to the script / exe directory).
- `usage.jsonl` — append-only metered records written by the proxy for proxied requests.
- `config.json` — provider/model configuration (user-specific, **never committed**).
- `traces_state.json` — incremental-scan checkpoint for the trace reader (mtime + size).
- `proxy.log` — operational log (no keys inside).

## Build & Release

```bash
# backend → standalone exe
cd token-proxy
pyinstaller -F -n token-proxy main.py
cp dist/token-proxy.exe ../token-widget/src-tauri/resources/

# frontend → installer
cd ../token-widget
npm run build
npm run tauri build        # NSIS installer under src-tauri/target/release/bundle/
```

## Repository Layout

```
workbuddy-token-meter/
├── token-proxy/           # Python backend: proxy + trace reader + aggregation
│   ├── main.py
│   ├── proxy/
│   │   ├── handler.py     # HTTP endpoints + transparent forwarding
│   │   ├── aggregate.py   # in-memory aggregation & persistence
│   │   ├── trace_reader.py# side-channel reader for WorkBuddy traces
│   │   └── config.py      # constants, routing, proxy resolution
│   ├── config.example.json
│   └── start_dev.bat
└── token-widget/          # Tauri 2 + React 19 desktop floating widget
    ├── src/               # React app (Recharts for trend charts)
    └── src-tauri/
```

## License

[MIT](./LICENSE)
