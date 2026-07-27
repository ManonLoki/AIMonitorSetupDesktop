// 声明 application 模块（应用服务层：编排业务逻辑、对接 Tauri 运行时能力）
mod application;
// 声明 commands 模块（Tauri 命令适配层：仅做参数/返回值的传输适配）
mod commands;
// 声明 domain 模块（领域层：业务实体与纯逻辑）
mod domain;

// 引入应用层的监控服务
use application::monitor::MonitorService;
// 引入应用层运行时相关函数：窗口事件处理、桌面运行时初始化、显示主窗口
use application::runtime::{handle_window_event, setup_desktop_runtime, show_main_window};
// 引入 Tauri 的 Manager trait，提供 app.manage()/app.path() 等能力
use tauri::Manager;

/// 命令型 AI Hook 复用主程序的轻量 relay 模式。返回 `Some(exit_code)` 时调用方
/// 必须立即退出，不能继续初始化 Tauri 或触发单实例窗口逻辑。
#[must_use]
pub fn run_hook_relay_if_requested() -> Option<i32> {
    application::hook_relay::run_from_process_args()
}

/// Starts the native Tauri application.
///
/// # Panics
///
/// Panics when Tauri cannot initialize or run the application runtime.
// 仅在移动端目标下，将该函数标记为移动端入口点
#[cfg_attr(mobile, tauri::mobile_entry_point)]
// 启动原生 Tauri 应用的入口函数
pub fn run() {
    // 使用默认配置开始构建 Tauri 应用
    tauri::Builder::default()
        // 注册单实例插件：当应用已在运行时，再次启动会触发此回调而非新开一个实例
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            // 将已有实例的主窗口显示出来，而不是启动新实例
            show_main_window(app);
        }))
        // 注册开机自启插件
        .plugin({
            // 构建自启动插件，指定应用名称
            let autostart = tauri_plugin_autostart::Builder::new().app_name("AIMonitor");
            // 仅在 macOS 上：指定使用 LaunchAgent 方式实现开机自启
            #[cfg(target_os = "macos")]
            let autostart =
                autostart.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
            // 配置自启动时附加 "--silent" 参数（用于静默启动，不弹出主窗口），并完成构建
            autostart.arg("--silent").build()
        })
        // 注册对话框插件（用于原生文件/目录选择等对话框能力）
        .plugin(tauri_plugin_dialog::init())
        // 注册系统打开能力，设置页的 GitHub 链接由默认浏览器打开
        .plugin(tauri_plugin_opener::init())
        // 注册应用初始化（setup）钩子，在应用启动时执行一次性初始化逻辑
        .setup(|app| {
            // 初始化桌面运行时：设置激活策略、构建托盘菜单与图标
            setup_desktop_runtime(app)?;
            // 仅在 macOS 上：若开机自启已启用，则显式再启用一次自启动插件本身
            #[cfg(target_os = "macos")]
            {
                // 引入自启动管理器扩展 trait
                use tauri_plugin_autostart::ManagerExt;

                // 获取自启动管理器
                let autostart = app.autolaunch();
                // 若当前已启用开机自启（读取失败则视为未启用）
                if autostart.is_enabled().unwrap_or(false) {
                    // 再次调用启用接口，失败则转换为 io::Error 并向上传播
                    autostart.enable().map_err(std::io::Error::other)?;
                }
            }
            // 获取应用数据目录路径
            let app_data_dir = app.path().app_data_dir()?;
            // 获取用户主目录路径（用于定位各 AI 工具的 hook 配置位置）
            let config_home = app.path().home_dir()?;
            // 加载监控服务：基于应用数据目录与用户主目录初始化状态，失败则转换为 io::Error
            let service =
                MonitorService::load(&app_data_dir, &config_home).map_err(std::io::Error::other)?;
            // 启动 hook 监听器，接收外部工具发来的 hook 事件
            service.start_hook_listener();
            // 启动后台设备发现任务，定期在局域网内扫描监控设备
            service.start_background_device_discovery(app.handle().clone());
            // 将监控服务实例托管到 Tauri 状态管理中，供各命令通过 State 提取使用
            app.manage(service);
            // 判断本次启动参数中是否包含 "--silent"（即由开机自启静默拉起）
            let silent_start = std::env::args_os().any(|argument| argument == "--silent");
            // 非静默启动时才主动显示主窗口
            if !silent_start {
                show_main_window(app.handle());
            }
            // setup 钩子执行成功
            Ok(())
        })
        // 注册窗口事件处理回调（用于拦截关闭请求、改为隐藏窗口）
        .on_window_event(handle_window_event)
        // 注册可供前端 invoke 调用的命令列表
        .invoke_handler(tauri::generate_handler![
            // 系统模块：获取系统概览信息
            commands::system::get_system_overview,
            // 监控模块：获取当前监控设置
            commands::monitor::get_monitor_settings,
            // 监控模块：选择监控设备
            commands::monitor::select_monitor_device,
            // 监控模块：保存监控用户名
            commands::monitor::save_monitor_username,
            // 监控模块：保存设备发现轮询间隔
            commands::monitor::save_discovery_interval,
            // 监控模块：保存设置页选中的 AI 客户端
            commands::monitor::save_enabled_ai_tools,
            // 监控模块：发现局域网内的监控设备
            commands::monitor::discover_monitor_devices,
            // 监控模块：检查监控设备连接状态
            commands::monitor::check_monitor_connection,
            // 监控模块：列出远程设备上的图片
            commands::monitor::list_remote_images,
            // 监控模块：上传图片到远程设备
            commands::monitor::upload_remote_images,
            // 监控模块：删除远程设备上的图片
            commands::monitor::delete_remote_image,
            // 监控模块：列出所有 AI 配置档案
            commands::monitor::list_ai_profiles,
            // 监控模块：列出各 AI 工具的 hook 配置文件位置
            commands::monitor::list_hook_config_locations,
            // 监控模块：保存指定 AI 工具的 hook 配置目录
            commands::monitor::save_hook_config_directory,
            // 监控模块：保存 AI 配置档案
            commands::monitor::save_ai_profile,
            // 监控模块：写入 hook 配置文件
            commands::monitor::write_hook_config,
            // 运行时模块：获取运行时概览（开机自启、hook 中继状态等）
            commands::runtime::get_runtime_overview,
            // 运行时模块：更新开机自启设置
            commands::runtime::update_autostart,
        ])
        // 使用生成的应用上下文运行整个 Tauri 应用（阻塞直至应用退出）
        .run(tauri::generate_context!())
        // 若运行失败则直接 panic，并附带说明信息
        .expect("error while running tauri application");
}
