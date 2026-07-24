// Tauri command arguments are transport-owned values deserialized by the
// generated adapter, even when the application service only borrows them.
#![allow(clippy::needless_pass_by_value)]

use tauri::State;

use crate::{
    application::monitor::{ConnectionStatus, ImageUpload, MonitorService, RemoteImage},
    domain::monitor::{
        AiProfile, DiscoveredMonitorDevice, HookConfigPreview, HookConfigWriteResult,
        LocalHookConfig, MonitorSettings,
    },
};

#[tauri::command]
pub fn get_monitor_settings(service: State<'_, MonitorService>) -> Result<MonitorSettings, String> {
    service.settings()
}

#[tauri::command]
pub fn save_monitor_settings(
    service: State<'_, MonitorService>,
    device: DiscoveredMonitorDevice,
    username: String,
) -> Result<MonitorSettings, String> {
    service.save_settings(&device, &username)
}

#[tauri::command]
pub async fn discover_monitor_devices() -> Result<Vec<DiscoveredMonitorDevice>, String> {
    tauri::async_runtime::spawn_blocking(MonitorService::discover_devices)
        .await
        .map_err(|error| format!("设备发现任务失败：{error}"))?
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
pub fn write_ai_profile(
    service: State<'_, MonitorService>,
    profile: AiProfile,
) -> Result<HookConfigWriteResult, String> {
    service.write_profile(profile)
}

#[tauri::command]
pub fn preview_hook_config(
    service: State<'_, MonitorService>,
    profile: AiProfile,
) -> Result<HookConfigPreview, String> {
    service.hook_config_preview(profile)
}

#[tauri::command]
pub fn list_local_hook_configs(
    service: State<'_, MonitorService>,
) -> Result<Vec<LocalHookConfig>, String> {
    service.local_hook_configs()
}
