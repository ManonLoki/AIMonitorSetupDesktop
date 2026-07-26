// Tauri command arguments are transport-owned values deserialized by the
// generated adapter, even when the application service only borrows them.
#![allow(clippy::needless_pass_by_value)]

use tauri::{AppHandle, State};

use crate::{
    application::monitor::{ConnectionStatus, ImageUpload, MonitorService, RemoteImage},
    domain::monitor::{
        AiProfile, AiTool, DiscoveredMonitorDevice, HookConfigLocation, HookConfigWriteResult,
        MonitorSettings,
    },
};

#[tauri::command]
pub fn get_monitor_settings(service: State<'_, MonitorService>) -> Result<MonitorSettings, String> {
    service.settings()
}

#[tauri::command]
pub fn select_monitor_device(
    service: State<'_, MonitorService>,
    device: DiscoveredMonitorDevice,
) -> Result<MonitorSettings, String> {
    service.select_device(&device)
}

#[tauri::command]
pub fn save_monitor_username(
    service: State<'_, MonitorService>,
    username: String,
) -> Result<MonitorSettings, String> {
    service.save_username(&username)
}

#[tauri::command]
pub fn save_discovery_interval(
    service: State<'_, MonitorService>,
    minutes: u64,
) -> Result<MonitorSettings, String> {
    service.save_discovery_interval(minutes)
}

#[tauri::command]
pub async fn discover_monitor_devices(
    app: AppHandle,
    service: State<'_, MonitorService>,
) -> Result<Vec<DiscoveredMonitorDevice>, String> {
    let candidates =
        tauri::async_runtime::spawn_blocking(MonitorService::discover_device_candidates)
            .await
            .map_err(|error| format!("设备发现任务失败：{error}"))??;
    let devices = service.finish_device_discovery(candidates).await?;
    service.publish_online_devices(&app, devices)
}

#[tauri::command]
pub async fn check_monitor_connection(
    service: State<'_, MonitorService>,
    base_url: Option<String>,
) -> Result<ConnectionStatus, String> {
    service.check_connection(base_url.as_deref()).await
}

#[tauri::command]
pub async fn list_remote_images(
    service: State<'_, MonitorService>,
) -> Result<Vec<RemoteImage>, String> {
    service.images().await
}

#[tauri::command]
pub async fn upload_remote_images(
    service: State<'_, MonitorService>,
    images: Vec<ImageUpload>,
) -> Result<Vec<String>, String> {
    service.upload_images(images).await
}

#[tauri::command]
pub async fn delete_remote_image(
    service: State<'_, MonitorService>,
    filename: String,
) -> Result<(), String> {
    service.delete_image(&filename).await
}

#[tauri::command]
pub fn list_ai_profiles(service: State<'_, MonitorService>) -> Result<Vec<AiProfile>, String> {
    service.profiles()
}

#[tauri::command]
pub fn list_hook_config_locations(
    service: State<'_, MonitorService>,
) -> Result<Vec<HookConfigLocation>, String> {
    service.hook_config_locations()
}

#[tauri::command]
pub fn save_hook_config_directory(
    service: State<'_, MonitorService>,
    tool: AiTool,
    directory: String,
) -> Result<HookConfigLocation, String> {
    service.save_hook_config_directory(tool, &directory)
}

#[tauri::command]
pub fn save_ai_profile(
    service: State<'_, MonitorService>,
    profile: AiProfile,
) -> Result<AiProfile, String> {
    service.save_profile(profile)
}

#[tauri::command]
pub fn write_hook_config(
    service: State<'_, MonitorService>,
    tool: AiTool,
) -> Result<HookConfigWriteResult, String> {
    service.write_hook_config(tool)
}
