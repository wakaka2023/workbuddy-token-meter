//! 本地配置库：config.json / config-ledger.json / ~/.workbuddy/models.json 的文件读写。
//!
//! v0.2.4 起配置管理不再依赖 token-proxy 进程：widget 直接读写数据目录下的
//! config.json（与按需拉起的代理共用同一文件，文件即唯一真源）。语义对齐旧
//! Python 实现（config.py / import_wb.py / route_switch.py / ledger.py），保证
//! 前端设置面板所见一致：key 脱敏回显、合并时按 (渠道,keyId) 找回真实值、
//! 空 payload 保护、模型登记表 origin_url 首见快照永不覆盖。
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde_json::{json, Map, Value};

const PROXY_HOST: &str = "127.0.0.1:8787";
const PROXY_HOST2: &str = "localhost:8787";
const PROXY_URL: &str = "http://127.0.0.1:8787/v1";
const BAK_KEEP: usize = 5;

/// 非对象 Value 的兜底空映射（obj() 返回引用，避免引用临时值）
static EMPTY_OBJ: LazyLock<Map<String, Value>> = LazyLock::new(Map::new);

fn now_ts() -> String {
    let d = chrono::Local::now();
    d.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn now_stamp() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

fn read_json(path: &Path) -> Option<Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&content).ok()
}

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    let s = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(path, s).map_err(|e| e.to_string())
}

fn obj(v: &Value) -> &Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => &EMPTY_OBJ,
    }
}

pub struct ConfigStore {
    data_dir: PathBuf,
    home: PathBuf,
    lock: Mutex<()>,
}

impl ConfigStore {
    pub fn new(data_dir: PathBuf) -> Self {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let _ = fs::create_dir_all(&data_dir);
        ConfigStore { data_dir, home, lock: Mutex::new(()) }
    }

    fn config_file(&self) -> PathBuf {
        self.data_dir.join("config.json")
    }

    fn ledger_file(&self) -> PathBuf {
        self.data_dir.join("config-ledger.json")
    }

    fn models_json(&self) -> PathBuf {
        self.home.join(".workbuddy").join("models.json")
    }

    /// 首次运行：数据目录缺 config.json 时先复制捆绑空模板；捆绑缺失则写最小默认。
    pub fn ensure_default(&self, bundled: Option<&Path>) {
        let _g = self.lock.lock().unwrap();
        if self.config_file().exists() {
            return;
        }
        if let Some(b) = bundled {
            if b.exists() {
                if let Ok(content) = fs::read(b) {
                    let _ = fs::write(self.config_file(), content);
                    return;
                }
            }
        }
        let _ = write_json(
            &self.config_file(),
            &json!({
                "_comment": "渠道/模型均为空时 widget 处于仅统计模式；如需转发自定义模型请用一键导入或在此手动添加",
                "channels": {},
                "models": {}
            }),
        );
    }

    fn load_config(&self) -> Value {
        let mut v = read_json(&self.config_file()).unwrap_or_else(|| json!({}));
        let m = v.as_object_mut().unwrap();
        m.entry("channels").or_insert_with(|| json!({}));
        m.entry("models").or_insert_with(|| json!({}));
        v
    }

    // ---- key 脱敏（与 Python _mask_key / _is_masked_key 一致）----

    fn mask_key(key: &str) -> String {
        if key.is_empty() || key.len() <= 10 {
            return key.to_string();
        }
        format!("{}...{}", &key[..6], &key[key.len() - 4..])
    }

    fn is_masked_key(key: &str) -> bool {
        key.is_empty() || key.contains("...") || key.len() < 30
    }

    fn masked_channels(channels: &Value) -> Value {
        let mut out = Map::new();
        for (cname, ch) in obj(channels) {
            let mut cp = ch.clone();
            if let Some(keys) = ch.get("keys").and_then(|k| k.as_array()) {
                let masked: Vec<Value> = keys
                    .iter()
                    .map(|k| {
                        json!({
                            "id": k.get("id"),
                            "name": k.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                            "key": Self::mask_key(k.get("key").and_then(|x| x.as_str()).unwrap_or("")),
                        })
                    })
                    .collect();
                cp.as_object_mut().unwrap().insert("keys".into(), Value::Array(masked));
            }
            out.insert(cname.clone(), cp);
        }
        Value::Object(out)
    }

    /// GET /config 等价返回：channels/models key 脱敏，保留 _comment 等顶层字段。
    pub fn masked(&self) -> Value {
        let _g = self.lock.lock().unwrap();
        let mut cfg = self.load_config();
        let m = cfg.as_object_mut().unwrap();
        let channels = m.get("channels").cloned().unwrap_or_else(|| json!({}));
        let models = m.get("models").cloned().unwrap_or_else(|| json!({}));
        m.insert("channels".into(), Self::masked_channels(&channels));
        m.insert("models".into(), models);
        cfg
    }

    pub fn counts(&self) -> (usize, usize) {
        let cfg = self.load_config();
        (obj(cfg.get("channels").unwrap_or(&Value::Null)).len(), obj(cfg.get("models").unwrap_or(&Value::Null)).len())
    }

    pub fn has_any_key(&self) -> bool {
        let cfg = self.load_config();
        let channels = cfg.get("channels").cloned().unwrap_or_else(|| json!({}));
        for ch in obj(&channels).values() {
            if let Some(keys) = ch.get("keys").and_then(|k| k.as_array()) {
                for k in keys {
                    let raw = k.get("key").and_then(|x| x.as_str()).unwrap_or("");
                    if !raw.is_empty() && !raw.contains("REPLACE") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 任一 WorkBuddy 自定义模型 url 指向本机代理 → 需要拉起代理转发。
    pub fn proxy_needed(&self) -> bool {
        let Some(v) = read_json(&self.models_json()) else { return false };
        let Some(models) = v.as_array() else { return false };
        models.iter().any(|m| {
            let url = m.get("url").and_then(|x| x.as_str()).unwrap_or("");
            url.contains(PROXY_HOST) || url.contains(PROXY_HOST2)
        })
    }

    // ---- PUT /config 等价：合并写入（脱敏 key 找回真实值）----

    fn merge_channels_keys(cur: &Value, new: &Value) -> (Value, Vec<String>) {
        let mut merged = Map::new();
        let mut dropped = Vec::new();
        for (cname, ch) in obj(new) {
            let cur_ch = obj(cur).get(cname).cloned().unwrap_or_else(|| json!({}));
            let mut cp = cur_ch.clone();
            let m = cp.as_object_mut().unwrap();
            for (k, v) in obj(ch) {
                if k != "keys" {
                    m.insert(k.clone(), v.clone());
                }
            }
            let mut cur_keys: HashMap<String, Value> = HashMap::new();
            if let Some(keys) = cur_ch.get("keys").and_then(|k| k.as_array()) {
                for k in keys {
                    if let Some(id) = k.get("id").and_then(|x| x.as_str()) {
                        cur_keys.insert(id.to_string(), k.clone());
                    }
                }
            }
            let mut new_keys = Vec::new();
            if let Some(keys) = ch.get("keys").and_then(|k| k.as_array()) {
                for k in keys {
                    let mut kk = k.clone();
                    let raw = k.get("key").and_then(|x| x.as_str()).unwrap_or("");
                    if Self::is_masked_key(raw) {
                        let real = cur_keys
                            .get(k.get("id").and_then(|x| x.as_str()).unwrap_or(""))
                            .and_then(|x| x.get("key").and_then(|y| y.as_str()))
                            .map(|s| s.to_string());
                        match real {
                            Some(r) => {
                                kk.as_object_mut().unwrap().insert("key".into(), Value::String(r));
                                new_keys.push(kk);
                            }
                            None => {
                                if let Some(id) = k.get("id").and_then(|x| x.as_str()) {
                                    dropped.push(format!("{cname}/{}", id));
                                }
                            }
                        }
                    } else {
                        new_keys.push(kk);
                    }
                }
            }
            if !new_keys.is_empty() {
                m.insert("keys".into(), Value::Array(new_keys));
            } else if let Some(keys) = cur_ch.get("keys") {
                m.insert("keys".into(), keys.clone());
            }
            merged.insert(cname.clone(), cp);
        }
        (Value::Object(merged), dropped)
    }

    fn merge_models_fields(cur: &Value, new: &Value) -> Value {
        let mut merged = Map::new();
        for (mid, mv) in obj(new) {
            let mut cp = obj(cur).get(mid).cloned().unwrap_or_else(|| json!({}));
            let m = cp.as_object_mut().unwrap();
            for (k, v) in obj(mv) {
                m.insert(k.clone(), v.clone());
            }
            merged.insert(mid.clone(), cp);
        }
        Value::Object(merged)
    }

    pub fn put(&self, payload: &Value) -> Result<Value, String> {
        let _g = self.lock.lock().unwrap();
        let mut cfg = self.load_config();
        let new_channels = payload.get("channels");
        let new_models = payload.get("models");
        let has_c = new_channels.map(|c| obj(c).len() > 0).unwrap_or(false);
        let has_m = new_models.map(|m| obj(m).len() > 0).unwrap_or(false);
        let cur_c = cfg.get("channels").cloned().unwrap_or_else(|| json!({}));
        let cur_m = cfg.get("models").cloned().unwrap_or_else(|| json!({}));
        if !has_c && !has_m && (obj(&cur_c).len() > 0 || obj(&cur_m).len() > 0) {
            return Err("refusing to overwrite non-empty config with empty payload".into());
        }
        if let Some(c) = new_channels {
            let (merged, dropped) = Self::merge_channels_keys(&cur_c, c);
            if !dropped.is_empty() {
                eprintln!("[CFG] dropped masked keys with no real source: {}", dropped.join(", "));
            }
            cfg.as_object_mut().unwrap().insert("channels".into(), merged);
        }
        if let Some(m) = new_models {
            cfg.as_object_mut().unwrap().insert("models".into(), Self::merge_models_fields(&cur_m, m));
        }
        write_json(&self.config_file(), &cfg)?;
        Ok(json!({
            "ok": true,
            "channels": obj(cfg.get("channels").unwrap_or(&Value::Null)).len(),
            "models": obj(cfg.get("models").unwrap_or(&Value::Null)).len(),
        }))
    }

    // ---- POST /keys 等价 ----

    pub fn key_op(&self, req: &Value) -> Result<Value, String> {
        let _g = self.lock.lock().unwrap();
        let channel = req.get("channel").and_then(|x| x.as_str()).unwrap_or("");
        let action = req.get("action").and_then(|x| x.as_str()).unwrap_or("");
        if channel.is_empty() || !matches!(action, "set" | "add" | "del") {
            return Err("channel & action(set|add|del) required".into());
        }
        let mut cfg = self.load_config();
        let mut channels = cfg.get("channels").cloned().unwrap_or_else(|| json!({}));
        let mut ch = obj(&channels)
            .get(channel)
            .cloned()
            .ok_or_else(|| format!("channel '{channel}' not found in config"))?;
        let mut keys = ch
            .get("keys")
            .and_then(|k| k.as_array())
            .cloned()
            .unwrap_or_default();
        let key_id = req.get("keyId").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mut out_id = key_id.clone();
        match action {
            "set" => {
                if !keys.iter().any(|k| k.get("id").and_then(|x| x.as_str()) == Some(key_id.as_str())) {
                    return Err(format!("keyId '{key_id}' not found"));
                }
                ch.as_object_mut()
                    .unwrap()
                    .insert("activeKey".into(), Value::String(key_id));
            }
            "add" => {
                let raw = req.get("key").and_then(|x| x.as_str()).unwrap_or("");
                if raw.is_empty() {
                    return Err("key required".into());
                }
                let new_id = format!("k{}", keys.len() + 1);
                keys.push(json!({
                    "id": new_id,
                    "name": req.get("name").and_then(|x| x.as_str()).unwrap_or(new_id.as_str()),
                    "key": raw,
                }));
                out_id = new_id;
                if !ch.get("activeKey").and_then(|x| x.as_str()).map(|s| !s.is_empty()).unwrap_or(false) {
                    ch.as_object_mut().unwrap().insert("activeKey".into(), Value::String(out_id.clone()));
                }
            }
            "del" => {
                if !keys.iter().any(|k| k.get("id").and_then(|x| x.as_str()) == Some(key_id.as_str())) {
                    return Err(format!("keyId '{key_id}' not found"));
                }
                keys.retain(|k| k.get("id").and_then(|x| x.as_str()) != Some(key_id.as_str()));
                let active = ch.get("activeKey").and_then(|x| x.as_str()).unwrap_or("");
                if active == key_id {
                    let next = keys.first().and_then(|k| k.get("id").and_then(|x| x.as_str())).unwrap_or("");
                    ch.as_object_mut().unwrap().insert("activeKey".into(), Value::String(next.into()));
                }
            }
            _ => unreachable!(),
        }
        ch.as_object_mut().unwrap().insert("keys".into(), Value::Array(keys));
        channels.as_object_mut().unwrap().insert(channel.into(), ch.clone());
        cfg.as_object_mut().unwrap().insert("channels".into(), channels);
        write_json(&self.config_file(), &cfg)?;
        let active = ch.get("activeKey").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let masked = Self::masked_channels(&json!({channel: ch}))
            .get(channel)
            .and_then(|x| x.get("keys"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        self.ledger_log("keys_change", &format!("{channel} {action} {out_id}"));
        Ok(json!({
            "ok": true,
            "channel": channel,
            "action": action,
            "keyId": out_id,
            "activeKey": active,
            "keys": masked,
        }))
    }

    // ---- config-ledger.json（模型登记表 + 操作日志）----

    fn load_ledger(&self) -> Value {
        let mut v = read_json(&self.ledger_file()).unwrap_or_else(|| json!({}));
        let m = v.as_object_mut().unwrap();
        m.entry("models").or_insert_with(|| json!([]));
        m.entry("changelog").or_insert_with(|| json!([]));
        v
    }

    fn save_ledger(&self, v: &Value) {
        let _ = write_json(&self.ledger_file(), v);
    }

    pub fn ledger_models(&self) -> Value {
        self.load_ledger().get("models").cloned().unwrap_or_else(|| json!([]))
    }

    pub fn ledger_changelog(&self) -> Value {
        let v = self.load_ledger();
        let logs = v.get("changelog").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        let n = logs.len().min(50);
        Value::Array(logs[logs.len() - n..].to_vec())
    }

    pub fn ledger_summary(&self) -> Value {
        let v = self.load_ledger();
        let models = v.get("models").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
        let logs = v.get("changelog").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
        json!({ "models": models, "logs": logs })
    }

    pub fn ledger(&self) -> Value {
        json!({
            "models": self.ledger_models(),
            "changelog": self.ledger_changelog(),
            "summary": self.ledger_summary(),
        })
    }

    fn ledger_log(&self, op: &str, detail: &str) {
        let mut v = self.load_ledger();
        let logs = v.get_mut("changelog").unwrap();
        let arr = logs.as_array_mut().unwrap();
        arr.push(json!({ "ts": now_ts(), "op": op, "detail": detail }));
        if arr.len() > 200 {
            arr.drain(..arr.len() - 200);
        }
        self.save_ledger(&v);
    }

    fn ledger_upsert(&self, name: &str, channel: Option<&str>, origin_url: Option<&str>, current_url: &str, route: &str, key_ref: Option<&str>) {
        let mut v = self.load_ledger();
        let models = v.get_mut("models").unwrap();
        let arr = models.as_array_mut().unwrap();
        let mut found = false;
        for m in arr.iter_mut() {
            if m.get("name").and_then(|x| x.as_str()) == Some(name) {
                let mm = m.as_object_mut().unwrap();
                mm.insert("current_url".into(), Value::String(current_url.into()));
                mm.insert("route".into(), Value::String(route.into()));
                if let Some(kr) = key_ref {
                    mm.insert("key_ref".into(), Value::String(kr.into()));
                }
                if mm.get("origin_url").and_then(|x| x.as_str()).map(|s| s.is_empty()).unwrap_or(true) {
                    if let Some(ou) = origin_url {
                        mm.insert("origin_url".into(), Value::String(ou.into()));
                    }
                }
                mm.insert("updated_at".into(), Value::String(now_ts()));
                found = true;
                break;
            }
        }
        if !found {
            arr.push(json!({
                "name": name,
                "channel": channel.unwrap_or(""),
                "origin_url": origin_url.unwrap_or(current_url),
                "current_url": current_url,
                "route": route,
                "key_ref": key_ref.unwrap_or(""),
                "updated_at": now_ts(),
            }));
        }
        self.save_ledger(&v);
    }

    // ---- models.json 读取（WorkBuddy 活配置）----

    fn read_wb_models(&self) -> Option<Value> {
        let v = read_json(&self.models_json())?;
        if v.is_array() { Some(v) } else { None }
    }

    /// 模型路由概览（模型管理面板用；key 存在性提示）。
    pub fn route_models(&self) -> Value {
        let Some(models) = self.read_wb_models() else { return json!([]) };
        let reg = self.ledger_models();
        let mut out = Vec::new();
        for m in models.as_array().unwrap() {
            let name = m.get("name").and_then(|x| x.as_str()).map(|s| s.trim()).filter(|s| !s.is_empty())
                .or_else(|| m.get("id").and_then(|x| x.as_str()).map(|s| s.trim()))
                .unwrap_or("")
                .to_string();
            let url = m.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }
            let route = if url.contains(PROXY_HOST) || url.contains(PROXY_HOST2) { "proxy" } else { "direct" };
            let ak = m.get("apiKey").and_then(|x| x.as_str()).unwrap_or("");
            let origin = reg.as_array().unwrap().iter()
                .find(|r| r.get("name").and_then(|x| x.as_str()) == Some(name.as_str()))
                .and_then(|r| r.get("origin_url").and_then(|x| x.as_str()))
                .map(|s| s.to_string());
            out.push(json!({
                "id": m.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                "name": name,
                "url": url,
                "route": route,
                "has_key": !ak.is_empty(),
                "origin_url": origin.unwrap_or_default(),
            }));
        }
        Value::Array(out)
    }

    fn backup_models(&self) {
        let target = self.models_json();
        if !target.exists() {
            return;
        }
        let bak = PathBuf::from(format!("{}.bak-{}", target.to_string_lossy(), now_stamp()));
        let _ = fs::copy(&target, &bak);
        // 保留最近 BAK_KEEP 份
        if let Some(parent) = target.parent() {
            if let Ok(rd) = fs::read_dir(parent) {
                let mut baks: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("models.json.bak-")).unwrap_or(false)
                    })
                    .collect();
                baks.sort();
                while baks.len() > BAK_KEEP {
                    let _ = fs::remove_file(baks.remove(0));
                }
            }
        }
    }

    /// 路由切换（direct ↔ proxy）。proxy 需 WorkBuddy 重启生效，由 message 提示。
    pub fn switch_route(&self, name: &str, route: &str) -> Result<Value, String> {
        let _g = self.lock.lock().unwrap();
        if !matches!(route, "proxy" | "direct") {
            return Err("target must be 'proxy' or 'direct'".into());
        }
        let mut models = self.read_wb_models().ok_or_else(|| "cannot read WorkBuddy models.json".to_string())?;
        let arr = models.as_array_mut().unwrap();
        let target = arr
            .iter_mut()
            .find(|m| {
                m.get("name").and_then(|x| x.as_str()) == Some(name)
                    || m.get("id").and_then(|x| x.as_str()) == Some(name)
            })
            .ok_or_else(|| format!("model '{name}' not found in WorkBuddy models"))?;
        let url = target.get("url").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let is_proxy = url.contains(PROXY_HOST) || url.contains(PROXY_HOST2);
        let new_url = match (route, is_proxy) {
            ("proxy", false) => {
                self.backup_models();
                PROXY_URL.to_string()
            }
            ("direct", true) => {
                let origin = self.ledger_models().as_array().unwrap().iter()
                    .find(|r| r.get("name").and_then(|x| x.as_str()) == Some(name))
                    .and_then(|r| r.get("origin_url").and_then(|x| x.as_str()))
                    .map(|s| s.to_string());
                let origin = origin.ok_or_else(|| format!("no origin_url recorded for '{name}' (import first)"))?;
                self.backup_models();
                origin
            }
            _ => {
                return Ok(json!({ "ok": true, "message": format!("already {route}, no change"), "url": url }));
            }
        };
        target.as_object_mut().unwrap().insert("url".into(), Value::String(new_url.clone()));
        write_json(&self.models_json(), &models).map_err(|e| format!("write failed: {e}"))?;
        self.ledger_upsert(name, None, None, &new_url, route, None);
        self.ledger_log("route_change", &format!("{name} -> {route}"));
        Ok(json!({
            "ok": true,
            "message": format!("switched to {route}, restart WorkBuddy to take effect"),
            "url": new_url,
        }))
    }

    // ---- 一键导入 WorkBuddy 自定义模型（对齐 import_wb.py）----

    fn channel_from_url(url: &str) -> Option<(String, String)> {
        let idx = url.find("://")?;
        let scheme = &url[..idx];
        let rest = &url[idx + 3..];
        let host_port = rest.split('/').next().unwrap_or("").to_string();
        let host = host_port.split(':').next().unwrap_or("").to_lowercase();
        let last2 = {
            let mut v: Vec<&str> = host.split('.').collect();
            if v.len() >= 2 {
                v.split_off(v.len() - 2).join(".")
            } else {
                host.clone()
            }
        };
        if last2.is_empty() {
            return None;
        }
        Some((last2, format!("{scheme}://{host_port}")))
    }

    pub fn import_workbuddy(&self) -> Value {
        let _g = self.lock.lock().unwrap();
        let Some(models) = self.read_wb_models() else {
            return json!({ "ok": false, "error": "cannot read ~/.workbuddy/models.json" });
        };
        let arr = models.as_array().unwrap();
        if arr.is_empty() {
            return json!({ "ok": true, "imported": 0, "skipped": 0, "channels": {}, "message": "WorkBuddy 中未配置自定义模型" });
        }
        let mut cfg = self.load_config();
        let mut imported = 0usize;
        let mut skipped = 0usize;
        let mut ch_summary: Map<String, Value> = Map::new();
        for m in arr {
            let url = m.get("url").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            let mid = m.get("id").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            let mname = m.get("name").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            let mname = if mname.is_empty() { mid.clone() } else { mname };
            let raw_key = m.get("apiKey").and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
            if url.is_empty() || mname.is_empty() {
                continue;
            }
            if url.contains(PROXY_HOST) || url.contains(PROXY_HOST2) {
                skipped += 1;
                continue;
            }
            let Some((ch_name, ch_base)) = Self::channel_from_url(&url) else { continue };
            let cfg_m = cfg.as_object_mut().unwrap();
            let channels = cfg_m.entry("channels".to_string()).or_insert_with(|| json!({}));
            let ch = channels.as_object_mut().unwrap().entry(ch_name.clone()).or_insert_with(|| json!({}));
            let cm = ch.as_object_mut().unwrap();
            cm.entry("base".to_string()).or_insert_with(|| Value::String(ch_base.clone()));
            cm.entry("label".to_string()).or_insert_with(|| Value::String(ch_name.clone()));
            cm.entry("proxy".to_string()).or_insert_with(|| Value::String("auto".into()));
            let keys = cm.entry("keys".to_string()).or_insert_with(|| json!([])).as_array_mut().unwrap().clone();
            let mut key_id: Option<String> = None;
            if !raw_key.is_empty() {
                let found = keys.iter().find(|k| k.get("key").and_then(|x| x.as_str()) == Some(raw_key.as_str())).cloned();
                match found {
                    Some(k) => key_id = k.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()),
                    None => {
                        let new_id = format!("k{}", keys.len() + 1);
                        cm.entry("keys".to_string()).or_insert_with(|| json!([])).as_array_mut().unwrap().push(json!({
                            "id": new_id,
                            "name": ch_name,
                            "key": raw_key,
                        }));
                        if !cm.contains_key("activeKey") {
                            cm.insert("activeKey".into(), Value::String(new_id.clone()));
                        }
                        key_id = Some(new_id);
                    }
                }
            }
            let models_map = cfg_m.entry("models".to_string()).or_insert_with(|| json!({}));
            let row = models_map.as_object_mut().unwrap().entry(mid.clone()).or_insert_with(|| json!({}));
            let rm = row.as_object_mut().unwrap();
            rm.insert("channel".into(), Value::String(ch_name.clone()));
            rm.insert("name".into(), Value::String(mname.clone()));
            if let Some(kid) = &key_id {
                if rm.get("key").and_then(|x| x.as_str()) != Some(kid.as_str()) {
                    rm.insert("key".into(), Value::String(kid.clone()));
                }
            }
            rm.entry("price".to_string()).or_insert_with(|| json!({}));
            // 台账
            let origin = url.clone();
            let key_ref = key_id.map(|kid| format!("{ch_name}/{kid}"));
            self.ledger_upsert(&mname, Some(&ch_name), Some(&origin), &url, "direct", key_ref.as_deref());
            imported += 1;
            *ch_summary.entry(ch_name).or_insert_with(|| json!(0)) =
                json!(ch_summary.get(&ch_name).and_then(|x| x.as_u64()).unwrap_or(0) + 1);
        }
        match write_json(&self.config_file(), &cfg) {
            Ok(()) => {
                self.ledger_log("import", &format!("imported {imported}, skipped {skipped}, channels {}", ch_summary.len()));
                json!({ "ok": true, "imported": imported, "skipped": skipped, "channels": Value::Object(ch_summary) })
            }
            Err(e) => json!({ "ok": false, "error": e }),
        }
    }
}

