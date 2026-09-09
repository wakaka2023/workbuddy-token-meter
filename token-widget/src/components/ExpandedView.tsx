import { memo, useState } from "react";
import { Area, AreaChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { Stats } from "../types";
import {
  RECENT_LIMITS,
  RECENT_LIMIT_KEY,
  TREND_RANGES,
  TREND_RANGE_KEY,
  type TrendRangeKey,
} from "../constants";
import { fmt, fmtTime, loadLS, saveLS } from "../utils";

interface Props {
  stats: Stats | null;
}

function ExpandedView({ stats }: Props) {
  const [trendKey, setTrendKey] = useState<TrendRangeKey>(() =>
    loadLS(TREND_RANGE_KEY, "d30", (v) =>
      TREND_RANGES.some((r) => r.key === v) ? (v as TrendRangeKey) : null,
    ),
  );
  const [recentLimit, setRecentLimit] = useState<number>(() =>
    loadLS(RECENT_LIMIT_KEY, 10, (v) =>
      (RECENT_LIMITS as readonly number[]).includes(Number(v)) ? Number(v) : null,
    ),
  );

  const total = stats?.total;
  const byDay = stats?.by_day ?? [];
  const lastDay = byDay.length > 0 ? byDay[byDay.length - 1] : null;
  const totalTokens = total
    ? total.prompt_tokens + total.completion_tokens + total.cache_read_tokens
    : 0;
  const cacheRate =
    total && total.prompt_tokens > 0
      ? ((total.cache_read_tokens / total.prompt_tokens) * 100).toFixed(1)
      : "0.0";
  const credits = total?.credits ?? 0;
  const calls = total?.calls ?? 0;

  const modelsStat = stats
    ? [...(stats.by_model ?? [])].sort((a, b) => b.calls - a.calls)
    : [];

  const recent = stats ? [...stats.records].reverse().slice(0, recentLimit) : [];

  // 时间轴槽位：h24 生成连续 24 整点；d7/d30 生成连续自然日；all 从最早数据日到今天。
  // 后端 by_hour/by_day 是"有记录的稀疏键"，直接画会时间轴断裂（0 点跳 8 点、缺天跳过），
  // 须生成连续槽位逐点对齐、空缺补 0。
  const slots = (() => {
    const pad = (n: number) => String(n).padStart(2, "0");
    const out: { key: string }[] = [];
    const now = new Date();
    if (trendKey === "h24") {
      for (let i = 23; i >= 0; i--) {
        const t = new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours() - i, 0, 0);
        out.push({
          key: `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())} ${pad(t.getHours())}`,
        });
      }
    } else {
      const days = trendKey === "d7" ? 7 : trendKey === "d30" ? 30 : 0;
      const first = days > 0
        ? new Date(now.getFullYear(), now.getMonth(), now.getDate() - (days - 1))
        : (() => {
            const d0 = stats?.by_day?.[0]?.date;
            return d0 ? new Date(`${d0}T00:00:00`) : now;
          })();
      for (
        let d = new Date(first.getFullYear(), first.getMonth(), first.getDate());
        d.getTime() <= now.getTime();
        d.setDate(d.getDate() + 1)
      ) {
        out.push({ key: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}` });
      }
    }
    return out;
  })();
  const isHourView = trendKey === "h24";
  const trendSrc = isHourView ? stats?.by_hour : stats?.by_day;
  const hasTrendData = (trendSrc?.length ?? 0) > 0;
  const srcIndex = new Map((trendSrc ?? []).map((d) => [d.date, d]));
  const trend = slots.map((s) => {
    const d = srcIndex.get(s.key);
    return {
      date: s.key,
      prompt_tokens: d?.prompt_tokens ?? 0,
      completion_tokens: d?.completion_tokens ?? 0,
      cache_read_tokens: d?.cache_read_tokens ?? 0,
      calls: d?.calls ?? 0,
      total:
        (d?.prompt_tokens ?? 0) +
        (d?.completion_tokens ?? 0) +
        (d?.cache_read_tokens ?? 0),
    };
  });

  // 今日卡片：by_day 最后一天是否真的今天，避免无请求日显示旧日期
  const today = new Date();
  const todayKey = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(today.getDate()).padStart(2, "0")}`;
  const isToday = lastDay?.date === todayKey;
  const todayTokens = isToday
    ? lastDay.prompt_tokens + lastDay.completion_tokens + lastDay.cache_read_tokens
    : 0;

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
          <div className="card-sub">
            {isToday && lastDay ? `${lastDay.calls} 次调用` : "暂无今日数据"}
          </div>
        </div>
        <div className="card cache">
          <div className="card-label">缓存命中率</div>
          <div className="card-value">{cacheRate}%</div>
          <div className="card-sub">cache {fmt(total?.cache_read_tokens ?? 0)}</div>
        </div>
        <div className="card cost">
          <div className="card-label">官方积分</div>
          <div className="card-value">{credits.toFixed(2)}</div>
          <div className="card-sub">内置渠道回传 · 自定义渠道可能不计</div>
        </div>
      </div>

      <section className="panel chart-panel">
        <div className="panel-title-row">
          <span className="panel-title">Token 趋势（按天）</span>
          <span className="panel-controls">
            <select
              className="mini-select"
              value={trendKey}
              onChange={(e) => {
                const k = e.target.value as TrendRangeKey;
                setTrendKey(k);
                saveLS(TREND_RANGE_KEY, k);
              }}
              title="趋势图时间范围"
            >
              {TREND_RANGES.map((r) => (
                <option key={r.key} value={r.key}>
                  {r.label}
                </option>
              ))}
            </select>
          </span>
        </div>
        {hasTrendData && trend.length > 1 ? (
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
                  interval="preserveStartEnd"
                  minTickGap={24}
                  tickFormatter={(d: string) =>
                    isHourView ? `${d.slice(5, 10)} ${d.slice(11, 13)}时` : d.slice(5)
                  }
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
                  labelFormatter={(d) => (isHourView ? `${String(d).slice(5)}:00` : String(d))}
                />
                <Area type="monotone" dataKey="prompt_tokens" name="输入" stackId="1" stroke="var(--c-in)" fill="url(#gIn)" />
                <Area type="monotone" dataKey="completion_tokens" name="输出" stackId="1" stroke="var(--c-out)" fill="url(#gOut)" />
                <Area type="monotone" dataKey="cache_read_tokens" name="缓存命中" stackId="1" stroke="var(--c-cache)" fill="url(#gCache)" />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        ) : (
          <div className="empty">所选时间范围内暂无数据</div>
        )}
      </section>

      <section className="panel">
        <div className="panel-title-row">
          <span className="panel-title">按模型</span>
          <span className="panel-controls model-count">{modelsStat.length} 个模型</span>
        </div>
        <div className="model-scroll">
          <table className="model-table">
            <thead>
              <tr>
                <th>模型</th>
                <th>渠道</th>
                <th>调用</th>
                <th>输入</th>
                <th>缓存率</th>
                <th>积分</th>
              </tr>
            </thead>
            <tbody>
              {modelsStat.map((m) => (
                <tr key={`${m.model}-${m.label}`}>
                  <td className="mono">{m.model}</td>
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
                  <td className="mono num">
                    {m.credits > 0 ? m.credits.toFixed(2) : "—"}
                  </td>
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
        </div>
      </section>

      <section className="panel">
        <div className="panel-title-row">
          <span className="panel-title">最近请求</span>
          <select
            className="mini-select"
            value={recentLimit}
            onChange={(e) => {
              const v = Number(e.target.value);
              setRecentLimit(v);
              saveLS(RECENT_LIMIT_KEY, String(v));
            }}
            title="最近请求条数"
          >
            {RECENT_LIMITS.map((n) => (
              <option key={n} value={n}>
                近 {n} 条
              </option>
            ))}
          </select>
        </div>
        <ul className="req-list">
          {recent.map((r, i) => {
            const ok = r.ok !== false;
            const errText = String(r.error ?? "");
            const dur = Number(r.duration_ms ?? 0);
            return (
              <li key={i} className={ok ? "" : "req-err"}>
                <span className={`req-status-dot ${ok ? "ok" : "err"}`} />
                <span
                  className="req-model mono"
                  title={ok ? String(r.model ?? "") : errText || `HTTP ${r.status ?? "?"}`}
                >
                  {String(r.model ?? "")}
                </span>
                <span className="req-tokens mono">
                  {fmt(Number(r.prompt_tokens ?? 0))}→{fmt(Number(r.completion_tokens ?? 0))}
                </span>
                <span className="req-ms mono">{dur > 0 ? `${dur}ms` : "—"}</span>
                <span className="req-cost mono">
                  {ok && Number(r.credit ?? 0) > 0
                    ? `${Number(r.credit).toFixed(2)}积分`
                    : ok
                      ? ""
                      : errText.slice(0, 18) || `HTTP ${r.status ?? "?"}`}
                </span>
                <span className="req-time mono">{fmtTime(r.ts)}</span>
              </li>
            );
          })}
          {recent.length === 0 && <li className="empty">暂无请求记录</li>}
        </ul>
      </section>
    </>
  );
}

export default memo(ExpandedView);
