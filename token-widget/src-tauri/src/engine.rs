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
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};
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
struct ModelAgg {
    label: String,
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    calls: i64,
    credits: f64,
}

#[derive(Default, Clone)]
struct DayAgg {
    prompt: i64,
    completion: i64,
    cache_read: i64,
    calls: i64,
}

#[derive(Clone)]
struct Rec {
    ts: String,
    model: String,
    label: String,
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    credit: f64,
    ok: bool,
    file: String,
    call_id: String,
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

struct Aggregate {
    total: Tot,
    by_model: Vec<(String, ModelAgg)>, // 保序（首见顺序）
    by_day: BTreeMap<String, DayAgg>,
    by_model_day: BTreeMap<String, BTreeMap<String, DayAgg>>,
    last_success: Vec<(String, String)>,
    recent: Vec<Rec>,
}

impl Aggregate {
    fn new() -> Self {
        Aggregate {
            total: Tot::default(),
            by_model: Vec::new(),
            by_day: BTreeMap::new(),
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
        match self.by_model.iter_mut().find(|(k, _)| k == &r.model) {
            Some((_, m)) => {
                m.prompt += r.prompt;
                m.completion += r.completion;
                m.reasoning += r.reasoning;
                m.cache_read += r.cache_read;
                m.credits += r.credit;
                m.calls += 1;
            }
            None => {
                self.by_model.push((
                    r.model.clone(),
                    ModelAgg {
                        label: r.label.clone(),
                        prompt: r.prompt,
                        completion: r.completion,
                        reasoning: r.reasoning,
                        cache_read: r.cache_read,
                        credits: r.credit,
                        calls: 1,
                    },
                ));
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

/// 读 models.json -> {自定义名/id: provider label}。不可读/异常返回空映射。
fn load_model_index(models_json: &Path) -> HashMap<String, String> {
    let mut idx = HashMap::new();
    let Ok(content) = fs::read_to_string(models_json) else { return idx };
    let Ok(v) = serde_json::from_str::<Value>(&content) else { return idx };
    let Some(arr) = v.as_array() else { return idx };
    for m in arr {
        let name = m.get("name").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let mid = m.get("id").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        let url = m.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if name.is_empty() && mid.is_empty() {
            continue;
        }
        let provider = provider_from_url(&url);
        if !name.is_empty() {
            idx.entry(name).or_insert(provider.clone());
        }
        if !mid.is_empty() {
            idx.entry(mid).or_insert(provider);
        }
    }
    idx
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
fn rec_from_call(v: &Value, file: &str, custom: &HashMap<String, String>) -> Option<Rec> {
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
        custom.get(&model).cloned().unwrap_or_else(|| "自定义".into())
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
        file: file.to_string(),
        call_id,
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
    agg: Aggregate,
    /// 会话 jsonl 绝对路径 -> 已读字节偏移
    state: HashMap<String, u64>,
    /// (file, callId) 去重；懒加载自缓存
    seen: Option<HashSet<(String, String)>>,
    last_scan: f64,
    scan_ttl: f64,
    progress: Mutex<Progress>,
}

impl Engine {
    pub fn new(data_dir: PathBuf) -> Self {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let cache_dir = data_dir.join(CACHE_DIR_NAME);
        let _ = fs::create_dir_all(&cache_dir);
        Engine {
            home,
            cache_dir,
            agg: Aggregate::new(),
            state: HashMap::new(),
            seen: None,
            last_scan: 0.0,
            scan_ttl: SCAN_TTL,
            progress: Mutex::new(Progress::default()),
        }
    }

    fn now() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
    }

    fn models_json(&self) -> PathBuf {
        self.home.join(".workbuddy").join("models.json")
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
                    "file": r.file, "callId": r.call_id, "reqId": r.req_id,
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
        let mut all: Vec<Rec> = Vec::new();
        for fp in self.cache_files() {
            let Ok(content) = fs::read_to_string(&fp) else { continue };
            for line in content.lines() {
                let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                let ts = v.get("ts").and_then(|x| x.as_str()).unwrap_or("").to_string();
                if ts.is_empty() {
                    continue;
                }
                all.push(Rec {
                    ts,
                    model: v.get("model").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                    label: v.get("label").and_then(|x| x.as_str()).unwrap_or("内置").to_string(),
                    prompt: num(&v, "prompt_tokens"),
                    completion: num(&v, "completion_tokens"),
                    reasoning: num(&v, "reasoning_tokens"),
                    cache_read: num(&v, "cache_read_tokens"),
                    credit: fnum(&v, "credit"),
                    ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(true),
                    file: v.get("file").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    call_id: v.get("callId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    req_id: v.get("reqId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                });
            }
        }
        all.sort_by(|a, b| a.ts.cmp(&b.ts));
        let n = all.len();
        let mut agg = Aggregate::new();
        for rec in all {
            agg.apply(&rec);
            agg.push_recent(rec);
        }
        self.agg = agg;
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
        let custom = load_model_index(&self.models_json());
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
                        let Some(rec) = rec_from_call(&v, &path_key, &custom) else { continue };
                        seen.insert(key.clone());
                        pending.insert(key, rec);
                    }
                    "function_call_result" => {
                        if let Some(mut rec) = pending.remove(&key) {
                            rec.ok = v.get("status").and_then(|x| x.as_str()) == Some("completed");
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
            for rec in new_recs {
                self.agg.apply(&rec);
                self.agg.push_recent(rec);
            }
            self.save_state();
        } else if scanned > 0 {
            self.save_state();
        }
        self.last_scan = Self::now();
        {
            let mut p = self.progress.lock().unwrap();
            p.running = false;
            p.done = true;
            p.scanned = scanned.max(p.total);
        }
        n
    }

    pub fn lazy_scan_needed(&self) -> bool {
        Self::now() - self.last_scan > self.scan_ttl
    }

    pub fn set_scan_ttl(&mut self, seconds: f64) {
        self.scan_ttl = seconds.max(1.0);
    }

    pub fn snapshot(&self) -> Value {
        let agg = &self.agg;
        let total = json!({
            "prompt_tokens": agg.total.prompt,
            "completion_tokens": agg.total.completion,
            "reasoning_tokens": agg.total.reasoning,
            "cache_read_tokens": agg.total.cache_read,
            "calls": agg.total.calls,
            "credits": agg.total.credits,
            "cost": 0,
        });
        let mut by_model = serde_json::Map::new();
        for (name, m) in &agg.by_model {
            by_model.insert(
                name.clone(),
                json!({
                    "model": name,
                    "label": m.label,
                    "prompt_tokens": m.prompt,
                    "completion_tokens": m.completion,
                    "reasoning_tokens": m.reasoning,
                    "cache_read_tokens": m.cache_read,
                    "calls": m.calls,
                    "credits": m.credits,
                    "cost": 0,
                }),
            );
        }
        let by_day: Vec<Value> = agg
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
        for (day, models) in &agg.by_model_day {
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
        let last_success: serde_json::Map<String, Value> = agg
            .last_success
            .iter()
            .map(|(k, ts)| (k.clone(), Value::String(ts.clone())))
            .collect();
        let records: Vec<Value> = agg
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
                    "cost": 0,
                })
            })
            .collect();
        json!({
            "total": total,
            "by_model": Value::Object(by_model),
            "by_day": by_day,
            "by_model_day": Value::Object(by_model_day),
            "last_success": Value::Object(last_success),
            "records": records,
        })
    }

    pub fn progress_snapshot(&self) -> Value {
        let p = self.progress.lock().unwrap();
        json!({
            "running": p.running,
            "total": p.total,
            "scanned": p.scanned,
            "records": p.records,
            "done": p.done,
        })
    }
}
