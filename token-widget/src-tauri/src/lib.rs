use std::io::Write;
use std::net::TcpStream;
use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

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

/// 检测 8787 是否已有代理在监听（TcpStream 连一次即知，不用额外 crate）
fn proxy_alive() -> bool {
    TcpStream::connect(("127.0.0.1", 8787)).is_ok()
}

/// 若代理未运行则拉起 token-proxy.exe。
/// 数据目录指向 %APPDATA% 下的 token-proxy，避免写安装目录（Program Files 无权限）。
fn ensure_proxy(app: &tauri::AppHandle) {
    if proxy_alive() {
        return;
    }
    // 优先资源目录（打包后）；dev 模式回退到 token-proxy/dist（CARGO_MANIFEST_DIR=src-tauri）
    let exe = app
        .path()
        .resource_dir()
        .ok()
        .map(|d| d.join("token-proxy.exe"))
        .filter(|p| p.exists())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../token-proxy/dist/token-proxy.exe")
        });
    if !exe.exists() {
        eprintln!("token-proxy.exe not found: {}", exe.display());
        return;
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::new())
        .join("token-proxy");
    let _ = Command::new(&exe)
        .env("TOKEN_PROXY_DATA_DIR", &data_dir)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW：exe 无控制台黑窗
        .spawn();
    eprintln!("spawned {} with DATA_DIR={}", exe.display(), data_dir.display());
}

/// 向本地代理发 POST /shutdown（优雅 flush 后退出），不等待响应。
/// 无论代理是 Rust 拉起的 exe 还是 dev 手动启动的 python，一律通过此 HTTP
/// 端点关闭，避免残留子进程占用端口。进程未监听时静默跳过。
fn shutdown_proxy() {
    if !proxy_alive() {
        return;
    }
    if let Ok(mut s) = TcpStream::connect(("127.0.0.1", 8787)) {
        let _ = s.write_all(
            b"POST /shutdown HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let _ = s.flush();
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    shutdown_proxy();
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            ensure_proxy(app.handle());
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
        .invoke_handler(tauri::generate_handler![quit_app])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");
    app.run(|_app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            shutdown_proxy();
        }
    });
}
