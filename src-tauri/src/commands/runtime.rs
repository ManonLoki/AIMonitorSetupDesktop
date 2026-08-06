// Tauri owns command argument deserialization and passes these adapter values
// by value even though the application layer only borrows them.
// 允许 needless_pass_by_value：因为 Tauri 命令的参数是由框架反序列化后按值传入的，
// 即便应用层实际只需要借用，这里也不算多余的按值传参
#![allow(clippy::needless_pass_by_value)]

// 引入 Tauri 的 AppHandle（应用句柄）和 State（托管状态提取器）
use tauri::{AppHandle, State};

// 引入应用层的监控服务，以及运行时概览相关的类型与函数
use crate::application::{
    monitor::MonitorService,
    runtime::{RuntimeOverview, runtime_overview, set_autostart},
};
use crate::domain::AppError;

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 获取运行时概览（开机自启状态、静默启动支持情况、hook 中继状态等）
pub fn get_runtime_overview(
    // Tauri 注入的应用句柄，用于访问自启动插件等能力
    app: AppHandle,
    // Tauri 托管状态中取出的监控服务实例
    monitor: State<'_, MonitorService>,
) -> Result<RuntimeOverview, AppError> {
    // 转发给应用层函数完成具体逻辑
    runtime_overview(&app, &monitor)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 更新开机自启设置，并返回更新后的运行时概览
pub fn update_autostart(
    // Tauri 注入的应用句柄
    app: AppHandle,
    // Tauri 托管状态中取出的监控服务实例
    monitor: State<'_, MonitorService>,
    // 前端传入的开机自启开关目标状态
    enabled: bool,
) -> Result<RuntimeOverview, AppError> {
    // 先调用应用层函数设置开机自启状态，失败则直接向上传播错误
    set_autostart(&app, enabled)?;
    // 设置成功后重新获取最新的运行时概览并返回
    runtime_overview(&app, &monitor)
}
