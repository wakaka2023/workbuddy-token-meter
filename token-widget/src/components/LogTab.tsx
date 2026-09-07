import type { LedgerData } from "../types";

interface Props {
  ledger: LedgerData | null;
}

export default function LogTab({ ledger }: Props) {
  return (
    <>
      <div className="set-card">
        <div className="set-title">模型登记表</div>
        <p className="set-desc">WorkBuddy models.json 中自定义模型的路由状态</p>
        {ledger && ledger.models.length > 0 ? (
          <div className="log-list">
            {ledger.models.map((m, i) => (
              <div className="s-row ledger-row" key={i}>
                <span className="route-name">{m.name}</span>
                <span className={`route-tag ${m.route}`}>
                  {m.route === "proxy" ? "走代理" : "直连"}
                </span>
                <span className="ledger-ts">{m.channel ?? "—"}</span>
              </div>
            ))}
          </div>
        ) : (
          <div className="set-desc">暂无模型登记。点击"模型"页签执行一键导入。</div>
        )}
      </div>
      <div className="set-card">
        <div className="set-title">操作日志</div>
        {ledger && ledger.changelog.length > 0 ? (
          <div className="log-list">
            {ledger.changelog.slice().reverse().map((c, i) => (
              <div className="s-row ledger-row" key={i}>
                <span className="route-name">{c.op}</span>
                <span className="hint">{c.detail}</span>
                <span className="ledger-ts">{c.ts}</span>
              </div>
            ))}
          </div>
        ) : (
          <div className="set-desc">暂无操作记录。</div>
        )}
      </div>
    </>
  );
}