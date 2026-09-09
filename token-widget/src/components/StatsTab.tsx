import type { Dispatch, SetStateAction } from "react";
import type { CacheScope, ScanProgress } from "../types";

interface Props {
  pollMs: number;
  setPollMs: Dispatch<SetStateAction<number>>;
  cacheScope: CacheScope;
  setCacheScope: Dispatch<SetStateAction<CacheScope>>;
  scanProgress: ScanProgress | null;
  onScan: (force: boolean) => void;
  autoScan: boolean;
  setAutoScan: (v: boolean) => void;
}

const POLL_CHIPS = [15, 30, 60] as const;

export default function StatsTab({
  pollMs, setPollMs, cacheScope, setCacheScope,
  scanProgress, onScan, autoScan, setAutoScan,
}: Props) {
  const pct =
    scanProgress && scanProgress.total > 0
      ? Math.min(100, Math.round((scanProgress.scanned / scanProgress.total) * 100))
      : 0;
  return (
    <>
      <div className="set-card">
        <div className="set-title">数据更新</div>
        <p className="set-desc">
          扫描 WorkBuddy 本地会话记录（%USERPROFILE%\.workbuddy\projects 下的 jsonl），
          内置与自定义模型统一记账；结果缓存在 %APPDATA% 下，重启秒级恢复。
        </p>
        <div className="toggle-row">
          <div>
            <div className="toggle-label">自动增量扫描</div>
            <div className="toggle-sub">
              {autoScan
                ? `已开启：每 ${Math.round(pollMs / 1000)} 秒自动扫一次新记录`
                : "已关闭：统计只在手动扫描后更新"}
            </div>
          </div>
          <button
            className={`switch${autoScan ? " on" : ""}`}
            onClick={() => setAutoScan(!autoScan)}
            role="switch"
            aria-checked={autoScan}
            title="开启后按刷新频率自动增量扫描"
          />
        </div>
        <div className="s-row scan-actions">
          <button className="add-btn" onClick={() => onScan(false)}>增量扫描</button>
          <button className="add-btn" onClick={() => onScan(true)}>重新全量扫描</button>
        </div>
        {scanProgress && (
          <div className="scan-progress">
            <div className="scan-bar">
              <div className="scan-bar-fill" style={{ width: `${pct}%` }} />
            </div>
            <span>
              {scanProgress.running ? (
                `扫描中… ${scanProgress.scanned}/${scanProgress.total} 文件`
              ) : (
                `最近扫描：${scanProgress.scanned}/${scanProgress.total} 文件` +
                (scanProgress.records > 0 ? `，新增 ${scanProgress.records} 条` : "")
              )}
            </span>
          </div>
        )}
        <div className="set-title">刷新频率</div>
        <div className="acrylic-row">
          <label className="acrylic-label" htmlFor="poll-sec">每</label>
          <input
            id="poll-sec" type="number" min={5} max={600} step={5}
            value={Math.round(pollMs / 1000)}
            onChange={(e) => {
              const s = Number(e.target.value);
              if (Number.isFinite(s) && s >= 5 && s <= 600) setPollMs(Math.round(s * 1000));
            }}
          />
          <span className="acrylic-val">秒刷新一次</span>
        </div>
        <div className="poll-presets">
          {POLL_CHIPS.map((s) => (
            <button
              key={s}
              className={`poll-chip${Math.round(pollMs / 1000) === s ? " on" : ""}`}
              onClick={() => setPollMs(s * 1000)}
            >
              {s} 秒
            </button>
          ))}
        </div>
      </div>
      <div className="set-card">
        <div className="set-title">Mini 缓存率口径</div>
        <p className="set-desc">mini 窗口右上角缓存命中率的统计范围</p>
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
      </div>
    </>
  );
}