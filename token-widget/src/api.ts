import type { ProxyConfig, ProviderCfg, Stats } from "./types";
import { PROXY } from "./constants";

export async function fetchStats(): Promise<Stats> {
  const r = await fetch(`${PROXY}/stats`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function fetchConfig(): Promise<ProxyConfig> {
  const r = await fetch(`${PROXY}/config`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function putConfig(payload: {
  models: ProxyConfig["models"];
  providers: Record<string, ProviderCfg>;
}): Promise<{ models: number; providers: number }> {
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
