mod application;
mod commands;
mod domain;

use application::monitor::MonitorService;
use tauri::Manager;

/// Starts the native Tauri application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application runtime.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let config_home = app.path().home_dir()?;
            let service =
                MonitorService::load(&app_data_dir, &config_home).map_err(std::io::Error::other)?;
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::system::get_system_overview,
            commands::monitor::get_monitor_settings,
            commands::monitor::save_monitor_settings,
            commands::monitor::discover_monitor_devices,
            commands::monitor::check_monitor_connection,
            commands::monitor::list_remote_images,
            commands::monitor::upload_remote_images,
            commands::monitor::delete_remote_image,
            commands::monitor::list_ai_profiles,
            commands::monitor::list_hook_config_locations,
            commands::monitor::save_hook_config_directory,
            commands::monitor::write_ai_profile,
            commands::monitor::preview_hook_config,
            commands::monitor::list_local_hook_configs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
