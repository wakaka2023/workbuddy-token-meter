import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Stats } from "../types";
import { fmt } from "../utils";

interface Props {
  stats: Stats | null;
}

function ExpandedView({ stats }: Props) {
  const total = stats?.total;
  const today = stats?.by_day[stats.by_day.length - 1];
  const totalTokens = total
    ? total.prompt_tokens + total.completion_tokens + total.cache_read_tokens
    : 0;
  const todayTokens = today
    ? today.prompt_tokens + today.completion_tokens + today.cache_read_tokens
    : 0;
  const cacheRate =
    total && total.prompt_tokens > 0
      ? ((total.cache_read_tokens / total.prompt_tokens) * 100).toFixed(1)
      : "0.0";
  const cost = total?.cost ?? 0;
  const calls = total?.calls ?? 0;

  const modelsStat = stats
    ? Object.entries(stats.by_model)
        .map(([id, m]) => ({ id, ...m }))
        .sort((a, b) => b.calls - a.calls)
    : [];

  const recent = stats ? [...stats.records].reverse().slice(0, 5) : [];

  const trend = (stats?.by_day ?? []).map((d) => ({
    ...d,
    total: d.prompt_tokens + d.completion_tokens + d.cache_read_tokens,
  }));

  return (
    <>
      <div className="cards">
        <div className="card primary in">
          <div className="card-label">总 Token</div>
          <div className="card-value">{fmt(totalTokens)}</div>
          <div className="card-sub">{calls} 次调用</div>
        </div>
        <div className="card today">
          <div className="card-label">今日 Token</div>
          <div className="card-value">{fmt(todayTokens)}</div>
          <div className="card-sub">{today ? `${today.calls} 次调用` : "暂无数据"}</div>
        </div>
        <div className="card cache">
          <div className="card-label">缓存命中率</div>
          <div className="card-value">{cacheRate}%</div>
          <div className="card-sub">cache {fmt(total?.cache_read_tokens ?? 0)}</div>
        </div>
        <div className="card cost">
          <div className="card-label">预估费用</div>
          <div className="card-value">¥{cost.toFixed(2)}</div>
          <div className="card-sub">按 config 单价</div>
        </div>
      </div>

      {trend.length > 1 && (
        <section className="panel chart-panel">
          <div className="panel-title-row">
            <span className="panel-title">Token 趋势（按天）</span>
            <span className="chart-legend">
              <span className="legend-item">
                <span className="legend-dot" style={{ background: "var(--c-in)" }} />
                输入
              </span>
              <span className="legend-item">
                <span className="legend-dot" style={{ background: "var(--c-out)" }} />
                输出
              </span>
              <span className="legend-item">
                <span className="legend-dot" style={{ background: "var(--c-cache)" }} />
                缓存
              </span>
            </span>
          </div>
          <div className="chart-box">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={trend}>
                <defs>
                  <linearGradient id="gIn" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" style={{ stopColor: "var(--c-in)", stopOpacity: 0.45 }} />
                    <stop offset="100%" style={{ stopColor: "var(--c-in)", stopOpacity: 0.04 }} />
                  </linearGradient>
                  <linearGradient id="gOut" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" style={{ stopColor: "var(--c-out)", stopOpacity: 0.45 }} />
                    <stop offset="100%" style={{ stopColor: "var(--c-out)", stopOpacity: 0.04 }} />
                  </linearGradient>
                  <linearGradient id="gCache" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" style={{ stopColor: "var(--c-cache)", stopOpacity: 0.45 }} />
                    <stop offset="100%" style={{ stopColor: "var(--c-cache)", stopOpacity: 0.04 }} />
                  </linearGradient>
                </defs>
                <XAxis
                  dataKey="date"
                  tick={{ fontSize: 9, fill: "var(--text-sub)" }}
                  tickLine={false}
                  axisLine={false}
                />
                <YAxis
                  tick={{ fontSize: 9, fill: "var(--text-sub)" }}
                  tickLine={false}
                  axisLine={false}
                  width={36}
                  tickFormatter={fmt}
                />
                <Tooltip
                  contentStyle={{
                    background: "var(--card-bg)",
                    border: "1px solid var(--card-border)",
                    borderRadius: 8,
                    fontSize: 11,
                    color: "var(--text-main)",
                  }}
                  labelStyle={{ color: "var(--text-strong)" }}
                />
                <Area type="monotone" dataKey="prompt_tokens" name="输入" stackId="1" stroke="var(--c-in)" fill="url(#gIn)" />
                <Area type="monotone" dataKey="completion_tokens" name="输出" stackId="1" stroke="var(--c-out)" fill="url(#gOut)" />
                <Area type="monotone" dataKey="cache_read_tokens" name="缓存命中" stackId="1" stroke="var(--c-cache)" fill="url(#gCache)" />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </section>
      )}

      <section className="panel">
        <div className="panel-title">按模型</div>
        <table className="model-table">
          <thead>
            <tr>
              <th>模型</th>
              <th>渠道</th>
              <th>调用</th>
              <th>输入</th>
              <th>缓存率</th>
              <th>费用</th>
            </tr>
          </thead>
          <tbody>
            {modelsStat.map((m) => (
              <tr key={m.id}>
                <td className="mono">{m.id}</td>
                <td>
                  <span className="prov-chip">{m.label}</span>
                </td>
                <td className="num">{m.calls}</td>
                <td className="mono num">{fmt(m.prompt_tokens)}</td>
                <td className="num">
                  {m.prompt_tokens > 0
                    ? ((m.cache_read_tokens / m.prompt_tokens) * 100).toFixed(0)
                    : "0"}
                  %
                </td>
                <td className="mono num">¥{m.cost.toFixed(2)}</td>
              </tr>
            ))}
            {modelsStat.length === 0 && (
              <tr>
                <td colSpan={6} className="empty">
                  暂无数据
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>

      <section className="panel">
        <div className="panel-title">最近请求</div>
        <ul className="req-list">
          {recent.map((r, i) => {
            const ok = r.ok !== false;
            return (
              <li key={i} className={ok ? "" : "req-err"}>
                <span className={`req-status-dot ${ok ? "ok" : "err"}`} />
                <span className="req-model mono" title={ok ? "" : String(r.error ?? "")}>
                  {String(r.model ?? "")}
                </span>
                <span className="req-tokens mono">
                  {fmt(Number(r.prompt_tokens ?? 0))}→{fmt(Number(r.completion_tokens ?? 0))}
                </span>
                <span className="req-ms mono">
                  {r.duration_ms ? `${Math.round(Number(r.duration_ms))}ms` : "—"}
                </span>
                <span className="req-cost mono">
                  {ok && r.cost !== undefined
                    ? `${Number(r.cost).toFixed(4)}元`
                    : ok
                      ? ""
                      : `HTTP ${r.status ?? "?"}`}
                </span>
                <span className="req-time mono">{String(r.ts ?? "").slice(11)}</span>
              </li>
            );
          })}
          {recent.length === 0 && <li className="empty">暂无请求记录</li>}
        </ul>
      </section>
    </>
  );
}

export default ExpandedView;
