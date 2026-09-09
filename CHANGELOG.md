# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned
- Long-term daily ledger (`daily-usage.json`) for history queries
- Cost calculation with a multi-source price library
- Sub-agent (expert team) token aggregation from `subagents/*.jsonl`

## [0.3.0] - 2026-09-09

### Added

- Mini window shows per-request credit consumption.
- Expanded view: trend chart range selector (24h / 7d / 30d / all) and recent-request limit.
- Settings: new **Stats** and **Log** tabs.

### Changed

- **Stats source switched to WorkBuddy session JSONL** (`~/.workbuddy/projects/**/*.jsonl`),
  replacing the trace-based pipeline.
- Settings reorganized into five tabs (Appearance / Stats / Channels / Models / Log).
- Model table splits rows by (model, channel), each channel counted independently.
- Channel attribution derived from `models.json` timeline snapshots instead of model display names.

### Fixed

- Full scan no longer double-counts already scanned records.
- No more UI stutter while polling: aggregation moved to an immutable snapshot with generation-based skipping.
- Model table: display names no longer leak across channels, and built-in case variants
  (e.g. `Deepseek-V4-Flash` / `deepseek-v4-flash`) merge into one row.
- Trend chart fills missing hours/days with zeroes instead of leaving gaps.
- Quitting hides the window immediately instead of waiting for the proxy handshake.

## [0.2.4] - 2026-09-02

### Changed

- **Architecture: trace-first single-channel (stats no longer depend on the proxy)**.
  The statistics engine and config management now live inside the Rust widget; the
  Python proxy is demoted to an optional, on-demand forwarder. Users who only use
  built-in WorkBuddy models run with **no proxy process and no open 8787 port** —
  this fixes the v0.2.3 failure where stats/config/scan all broke when the proxy
  could not start.
  - New local Rust engine (`engine.rs`) scans `~/.workbuddy/traces` directly,
    incremental `(mtime, size)` scan with `(traceId, spanId)` upsert dedup, and a
    daily-shard cache marked `"engine":"widget-v1"` (old Python state ignored →
    triggers one full rebuild). `snapshot()` keeps the exact legacy `/stats` JSON
    contract, so the frontend stats views required zero changes.
  - New local config store (`configstore.rs`) ports config / import / route-switch /
    ledger from the Python side: masked responses, empty-payload guard, key ops,
    WorkBuddy import, direct↔proxy model routing with 5-deep models.json backups.
  - Proxy is launched on demand only when `models.json` contains a model routed to
    `127.0.0.1:8787`, and liveness is verified via `/health` (fixes the old
    "port occupied by another process = false alive" bug).
  - Proxy runs in **lean mode** when spawned by the widget (`TOKEN_PROXY_LEAN=1`):
    forwarding only, no trace-cache/scan threads (no double-writing the cache the
    Rust engine now owns). Proxy config is hot-reloaded via mtime polling so
    widget-written config applies to a running proxy.
- **Empty default config**: both `config.example.json` files and the bundled
  `resources/config.json` are now an empty template
  (`{"channels": {}, "models": {}}`) — no template/placeholder keys to confuse
  built-in-only users.
- **Frontend rewired to Tauri commands**: stats/config/scan/ledger/route/key/proxy
  control all call `invoke(...)` instead of HTTP against the proxy; only upstream
  model discovery (`fetch_models`, which genuinely needs the proxy + a real key)
  still uses HTTP, with a graceful `proxy_down` fallback.
- Version bumped to 0.2.4.

### Fixed

- Model-name extraction from trace `toolInput`: the Rust engine now searches the
  extracted system content for "powered by" instead of using the whole content as
  the model name (no more multi-KB garbage names).
- Non-ASCII model names (e.g. `GLM-5.3-Flash(B.AI测试)`) are no longer truncated
  at the first UTF-8 continuation byte, so Chinese custom-model names match
  `models.json` and are labeled as custom instead of `内置`.
- `provider_from_url` falls back to `自定义` for local proxy hosts instead of
  deriving a meaningless label.

## [0.2.3] - 2026-09-02

### Added

- **Exit / window close kills backend process** — `POST /shutdown` endpoint flushes
  pending records and exits cleanly; Rust side calls it before `app.exit(0)` and on
  `RunEvent::Exit` as a fallback; frontend × button triggers `quit_app` Tauri command.
- **Configurable refresh interval** — new setting in the appearance tab (5–600s, step 5),
  persisted to localStorage; `POLL_MS` replaced by a dynamic `pollMs` state that drives
  both the frontend polling interval and the backend trace-scan TTL (via `POST /config/poll`).
- **Last-refresh timestamp** — shows the time of the last successful `/stats` poll
  in the status bar (removed after testing, functionality retained).
- **Provider label fix for proxy-routed models** — changed `_model_label` to return
  the channel provider display name (e.g. `B.AI`) instead of the model display name
  (`GLM-5.3-Flash(B.AI测试)`), unifying labels across proxy and trace paths.

### Changed

- **mini window now uses a fixed size** (240×132) instead of content-adaptive sizing,
  removing the ResizeObserver / fit-content complexity that caused unwanted "auto-resize"
  behaviour. Users can freely drag to resize after switching modes.
- **CSS split** — monolithic `App.css` (1416 lines) split into 6 module files under
  `src/styles/`: `theme.css`, `base.css`, `mini.css`, `titlebar.css`, `settings.css`,
  `stats.css`.
- **SettingsPanel split** — 820-line component split into 5 tab components:
  `AppearanceTab`, `ChannelTab`, `ModelTab`, `ScanTab`, `ConfigTab`.
- **handler.py refactored** — the 200-line `do_POST` forwarding logic extracted into
  `_forward_chat()`; added `_send_error()` helper to eliminate repeated error-response
  boilerplate.
- **Version bumped** to 0.2.3 (from 0.1.0).
- **README now defaults to Chinese** — main `README.md` shows Chinese content;
  English version moved to `README.en.md`.

### Added

- **Channel + model two-entity config** (`channels` + `models` in `config.json`):
  channels own `base` / proxy strategy / API key pool, models reference a channel
  and optionally pin one of its keys. Key pool is maintained once per channel;
  different models under the same provider can each use a different key
  (e.g. `deepseek-v4-flash` vs `glm-5.3-flash` under `b.ai`).
- **Fetch upstream model list** (`POST /channels/{name}/fetch_models`): queries
  `GET {base}/models` with the channel's active key and caches `availableModels` /
  `fetchedAt` on the channel row — one-click model discovery for OpenAI-compatible
  providers (b.ai exposes 44 models). Unsupported providers degrade gracefully:
  manual model entry is always available.
- **Simplified settings UI**: new "渠道" tab (provider card: base / proxy / key
  pool chips / fetch button); "模型" tab reduced to a single slim row per model
  (name → channel dropdown → key dropdown → price); WorkBuddy route switch block
  merged into the models tab; removed the standalone "模型路由" tab.
- **Masked-key drop guard on PUT** (channels layer): masked keys that cannot be
  resolved to a real key are dropped with a log line instead of being persisted,
  preventing upstream 401s caused by masking on disk.

### Fixed

- Lock-ordering deadlock in `PUT /config` (logging inside the shared lock).
- Config storage now keeps keys only on channels; model rows never hold key
  material, shrinking the masked-key surface.

## [0.2.0] - 2026-09-02

### Added

- **Proxy dual-mode** (`service` / `full`): scanning & statistics available
  without any key; forwarding enabled after a key is configured (`POST /mode/start`).
- **Trace scan cache** (`trace-cache/`): parsed, de-identified daily shards make
  restarts near-instant; incremental upsert + force full rebuild + progress endpoint.
- **Config ledger** (`config-ledger.json`): versioned snapshots, model registry
  with immutable `origin_url`, and an operation changelog with rollback.
- **One-click WorkBuddy import** (`POST /import/workbuddy`): imports custom models
  from `~/.workbuddy/models.json`, groups them into channels by URL host.
- **Route switch** (`POST /models/{name}/route`): flip a WorkBuddy custom model
  between direct and proxy routing (backed up, reversible).

### Fixed

- Double counting of proxied requests in `/stats`.
- Non-reentrant lock deadlock in scan (`maybe_scan`).
- Custom-model label regression in the trace reader.
- Front-end saving an empty config over a real one (double guard on client &
  server).

## [0.1.0] - 2026-09-02

### Added

- **Desktop floating widget** (Tauri 2 + React 19 + Recharts): frameless, transparent,
  always-on-top mini window with live totals, per-model breakdown and daily trend chart.
- **Dual-channel token accounting**:
  - proxied requests metered from upstream `usage` responses (`usage.jsonl`);
  - built-in models and directly-connected custom models metered by a passive
    side-channel reader over WorkBuddy local trace files (incremental scan, 30s TTL).
- **Provider-aware labels**: custom models tagged with their real provider
  (e.g. `B.AI`), built-in models tagged `内置`.
- **Aggregation API** (`GET /stats`): total / by_model / by_day / by_model_day /
  recent records.
- **OpenAI-compatible local proxy** (`:8787`): transparent sync + SSE streaming
  forwarding, multi-provider routing (`b.ai` / `aliyun`), multi-key rotation
  (activate / add / delete via `POST /keys`), retry with backoff on retryable
  errors, per-provider proxy strategy (auto probe / direct / forced).
- **Security defaults**: API keys masked in all responses and never written to
  logs; repository ships only a sanitized `config.example.json`.
- Bilingual documentation (English + 简体中文) and MIT license.
