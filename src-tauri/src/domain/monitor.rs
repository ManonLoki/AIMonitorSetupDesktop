// 标准库：HashSet 用于去重校验，Path 用于校验目录是否为绝对路径。
use std::collections::HashSet;
use std::path::Path;

// serde：结构体/枚举的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};
// serde_json：动态 JSON 值（Map/Value）与 json! 宏，用于生成/合并 Hooks 配置。
use serde_json::{Map, Value, json};

// 未配置设备时的占位基地址，仅用于默认值展示。
const DEFAULT_BASE_URL: &str = "http://192.168.1.100:8080";
// 本应用写入 Hooks 配置时用来标记“托管条目”的前缀，用于识别/合并/移除自身写入的内容。
const MANAGED_HOOK_PREFIX: &str = "AIMonitor";
// 本机 Hook 中继服务监听的固定端口。
pub const DEFAULT_HOOK_RELAY_PORT: u16 = 10_240;
/// 在线设备自动检查的默认间隔：启动后立即检查一次，之后每分钟刷新。
pub const DEFAULT_DISCOVERY_INTERVAL_MINUTES: u64 = 1;
// 自动检查间隔允许的最小值（分钟），避免设为 0 造成忙轮询。
pub const MIN_DISCOVERY_INTERVAL_MINUTES: u64 = 1;
// 自动检查间隔允许的最大值（分钟），避免设置过大导致长时间感知不到设备上下线。
pub const MAX_DISCOVERY_INTERVAL_MINUTES: u64 = 60;

// 与前端 TypeScript 对接的 DTO：序列化为 camelCase 字段名。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 所有设备共享的显示用户名。
    #[serde(default)]
    pub username: String,
    /// 当前 UI 选中的设备。仅用于页面上下文，不决定 Hook 转发目标。
    pub base_url: String,
    // 字段缺失时反序列化为空字符串，保持向后兼容。
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    /// 在线设备自动检查间隔（分钟）。修改后由后台发现循环下一次轮询立即生效。
    #[serde(default = "default_discovery_interval_minutes")]
    pub discovery_interval_minutes: u64,
}

// 供 serde default 属性调用，反序列化时缺省该字段则填入默认间隔。
fn default_discovery_interval_minutes() -> u64 {
    DEFAULT_DISCOVERY_INTERVAL_MINUTES
}

// 手动实现 Default，为各字段指定初始值（而不是全部用类型默认值）。
impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            username: String::new(),
            base_url: DEFAULT_BASE_URL.to_owned(),
            device_id: String::new(),
            device_name: String::new(),
            discovery_interval_minutes: DEFAULT_DISCOVERY_INTERVAL_MINUTES,
        }
    }
}

/// 校验用户在设置页填写的自动检查间隔，防止 0（忙轮询）或过大的值
/// （长时间感知不到设备上下线）。
pub fn validate_discovery_interval_minutes(minutes: u64) -> Result<u64, String> {
    // 超出 [MIN, MAX] 区间则拒绝并返回中文错误信息。
    if !(MIN_DISCOVERY_INTERVAL_MINUTES..=MAX_DISCOVERY_INTERVAL_MINUTES).contains(&minutes) {
        return Err(format!(
            "自动检查间隔必须在 {MIN_DISCOVERY_INTERVAL_MINUTES} 到 {MAX_DISCOVERY_INTERVAL_MINUTES} 分钟之间"
        ));
    }
    // 校验通过，原样返回该分钟数。
    Ok(minutes)
}

// 一条已持久化的设备路由：设备 ID/名称 + 其基地址，用于 Hook 转发目标定位。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDeviceRoute {
    pub base_url: String,
    pub device_id: String,
    pub device_name: String,
}

// 一次发现流程中找到的设备原始信息（尚未落库为路由）。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredMonitorDevice {
    pub id: String,
    pub name: String,
    pub api_version: String,
    pub base_url: String,
    pub path: String,
    // 缺失时默认视为通过 mDNS 发现。
    #[serde(default)]
    pub discovery_source: DiscoverySource,
}

/// 设备是如何被找到的；决定发现流程的信任优先级：mDNS 优先，
/// 失败后回退到 UDP 广播，再回退到已保存地址。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiscoverySource {
    // 默认来源：mDNS 局域网发现，信任优先级最高。
    #[default]
    Mdns,
    UdpBroadcast,
    SavedAddress,
}

// 应用支持接入的三种 AI 编程工具；Hash 派生用于放入 HashSet 做去重校验。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum AiTool {
    Codex,
    ClaudeCode,
    Cursor,
}

impl AiTool {
    // 遍历全部工具时使用的固定顺序数组。
    pub const ALL: [Self; 3] = [Self::Codex, Self::ClaudeCode, Self::Cursor];
}

/// AI 实例在展示屏上呈现的状态。`Idle`/`Running`/`Asking`/`Error` 是当前
/// 有效的四种展示行为（见 `DISPLAY_BEHAVIORS`），每个 Profile 必须四选四配齐。
/// AI 实例在展示屏上呈现的状态。`Idle`/`Running`/`Asking`/`Error` 是当前
/// 有效的四种展示行为（见 `DISPLAY_BEHAVIORS`），每个 Profile 必须四选四配齐。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum HookBehavior {
    Idle,
    Running,
    Asking,
    Error,
}

// 某个展示行为对应的具体内容：文案 + 图片文件名。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookContent {
    pub behavior: HookBehavior,
    // 文案允许为空，缺省填空字符串。
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image: String,
}

// 一个 AI 工具在某设备、某展示位上的完整配置。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProfile {
    /// Profile 所属的 `AIMonitor` 设备。
    #[serde(default)]
    pub device_id: String,
    pub tool: AiTool,
    /// 在展示屏上的显示位置，取值范围 1-25（校验见 `validate_profile`）。
    pub slot: u8,
    // 四种行为的内容列表；数量与内容由 validate_profile 校验。
    #[serde(default)]
    pub hooks: Vec<HookContent>,
}

// 生成的 Hooks 配置文件预览：目标文件名 + 文件内容（尚未写入磁盘）。
#[derive(Clone, Debug)]
pub struct HookConfigPreview {
    pub filename: String,
    pub content: String,
}

// 一次写入 Hooks 配置到磁盘后的结果，返回给前端展示。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigWriteResult {
    pub tool: AiTool,
    pub filename: String,
    pub config_changed: bool,
    /// 仅当写入的是 Codex 且配置发生变化时为真：Codex 不会热加载
    /// hooks.json，需要提示用户手动确认写入内容。
    pub requires_review: bool,
    /// 仅当写入的是 Codex 且配置发生变化时为真：需要提示用户重启 Codex
    /// 才能使新的 hooks 配置生效。
    pub restart_required: bool,
}

// 用户为各工具自定义的 Hooks 配置目录（为空表示使用默认目录）。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigDirectories {
    #[serde(default)]
    pub codex: String,
    #[serde(default)]
    pub claude_code: String,
    #[serde(default)]
    pub cursor: String,
}

impl HookConfigDirectories {
    // 按工具类型取出对应的自定义目录（可能为空字符串）。
    pub fn get(&self, tool: AiTool) -> &str {
        match tool {
            AiTool::Codex => &self.codex,
            AiTool::ClaudeCode => &self.claude_code,
            AiTool::Cursor => &self.cursor,
        }
    }

    // 按工具类型写入对应的自定义目录。
    pub fn set(&mut self, tool: AiTool, directory: String) {
        match tool {
            AiTool::Codex => self.codex = directory,
            AiTool::ClaudeCode => self.claude_code = directory,
            AiTool::Cursor => self.cursor = directory,
        }
    }
}

// 某工具 Hooks 配置文件的最终定位信息（目录 + 完整路径 + 是否自定义目录）。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigLocation {
    pub tool: AiTool,
    pub directory: String,
    pub config_path: String,
    pub is_custom: bool,
}

// 需要持久化到磁盘的全部监控数据：设置、设备路由、AI 配置、Hooks 目录。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedMonitorData {
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
    // 当前选中设备的基地址必须是合法格式。
    normalize_base_url(&data.settings.base_url)?;
    // 自动检查间隔必须在允许范围内。
    validate_discovery_interval_minutes(data.settings.discovery_interval_minutes)?;
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

    // 依次检查三个工具的自定义 Hooks 配置目录：非空时必须是绝对路径。
    for directory in [
        &data.hook_config_directories.codex,
        &data.hook_config_directories.claude_code,
        &data.hook_config_directories.cursor,
    ] {
        if !directory.is_empty() && !Path::new(directory).is_absolute() {
            return Err("持久化 Hooks 配置目录必须使用绝对路径".to_owned());
        }
    }
    Ok(())
}

// 返回该工具的 Hooks 配置文件名：Codex/Cursor 用 hooks.json，Claude Code 复用其 settings.json。
pub fn hook_config_filename(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex | AiTool::Cursor => "hooks.json",
        AiTool::ClaudeCode => "settings.json",
    }
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
    // 展示位置必须落在 1-25 之间。
    if !(1..=25).contains(&profile.slot) {
        return Err("显示位置必须在 1 到 25 之间".to_owned());
    }
    // 必须正好四条 hook（对应四种行为），多了少了都不合法。
    if profile.hooks.len() != 4 {
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

/// 根据 Profile 生成目标工具（Codex/Claude Code/Cursor）原生的 hooks 配置
/// 文件内容：每个原生事件只携带 Hook 类型，直接请求固定的本机中继接口。
pub fn generate_hook_config(profile: AiProfile) -> Result<HookConfigPreview, String> {
    // 生成前先完整校验一遍 Profile，确保后续逻辑可以假设数据合法。
    let profile = validate_profile(profile)?;
    // 累积每个事件名到其 handler 配置的映射。
    let mut hooks = Map::new();

    // 遍历该工具的原生“状态”事件（空闲/运行中/询问/异常对应的各个生命周期事件），
    // 为每个事件生成中继命令并写入 hooks 映射。
    for event in native_state_events(profile.tool) {
        let commands = managed_commands(profile.tool, event.name);
        insert_handler(
            &mut hooks,
            profile.tool,
            event.name,
            event.matcher,
            &commands,
        );
    }
    // 会话结束事件单独处理：不属于四种展示行为，但同样需要通知中继（用于释放/复位展示状态）。
    let session_end_event = native_session_end_event(profile.tool);
    let session_end_commands = managed_commands(profile.tool, session_end_event);
    insert_handler(
        &mut hooks,
        profile.tool,
        session_end_event,
        None,
        &session_end_commands,
    );

    // Cursor 的 hooks.json 额外要求顶层带 version 字段，其余工具不需要。
    let config = if profile.tool == AiTool::Cursor {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    } else {
        json!({ "hooks": Value::Object(hooks) })
    };
    // 各工具的 Hooks 配置文件在其项目内的相对路径。
    let filename = match profile.tool {
        AiTool::Codex => ".codex/hooks.json",
        AiTool::ClaudeCode => ".claude/settings.json",
        AiTool::Cursor => ".cursor/hooks.json",
    };

    Ok(HookConfigPreview {
        filename: filename.to_owned(),
        // 序列化为带缩进的 JSON 文本，便于用户预览/审阅。
        content: serde_json::to_string_pretty(&config)
            .map_err(|error| format!("无法生成 Hooks 配置：{error}"))?,
    })
}

/// 将生成的 hooks 配置合并进用户现有的配置文件，只替换本工具此前写入的
/// 托管条目（通过 `MANAGED_HOOK_PREFIX` 识别），保留用户手工添加的其他内容。
pub fn merge_hook_config(
    existing_content: Option<&str>,
    generated: &HookConfigPreview,
    tool: AiTool,
) -> Result<HookConfigPreview, String> {
    // 没有现有内容（文件不存在）时，以空对象作为合并起点。
    let mut existing = match existing_content {
        Some(content) => serde_json::from_str::<Value>(content)
            .map_err(|error| format!("现有 Hooks 配置格式错误：{error}"))?,
        None => json!({}),
    };
    // 解析本次新生成的配置文本为 JSON 值。
    let generated_value = serde_json::from_str::<Value>(&generated.content)
        .map_err(|error| format!("生成的 Hooks 配置格式错误：{error}"))?;
    // 两边的根节点都必须是 JSON 对象，否则无法按字段合并。
    let existing_root = existing
        .as_object_mut()
        .ok_or_else(|| "现有 Hooks 配置的根节点必须是对象".to_owned())?;
    let generated_root = generated_value
        .as_object()
        .ok_or_else(|| "生成的 Hooks 配置的根节点必须是对象".to_owned())?;

    // 除 "hooks" 外的顶层字段（比如 Cursor 的 version）只在缺失时补齐，
    // 不覆盖用户已有的自定义值。
    for (key, value) in generated_root {
        if key != "hooks" {
            existing_root
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    // 取出（或初始化）现有配置中的 hooks 对象，后续原地修改。
    let existing_hooks = existing_root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "现有配置中的 hooks 必须是对象".to_owned())?;
    // 取出本次生成的 hooks 对象，用于后续逐事件追加。
    let generated_hooks = generated_root
        .get("hooks")
        .and_then(Value::as_object)
        .ok_or_else(|| "生成的配置缺少 hooks 对象".to_owned())?;
    // 先收集现有事件名列表（避免边遍历边修改 map）。
    let existing_events: Vec<_> = existing_hooks.keys().cloned().collect();
    // 第一步：清理现有配置里属于本工具此前写入的托管条目，
    // 为后续重新插入最新生成的条目腾出位置，避免重复累积。
    for event in existing_events {
        let should_remove = existing_hooks.get_mut(&event).is_some_and(|entries| {
            // 非数组的事件条目原样保留，不做处理。
            let Some(entries) = entries.as_array_mut() else {
                return false;
            };
            // 从该事件的条目数组中移除所有带本工具托管标记的条目。
            remove_managed_entries(entries, tool);
            // 清理后数组为空，说明该事件下已经没有条目了，可以整体移除该事件键。
            entries.is_empty()
        });
        if should_remove {
            existing_hooks.remove(&event);
        }
    }

    // 第二步：把本次新生成的每个事件条目追加进现有配置对应事件的数组末尾。
    for (event, generated_entries) in generated_hooks {
        let generated_entries = generated_entries
            .as_array()
            .ok_or_else(|| format!("生成的 {event} 配置必须是数组"))?;
        // 若现有配置里还没有这个事件，先插入一个空数组。
        let existing_entries = existing_hooks
            .entry(event.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("现有配置中的 {event} 必须是数组"))?;

        // 追加生成的条目（clone 是因为 generated_entries 只是借用）。
        existing_entries.extend(generated_entries.iter().cloned());
    }

    Ok(HookConfigPreview {
        filename: generated.filename.clone(),
        content: serde_json::to_string_pretty(&existing)
            .map_err(|error| format!("无法生成合并后的 Hooks 配置：{error}"))?,
    })
}

impl HookBehavior {
    // 当前所有有效的展示行为，顺序固定，供 validate_profile 做“四选四”检查。
    const DISPLAY_BEHAVIORS: [Self; 4] = [Self::Idle, Self::Running, Self::Asking, Self::Error];
}

// 工具原生生命周期事件与展示行为的映射：事件名、可选匹配器（用于区分同名事件的子类型）、
// 触发后应切换到的展示行为。
struct NativeStateEvent {
    name: &'static str,
    matcher: Option<&'static str>,
    behavior: HookBehavior,
}

// 按工具类型分发到各自的原生事件表。
fn native_state_events(tool: AiTool) -> Vec<NativeStateEvent> {
    match tool {
        AiTool::Cursor => cursor_state_events(),
        AiTool::ClaudeCode => claude_state_events(),
        AiTool::Codex => codex_state_events(),
    }
}

// Cursor 的原生事件到展示行为映射表（Cursor 事件名为 camelCase）。
fn cursor_state_events() -> Vec<NativeStateEvent> {
    state_events(&[
        ("workspaceOpen", HookBehavior::Idle),
        ("sessionStart", HookBehavior::Idle),
        ("beforeSubmitPrompt", HookBehavior::Running),
        ("afterFileEdit", HookBehavior::Running),
        ("afterShellExecution", HookBehavior::Running),
        ("afterMCPExecution", HookBehavior::Running),
        ("beforeShellExecution", HookBehavior::Asking),
        ("beforeMCPExecution", HookBehavior::Asking),
        ("preToolUse", HookBehavior::Running),
        ("postToolUseFailure", HookBehavior::Error),
        ("stop", HookBehavior::Idle),
    ])
}

// Claude Code 的原生事件到展示行为映射表（事件名为 PascalCase）。
fn claude_state_events() -> Vec<NativeStateEvent> {
    let mut events = state_events(&[
        ("SessionStart", HookBehavior::Idle),
        ("UserPromptSubmit", HookBehavior::Running),
        ("PreToolUse", HookBehavior::Running),
        ("PostToolUse", HookBehavior::Running),
        ("PermissionRequest", HookBehavior::Asking),
        ("Elicitation", HookBehavior::Asking),
        ("PostToolUseFailure", HookBehavior::Error),
        ("Stop", HookBehavior::Idle),
        ("StopFailure", HookBehavior::Error),
        ("SubagentStart", HookBehavior::Running),
        ("SubagentStop", HookBehavior::Running),
        ("PreCompact", HookBehavior::Running),
        ("PostCompact", HookBehavior::Running),
    ]);
    // Stop is the primary end-of-turn signal. Claude does not emit it for every
    // termination path, so idle_prompt provides a second authoritative signal
    // that the whole session is waiting for user input. In particular, this
    // prevents a late SubagentStop update from leaving the slot running.
    // 额外追加一条带 matcher 的 Notification 事件，作为 Stop 之外的第二重“空闲”信号来源。
    events.push(NativeStateEvent {
        name: "Notification",
        matcher: Some("idle_prompt"),
        behavior: HookBehavior::Idle,
    });
    events
}

// Codex 的原生事件到展示行为映射表（事件名为 PascalCase，不含 Error 独立事件）。
fn codex_state_events() -> Vec<NativeStateEvent> {
    state_events(&[
        ("SessionStart", HookBehavior::Idle),
        ("UserPromptSubmit", HookBehavior::Running),
        ("PreToolUse", HookBehavior::Running),
        ("PostToolUse", HookBehavior::Running),
        ("PermissionRequest", HookBehavior::Asking),
        ("Stop", HookBehavior::Idle),
        ("SubagentStart", HookBehavior::Running),
        ("SubagentStop", HookBehavior::Running),
        ("PreCompact", HookBehavior::Running),
        ("PostCompact", HookBehavior::Running),
    ])
}

// 将 (事件名, 行为) 元组数组批量转换为 NativeStateEvent 列表，matcher 统一置空。
fn state_events(events: &[(&'static str, HookBehavior)]) -> Vec<NativeStateEvent> {
    events
        .iter()
        .map(|(name, behavior)| NativeStateEvent {
            name,
            matcher: None,
            behavior: *behavior,
        })
        .collect()
}

// 会话结束事件名：Cursor 用 camelCase 的 "sessionEnd"，其余工具用 PascalCase 的 "SessionEnd"。
fn native_session_end_event(tool: AiTool) -> &'static str {
    if tool == AiTool::Cursor {
        "sessionEnd"
    } else {
        "SessionEnd"
    }
}

// 一个 Hook 事件触发后，展示屏应执行的动作：切换到某个行为展示，或者释放（退出）该展示位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookTransition {
    Display(HookBehavior),
    Release,
}

// 根据工具类型和原生事件名，计算该事件对应的展示状态迁移。
pub fn hook_transition(tool: AiTool, event: &str) -> Option<HookTransition> {
    if event == native_session_end_event(tool) {
        // Codex 的会话结束事件仍视为“回到空闲”展示，而不是直接释放展示位；
        // 其余工具的会话结束则意味着释放该展示位。
        return Some(if tool == AiTool::Codex {
            HookTransition::Display(HookBehavior::Idle)
        } else {
            HookTransition::Release
        });
    }

    // 在该工具的原生状态事件表中查找匹配的事件名，取其对应展示行为。
    native_state_events(tool)
        .into_iter()
        .find(|candidate| candidate.name == event)
        .map(|candidate| HookTransition::Display(candidate.behavior))
}

// 判断某个事件是否为“权威性”的终结事件——出现时可以确信当前操作已经结束，
// 可以覆盖掉可能滞后到达的其他“运行中”事件。
pub fn is_authoritative_terminal_event(tool: AiTool, event: &str) -> bool {
    event == native_session_end_event(tool)
        || matches!(event, "Stop" | "stop" | "Notification" | "StopFailure")
}

// 判断某个事件是否可能“滞后完成”——即该事件可能在真正的终结事件之后才到达，
// 因此不应该用它覆盖掉更晚出现的权威终结状态。
pub fn is_late_completion_event(event: &str) -> bool {
    matches!(
        event,
        "PostToolUse"
            | "SubagentStop"
            | "PostCompact"
            | "afterFileEdit"
            | "afterShellExecution"
            | "afterMCPExecution"
            | "afterAgentResponse"
            | "afterAgentThought"
            | "postToolUse"
    )
}

// 同一个中继调用的两种平台实现：POSIX shell 命令，以及 Windows 下的 PowerShell 命令。
struct ManagedCommands {
    posix: String,
    windows: String,
}

// 为某个事件构造该工具原生格式的 handler JSON，并写入 hooks 映射。
fn insert_handler(
    hooks: &mut Map<String, Value>,
    tool: AiTool,
    event: &str,
    matcher: Option<&str>,
    commands: &ManagedCommands,
) {
    let handler = match tool {
        // Cursor：条目是一个只含 command 字段的数组。
        AiTool::Cursor => json!([{ "command": platform_command(commands) }]),
        AiTool::ClaudeCode => {
            // Claude Code：外层是 hooks 数组，内层每项是 { type, command }。
            let mut group = json!({
                "hooks": [{
                    "type": "command",
                    "command": platform_command(commands)
                }]
            });
            // 若该事件需要按 matcher 区分子类型（例如 Notification 的 idle_prompt），追加该字段。
            if let Some(matcher) = matcher {
                group["matcher"] = Value::String(matcher.to_owned());
            }
            Value::Array(vec![group])
        }
        // Codex：同时写入 POSIX 和 Windows 两条命令，由 Codex 运行时按平台选用。
        AiTool::Codex => json!([{
            "hooks": [{
                "type": "command",
                "command": commands.posix,
                "commandWindows": commands.windows
            }]
        }]),
    };
    hooks.insert(event.to_owned(), handler);
}

// Windows 平台编译时，Cursor/Claude Code 使用 PowerShell 命令。
#[cfg(windows)]
fn platform_command(commands: &ManagedCommands) -> &str {
    &commands.windows
}

// 非 Windows 平台编译时，Cursor/Claude Code 使用 POSIX shell 命令。
#[cfg(not(windows))]
fn platform_command(commands: &ManagedCommands) -> &str {
    &commands.posix
}

// 从某事件的条目数组中原地移除所有属于本工具的托管条目。
fn remove_managed_entries(entries: &mut Vec<Value>, tool: AiTool) {
    if tool == AiTool::Cursor {
        // Cursor 条目本身就是扁平的 { command } 对象，直接按是否托管过滤。
        entries.retain(|entry| !entry_is_managed(entry, tool));
        return;
    }

    // Claude Code / Codex 条目是分组结构，每组内嵌一个 hooks 数组；
    // 先过滤掉组内被标记为托管的 handler，若过滤后该组已空则整组移除。
    entries.retain_mut(|group| {
        let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            // 没有 hooks 数组的异常结构，保守起见原样保留。
            return true;
        };
        handlers.retain(|handler| !entry_is_managed(handler, tool));
        !handlers.is_empty()
    });
}

// 判断某个条目是否带有本工具的托管标记：检查 command / commandWindows 字段内容。
fn entry_is_managed(entry: &Value, tool: AiTool) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .any(|command| command_has_marker(command, &managed_hook_marker(tool)))
}

// 为指定工具和事件生成 POSIX / Windows 两个版本的中继调用命令：
// 命令内嵌入一个带工具标记的注释/变量（用于识别本应用写入的托管条目），
// 然后通过 curl / Invoke-RestMethod 把事件类型 POST 给本机 Hook 中继服务。
fn managed_commands(tool: AiTool, event: &str) -> ManagedCommands {
    // 该工具专属的托管标记字符串，写入命令中便于日后识别/清理。
    let marker = managed_hook_marker(tool);
    // 请求体固定为 { "type": <event> }，序列化失败视为不可能发生的内部错误。
    let payload = serde_json::to_string(&json!({ "type": event }))
        .expect("fixed Hook event payload must serialize");
    // 本机 Hook 中继服务的完整 URL，路径按工具区分。
    let url = format!(
        "http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/{}",
        ai_tool_slug(tool)
    );
    // POSIX shell 命令主体：先用 `: '标记'` 这种 no-op 写法内嵌标记，
    // 再用 curl 发起短超时的 POST 请求。
    let posix_marked = format!(
        ": {}; curl --silent --show-error --fail --connect-timeout 1 --max-time 3 \
         --request POST --header 'Content-Type: application/json' --data-binary {} {}",
        shell_quote(&marker),
        shell_quote(&payload),
        shell_quote(&url),
    );
    let posix = match tool {
        // Cursor 期望 hook 脚本的 stdout 是合法 JSON，因此丢弃 curl 输出后补一个空对象。
        AiTool::Cursor => format!("{posix_marked} >/dev/null && printf '{{}}'"),
        // Codex Desktop and Claude Code interpret hook stdout as protocol
        // output. The monitor response is transport data, not hook feedback.
        // 这两个工具会把 stdout 当协议数据解析，因此直接丢弃 curl 输出。
        AiTool::Codex | AiTool::ClaudeCode => format!("{posix_marked} >/dev/null"),
    };
    // Windows 版本使用 PowerShell：同样先内嵌标记变量，再用 Invoke-RestMethod 发起 POST。
    let mut windows_script = format!(
        "$null = '{}'; $ProgressPreference = 'SilentlyContinue'; \
         Invoke-RestMethod -Uri '{}' -Method Post -ContentType 'application/json' \
         -Body '{}' -TimeoutSec 3 | Out-Null",
        powershell_quote(&marker),
        powershell_quote(&url),
        powershell_quote(&payload),
    );
    // Cursor 在 Windows 下同样需要输出一个空 JSON 对象作为 hook 返回值。
    if tool == AiTool::Cursor {
        windows_script.push_str("; Write-Output '{}'");
    }
    // 用 -EncodedCommand 传参，避免脚本里的引号/特殊字符被外层 shell 破坏。
    let windows = format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encode_powershell_command(&windows_script)
    );
    ManagedCommands { posix, windows }
}

// 拼出某工具专属的托管标记字符串，形如 "AIMonitor|tool=codex"。
fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}|tool={}", ai_tool_slug(tool))
}

// 工具在 URL 路径 / 标记字符串中使用的小写短标识。
const fn ai_tool_slug(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex => "codex",
        AiTool::ClaudeCode => "claude-code",
        AiTool::Cursor => "cursor",
    }
}

// 工具的展示用中文/英文名称（用于错误提示等 UI 文案）。
pub const fn ai_tool_name(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex => "Codex",
        AiTool::ClaudeCode => "Claude Code",
        AiTool::Cursor => "Cursor",
    }
}

// 按 POSIX shell 单引号规则转义字符串：把内部单引号替换为 '"'"' 拼接技巧。
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

// 按 PowerShell 单引号字符串规则转义：内部单引号加倍。
fn powershell_quote(value: &str) -> String {
    value.replace('\'', "''")
}

// 判断某条命令字符串（可能是明文 POSIX 命令，也可能是 Base64 编码的 PowerShell 命令）
// 是否包含给定标记，且标记后紧跟单引号（避免误匹配到标记的前缀子串）。
fn command_has_marker(command: &str, marker: &str) -> bool {
    decoded_hook_command(command).is_some_and(|decoded| {
        decoded
            .match_indices(marker)
            .any(|(start, _)| decoded[start + marker.len()..].starts_with('\''))
    })
}

// 尽量得到命令的“可读”文本：明文命令直接返回；PowerShell -EncodedCommand
// 编码的命令则解码 Base64 再按 UTF-16LE 还原为字符串，用于后续标记匹配。
fn decoded_hook_command(command: &str) -> Option<String> {
    // 命令里已经能直接看到托管前缀，说明是明文（POSIX）命令，无需解码。
    if command.contains(MANAGED_HOOK_PREFIX) {
        return Some(command.to_owned());
    }
    // 从 "-EncodedCommand " 之后取出紧跟着的一段（到下一个空白为止）作为 Base64 串。
    let encoded = command
        .split_once("-EncodedCommand ")
        .map(|(_, encoded)| encoded.split_whitespace().next().unwrap_or(""))?;
    let bytes = decode_base64(encoded)?;
    // PowerShell -EncodedCommand 使用 UTF-16LE，字节数必须是偶数。
    if bytes.len() % 2 != 0 {
        return None;
    }
    // 按小端序两两组合还原为 UTF-16 code unit 序列。
    let utf16: Vec<_> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&utf16).ok()
}

// 把 PowerShell 脚本编码为 -EncodedCommand 所需的格式：UTF-16LE 字节后再 Base64 编码。
fn encode_powershell_command(script: &str) -> String {
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    encode_base64(&bytes)
}

// 标准 Base64 编码实现（自实现而非依赖第三方库）：每 3 字节输入编码为 4 个输出字符。
pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    // 标准 Base64 字符表（不含 URL-safe 变体）。
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // 预分配容量：每 3 字节输入产生 4 字节输出，向上取整分组数。
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    // 按 3 字节一组处理输入（最后一组可能不足 3 字节）。
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        // 分组不足 3/2 字节时，缺失的字节按 0 处理（真正的截断由后面 '=' 填充体现）。
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        // 第一个输出字符：取第一字节的高 6 位。
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        // 第二个输出字符：第一字节低 2 位 + 第二字节高 4 位。
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0b11) << 4) | (second >> 4))],
        ));
        // 第三个输出字符：分组含第二字节时才有效，否则用 '=' 填充。
        encoded.push(if chunk.len() > 1 {
            char::from(ALPHABET[usize::from(((second & 0b1111) << 2) | (third >> 6))])
        } else {
            '='
        });
        // 第四个输出字符：分组含第三字节时才有效，否则用 '=' 填充。
        encoded.push(if chunk.len() > 2 {
            char::from(ALPHABET[usize::from(third & 0b11_1111)])
        } else {
            '='
        });
    }
    encoded
}

// 标准 Base64 解码实现，仅供本文件内部用于还原 -EncodedCommand 内容。
fn decode_base64(value: &str) -> Option<Vec<u8>> {
    // 把单个 Base64 字符换算成对应的 6 位数值；非法字符返回 None。
    fn sextet(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let bytes = value.as_bytes();
    // 合法 Base64 文本长度必须是 4 的倍数，且不能为空。
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    // 每 4 个字符一组解码，还原出最多 3 个原始字节。
    for chunk in bytes.chunks_exact(4) {
        let a = sextet(chunk[0])?;
        let b = sextet(chunk[1])?;
        // 第三、四个字符可能是 '=' 填充，此时对应位置不解码为有效 sextet。
        let c = (chunk[2] != b'=').then(|| sextet(chunk[2])).flatten();
        let d = (chunk[3] != b'=').then(|| sextet(chunk[3])).flatten();
        // 第一个输出字节：由 a 的全部 6 位与 b 的高 2 位组成。
        decoded.push((a << 2) | (b >> 4));
        if let Some(c) = c {
            // 第二个输出字节：由 b 的低 4 位与 c 的高 4 位组成。
            decoded.push((b << 4) | (c >> 2));
            if let Some(d) = d {
                // 第三个输出字节：由 c 的低 2 位与 d 的全部 6 位组成。
                decoded.push((c << 6) | d);
            } else if chunk[3] != b'=' {
                // d 既不是合法 sextet 也不是 '='，说明是非法字符，解码失败。
                return None;
            }
        } else if chunk[2] != b'=' || chunk[3] != b'=' {
            // c 不是合法 sextet 但第三、四位不全是 '='，说明填充格式非法。
            return None;
        }
    }
    Some(decoded)
}

/// 长边超过该像素数的上传图片会被等比缩小到该尺寸以内。
pub const MAX_UPLOAD_IMAGE_EDGE: u32 = 800;
// 重新编码 JPEG 时使用的固定质量参数（0-100，值越大质量越高、体积越大）。
const UPLOAD_JPEG_QUALITY: u8 = 82;

/// 缩放并压缩待上传的 JPEG/PNG 图片，降低展示屏（Android 端）解码大图的压力：
/// 任一边超过 [`MAX_UPLOAD_IMAGE_EDGE`] 时等比缩小到该尺寸以内；未超限的图片
/// 仍会重新编码以获得更小的体积，若重新编码后反而更大则保留原始字节。
///
/// GIF 原样返回：`image` 解码 GIF 只保留第一帧，重新编码会丢失动画。
pub fn resize_and_compress_image(bytes: &[u8], mime_type: &str) -> Result<Vec<u8>, String> {
    // 只处理 JPEG/PNG；其他类型（例如 GIF）原样返回，不做解码重编码。
    let format = match mime_type {
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/png" => image::ImageFormat::Png,
        _ => return Ok(bytes.to_vec()),
    };

    // 按声明的格式解码图片；解码失败说明数据损坏或格式不匹配。
    let decoded = image::load_from_memory_with_format(bytes, format)
        .map_err(|error| format!("图片解码失败：{error}"))?;
    // 任一边超过上限就需要缩放。
    let needs_resize =
        decoded.width() > MAX_UPLOAD_IMAGE_EDGE || decoded.height() > MAX_UPLOAD_IMAGE_EDGE;
    let image = if needs_resize {
        // 使用 Lanczos3 滤波器等比缩放，使长边不超过 MAX_UPLOAD_IMAGE_EDGE。
        decoded.resize(
            MAX_UPLOAD_IMAGE_EDGE,
            MAX_UPLOAD_IMAGE_EDGE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        // 未超限则保持原尺寸，后面仍会重新编码一次尝试压缩体积。
        decoded
    };

    let mut output = Vec::new();
    match format {
        image::ImageFormat::Jpeg => {
            // 按固定质量参数重新编码为 JPEG。
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                &mut output,
                UPLOAD_JPEG_QUALITY,
            );
            image
                .write_with_encoder(encoder)
                .map_err(|error| format!("图片压缩失败：{error}"))?;
        }
        image::ImageFormat::Png => {
            // PNG 使用最高压缩级别 + 自适应滤波器，尽量减小体积。
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut output,
                image::codecs::png::CompressionType::Best,
                image::codecs::png::FilterType::Adaptive,
            );
            image
                .write_with_encoder(encoder)
                .map_err(|error| format!("图片压缩失败：{error}"))?;
        }
        // format 在函数开头已被限定为 Jpeg 或 Png 之一，其他分支不可达。
        _ => unreachable!("format 只能是 Jpeg 或 Png"),
    }

    // 没有缩放且重新编码后体积没有变小，则保留原始字节，避免做无意义的替换。
    if !needs_resize && output.len() >= bytes.len() {
        return Ok(bytes.to_vec());
    }
    Ok(output)
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use serde_json::Value;

    // 引入被测的类型与函数（父模块中的公开/私有项）。
    use super::{
        AiProfile, AiTool, DEFAULT_BASE_URL, DEFAULT_DISCOVERY_INTERVAL_MINUTES, HookBehavior,
        HookConfigDirectories, HookConfigPreview, HookContent, HookTransition, MANAGED_HOOK_PREFIX,
        MAX_DISCOVERY_INTERVAL_MINUTES, MAX_UPLOAD_IMAGE_EDGE, MonitorDeviceRoute, MonitorSettings,
        SavedMonitorData, command_has_marker, decoded_hook_command, generate_hook_config,
        hook_transition, managed_hook_marker, merge_hook_config, normalize_base_url,
        resize_and_compress_image, validate_discovery_interval_minutes, validate_profile,
        validate_saved_monitor_data, validate_username,
    };

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

    // 验证自动检查间隔的默认值为 1 分钟，且 0 和超过上限的值都会被拒绝，
    // 默认值本身则应该通过校验。
    #[test]
    fn discovery_interval_defaults_to_one_minute_and_rejects_out_of_range_values() {
        assert_eq!(
            MonitorSettings::default().discovery_interval_minutes,
            DEFAULT_DISCOVERY_INTERVAL_MINUTES
        );
        assert_eq!(DEFAULT_DISCOVERY_INTERVAL_MINUTES, 1);
        // 0 属于忙轮询，应当被拒绝。
        assert!(validate_discovery_interval_minutes(0).is_err());
        // 超过最大值同样应当被拒绝。
        assert!(validate_discovery_interval_minutes(MAX_DISCOVERY_INTERVAL_MINUTES + 1).is_err());
        // 默认值本身应当合法。
        assert_eq!(
            validate_discovery_interval_minutes(DEFAULT_DISCOVERY_INTERVAL_MINUTES).unwrap(),
            DEFAULT_DISCOVERY_INTERVAL_MINUTES
        );
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

    // 验证 Cursor 生成的 Hooks 配置：事件名使用 camelCase，包含预期事件，
    // 不含 Claude/Codex 专属事件，且中继调用格式正确（带 printf 空对象输出）。
    #[test]
    fn cursor_preview_uses_cursor_event_names_and_shape() {
        let preview = generate_hook_config(profile(AiTool::Cursor)).unwrap();

        assert_eq!(preview.filename, ".cursor/hooks.json");
        assert!(preview.content.contains("\"beforeSubmitPrompt\""));
        assert!(preview.content.contains("\"beforeShellExecution\""));
        assert!(preview.content.contains("\"beforeMCPExecution\""));
        assert!(preview.content.contains("\"afterFileEdit\""));
        assert!(preview.content.contains("\"workspaceOpen\""));
        // Cursor 事件表中没有 postToolUse，确认未被误写入。
        assert!(!preview.content.contains("\"postToolUse\""));
        assert!(preview.content.contains("\"sessionEnd\""));
        assert!(preview.content.contains("127.0.0.1:10240/api/hooks/cursor"));
        assert!(preview.content.contains("AIMonitor|tool=cursor"));
        // Cursor 需要 stdout 输出空 JSON 对象。
        assert!(preview.content.contains("printf '{}'"));
        // 不应出现 Claude Code 专属的 Notification 事件（小写形式）。
        assert!(!preview.content.contains("\"notification\""));
        // Cursor 的条目结构没有 "type": "command" 字段（这是 Claude/Codex 的结构）。
        assert!(!preview.content.contains("\"type\": \"command\""));
    }

    // 验证 Claude Code 生成的 Hooks 配置：事件名使用 PascalCase，覆盖权限/生命周期事件，
    // 且 Notification 事件带有 idle_prompt matcher，而普通事件（如 SessionStart）没有 matcher。
    #[test]
    fn claude_preview_covers_permission_and_lifecycle_events() {
        let preview = generate_hook_config(profile(AiTool::ClaudeCode)).unwrap();

        assert_eq!(preview.filename, ".claude/settings.json");
        assert!(preview.content.contains("\"SessionStart\""));
        assert!(preview.content.contains("\"PermissionRequest\""));
        assert!(preview.content.contains("\"Elicitation\""));
        assert!(preview.content.contains("\"PostToolUse\""));
        assert!(preview.content.contains("\"PostToolUseFailure\""));
        assert!(preview.content.contains("\"StopFailure\""));
        assert!(preview.content.contains("\"SessionEnd\""));
        assert!(preview.content.contains("\"Notification\""));
        assert!(preview.content.contains("AIMonitor|tool=claude-code"));
        // Claude Code 会把 stdout 当协议数据解析，命令输出必须被丢弃。
        assert!(preview.content.contains(">/dev/null"));
        let value: Value = serde_json::from_str(&preview.content).unwrap();
        // Notification 事件应带有 idle_prompt matcher，用于区分具体子类型。
        assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
        // 普通事件不应携带 matcher 字段。
        assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
    }

    // 验证 Codex 生成的 Hooks 配置：事件名使用 PascalCase 且没有独立 Error 事件；
    // 每个 handler 同时包含 POSIX 命令（command）和 Windows 命令（commandWindows，
    // 以 -EncodedCommand 方式编码），且都能被正确解码、包含中继地址与托管标记。
    #[test]
    fn codex_preview_uses_pascal_case_and_nested_handlers() {
        let preview = generate_hook_config(profile(AiTool::Codex)).unwrap();

        assert_eq!(preview.filename, ".codex/hooks.json");
        assert!(preview.content.contains("\"SessionStart\""));
        assert!(preview.content.contains("\"UserPromptSubmit\""));
        assert!(preview.content.contains("\"PermissionRequest\""));
        // Codex 没有独立的 "Error" 事件（异常状态通过其他事件间接体现）。
        assert!(!preview.content.contains("\"Error\""));
        assert!(preview.content.contains("\"PostToolUse\""));
        assert!(preview.content.contains("\"SessionEnd\""));
        assert!(preview.content.contains(">/dev/null"));
        assert!(preview.content.contains("AIMonitor|tool=codex"));
        assert!(preview.content.contains("\"type\": \"command\""));
        assert!(preview.content.contains("\"commandWindows\""));
        assert!(preview.content.contains("powershell.exe"));
        assert!(preview.content.contains("-EncodedCommand"));
        let value: Value = serde_json::from_str(&preview.content).unwrap();
        // 取出 SessionEnd 的 POSIX 命令，确认其中携带中继地址和事件名。
        let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(session_end.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(session_end.contains("SessionEnd"));
        // 取出 Stop 的 Windows 命令（编码后的 PowerShell），解码后验证内容正确。
        let windows = value["hooks"]["Stop"][0]["hooks"][0]["commandWindows"]
            .as_str()
            .unwrap();
        let decoded = decoded_hook_command(windows).unwrap();
        assert!(decoded.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(decoded.contains(MANAGED_HOOK_PREFIX));
        // command_has_marker 应能在未解码的编码命令上直接识别出托管标记。
        assert!(command_has_marker(
            windows,
            &managed_hook_marker(AiTool::Codex)
        ));
    }

    // 验证 Codex 的合并逻辑是幂等的：重复合并不会让托管条目累积，
    // 且用户手工添加的其他命令、其他顶层字段（如 permissions）会被保留。
    #[test]
    fn codex_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(profile(AiTool::Codex)).unwrap();
        // 第一次合并：从空配置开始生成初始文件。
        let first = merge_hook_config(None, &generated, AiTool::Codex).unwrap();
        let mut value: Value = serde_json::from_str(&first.content).unwrap();
        // 模拟用户手工添加的其他顶层字段和 Stop 事件的其他命令。
        value["permissions"] = serde_json::json!({ "allow": ["Bash"] });
        value["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "hooks": [{ "type": "command", "command": "other-app notify" }]
            }));
        let existing = serde_json::to_string_pretty(&value).unwrap();
        // 第二次合并：应当只替换本工具的托管条目，不影响用户添加的内容。
        let merged = merge_hook_config(Some(&existing), &generated, AiTool::Codex).unwrap();
        let value: Value = serde_json::from_str(&merged.content).unwrap();
        let stop = value["hooks"]["Stop"].as_array().unwrap();
        let serialized = serde_json::to_string(stop).unwrap();

        // 用户的 "other-app notify" 命令应恰好保留一份，没有被重复或删除。
        assert_eq!(serialized.matches("other-app notify").count(), 1);
        // 托管标记应恰好出现一次，说明没有累积重复的托管条目。
        assert_eq!(serialized.matches(MANAGED_HOOK_PREFIX).count(), 1);
        // 用户手工添加的 permissions 字段应被完整保留。
        assert_eq!(value["permissions"]["allow"][0], "Bash");
    }

    // 验证 Cursor 的合并逻辑同样幂等，且能正确保留用户命令、去重已有的托管条目。
    #[test]
    fn cursor_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(profile(AiTool::Cursor)).unwrap();
        // 预置一份现有配置：包含用户命令和一条旧格式的托管条目。
        let existing = r#"{
          "version": 1,
          "hooks": {
            "stop": [
              { "command": "other-app stop" },
              { "command": ": 'AIMonitor|tool=cursor'; curl current" }
            ]
          }
        }"#;

        // 连续合并两次，验证第二次不会让内容继续累积。
        let first = merge_hook_config(Some(existing), &generated, AiTool::Cursor).unwrap();
        let second = merge_hook_config(Some(&first.content), &generated, AiTool::Cursor).unwrap();
        let value: Value = serde_json::from_str(&second.content).unwrap();
        let stop = serde_json::to_string(&value["hooks"]["stop"]).unwrap();

        assert_eq!(stop.matches("other-app stop").count(), 1);
        assert_eq!(stop.matches("AIMonitor|tool=cursor").count(), 1);
    }

    // 验证当现有配置的根结构不合法（hooks 应为对象却是数组）时，合并应返回错误而不是崩溃。
    #[test]
    fn merge_rejects_an_invalid_existing_config() {
        let generated = HookConfigPreview {
            filename: ".cursor/hooks.json".to_owned(),
            content: r#"{"version":1,"hooks":{}}"#.to_owned(),
        };

        assert!(merge_hook_config(Some(r#"{"hooks":[]}"#), &generated, AiTool::Cursor).is_err());
    }

    // 验证生成的命令直接请求稳定的本机中继地址，且不包含历史上淘汰的实现方式
    // （旧的 curl 重试参数、独立脚本文件、使用默认基地址、直接携带 behavior 字段）。
    #[test]
    fn hook_commands_post_directly_to_the_stable_local_relay() {
        let preview = generate_hook_config(profile(AiTool::Codex)).unwrap();

        assert!(preview.content.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(preview.content.contains(r#"\"type\":\"SessionStart\""#));
        assert!(!preview.content.contains("--retry"));
        assert!(!preview.content.contains("aimonitor-hook.sh"));
        assert!(!preview.content.contains("aimonitor-hook.ps1"));
        assert!(!preview.content.contains(DEFAULT_BASE_URL));
        assert!(!preview.content.contains("\"behavior\":\"running\""));
    }

    // 验证生成的 Hooks 配置只依赖 slot/工具类型等结构性字段，
    // 修改展示文案/图片（不改变 slot 以外的结构）不会改变生成结果，
    // 从而保证仅仅编辑显示内容不需要重新写入 Hooks 配置文件。
    #[test]
    fn hook_config_is_identical_when_display_content_changes() {
        let first = profile(AiTool::Codex);
        let mut second = first.clone();
        second.slot = 23;
        second.hooks[0].content = "完全不同的文案".to_owned();
        second.hooks[0].image = "another-idle.png".to_owned();

        assert_eq!(
            generate_hook_config(first).unwrap().content,
            generate_hook_config(second).unwrap().content
        );
    }

    // 验证 hook_transition 对不同工具、不同事件名能返回正确的状态迁移：
    // Claude 的 Notification 对应 Idle 展示，Codex 的 PermissionRequest 对应 Asking 展示，
    // Cursor 的 sessionEnd 对应释放展示位，未知事件返回 None。
    #[test]
    fn hook_transitions_keep_state_rules_in_the_desktop_backend() {
        assert_eq!(
            hook_transition(AiTool::ClaudeCode, "Notification"),
            Some(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            hook_transition(AiTool::Codex, "PermissionRequest"),
            Some(HookTransition::Display(HookBehavior::Asking))
        );
        assert_eq!(
            hook_transition(AiTool::Cursor, "sessionEnd"),
            Some(HookTransition::Release)
        );
        assert_eq!(hook_transition(AiTool::Codex, "Unknown"), None);
    }

    // 测试辅助函数：生成指定宽高、指定格式的纯色测试图片字节数据。
    fn encode_test_image(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
        let image = image::DynamicImage::new_rgb8(width, height);
        let mut bytes = Vec::new();
        image
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .unwrap();
        bytes
    }

    // 验证超过上限尺寸的 JPEG 图片会被等比缩小，长边缩到 MAX_UPLOAD_IMAGE_EDGE，
    // 短边按原图宽高比例同步缩小。
    #[test]
    fn oversized_jpeg_upload_is_scaled_to_max_edge() {
        let source = encode_test_image(2000, 500, image::ImageFormat::Jpeg);

        let processed = resize_and_compress_image(&source, "image/jpeg").unwrap();

        let decoded =
            image::load_from_memory_with_format(&processed, image::ImageFormat::Jpeg).unwrap();
        assert_eq!(decoded.width(), MAX_UPLOAD_IMAGE_EDGE);
        assert_eq!(decoded.height(), 200);
    }

    // 验证未超限的小尺寸 PNG 图片在处理后尺寸保持不变。
    #[test]
    fn small_png_upload_keeps_its_dimensions() {
        let source = encode_test_image(100, 50, image::ImageFormat::Png);

        let processed = resize_and_compress_image(&source, "image/png").unwrap();

        let decoded =
            image::load_from_memory_with_format(&processed, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (100, 50));
    }

    // 验证 GIF 图片原样透传，不做任何解码/重新编码（避免丢失动画帧）。
    #[test]
    fn gif_upload_passes_through_unchanged() {
        let source = b"not really a gif but bytes are opaque here".to_vec();

        let processed = resize_and_compress_image(&source, "image/gif").unwrap();

        assert_eq!(processed, source);
    }

    // 验证损坏的 JPEG 数据在解码阶段会被拒绝并返回错误，而不是 panic。
    #[test]
    fn corrupt_jpeg_upload_is_rejected() {
        let result = resize_and_compress_image(b"not a jpeg", "image/jpeg");

        assert!(result.is_err());
    }
}
