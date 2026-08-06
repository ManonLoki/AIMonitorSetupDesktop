// 标准库：HashSet 用于去重校验，IP/Path 用于校验地址与目录。
use std::{collections::HashSet, net::Ipv6Addr, path::Path};
// serde：结构体的序列化与反序列化派生宏。
use http::{Uri, uri::Scheme};
use serde::{Deserialize, Serialize};

use crate::domain::AppError;

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
pub fn validate_saved_monitor_data(data: &SavedMonitorData) -> Result<(), AppError> {
    validate_client_id(&data.client_id)?;
    // 当前选中设备的基地址必须是合法格式。
    normalize_base_url(&data.settings.base_url)?;
    // 自动检查间隔必须在允许范围内。
    validate_discovery_interval_minutes(data.settings.discovery_interval_minutes)?;
    if normalize_enabled_ai_tools(&data.settings.enabled_ai_tools) != data.settings.enabled_ai_tools
    {
        return Err(AppError::new("error.savedData.aiToolsInvalid"));
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
            return Err(AppError::new("error.savedData.deviceRouteMissingIdOrName"));
        }
        // 每条设备路由的基地址也必须合法。
        normalize_base_url(&device.base_url)?;
        // insert 返回 false 表示该 ID 已存在，说明设备路由重复。
        if !device_ids.insert(device.device_id.as_str()) {
            return Err(AppError::new("error.savedData.deviceRouteDuplicate")
                .param("deviceId", device.device_id.clone()));
        }
    }

    // 若设置里选中了某个设备 ID，必须能在 devices 列表里找到对应路由，
    // 且路由信息要与 settings 中冗余保存的字段完全一致。
    if !data.settings.device_id.is_empty() {
        let selected = data
            .devices
            .iter()
            .find(|device| device.device_id == data.settings.device_id)
            .ok_or_else(|| AppError::new("error.savedData.currentDeviceMissingRoute"))?;
        if selected.base_url != data.settings.base_url
            || selected.device_name != data.settings.device_name
        {
            return Err(AppError::new("error.savedData.currentDeviceMismatch"));
        }
    }

    // 收集 (设备ID, 工具) 组合，检测同一设备下同一工具是否被重复配置。
    let mut profile_keys = HashSet::new();
    for profile in &data.profiles {
        // Profile 关联的设备必须真实存在于 device_ids 中。
        if profile.device_id.trim().is_empty() || !device_ids.contains(profile.device_id.as_str()) {
            return Err(AppError::new("error.savedData.aiConfigUnknownDevice"));
        }
        // 复用单个 Profile 的完整校验逻辑（位置范围、四种行为齐全等）。
        validate_profile(profile.clone())?;
        // insert 返回 false 说明该设备的该工具已经配置过一次，属于重复。
        if !profile_keys.insert((profile.device_id.as_str(), profile.tool)) {
            return Err(AppError::new("error.savedData.aiConfigDuplicate")
                .param("deviceId", profile.device_id.clone())
                .param("tool", ai_tool_name(profile.tool)));
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
            return Err(AppError::new("error.savedData.hooksDirectoryNotAbsolute"));
        }
    }
    Ok(())
}

/// 控制端身份会进入 URL path 和接收端租约表，仅允许紧凑的 ASCII 标识。
pub fn validate_client_id(value: &str) -> Result<&str, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AppError::new("error.savedData.clientIdInvalid"));
    }
    Ok(value)
}

/// `normalize_base_url` 的规范化结果，附带解析过程中已经确定、调用方
/// 常需要复用的主机分类，避免调用方通过字符串前缀猜测 `normalize_base_url`
/// 输出的内部格式。
pub struct NormalizedBaseUrl {
    pub value: String,
    /// 主机是否是 IPv6 字面量地址（形如 `[::1]`），供候选地址排序等场景
    /// 直接使用，无需重新解析规范化后的字符串。
    pub is_ipv6_literal: bool,
}

// 规范化并校验用户输入的设备基地址：必须是无路径、查询和用户信息的
// HTTP(S) origin。普通 IPv6 可用；链路本地 IPv6 必须依赖 zone identifier，
// 而 reqwest 0.12 无法传输，因此无论是否显式带 zone 都在持久化边界拒绝。
pub fn normalize_base_url(value: &str) -> Result<String, AppError> {
    normalize_base_url_parts(value).map(|parts| parts.value)
}

/// 与 [`normalize_base_url`] 校验规则完全一致，但同时返回解析过程中得到的
/// 主机分类，供需要它的调用方（如候选地址优先级排序）直接使用。
pub fn normalize_base_url_parts(value: &str) -> Result<NormalizedBaseUrl, AppError> {
    // 先去首尾空白，再去掉任意数量的根路径斜杠，避免后续拼接路径
    // 时出现双斜杠。内部空白仍是非法 URI。
    let normalized = value.trim().trim_end_matches('/');
    // `http::Uri` 专注 HTTP request-target，不建模 fragment，因此在解析前
    // 显式拒绝 `#`。
    if normalized.is_empty() || normalized.contains(char::is_whitespace) || normalized.contains('#')
    {
        return Err(AppError::new("error.savedData.baseUrlInvalidScheme"));
    }

    let uri = normalized.parse::<Uri>().map_err(|error| {
        AppError::new("error.savedData.baseUrlNotAUri").param("detail", error.to_string())
    })?;
    let scheme = uri
        .scheme()
        .filter(|scheme| **scheme == Scheme::HTTP || **scheme == Scheme::HTTPS)
        .ok_or_else(|| AppError::new("error.savedData.baseUrlInvalidScheme"))?;
    let authority = uri
        .authority()
        .ok_or_else(|| AppError::new("error.savedData.baseUrlMissingHost"))?;
    if authority.as_str().contains('@') || authority.host().is_empty() {
        return Err(AppError::new("error.savedData.baseUrlMissingHost"));
    }
    let host = authority.host();
    let is_ipv6_literal = host.starts_with('[');
    let is_link_local_ipv6 = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .and_then(|host| host.parse::<Ipv6Addr>().ok())
        .is_some_and(|host| host.is_unicast_link_local());
    if host.contains('%') || is_link_local_ipv6 {
        return Err(AppError::new("error.savedData.baseUrlLinkLocalUnsupported"));
    }
    // `http::Uri` 会保留无法解析为 u16 的端口文本，但对空端口段（如
    // "example.com:"）连 `authority.port()` 也视为没写端口；根据 host 后缀
    // 直接判断是否显式写了端口，才能严格拒绝空、非数字、越界或 0 端口。
    let port_suffix = &authority.as_str()[authority.host().len()..];
    if !port_suffix.is_empty() && authority.port_u16().is_none_or(|port| port == 0) {
        return Err(AppError::new("error.savedData.baseUrlInvalidPort"));
    }
    if uri.path() != "/" || uri.query().is_some() {
        return Err(AppError::new("error.savedData.baseUrlExtraComponents"));
    }

    Ok(NormalizedBaseUrl {
        value: format!("{}://{}", scheme.as_str(), authority.as_str()),
        is_ipv6_literal,
    })
}

// 将一次发现结果转换为可持久化的设备路由，同时校验 ID/名称/基地址均有效。
pub fn validate_device_route(
    device: &DiscoveredMonitorDevice,
) -> Result<MonitorDeviceRoute, AppError> {
    let device_id = device.id.trim();
    let device_name = device.name.trim();
    // ID 或名称为空说明用户还没有真正选中一个已发现的设备。
    if device_id.is_empty() || device_name.is_empty() {
        return Err(AppError::new("error.savedData.deviceNotSelected"));
    }

    Ok(MonitorDeviceRoute {
        base_url: normalize_base_url(&device.base_url)?,
        device_id: device_id.to_owned(),
        device_name: device_name.to_owned(),
    })
}

// 校验并规范化显示用户名：去空白后不能为空。
pub fn validate_username(username: &str) -> Result<String, AppError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(AppError::new("error.savedData.usernameRequired"));
    }
    Ok(username.to_owned())
}

/// 校验 Profile 是否可用于生成 Hooks 配置：位置在 1-25 之间，且四种展示
/// 行为（空闲/运行中/询问/异常）各配置一次、都选择了图片。
pub fn validate_profile(mut profile: AiProfile) -> Result<AiProfile, AppError> {
    // 先去除设备 ID 首尾空白，回写到 profile 上。
    let device_id = profile.device_id.trim().to_owned();
    profile.device_id = device_id;
    // 展示位置必须落在领域层声明的闭区间内。
    if !(MIN_PROFILE_SLOT..=MAX_PROFILE_SLOT).contains(&profile.slot) {
        return Err(AppError::new("error.savedData.slotOutOfRange")
            .param("min", MIN_PROFILE_SLOT.to_string())
            .param("max", MAX_PROFILE_SLOT.to_string()));
    }
    // 必须为领域层声明的每种展示行为各提供一条配置，多了少了都不合法。
    if profile.hooks.len() != HookBehavior::DISPLAY_BEHAVIORS.len() {
        return Err(AppError::new("error.savedData.behaviorsIncomplete"));
    }

    // 用于记录已经出现过的行为类型，检测重复配置。
    let mut behaviors = HashSet::new();
    for hook in &mut profile.hooks {
        // 去除文案和图片文件名首尾空白，回写到 hook 上。
        hook.content = hook.content.trim().to_owned();
        hook.image = hook.image.trim().to_owned();
        // 每种行为都必须选择图片，否则展示屏无法渲染。
        if hook.image.is_empty() {
            return Err(AppError::new("error.savedData.behaviorMissingImage"));
        }
        // insert 返回 false 说明该行为已经出现过一次，属于重复配置。
        if !behaviors.insert(hook.behavior) {
            return Err(AppError::new("error.savedData.behaviorDuplicate"));
        }
    }
    // 四种行为（空闲/运行中/询问/异常）必须全部出现在已配置集合中。
    if !HookBehavior::DISPLAY_BEHAVIORS
        .iter()
        .all(|behavior| behaviors.contains(behavior))
    {
        return Err(AppError::new("error.savedData.behaviorsIncomplete"));
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
            normalize_base_url(" http://192.168.1.10:8080/// ").unwrap(),
            "http://192.168.1.10:8080"
        );
        assert_eq!(
            normalize_base_url("HTTPS://Example.COM/").unwrap(),
            "https://Example.COM"
        );
    }

    #[test]
    fn base_url_accepts_ipv4_and_transportable_ipv6_origins() {
        for (input, expected) in [
            ("http://127.0.0.1", "http://127.0.0.1"),
            ("http://[fd00::20]:8080/", "http://[fd00::20]:8080"),
        ] {
            assert_eq!(normalize_base_url(input).unwrap(), expected);
        }
    }

    #[test]
    fn base_url_rejects_non_origin_components_and_invalid_authorities() {
        for invalid in [
            "",
            "ftp://example.com",
            "example.com",
            "http://",
            "http://:8080",
            "http://user@example.com",
            "http://user:password@example.com",
            "http://example.com:",
            "http://example.com:not-a-port",
            "http://example.com:65536",
            "http://example.com:0",
            "http://[fe80::1%2512]:8080",
            "http://[fe80::1]:8080",
            "http://example.com/api",
            "http://example.com?ready=true",
            "http://example.com#status",
            "http://example .com",
        ] {
            assert!(
                normalize_base_url(invalid).is_err(),
                "{invalid:?} must not be accepted as a device origin"
            );
        }
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
