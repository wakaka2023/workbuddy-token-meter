import { useEffect, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type {
  CacheScope,
  ModelRow,
  Mode,
  ProviderRow,
  ProxyConfig,
  SettingsTab,
  Stats,
  Theme,
} from "./types";
import {
  ACRYLIC_KEY,
  CACHE_SCOPE_KEY,
  EXPD_SIZE,
  ICON_CLOSE,
  ICON_COLLAPSE,
  ICON_EXPAND,
  ICON_MINIMIZE,
  ICON_SETTINGS,
  MINI_SIZE,
  POLL_MS,
  PROXY,
  THEME_BG_RGB,
  THEME_KEY,
  acrylicAlpha,
} from "./constants";
import { loadLS, saveLS } from "./utils";
import { fetchConfig, fetchStats, postKey, putConfig } from "./api";
import { Icon } from "./components/Icon";
import MiniView from "./components/MiniView";
import ExpandedView from "./components/ExpandedView";
import SettingsPanel from "./components/SettingsPanel";
import "./App.css";

function App() {
  const [stats, setStats] = useState<Stats | null>(null);
  const [online, setOnline] = useState<boolean | null>(null);
  const [mode, setMode] = useState<Mode>("mini");
  const [theme, setTheme] = useState<Theme>(() =>
    loadLS(THEME_KEY, "light", (v) => (v === "dark" ? "dark" : "light")),
  );
  const [acrylic, setAcrylic] = useState<number>(() =>
    loadLS(ACRYLIC_KEY, 60, (v) => {
      const n = Number(v);
      return Number.isFinite(n) ? Math.min(100, Math.max(0, n)) : null;
    }),
  );
  const [showSettings, setShowSettings] = useState(false);
  const [cacheScope, setCacheScope] = useState<CacheScope>(() =>
    loadLS(CACHE_SCOPE_KEY, "today", (v) => (v === "model" ? "model" : "today")),
  );
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("appearance");
  const [models, setModels] = useState<ModelRow[]>([]);
  const [providers, setProviders] = useState<ProviderRow[]>([]);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState("");
  const [newKey, setNewKey] = useState<Record<string, { name: string; key: string }>>({});

  const refresh = async () => {
    try {
      setStats(await fetchStats());
      setOnline(true);
    } catch {
      setOnline(false);
    }
  };

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, POLL_MS);
    return () => clearInterval(t);
  }, []);

  useEffect(() => saveLS(THEME_KEY, theme), [theme]);
  useEffect(() => saveLS(ACRYLIC_KEY, String(acrylic)), [acrylic]);
  useEffect(() => saveLS(CACHE_SCOPE_KEY, cacheScope), [cacheScope]);

  // 亚克力强度实时覆盖 widget 背景透明度
  useEffect(() => {
    const [top, bottom] = THEME_BG_RGB[theme];
    const a = acrylicAlpha(acrylic);
    const w = document.querySelector<HTMLElement>(".widget");
    if (w) {
      w.style.setProperty("--bg-top", `rgba(${top}, ${a})`);
      w.style.setProperty("--bg-bottom", `rgba(${bottom}, ${Math.min(1, a + 0.08)})`);
    }
  }, [theme, acrylic]);

  const close = () => getCurrentWindow().close();
  const minimize = () => getCurrentWindow().minimize();

  const toggleMode = async () => {
    const next = mode === "mini" ? "expanded" : "mini";
    const sz = next === "mini" ? MINI_SIZE : EXPD_SIZE;
    await getCurrentWindow().setSize(new LogicalSize(sz.w, sz.h));
    setMode(next);
    if (next === "mini") setShowSettings(false);
  };

  const handleKeyOp = async (body: Record<string, string>) => {
    setSaveMsg("");
    try {
      const res = await postKey(body);
      setProviders(
        providers.map((p) =>
          p.name === body.provider
            ? { ...p, keys: res.keys, activeKey: res.activeKey }
            : p,
        ),
      );
      setSaveMsg(`已切换 ${body.provider} → ${res.activeKey}`);
    } catch (e) {
      setSaveMsg(`操作失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const openSettings = async () => {
    if (mode === "mini") await toggleMode();
    setShowSettings(true);
    setSaveMsg("");
    try {
      const cfg = await fetchConfig();
      setModels(
        Object.entries(cfg.models).map(([id, m]) => ({
          id,
          provider: m.provider,
          input: String(m.price.input ?? 0),
          output: String(m.price.output ?? 0),
          cacheRead: String(m.price.cache_read ?? 0),
        })),
      );
      setProviders(
        Object.entries(cfg.providers).map(([name, p]) => {
          const raw = p.proxy ?? "auto";
          const isUrl = /^https?:\/\//i.test(raw);
          // 配置里存的是完整地址，UI 只展示端口号
          const portMatch = isUrl ? raw.match(/:(\d+)/) : null;
          return {
            name,
            base: p.base,
            label: p.label,
            proxy: raw,
            proxyMode: (isUrl ? "custom" : raw) as ProviderRow["proxyMode"],
            proxyUrl: portMatch ? portMatch[1] : "",
            keys: p.keys ?? [],
            activeKey: p.activeKey ?? "",
          };
        }),
      );
    } catch {
      setSaveMsg("读取配置失败，代理未运行？");
    }
  };

  const save = async () => {
    if (models.some((m) => !m.id.trim()) || providers.some((p) => !p.name.trim())) {
      setSaveMsg("模型 id 与 provider 名称不能为空");
      return;
    }
    setSaving(true);
    setSaveMsg("");
    try {
      const modelsObj: ProxyConfig["models"] = {};
      for (const m of models) {
        modelsObj[m.id.trim()] = {
          provider: m.provider,
          price: {
            input: Number(m.input) || 0,
            output: Number(m.output) || 0,
            cache_read: Number(m.cacheRead) || 0,
          },
        };
      }
      const providersObj: ProxyConfig["providers"] = {};
      for (const p of providers) {
        providersObj[p.name.trim()] = {
          base: p.base.trim(),
          label: p.label.trim() || p.name.trim(),
          proxy:
            p.proxyMode === "custom"
              ? p.proxyUrl.trim()
                ? `http://127.0.0.1:${p.proxyUrl.trim()}`
                : "auto"
              : p.proxyMode,
          keys: p.keys,
          activeKey: p.activeKey || undefined,
        };
      }
      const res = await putConfig({ models: modelsObj, providers: providersObj });
      setSaveMsg(`已保存并热加载（模型 ${res.models} / 渠道 ${res.providers}）`);
      refresh();
      setTimeout(() => setShowSettings(false), 900);
    } catch (e) {
      setSaveMsg(`保存失败：${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  };

  // 当前模型的今日缓存率（后端 by_model_day；缺失时回退全模型口径）
  const today = stats?.by_day[stats.by_day.length - 1];
  const todayCacheRate =
    today && today.prompt_tokens > 0
      ? ((today.cache_read_tokens / today.prompt_tokens) * 100).toFixed(1)
      : "0.0";
  const lastRec = stats?.records?.[stats.records.length - 1] ?? null;
  const modelTodayCacheRate = (() => {
    if (!lastRec || !stats?.by_model_day) return todayCacheRate;
    const day = new Date();
    const pad = (n: number) => String(n).padStart(2, "0");
    const key = `${day.getFullYear()}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`;
    const m = stats.by_model_day[key]?.[String(lastRec.model ?? "")];
    if (!m || m.prompt_tokens <= 0) return todayCacheRate;
    return ((m.cache_read_tokens / m.prompt_tokens) * 100).toFixed(1);
  })();

  const providerNames = providers.map((p) => p.name);
  const miniFailed = mode === "mini" && lastRec ? lastRec.ok === false : false;

  return (
    <div className={`widget mode-${mode} theme-${theme}${miniFailed ? " mini-failed" : ""}`}>
      <header className="titlebar" data-tauri-drag-region>
        <span className="title" data-tauri-drag-region>
          {mode === "mini" ? "Token" : "Token Widget"}
        </span>
        <div className="title-actions">
          <button
            className="icon-btn"
            onClick={toggleMode}
            title={mode === "mini" ? "展开详细" : "收缩简洁"}
          >
            <Icon d={mode === "mini" ? ICON_EXPAND : ICON_COLLAPSE} />
          </button>
          {mode === "expanded" && (
            <button className="icon-btn" onClick={openSettings} title="设置">
              <Icon d={ICON_SETTINGS} />
            </button>
          )}
          <button className="icon-btn" onClick={minimize} title="最小化">
            <Icon d={ICON_MINIMIZE} />
          </button>
          <button className="close-btn" onClick={close} title="关闭">
            <Icon d={ICON_CLOSE} />
          </button>
        </div>
      </header>

      <div className={`body-wrap ${mode === "expanded" ? "expanded-body" : ""}`}>
        {mode === "mini" ? (
          <MiniView
            online={online}
            lastRec={lastRec}
            cacheScope={cacheScope}
            todayCacheRate={todayCacheRate}
            modelTodayCacheRate={modelTodayCacheRate}
          />
        ) : showSettings ? (
          <SettingsPanel
            theme={theme}
            setTheme={setTheme}
            acrylic={acrylic}
            setAcrylic={setAcrylic}
            cacheScope={cacheScope}
            setCacheScope={setCacheScope}
            settingsTab={settingsTab}
            setSettingsTab={setSettingsTab}
            models={models}
            setModels={setModels}
            providers={providers}
            setProviders={setProviders}
            providerNames={providerNames}
            newKey={newKey}
            setNewKey={setNewKey}
            saveMsg={saveMsg}
            setSaveMsg={setSaveMsg}
            saving={saving}
            onSave={save}
            onClose={() => setShowSettings(false)}
            onKeyOp={handleKeyOp}
          />
        ) : (
          <ExpandedView stats={stats} />
        )}
      </div>

      {mode === "expanded" && (
        <footer className="statusbar">
          <span className={`dot ${online === false ? "err" : "ok"}`} />
          {online === null
            ? "连接中…"
            : online
              ? "代理正常"
              : `无法连接 ${PROXY}`}
          <span className="status-right">{POLL_MS / 1000}s</span>
        </footer>
      )}
    </div>
  );
}

export default App;
