//! 本地统计引擎：扫描 WorkBuddy trace 并聚合 token 用量。
//!
//! v0.2.4 起统计不再依赖 token-proxy 进程/8787 端口，widget 内建引擎直读
//! `~/.workbuddy/traces/*/trace_*.json`。设计对齐旧 Python 实现（trace_reader/
//! trace_cache/aggregate 三模块语义），但去掉了"走代理模型从 trace 排除"的规则：
//! WorkBuddy 无论内置还是自定义（含走代理）都会写 trace，trace 是唯一完备记账源，
//! 因此引擎对所有 generation span 一视同仁，按 models.json 匹配判定自定义/内置。
//!
//! 持久化：DATA_DIR/trace-cache/YYYY-MM-DD.jsonl（按天分片，供重启秒级重建聚合），
//! DATA_DIR/trace-cache/_state.json（文件 mtime/size 增量去重）。缓存是派生物，
//! 可随时全量重建；用户删原始 trace 不影响已有账本。
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use serde_json::{json, Value};

const RECENT_KEEP: usize = 200;
const SCAN_TTL: f64 = 30.0;
const STATE_FILE: &str = "_state.json";
const ENGINE_MARK: &str = "widget-v1";

// ---- 数据模型（与旧 /stats 返回契约一致）----

#[derive(Default, Clone)]
struct Tot {
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    calls: i64,
}

#[derive(Default, Clone)]
struct ModelAgg {
    label: String,
    prompt: i64,
    completion: i64,
    reasoning: i64,
    cache_read: i64,
    calls: i64,
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
    duration_ms: i64,
    ok: bool,
    trace_id: String,
    span_id: String,
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
        t.calls += 1;
        match self.by_model.iter_mut().find(|(k, _)| k == &r.model) {
            Some((_, m)) => {
                m.prompt += r.prompt;
                m.completion += r.completion;
                m.reasoning += r.reasoning;
                m.cache_read += r.cache_read;
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

// ---- models.json 解析：自定义模型 id/name -> provider 显示名 ----

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

// ---- trace 解析（对齐 Python trace_reader）----

/// toolInput 可能是序列化 JSON 字符串/JSON 对象/裸文本。优先解析 JSON 取首条
/// system content；截断无法解析时只搜前 2000 字符（system 恒为首条消息，避免
/// user 消息里的 "powered by" 误匹配）。
fn powered_from_tool_input(v: &Value) -> Option<String> {
    let hay = match v {
        Value::String(s) => {
            // 可解析 JSON：取其 system content 全文再搜；否则退化搜前 2000 字符
            match serde_json::from_str::<Value>(s) {
                Ok(inner) => system_text(&inner).unwrap_or_else(|| s[..byte_floor(s, 2000)].to_string()),
                Err(_) => s[..byte_floor(s, 2000)].to_string(),
            }
        }
        other => system_text(other)?,
    };
    search_powered(&hay, usize::MAX)
}

/// 返回 <= limit 的最大 char 边界字节下标（避免切在多字节字符中间 panic）
fn byte_floor(s: &str, limit: usize) -> usize {
    let mut idx = s.len().min(limit);
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// 从 dict/list 形态中取首条 system content 纯文本
fn system_text(v: &Value) -> Option<String> {
    let mut raw: Option<String> = None;
    if let Value::Object(map) = v {
        if let Some(Value::String(c)) = map.get("content") {
            raw = Some(c.clone());
        } else if let Some(Value::Array(msgs)) = map.get("messages") {
            raw = first_system(msgs);
        }
    } else if let Value::Array(arr) = v {
        raw = first_system(arr);
    }
    raw
}

fn first_system(msgs: &[Value]) -> Option<String> {
    for m in msgs {
        if let Some(map) = m.as_object() {
            if map.get("role").and_then(|x| x.as_str()) == Some("system") {
                if let Some(Value::String(c)) = map.get("content") {
                    return Some(c.clone());
                }
            }
        }
    }
    None
}

/// 大小写不敏感搜 "powered by"，捕获模型名：以大写/数字开头，取到
/// 换行/引号/反斜杠为止，trim 后返回。限前 limit 字符防大 system prompt。
fn search_powered(s: &str, limit: usize) -> Option<String> {
    let hay = &s[..s.len().min(limit)];
    let lower = hay.to_lowercase();
    let needle = "powered by";
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find(needle) {
        let mut i = search_from + rel + needle.len();
        // 跳过后续空白
        let bytes = hay.as_bytes();
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t' || bytes[i] == b'\r' || bytes[i] == b'\n') {
            i += 1;
        }
        // 取模型名字符：字母数字 . - _ ( ) : 空格，到引号/反斜杠/换行停。
        // 模型名可能含中文（如自定义名 "GLM-5.3-Flash(B.AI测试)"），c>=0x80 覆盖 UTF-8 续字节
        let mut end = i;
        while end < bytes.len() {
            let c = bytes[end];
            if c == b'"' || c == b'\\' || c == b'\r' || c == b'\n' {
                break;
            }
            if !(c.is_ascii_alphanumeric() || c >= 0x80 || matches!(c, b'.' | b'-' | b'_' | b'(' | b')' | b':' | b' ')) {
                break;
            }
            end += 1;
        }
        let name = hay[i..end].trim().to_string();
        if name.starts_with(|c: char| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            return Some(name);
        }
        search_from = end.max(search_from + needle.len());
    }
    None
}

/// toolOutput -> (usage, model)。非 JSON / 无 usage 返回 None。
fn parse_tool_output(out: &Value) -> Option<(Value, String)> {
    let v = match out {
        Value::String(s) => serde_json::from_str::<Value>(s).ok()?,
        other => other.clone(),
    };
    let arr = match v {
        Value::Array(a) => a,
        Value::Object(_) => vec![v],
        _ => return None,
    };
    let first = arr.first()?.as_object()?;
    let usage = first.get("usage")?.clone();
    let model = first.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some((usage, model))
}

fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}

/// details 子对象取值：v = completion_tokens_details / prompt_tokens_details
fn nested(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(|x| x.as_i64()).unwrap_or(0)
}

fn to_local(iso: &str) -> String {
    if let Ok(dt) = DateTime::parse_from_rfc3339(iso) {
        let local: DateTime<Local> = dt.with_timezone(&Local);
        return local.format("%Y-%m-%d %H:%M:%S").to_string();
    }
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 从 generation span 提取一条记账记录。
/// powered 命中自定义名/ID -> 按自定义计（label=provider）；否则要求大写/数字开头
/// -> 内置。usage 两个主字段都 0 视为无用量跳过。
fn rec_from_span(sp: &Value, custom: &HashMap<String, String>) -> Option<Rec> {
    let powered = powered_from_tool_input(sp.get("toolInput")?)?.trim().to_string();
    if powered.is_empty() {
        return None;
    }
    let is_custom = custom.contains_key(&powered);
    if !is_custom && !powered.starts_with(|c: char| c.is_ascii_uppercase() || c.is_ascii_digit()) {
        return None;
    }
    let label = if is_custom {
        custom.get(&powered).cloned().unwrap_or_else(|| "自定义".into())
    } else {
        "内置".into()
    };
    let (usage, _model) = parse_tool_output(sp.get("toolOutput")?)?;
    let prompt = num(&usage, "prompt_tokens");
    let completion = num(&usage, "completion_tokens");
    if prompt == 0 && completion == 0 {
        return None;
    }
    let pdet = usage.get("prompt_tokens_details").unwrap_or(&Value::Null);
    let cdet = usage.get("completion_tokens_details").unwrap_or(&Value::Null);
    Some(Rec {
        ts: to_local(sp.get("startedAt").and_then(|x| x.as_str()).unwrap_or("")),
        model: powered,
        label,
        prompt,
        completion,
        reasoning: nested(cdet, "reasoning_tokens"),
        cache_read: nested(pdet, "cached_tokens"),
        duration_ms: sp.get("duration").and_then(|x| x.as_i64()).unwrap_or(0),
        ok: sp.get("status").and_then(|x| x.as_str()) == Some("ok"),
        trace_id: sp.get("traceId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        span_id: sp.get("spanId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    })
}

// ---- 扫描引擎 ----

pub struct Engine {
    home: PathBuf,
    cache_dir: PathBuf,
    agg: Aggregate,
    /// trace 文件绝对路径 -> [mtime, size]
    state: HashMap<String, Vec<i64>>,
    /// (traceId, spanId) 去重；懒加载自缓存
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
        let cache_dir = data_dir.join("trace-cache");
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
                if name.ends_with(".jsonl") && name.len() == 15 {
                    out.push(e.path());
                }
            }
        }
        out.sort();
        out
    }

    /// 懒加载 seen：从按天缓存扫出全部 (traceId, spanId)
    fn seen_keys(&mut self) -> &HashSet<(String, String)> {
        if self.seen.is_none() {
            let mut set = HashSet::new();
            for fp in self.cache_files() {
                let Ok(content) = fs::read_to_string(&fp) else { continue };
                for line in content.lines() {
                    if let Ok(v) = serde_json::from_str::<Value>(line) {
                        if let (Some(t), Some(s)) = (
                            v.get("traceId").and_then(|x| x.as_str()),
                            v.get("spanId").and_then(|x| x.as_str()),
                        ) {
                            set.insert((t.to_string(), s.to_string()));
                        }
                    }
                }
            }
            self.seen = Some(set);
        }
        self.seen.as_ref().unwrap()
    }

    fn save_state(&self) {
        let files: serde_json::Map<String, Value> = self
            .state
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        let payload = json!({ "engine": ENGINE_MARK, "files": files });
        if let Ok(s) = serde_json::to_string(&payload) {
            let _ = fs::write(self.cache_dir.join(STATE_FILE), s);
        }
    }

    fn load_state(&mut self) {
        let Ok(content) = fs::read_to_string(self.cache_dir.join(STATE_FILE)) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&content) else { return };
        // 仅认本引擎标记；旧 Python 版 state 忽略（首次触发全量重建）
        if v.get("engine").and_then(|x| x.as_str()) != Some(ENGINE_MARK) {
            return;
        }
        if let Some(files) = v.get("files").and_then(|x| x.as_object()) {
            for (k, val) in files {
                if let Some(arr) = val.as_array() {
                    let nums: Vec<i64> = arr.iter().filter_map(|x| x.as_i64()).collect();
                    if nums.len() == 2 {
                        self.state.insert(k.clone(), nums);
                    }
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
                    "duration_ms": r.duration_ms, "ok": r.ok,
                    "traceId": r.trace_id, "spanId": r.span_id,
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
            self.last_scan = 0.0; // 无缓存 → 首次 get_stats 触发全量建缓存
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
                let (Some(tid), Some(sid)) = (
                    v.get("traceId").and_then(|x| x.as_str()),
                    v.get("spanId").and_then(|x| x.as_str()),
                ) else { continue };
                all.push(Rec {
                    ts: v.get("ts").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                    model: v.get("model").and_then(|x| x.as_str()).unwrap_or("?").to_string(),
                    label: v.get("label").and_then(|x| x.as_str()).unwrap_or("内置").to_string(),
                    prompt: num(&v, "prompt_tokens"),
                    completion: num(&v, "completion_tokens"),
                    reasoning: num(&v, "reasoning_tokens"),
                    cache_read: num(&v, "cache_read_tokens"),
                    duration_ms: num(&v, "duration_ms"),
                    ok: v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false),
                    trace_id: tid.to_string(),
                    span_id: sid.to_string(),
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

    fn collect_trace_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let traces_root = self.home.join(".workbuddy").join("traces");
        let Ok(pid_dir) = fs::read_dir(traces_root) else { return out };
        for e in pid_dir.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let Ok(rd) = fs::read_dir(e.path()) else { continue };
            for f in rd.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                if name.starts_with("trace_") && name.ends_with(".json") {
                    out.push(f.path());
                }
            }
        }
        out
    }

    /// 扫描：增量解析变更文件；force 时清缓存全量重建。返回新增记录数。
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
        let paths = self.collect_trace_files();
        {
            let mut p = self.progress.lock().unwrap();
            p.running = true;
            p.total = paths.len();
        }
        // 复用缓存里的 (traceId,spanId) 做 upsert 去重
        let mut seen = self.seen_keys().clone();
        let mut new_recs: Vec<Rec> = Vec::new();
        let mut scanned = 0usize;
        for p in &paths {
            let Ok(meta) = fs::metadata(p) else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let key = vec![mtime, meta.len() as i64];
            let path_key = p.to_string_lossy().into_owned();
            if self.state.get(&path_key) == Some(&key) {
                continue;
            }
            scanned += 1;
            let Ok(content) = fs::read_to_string(p) else { continue };
            let Ok(data) = serde_json::from_str::<Value>(&content) else { continue };
            let Some(spans) = data.get("spans").and_then(|x| x.as_array()) else {
                self.state.insert(path_key, key);
                continue;
            };
            let mut file_new = 0usize;
            for sp in spans {
                if sp.get("type").and_then(|x| x.as_str()) != Some("generation") {
                    continue;
                }
                let (Some(tid), Some(sid)) = (
                    sp.get("traceId").and_then(|x| x.as_str()),
                    sp.get("spanId").and_then(|x| x.as_str()),
                ) else { continue };
                let dedup = (tid.to_string(), sid.to_string());
                if seen.contains(&dedup) {
                    continue;
                }
                let Some(rec) = rec_from_span(sp, &custom) else { continue };
                seen.insert(dedup);
                new_recs.push(rec);
                file_new += 1;
            }
            self.state.insert(path_key, key);
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
                    "duration_ms": r.duration_ms,
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

