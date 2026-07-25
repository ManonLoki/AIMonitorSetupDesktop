mod application;
mod commands;
mod domain;

use application::monitor::MonitorService;
use application::runtime::{handle_window_event, setup_desktop_runtime, show_main_window};
use tauri::Manager;

/// Starts the native Tauri application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application runtime.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
        .plugin({
            let autostart = tauri_plugin_autostart::Builder::new().app_name("AIMonitor");
            #[cfg(target_os = "macos")]
            let autostart =
                autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
            autostart.arg("--silent").build()
        })
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            setup_desktop_runtime(app)?;
            #[cfg(target_os = "macos")]
            {
                use tauri_plugin_autostart::ManagerExt;

                let autostart = app.autolaunch();
                if autostart.is_enabled().unwrap_or(false) {
                    autostart.enable().map_err(std::io::Error::other)?;
                }
            }
            let app_data_dir = app.path().app_data_dir()?;
            let config_home = app.path().home_dir()?;
            let service =
                MonitorService::load(&app_data_dir, &config_home).map_err(std::io::Error::other)?;
            service.start_legacy_hook_cleanup();
            service.start_hook_listener();
            service.start_background_device_discovery(app.handle().clone());
            app.manage(service);
            let silent_start = std::env::args_os().any(|argument| argument == "--silent");
            if !silent_start {
                show_main_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(handle_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_overview,
            commands::monitor::get_monitor_settings,
            commands::monitor::select_monitor_device,
            commands::monitor::save_monitor_username,
            commands::monitor::discover_monitor_devices,
            commands::monitor::check_monitor_connection,
            commands::monitor::list_remote_images,
            commands::monitor::upload_remote_images,
            commands::monitor::delete_remote_image,
            commands::monitor::list_ai_profiles,
            commands::monitor::list_hook_config_locations,
            commands::monitor::save_hook_config_directory,
            commands::monitor::save_ai_profile,
            commands::monitor::write_hook_config,
            commands::monitor::list_local_hook_configs,
            commands::runtime::get_runtime_overview,
            commands::runtime::update_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
