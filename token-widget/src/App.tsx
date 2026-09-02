import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type {
  CacheScope,
  ChannelCfg,
  ChannelRow,
  LedgerData,
  ModelCfg,
  ModelRow,
  Mode,
  ProxyStatus,
  RouteModel,
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
  POLL_KEY,
  POLL_MS,
  THEME_BG_RGB,
  THEME_KEY,
  acrylicAlpha,
} from "./constants";
import { fmtTime, loadLS, saveLS } from "./utils";
import {
  fetchChannelModels,
  fetchConfig,
  fetchLedger,
  fetchRouteModels,
  fetchScanProgress,
  fetchStats,
  fetchStatus,
  importWorkbuddy,
  modeStart,
  modeStop,
  switchRoute,
  triggerScan,
  postKey,
  putConfig,
  setPollInterval,
} from "./api";
import { Icon } from "./components/Icon";
import MiniView from "./components/MiniView";
import ExpandedView from "./components/ExpandedView";
import SettingsPanel from "./components/SettingsPanel";
import "./styles/theme.css";
import "./styles/base.css";
import "./styles/mini.css";
import "./styles/titlebar.css";
import "./styles/settings.css";
import "./styles/stats.css";

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
  const [pollMs, setPollMs] = useState<number>(() =>
    loadLS(POLL_KEY, POLL_MS, (v) => {
      const n = Number(v);
      return Number.isFinite(n) && n >= 1000 && n <= 600000 ? n : null;
    }),
  );
  const [channels, setChannels] = useState<ChannelRow[]>([]);
  const [models, setModels] = useState<ModelRow[]>([]);
  const [saving, setSaving] = useState(false);
  const [saveMsg, setSaveMsg] = useState("");
  const [newKey, setNewKey] = useState<Record<string, { name: string; key: string }>>({});
  // v0.2 新增：代理状态 / 模型路由 / 台账
  const [status, setStatus] = useState<ProxyStatus | null>(null);
  const [routeModels, setRouteModels] = useState<RouteModel[]>([]);
  const [ledger, setLedger] = useState<LedgerData | null>(null);
  const [scanProgress, setScanProgress] = useState<{ running: boolean; total: number; scanned: number; records: number; done: boolean } | null>(null);
  const [opMsg, setOpMsg] = useState("");

  // 扫描进度轮询：running 期间每 1s 拉一次 /scan/progress
  useEffect(() => {
    if (!scanProgress?.running) return;
    const t = setInterval(async () => {
      try {
        setScanProgress(await fetchScanProgress());
      } catch {
        /* 忽略 */
      }
    }, 1000);
    return () => clearInterval(t);
  }, [scanProgress?.running]);

  const refresh = async () => {
    try {
      setStats(await fetchStats());
      setOnline(true);
      try {
        setStatus(await fetchStatus());
      } catch {
        /* status 可选 */
      }
    } catch {
      setOnline(false);
    }
  };

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, pollMs);
    return () => clearInterval(t);
  }, [pollMs]);

  useEffect(() => saveLS(THEME_KEY, theme), [theme]);
  useEffect(() => saveLS(ACRYLIC_KEY, String(acrylic)), [acrylic]);
  useEffect(() => saveLS(CACHE_SCOPE_KEY, cacheScope), [cacheScope]);
  useEffect(() => saveLS(POLL_KEY, String(pollMs)), [pollMs]);

  // 刷新频率全局统一：把前端设置同步到后端 trace 扫描间隔（后台静默，失败不阻塞）
  useEffect(() => {
    setPollInterval(pollMs).catch(() => {});
  }, [pollMs]);

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

  const close = () => invoke("quit_app");
  const minimize = () => getCurrentWindow().minimize();

  const toggleMode = async () => {
    const next = mode === "mini" ? "expanded" : "mini";
    if (next === "mini") {
      setMode(next);
      setShowSettings(false);
      // 固定一个稍小的 mini 尺寸；用户之后可自行拖拽微调，不强制覆盖
      await getCurrentWindow().setSize(new LogicalSize(MINI_SIZE.w, MINI_SIZE.h));
    } else {
      await getCurrentWindow().setSize(new LogicalSize(EXPD_SIZE.w, EXPD_SIZE.h));
      setMode(next);
    }
  };

  const handleKeyOp = async (body: Record<string, string>) => {
    setSaveMsg("");
    try {
      const res = await postKey(body);
      const cid = body.channel;
      setChannels(
        channels.map((c) =>
          c.id === cid ? { ...c, keys: res.keys, activeKey: res.activeKey ?? "" } : c,
        ),
      );
      setSaveMsg(`已保存渠道 ${cid} 的 key 配置`);
    } catch (e) {
      setSaveMsg(`操作失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  // 拉取渠道可用模型列表（b.ai 等支持 /models 的上游）；失败提示但保留手填入口
  const handleFetchModels = async (cid: string) => {
    setSaveMsg("");
    try {
      const res = await fetchChannelModels(cid);
      if (!res.ok) {
        setSaveMsg(res.hint ?? res.error ?? `拉取模型失败（渠道 ${cid}）`);
        return;
      }
      setChannels(
        channels.map((c) =>
          c.id === cid ? { ...c, availableModels: res.models, fetchedAt: res.cached_at } : c,
        ),
      );
      setSaveMsg(`渠道 ${cid} 拉取到 ${res.count} 个模型`);
    } catch (e) {
      setSaveMsg(`拉取失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleModeStart = async () => {
    try {
      const s = await modeStart();
      setStatus(s);
      setOpMsg("代理已启动");
    } catch (e) {
      setOpMsg(`启动失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleModeStop = async () => {
    try {
      const s = await modeStop();
      setStatus(s);
      setOpMsg("代理已停止（仅统计模式不受影响）");
    } catch (e) {
      setOpMsg(`停止失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleImport = async () => {
    setOpMsg("导入中…");
    try {
      const res = await importWorkbuddy();
      setOpMsg(`导入完成：${res.imported ?? 0} 个直连模型，${res.skipped ?? 0} 个跳过`);
      await loadSettings();
    } catch (e) {
      setOpMsg(`导入失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleRouteSwitch = async (name: string, route: "proxy" | "direct") => {
    try {
      const res = await switchRoute(name, route);
      setOpMsg(res.message);
      await loadSettings();
    } catch (e) {
      setOpMsg(`切换失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const handleScan = async (force: boolean) => {
    try {
      await triggerScan(force);
      setScanProgress({ running: true, total: 0, scanned: 0, records: 0, done: false });
      setOpMsg(force ? "已开始全量重建" : "已开始增量扫描");
    } catch (e) {
      setOpMsg(`扫描触发失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const openSettings = async () => {
    if (mode === "mini") await toggleMode();
    setShowSettings(true);
    await loadSettings();
  };

  // 拉取配置到编辑 state（打开面板 / 手动刷新 / 导入路由操作后调用）
  const loadSettings = async () => {
    setSaveMsg("");
    try {
      const cfg = await fetchConfig();
      setChannels(
        Object.entries(cfg.channels ?? {}).map(([id, ch]) => {
          const raw = ch.proxy ?? "auto";
          const isUrl = /^https?:\/\//i.test(raw);
          const portMatch = isUrl ? raw.match(/:(\d+)/) : null;
          return {
            id,
            label: ch.label ?? id,
            base: ch.base ?? "",
            proxyMode: (isUrl ? "custom" : raw) as ChannelRow["proxyMode"],
            proxyUrl: portMatch ? portMatch[1] : "",
            keys: ch.keys ?? [],
            activeKey: ch.activeKey ?? "",
            availableModels: ch.availableModels ?? [],
            fetchedAt: ch.fetchedAt ?? "",
          };
        }),
      );
      setModels(
        Object.entries(cfg.models ?? {}).map(([id, m]) => {
          const p = m.price;
          return {
            id,
            name: (m as any).name ?? (m as any).label ?? "", // 兼容后端 label→name 过渡
            channel: m.channel ?? "",
            key: m.key ?? "",
            input: String(p?.input ?? 0),
            output: String(p?.output ?? 0),
            cacheRead: String(p?.cache_read ?? 0),
          };
        }),
      );
      setRouteModels(await fetchRouteModels());
      setLedger(await fetchLedger());
      setStatus(await fetchStatus());
      setSaveMsg("");
    } catch (e) {
      setSaveMsg(`读取配置失败：${e instanceof Error ? e.message : String(e)}`);
    }
  };

  const save = async () => {
    if (models.some((m) => !m.id.trim())) {
      setSaveMsg("模型名不能为空");
      return;
    }
    if (channels.some((c) => !c.id.trim())) {
      setSaveMsg("渠道名不能为空");
      return;
    }
    // 空保护：没有任何渠道/模型时拒绝保存，防止空配置覆盖真实配置
    if (channels.length === 0 && models.length === 0) {
      setSaveMsg("暂无渠道/模型可保存：仅统计模式无需配置；如需转发请先一键导入或添加渠道");
      return;
    }
    setSaving(true);
    setSaveMsg("");
    try {
      const channelsObj: Record<string, ChannelCfg> = {};
      for (const c of channels) {
        channelsObj[c.id.trim()] = {
          label: c.label.trim() || c.id.trim(),
          base: c.base.trim(),
          proxy:
            c.proxyMode === "custom"
              ? c.proxyUrl.trim()
                ? `http://127.0.0.1:${c.proxyUrl.trim()}`
                : "auto"
              : c.proxyMode,
          keys: c.keys,
          activeKey: c.activeKey || undefined,
          availableModels: c.availableModels.length ? c.availableModels : undefined,
          fetchedAt: c.fetchedAt || undefined,
        };
      }
      const modelsObj: Record<string, ModelCfg> = {};
      for (const m of models) {
        modelsObj[m.id.trim()] = {
          name: m.name.trim() || m.id.trim(),
          channel: m.channel.trim(),
          key: m.key.trim() || undefined,
          price: {
            input: Number(m.input) || 0,
            output: Number(m.output) || 0,
            cache_read: Number(m.cacheRead) || 0,
          },
        };
      }
      const res = await putConfig({ channels: channelsObj, models: modelsObj });
      setSaveMsg(`已保存并热加载（渠道 ${res.channels} · 模型 ${res.models}）`);
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

  const miniFailed = mode === "mini" && lastRec ? lastRec.ok === false : false;

  // 状态栏文案：区分「代理运行 / 需要代理但未启动 / 仅统计模式（无需代理）」
  const configEmpty =
    status === null || (status.config_channels === 0 && status.config_models === 0);
  const statusText = (() => {
    if (online === null) return "连接中…";
    if (!online) return "本地引擎未就绪";
    if (!status) return "";
    if (status.running) {
      return status.mode === "full"
        ? `代理转发中 · 端口 ${status.port}`
        : "代理运行中（未配置转发 key）";
    }
    if (status.needed) return "代理未启动（检测到走代理模型）";
    return configEmpty ? "仅统计模式（内置模型）" : "仅统计模式";
  })();

  return (
    <div className={`widget mode-${mode} theme-${theme}${miniFailed ? " mini-failed" : ""}`}>
      <header className="titlebar" data-tauri-drag-region>
        <span className="title" data-tauri-drag-region>
          {mode === "mini" ? "Token" : "Token Widget"}
        </span>
        {mode === "mini" && lastRec && (
          <span className="title-time mono" data-tauri-drag-region>
            {fmtTime(lastRec.ts)}
          </span>
        )}
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
            pollMs={pollMs}
            setPollMs={setPollMs}
            channels={channels}
            setChannels={setChannels}
            models={models}
            setModels={setModels}
            newKey={newKey}
            setNewKey={setNewKey}
            saveMsg={saveMsg}
            setSaveMsg={setSaveMsg}
            saving={saving}
            onSave={save}
            onClose={() => setShowSettings(false)}
            onKeyOp={handleKeyOp}
            onFetchModels={handleFetchModels}
            status={status}
            routeModels={routeModels}
            ledger={ledger}
            scanProgress={scanProgress}
            opMsg={opMsg}
            onModeStart={handleModeStart}
            onModeStop={handleModeStop}
            onImport={handleImport}
            onRouteSwitch={handleRouteSwitch}
            onScan={handleScan}
            onRefresh={loadSettings}
          />
        ) : (
          <ExpandedView stats={stats} />
        )}
      </div>

      {mode === "expanded" && (
        <footer className="statusbar">
          <span className={`dot ${online === false ? "err" : "ok"}`} />
          <span>{statusText}</span>
          <span className="status-right">
            {status
              ? `渠道 ${status.config_channels} · 模型 ${status.config_models}`
              : ""}
            {pollMs / 1000}s
          </span>
        </footer>
      )}
    </div>
  );
}

export default App;
