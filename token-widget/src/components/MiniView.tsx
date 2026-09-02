import type { CacheScope } from "../types";
import { fmt } from "../utils";

interface Props {
  online: boolean | null;
  lastRec: Record<string, unknown> | null;
  cacheScope: CacheScope;
  todayCacheRate: string;
  modelTodayCacheRate: string;
}

function MiniView({ online, lastRec, cacheScope, todayCacheRate, modelTodayCacheRate }: Props) {
  const failed = lastRec ? lastRec.ok === false : false;
  const rate = cacheScope === "model" ? modelTodayCacheRate : todayCacheRate;
  const rateTip =
    cacheScope === "model"
      ? `该模型今日缓存命中率（${lastRec?.model ?? ""}）`
      : "全部模型今日缓存命中率";
  const rateNum = parseFloat(rate) || 0;
  const rateTier = rateNum >= 70 ? "high" : rateNum >= 40 ? "mid" : "low";

  return (
    <div className="mini-body">
      {lastRec ? (
        <div className="mini-rec">
          <div className="mini-rec-top">
            <span
              className="mini-model"
              title={failed ? String(lastRec.error ?? "") : String(lastRec.model ?? "")}
            >
              {String(lastRec.model ?? "")}
            </span>
            {failed ? (
              <span className="mini-fail-badge">HTTP {String(lastRec.status ?? "?")}</span>
            ) : (
              <span className="mini-prov">{String(lastRec.label ?? "")}</span>
            )}
          </div>
          {failed ? (
            <div className="mini-err-line" title={String(lastRec.error ?? "")}>
              请求失败
            </div>
          ) : (
            <div className="mini-grid">
              <span className="mini-cell mini-in" title="输入">
                <i className="mini-sym">↑</i>
                <b className="mini-val">{fmt(Number(lastRec.prompt_tokens ?? 0))}</b>
              </span>
              <span className="mini-cell mini-out" title="输出">
                <i className="mini-sym">↓</i>
                <b className="mini-val">{fmt(Number(lastRec.completion_tokens ?? 0))}</b>
              </span>
              <span className="mini-cell mini-cache-tok" title="缓存命中">
                <i className="mini-sym">◎</i>
                <b className="mini-val">{fmt(Number(lastRec.cache_read_tokens ?? 0))}</b>
              </span>
              <span className={`mini-cell mini-rate-cell mini-rate-${rateTier}`} title={rateTip}>
                <i className="mini-sym mini-rate-dot" />
                <b className="mini-val">{rate}%</b>
              </span>
            </div>
          )}
        </div>
      ) : (
        <div className="mini-empty">{online ? "等待请求…" : "代理未连接"}</div>
      )}
    </div>
  );
}

export default MiniView;
