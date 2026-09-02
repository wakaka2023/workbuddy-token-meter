import { useEffect, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import type {
  CacheScope,
  ChannelRow,
  LedgerData,
  ModelRow,
  ProxyStatus,
  RouteModel,
  ScanProgress,
  SettingsTab,
  Theme,
} from "../types";

interface Props {
  theme: Theme;
  setTheme: Dispatch<SetStateAction<Theme>>;
  acrylic: number;
  setAcrylic: Dispatch<SetStateAction<number>>;
  cacheScope: CacheScope;
  setCacheScope: Dispatch<SetStateAction<CacheScope>>;
  settingsTab: SettingsTab;
  setSettingsTab: Dispatch<SetStateAction<SettingsTab>>;
  channels: ChannelRow[];
  setChannels: Dispatch<SetStateAction<ChannelRow[]>>;
  models: ModelRow[];
  setModels: Dispatch<SetStateAction<ModelRow[]>>;
  newKey: Record<string, { name: string; key: string }>;
  setNewKey: Dispatch<SetStateAction<Record<string, { name: string; key: string }>>>;
  saveMsg: string;
  setSaveMsg: Dispatch<SetStateAction<string>>;
  saving: boolean;
  onSave: () => void;
  onClose: () => void;
  onKeyOp: (body: Record<string, string>) => void;
  onFetchModels: (channel: string) => void;
  // v0.2
  status: ProxyStatus | null;
  routeModels: RouteModel[];
  ledger: LedgerData | null;
  scanProgress: ScanProgress | null;
  opMsg: string;
  onModeStart: () => void;
  onModeStop: () => void;
  onImport: () => void;
  onRouteSwitch: (name: string, route: "proxy" | "direct") => void;
  onScan: (force: boolean) => void;
  onRefresh: () => void;
}

const emptyChannel = (): ChannelRow => ({
  id: "",
  label: "",
  base: "",
  proxyMode: "auto",
  proxyUrl: "",
  keys: [],
  activeKey: "",
  availableModels: [],
  fetchedAt: "",
});

const emptyModel = (): ModelRow => ({
  id: "",
  name: "",
  channel: "",
  key: "",
  input: "0",
  output: "0",
  cacheRead: "0",
});

function SettingsPanel({
  theme,
  setTheme,
  acrylic,
  setAcrylic,
  cacheScope,
  setCacheScope,
  settingsTab,
  setSettingsTab,
  channels,
  setChannels,
  models,
  setModels,
  newKey,
  setNewKey,
  saveMsg,
  setSaveMsg,
  saving,
  onSave,
  onClose,
  onKeyOp,
  onFetchModels,
  status,
  routeModels,
  ledger,
  scanProgress,
  opMsg,
  onModeStart,
  onModeStop,
  onImport,
  onRouteSwitch,
  onScan,
  onRefresh,
}: Props) {
  const patchChannel = (i: number, p: Partial<ChannelRow>) =>
    setChannels(channels.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const patchModel = (i: number, p: Partial<ModelRow>) =>
    setModels(models.map((x, j) => (j === i ? { ...x, ...p } : x)));

  // 模型选渠道后：若该渠道有可用模型列表，默认把 key 置为渠道激活 key
  const channelOf = (id: string) => channels.find((c) => c.id === id);
  // 渠道卡片 key 添加表单是否展开（默认收起；key 池为空时自动展开）
  const [addKeyOpen, setAddKeyOpen] = useState<Record<string, boolean>>({});
  // key 添加表单可见性：显式开 / 未显式设且 key 池为空（首加自动展开）
  const keyFormOpen = (c: ChannelRow) =>
    addKeyOpen[c.id] === true || (addKeyOpen[c.id] === undefined && c.keys.length === 0);

  // 模型 id 候选浮层（替代 datalist：WebView2 中 datalist 弹出不可靠且与输入框原生箭头重复）
  const [idDd, setIdDd] = useState<{
    i: number;
    top: number;
    left: number;
    width: number;
    q: string;
  } | null>(null);
  // 计算浮层锚点（fixed 定位，向下弹出；窗口底部空间不足则向上）
  const openIdDd = (i: number, btn: HTMLButtonElement) => {
    if (idDd?.i === i) {
      setIdDd(null);
      return;
    }
    const inp = btn.parentElement?.querySelector<HTMLInputElement>("input");
    if (!inp) return;
    const r = inp.getBoundingClientRect();
    const ddH = 240;
    const roomBottom = window.innerHeight - r.bottom - 8;
    const up = roomBottom < ddH && r.top > ddH;
    setIdDd({
      i,
      top: up ? Math.max(6, r.top - ddH + 6) : r.bottom + 4,
      left: Math.max(6, Math.min(r.left, window.innerWidth - 260)),
      width: Math.max(200, r.width),
      q: "",
    });
  };
  // 浮层打开期间：Esc / 点击遮罩 / 外部滚动 / 窗口缩放 → 关闭
  useEffect(() => {
    if (!idDd) return;
    const close = () => setIdDd(null);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && close();
    // 浮层内部列表滚动不关闭；外部滚动（如 expanded-body）关闭避免错位
    const onScroll = (e: Event) => {
      const t = e.target as HTMLElement | null;
      if (t?.closest?.(".dd-panel")) return;
      close();
    };
    window.addEventListener("resize", close);
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("resize", close);
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("keydown", esc);
    };
  }, [idDd]);

  return (
    <section className="settings">
      <div className="settings-tabs">
        <button
          className={`settings-tab${settingsTab === "appearance" ? " active" : ""}`}
          onClick={() => setSettingsTab("appearance")}
        >
          外观
        </button>
        <button
          className={`settings-tab${settingsTab === "channels" ? " active" : ""}`}
          onClick={() => setSettingsTab("channels")}
        >
          渠道
        </button>
        <button
          className={`settings-tab${settingsTab === "models" ? " active" : ""}`}
          onClick={() => setSettingsTab("models")}
        >
          模型
        </button>
        <button
          className={`settings-tab${settingsTab === "scan" ? " active" : ""}`}
          onClick={() => setSettingsTab("scan")}
        >
          扫描
        </button>
        <button
          className={`settings-tab${settingsTab === "config" ? " active" : ""}`}
          onClick={() => setSettingsTab("config")}
        >
          配置管理
        </button>
      </div>

      {settingsTab === "appearance" && (
        <>
          <div className="settings-title">主题</div>
          <div className="theme-toggle">
            <button
              className={`theme-btn dark${theme === "dark" ? " active" : ""}`}
              onClick={() => setTheme("dark")}
            >
              <span className="swatch" />
              深色
            </button>
            <button
              className={`theme-btn light${theme === "light" ? " active" : ""}`}
              onClick={() => setTheme("light")}
            >
              <span className="swatch" />
              浅色
            </button>
          </div>
          <div className="acrylic-row">
            <label className="acrylic-label" htmlFor="acrylic-range">
              亚克力强度
            </label>
            <input
              id="acrylic-range"
              type="range"
              min={0}
              max={100}
              value={acrylic}
              onChange={(e) => setAcrylic(Number(e.target.value))}
            />
            <span className="acrylic-val">{acrylic}%</span>
          </div>
          <div className="settings-title">Mini 缓存率口径</div>
          <div className="theme-toggle">
            <button
              className={`theme-btn dark${cacheScope === "today" ? " active" : ""}`}
              onClick={() => setCacheScope("today")}
              title="全部模型今日的缓存命中率"
            >
              <span className="swatch" />
              全模型今日
            </button>
            <button
              className={`theme-btn light${cacheScope === "model" ? " active" : ""}`}
              onClick={() => setCacheScope("model")}
              title="当前请求模型今日的缓存命中率"
            >
              <span className="swatch" />
              当前模型
            </button>
          </div>
        </>
      )}

      {settingsTab === "channels" && (
        <>
          <div className="settings-title">渠道配置（base + 代理 + API Key 池）</div>
          <p className="settings-desc">
            一个渠道 = 一个上游供应商。此处仅维护渠道的 base、网络代理与 API Key 池；
            模型如何路由、用哪把 key、价格，均在「模型」页配置。渠道拉取的模型列表供模型页添加时直接选择。
          </p>
          <div className="settings-table">
            {channels.length === 0 && (
              <div className="settings-desc">尚未添加渠道。点击底部"＋ 添加渠道"。</div>
            )}
            {channels.map((c, i) => (
              <div className="model-block" key={i}>
                {/* ① 头部：渠道身份 + 右上角删除 */}
                <div className="ch-head">
                  <div className="ch-title">
                    <input
                      className="s-input s-id"
                      value={c.id}
                      placeholder="渠道名（如 b.ai）"
                      title="渠道标识"
                      onChange={(e) => patchChannel(i, { id: e.target.value })}
                    />
                    <input
                      className="s-input s-lab"
                      value={c.label}
                      placeholder="显示名"
                      title="统计与日志中的显示名"
                      onChange={(e) => patchChannel(i, { label: e.target.value })}
                    />
                  </div>
                  <div className="ch-actions">
                    <button
                      className="del-btn"
                      title="删除渠道（引用它的模型需改选渠道）"
                      onClick={() => setChannels(channels.filter((_, j) => j !== i))}
                    >
                      ✕
                    </button>
                  </div>
                </div>
                {/* ② 连接设置：base + 网络通道 */}
                <div className="s-row">
                  <input
                    className="s-input s-base"
                    value={c.base}
                    placeholder="上游 base URL，如 https://api.b.ai/v1"
                    onChange={(e) => patchChannel(i, { base: e.target.value })}
                  />
                  <select
                    className="s-input s-proxy-sel"
                    value={c.proxyMode}
                    title="网络通道：自动检测(本机7897可用则走VPN，否则直连)/强制直连/强制走指定代理地址"
                    onChange={(e) =>
                      patchChannel(i, { proxyMode: e.target.value as ChannelRow["proxyMode"] })
                    }
                  >
                    <option value="auto">自动检测</option>
                    <option value="direct">直连</option>
                    <option value="custom">强制代理</option>
                  </select>
                  {c.proxyMode === "custom" && (
                    <input
                      className="s-input s-proxy-url"
                      value={c.proxyUrl}
                      placeholder="7897"
                      title="强制代理端口（本机 127.0.0.1）"
                      inputMode="numeric"
                      onChange={(e) => patchChannel(i, { proxyUrl: e.target.value.replace(/\D/g, "") })}
                    />
                  )}
                </div>
                {/* ③ Key 池：点击 chip 切换激活；添加表单默认收起，key 池为空时自动展开，可取消收起 */}
                <div className="key-block">
                  <div className="key-head">
                    <span className="key-prov">API Key 池{c.keys.length > 0 ? `（${c.keys.length}）` : ""}</span>
                    <span className="hint">点击 chip 切换激活 · 即时生效</span>
                  </div>
                  {c.keys.length > 0 && (
                    <div className="key-chips">
                      {c.keys.map((k) => (
                        <span
                          key={k.id}
                          className={`key-chip${k.id === c.activeKey ? " active" : ""}`}
                          title={k.id === c.activeKey ? "当前激活" : "点击切换"}
                          onClick={() =>
                            k.id !== c.activeKey &&
                            onKeyOp({ channel: c.id, action: "set", keyId: k.id })
                          }
                        >
                          {k.name || "未命名"} · {k.key}
                          <span
                            className="key-del"
                            title="删除该 key"
                            onClick={(e) => {
                              e.stopPropagation();
                              onKeyOp({ channel: c.id, action: "del", keyId: k.id });
                            }}
                          >
                            ✕
                          </span>
                        </span>
                      ))}
                    </div>
                  )}
                  {c.keys.length === 0 && (
                    <div className="key-empty-hint">还没有 key —— 添加后即可被模型页选用并拉取模型。</div>
                  )}
                  {keyFormOpen(c) ? (
                    <div className="key-add">
                      <input
                        className="s-input s-key-name"
                        placeholder="key 名称"
                        value={newKey[c.id]?.name ?? ""}
                        onChange={(e) =>
                          setNewKey({
                            ...newKey,
                            [c.id]: { ...(newKey[c.id] ?? { key: "" }), name: e.target.value },
                          })
                        }
                      />
                      <input
                        className="s-input s-key-val"
                        placeholder="sk-..."
                        value={newKey[c.id]?.key ?? ""}
                        onChange={(e) =>
                          setNewKey({
                            ...newKey,
                            [c.id]: { ...(newKey[c.id] ?? { name: "" }), key: e.target.value },
                          })
                        }
                      />
                      <button
                        className="add-btn s-key-add-btn"
                        onClick={() => {
                          const v = newKey[c.id];
                          if (!c.id.trim()) {
                            setSaveMsg("请先填写渠道名再添加 key");
                            return;
                          }
                          if (!v?.key.trim()) {
                            setSaveMsg("key 不能为空");
                            return;
                          }
                          onKeyOp({ channel: c.id, action: "add", name: v.name.trim(), key: v.key.trim() });
                          setNewKey({ ...newKey, [c.id]: { name: "", key: "" } });
                          setAddKeyOpen({ ...addKeyOpen, [c.id]: false });
                        }}
                      >
                        添加
                      </button>
                      <button
                        className="cancel-btn key-cancel-btn"
                        onClick={() => setAddKeyOpen({ ...addKeyOpen, [c.id]: false })}
                      >
                        取消
                      </button>
                    </div>
                  ) : (
                    <button
                      className="add-btn key-add-toggle"
                      onClick={() => setAddKeyOpen({ ...addKeyOpen, [c.id]: true })}
                    >
                      ＋ 添加 key
                    </button>
                  )}
                </div>
                {/* ④ 模型列表：拉取 / 重新拉取（同一按钮，可反复获取最新） */}
                <div className="s-row fetch-row">
                  <button
                    className="add-btn fetch-btn"
                    title={
                      c.fetchedAt
                        ? "重新请求该渠道 {base}/models 获取最新模型列表"
                        : "请求该渠道 {base}/models 拉取可用模型列表"
                    }
                    onClick={() => {
                      if (!c.id.trim()) {
                        setSaveMsg("请先填写渠道名再拉取模型");
                        return;
                      }
                      onFetchModels(c.id);
                    }}
                  >
                    {c.fetchedAt ? "⟳ 重新拉取模型" : "⟳ 拉取模型"}
                  </button>
                  {c.fetchedAt ? (
                    <span className="hint">
                      {c.fetchedAt} 已拉取 {c.availableModels.length} 个
                    </span>
                  ) : (
                    <span className="hint">上游支持 /models 时可一键拉取</span>
                  )}
                </div>
              </div>
            ))}
          </div>
          <button className="add-btn" onClick={() => setChannels([...channels, emptyChannel()])}>
            ＋ 添加渠道
          </button>
        </>
      )}

      {settingsTab === "models" && (
        <>
          <div className="settings-title">模型配置（id → 渠道 + key + 价格）</div>
          <p className="settings-desc">
            模型有两个关键字段：<b>id</b>（请求体 model 名，必须与所属渠道上游一致，可从渠道拉取列表选）与{" "}
            <b>name</b>（WorkBuddy 展示名）。渠道负责 key 池与 base，模型只是引用它们。
          </p>
          <div className="settings-table">
            {models.length === 0 && (
              <div className="settings-desc">
                尚未添加模型。点"＋ 添加模型"后选渠道，id 可直接从渠道拉取的模型列表选。
              </div>
            )}
            {models.map((m, i) => {
              const ch = channelOf(m.channel);
              const keyOptions = ch?.keys ?? [];
              return (
                <div className="model-block model-row-slim" key={i}>
                  <div className="s-row">
                    <div className="mdl-field">
                      <input
                        className="s-input s-id"
                        value={m.id}
                        placeholder="模型 id（如 deepseek-v4-flash）"
                        title="请求体 model 名：可从渠道拉取的模型列表下拉选择，也可手动填（须与上游一致）"
                        onChange={(e) => patchModel(i, { id: e.target.value })}
                      />
                      <button
                        className="mdl-arrow"
                        title="从该渠道已拉取的模型列表中选择"
                        onClick={(e) => openIdDd(i, e.currentTarget)}
                      >
                        ▾
                      </button>
                    </div>
                    <input
                      className="s-input s-lab"
                      value={m.name}
                      placeholder="name（WorkBuddy 显示名）"
                      title="WorkBuddy 中的显示名"
                      onChange={(e) => patchModel(i, { name: e.target.value })}
                    />
                    <button
                      className="del-btn"
                      title="删除模型"
                      onClick={() => setModels(models.filter((_, j) => j !== i))}
                    >
                      ✕
                    </button>
                  </div>
                  <div className="s-row">
                    <select
                      className="s-input s-proxy-sel"
                      value={m.channel}
                      title="所属渠道（渠道负责 base/代理/key 池）"
                      onChange={(e) => {
                        const cid = e.target.value;
                        const chh = channelOf(cid);
                        patchModel(i, {
                          channel: cid,
                          key: chh?.activeKey ?? "",
                        });
                      }}
                    >
                      <option value="">选择渠道…</option>
                      {channels.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.label || c.id}
                        </option>
                      ))}
                    </select>
                    <select
                      className="s-input s-key-sel"
                      value={m.key}
                      title="使用的 key（跟随渠道默认则选空白）"
                      disabled={!ch}
                      onChange={(e) => patchModel(i, { key: e.target.value })}
                    >
                      <option value="">渠道默认</option>
                      {keyOptions.map((k) => (
                        <option key={k.id} value={k.id}>
                          {k.name}（{k.key}）
                        </option>
                      ))}
                    </select>
                  </div>
                  <div className="s-row price-row">
                    <span className="price-hint">价格（元/百万 token）</span>
                    <input
                      className="s-input s-num"
                      value={m.input}
                      placeholder="入"
                      title="输入价格"
                      onChange={(e) => patchModel(i, { input: e.target.value })}
                    />
                    <input
                      className="s-input s-num"
                      value={m.output}
                      placeholder="出"
                      title="输出价格"
                      onChange={(e) => patchModel(i, { output: e.target.value })}
                    />
                    <input
                      className="s-input s-num"
                      value={m.cacheRead}
                      placeholder="缓存"
                      title="缓存命中价格"
                      onChange={(e) => patchModel(i, { cacheRead: e.target.value })}
                    />
                  </div>
                </div>
              );
            })}
          </div>
          {/* 模型 id 候选浮层（fixed 定位，避开滚动容器裁剪） */}
          {idDd &&
            (() => {
              const mm = models[idDd.i];
              if (!mm) return null;
              const ch2 = channelOf(mm.channel);
              const cand = ch2?.availableModels ?? [];
              const q = (idDd.q ?? "").toLowerCase().trim();
              const shown = q ? cand.filter((x) => x.toLowerCase().includes(q)) : cand;
              return (
                <>
                  <div className="dd-mask" onClick={() => setIdDd(null)} />
                  <div
                    className="dd-panel"
                    style={{ top: idDd.top, left: idDd.left, width: idDd.width }}
                  >
                    <div className="dd-head">
                      <span className="hint">
                        {ch2
                          ? `来自「${ch2.label || ch2.id}」的模型${cand.length > 0 ? `（${cand.length}）` : ""}`
                          : "先选择所属渠道"}
                      </span>
                      <span className="dd-tip">点击选择 · Esc 关闭</span>
                    </div>
                    {ch2 && cand.length > 0 && (
                      <input
                        className="s-input dd-filter"
                        placeholder="筛选模型…"
                        value={idDd.q}
                        autoFocus
                        onChange={(e) => setIdDd({ ...idDd, q: e.target.value })}
                      />
                    )}
                    <div className="dd-list">
                      {!ch2 ? (
                        <div className="dd-empty">请先在模型行下方选择所属渠道。</div>
                      ) : cand.length === 0 ? (
                        <div className="dd-empty">
                          该渠道尚未拉取模型。请到「渠道」页点击「拉取模型」，或直接在此输入 id。
                        </div>
                      ) : shown.length === 0 ? (
                        <div className="dd-empty">没有匹配「{idDd.q}」的模型。</div>
                      ) : (
                        shown.map((x) => (
                          <button
                            key={x}
                            className={`dd-item${x === mm.id ? " sel" : ""}`}
                            onClick={() => {
                              patchModel(idDd.i, { id: x });
                              setIdDd(null);
                            }}
                          >
                            {x}
                          </button>
                        ))
                      )}
                    </div>
                  </div>
                </>
              );
            })()}
          <button className="add-btn" onClick={() => setModels([...models, emptyModel()])}>
            ＋ 添加模型
          </button>

          <div className="settings-title" style={{ marginTop: 16 }}>
            WorkBuddy 模型路由（自定义模型直连 / 走代理）
          </div>
          <div className="s-row">
            <button className="add-btn" onClick={onImport}>
              从 WorkBuddy 一键导入
            </button>
            <span className="hint">导入直连模型的 key 并登记原始地址，可随时切换</span>
          </div>
          <div className="settings-table">
            {routeModels.length === 0 && (
              <div className="settings-desc">未检测到自定义模型，或尚未导入。</div>
            )}
            {routeModels.map((m, i) => (
              <div className="s-row route-row" key={i}>
                <span className="route-name" title={m.url}>
                  {m.name}
                </span>
                <span className={`route-tag ${m.route}`}>
                  {m.route === "proxy" ? "走代理" : "直连"}
                </span>
                <button
                  className="add-btn"
                  disabled={m.route === "proxy"}
                  onClick={() => onRouteSwitch(m.name, "proxy")}
                >
                  走代理
                </button>
                <button
                  className="add-btn"
                  disabled={m.route === "direct"}
                  onClick={() => onRouteSwitch(m.name, "direct")}
                >
                  直连
                </button>
              </div>
            ))}
          </div>
          <p className="settings-desc">
            切换会修改 WorkBuddy 的 models.json（自动备份），重启 WorkBuddy 后生效。
          </p>
        </>
      )}

      {settingsTab === "scan" && (
        <>
          <div className="settings-title">扫描统计（本地 trace 记账）</div>
          <p className="settings-desc">
            扫描 WorkBuddy 本地 trace 文件，加载内置模型与直连自定义模型的用量。
            首次约 8s，之后增量秒级；结果缓存在 %APPDATA% 的 trace-cache。
          </p>
          <div className="s-row scan-actions">
            <button className="add-btn" onClick={() => onScan(false)}>
              增量扫描
            </button>
            <button className="add-btn" onClick={() => onScan(true)}>
              重新全量扫描
            </button>
          </div>
          {scanProgress && (
            <div className="scan-progress">
              {scanProgress.running ? (
                <span>
                  扫描中… {scanProgress.scanned}/{scanProgress.total} 文件
                </span>
              ) : (
                <span>
                  最近扫描：{scanProgress.scanned}/{scanProgress.total} 文件
                  {scanProgress.records > 0 && `，新增 ${scanProgress.records} 条`}
                </span>
              )}
            </div>
          )}
        </>
      )}

      {settingsTab === "config" && (
        <>
          <div className="settings-title">代理运行状态</div>
          <div className="s-row">
            {status ? (
              <>
                <span className={`route-tag ${status.mode}`}>
                  {status.mode === "full" ? "转发中" : "待配置"}
                </span>
                <span className="hint">
                  端口 {status.port} · 转发 {status.forwarded} · 已运行 {Math.floor(status.uptime / 60)}m
                </span>
                {status.mode === "full" ? (
                  <button className="add-btn" onClick={onModeStop}>
                    停止代理
                  </button>
                ) : (
                  <button className="add-btn" onClick={onModeStart}>
                    启动代理
                  </button>
                )}
              </>
            ) : (
              <span className="hint">无法获取代理状态</span>
            )}
          </div>
          <div className="settings-title">模型登记表（当前状态）</div>
          <div className="settings-table">
            {ledger && ledger.models.length > 0 ? (
              ledger.models.map((m, i) => (
                <div className="s-row ledger-row" key={i}>
                  <span className="route-name">{m.name}</span>
                  <span className={`route-tag ${m.route}`}>
                    {m.route === "proxy" ? "走代理" : "直连"}
                  </span>
                  <span className="hint">{m.channel ?? "—"}</span>
                </div>
              ))
            ) : (
              <div className="settings-desc">暂无模型登记。点击"模型"页签执行一键导入。</div>
            )}
          </div>
          <div className="settings-title">操作日志</div>
          <div className="settings-table">
            {ledger && ledger.changelog.length > 0 ? (
              ledger.changelog
                .slice()
                .reverse()
                .map((c, i) => (
                  <div className="s-row ledger-row" key={i}>
                    <span className="route-name">{c.op}</span>
                    <span className="hint">
                      {c.detail} · {c.ts}
                    </span>
                  </div>
                ))
            ) : (
              <div className="settings-desc">暂无操作记录。</div>
            )}
          </div>
        </>
      )}

      <div className="s-footer">
        <span className={`save-msg ${saveMsg.startsWith("已保存") ? "ok" : ""}`}>
          {opMsg || saveMsg}
        </span>
        <button className="cancel-btn" onClick={onRefresh} title="重新从代理拉取配置">
          刷新配置
        </button>
        <button className="cancel-btn" onClick={onClose}>
          取消
        </button>
        <button className="save-btn" onClick={onSave} disabled={saving}>
          {saving ? "保存中…" : "保存并热加载"}
        </button>
      </div>
    </section>
  );
}

export default SettingsPanel;