// Tauri owns command argument deserialization and passes these adapter values
// by value even though the application layer only borrows them.
#![allow(clippy::needless_pass_by_value)]

use tauri::{AppHandle, State};

use crate::application::{
    monitor::MonitorService,
    runtime::{RuntimeOverview, runtime_overview, set_autostart},
};

#[tauri::command]
pub fn get_runtime_overview(
    app: AppHandle,
    monitor: State<'_, MonitorService>,
) -> Result<RuntimeOverview, String> {
    runtime_overview(&app, &monitor)
}

#[tauri::command]
pub fn update_autostart(
    app: AppHandle,
    monitor: State<'_, MonitorService>,
    enabled: bool,
) -> Result<RuntimeOverview, String> {
    set_autostart(&app, enabled)?;
    runtime_overview(&app, &monitor)
}
