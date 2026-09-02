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
import AppearanceTab from "./AppearanceTab";
import ChannelTab from "./ChannelTab";
import ConfigTab from "./ConfigTab";
import ModelTab from "./ModelTab";
import ScanTab from "./ScanTab";

interface Props {
  theme: Theme;
  setTheme: Dispatch<SetStateAction<Theme>>;
  acrylic: number;
  setAcrylic: Dispatch<SetStateAction<number>>;
  cacheScope: CacheScope;
  setCacheScope: Dispatch<SetStateAction<CacheScope>>;
  settingsTab: SettingsTab;
  setSettingsTab: Dispatch<SetStateAction<SettingsTab>>;
  pollMs: number;
  setPollMs: Dispatch<SetStateAction<number>>;
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

function SettingsPanel({
  theme, setTheme, acrylic, setAcrylic,
  cacheScope, setCacheScope, settingsTab, setSettingsTab,
  pollMs, setPollMs,
  channels, setChannels, models, setModels,
  newKey, setNewKey, saveMsg, setSaveMsg, saving,
  onSave, onClose, onKeyOp, onFetchModels,
  status, routeModels, ledger, scanProgress, opMsg,
  onModeStart, onModeStop, onImport, onRouteSwitch, onScan, onRefresh,
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
        <AppearanceTab
          theme={theme} setTheme={setTheme}
          acrylic={acrylic} setAcrylic={setAcrylic}
          cacheScope={cacheScope} setCacheScope={setCacheScope}
          pollMs={pollMs} setPollMs={setPollMs}
        />
      )}

      {settingsTab === "channels" && (
        <ChannelTab
          channels={channels} setChannels={setChannels}
          newKey={newKey} setNewKey={setNewKey}
          setSaveMsg={setSaveMsg}
          onKeyOp={onKeyOp} onFetchModels={onFetchModels}
        />
      )}

      {settingsTab === "models" && (
        <ModelTab
          channels={channels} models={models} setModels={setModels}
          routeModels={routeModels} onImport={onImport} onRouteSwitch={onRouteSwitch}
        />
      )}

      {settingsTab === "scan" && (
        <ScanTab scanProgress={scanProgress} onScan={onScan} />
      )}

      {settingsTab === "config" && (
        <ConfigTab
          status={status} ledger={ledger}
          onModeStart={onModeStart} onModeStop={onModeStop}
        />
      )}

      <div className="s-footer">
        <span className={`save-msg ${saveMsg.startsWith("已保存") ? "ok" : ""}`}>
          {opMsg || saveMsg}
        </span>
        <button className="cancel-btn" onClick={onRefresh} title="重新读取本地配置">
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