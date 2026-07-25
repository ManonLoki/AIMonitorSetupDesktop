use serde::Serialize;
use tauri::{
    App, AppHandle, Manager, Runtime, Window, WindowEvent,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
};
use tauri_plugin_autostart::ManagerExt;

use super::monitor::{HookRelayStatus, MonitorService};

const TRAY_ID: &str = "aimonitor-tray";
const TOGGLE_WINDOW_MENU_ID: &str = "toggle-window";
const AUTOSTART_MENU_ID: &str = "autostart";
const QUIT_MENU_ID: &str = "quit";

pub struct RuntimeMenuState<R: Runtime> {
    toggle_window: MenuItem<R>,
    autostart: CheckMenuItem<R>,
    _tray: TrayIcon<R>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOverview {
    pub autostart_enabled: bool,
    pub silent_start_supported: bool,
    pub hook_relay: HookRelayStatus,
}

pub fn runtime_overview(
    app: &AppHandle,
    monitor: &MonitorService,
) -> Result<RuntimeOverview, String> {
    Ok(RuntimeOverview {
        autostart_enabled: app
            .autolaunch()
            .is_enabled()
            .map_err(|error| format!("无法读取开机自启状态：{error}"))?,
        silent_start_supported: true,
        hook_relay: monitor.hook_relay_status()?,
    })
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let result = if enabled {
        manager
            .enable()
            .map_err(|error| format!("无法启用开机自启：{error}"))
    } else {
        manager
            .disable()
            .map_err(|error| format!("无法关闭开机自启：{error}"))
    };
    if result.is_ok() {
        sync_tray_autostart(app, enabled);
    }
    result
}

pub fn setup_desktop_runtime(app: &mut App) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    let toggle_window =
        MenuItem::with_id(app, TOGGLE_WINDOW_MENU_ID, "显示窗口", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        AUTOSTART_MENU_ID,
        "开机自启",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_window, &autostart, &separator, &quit])?;

    let toggle_for_events = toggle_window.clone();
    let autostart_for_events = autostart.clone();
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("AIMonitor")
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            TOGGLE_WINDOW_MENU_ID => toggle_main_window(app, &toggle_for_events),
            AUTOSTART_MENU_ID => {
                let enabled = autostart_for_events.is_checked().unwrap_or(false);
                if set_autostart(app, enabled).is_err() {
                    let _ = autostart_for_events.set_checked(!enabled);
                }
            }
            QUIT_MENU_ID => app.exit(0),
            _ => {}
        });
    let tray = match app.default_window_icon().cloned() {
        Some(icon) => tray.icon(icon).build(app)?,
        None => tray.build(app)?,
    };

    app.manage(RuntimeMenuState {
        toggle_window,
        autostart,
        _tray: tray,
    });
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        sync_window_menu_label(app, true);
    }
}

pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    if let WindowEvent::CloseRequested { api, .. } = event {
        api.prevent_close();
        let _ = window.hide();
        sync_window_menu_label(window.app_handle(), false);
    }
}

fn toggle_main_window<R: Runtime>(app: &AppHandle<R>, item: &MenuItem<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        let _ = item.set_text("显示窗口");
    } else {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = item.set_text("隐藏窗口");
    }
}

fn sync_window_menu_label(app: &AppHandle, visible: bool) {
    if let Some(state) = app.try_state::<RuntimeMenuState<tauri::Wry>>() {
        let _ = state.toggle_window.set_text(if visible {
            "隐藏窗口"
        } else {
            "显示窗口"
        });
    }
}

fn sync_tray_autostart(app: &AppHandle, enabled: bool) {
    if let Some(state) = app.try_state::<RuntimeMenuState<tauri::Wry>>() {
        let _ = state.autostart.set_checked(enabled);
    }
}
