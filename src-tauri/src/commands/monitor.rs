// Tauri command arguments are transport-owned values deserialized by the
// generated adapter, even when the application service only borrows them.
// 允许 needless_pass_by_value：Tauri 命令参数由生成的适配器反序列化后按值持有，
// 即使应用服务内部只需要借用，也不视为多余的按值传参
#![allow(clippy::needless_pass_by_value)]

// 引入 Tauri 的 AppHandle（应用句柄）和 State（托管状态提取器）
use tauri::{AppHandle, State};

use crate::{
    // 引入应用层的连接状态、图片上传参数、监控服务、远程图片类型
    application::monitor::{ConnectionStatus, ImageUpload, MonitorService, RemoteImage},
    // 引入领域层的 AI 配置档案、AI 工具类型、发现到的设备、hook 配置位置及写入结果、监控设置等类型
    domain::monitor::{
        AiProfile, AiTool, DiscoveredMonitorDevice, HookConfigLocation, HookConfigWriteResult,
        MonitorSettings,
    },
};

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 获取当前监控配置
pub fn get_monitor_settings(service: State<'_, MonitorService>) -> Result<MonitorSettings, String> {
    // 委托给监控服务读取当前设置
    service.settings()
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 选择一台已发现的监控设备作为当前设备
pub fn select_monitor_device(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的、要选择的设备信息
    device: DiscoveredMonitorDevice,
) -> Result<MonitorSettings, String> {
    // 委托给监控服务处理设备选择，并返回更新后的设置
    service.select_device(&device)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 保存监控设备使用的用户名
pub fn save_monitor_username(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的新用户名
    username: String,
) -> Result<MonitorSettings, String> {
    // 委托给监控服务保存用户名，并返回更新后的设置
    service.save_username(&username)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 保存设备发现的轮询间隔（分钟）
pub fn save_discovery_interval(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的间隔分钟数
    minutes: u64,
) -> Result<MonitorSettings, String> {
    // 委托给监控服务保存发现间隔，并返回更新后的设置
    service.save_discovery_interval(minutes)
}

#[tauri::command]
// 保存设置页勾选的 AI 客户端列表
pub fn save_enabled_ai_tools(
    service: State<'_, MonitorService>,
    tools: Vec<AiTool>,
) -> Result<MonitorSettings, String> {
    service.save_enabled_ai_tools(&tools)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 异步发现局域网内的监控设备
pub async fn discover_monitor_devices(
    // Tauri 注入的应用句柄，用于向前端发布事件等
    app: AppHandle,
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
) -> Result<Vec<DiscoveredMonitorDevice>, String> {
    // 在阻塞线程池中执行设备发现候选项收集（该操作可能阻塞，故放入 spawn_blocking）
    let candidates =
        tauri::async_runtime::spawn_blocking(MonitorService::discover_device_candidates)
            // 等待阻塞任务完成
            .await
            // 任务本身失败（如线程 panic）时转换为字符串错误；`??` 再展开内部 Result
            .map_err(|error| format!("设备发现任务失败：{error}"))??;
    // 对候选设备做进一步处理（如连通性校验等），得到最终设备列表
    let devices = service.finish_device_discovery(candidates).await?;
    // 将在线设备列表发布给前端（如通过事件），并返回结果
    service.publish_online_devices(&app, devices)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 检查与监控设备的连接状态
pub async fn check_monitor_connection(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 可选的目标设备基础 URL，为空则使用当前已选设备
    base_url: Option<String>,
) -> Result<ConnectionStatus, String> {
    // 委托给监控服务异步检查连接状态
    service.check_connection(base_url.as_deref()).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 列出远程设备上已有的图片
pub async fn list_remote_images(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
) -> Result<Vec<RemoteImage>, String> {
    // 委托给监控服务异步获取远程图片列表
    service.images().await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 上传图片到远程监控设备
pub async fn upload_remote_images(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的待上传图片数据列表
    images: Vec<ImageUpload>,
) -> Result<Vec<String>, String> {
    // 委托给监控服务异步执行上传，返回上传结果（如文件名列表）
    service.upload_images(images).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 删除远程设备上的一张图片
pub async fn delete_remote_image(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的待删除文件名
    filename: String,
) -> Result<(), String> {
    // 委托给监控服务异步执行删除
    service.delete_image(&filename).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 列出所有已保存的 AI 配置档案
pub fn list_ai_profiles(service: State<'_, MonitorService>) -> Result<Vec<AiProfile>, String> {
    // 委托给监控服务读取 AI 配置档案列表
    service.profiles()
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 列出各 AI 工具对应的 hook 配置文件位置
pub fn list_hook_config_locations(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
) -> Result<Vec<HookConfigLocation>, String> {
    // 委托给监控服务获取 hook 配置位置列表
    service.hook_config_locations()
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 保存指定 AI 工具的 hook 配置目录
pub fn save_hook_config_directory(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端指定的目标 AI 工具类型
    tool: AiTool,
    // 前端传入的配置目录路径
    directory: String,
) -> Result<HookConfigLocation, String> {
    // 委托给监控服务保存该工具的 hook 配置目录，并返回更新后的位置信息
    service.save_hook_config_directory(tool, &directory)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 保存一个 AI 配置档案
pub fn save_ai_profile(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的待保存配置档案
    profile: AiProfile,
) -> Result<AiProfile, String> {
    // 委托给监控服务保存配置档案，并返回保存后的结果
    service.save_profile(profile)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 为指定 AI 工具写入 hook 配置文件
pub fn write_hook_config(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端指定的目标 AI 工具类型
    tool: AiTool,
) -> Result<HookConfigWriteResult, String> {
    // 委托给监控服务执行 hook 配置文件写入，并返回写入结果
    service.write_hook_config(tool)
}
