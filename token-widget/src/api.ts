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
import { PROXY } from "./constants";

export async function fetchStats(): Promise<Stats> {
  const r = await fetch(`${PROXY}/stats`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function fetchStatus(): Promise<ProxyStatus> {
  const r = await fetch(`${PROXY}/status`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function fetchRouteModels(): Promise<RouteModel[]> {
  const r = await fetch(`${PROXY}/models`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  const d = await r.json();
  return d.models ?? [];
}

export async function fetchLedger(): Promise<LedgerData> {
  const r = await fetch(`${PROXY}/config/ledger`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function fetchScanProgress(): Promise<ScanProgress> {
  const r = await fetch(`${PROXY}/scan/progress`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function triggerScan(force: boolean): Promise<{ ok: boolean; started: string }> {
  const r = await fetch(`${PROXY}/scan`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ force }),
  });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.error ?? `HTTP ${r.status}`);
  return res;
}

export async function importWorkbuddy(): Promise<Record<string, unknown>> {
  const r = await fetch(`${PROXY}/import/workbuddy`, { method: "POST" });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.error ?? `HTTP ${r.status}`);
  return res;
}

export async function switchRoute(
  name: string,
  route: "proxy" | "direct",
): Promise<{ ok: boolean; message: string }> {
  const r = await fetch(`${PROXY}/models/${encodeURIComponent(name)}/route`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ route }),
  });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.message ?? res.error ?? `HTTP ${r.status}`);
  return res;
}

export async function modeStart(): Promise<ProxyStatus> {
  const r = await fetch(`${PROXY}/mode/start`, { method: "POST" });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.error ?? res.message ?? `HTTP ${r.status}`);
  return res;
}

export async function modeStop(): Promise<ProxyStatus> {
  const r = await fetch(`${PROXY}/mode/stop`, { method: "POST" });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.error ?? res.message ?? `HTTP ${r.status}`);
  return res;
}

export async function fetchConfig(): Promise<ProxyConfig> {
  const r = await fetch(`${PROXY}/config`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function putConfig(payload: {
  channels: Record<string, ChannelCfg>;
  models: Record<string, ModelCfg>;
}): Promise<{ channels: number; models: number }> {
  const r = await fetch(`${PROXY}/config`, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.error ?? `HTTP ${r.status}`);
  return res;
}

export async function postKey(
  body: Record<string, string>,
): Promise<{ keys: Array<{ id: string; name: string; key: string }>; activeKey: string }> {
  const r = await fetch(`${PROXY}/keys`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const res = await r.json();
  if (!r.ok || !res.ok) throw new Error(res.error ?? `HTTP ${r.status}`);
  return res;
}

export async function fetchChannelModels(
  channel: string,
): Promise<{ ok: boolean; models: string[]; count: number; cached_at: string; error?: string; hint?: string }> {
  const r = await fetch(`${PROXY}/channels/${encodeURIComponent(channel)}/fetch_models`, {
    method: "POST",
  });
  const res = await r.json();
  return res;
}
