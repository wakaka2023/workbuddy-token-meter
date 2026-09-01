import type { Dispatch, SetStateAction } from "react";
import type { CacheScope, ModelRow, ProviderRow, SettingsTab, Theme } from "../types";

interface Props {
  theme: Theme;
  setTheme: Dispatch<SetStateAction<Theme>>;
  acrylic: number;
  setAcrylic: Dispatch<SetStateAction<number>>;
  cacheScope: CacheScope;
  setCacheScope: Dispatch<SetStateAction<CacheScope>>;
  settingsTab: SettingsTab;
  setSettingsTab: Dispatch<SetStateAction<SettingsTab>>;
  models: ModelRow[];
  setModels: Dispatch<SetStateAction<ModelRow[]>>;
  providers: ProviderRow[];
  setProviders: Dispatch<SetStateAction<ProviderRow[]>>;
  providerNames: string[];
  newKey: Record<string, { name: string; key: string }>;
  setNewKey: Dispatch<SetStateAction<Record<string, { name: string; key: string }>>>;
  saveMsg: string;
  setSaveMsg: Dispatch<SetStateAction<string>>;
  saving: boolean;
  onSave: () => void;
  onClose: () => void;
  onKeyOp: (body: Record<string, string>) => void;
}

function SettingsPanel({
  theme,
  setTheme,
  acrylic,
  setAcrylic,
  cacheScope,
  setCacheScope,
  settingsTab,
  setSettingsTab,
  models,
  setModels,
  providers,
  setProviders,
  providerNames,
  newKey,
  setNewKey,
  saveMsg,
  setSaveMsg,
  saving,
  onSave,
  onClose,
  onKeyOp,
}: Props) {
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
          className={`settings-tab${settingsTab === "models" ? " active" : ""}`}
          onClick={() => setSettingsTab("models")}
        >
          模型价格
        </button>
        <button
          className={`settings-tab${settingsTab === "providers" ? " active" : ""}`}
          onClick={() => setSettingsTab("providers")}
        >
          渠道
        </button>
        <button
          className={`settings-tab${settingsTab === "keys" ? " active" : ""}`}
          onClick={() => setSettingsTab("keys")}
        >
          API Key
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

      {settingsTab === "models" && (
        <>
          <div className="settings-title">模型价格（元/百万 token）</div>
          <div className="settings-table">
            {models.map((m, i) => (
              <div className="s-row" key={i}>
                <input
                  className="s-input s-id"
                  value={m.id}
                  placeholder="模型 id"
                  onChange={(e) =>
                    setModels(models.map((x, j) => (j === i ? { ...x, id: e.target.value } : x)))
                  }
                />
                <select
                  className="s-input s-prov"
                  value={m.provider}
                  onChange={(e) =>
                    setModels(models.map((x, j) => (j === i ? { ...x, provider: e.target.value } : x)))
                  }
                >
                  {providerNames.map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
                <input
                  className="s-input s-num"
                  value={m.input}
                  placeholder="入"
                  title="输入价格"
                  onChange={(e) =>
                    setModels(models.map((x, j) => (j === i ? { ...x, input: e.target.value } : x)))
                  }
                />
                <input
                  className="s-input s-num"
                  value={m.output}
                  placeholder="出"
                  title="输出价格"
                  onChange={(e) =>
                    setModels(models.map((x, j) => (j === i ? { ...x, output: e.target.value } : x)))
                  }
                />
                <input
                  className="s-input s-num"
                  value={m.cacheRead}
                  placeholder="缓存"
                  title="缓存命中价格"
                  onChange={(e) =>
                    setModels(
                      models.map((x, j) => (j === i ? { ...x, cacheRead: e.target.value } : x)),
                    )
                  }
                />
                <button
                  className="del-btn"
                  title="删除模型"
                  onClick={() => setModels(models.filter((_, j) => j !== i))}
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
          <button
            className="add-btn"
            onClick={() =>
              setModels([
                ...models,
                { id: "", provider: providerNames[0] ?? "", input: "0", output: "0", cacheRead: "0" },
              ])
            }
          >
            + 添加模型
          </button>
        </>
      )}

      {settingsTab === "providers" && (
        <>
          <div className="settings-title">渠道（provider → 上游 base）</div>
          <div className="settings-table">
            {providers.map((p, i) => (
              <div className="prov-block" key={i}>
                <div className="s-row">
                  <input
                    className="s-input s-id"
                    value={p.name}
                    placeholder="渠道名"
                    onChange={(e) =>
                      setProviders(providers.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))
                    }
                  />
                  <input
                    className="s-input s-base"
                    value={p.base}
                    placeholder="上游 base URL"
                    onChange={(e) =>
                      setProviders(providers.map((x, j) => (j === i ? { ...x, base: e.target.value } : x)))
                    }
                  />
                  <input
                    className="s-input s-lab"
                    value={p.label}
                    placeholder="显示名"
                    onChange={(e) =>
                      setProviders(providers.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)))
                    }
                  />
                  <button
                    className="del-btn"
                    title="删除渠道"
                    onClick={() => setProviders(providers.filter((_, j) => j !== i))}
                  >
                    ✕
                  </button>
                </div>
                <div className="s-row prov-proxy-row">
                  <select
                    className="s-input s-proxy-sel"
                    value={p.proxyMode}
                    title="网络通道：自动检测(本机7897可用则走VPN，否则直连)/强制直连/强制走指定代理地址"
                    onChange={(e) =>
                      setProviders(
                        providers.map((x, j) =>
                          j === i ? { ...x, proxyMode: e.target.value as typeof p.proxyMode } : x,
                        ),
                      )
                    }
                  >
                    <option value="auto">自动检测</option>
                    <option value="direct">直连</option>
                    <option value="custom">强制代理</option>
                  </select>
                  {p.proxyMode === "custom" && (
                    <input
                      className="s-input s-proxy-url"
                      value={p.proxyUrl}
                      placeholder="7897"
                      title="强制代理端口（本机 127.0.0.1）"
                      inputMode="numeric"
                      onChange={(e) =>
                        setProviders(
                          providers.map((x, j) =>
                            j === i ? { ...x, proxyUrl: e.target.value.replace(/\D/g, "") } : x,
                          ),
                        )
                      }
                    />
                  )}
                </div>
              </div>
            ))}
          </div>
          <button
            className="add-btn"
            onClick={() =>
              setProviders([
                ...providers,
                {
                  name: "",
                  base: "",
                  label: "",
                  proxy: "auto",
                  proxyMode: "auto",
                  proxyUrl: "",
                  keys: [],
                  activeKey: "",
                },
              ])
            }
          >
            + 添加渠道
          </button>
        </>
      )}

      {settingsTab === "keys" && (
        <>
          <div className="settings-title">API Key 管理</div>
          <div className="settings-table">
            {providers.map((p, i) => (
              <div className="key-block" key={i}>
                <div className="key-head">
                  <span className="key-prov">{p.label || p.name || "(未命名渠道)"}</span>
                  {p.keys.length > 0 && (
                    <select
                      className="s-input s-key-sel"
                      value={p.activeKey}
                      title="激活 key"
                      onChange={(e) =>
                        onKeyOp({ provider: p.name, action: "set", keyId: e.target.value })
                      }
                    >
                      {p.keys.map((k) => (
                        <option key={k.id} value={k.id}>
                          {k.name}（{k.key}）
                        </option>
                      ))}
                    </select>
                  )}
                </div>
                {p.keys.length > 0 && (
                  <div className="key-chips">
                    {p.keys.map((k) => (
                      <span
                        key={k.id}
                        className={`key-chip${k.id === p.activeKey ? " active" : ""}`}
                        title={k.id === p.activeKey ? "当前激活" : "点击切换"}
                        onClick={() =>
                          k.id !== p.activeKey &&
                          onKeyOp({ provider: p.name, action: "set", keyId: k.id })
                        }
                      >
                        {k.name} · {k.key}
                        <span
                          className="key-del"
                          title="删除该 key"
                          onClick={(e) => {
                            e.stopPropagation();
                            onKeyOp({ provider: p.name, action: "del", keyId: k.id });
                          }}
                        >
                          ✕
                        </span>
                      </span>
                    ))}
                  </div>
                )}
                <div className="key-add">
                  <input
                    className="s-input s-key-name"
                    placeholder="key 名称"
                    value={newKey[p.name]?.name ?? ""}
                    onChange={(e) =>
                      setNewKey({
                        ...newKey,
                        [p.name]: { ...(newKey[p.name] ?? { key: "" }), name: e.target.value },
                      })
                    }
                  />
                  <input
                    className="s-input s-key-val"
                    placeholder="sk-..."
                    value={newKey[p.name]?.key ?? ""}
                    onChange={(e) =>
                      setNewKey({
                        ...newKey,
                        [p.name]: { ...(newKey[p.name] ?? { name: "" }), key: e.target.value },
                      })
                    }
                  />
                  <button
                    className="add-btn s-key-add-btn"
                    onClick={() => {
                      const v = newKey[p.name];
                      if (!v?.key.trim()) {
                        setSaveMsg("key 不能为空");
                        return;
                      }
                      onKeyOp({ provider: p.name, action: "add", name: v.name.trim(), key: v.key.trim() });
                      setNewKey({ ...newKey, [p.name]: { name: "", key: "" } });
                    }}
                  >
                    添加
                  </button>
                </div>
              </div>
            ))}
          </div>
        </>
      )}

      <div className="s-footer">
        <span className={`save-msg ${saveMsg.startsWith("已保存") ? "ok" : ""}`}>{saveMsg}</span>
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
