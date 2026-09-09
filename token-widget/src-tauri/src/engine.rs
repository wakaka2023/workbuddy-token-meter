//! 本地统计引擎：扫描 WorkBuddy 会话 jsonl 并聚合 token 用量。
//!
//! v0.3.0（feat/jsonl-source 分支）起数据源从 trace 切到会话 jsonl：
//! `~/.workbuddy/projects/<项目>/<session-id>.jsonl`，每个会话一个文件、按行追加。
//! 相比 trace（2939 文件 / 984 MB / 全量 60s+），jsonl 只有几十个文件、百 MB 级，
//! 全量解析约 1 秒，且记录更全（trace 里部分 generation span 的 toolOutput 为空会被跳过）。
//!
//! 一次 LLM 请求在 jsonl 里对应两条行：
//!   - `type=function_call`：带 `providerData.usage`（inputTokens/outputTokens/
//!     inputTokensDetails[].cached_tokens/outputTokensDetails[].reasoning_tokens）、
//!     `requestModelId`（`custom-local:` 前缀 = 自定义渠道）、`requestModelName`（展示名）、
//!     `conversationRequestId`（与 workbuddy.db 积分表的 requestId 对应）。
//!     auto 档（fast-model/balanced-model/deep-model）时 requestModelName 只是档位名，
//!     真实路由的后端模型在 providerData.model（如 glm-5.3-flash），聚合以真实模型为键；
//!     rawUsage.credit 为官方积分扣费（仅内置渠道有）。
//!   - `type=function_call_result`：`status` = completed / incomplete，失败时带
//!     `providerData.error`。
//! 两行按 callId 配对，配对成功即落账；扫描结束时仍未配对的按成功落账（失败率约 0.2%，
//! 且 result 行通常紧跟 call 行，跨批概率极低）。
//!
//! 注意：jsonl **没有响应时间字段**，三种推算方式（call→result 时间差、相邻事件间隔、
//! 按 traceId 回查 trace）经实测全部不可行——前两者量级差 26 倍且与输出量无关，
//! 后者因 jsonl 与 trace 的 ID 空间完全隔离（交叉比对命中率 0）无法关联。
//! 因此统计里不再有 duration_ms。
//!
//! 增量：state 记录每个文件的已读字节偏移，只解析新增行；只读到最后一个换行符为止，
//! 避免追加写入时的半行。文件变短（被 compact 重写）则 offset 归零重读，靠
//! (file, callId) 去重保证不重复记账。
//!
//! 持久化：DATA_DIR/stats-cache/YYYY-MM-DD.jsonl（按天分片，供重启秒级重建聚合），
//! DATA_DIR/stats-cache/_state.json（文件字节偏移）。缓存是派生物，可随时全量重建。
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, NaiveDateTime, TimeZone};
use serde_json::{json, Value};

const RECENT_KEEP: usize = 200;
const SCAN_TTL: f64 = 30.0;
const STATE_FILE: &str = "_state.json";
/// v2 = jsonl 数据源；v1 是 trace 数据源，缓存格式不兼容，靠标记隔离
const ENGINE_MARK: &str = "widget-v2";
const CACHE_DIR_NAME: &str = "stats-cache";

// ---- 数据模型（与旧 /stats 返回契约一致）----

#[derive(Default, Clone)]
struct Tot {
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    calls: i64,
    credits: f64,
}

#[derive(Default, Clone)]
struct LabelAgg {
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    calls: i64,
    credits: f64,
    /// 该渠道下各显示名出现次数：换过显示名时取本渠道内调用最多的名字，
    /// 避免跨渠道串名（如 B.AI 时期的名字被套到 SENSENOVA 行上）
    names: HashMap<String, u32>,
}

#[derive(Default, Clone)]
struct ModelAgg {
    /// 渠道 -> 独立统计：同一模型多渠道时按渠道拆行展示
    by_label: HashMap<String, LabelAgg>,
}

#[derive(Default, Clone)]
struct DayAgg {
    prompt: i64,
    completion: i64,
    cache_read: i64,
    calls: i64,
}

#[derive(Clone)]
pub(crate) struct Rec {
    ts: String,
    model: String,
    label: String,
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    credit: f64,
    ok: bool,
    /// result 与 call 的时间戳差（毫秒）；未配对落账时为 0
    duration_ms: i64,
    /// 非 completed 时的错误信息（providerData.error.message）
    error: String,
    /// call 行时间戳（毫秒），配对时用于计算耗时；持久化不含此字段
    call_ts: i64,
    file: String,
    call_id: String,
    /// requestModelId（去重记账用同一记录，历史缓存无此字段为空串）
    mid: String,
    req_id: String,
}

#[derive(Clone, Copy)]
pub struct Progress {
    pub running: bool,
    pub total: usize,
    pub scanned: usize,
    pub records: usize,
    pub done: bool,
}

impl Default for Progress {
    fn default() -> Self {
        Progress { running: false, total: 0, scanned: 0, records: 0, done: true }
    }
}

/// 聚合结果（不可变快照）。gen 每真正落账 +1：前端拿它判断「数据是否变化」，
/// 相同则跳过整包 setStats 与重渲染——这是消除 30s 轮询渲染脉冲的关键。
#[derive(Clone)]
pub(crate) struct Aggregate {
    gen: u64,
    total: Tot,
    by_model: Vec<(String, ModelAgg)>, // 保序（首见顺序）
    by_day: BTreeMap<String, DayAgg>,
    /// 小时聚合（键 YYYY-MM-DD HH），仅供趋势图近 24 小时视图，to_json 输出最近一批
    by_hour: BTreeMap<String, DayAgg>,
    by_model_day: BTreeMap<String, BTreeMap<String, DayAgg>>,
    last_success: Vec<(String, String)>,
    recent: Vec<Rec>,
}

/// 聚合快照读写端：读路径 clone Arc（纳秒级、永不等待），
/// 写路径整体换入（全量重建）或 make_mut 就地追加（增量，无读者时零复制）。
/// 与 Engine 大锁解耦：扫描持 Engine 锁期间，读快照完全不受影响。
pub(crate) struct Snapshot {
    inner: Mutex<Arc<Aggregate>>,
}

impl Snapshot {
    pub(crate) fn new() -> Self {
        Snapshot { inner: Mutex::new(Arc::new(Aggregate::new())) }
    }

    pub(crate) fn get(&self) -> Arc<Aggregate> {
        self.inner.lock().unwrap().clone()
    }

    /// 全量重建提交：从零构建的 agg 整体换入，gen 自增（保证前端感知重建）
    pub(crate) fn replace(&self, agg: Aggregate) {
        let mut g = self.inner.lock().unwrap();
        let mut agg = agg;
        agg.gen = g.gen + 1;
        *g = Arc::new(agg);
    }

    /// 增量落账：就地追加。有读者在途时 make_mut 会复制旧快照——旧读者继续读旧数据，
    /// 写者改新副本，天然双缓冲。有记录落账返回 true（gen 已 +1）。
    pub(crate) fn apply_delta(&self, recs: Vec<Rec>) -> bool {
        if recs.is_empty() {
            return false;
        }
        let mut g = self.inner.lock().unwrap();
        let agg = Arc::make_mut(&mut g);
        agg.gen += 1;
        for rec in recs {
            agg.apply(&rec);
            agg.push_recent(rec);
        }
        true
    }
}

impl Aggregate {
    pub(crate) fn gen(&self) -> u64 {
        self.gen
    }

    fn new() -> Self {
        Aggregate {
            gen: 0,
            total: Tot::default(),
            by_model: Vec::new(),
            by_day: BTreeMap::new(),
            by_hour: BTreeMap::new(),
            by_model_day: BTreeMap::new(),
            last_success: Vec::new(),
            recent: Vec::new(),
        }
    }

    fn apply(&mut self, r: &Rec) {
        let t = &mut self.total;
        t.prompt += r.prompt;
        t.completion += r.completion;
        t.reasoning += r.reasoning;
        t.cache_read += r.cache_read;
        t.credits += r.credit;
        t.calls += 1;
        // 统一按 mid 归组：内置大小写变体（Deepseek-V4-Flash / deepseek-v4-flash）共享同一 mid
        // 应合并到一行；mid 缺失时（早期历史记录）回退到 model 字符串。
        let group = if r.mid.is_empty() { &r.model } else { &r.mid };
        match self.by_model.iter_mut().find(|(k, _)| k == group) {
            Some((_, m)) => {
                let la = m.by_label.entry(r.label.clone()).or_default();
                *la.names.entry(r.model.clone()).or_insert(0) += 1;
                la.prompt += r.prompt;
                la.completion += r.completion;
                la.reasoning += r.reasoning;
                la.cache_read += r.cache_read;
                la.credits += r.credit;
                la.calls += 1;
            }
            None => {
                let mut names = HashMap::new();
                names.insert(r.model.clone(), 1u32);
                let la = LabelAgg {
                    prompt: r.prompt,
                    completion: r.completion,
                    reasoning: r.reasoning,
                    cache_read: r.cache_read,
                    credits: r.credit,
                    calls: 1,
                    names,
                };
                let mut by_label = HashMap::new();
                by_label.insert(r.label.clone(), la);
                self.by_model.push((group.clone(), ModelAgg { by_label }));
            }
        }
        if r.ts.len() >= 10 {
            let day = r.ts[..10].to_string();
            let d = self.by_day.entry(day.clone()).or_default();
            d.prompt += r.prompt;
            d.completion += r.completion;
            d.cache_read += r.cache_read;
            d.calls += 1;
            let md = self.by_model_day.entry(day).or_default().entry(r.model.clone()).or_default();
            md.prompt += r.prompt;
            md.completion += r.completion;
            md.cache_read += r.cache_read;
            md.calls += 1;
        }
        if r.ts.len() >= 13 {
            let h = r.ts[..13].to_string();
            let d = self.by_hour.entry(h).or_default();
            d.prompt += r.prompt;
            d.completion += r.completion;
            d.cache_read += r.cache_read;
            d.calls += 1;
        }
        match self.last_success.iter_mut().find(|(k, _)| k == &r.model) {
            Some((_, ts)) => *ts = r.ts.clone(),
            None => self.last_success.push((r.model.clone(), r.ts.clone())),
        }
    }

    fn push_recent(&mut self, rec: Rec) {
        self.recent.push(rec);
        self.recent.sort_by(|a, b| a.ts.cmp(&b.ts));
        if self.recent.len() > RECENT_KEEP {
            let drop = self.recent.len() - RECENT_KEEP;
            self.recent.drain(..drop);
        }
    }
}

// ---- models.json 解析：自定义模型名 -> provider 显示名 ----

fn provider_from_url(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();
    if host == "api.b.ai" || host == "b.ai" {
        return "B.AI".into();
    }
    // 本地代理地址无 provider 含义，归为"自定义"
    if host.is_empty() || host == "127.0.0.1" || host == "localhost" || host == "::1" {
        return "自定义".into();
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2].to_uppercase()
    } else {
        host.to_uppercase()
    }
}

// ---- 渠道解析：多源配置还原「请求发生时模型被路由到的上游」 ----
//
// 请求 jsonl 本身不带渠道字段（无 url），渠道只能从配置还原。信号分层：
//   L1 路由时间线（~/.workbuddy/models.json 及其 .bak-* 备份）：
//      每份备份是某个时刻的完整路由快照，按时间排序成时间窗；
//      请求 ts 落在哪个窗就按哪个快照的 url 识别——直连/换渠道/改名都能还原。
//   L2 当前显式配置（数据目录 config.json + config-ledger.json）：
//      models[id].channel -> channels[ch].base；ledger 的 name -> current_url 供显示名反查。
//   L3 旧代理路由（token-proxy/config.json：models[id].provider -> providers[pid].base）：
//      覆盖走本地代理期（models.json 的 url 是 127.0.0.1 时，真实上游在代理侧）。
// 显示名只作为配置文件里的反查键，绝不参与渠道判定；全查不到保持"自定义"。

struct RouteSnap {
    /// 该快照生效起始毫秒时间戳（请求 ts >= start 且 < 下一窗时使用）
    start: i64,
    by_id: HashMap<String, String>,
    /// 显示名 -> (url, model_id)，供历史缓存（无 model_id）反查
    by_name: HashMap<String, (String, String)>,
}

pub(crate) struct ChannelResolver {
    timeline: Vec<RouteSnap>,
    proxy: HashMap<String, String>,
    /// mid -> 渠道切换点（ms）：ts >= switch 用当前快照，否则用旧时间线。
    /// 由 compute_switch 从缓存记录反推：当前配置名首现 / 旧快照名最后出现。
    switch: HashMap<String, i64>,
}

fn is_loopback(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    host == "127.0.0.1" || host == "localhost" || host == "::1"
}

/// 渠道名取自上游 host；loopback/空视为无渠道信息
fn upstream_channel(url: &str) -> Option<String> {
    if is_loopback(url) {
        return None;
    }
    let p = provider_from_url(url);
    if p.is_empty() || p == "自定义" {
        None
    } else {
        Some(p)
    }
}

fn parse_clock(ts: &str, fmt: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(ts, fmt)
        .ok()
        .and_then(|d| d.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp_millis())
}

fn parse_bak_ts(name: &str) -> Option<i64> {
    let rest = name.strip_prefix("models.json.bak-")?;
    parse_clock(rest, "%Y%m%d-%H%M%S")
}

impl ChannelResolver {
    pub(crate) fn build(home: &Path) -> Self {
        let mut timeline: Vec<RouteSnap> = Vec::new();
        if let Ok(rd) = fs::read_dir(home.join(".workbuddy")) {
            let mut snaps: Vec<(i64, PathBuf)> = Vec::new();
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let t = if name == "models.json" {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .map(|m| m.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0))
                        .unwrap_or(i64::MAX)
                } else if let Some(t) = parse_bak_ts(&name) {
                    t
                } else {
                    continue;
                };
                snaps.push((t, e.path()));
            }
            snaps.sort_by_key(|(t, _)| *t);
            for (start, p) in snaps {
                let mut snap = RouteSnap { start, by_id: HashMap::new(), by_name: HashMap::new() };
                let Ok(content) = fs::read_to_string(&p) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&content) else { continue };
                let Some(arr) = v.as_array() else { continue };
                for m in arr {
                    let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                    let mid = m.get("id").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
                    let url = m.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    if url.is_empty() {
                        continue;
                    }
                    if !mid.is_empty() {
                        snap.by_id.insert(mid.clone(), url.clone());
                    }
                    if !name.is_empty() {
                        snap.by_name.insert(name, (url, mid));
                    }
                }
                timeline.push(snap);
            }
        }

        let mut proxy = HashMap::new();
        if let Ok(content) =
            fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../token-proxy/config.json"))
        {
            if let Ok(v) = serde_json::from_str::<Value>(&content) {
                let providers = v.get("providers").and_then(|x| x.as_object());
                if let Some(models) = v.get("models").and_then(|x| x.as_object()) {
                    for (mid, m) in models {
                        let Some(pid) = m.get("provider").and_then(|x| x.as_str()) else { continue };
                        let base = providers
                            .and_then(|p| p.get(pid))
                            .and_then(|p| p.get("base"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("");
                        if !base.is_empty() {
                            proxy.insert(mid.clone(), base.to_string());
                        }
                    }
                }
            }
        }

        ChannelResolver { timeline, proxy, switch: HashMap::new() }
    }

    /// 快照内按 mid 解析渠道：真实上游直接取；loopback（走本地代理）用代理路由表还原
    fn chan_from_snap(&self, snap: &RouteSnap, mid: &str) -> Option<String> {
        let url = snap.by_id.get(mid)?;
        if let Some(ch) = upstream_channel(url) {
            return Some(ch);
        }
        if let Some(purl) = self.proxy.get(mid) {
            if let Some(ch) = upstream_channel(purl) {
                return Some(ch);
            }
        }
        None
    }

    fn snap_at(&self, ts: i64) -> Option<&RouteSnap> {
        self.timeline.iter().rev().find(|s| s.start <= ts)
    }

    /// 按 model_id（自动去 custom-local: 前缀）解析渠道；失败返回 None。
    /// 解析顺序：ts >= 该 mid 切换点 → 当前快照；否则 → 旧时间线（最早备份前伸）；
    /// 旧快照走代理但代理路由缺失 → 当前快照兜底；均失败 → 代理路由表兜底。
    pub(crate) fn resolve(&self, model_id: &str, ts: i64) -> Option<String> {
        let mid = model_id.strip_prefix("custom-local:").unwrap_or(model_id);
        if let Some(&sw) = self.switch.get(mid) {
            if ts >= sw {
                if let Some(snap) = self.timeline.last() {
                    if let Some(ch) = self.chan_from_snap(snap, mid) {
                        return Some(ch);
                    }
                }
            }
        }
        let has_baks = self.timeline.len() >= 2;
        if has_baks {
            let (baks, cur) = self.timeline.split_at(self.timeline.len() - 1);
            let snap = baks.iter().rev().find(|s| s.start <= ts).or_else(|| baks.first());
            if let Some(snap) = snap {
                if let Some(ch) = self.chan_from_snap(snap, mid) {
                    return Some(ch);
                }
                // 备份内容在备份前已生效（最早备份前伸）；代理 config 缺失该 mid 路由时用当前配置兜底
                if let Some(ch) = self.chan_from_snap(&cur[0], mid) {
                    return Some(ch);
                }
                return None;
            }
        }
        if let Some(purl) = self.proxy.get(mid) {
            if let Some(ch) = upstream_channel(purl) {
                return Some(ch);
            }
        }
        None
    }

    /// 按显示名反查（历史缓存无 model_id 时）；仅沿时间线精确匹配，
    /// 不查当前态配置——避免把现在的渠道套到历史记录上。
    /// 用户可能给显示名附加括号后缀（如 "GLM-5.3-Flash(B.AI测试)"），
    /// 精确匹配失败时剥离末尾括号段再次匹配配置 name（括号内容不参与判断）。
    pub(crate) fn resolve_by_name(&self, name: &str, ts: i64) -> Option<String> {
        let stripped = name.rfind('(').map(|i| name[..i].trim().to_string());
        let candidates: Vec<&str> = match &stripped {
            Some(s) if s != name => vec![name, s],
            _ => vec![name],
        };
        for cand in candidates {
            if let Some(snap) = self.snap_at(ts) {
                if let Some((url, mid)) = snap.by_name.get(cand) {
                    if let Some(ch) = upstream_channel(url) {
                        return Some(ch);
                    }
                    if let Some(purl) = self.proxy.get(mid) {
                        if let Some(ch) = upstream_channel(purl) {
                            return Some(ch);
                        }
                    }
                    return None;
                }
            }
        }
        None
    }

    /// 从缓存记录反推渠道切换点：switch[mid] = max(当前配置名首现 ts, 旧快照名最后出现 ts)，
    /// 无切换信号（新模型/全程同名）用当前快照起点（mtime）兜底。
    pub(crate) fn compute_switch<I>(&mut self, rows: I)
    where
        I: Iterator<Item = (String, String, i64)>,
    {
        let Some(cur) = self.timeline.last() else { return };
        let mtime = cur.start;
        let mut mid_to_name: HashMap<String, String> = HashMap::new();
        for (nm, (_, mid)) in &cur.by_name {
            if !mid.is_empty() {
                mid_to_name.entry(mid.clone()).or_insert_with(|| nm.clone());
            }
        }
        let mut old_mid_to_name: HashMap<String, String> = HashMap::new();
        if self.timeline.len() >= 2 {
            let last_bak = &self.timeline[self.timeline.len() - 2];
            for (nm, (_, mid)) in &last_bak.by_name {
                if !mid.is_empty() {
                    old_mid_to_name.entry(mid.clone()).or_insert_with(|| nm.clone());
                }
            }
        }
        let mut first_ts: HashMap<String, i64> = HashMap::new();
        let mut last_ts: HashMap<String, i64> = HashMap::new();
        for (mid, nm, ts) in rows {
            let mid = mid.strip_prefix("custom-local:").unwrap_or(&mid).to_string();
            if let Some(cur_name) = mid_to_name.get(&mid) {
                if &nm == cur_name {
                    let e = first_ts.entry(mid.clone()).or_insert(ts);
                    if ts < *e {
                        *e = ts;
                    }
                }
            }
            if let Some(old_name) = old_mid_to_name.get(&mid) {
                if &nm == old_name {
                    let e = last_ts.entry(mid.clone()).or_insert(ts);
                    if ts > *e {
                        *e = ts;
                    }
                }
            }
        }
        let mut switch = HashMap::new();
        for (mid, cur_name) in mid_to_name {
            let a = first_ts.get(&mid).copied();
            let b = if old_mid_to_name.get(&mid) != Some(&cur_name) {
                last_ts.get(&mid).copied()
            } else {
                None
            };
            let base = match (a, b) {
                (Some(x), Some(y)) => x.max(y),
                (Some(x), None) => x,
                (None, Some(y)) => y,
                (None, None) => mtime,
            };
            switch.insert(mid, base.min(mtime));
        }
        self.switch = switch;
    }
}

// ---- 会话 jsonl 解析 ----

fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}

fn fnum(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

/// 毫秒时间戳 -> 本地 "YYYY-MM-DD HH:MM:SS"
fn ms_to_local(ms: i64) -> String {
    let secs = ms / 1000;
    let nsec = ((ms % 1000) * 1_000_000) as u32;
    match Local.timestamp_opt(secs, nsec) {
        chrono::LocalResult::Single(dt) => dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        _ => Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

/// usage 里的 details 字段可能是数组（[{cached_tokens: N}]）或对象，统一求和
fn sum_details(v: Option<&Value>, key: &str) -> i64 {
    match v {
        Some(Value::Array(a)) => a.iter().map(|x| x.get(key).and_then(|y| y.as_i64()).unwrap_or(0)).sum(),
        Some(Value::Object(_)) => v.and_then(|x| x.get(key)).and_then(|x| x.as_i64()).unwrap_or(0),
        _ => 0,
    }
}

/// 从 `type=function_call` 行提取一条记账记录。
/// requestModelId 带 `custom-local:` 前缀 = 自定义渠道，其余为内置模型。
/// auto 档（fast-model/balanced-model/deep-model）的 requestModelName 只是档位名
/// （快速/均衡/极致），实际路由的后端模型在 providerData.model（如 glm-5.3-flash），
/// 此时以真实模型为聚合键、档位名进 label，与桌面端展示一致。
/// rawUsage.credit 为官方积分扣费（仅内置渠道有，自定义渠道无此字段记 0）。
/// usage 两个主字段都 0 视为无用量跳过；无 callId 无法与 result 配对，也跳过。
fn rec_from_call(v: &Value, file: &str, res: &ChannelResolver) -> Option<Rec> {
    let pd = v.get("providerData")?;
    let usage = pd.get("usage")?;
    if !usage.is_object() {
        return None;
    }
    let prompt = usage.get("inputTokens").and_then(|x| x.as_i64()).unwrap_or(0);
    let completion = usage.get("outputTokens").and_then(|x| x.as_i64()).unwrap_or(0);
    if prompt == 0 && completion == 0 {
        return None;
    }
    let call_id = v.get("callId").and_then(|x| x.as_str())?.to_string();
    if call_id.is_empty() {
        return None;
    }
    let model_id = pd.get("requestModelId").and_then(|x| x.as_str()).unwrap_or("");
    let call_ts = v.get("timestamp").and_then(|x| x.as_i64()).unwrap_or(0);
    let is_custom = model_id.starts_with("custom-local:");
    let is_tier = matches!(model_id, "fast-model" | "balanced-model" | "deep-model");
    let backend_model = pd.get("model").and_then(|x| x.as_str()).unwrap_or("").trim();
    let model_name = pd.get("requestModelName").and_then(|x| x.as_str()).unwrap_or("").trim();
    let model = if is_tier && !backend_model.is_empty() {
        backend_model.to_string()
    } else if model_name.is_empty() {
        model_id.to_string()
    } else {
        model_name.to_string()
    };
    if model.is_empty() {
        return None;
    }
    let label = if is_custom {
        res.resolve(model_id, call_ts).unwrap_or_else(|| "自定义".into())
    } else {
        match model_id {
            "fast-model" => "内置·快速",
            "balanced-model" => "内置·均衡",
            "deep-model" => "内置·极致",
            _ => "内置",
        }
        .into()
    };
    Some(Rec {
        ts: ms_to_local(v.get("timestamp").and_then(|x| x.as_i64()).unwrap_or(0)),
        model,
        label,
        prompt,
        completion,
        reasoning: sum_details(usage.get("outputTokensDetails"), "reasoning_tokens"),
        cache_read: sum_details(usage.get("inputTokensDetails"), "cached_tokens"),
        credit: pd.get("rawUsage").map(|ru| fnum(ru, "credit")).unwrap_or(0.0),
        ok: true,
        duration_ms: 0,
        error: String::new(),
        call_ts,
        file: file.to_string(),
        call_id,
        mid: model_id.to_string(),
        req_id: pd
            .get("conversationRequestId")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// 从 offset 处读到文件尾。失败返回 None（下一轮再试）。
fn read_from_offset(path: &Path, offset: u64) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(offset)).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

// ---- 扫描引擎 ----

pub struct Engine {
    home: PathBuf,
    cache_dir: PathBuf,
    /// 会话 jsonl 绝对路径 -> 已读字节偏移
    state: HashMap<String, u64>,
    /// (file, callId) 去重；懒加载自缓存
    seen: Option<HashSet<(String, String)>>,
    last_scan: f64,
    scan_ttl: f64,
    /// 守护线程是否允许按 ttl 周期自动增量扫描（对应前端「自动扫描」开关）
    auto: bool,
    /// 扫描进度独立小锁：扫描期间可被实时读到，不依赖 Engine 大锁
    progress: Arc<Mutex<Progress>>,
    /// 聚合结果写端：scan/boot 完成后 commit，读端在 lib.rs 直接 clone 快照
    snap: Arc<Snapshot>,
    /// 渠道切换点（mid -> ms），rebuild 时反推，scan 复用
    switch_ts: HashMap<String, i64>,
}

impl Engine {
    pub(crate) fn new(data_dir: PathBuf, snap: Arc<Snapshot>) -> Self {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let cache_dir = data_dir.join(CACHE_DIR_NAME);
        let _ = fs::create_dir_all(&cache_dir);
        Engine {
            home,
            cache_dir,
            state: HashMap::new(),
            seen: None,
            last_scan: 0.0,
            scan_ttl: SCAN_TTL,
            auto: false,
            progress: Arc::new(Mutex::new(Progress::default())),
            snap,
            switch_ts: HashMap::new(),
        }
    }

    /// 进度锁读端句柄：AppState 持同一 Arc，扫描期间实时可读
    pub(crate) fn progress_handle(&self) -> Arc<Mutex<Progress>> {
        self.progress.clone()
    }

    pub(crate) fn auto_enabled(&self) -> bool {
        self.auto
    }

    pub fn set_auto_scan(&mut self, on: bool) {
        self.auto = on;
    }

    fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }

    fn cache_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.cache_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".jsonl") {
                    out.push(e.path());
                }
            }
        }
        out.sort();
        out
    }

    /// 懒加载 seen：从按天缓存扫出全部 (file, callId)
    fn seen_keys(&mut self) -> &HashSet<(String, String)> {
        if self.seen.is_none() {
            let mut set = HashSet::new();
            for fp in self.cache_files() {
                let Ok(content) = fs::read_to_string(&fp) else { continue };
                for line in content.lines() {
                    if let Ok(v) = serde_json::from_str::<Value>(line) {
                        if let (Some(f), Some(c)) = (
                            v.get("file").and_then(|x| x.as_str()),
                            v.get("callId").and_then(|x| x.as_str()),
                        ) {
                            set.insert((f.to_string(), c.to_string()));
                        }
                    }
                }
            }
            self.seen = Some(set);
        }
        self.seen.as_ref().unwrap()
    }

    fn save_state(&self) {
        let files: serde_json::Map<String, Value> =
            self.state.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        let payload = json!({ "engine": ENGINE_MARK, "files": files });
        if let Ok(s) = serde_json::to_string(&payload) {
            let _ = fs::write(self.cache_dir.join(STATE_FILE), s);
        }
    }

    fn load_state(&mut self) {
        let Ok(content) = fs::read_to_string(self.cache_dir.join(STATE_FILE)) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&content) else { return };
        // 仅认本引擎标记；trace 版（v1）state 忽略，避免偏移语义混用
        if v.get("engine").and_then(|x| x.as_str()) != Some(ENGINE_MARK) {
            return;
        }
        if let Some(files) = v.get("files").and_then(|x| x.as_object()) {
            for (k, val) in files {
                if let Some(off) = val.as_u64() {
                    self.state.insert(k.clone(), off);
                }
            }
        }
    }

    fn append_recs(&self, recs: &[Rec]) {
        let mut by_day: BTreeMap<String, Vec<&Rec>> = BTreeMap::new();
        for r in recs {
            let day = if r.ts.len() >= 10 { r.ts[..10].to_string() } else { "unknown".into() };
            by_day.entry(day).or_default().push(r);
        }
        for (day, day_recs) in by_day {
            let p = self.cache_dir.join(format!("{day}.jsonl"));
            let mut out = String::new();
            for r in day_recs {
                let rec = json!({
                    "ts": r.ts, "model": r.model, "label": r.label,
                    "prompt_tokens": r.prompt, "completion_tokens": r.completion,
                    "reasoning_tokens": r.reasoning, "cache_read_tokens": r.cache_read,
                    "credit": r.credit, "ok": r.ok,
                    "duration_ms": r.duration_ms, "error": r.error,
                    "file": r.file, "callId": r.call_id, "mid": r.mid, "reqId": r.req_id,
                });
                out.push_str(&serde_json::to_string(&rec).unwrap());
                out.push('\n');
            }
            if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&p) {
                let _ = f.write_all(out.as_bytes());
            }
        }
    }

    /// 启动引导：加载增量 state，再从按天缓存重建聚合。返回缓存记录数。
    pub fn boot(&mut self) -> usize {
        self.load_state();
        let n = self.rebuild_from_cache();
        if n == 0 {
            self.last_scan = 0.0; // 无缓存 → 首次扫描触发全量建缓存
        }
        n
    }

    /// 从按天缓存重建聚合，重启秒级。返回记录数。
    pub fn rebuild_from_cache(&mut self) -> usize {
        let mut res = ChannelResolver::build(&self.home);
        let mut rows: Vec<(String, String, i64)> = Vec::new();
        let mut raw: Vec<(Value, String, String, String, String, i64)> = Vec::new();
        for fp in self.cache_files() {
            let Ok(content) = fs::read_to_string(&fp) else { continue };
            for line in content.lines() {
                let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                let ts = v.get("ts").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if ts.is_empty() {
                    continue;
                }
                let model = v.get("model").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                let mid = v.get("mid").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let old_label = v.get("label").and_then(|x| x.as_str()).unwrap_or("内置").to_string();
                let ts_ms = parse_clock(&ts, "%Y-%m-%d %H:%M:%S").unwrap_or(0);
                rows.push((mid.clone(), model.clone(), ts_ms));
                raw.push((v, ts, model, mid, old_label, ts_ms));
            }
        }
        res.compute_switch(rows.into_iter());
        self.switch_ts = res.switch.clone();
        let mut all: Vec<Rec> = Vec::new();
        for (v, ts, model, mid, old_label, ts_ms) in raw {
            // 历史缓存无 mid（旧格式）：仅当原 label 是"自定义"时尝试按显示名反查修正；
            // 有 mid 的自定义模型统一走解析管道，失败归"自定义"（不保留可能被污染的旧 label）
            let label = if !mid.is_empty() {
                if mid.starts_with("custom-local:") {
                    res.resolve(&mid, ts_ms).unwrap_or_else(|| "自定义".into())
                } else {
                    old_label
                }
            } else if old_label == "自定义" {
                res.resolve_by_name(&model, ts_ms).unwrap_or(old_label)
            } else {
                old_label
            };
            all.push(Rec {
                ts,
                model,
                label,
                prompt: num(&v, "prompt_tokens"),
                completion: num(&v, "completion_tokens"),
                reasoning: num(&v, "reasoning_tokens"),
                cache_read: num(&v, "cache_read_tokens"),
                credit: fnum(&v, "credit"),
                ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(true),
                duration_ms: v.get("duration_ms").and_then(|x| x.as_i64()).unwrap_or(0),
                error: v.get("error").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                call_ts: ts_ms,
                file: v.get("file").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                call_id: v.get("callId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                mid,
                req_id: v.get("reqId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            });
        }
        all.sort_by(|a, b| a.ts.cmp(&b.ts));
        let n = all.len();
        let mut agg = Aggregate::new();
        for rec in all {
            agg.apply(&rec);
            agg.push_recent(rec);
        }
        self.snap.replace(agg);
        n
    }

    fn clear_cache(&mut self) {
        self.state.clear();
        self.seen = None;
        for fp in self.cache_files() {
            let _ = fs::remove_file(&fp);
        }
        let _ = fs::remove_file(self.cache_dir.join(STATE_FILE));
    }

    fn walk_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                Self::walk_jsonl(&p, out);
            } else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                out.push(p);
            }
        }
    }

    /// 枚举 ~/.workbuddy/projects 下所有会话 jsonl
    fn collect_session_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let root = self.home.join(".workbuddy").join("projects");
        Self::walk_jsonl(&root, &mut out);
        out.sort();
        out
    }

    /// 扫描：按字节偏移增量解析新增行；force 时清缓存全量重建。返回新增记录数。
    /// 由外部 Mutex<Engine> 串行化，函数内不再自持锁。
    pub fn scan(&mut self, force: bool) -> usize {
        {
            let mut p = self.progress.lock().unwrap();
            p.running = false;
            p.done = false;
            p.scanned = 0;
            p.records = 0;
        }
        if force {
            self.clear_cache();
        }
        let mut res = ChannelResolver::build(&self.home);
        if self.switch_ts.is_empty() {
            // 无缓存重建过（首装/异常）：当前快照的 mid 都视为从当前时刻起用当前渠道
            res.compute_switch(std::iter::empty());
        } else {
            res.switch = self.switch_ts.clone();
        }
        let paths = self.collect_session_files();
        {
            let mut p = self.progress.lock().unwrap();
            p.running = true;
            p.total = paths.len();
        }
        let mut seen = self.seen_keys().clone();
        let mut new_recs: Vec<Rec> = Vec::new();
        // (file, callId) -> 待与 result 配对的记录
        let mut pending: HashMap<(String, String), Rec> = HashMap::new();
        let mut scanned = 0usize;

        for p in &paths {
            let Ok(meta) = fs::metadata(p) else { continue };
            let size = meta.len();
            let path_key = p.to_string_lossy().into_owned();
            let mut offset = self.state.get(&path_key).copied().unwrap_or(0);
            // 文件变短说明被 compact 重写，从头重读（靠 seen 去重避免重复记账）
            if size < offset {
                offset = 0;
            }
            if size == offset {
                continue;
            }
            let Some(text) = read_from_offset(p, offset) else { continue };
            // 只处理到最后一个换行，避免追加写入的半行
            let end = text.rfind('\n').map(|i| i + 1).unwrap_or(0);
            if end == 0 {
                continue;
            }
            scanned += 1;
            let new_offset = offset + end as u64;
            let mut file_new = 0usize;

            for line in text[..end].lines() {
                let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                let Some(call_id) = v.get("callId").and_then(|x| x.as_str()) else { continue };
                let key = (path_key.clone(), call_id.to_string());
                match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                    "function_call" => {
                        if seen.contains(&key) {
                            continue;
                        }
                        let Some(rec) = rec_from_call(&v, &path_key, &res) else { continue };
                        seen.insert(key.clone());
                        pending.insert(key, rec);
                    }
                    "function_call_result" => {
                        if let Some(mut rec) = pending.remove(&key) {
                            let completed = v.get("status").and_then(|x| x.as_str()) == Some("completed");
                            rec.ok = completed;
                            // 耗时 = result 与 call 的时间戳差（同一行流内配对，近似请求时长）
                            let rts = v.get("timestamp").and_then(|x| x.as_i64()).unwrap_or(0);
                            let cts = rec.call_ts;
                            if rts > 0 && cts > 0 {
                                rec.duration_ms = (rts - cts).max(0);
                            }
                            if !completed {
                                // 错误信息在 result 的 providerData.error.message（如中断/异常）
                                if let Some(msg) = v
                                    .get("providerData")
                                    .and_then(|p| p.get("error"))
                                    .and_then(|e| e.get("message"))
                                    .and_then(|m| m.as_str())
                                {
                                    rec.error = msg.to_string();
                                }
                            }
                            new_recs.push(rec);
                            file_new += 1;
                        }
                    }
                    _ => {}
                }
            }
            // 本文件扫完仍未配对的一律按成功落账，不留到下一批
            for (_, rec) in pending.drain() {
                new_recs.push(rec);
                file_new += 1;
            }
            self.state.insert(path_key, new_offset);
            {
                let mut p = self.progress.lock().unwrap();
                p.scanned = scanned;
                p.records += file_new;
            }
        }

        let n = new_recs.len();
        if n > 0 {
            self.append_recs(&new_recs);
            if force {
                // 全量重建：从本轮解析的全部记录构建完整聚合，整体换入快照
                self.commit_full(&new_recs);
            } else {
                // 增量：就地追加（新记录少，make_mut 毫秒级）
                self.snap.apply_delta(new_recs);
            }
            self.save_state();
        } else if scanned > 0 {
            self.save_state();
        }
        // 把本轮学习到的 (file, callId) 全量回写内存去重集，与刚写盘的缓存保持一致。
        // 否则 force 全量扫描后内存 seen 为空/旧，一旦会话 jsonl 被 compact 重写触发
        // offset 归零重读，去重失效，同一批历史请求会被重复记账导致统计累加。
        self.seen = Some(seen);
        self.last_scan = Self::now();
        {
            let mut p = self.progress.lock().unwrap();
            p.running = false;
            p.done = true;
            p.scanned = scanned.max(p.total);
        }
        n
    }

    /// 全量重建提交：在快照锁外构建完整聚合，完成后整体换入（锁内仅换 Arc，读路径无感知）
    fn commit_full(&self, recs: &[Rec]) {
        let mut agg = Aggregate::new();
        for rec in recs {
            agg.apply(rec);
            agg.push_recent(rec.clone());
        }
        self.snap.replace(agg);
    }

    pub fn lazy_scan_needed(&self) -> bool {
        Self::now() - self.last_scan > self.scan_ttl
    }

    pub fn set_scan_ttl(&mut self, seconds: f64) {
        self.scan_ttl = seconds.max(1.0);
    }
}

impl Aggregate {
    /// 序列化为 /stats 契约（total/by_model/by_day/by_model_day/last_success/records），
    /// 顶层带 gen：前端以此判断数据是否变化，避免整包重渲染。调用方应在阻塞线程池执行。
    pub(crate) fn to_json(&self) -> Value {
        let total = json!({
            "prompt_tokens": self.total.prompt,
            "completion_tokens": self.total.completion,
            "reasoning_tokens": self.total.reasoning,
            "cache_read_tokens": self.total.cache_read,
            "calls": self.total.calls,
            "credits": self.total.credits,
            "cost": 0,
        });
        let mut by_model: Vec<Value> = Vec::new();
        for (name, m) in &self.by_model {
            let mut labels: Vec<(&String, &LabelAgg)> = m.by_label.iter().collect();
            labels.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));
            for (label, la) in labels {
                let display = la
                    .names
                    .iter()
                    .max_by_key(|(_, c)| **c)
                    .map(|(n, _)| n.clone())
                    .unwrap_or_else(|| name.clone());
                by_model.push(json!({
                    "model": display,
                    "label": label,
                    "prompt_tokens": la.prompt,
                    "completion_tokens": la.completion,
                    "reasoning_tokens": la.reasoning,
                    "cache_read_tokens": la.cache_read,
                    "calls": la.calls,
                    "credits": la.credits,
                    "cost": 0,
                }));
            }
        }
        let by_day: Vec<Value> = self
            .by_day
            .iter()
            .map(|(day, d)| {
                json!({
                    "date": day,
                    "prompt_tokens": d.prompt,
                    "completion_tokens": d.completion,
                    "cache_read_tokens": d.cache_read,
                    "calls": d.calls,
                })
            })
            .collect();
        let mut by_model_day = serde_json::Map::new();
        for (day, models) in &self.by_model_day {
            let mut mm = serde_json::Map::new();
            for (name, d) in models {
                mm.insert(
                    name.clone(),
                    json!({
                        "date": day,
                        "prompt_tokens": d.prompt,
                        "completion_tokens": d.completion,
                        "cache_read_tokens": d.cache_read,
                        "calls": d.calls,
                    }),
                );
            }
            by_model_day.insert(day.clone(), Value::Object(mm));
        }
        let last_success: serde_json::Map<String, Value> = self
            .last_success
            .iter()
            .map(|(k, ts)| (k.clone(), Value::String(ts.clone())))
            .collect();
        let records: Vec<Value> = self
            .recent
            .iter()
            .map(|r| {
                json!({
                    "ts": r.ts,
                    "model": r.model,
                    "label": r.label,
                    "prompt_tokens": r.prompt,
                    "completion_tokens": r.completion,
                    "reasoning_tokens": r.reasoning,
                    "cache_read_tokens": r.cache_read,
                    "credit": r.credit,
                    "ok": r.ok,
                    "duration_ms": r.duration_ms,
                    "error": r.error,
                    "cost": 0,
                })
            })
            .collect();
        let by_hour: Vec<Value> = self
            .by_hour
            .iter()
            .rev()
            .take(48)
            .rev() // 恢复时间升序
            .map(|(hour, d)| {
                json!({
                    "date": hour,
                    "prompt_tokens": d.prompt,
                    "completion_tokens": d.completion,
                    "cache_read_tokens": d.cache_read,
                    "calls": d.calls,
                })
            })
            .collect();
        json!({
            "gen": self.gen,
            "total": total,
            "by_model": by_model,
            "by_day": by_day,
            "by_hour": by_hour,
            "by_model_day": Value::Object(by_model_day),
            "last_success": Value::Object(last_success),
            "records": records,
        })
    }
}
