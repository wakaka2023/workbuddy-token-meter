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
  credits: number; // 官方积分扣费（仅内置渠道，jsonl rawUsage.credit 聚合）
}

export interface Stats {
  /** 聚合版本号：仅数据真正变化时 +1，前端据此跳过无变化的整包重渲染 */
  gen?: number;
  total: Record<string, number>;
  by_model: Record<string, ModelAgg>;
  by_day: DayAgg[];
  /** 小时级聚合（键为 YYYY-MM-DD HH），供趋势图近 24 小时视图 */
  by_hour?: DayAgg[];
  by_model_day?: Record<string, Record<string, DayAgg>>;
  last_success: Record<string, string>;
  records: Array<Record<string, unknown>>;
}

/** get_stats 响应：gen 未变时 data 为 null，前端保留旧 stats 跳过重渲染 */
export interface StatsResp {
  gen: number;
  data: Stats | null;
}

export interface KeyCfg {
  id: string;
  name: string;
  key: string;
}

export interface ChannelCfg {
  label?: string;
  base?: string;
  proxy?: string; // 网络通道：auto(默认)/direct/http(s)://显式地址
  keys?: KeyCfg[];
  activeKey?: string;
  availableModels?: string[]; // fetch_models 拉取的可用模型列表
  fetchedAt?: string; // 拉取时间
}

export interface ModelCfg {
  name?: string; // WorkBuddy 侧显示名（展示用）
  channel?: string; // 所属渠道名
  key?: string; // 指定该渠道 key 池中的 key id；未指定跟随渠道 activeKey
  price?: { input: number; output: number; cache_read: number };
  defaults?: Record<string, unknown>;
}

export interface ProxyConfig {
  _comment?: string;
  channels: Record<string, ChannelCfg>;
  models: Record<string, ModelCfg>;
}

export interface ChannelRow {
  id: string;
  label: string;
  base: string;
  proxyMode: "auto" | "direct" | "custom"; // UI 选择
  proxyUrl: string; // custom 模式下的代理地址（端口）
  keys: KeyCfg[];
  activeKey: string;
  availableModels: string[];
  fetchedAt: string;
}

export interface ModelRow {
  id: string; // 模型 id：渠道上游真实模型名（请求体 model），必须对应渠道
  name: string; // 显示名（WorkBuddy 里展示的名字）
  channel: string; // 所属渠道
  key: string; // 该渠道 key 池中的 key id
  input: string;
  output: string;
  cacheRead: string;
}

export type Mode = "mini" | "expanded";
export type Theme = "dark" | "light";
export type CacheScope = "today" | "model";
export type SettingsTab = "appearance" | "channels" | "models" | "scan" | "config";

export interface ProxyStatus {
  running: boolean; // 代理进程是否存活（/health 校验过）
  needed: boolean; // models.json 是否存在走代理路由的自定义模型
  mode: "service" | "full";
  port: number;
  forwarded: number;
  uptime: number;
  has_key: boolean;
  config_channels: number; // 本地 config.json 计数（判断是否仅统计模式）
  config_models: number;
}

export interface RouteModel {
  id: string;
  name: string;
  url: string;
  route: "proxy" | "direct";
  has_key: boolean;
  origin_url?: string;
}

export interface LedgerModel {
  name: string;
  channel: string | null;
  origin_url: string | null;
  current_url: string;
  route: "proxy" | "direct";
  key_ref: string | null;
  updated_at?: string;
}

export interface LedgerLog {
  ts: string;
  op: string;
  detail: string;
}

export interface LedgerSummary {
  models: number;
  logs: number;
}

export interface LedgerData {
  models: LedgerModel[];
  changelog: LedgerLog[];
  summary: LedgerSummary;
}

export interface ScanProgress {
  running: boolean;
  total: number;
  scanned: number;
  records: number;
  done: boolean;
}
