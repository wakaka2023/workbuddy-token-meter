import { useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import type { ChannelRow } from "../types";

interface Props {
  channels: ChannelRow[];
  setChannels: Dispatch<SetStateAction<ChannelRow[]>>;
  newKey: Record<string, { name: string; key: string }>;
  setNewKey: Dispatch<SetStateAction<Record<string, { name: string; key: string }>>>;
  setSaveMsg: Dispatch<SetStateAction<string>>;
  onKeyOp: (body: Record<string, string>) => void;
  onFetchModels: (channel: string) => void;
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

export default function ChannelTab({
  channels, setChannels, newKey, setNewKey, setSaveMsg, onKeyOp, onFetchModels,
}: Props) {
  const patchChannel = (i: number, p: Partial<ChannelRow>) =>
    setChannels(channels.map((x, j) => (j === i ? { ...x, ...p } : x)));

  // key 添加表单是否展开（默认收起；key 池为空时自动展开，可取消收起）
  const [addKeyOpen, setAddKeyOpen] = useState<Record<string, boolean>>({});
  const keyFormOpen = (c: ChannelRow) =>
    addKeyOpen[c.id] === true || (addKeyOpen[c.id] === undefined && c.keys.length === 0);

  return (
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
            <div className="ch-labels">
              <span>渠道名</span>
              <span>显示名</span>
            </div>
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
                <span className="hint">{c.fetchedAt} 已拉取 {c.availableModels.length} 个</span>
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
  );
}