# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Planned
- Long-term daily ledger (`daily-usage.json`) for history queries
- Cost calculation with a multi-source price library
- Sub-agent (expert team) token aggregation from `subagents/*.jsonl`

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
