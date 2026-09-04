import type { ScanProgress } from "../types";

interface Props {
  scanProgress: ScanProgress | null;
  onScan: (force: boolean) => void;
  autoScan: boolean;
  setAutoScan: (v: boolean) => void;
  pollMs: number;
}

export default function ScanTab({ scanProgress, onScan, autoScan, setAutoScan, pollMs }: Props) {
  return (
    <>
      <div className="settings-title">扫描统计（本地会话记录记账）</div>
      <p className="settings-desc">
        扫描 WorkBuddy 本地会话记录（%USERPROFILE%\.workbuddy\projects 下的 jsonl），
        内置与自定义模型统一记账，不依赖代理进程，也不需要启动代理。
        首次全量约 1 秒，之后按文件增量读取；结果缓存在 %APPDATA% 下，重启秒级恢复。
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
          {scanProgress.running ? (
            <span>扫描中… {scanProgress.scanned}/{scanProgress.total} 文件</span>
          ) : (
            <span>
              最近扫描：{scanProgress.scanned}/{scanProgress.total} 文件
              {scanProgress.records > 0 && `，新增 ${scanProgress.records} 条`}
            </span>
          )}
        </div>
      )}
    </>
  );
}