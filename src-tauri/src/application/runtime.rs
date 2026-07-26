// 引入 serde 的 Serialize，用于将结构体序列化后返回给前端
use serde::Serialize;
// 引入 Tauri 相关的应用、应用句柄、状态管理、运行时、窗口、窗口事件类型，
// 以及菜单相关类型（勾选菜单项、菜单、菜单项、预定义菜单项）和托盘相关类型（托盘图标、托盘图标构建器）
use tauri::{
    App, AppHandle, Manager, Runtime, Window, WindowEvent,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
};
// 引入开机自启插件的管理器扩展 trait，用于获取/设置自启动状态
use tauri_plugin_autostart::ManagerExt;

// 引入上级模块中监控相关的 hook 中继状态类型与监控服务
use super::monitor::{HookRelayStatus, MonitorService};

// 托盘图标的固定标识符
const TRAY_ID: &str = "aimonitor-tray";
// 菜单项「显示窗口」的固定标识符
const SHOW_WINDOW_MENU_ID: &str = "show-window";
// 菜单项「开机自启」的固定标识符
const AUTOSTART_MENU_ID: &str = "autostart";
// 菜单项「退出」的固定标识符
const QUIT_MENU_ID: &str = "quit";

// 运行时托管的菜单状态，持有需要在运行期间动态更新的菜单项与托盘图标句柄
pub struct RuntimeMenuState<R: Runtime> {
    // 「开机自启」勾选菜单项，用于同步勾选状态
    autostart: CheckMenuItem<R>,
    // 托盘图标句柄，以下划线前缀命名表示仅用于持有生命周期、不主动读取
    _tray: TrayIcon<R>,
}

// 派生 Serialize，使该结构体可被序列化返回前端
#[derive(Serialize)]
// 序列化时字段名转换为 camelCase，匹配 TypeScript 端命名习惯
#[serde(rename_all = "camelCase")]
// 运行时概览信息：开机自启状态、静默启动是否支持、hook 中继状态
pub struct RuntimeOverview {
    // 是否已启用开机自启
    pub autostart_enabled: bool,
    // 是否支持静默启动（当前固定为 true）
    pub silent_start_supported: bool,
    // hook 中继（监听/转发）当前状态
    pub hook_relay: HookRelayStatus,
}

// 组装运行时概览信息
pub fn runtime_overview(
    // Tauri 应用句柄，用于访问自启动插件
    app: &AppHandle,
    // 监控服务引用，用于查询 hook 中继状态
    monitor: &MonitorService,
) -> Result<RuntimeOverview, String> {
    Ok(RuntimeOverview {
        // 读取当前开机自启是否启用
        autostart_enabled: app
            .autolaunch()
            .is_enabled()
            // 读取失败时转换为业务可读的中文错误信息
            .map_err(|error| format!("无法读取开机自启状态：{error}"))?,
        // 静默启动能力固定为已支持
        silent_start_supported: true,
        // 查询监控服务的 hook 中继状态，失败则向上传播错误
        hook_relay: monitor.hook_relay_status()?,
    })
}

// 设置开机自启的启用/关闭状态
pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    // 获取自启动管理器
    let manager = app.autolaunch();
    // 根据目标状态调用启用或关闭接口，并统一转换错误信息为中文
    let result = if enabled {
        manager
            .enable()
            .map_err(|error| format!("无法启用开机自启：{error}"))
    } else {
        manager
            .disable()
            .map_err(|error| format!("无法关闭开机自启：{error}"))
    };
    // 操作成功后同步托盘菜单中的勾选状态，保持 UI 与实际状态一致
    if result.is_ok() {
        sync_tray_autostart(app, enabled);
    }
    // 将设置结果（成功或错误）返回给调用方
    result
}

// 初始化桌面运行时：设置激活策略、构建托盘菜单与托盘图标，并托管菜单状态
pub fn setup_desktop_runtime(app: &mut App) -> tauri::Result<()> {
    // 仅在 macOS 上生效：将应用激活策略设置为「附属应用」，不在 Dock 中显示独立图标
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    // 读取当前是否已启用开机自启，读取失败则默认视为未启用
    let autostart_enabled = app.autolaunch().is_enabled().unwrap_or(false);
    // 构建「显示窗口」菜单项
    let show_window = MenuItem::with_id(app, SHOW_WINDOW_MENU_ID, "显示窗口", true, None::<&str>)?;
    // 构建「开机自启」勾选菜单项，初始勾选状态与实际自启动状态保持一致
    let autostart = CheckMenuItem::with_id(
        app,
        AUTOSTART_MENU_ID,
        "开机自启",
        true,
        autostart_enabled,
        None::<&str>,
    )?;
    // 构建菜单分隔线
    let separator = PredefinedMenuItem::separator(app)?;
    // 构建「退出」菜单项
    let quit = MenuItem::with_id(app, QUIT_MENU_ID, "退出", true, None::<&str>)?;
    // 将上述菜单项按顺序组装成完整菜单
    let menu = Menu::with_items(app, &[&show_window, &autostart, &separator, &quit])?;

    // 克隆一份「开机自启」菜单项句柄，供事件回调闭包捕获使用
    let autostart_for_events = autostart.clone();
    // 开始构建系统托盘图标
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        // 绑定托盘的右键/左键菜单
        .menu(&menu)
        // 设置鼠标悬停提示文字
        .tooltip("AIMonitor")
        // 左键点击时也弹出菜单
        .show_menu_on_left_click(true)
        // 注册菜单项点击事件处理回调
        .on_menu_event(move |app, event| match event.id().as_ref() {
            // 点击「显示窗口」：始终显示并聚焦主窗口。
            SHOW_WINDOW_MENU_ID => show_main_window(app),
            // 点击「开机自启」：读取当前勾选状态并尝试应用，失败则回滚勾选框显示状态
            AUTOSTART_MENU_ID => {
                let enabled = autostart_for_events.is_checked().unwrap_or(false);
                if set_autostart(app, enabled).is_err() {
                    let _ = autostart_for_events.set_checked(!enabled);
                }
            }
            // 点击「退出」：以状态码 0 退出整个应用
            QUIT_MENU_ID => app.exit(0),
            // 其他未知菜单事件：忽略
            _ => {}
        });
    // 若应用配置了默认窗口图标，则将其同时用作托盘图标；否则使用无图标的默认托盘样式
    let tray = match app.default_window_icon().cloned() {
        Some(icon) => tray.icon(icon).build(app)?,
        None => tray.build(app)?,
    };

    // 将菜单项与托盘图标句柄托管到 Tauri 状态管理中，供后续动态更新使用
    app.manage(RuntimeMenuState {
        autostart,
        _tray: tray,
    });
    // 初始化成功
    Ok(())
}

// 显示主窗口：取消最小化并聚焦。
pub fn show_main_window(app: &AppHandle) {
    // 尝试获取名为 "main" 的窗口句柄
    if let Some(window) = app.get_webview_window("main") {
        // 显示窗口（忽略可能的错误）
        let _ = window.show();
        // 取消最小化状态（忽略可能的错误）
        let _ = window.unminimize();
        // 将焦点设置到该窗口（忽略可能的错误）
        let _ = window.set_focus();
    }
}

// 处理窗口事件：拦截关闭请求，改为隐藏窗口而非真正退出
pub fn handle_window_event(window: &Window, event: &WindowEvent) {
    // 仅处理「请求关闭窗口」事件
    if let WindowEvent::CloseRequested { api, .. } = event {
        // 阻止默认的关闭行为
        api.prevent_close();
        // 改为隐藏窗口（忽略可能的错误）
        let _ = window.hide();
    }
}

// 同步托盘菜单中「开机自启」勾选项的勾选状态
fn sync_tray_autostart(app: &AppHandle, enabled: bool) {
    // 从托管状态中取出菜单状态（若尚未初始化则跳过）
    if let Some(state) = app.try_state::<RuntimeMenuState<tauri::Wry>>() {
        // 设置勾选状态（忽略可能的错误）
        let _ = state.autostart.set_checked(enabled);
    }
}
