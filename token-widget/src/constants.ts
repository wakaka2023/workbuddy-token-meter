import type { Theme } from "./types";

export const PROXY = "http://127.0.0.1:8787";
export const POLL_MS = 30000;
export const MINI_SIZE = { w: 250, h: 130 };
export const EXPD_SIZE = { w: 500, h: 640 };

export const THEME_KEY = "tw-theme";
export const ACRYLIC_KEY = "tw-acrylic";
export const CACHE_SCOPE_KEY = "tw-cache-scope";

// 深/浅主题各自的基础 RGB（亚克力滑块插值透明度）
export const THEME_BG_RGB: Record<Theme, [string, string]> = {
  dark: ["34, 42, 66", "14, 17, 30"],
  light: ["248, 250, 255", "226, 233, 249"],
};

// 滑块值 -> 不透明度：0 → 0.15，100 → 0.95
export const acrylicAlpha = (v: number) => 0.15 + (Math.min(100, Math.max(0, v)) / 100) * 0.8;

export const ICON_EXPAND = "M9 4H4v5M15 20h5v-5M4 4l6 6M20 20l-6-6";
export const ICON_COLLAPSE = "M9 20H4v-5M15 4h5v5M4 20l6-6M20 4l-6 6";
export const ICON_SETTINGS =
  "M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z" +
  "M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1 1.55V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1-1.55 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.55-1H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.55-1 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.09a1.7 1.7 0 0 0 1-1.55V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.55h.09a1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.09a1.7 1.7 0 0 0 1.55 1H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.55 1Z";
export const ICON_MINIMIZE = "M5 12h14";
export const ICON_CLOSE = "M6 6l12 12M18 6L6 18";
