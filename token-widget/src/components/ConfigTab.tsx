import type { LedgerData, ProxyStatus } from "../types";

interface Props {
  status: ProxyStatus | null;
  ledger: LedgerData | null;
  onModeStart: () => void;
  onModeStop: () => void;
}

export default function ConfigTab({ status, ledger, onModeStart, onModeStop }: Props) {
  return (
    <>
      <div className="settings-title">代理运行状态</div>
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
          ledger.changelog.slice().reverse().map((c, i) => (
            <div className="s-row ledger-row" key={i}>
              <span className="route-name">{c.op}</span>
              <span className="hint">{c.detail} · {c.ts}</span>
            </div>
          ))
        ) : (
          <div className="settings-desc">暂无操作记录。</div>
        )}
      </div>
    </>
  );
}