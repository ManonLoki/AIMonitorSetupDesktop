// Tauri command arguments are transport-owned values deserialized by the
// generated adapter, even when the application service only borrows them.
// 允许 needless_pass_by_value：Tauri 命令参数由生成的适配器反序列化后按值持有，
// 即使应用服务内部只需要借用，也不视为多余的按值传参
#![allow(clippy::needless_pass_by_value)]

// 引入 Tauri 的 AppHandle（应用句柄）和 State（托管状态提取器）
use tauri::{AppHandle, State};

use crate::{
    // 引入应用层的连接状态、图片上传参数、监控服务、远程图片类型
    application::monitor::{
        ConnectionStatus, ImageUpload, MonitorDeviceSnapshot, MonitorService, RemoteImageGallery,
    },
    domain::AppError,
    // 引入领域层的 AI 配置档案、AI 工具类型、发现到的设备、hook 配置位置及写入结果、监控设置等类型
    domain::monitor::{
        AiProfileDraft, AiProfileDraftSet, AiTool, DiscoveredMonitorDevice, HookConfigLocation,
        HookConfigWriteResult, MonitorCapabilities, MonitorSettings, monitor_capabilities,
    },
};

/// 返回 Rust 领域层声明的工具目录、配置范围与 Hook 行为顺序。
#[tauri::command]
pub fn get_monitor_capabilities() -> MonitorCapabilities {
    monitor_capabilities()
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 获取当前监控配置
pub fn get_monitor_settings(
    service: State<'_, MonitorService>,
) -> Result<MonitorSettings, AppError> {
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
) -> Result<MonitorDeviceSnapshot, AppError> {
    // 委托给监控服务处理设备选择，并返回同一版本的设备业务快照
    service.select_device_snapshot(&device)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 保存监控设备使用的用户名
pub fn save_monitor_username(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端传入的新用户名
    username: String,
) -> Result<MonitorSettings, AppError> {
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
) -> Result<MonitorSettings, AppError> {
    // 委托给监控服务保存发现间隔，并返回更新后的设置
    service.save_discovery_interval(minutes)
}

#[tauri::command]
// 保存设置页勾选的 AI 客户端列表
pub fn save_enabled_ai_tools(
    service: State<'_, MonitorService>,
    tools: Vec<AiTool>,
) -> Result<MonitorSettings, AppError> {
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
) -> Result<MonitorDeviceSnapshot, AppError> {
    // 完整发现、自动选择与原子快照构造均由应用服务编排。
    service.refresh_online_devices(&app).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 检查与监控设备的连接状态
pub async fn check_monitor_connection(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 可选的目标设备基础 URL，为空则使用当前已选设备
    base_url: Option<String>,
) -> Result<ConnectionStatus, AppError> {
    // 委托给监控服务异步检查连接状态
    service.check_connection(base_url.as_deref()).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 列出远程设备上已有的图片
pub async fn list_remote_images(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 查询发起时的设备并发令牌；选择变化时 Rust 拒绝返回其他设备的数据
    expected_device_id: String,
) -> Result<RemoteImageGallery, AppError> {
    // 委托给监控服务异步获取远程图片列表
    service.images(&expected_device_id).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 上传图片到远程监控设备
pub async fn upload_remote_images(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    expected_device_id: String,
    // 前端传入的待上传图片数据列表
    images: Vec<ImageUpload>,
) -> Result<Vec<String>, AppError> {
    // 委托给监控服务异步执行上传，返回上传结果（如文件名列表）
    service.upload_images(&expected_device_id, images).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 删除远程设备上的一张图片
pub async fn delete_remote_image(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    expected_device_id: String,
    // 前端传入的待删除文件名
    filename: String,
) -> Result<(), AppError> {
    // 委托给监控服务异步执行删除
    service.delete_image(&expected_device_id, &filename).await
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 列出当前设备全部 AI 工具的可编辑 Profile 草稿
pub fn list_ai_profiles(service: State<'_, MonitorService>) -> Result<AiProfileDraftSet, AppError> {
    // 已保存值和未保存默认值均由 Rust 应用服务合并生成
    service.profile_drafts()
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 列出各 AI 工具对应的 hook 配置文件位置
pub fn list_hook_config_locations(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
) -> Result<Vec<HookConfigLocation>, AppError> {
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
) -> Result<HookConfigLocation, AppError> {
    // 委托给监控服务保存该工具的 hook 配置目录，并返回更新后的位置信息
    service.save_hook_config_directory(tool, &directory)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 保存一个不携带设备归属的 AI Profile 草稿
pub fn save_ai_profile(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 草稿读取时绑定的设备并发令牌，不作为调用方指定的写入目标
    expected_device_id: String,
    // 前端传入的待保存草稿，设备 ID 由 Rust 绑定到当前设备
    profile: AiProfileDraft,
) -> Result<AiProfileDraft, AppError> {
    // 委托给监控服务校验、持久化并返回规范化后的草稿
    service.save_profile_draft(&expected_device_id, profile)
}

// 声明这是一个可被前端通过 invoke 调用的 Tauri 命令
#[tauri::command]
// 为指定 AI 工具写入 hook 配置文件
pub fn write_hook_config(
    // Tauri 托管状态中取出的监控服务实例
    service: State<'_, MonitorService>,
    // 前端指定的目标 AI 工具类型
    tool: AiTool,
) -> Result<HookConfigWriteResult, AppError> {
    // 委托给监控服务执行 hook 配置文件写入，并返回写入结果
    service.write_hook_config(tool)
}
