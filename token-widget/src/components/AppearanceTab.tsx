import type { Dispatch, SetStateAction } from "react";
import type { Theme } from "../types";

interface Props {
  theme: Theme;
  setTheme: Dispatch<SetStateAction<Theme>>;
  acrylic: number;
  setAcrylic: Dispatch<SetStateAction<number>>;
}

export default function AppearanceTab({
  theme, setTheme, acrylic, setAcrylic,
}: Props) {
  return (
    <div className="set-card">
      <div className="set-title">主题</div>
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
      <div className="set-title">亚克力强度</div>
      <div className="acrylic-row">
        <input
          type="range" min={0} max={100}
          value={acrylic} onChange={(e) => setAcrylic(Number(e.target.value))}
        />
        <span className="acrylic-val">{acrylic}%</span>
      </div>
    </div>
  );
}