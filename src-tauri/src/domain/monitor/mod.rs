// 监控领域模型：设置、设备发现、AI Hook 配置生成/合并、Hook 生命周期状态机、
// 上传图片处理均拆分到本目录下的子模块中，本文件只保留模块声明、共享常量
// 与统一的对外重导出，保证 `crate::domain::monitor::X` 的引用路径保持不变。
// device/settings/image_processing 额外标注 pub(crate)：除了下方的重导出，
// 其它子模块的测试代码也会直接按子模块路径引用几个测试专用的构造函数/类型
// （如 HookContent、default_enabled_ai_tools），避免为此在公共 API 上
// 保留生产代码从未用到的重导出。
pub(crate) mod device;
mod hook_config_types;
mod hook_state_machine;
mod hooks;
pub(crate) mod image_processing;
mod payload;
mod saved_data;
pub(crate) mod settings;

// 以下重导出保持 `crate::domain::monitor::X` 的路径与拆分前完全一致。
pub use device::{
    AiProfile, AiTool, DiscoveredMonitorDevice, DiscoverySource, HookBehavior, MonitorDeviceRoute,
    normalize_enabled_ai_tools,
};
pub use hook_config_types::{
    HookConfigDirectories, HookConfigLocation, HookConfigPreview, HookConfigWriteResult,
};
pub(crate) use hook_state_machine::encode_base64;
pub use hook_state_machine::{HookEventDecision, HookStateMachine, HookTransition};
pub(crate) use hooks::managed_hook_marker;
pub use hooks::{
    ai_tool_name, contains_managed_hook_config, generate_hook_auxiliary_configs,
    generate_hook_config, hook_config_filename, hook_requires_review, hook_restart_required,
    merge_hook_config, tool_from_slug,
};
pub use image_processing::process_image_upload;
pub use payload::{MinimalHookPayload, minimize_native_hook_payload};
pub use saved_data::{
    SavedMonitorData, normalize_base_url, validate_device_route, validate_profile,
    validate_saved_monitor_data, validate_username,
};
pub use settings::{MonitorSettings, validate_discovery_interval_minutes};

// 未配置设备时的占位基地址，仅用于默认值展示。
const DEFAULT_BASE_URL: &str = "http://192.168.1.100:8080";
// 本机 Hook 中继服务监听的固定端口。
pub const DEFAULT_HOOK_RELAY_PORT: u16 = 10_240;
/// 在线设备自动检查的默认间隔：启动后立即检查一次，之后每分钟刷新。
pub const DEFAULT_DISCOVERY_INTERVAL_MINUTES: u64 = 1;
// 自动检查间隔允许的最小值（分钟），避免设为 0 造成忙轮询。
pub const MIN_DISCOVERY_INTERVAL_MINUTES: u64 = 1;
// 自动检查间隔允许的最大值（分钟），避免设置过大导致长时间感知不到设备上下线。
pub const MAX_DISCOVERY_INTERVAL_MINUTES: u64 = 60;

/// AI 客户端原生 Hook 输入允许的最大体积。原始输入只在短生命周期的 relay
/// 子进程里解析，随后立即压缩为下方的最小信封，不会进入 listener 队列。
pub const MAX_NATIVE_HOOK_INPUT_BYTES: usize = 4 * 1024 * 1024;
