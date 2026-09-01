export function fmt(n: number): string {
  if (n >= 1e6) return (n / 1e6).toFixed(1) + "M";
  if (n >= 1e3) return (n / 1e3).toFixed(1) + "K";
  return String(n);
}

// 今天只显示 HH:MM:SS，跨天带 MM-DD
export function fmtTime(ts: unknown): string {
  const s = String(ts ?? "");
  if (!s) return "";
  const d = new Date(s);
  const pad = (n: number) => String(n).padStart(2, "0");
  const hms = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  if (isNaN(d.getTime())) return s.length >= 19 ? s.slice(11, 19) : s;
  return d.toDateString() === new Date().toDateString()
    ? hms
    : `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hms}`;
}

export function loadLS<T>(key: string, fallback: T, parse?: (raw: string | null) => T | null): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw == null) return fallback;
    return parse ? parse(raw) ?? fallback : (raw as T);
  } catch {
    return fallback;
  }
}

export function saveLS(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* ignore */
  }
}
