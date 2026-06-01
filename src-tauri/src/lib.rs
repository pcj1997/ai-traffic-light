mod hooks;
mod sessions;

use sessions::{
    clear_sessions, delete_session, read_status, SessionLifecycleMonitor, StatusSnapshot,
};
use std::thread;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

#[tauri::command]
fn get_status() -> StatusSnapshot {
    read_status()
}

#[tauri::command]
fn install_hooks(app: AppHandle) -> Result<String, String> {
    hooks::install(&app)
}

#[tauri::command]
fn hooks_installed(app: AppHandle) -> bool {
    hooks::is_installed(&app)
}

#[tauri::command]
fn remove_session(id: String) -> Result<(), String> {
    delete_session(&id)
}

#[tauri::command]
fn clear_session_history() -> usize {
    clear_sessions()
}

#[cfg(target_os = "windows")]
fn tray_icon(_state: &str) -> Image<'static> {
    tauri::include_image!("icons/32x32.png").to_owned()
}

#[cfg(not(target_os = "windows"))]
fn tray_icon(state: &str) -> Image<'static> {
    let size = 32u32;
    let color = match state {
        "working" => [250, 204, 21, 255],
        "waiting" => [239, 68, 68, 255],
        "completed" => [34, 197, 94, 255],
        "error" => [239, 68, 68, 255],
        _ => [107, 114, 128, 255],
    };
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            if distance <= 11.5 {
                let offset = ((y * size + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
    Image::new_owned(rgba, size, size)
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_status,
            install_hooks,
            hooks_installed,
            remove_session,
            clear_session_history
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let status_item = MenuItemBuilder::with_id("status", "CodeBuddy Light：空闲")
                .enabled(false)
                .build(app)?;
            let show_item = MenuItemBuilder::with_id("show", "显示悬浮灯").build(app)?;
            let setup_item =
                MenuItemBuilder::with_id("setup", "安装 CodeBuddy Hooks").build(app)?;
            let clear_sessions_item =
                MenuItemBuilder::with_id("clear-sessions", "清除会话记录").build(app)?;
            let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
            let autostart_item = CheckMenuItemBuilder::with_id("autostart", "开机自启动")
                .checked(autostart_enabled)
                .build(app)?;
            let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = MenuBuilder::new(app)
                .item(&status_item)
                .separator()
                .item(&show_item)
                .item(&setup_item)
                .item(&clear_sessions_item)
                .item(&autostart_item)
                .separator()
                .item(&quit_item)
                .build()?;

            let menu_autostart_item = autostart_item.clone();
            let tray = TrayIconBuilder::with_id("main")
                .icon(tray_icon("idle"))
                .tooltip("CodeBuddy Light：空闲")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "show" => show_window(app),
                    "setup" => {
                        let _ = hooks::install(app);
                    }
                    "clear-sessions" => {
                        clear_sessions();
                    }
                    "autostart" => {
                        let manager = app.autolaunch();
                        let enabled = manager.is_enabled().unwrap_or(false);
                        let result = if enabled {
                            manager.disable()
                        } else {
                            manager.enable()
                        };
                        if result.is_ok() {
                            let _ = menu_autostart_item.set_checked(!enabled);
                        } else {
                            let _ = menu_autostart_item.set_checked(enabled);
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            let app_handle = app.handle().clone();
            let status_autostart_item = autostart_item.clone();
            let mut session_monitor = SessionLifecycleMonitor::default();
            thread::spawn(move || loop {
                thread::sleep(Duration::from_secs(1));
                session_monitor.poll();
                let snapshot = read_status();
                let tooltip = if snapshot.session_count > 0 {
                    format!(
                        "CodeBuddy Light：{} | {} 个会话状态",
                        snapshot.label, snapshot.session_count
                    )
                } else {
                    "CodeBuddy Light：空闲".to_string()
                };
                let _ = tray.set_tooltip(Some(&tooltip));
                let _ = tray.set_icon(Some(tray_icon(&snapshot.state)));
                let _ = status_item.set_text(tooltip);
                if let Ok(enabled) = app_handle.autolaunch().is_enabled() {
                    let _ = status_autostart_item.set_checked(enabled);
                }

                // Keep the handle alive for the application's lifetime.
                let _ = &app_handle;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running CodeBuddy Light");
}
