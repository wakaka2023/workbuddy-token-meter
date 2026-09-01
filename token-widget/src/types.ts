export interface DayAgg {
  date: string;
  prompt_tokens: number;
  completion_tokens: number;
  cache_read_tokens: number;
  calls: number;
}

export interface ModelAgg {
  label: string;
  prompt_tokens: number;
  completion_tokens: number;
  reasoning_tokens: number;
  cache_read_tokens: number;
  calls: number;
  cost: number;
}

export interface Stats {
  total: Record<string, number>;
  by_model: Record<string, ModelAgg>;
  by_day: DayAgg[];
  by_model_day?: Record<string, Record<string, DayAgg>>;
  last_success: Record<string, string>;
  records: Array<Record<string, unknown>>;
}

export interface ProviderCfg {
  base: string;
  label: string;
  proxy?: string; // 网络通道：auto(默认)/direct/env/http(s)://显式地址
  keys?: Array<{ id: string; name: string; key: string }>;
  activeKey?: string;
}

export interface ProxyConfig {
  _comment?: string;
  models: Record<
    string,
    {
      provider: string;
      price: { input: number; output: number; cache_read: number };
      defaults?: Record<string, unknown>;
    }
  >;
  providers: Record<string, ProviderCfg>;
  routes: Record<string, string>;
}

export interface ModelRow {
  id: string;
  provider: string;
  input: string;
  output: string;
  cacheRead: string;
}

export interface ProviderRow {
  name: string;
  base: string;
  label: string;
  proxy: string; // 持久化值：auto/direct/http(s)://...
  proxyMode: "auto" | "direct" | "custom"; // UI 选择
  proxyUrl: string; // custom 模式下的代理地址
  keys: Array<{ id: string; name: string; key: string }>;
  activeKey: string;
}

export type Mode = "mini" | "expanded";
export type Theme = "dark" | "light";
export type CacheScope = "today" | "model";
export type SettingsTab = "appearance" | "models" | "providers" | "keys";
