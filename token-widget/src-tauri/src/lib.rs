//! v0.2.4 入口：统计/配置/扫描全部本地化，token-proxy 降级为按需转发器。
//!
//! 架构变化（对应 0.2.4 重构）：
//! - 统计引擎（engine.rs）直读 ~/.workbuddy/traces，不再依赖 8787 端口；
//! - 配置管理（configstore.rs）直读写数据目录 config.json（与代理共用同一文件）；
//! - 代理仅在存在"走代理路由模型"（models.json url 指向 127.0.0.1:8787）或用户
//!   主动点击启动时才拉起，并以 /health 确认存活（修旧版"端口被占=假存活"）；
//! - 仅用内置模型的用户全程不启动代理：状态栏显示"仅统计模式"。
mod configstore;
mod engine;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use configstore::ConfigStore;
use engine::Engine;
use serde_json::{json, Value};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

const PROXY_PORT: u16 = 8787;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct ProxyCtl {
    spawned: bool,
}

struct AppState {
    engine: Arc<Mutex<Engine>>,
    store: ConfigStore,
    proxy: Mutex<ProxyCtl>,
}

/// 托盘「显示/隐藏」与左键单击共用：切换主窗口可见性
/// 注意：Windows 上最小化后 is_visible() 仍为 true，必须先判 is_minimized 并恢复，
/// 否则最小化后点托盘会走 hide() 分支导致窗口"消失打不开"。
fn toggle_window(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_minimized().unwrap_or(false) {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        } else if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn data_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// 代理可执行文件：优先资源目录（打包后），dev 回退 token-proxy/dist。
fn proxy_exe(app: &tauri::AppHandle) -> Option<PathBuf> {
    let res = app
        .path()
        .resource_dir()
        .ok()
        .map(|d| d.join("token-proxy.exe"))
        .filter(|p| p.exists());
    if res.is_some() {
        return res;
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../token-proxy/dist/token-proxy.exe");
    dev.exists().then_some(dev)
}

// ---- 极简 HTTP 客户端（仅本机回环，读 Content-Length）----

fn http_req(port: u16, method: &str, path: &str, body: &[u8]) -> Result<Value, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_millis(2000))).ok();
    s.set_write_timeout(Some(Duration::from_millis(2000))).ok();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
    if body.is_empty() {
        req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n");
    }
    let mut raw = req.into_bytes();
    raw.extend_from_slice(body);
    s.write_all(&raw).map_err(|e| e.to_string())?;
    s.flush().ok();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf);
    let Some(idx) = text.find("\r\n\r\n") else {
        return Err("no http header".into());
    };
    let payload = text[idx + 4..].trim();
    if payload.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(payload).map_err(|e| format!("bad json: {e}"))
}

fn proxy_health() -> bool {
    // /health 由代理进程应答才算真存活：仅端口监听（如被其它程序占用）不算
    matches!(http_req(PROXY_PORT, "GET", "/health", &[]), Ok(v) if v.get("status").and_then(|x| x.as_str()) == Some("ok"))
}

fn proxy_status_raw() -> Value {
    http_req(PROXY_PORT, "GET", "/status", &[]).unwrap_or_else(|_| json!({}))
}

fn shutdown_proxy() {
    if !proxy_health() {
        return;
    }
    let _ = http_req(PROXY_PORT, "POST", "/shutdown", &[]);
}

/// 拉起代理进程并轮询 /health（最多 ~4s）。env 同时给定数据目录与 lean 模式。
fn proxy_launch(app: &tauri::AppHandle, state: &AppState) -> Result<Value, String> {
    if proxy_health() {
        state.proxy.lock().unwrap().spawned = true;
        return Ok(json!({ "ok": true, "already_running": true }));
    }
    let Some(exe) = proxy_exe(app) else {
        return Err("token-proxy.exe not found (resources / token-proxy/dist)".into());
    };
    let dir = data_dir(app);
    let mode = if state.store.has_any_key() { "full" } else { "service" };
    let _ = Command::new(&exe)
        .env("TOKEN_PROXY_DATA_DIR", &dir)
        .env("TOKEN_PROXY_LEAN", "1")
        .env("TOKEN_PROXY_MODE", mode)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("spawn {} failed: {e}", exe.display()))?;
    // 轮询 /health：端口通但非本代理（旧版假存活）会被 /health 404 拦下
    let mut ok = false;
    for _ in 0..10 {
        thread::sleep(Duration::from_millis(400));
        if proxy_health() {
            ok = true;
            break;
        }
    }
    if !ok {
        return Err("代理启动后 /health 未就绪（端口可能被占用）".into());
    }
    state.proxy.lock().unwrap().spawned = true;
    eprintln!("[PROXY] spawned {} (mode={})", exe.display(), mode);
    Ok(json!({ "ok": true, "mode": mode }))
}

/// 需要代理（存在走代理模型）但未运行 → 自动拉起。配置/导入/路由操作后调用。
fn sync_proxy(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if state.store.proxy_needed() && !proxy_health() {
        let _ = proxy_launch(app, &state);
    }
}

// ---- 统计命令（本地引擎，不依赖代理）----

#[tauri::command]
fn get_stats(state: tauri::State<AppState>) -> Value {
    let mut e = state.engine.lock().unwrap();
    if e.lazy_scan_needed() {
        let _ = e.scan(false);
    }
    e.snapshot()
}

#[tauri::command]
fn force_scan(state: tauri::State<AppState>, force: bool) -> Result<Value, String> {
    let engine = state.engine.clone();
    thread::spawn(move || {
        let mut e = engine.lock().unwrap();
        let _ = e.scan(force);
    });
    Ok(json!({
        "ok": true,
        "started": if force { "full rebuild" } else { "incremental" },
    }))
}

#[tauri::command]
fn get_scan_progress(state: tauri::State<AppState>) -> Value {
    let e = state.engine.lock().unwrap();
    e.progress_snapshot()
}

#[tauri::command]
fn set_scan_interval(state: tauri::State<AppState>, interval_ms: u64) -> Result<Value, String> {
    let ms = interval_ms.clamp(1000, 600_000);
    let mut e = state.engine.lock().unwrap();
    e.set_scan_ttl(ms as f64 / 1000.0);
    Ok(json!({ "ok": true, "interval_ms": ms, "scan_ttl": ms as f64 / 1000.0 }))
}

// ---- 配置命令（本地文件，代理无关）----

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> Value {
    state.store.masked()
}

#[tauri::command]
fn put_config(app: tauri::AppHandle, state: tauri::State<AppState>, payload: Value) -> Result<Value, String> {
    let r = state.store.put(&payload)?;
    sync_proxy(&app);
    Ok(r)
}

#[tauri::command]
fn key_op(app: tauri::AppHandle, state: tauri::State<AppState>, body: Value) -> Result<Value, String> {
    let r = state.store.key_op(&body)?;
    sync_proxy(&app);
    Ok(r)
}

#[tauri::command]
fn import_workbuddy(app: tauri::AppHandle, state: tauri::State<AppState>) -> Value {
    let r = state.store.import_workbuddy();
    if r.get("ok").and_then(|x| x.as_bool()).unwrap_or(false) {
        sync_proxy(&app);
    }
    r
}

#[tauri::command]
fn get_route_models(state: tauri::State<AppState>) -> Value {
    json!({ "models": state.store.route_models() })
}

#[tauri::command]
fn switch_route(app: tauri::AppHandle, state: tauri::State<AppState>, name: String, route: String) -> Result<Value, String> {
    let r = state.store.switch_route(&name, &route)?;
    if route == "proxy" {
        sync_proxy(&app);
    }
    Ok(r)
}

#[tauri::command]
fn get_ledger(state: tauri::State<AppState>) -> Value {
    state.store.ledger()
}

// ---- 代理控制命令 ----

#[tauri::command]
fn proxy_start(app: tauri::AppHandle, state: tauri::State<AppState>) -> Result<Value, String> {
    proxy_launch(&app, &state).map(|r| {
        let mut v = r;
        let s = v.as_object_mut().unwrap();
        s.insert("status".into(), proxy_status_raw());
        s.insert("running".into(), json!(true));
        v
    })
}

#[tauri::command]
fn proxy_stop(state: tauri::State<AppState>) -> Result<Value, String> {
    shutdown_proxy();
    state.proxy.lock().unwrap().spawned = false;
    Ok(json!({ "ok": true, "running": false }))
}

#[tauri::command]
fn get_status(state: tauri::State<AppState>) -> Value {
    let running = proxy_health();
    let st = if running { proxy_status_raw() } else { json!({}) };
    let (channels, models) = state.store.counts();
    json!({
        "running": running,
        "needed": state.store.proxy_needed(),
        "has_key": state.store.has_any_key(),
        "mode": st.get("mode").and_then(|x| x.as_str()).unwrap_or("service"),
        "port": PROXY_PORT,
        "forwarded": st.get("forwarded").and_then(|x| x.as_u64()).unwrap_or(0),
        "uptime": st.get("uptime").and_then(|x| x.as_u64()).unwrap_or(0),
        "config_channels": channels,
        "config_models": models,
    })
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle, state: tauri::State<AppState>) {
    shutdown_proxy();
    state.proxy.lock().unwrap().spawned = false;
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let engine = Arc::new(Mutex::new(Engine::new(dir.clone())));
            // 启动先锁一次引擎：加载缓存并秒级重建聚合（新装无缓存则首次 get_stats 触发全量扫）
            {
                let mut e = engine.lock().unwrap();
                let n = e.boot();
                eprintln!("[ENGINE] boot from trace-cache: {n} records");
            }
            let store = ConfigStore::new(dir.clone());
            // 首次运行复制捆绑的空配置模板（无捆绑则写最小默认）
            let bundled = app
                .path()
                .resource_dir()
                .ok()
                .map(|d| d.join("config.json"))
                .filter(|p| p.exists())
                .or_else(|| {
                    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/config.json");
                    p.exists().then_some(p)
                });
            store.ensure_default(bundled.as_deref());
            app.manage(AppState {
                engine,
                store,
                proxy: Mutex::new(ProxyCtl { spawned: false }),
            });
            // 有走代理模型则按需拉起；纯内置用户不启动任何子进程
            sync_proxy(app.handle());

            let toggle = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &quit])?;

            // 透明无边框窗口靠托盘常驻；左键单击托盘切换窗口可见性
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "toggle" => toggle_window(app),
                    "quit" => {
                        shutdown_proxy();
                        app.exit(0)
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_stats,
            force_scan,
            get_scan_progress,
            set_scan_interval,
            get_config,
            put_config,
            key_op,
            import_workbuddy,
            get_route_models,
            switch_route,
            get_ledger,
            proxy_start,
            proxy_stop,
            get_status,
            quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|_app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            shutdown_proxy();
        }
    });
}
