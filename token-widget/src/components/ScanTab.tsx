import type { ScanProgress } from "../types";

interface Props {
  scanProgress: ScanProgress | null;
  onScan: (force: boolean) => void;
}

export default function ScanTab({ scanProgress, onScan }: Props) {
  return (
    <>
      <div className="settings-title">扫描统计（本地 trace 记账）</div>
      <p className="settings-desc">
        扫描 WorkBuddy 本地 trace 文件，加载内置模型与直连自定义模型的用量。
        首次约 8s，之后增量秒级；结果缓存在 %APPDATA% 的 trace-cache。
      </p>
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