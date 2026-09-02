# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned
- Long-term daily ledger (`daily-usage.json`) for history queries
- Cost calculation with a multi-source price library
- Sub-agent (expert team) token aggregation from `subagents/*.jsonl`

## [0.2.1] - 2026-09-02

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
