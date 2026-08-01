// 标准库：HashSet 用于去重校验，Path 用于校验目录。
use std::collections::HashSet;
use std::path::Path;

// serde：结构体的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};

use super::device::normalize_enabled_ai_tools;
use super::device::{AiProfile, DiscoveredMonitorDevice, HookBehavior, MonitorDeviceRoute};
use super::hook_config_types::HookConfigDirectories;
use super::hooks::ai_tool_name;
use super::settings::{MonitorSettings, validate_discovery_interval_minutes};
use super::{MAX_PROFILE_SLOT, MIN_PROFILE_SLOT};

// 需要持久化到磁盘的全部监控数据：设置、设备路由、AI 配置、Hooks 目录。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMonitorData {
    /// 当前控制端安装实例的稳定唯一标识。发送槽位状态和心跳时作为所有者身份。
    #[serde(default)]
    pub client_id: String,
    pub settings: MonitorSettings,
    /// 所有已经连接并保存过的设备路由。`settings` 只表示当前 UI 选中的设备；
    /// Hook 中继会遍历这里的路由，并按设备 ID 关联对应 Profile。
    #[serde(default)]
    pub devices: Vec<MonitorDeviceRoute>,
    #[serde(default)]
    pub profiles: Vec<AiProfile>,
    #[serde(default)]
    pub hook_config_directories: HookConfigDirectories,
}

/// 在应用接纳持久化数据前验证跨集合不变量，避免损坏或部分写入的数据让
/// “当前设备”、设备路由和 Profile 指向不同事实来源。
pub fn validate_saved_monitor_data(data: &SavedMonitorData) -> Result<(), String> {
    validate_client_id(&data.client_id)?;
    // 当前选中设备的基地址必须是合法格式。
    normalize_base_url(&data.settings.base_url)?;
    // 自动检查间隔必须在允许范围内。
    validate_discovery_interval_minutes(data.settings.discovery_interval_minutes)?;
    if normalize_enabled_ai_tools(&data.settings.enabled_ai_tools) != data.settings.enabled_ai_tools
    {
        return Err("AI 客户端设置包含重复项或顺序无效".to_owned());
    }
    // 用户名非空时才校验（允许尚未设置用户名的初始状态）。
    if !data.settings.username.is_empty() {
        validate_username(&data.settings.username)?;
    }

    // 收集所有设备 ID，供后续判断“设备是否存在”及“是否重复”使用。
    let mut device_ids = HashSet::new();
    for device in &data.devices {
        // 设备 ID 或名称为空视为脏数据，直接拒绝。
        if device.device_id.trim().is_empty() || device.device_name.trim().is_empty() {
            return Err("持久化设备路由缺少设备 ID 或名称".to_owned());
        }
        // 每条设备路由的基地址也必须合法。
        normalize_base_url(&device.base_url)?;
        // insert 返回 false 表示该 ID 已存在，说明设备路由重复。
        if !device_ids.insert(device.device_id.as_str()) {
            return Err(format!("设备路由重复：{}", device.device_id));
        }
    }

    // 若设置里选中了某个设备 ID，必须能在 devices 列表里找到对应路由，
    // 且路由信息要与 settings 中冗余保存的字段完全一致。
    if !data.settings.device_id.is_empty() {
        let selected = data
            .devices
            .iter()
            .find(|device| device.device_id == data.settings.device_id)
            .ok_or_else(|| "当前设备缺少对应的持久化路由".to_owned())?;
        if selected.base_url != data.settings.base_url
            || selected.device_name != data.settings.device_name
        {
            return Err("当前设备设置与持久化路由不一致".to_owned());
        }
    }

    // 收集 (设备ID, 工具) 组合，检测同一设备下同一工具是否被重复配置。
    let mut profile_keys = HashSet::new();
    for profile in &data.profiles {
        // Profile 关联的设备必须真实存在于 device_ids 中。
        if profile.device_id.trim().is_empty() || !device_ids.contains(profile.device_id.as_str()) {
            return Err("AI 配置关联了不存在的设备".to_owned());
        }
        // 复用单个 Profile 的完整校验逻辑（位置范围、四种行为齐全等）。
        validate_profile(profile.clone())?;
        // insert 返回 false 说明该设备的该工具已经配置过一次，属于重复。
        if !profile_keys.insert((profile.device_id.as_str(), profile.tool)) {
            return Err(format!(
                "设备 {} 的 {} AI 配置重复",
                profile.device_id,
                ai_tool_name(profile.tool)
            ));
        }
    }

    // 依次检查所有工具的自定义 Hooks 配置目录：非空时必须是绝对路径。
    for directory in [
        &data.hook_config_directories.codex,
        &data.hook_config_directories.claude_code,
        &data.hook_config_directories.cursor,
        &data.hook_config_directories.open_code,
        &data.hook_config_directories.work_buddy,
        &data.hook_config_directories.hermes,
        &data.hook_config_directories.open_claw,
        &data.hook_config_directories.code_buddy,
        &data.hook_config_directories.qwen_code,
        &data.hook_config_directories.kimi_code,
        &data.hook_config_directories.qoder,
        &data.hook_config_directories.gemini_cli,
        &data.hook_config_directories.github_copilot,
    ] {
        if !directory.is_empty() && !Path::new(directory).is_absolute() {
            return Err("持久化 Hooks 配置目录必须使用绝对路径".to_owned());
        }
    }
    Ok(())
}

/// 控制端身份会进入 URL path 和接收端租约表，仅允许紧凑的 ASCII 标识。
pub fn validate_client_id(value: &str) -> Result<&str, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("客户端 ID 必须是 1-128 位字母、数字、连字符或下划线".to_owned());
    }
    Ok(value)
}

// 规范化并校验用户输入的设备基地址：去空白、去掉末尾斜杠、要求 http/https 协议且不含空白。
pub fn normalize_base_url(value: &str) -> Result<String, String> {
    // 先去首尾空白，再去掉末尾多余的 '/'，避免拼接路径时出现双斜杠。
    let normalized = value.trim().trim_end_matches('/');
    // 只接受 http:// 或 https:// 开头的地址。
    let has_supported_scheme =
        normalized.starts_with("http://") || normalized.starts_with("https://");

    // 协议不受支持，或地址内部还含有空白字符（比如粘贴时带了空格），都视为非法。
    if !has_supported_scheme || normalized.contains(char::is_whitespace) {
        return Err("基地址必须是以 http:// 或 https:// 开头的有效地址".to_owned());
    }

    // 取出 "://" 之后的主机部分（authority），用于校验是否给了主机名/IP。
    let authority = normalized
        .split_once("://")
        .map_or("", |(_, authority)| authority);
    // authority 为空，或者以 ':' 开头（只写了端口没写主机），都视为非法。
    if authority.is_empty() || authority.starts_with(':') {
        return Err("基地址缺少有效的主机名或 IP".to_owned());
    }

    Ok(normalized.to_owned())
}

// 将一次发现结果转换为可持久化的设备路由，同时校验 ID/名称/基地址均有效。
pub fn validate_device_route(
    device: &DiscoveredMonitorDevice,
) -> Result<MonitorDeviceRoute, String> {
    let device_id = device.id.trim();
    let device_name = device.name.trim();
    // ID 或名称为空说明用户还没有真正选中一个已发现的设备。
    if device_id.is_empty() || device_name.is_empty() {
        return Err("请选择发现的 AIMonitor 设备".to_owned());
    }

    Ok(MonitorDeviceRoute {
        base_url: normalize_base_url(&device.base_url)?,
        device_id: device_id.to_owned(),
        device_name: device_name.to_owned(),
    })
}

// 校验并规范化显示用户名：去空白后不能为空。
pub fn validate_username(username: &str) -> Result<String, String> {
    let username = username.trim();
    if username.is_empty() {
        return Err("显示用户名不能为空".to_owned());
    }
    Ok(username.to_owned())
}

/// 校验 Profile 是否可用于生成 Hooks 配置：位置在 1-25 之间，且四种展示
/// 行为（空闲/运行中/询问/异常）各配置一次、都选择了图片。
pub fn validate_profile(mut profile: AiProfile) -> Result<AiProfile, String> {
    // 先去除设备 ID 首尾空白，回写到 profile 上。
    let device_id = profile.device_id.trim().to_owned();
    profile.device_id = device_id;
    // 展示位置必须落在领域层声明的闭区间内。
    if !(MIN_PROFILE_SLOT..=MAX_PROFILE_SLOT).contains(&profile.slot) {
        return Err(format!(
            "显示位置必须在 {MIN_PROFILE_SLOT} 到 {MAX_PROFILE_SLOT} 之间"
        ));
    }
    // 必须为领域层声明的每种展示行为各提供一条配置，多了少了都不合法。
    if profile.hooks.len() != HookBehavior::DISPLAY_BEHAVIORS.len() {
        return Err("必须配置空闲、运行中、询问和异常四种行为".to_owned());
    }

    // 用于记录已经出现过的行为类型，检测重复配置。
    let mut behaviors = HashSet::new();
    for hook in &mut profile.hooks {
        // 去除文案和图片文件名首尾空白，回写到 hook 上。
        hook.content = hook.content.trim().to_owned();
        hook.image = hook.image.trim().to_owned();
        // 每种行为都必须选择图片，否则展示屏无法渲染。
        if hook.image.is_empty() {
            return Err("每个行为都必须选择图片".to_owned());
        }
        // insert 返回 false 说明该行为已经出现过一次，属于重复配置。
        if !behaviors.insert(hook.behavior) {
            return Err("同一行为不能重复配置".to_owned());
        }
    }
    // 四种行为（空闲/运行中/询问/异常）必须全部出现在已配置集合中。
    if !HookBehavior::DISPLAY_BEHAVIORS
        .iter()
        .all(|behavior| behaviors.contains(behavior))
    {
        return Err("必须配置空闲、运行中、询问和异常四种行为".to_owned());
    }
    Ok(profile)
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use super::{
        AiProfile, HookConfigDirectories, MonitorDeviceRoute, MonitorSettings, SavedMonitorData,
        normalize_base_url, validate_profile, validate_saved_monitor_data, validate_username,
    };
    use crate::domain::monitor::{AiTool, HookBehavior, device::HookContent};

    // 测试用的工厂函数：构造一个四种行为齐全、校验可通过的合法 Profile。
    fn profile(tool: AiTool) -> AiProfile {
        AiProfile {
            device_id: "device-1".to_owned(),
            tool,
            slot: 4,
            hooks: vec![
                HookContent {
                    behavior: HookBehavior::Idle,
                    content: String::new(),
                    image: "idle.png".to_owned(),
                },
                HookContent {
                    behavior: HookBehavior::Running,
                    content: "正在思考".to_owned(),
                    image: "running.gif".to_owned(),
                },
                HookContent {
                    behavior: HookBehavior::Asking,
                    content: "需要确认".to_owned(),
                    image: "asking.png".to_owned(),
                },
                HookContent {
                    behavior: HookBehavior::Error,
                    content: "执行失败".to_owned(),
                    image: "error.png".to_owned(),
                },
            ],
        }
    }

    // 验证 normalize_base_url 会去除首尾空白，并去掉末尾多余的斜杠。
    #[test]
    fn base_url_is_trimmed_and_trailing_slashes_are_removed() {
        assert_eq!(
            normalize_base_url(" http://192.168.1.10:8080/ ").unwrap(),
            "http://192.168.1.10:8080"
        );
    }

    // 验证用户名校验：纯空白视为空用户名并拒绝，非空用户名会被去除首尾空白后保留。
    #[test]
    fn settings_require_a_username() {
        assert!(validate_username(" ").is_err());
        assert_eq!(validate_username(" Manon ").unwrap(), "Manon");
    }

    // 验证 Profile 校验规则：文案可以为空，但图片必须选择，否则拒绝。
    #[test]
    fn profile_allows_empty_content_but_requires_an_image() {
        // 清空图片后应当被拒绝。
        let mut invalid = profile(AiTool::Codex);
        invalid.hooks[0].image.clear();
        assert!(validate_profile(invalid).is_err());

        // 清空文案但保留图片，应当依然合法。
        let mut valid = profile(AiTool::Codex);
        valid.hooks[0].content.clear();
        assert!(validate_profile(valid).is_ok());
    }

    // 验证持久化数据校验：正常数据应通过；重复配置同一设备同一工具、
    // 或 Profile 关联了不存在的设备，都应当被拒绝。
    #[test]
    fn persisted_data_rejects_duplicate_or_cross_device_profiles() {
        let route = MonitorDeviceRoute {
            base_url: "http://192.168.1.10:8080".to_owned(),
            device_id: "device-1".to_owned(),
            device_name: "Desk".to_owned(),
        };
        let settings = MonitorSettings {
            base_url: route.base_url.clone(),
            device_id: route.device_id.clone(),
            device_name: route.device_name.clone(),
            ..MonitorSettings::default()
        };
        let valid = SavedMonitorData {
            client_id: "test-client".to_owned(),
            settings,
            devices: vec![route],
            profiles: vec![profile(AiTool::Codex)],
            hook_config_directories: HookConfigDirectories::default(),
        };
        // 基准数据本身应当通过校验。
        assert!(validate_saved_monitor_data(&valid).is_ok());

        // 同一设备重复添加同一工具的 Profile，应当被拒绝。
        let mut duplicate = valid.clone();
        duplicate.profiles.push(profile(AiTool::Codex));
        assert!(validate_saved_monitor_data(&duplicate).is_err());

        // Profile 关联了不存在的设备 ID，应当被拒绝。
        let mut orphaned = valid;
        orphaned.profiles[0].device_id = "unknown-device".to_owned();
        assert!(validate_saved_monitor_data(&orphaned).is_err());
    }
}
