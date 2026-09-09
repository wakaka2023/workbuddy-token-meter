import { useEffect, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { ChannelRow, ModelRow, ProxyStatus, RouteModel } from "../types";

interface Props {
  channels: ChannelRow[];
  models: ModelRow[];
  setModels: Dispatch<SetStateAction<ModelRow[]>>;
  routeModels: RouteModel[];
  onImport: () => void;
  onRouteSwitch: (name: string, route: "proxy" | "direct") => void;
  status: ProxyStatus | null;
  onModeStart: () => void;
  onModeStop: () => void;
}

const emptyModel = (): ModelRow => ({
  id: "",
  name: "",
  channel: "",
  key: "",
  input: "0",
  output: "0",
  cacheRead: "0",
});

export default function ModelTab({
  channels, models, setModels, routeModels, onImport, onRouteSwitch,
  status, onModeStart, onModeStop,
}: Props) {
  const patchModel = (i: number, p: Partial<ModelRow>) =>
    setModels(models.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const channelOf = (id: string) => channels.find((c) => c.id === id);

  const [idDd, setIdDd] = useState<{
    i: number;
    top: number;
    left: number;
    width: number;
    q: string;
  } | null>(null);

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

  useEffect(() => {
    if (!idDd) return;
    const close = () => setIdDd(null);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && close();
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

  const ddOverlay = idDd && models[idDd.i]
    ? (() => {
        const mm = models[idDd.i];
        const ch2 = channelOf(mm.channel);
        const cand = ch2?.availableModels ?? [];
        const q = (idDd.q ?? "").toLowerCase().trim();
        const shown = q ? cand.filter((x) => x.toLowerCase().includes(q)) : cand;
        return (
          <>
            <div className="dd-mask" onClick={() => setIdDd(null)} />
            <div className="dd-panel" style={{ top: idDd.top, left: idDd.left, width: idDd.width }}>
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
      })()
    : null;

  return (
    <>
      <div className="set-card">
        <div className="set-title">模型配置</div>
        <p className="set-desc">
          <b>id</b> 必须与所属渠道上游一致（可从渠道拉取列表选），<b>name</b> 是 WorkBuddy 展示名。
        </p>
        <div className="settings-table">
          {models.length === 0 && (
            <div className="set-desc">尚未添加模型。点"＋ 添加模型"后选渠道，id 可直接从渠道拉取的模型列表选。</div>
          )}
          {models.map((m, i) => {
            const ch = channelOf(m.channel);
            const keyOptions = ch?.keys ?? [];
            return (
              <div className="model-block model-row-slim" key={i}>
                <div className="s-row">
                  <span className="model-index">{i + 1}</span>
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
                      patchModel(i, { channel: cid, key: chh?.activeKey ?? "" });
                    }}
                  >
                    <option value="">选择渠道…</option>
                    {channels.map((c) => (
                      <option key={c.id} value={c.id}>{c.label || c.id}</option>
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
                      <option key={k.id} value={k.id}>{k.name}（{k.key}）</option>
                    ))}
                  </select>
                </div>
                <div className="s-row price-row">
                  <span className="price-hint">价格（元/百万 token）</span>
                  <input
                    className="s-input s-num"
                    value={m.input}
                    placeholder="入价"
                    title="输入价格"
                    onChange={(e) => patchModel(i, { input: e.target.value })}
                  />
                  <input
                    className="s-input s-num"
                    value={m.output}
                    placeholder="出价"
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
        <button className="add-btn" onClick={() => setModels([...models, emptyModel()])}>
          ＋ 添加模型
        </button>
      </div>

      {ddOverlay}

      <div className="set-card">
        <div className="set-title">WorkBuddy 模型路由</div>
        <p className="set-desc">自定义模型直连或走代理转发，修改 models.json（自动备份），重启 WorkBuddy 生效。</p>
        <div className="s-row">
          <button className="add-btn" onClick={onImport}>从 WorkBuddy 一键导入</button>
          <span className="hint">导入直连模型的 key 并登记原始地址，可随时切换</span>
        </div>
        <div className="settings-table">
          {routeModels.length === 0 && (
            <div className="set-desc">未检测到自定义模型，或尚未导入。</div>
          )}
          {routeModels.map((m, i) => (
            <div className="s-row route-row" key={i}>
              <span className="route-name" title={m.url}>{m.name}</span>
              <span className={`route-tag ${m.route}`}>
                {m.route === "proxy" ? "走代理" : "直连"}
              </span>
              <button className="add-btn" disabled={m.route === "proxy"} onClick={() => onRouteSwitch(m.name, "proxy")}>
                走代理
              </button>
              <button className="add-btn" disabled={m.route === "direct"} onClick={() => onRouteSwitch(m.name, "direct")}>
                直连
              </button>
            </div>
          ))}
        </div>
      </div>

      <div className="set-card">
        <div className="set-title">代理运行状态</div>
        <div className="s-row">
          {status ? (
            status.running ? (
              <>
                <span className={`route-tag ${status.mode}`}>
                  {status.mode === "full" ? "转发中" : "已运行"}
                </span>
                <span className="hint">
                  端口 {status.port} · 转发 {status.forwarded} · 已运行 {Math.floor(status.uptime / 60)}m
                </span>
                <button className="add-btn" onClick={onModeStop}>停止代理</button>
              </>
            ) : (
              <>
                <span className="route-tag service">未运行</span>
                <span className="hint">
                  {status.needed ? "检测到走代理模型，建议启动" : "仅统计模式无需启动代理"}
                </span>
                <button className="add-btn" onClick={onModeStart}>启动代理</button>
              </>
            )
          ) : (
            <span className="hint">无法获取代理状态</span>
          )}
        </div>
      </div>
    </>
  );
}