import type { Dispatch, SetStateAction } from "react";
import type { CacheScope, Theme } from "../types";

interface Props {
  theme: Theme;
  setTheme: Dispatch<SetStateAction<Theme>>;
  acrylic: number;
  setAcrylic: Dispatch<SetStateAction<number>>;
  cacheScope: CacheScope;
  setCacheScope: Dispatch<SetStateAction<CacheScope>>;
  pollMs: number;
  setPollMs: Dispatch<SetStateAction<number>>;
}

export default function AppearanceTab({
  theme, setTheme, acrylic, setAcrylic,
  cacheScope, setCacheScope, pollMs, setPollMs,
}: Props) {
  return (
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
        <label className="acrylic-label" htmlFor="acrylic-range">亚克力强度</label>
        <input
          id="acrylic-range" type="range" min={0} max={100}
          value={acrylic} onChange={(e) => setAcrylic(Number(e.target.value))}
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
      <div className="settings-title">刷新频率</div>
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
    </>
  );
}