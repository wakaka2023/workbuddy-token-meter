//! v0.2.4 入口：统计/配置/扫描全部本地化，token-proxy 降级为按需转发器。
//!
//! 架构变化（对应 0.2.4 重构）：
//! - 统计引擎（engine.rs）直读 ~/.workbuddy/projects 下的会话 jsonl，不再依赖 8787 端口；
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
use std::time::{Duration, Instant};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use configstore::ConfigStore;
use engine::{Engine, Progress, Snapshot};
use serde_json::{json, Value};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

const PROXY_PORT: u16 = 8787;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// 代理拉起失败后的最小重试间隔：sync_proxy 高频调用（配置/路由/导入后都触发）时，
/// 若 /health 未就绪就会反复 spawn 新进程堆积；冷却期内不再重复拉起。
const PROXY_RETRY_COOLDOWN: Duration = Duration::from_secs(10);

struct ProxyCtl {
    spawned: bool,
    last_attempt: Option<Instant>,
}

struct AppState {
    /// 扫描引擎（含可变扫描状态），仅后台扫描线程与低频设置命令触碰
    engine: Arc<Mutex<Engine>>,
    /// 聚合快照读端：get_stats 只 clone Arc，永不与扫描抢 Engine 大锁
    snap: Arc<Snapshot>,
    /// 扫描进度读端：独立于 Engine 大锁，扫描期间实时可读
    progress: Arc<Mutex<Progress>>,
    store: Arc<ConfigStore>,
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
    http_req_timeout(port, method, path, body, Duration::from_millis(2000))
}

fn http_req_timeout(
    port: u16,
    method: &str,
    path: &str,
    body: &[u8],
    timeout: Duration,
) -> Result<Value, String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).ok();
    s.set_write_timeout(Some(timeout)).ok();
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    if body.is_empty() {
        req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
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

/// 退出路径专用短超时：关代理不该让窗口多停几秒等回包
const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(300);
/// quit_app 与 RunEvent::Exit 都会调 shutdown_proxy，用它保证只真正执行一次
static PROXY_SHUTDOWN_DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn shutdown_proxy() {
    if PROXY_SHUTDOWN_DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    if !matches!(
        http_req_timeout(PROXY_PORT, "GET", "/health", &[], SHUTDOWN_TIMEOUT),
        Ok(v) if v.get("status").and_then(|x| x.as_str()) == Some("ok")
    ) {
        return;
    }
    let _ = http_req_timeout(PROXY_PORT, "POST", "/shutdown", &[], SHUTDOWN_TIMEOUT);
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
    // spawn 前先登记尝试时间；轮询失败时 sync_proxy 依据它进入冷却，不再连环拉起
    state.proxy.lock().unwrap().last_attempt = Some(Instant::now());
    let _ = Command::new(&exe)
        .env("TOKEN_PROXY_DATA_DIR", &dir)
        .env("TOKEN_PROXY_LEAN", "1")
        .env("TOKEN_PROXY_MODE", mode)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("spawn {} failed: {e}", exe.display()))?;
    // 轮询 /health：端口通但非本代理（旧版假存活）会被 /health 404 拦下。
    // PyInstaller onefile 冷启动需解压临时目录，给足 10s（20×500ms）再判失败。
    let mut ok = false;
    for _ in 0..20 {
        thread::sleep(Duration::from_millis(500));
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

/// 需要代理（存在走代理模型）但未运行 → 后台拉起。配置/导入/路由操作后调用，
/// 不阻塞命令线程（避免保存/切路由时卡 UI）。
fn sync_proxy(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    if !state.store.proxy_needed() || proxy_health() {
        return;
    }
    let mut ctl = state.proxy.lock().unwrap();
    // 距上次尝试不足冷却期则跳过：防止一次失败被后续多次 sync_proxy 连环触发堆积进程
    if ctl
        .last_attempt
        .is_some_and(|t| t.elapsed() < PROXY_RETRY_COOLDOWN)
    {
        return;
    }
    ctl.last_attempt = Some(Instant::now());
    drop(ctl);
    let app2 = app.clone();
    thread::spawn(move || {
        let state = app2.state::<AppState>();
        let _ = proxy_launch(&app2, &state);
    });
}

// ---- 统计命令（本地引擎，不依赖代理）----

#[tauri::command]
async fn get_stats(
    state: tauri::State<'_, AppState>,
    prev_gen: Option<u64>,
) -> Result<Value, String> {
    let agg = state.snap.get();
    // gen 未变说明数据没动：回几十字节占位，省掉全量 JSON 的序列化、IPC 传输与前端 parse
    if prev_gen == Some(agg.gen()) {
        return Ok(json!({ "gen": agg.gen(), "data": Value::Null }));
    }
    tauri::async_runtime::spawn_blocking(move || {
        Ok::<Value, String>(json!({ "gen": agg.gen(), "data": agg.to_json() }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn force_scan(state: tauri::State<AppState>, force: bool) -> Result<Value, String> {
    let engine = state.engine.clone();
    thread::spawn(move || {
        // 已有扫描持锁中则本轮放弃（互斥 + 防抖：连点/轮询不会叠出多轮并发扫描）
        let Ok(mut e) = engine.try_lock() else { return };
        let _ = e.scan(force);
    });
    Ok(json!({
        "ok": true,
        "started": if force { "full rebuild" } else { "incremental" },
    }))
}

#[tauri::command]
async fn get_scan_progress(state: tauri::State<'_, AppState>) -> Result<Value, String> {
    // 进度锁独立于 Engine 大锁：扫描期间也能实时读到真实进度，不再返回猜测值
    let p = *state.progress.lock().unwrap();
    Ok(json!({
        "running": p.running,
        "total": p.total,
        "scanned": p.scanned,
        "records": p.records,
        "done": p.done,
    }))
}

#[tauri::command]
async fn set_scan_interval(state: tauri::State<'_, AppState>, interval_ms: u64) -> Result<Value, String> {
    let ms = interval_ms.clamp(1000, 600_000);
    state.engine.lock().unwrap().set_scan_ttl(ms as f64 / 1000.0);
    Ok(json!({ "ok": true, "interval_ms": ms, "scan_ttl": ms as f64 / 1000.0 }))
}

#[tauri::command]
async fn set_auto_scan(state: tauri::State<'_, AppState>, enabled: bool) -> Result<Value, String> {
    state.engine.lock().unwrap().set_auto_scan(enabled);
    Ok(json!({ "ok": true, "auto_scan": enabled }))
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
fn proxy_start(app: tauri::AppHandle) -> Result<Value, String> {
    // 后台线程拉起并轮询 health，命令立即返回——避免启动慢时同步阻塞 10s 卡死 UI。
    // 启动结果由前端轮询 get_status（running 字段）感知。
    let app2 = app.clone();
    thread::spawn(move || {
        let state = app2.state::<AppState>();
        let _ = proxy_launch(&app2, &state);
    });
    Ok(json!({ "ok": true, "starting": true }))
}

#[tauri::command]
async fn proxy_stop(state: tauri::State<'_, AppState>) -> Result<Value, String> {
    state.proxy.lock().unwrap().spawned = false;
    // shutdown 是网络请求，可能等到超时，放阻塞线程池
    tauri::async_runtime::spawn_blocking(shutdown_proxy)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "ok": true, "running": false }))
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, AppState>) -> Result<Value, String> {
    // 网络探测与磁盘读（health/counts/needed）整体挪进阻塞线程池，不占 UI 主线程
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let running = proxy_health();
        let st = if running { proxy_status_raw() } else { json!({}) };
        let (channels, models) = store.counts();
        Ok::<Value, String>(json!({
            "running": running,
            "needed": store.proxy_needed(),
            "has_key": store.has_any_key(),
            "mode": st.get("mode").and_then(|x| x.as_str()).unwrap_or("service"),
            "port": PROXY_PORT,
            "forwarded": st.get("forwarded").and_then(|x| x.as_u64()).unwrap_or(0),
            "uptime": st.get("uptime").and_then(|x| x.as_u64()).unwrap_or(0),
            "config_channels": channels,
            "config_models": models,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn quit_app(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // 先隐藏窗口：视觉上立即消失。清理与 exit 交给后台线程，
    // 避免同步等代理回包时窗口还停在屏幕上（点击→消失的体感延迟）。
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
    state.proxy.lock().unwrap().spawned = false;
    let handle = app.clone();
    thread::spawn(move || {
        shutdown_proxy();
        handle.exit(0);
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        // 单实例：第二实例启动即退出，并把「再次启动」信号转给主实例（用户再点图标=唤出窗口）
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.unminimize();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let snap = Arc::new(Snapshot::new());
            let engine = Arc::new(Mutex::new(Engine::new(dir.clone(), snap.clone())));
            // 启动先锁一次引擎：加载缓存并秒级重建聚合，结果整体换入快照
            let progress = {
                let mut e = engine.lock().unwrap();
                let n = e.boot();
                eprintln!("[ENGINE] boot from jsonl cache: {n} records");
                e.progress_handle()
            };
            let store = Arc::new(ConfigStore::new(dir.clone()));
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
                engine: engine.clone(),
                snap,
                progress,
                store,
                proxy: Mutex::new(ProxyCtl { spawned: false, last_attempt: None }),
            });
            // 自动扫描守护线程：auto_scan 开启时按 scan_ttl 周期静默增量扫描。
            // 扫描独立于 UI 主线程与读路径；扫到新记录才 emit scan-done，前端据此刷新。
            let daemon = engine.clone();
            let daemon_app = app.handle().clone();
            thread::spawn(move || loop {
                thread::sleep(Duration::from_millis(500));
                let mut e = match daemon.try_lock() {
                    Ok(g) => g,
                    Err(_) => continue, // 上一轮扫描尚未结束，下一 tick 再试
                };
                if e.auto_enabled() && e.lazy_scan_needed() {
                    let n = e.scan(false);
                    if n > 0 {
                        let _ = daemon_app.emit("scan-done", json!({ "records": n }));
                    }
                }
            });
            // 不再自动拉起代理——首次打开什么都不做，用户需要时手动点"启动代理"

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
            set_auto_scan,
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
