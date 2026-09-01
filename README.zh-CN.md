# workbuddy-token-meter

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)]()
[![Platform](https://img.shields.io/badge/platform-Windows-0078d6.svg)]()
[![Python](https://img.shields.io/badge/python-3.10+-3776AB.svg)]()
[![License](https://img.shields.io/badge/license-MIT-green.svg)]()

**中文** | [English](./README.md)

一个轻量的 **桌面悬浮窗 + 本地代理** 组合，实时统计 [WorkBuddy](https://www.workbuddy.cn)
每次 LLM 请求的 token 用量。不侵入 WorkBuddy 内部，一眼看清每个对话花了多少 token——
按模型、按天、按 provider 维度聚合展示。

---

## 功能特性

- 🖥️ **桌面悬浮窗** — 无边框、透明、置顶的迷你窗口，实时展示总调用次数、token 数、费用、
  按模型明细与按天趋势图。
- 🔀 **双通道记账** — 走本地代理的请求从上游 `usage` 响应记账（`usage.jsonl`）；
  **内置模型**与**直连上游的自定义模型**通过旁路读取 WorkBuddy 本地 trace 文件记账。
  无 Hook、无注入、对响应速度零影响。
- 🏷️ **Provider 感知标签** — 自定义模型按真实 provider 名标记（如 `B.AI`，从
  `models.json` 解析），内置模型标记为 `内置`。
- 📊 **多维聚合** — 总量、按模型、按天、按模型×天、最近 N 条记录，统一由 `/stats`
  接口输出。
- 🔌 **OpenAI 兼容本地代理** — 透明转发（同步 + SSE 流式），多 provider 路由
  （`b.ai` / `aliyun`），多 key 轮换，可重试错误自动退避重试，渠道级代理策略
  （自动探测本机 VPN）。
- 🔐 **默认安全** — API key 在一切响应中脱敏、绝不写入日志；仓库仅保留脱敏后的
  `config.example.json`。

## 架构

```
┌─────────────────────┐   POST /v1/chat/completions   ┌─────────────────────────────┐
│      WorkBuddy      │ ───────────────────────────▶ │ token-proxy  (Python, :8787) │
│  自定义模型 base URL  │                               │  · 路由并转发到上游            │
│ http://127.0.0.1:8787/v1                            │  · 记账 → usage.jsonl        │
└─────────────────────┘                               └──────────────┬──────────────┘
                                                                      │
┌─────────────────────┐   旁路读取（零侵入）      ┌────────────────────▼──────────────┐
│ WorkBuddy 本地 trace │ ───────────────────────▶ │ trace_reader（进程内）            │
│ ~/.workbuddy/traces/│                          │  · 内置模型 + 直连自定义模型        │
│   */trace_*.json    │                          │  · 增量扫描，30s 惰性 TTL         │
└─────────────────────┘                          └──────────────┬──────────────┘
                                                                 │ GET /stats
                                                         ┌───────▼────────┐
                                                         │  token-widget  │
                                                         │ (Tauri 2 + React)│
                                                         └────────────────┘
```

两条独立的记账通道汇入同一个内存聚合，任何请求都**恰好计一次**——无论它走代理转发
还是直连上游。

## 快速开始（开发模式）

> 前置依赖：Python 3.10+、Node.js 20+、Rust 工具链（仅 widget 需要）。

```bash
# 1. 后端 —— 配置并启动代理
cd token-proxy
cp config.example.json config.json   # 填入你的 API key
pip install requests
python main.py                       # 监听 http://127.0.0.1:8787

# 2. 让 WorkBuddy 指向代理
#    WorkBuddy → 设置 → 自定义模型，base URL 填：
#    http://127.0.0.1:8787/v1

# 3. 前端 —— 启动悬浮窗
cd ../token-widget
npm install
npm run tauri dev
```

Windows 上也可直接双击 `token-proxy/start_dev.bat` 以可见窗口运行代理。

## HTTP API

| 方法 | 路径                  | 说明                                                        |
|------|-----------------------|-------------------------------------------------------------|
| GET  | `/stats`              | 完整聚合（total / by_model / by_day / by_model_day / recent）|
| GET  | `/requests`           | 仅最近记录                                                  |
| GET  | `/health`             | 健康检查，附当前调用数                                      |
| GET  | `/config`             | 当前配置（API key 已脱敏）                                  |
| PUT  | `/config`             | 更新 models / providers / routes（key 按 id 合并还原）      |
| POST | `/keys`               | 管理 provider 的 key：`set`（激活）/ `add` / `del`          |
| POST | `/v1/chat/completions`| OpenAI 兼容透明转发（同步 + SSE）                           |

## 数据与存储

- **数据目录** — `%APPDATA%\com.tauri-app.token-widget\token-proxy`
  （可用环境变量 `TOKEN_PROXY_DATA_DIR` 覆盖；未设置时回退到脚本 / exe 所在目录）。
- `usage.jsonl` — 代理为转发请求写入的追加式记账文件。
- `config.json` — provider / 模型配置（用户私有，**绝不入库**）。
- `traces_state.json` — trace 读取器的增量扫描断点（mtime + size）。
- `proxy.log` — 运行日志（不含任何 key）。

## 构建与发布

```bash
# 后端 → 独立 exe
cd token-proxy
pyinstaller -F -n token-proxy main.py
cp dist/token-proxy.exe ../token-widget/src-tauri/resources/

# 前端 → 安装包
cd ../token-widget
npm run build
npm run tauri build        # NSIS 安装包在 src-tauri/target/release/bundle/
```

## 仓库结构

```
workbuddy-token-meter/
├── token-proxy/           # Python 后端：代理 + trace 读取 + 聚合
│   ├── main.py
│   ├── proxy/
│   │   ├── handler.py     # HTTP 端点 + 透明转发
│   │   ├── aggregate.py   # 内存聚合与持久化
│   │   ├── trace_reader.py# WorkBuddy trace 旁路读取器
│   │   └── config.py      # 常量、路由、代理解析
│   ├── config.example.json
│   └── start_dev.bat
└── token-widget/          # Tauri 2 + React 19 桌面悬浮窗
    ├── src/               # React 应用（Recharts 趋势图）
    └── src-tauri/
```

## License

[MIT](./LICENSE)
