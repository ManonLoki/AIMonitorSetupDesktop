use crate::domain::system::{self, SystemOverview};

/// Tauri transport adapter. Business decisions belong in `domain`, not here.
#[tauri::command]
pub fn get_system_overview() -> SystemOverview {
    system::overview()
}
