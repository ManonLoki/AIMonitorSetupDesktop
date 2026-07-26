// 引入领域层的 system 模块（以别名方式整体引入）以及 SystemOverview 类型
use crate::domain::system::{self, SystemOverview};

/// Tauri transport adapter. Business decisions belong in `domain`, not here.
// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 获取系统概览信息的命令：仅做透传适配，不包含业务逻辑
pub fn get_system_overview() -> SystemOverview {
    // 直接委托给领域层的 overview 函数完成实际逻辑并返回结果
    system::overview()
}
