// Prevents additional console window on Windows in release, DO NOT REMOVE!!
// 在 Windows release 构建下禁用控制台子系统，避免额外弹出命令行窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 程序入口函数
fn main() {
    if let Some(exit_code) = ai_monitor_setup_lib::run_hook_relay_if_requested() {
        std::process::exit(exit_code);
    }
    // 调用库 crate 中的 run 函数，真正启动 Tauri 应用
    ai_monitor_setup_lib::run();
}
