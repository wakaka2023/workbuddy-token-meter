import { invoke } from "@tauri-apps/api/core";
import type {
  ChannelCfg,
  LedgerData,
  ModelCfg,
  ProxyConfig,
  ProxyStatus,
  RouteModel,
  ScanProgress,
  Stats,
} from "./types";

// 统计 / 配置 / 扫描 / 代理控制 全部走本地 Tauri 命令（v0.2.4 起不依赖 8787）。
// 仅「拉取上游渠道模型列表」需要代理进程（真实 key + 出网），走 proxyApi。

export async function fetchStats(): Promise<Stats> {
  return invoke<Stats>("get_stats");
}

export async function fetchStatus(): Promise<ProxyStatus> {
  return invoke<ProxyStatus>("get_status");
}

export async function fetchRouteModels(): Promise<RouteModel[]> {
  const d = await invoke<{ models: RouteModel[] }>("get_route_models");
  return d.models ?? [];
}

export async function fetchLedger(): Promise<LedgerData> {
  return invoke<LedgerData>("get_ledger");
}

export async function fetchScanProgress(): Promise<ScanProgress> {
  return invoke<ScanProgress>("get_scan_progress");
}

export async function triggerScan(force: boolean): Promise<{ ok: boolean; started: string }> {
  return invoke<{ ok: boolean; started: string }>("force_scan", { force });
}

export async function importWorkbuddy(): Promise<Record<string, unknown>> {
  return invoke<Record<string, unknown>>("import_workbuddy");
}

export async function switchRoute(
  name: string,
  route: "proxy" | "direct",
): Promise<{ ok: boolean; message: string; url: string }> {
  return invoke<{ ok: boolean; message: string; url: string }>("switch_route", { name, route });
}

export async function modeStart(): Promise<void> {
  await invoke<{ ok: boolean }>("proxy_start");
}

export async function modeStop(): Promise<ProxyStatus> {
  await invoke<{ ok: boolean }>("proxy_stop");
  return fetchStatus();
}

export async function fetchConfig(): Promise<ProxyConfig> {
  return invoke<ProxyConfig>("get_config");
}

export async function putConfig(payload: {
  channels: Record<string, ChannelCfg>;
  models: Record<string, ModelCfg>;
}): Promise<{ channels: number; models: number }> {
  return invoke<{ channels: number; models: number }>("put_config", { payload });
}

export async function postKey(
  body: Record<string, string>,
): Promise<{ keys: Array<{ id: string; name: string; key: string }>; activeKey: string }> {
  return invoke<{ keys: Array<{ id: string; name: string; key: string }>; activeKey: string }>("key_op", {
    body,
  });
}

// 代理 HTTP：仅拉取上游 /models（需要代理运行 + 真实 key 出网）
const PROXY = "http://127.0.0.1:8787";

export async function fetchChannelModels(
  channel: string,
): Promise<{ ok: boolean; models: string[]; count: number; cached_at: string; error?: string; hint?: string }> {
  let r: Response;
  try {
    r = await fetch(`${PROXY}/channels/${encodeURIComponent(channel)}/fetch_models`, {
      method: "POST",
    });
  } catch {
    return {
      ok: false,
      models: [],
      count: 0,
      cached_at: "",
      error: "proxy_down",
      hint: "代理未运行：请在「配置管理」页启动代理后再拉取模型列表",
    };
  }
  const res = await r.json().catch(() => ({}));
  return res;
}

export async function setPollInterval(ms: number): Promise<{ ok: boolean; interval_ms: number; scan_ttl: number }> {
  return invoke<{ ok: boolean; interval_ms: number; scan_ttl: number }>("set_scan_interval", {
    intervalMs: ms,
  });
}
