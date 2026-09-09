# workbuddy-token-meter

[![Version](https://img.shields.io/badge/version-0.3.0-blue.svg)](https://github.com/wakaka2023/workbuddy-token-meter/releases)
[![Platform](https://img.shields.io/badge/platform-Windows-0078d6.svg)]()
[![License](https://img.shields.io/badge/license-MIT-green.svg)](./LICENSE)

[中文](./README.md) | **English**

**A lightweight Windows desktop widget that tracks LLM token usage for
[WorkBuddy](https://www.workbuddy.cn) in real time.**

It never touches WorkBuddy itself or the request path — how many tokens were consumed, how much
came from cache, and how many credits were billed, all at a glance.

---

## Interface

<table>
  <tr>
    <td align="center" valign="top" width="300">
      <img src="./docs/screenshot-mini.png" width="290" alt="Mini mode" />
      <br />
      <sub><b>Mini mode</b> — sits in a corner of your desktop with the current model and per-call cost</sub>
    </td>
    <td align="center" valign="top" width="470">
      <img src="./docs/screenshot-expanded.png" width="460" alt="Expanded mode" />
      <br />
      <sub><b>Expanded mode</b> — click to expand for totals, trend chart, model table and channel breakdown</sub>
    </td>
  </tr>
</table>

## Features

- **Desktop widget** — borderless, translucent, always-on-top window with mini and expanded modes.
- **Non-intrusive** — reads WorkBuddy's local session logs on the side; no resident service or
  open port by default.
- **Multi-dimensional aggregation** — totals, by model and channel, and trend charts
  (last 24h / 7d / 1 month / all).
- **Channel awareness** — separates built-in from custom channels, so the same model is counted
  per channel.
- **Credit tracking** — calls on built-in channels also account for official credit consumption.
- **Local-first** — all data stays on your machine; keys are masked in the UI and never written to logs.

## Installation

Download the latest `token-widget_x64-setup.exe` from
[Releases](https://github.com/wakaka2023/workbuddy-token-meter/releases). An MSI package is
available as well.

## Usage

| Action | Description |
| --- | --- |
| Click the widget | Toggle between mini and expanded mode |
| Drag the widget | Move it anywhere on the desktop |
| Tray icon | Bring the window back or quit |
| Settings | Appearance / Stats / Channels / Models / Log — refresh interval, auto scan, theme |

## How it works

WorkBuddy appends each session to local JSONL files. This tool only reads those files, pairs
requests with their results by call ID, and aggregates them into a daily-sharded cache that can
be rebuilt in seconds after a restart.

```
┌──────────────┐   session logs (read-only)   ┌─────────────────────────┐
│  WorkBuddy   │ ──────────────────────────▶ │ Stats engine (Rust)     │
└──────────────┘  %USERPROFILE%\.workbuddy    │ · incremental scanning  │
                                             │ · daily-sharded cache   │
                                             └────────────┬────────────┘
                                                          │ in-memory snapshot
                                             ┌────────────▼────────────┐
                                             │ Widget (Tauri 2 + React)│
                                             └─────────────────────────┘
```

The bundled proxy is spawned on demand, only for custom models that need local forwarding.
Users on built-in models run without any child process.

## Development

Requires Node.js 20+ and the Rust toolchain.

```bash
cd token-widget
npm install
npm run tauri dev
```

Packaging: run `build_release.bat` on Windows. It backs up your private config and builds with a
sanitized one; artifacts land in `token-widget/src-tauri/target/release/bundle/`.

## Data & privacy

- Stats cache: `%APPDATA%\com.tauri-app.token-widget\stats-cache\` (derived data, rebuildable at any time)
- Config: `config.json` in the same directory (private, never committed)
- API keys are masked in every response and never logged

## License

[MIT](./LICENSE)
