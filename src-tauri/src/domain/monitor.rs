// 标准库：HashMap/HashSet 用于生命周期聚合与去重校验，Path 用于校验目录。
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

// serde：结构体/枚举的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};
mod hooks;

use hooks::{HookEventKind, event_kind};
#[cfg(test)]
use hooks::{
    MANAGED_HOOK_PREFIX, command_has_marker, decoded_hook_command, hook_transition,
    managed_hook_marker,
};
pub use hooks::{
    ai_tool_name, generate_hook_auxiliary_configs, generate_hook_config, hook_config_filename,
    hook_requires_review, hook_restart_required, merge_hook_config, tool_from_slug,
};

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
    /// 设置页选中的 AI 客户端；监控管理与 Hooks 管理共用这份可见范围。
    #[serde(default = "default_enabled_ai_tools")]
    pub enabled_ai_tools: Vec<AiTool>,
}

// 供 serde default 属性调用，反序列化时缺省该字段则填入默认间隔。
fn default_discovery_interval_minutes() -> u64 {
    DEFAULT_DISCOVERY_INTERVAL_MINUTES
}

/// 首次启动及旧版本配置升级时默认展示的三个 AI 客户端。
pub fn default_enabled_ai_tools() -> Vec<AiTool> {
    vec![AiTool::Codex, AiTool::ClaudeCode, AiTool::Cursor]
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
            enabled_ai_tools: default_enabled_ai_tools(),
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

// 应用支持接入的 AI 工具；Hash 派生用于放入 HashSet 做去重校验。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum AiTool {
    Codex,
    ClaudeCode,
    Cursor,
    OpenCode,
    WorkBuddy,
    #[serde(alias = "harness")]
    Hermes,
    OpenClaw,
    CodeBuddy,
}

impl AiTool {
    // 遍历全部工具时使用的固定顺序数组。
    pub const ALL: [Self; 8] = [
        Self::Codex,
        Self::ClaudeCode,
        Self::Cursor,
        Self::OpenCode,
        Self::WorkBuddy,
        Self::Hermes,
        Self::OpenClaw,
        Self::CodeBuddy,
    ];
}

/// 按应用固定顺序规范化用户选择，并消除重复项。
pub fn normalize_enabled_ai_tools(selected: &[AiTool]) -> Vec<AiTool> {
    AiTool::ALL
        .into_iter()
        .filter(|tool| selected.contains(tool))
        .collect()
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
    /// 工具要求用户审核新 Hook 且配置发生变化时为真。
    pub requires_review: bool,
    /// 工具需要重启当前会话或守护进程才能加载新配置时为真。
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
    #[serde(default)]
    pub open_code: String,
    #[serde(default)]
    pub work_buddy: String,
    #[serde(default)]
    #[serde(alias = "harness")]
    pub hermes: String,
    #[serde(default)]
    pub open_claw: String,
    #[serde(default)]
    pub code_buddy: String,
}

impl HookConfigDirectories {
    // 按工具类型取出对应的自定义目录（可能为空字符串）。
    pub fn get(&self, tool: AiTool) -> &str {
        match tool {
            AiTool::Codex => &self.codex,
            AiTool::ClaudeCode => &self.claude_code,
            AiTool::Cursor => &self.cursor,
            AiTool::OpenCode => &self.open_code,
            AiTool::WorkBuddy => &self.work_buddy,
            AiTool::Hermes => &self.hermes,
            AiTool::OpenClaw => &self.open_claw,
            AiTool::CodeBuddy => &self.code_buddy,
        }
    }

    // 按工具类型写入对应的自定义目录。
    pub fn set(&mut self, tool: AiTool, directory: String) {
        match tool {
            AiTool::Codex => self.codex = directory,
            AiTool::ClaudeCode => self.claude_code = directory,
            AiTool::Cursor => self.cursor = directory,
            AiTool::OpenCode => self.open_code = directory,
            AiTool::WorkBuddy => self.work_buddy = directory,
            AiTool::Hermes => self.hermes = directory,
            AiTool::OpenClaw => self.open_claw = directory,
            AiTool::CodeBuddy => self.code_buddy = directory,
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
    ] {
        if !directory.is_empty() && !Path::new(directory).is_absolute() {
            return Err("持久化 Hooks 配置目录必须使用绝对路径".to_owned());
        }
    }
    Ok(())
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

impl HookBehavior {
    // 当前所有有效的展示行为，顺序固定，供 validate_profile 做“四选四”检查。
    const DISPLAY_BEHAVIORS: [Self; 4] = [Self::Idle, Self::Running, Self::Asking, Self::Error];
}

// 一个 Hook 事件触发后，展示屏应执行的动作：切换到某个行为展示，或者释放（退出）该展示位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookTransition {
    Display(HookBehavior),
    Release,
}

/// Hook 事件经过生命周期算法后的处理决定。应用层只负责执行 `Forward`，
/// `Ignore` 表示这是重复或已经失去时序意义的事件，`Unsupported` 表示配置/请求
/// 中出现了该工具不认识的事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEventDecision {
    Forward(HookTransition),
    Ignore,
    Unsupported,
}

/// 单个工具最多保留的 Hook 会话数。结束墓碑也计入上限，避免缺失
/// `SessionEnd` 或监控进程中途启动时，无界积累会话状态。
pub(crate) const MAX_TRACKED_HOOK_SESSIONS: usize = 256;

/// 单个 AI 工具的生命周期状态。它不依赖墙上时钟，因此迟到多久的完成事件都
/// 不会越过已经收到的 Stop/SessionEnd，把监控屏错误地切回运行中。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookStateMachine {
    sessions: HashMap<String, HookSessionState>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HookSessionState {
    phase: HookPhase,
    turn_active: bool,
    turn_id: Option<String>,
    /// 已收到 `SessionEnd` 的会话保留为空墓碑，用于拒绝随后迟到的事件。
    ended: bool,
    /// 由应用层注入的进程内单调经过时间，领域层不直接读取系统时钟。
    last_seen_at: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HookPhase {
    #[default]
    Released,
    Idle,
    Running,
    Asking,
    Error,
}

impl HookStateMachine {
    /// 把原生 Hook 事件归一化后推进状态机，并返回唯一需要由应用层执行的动作。
    #[cfg(test)]
    pub fn apply(&mut self, tool: AiTool, event: &str) -> HookEventDecision {
        self.apply_event(tool, event, None, None)
    }

    /// 带会话/轮次标识推进状态。多会话共享同一个工具展示位时，以所有会话的
    /// 聚合状态为准；旧 turn 的迟到事件只会影响它自己的会话，且会被忽略。
    #[cfg(test)]
    pub fn apply_event(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
    ) -> HookEventDecision {
        self.apply_event_with_status(tool, event, session_id, turn_id, None)
    }

    /// Cursor 的 `stop` 通过 `status` 区分正常完成和异常结束；其他工具当前由
    /// 独立事件表达错误。协议差异由工具适配器解析，状态机只消费归一化类别。
    #[cfg(test)]
    pub fn apply_event_with_status(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
    ) -> HookEventDecision {
        self.apply_event_with_status_at(tool, event, session_id, turn_id, status, Duration::ZERO)
    }

    /// 使用调用方提供的单调经过时间推进状态机。只有被接纳的事件会刷新
    /// `last_seen_at`；被墓碑或轮次时序拒绝的迟到事件不能延长记录寿命。
    pub(crate) fn apply_event_with_status_at(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
        observed_at: Duration,
    ) -> HookEventDecision {
        let Some(event_kind) = event_kind(tool, event, status) else {
            return HookEventDecision::Unsupported;
        };
        let transition = event_kind.transition();
        let previous = self.aggregate_phase();
        let session_key = session_id.unwrap_or("__default__").to_owned();

        if event_kind == HookEventKind::SessionEnd {
            return self.apply_session_end(session_key, observed_at, previous);
        }

        if event_kind == HookEventKind::SessionStart {
            return self.apply_session_start(
                session_key,
                session_id.is_some(),
                observed_at,
                previous,
            );
        }

        // 结束墓碑只允许显式 SessionStart 覆盖；任何迟到事件（包括重复 Stop
        // 和新的工作事件）均不刷新墓碑时间，也不能隐式复活会话。
        if self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended)
        {
            return HookEventDecision::Ignore;
        }

        // 完成类事件不是可靠的工作起点。Monitor 若中途启动、没有见过对应
        // 会话或工作开始，则直接忽略且不留下幽灵记录。
        if matches!(event_kind, HookEventKind::WorkCompletion(_))
            && !self.sessions.contains_key(&session_key)
        {
            return HookEventDecision::Ignore;
        }

        self.ensure_capacity_for(&session_key);
        let session = self
            .sessions
            .entry(session_key)
            .or_insert_with(|| HookSessionState {
                last_seen_at: observed_at,
                ..HookSessionState::default()
            });

        if event_kind == HookEventKind::WorkStart {
            session.turn_active = true;
            session.turn_id = turn_id.map(str::to_owned);
            session.phase = HookPhase::Running;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if event_kind == HookEventKind::Stop {
            if turn_is_stale(session, turn_id) {
                return HookEventDecision::Ignore;
            }
            session.turn_active = false;
            session.phase = HookPhase::Idle;
            session.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }

        if matches!(event_kind, HookEventKind::WorkCompletion(_))
            && (!session.turn_active || turn_is_stale(session, turn_id))
        {
            return HookEventDecision::Ignore;
        }

        if turn_is_stale(session, turn_id) {
            return HookEventDecision::Ignore;
        }
        let next = match transition {
            HookTransition::Release => HookPhase::Released,
            HookTransition::Display(HookBehavior::Idle) => HookPhase::Idle,
            HookTransition::Display(HookBehavior::Running) => {
                session.turn_active = true;
                if let Some(turn_id) = turn_id {
                    session.turn_id = Some(turn_id.to_owned());
                }
                HookPhase::Running
            }
            HookTransition::Display(HookBehavior::Asking) => {
                session.turn_active = true;
                if let Some(turn_id) = turn_id {
                    session.turn_id = Some(turn_id.to_owned());
                }
                HookPhase::Asking
            }
            HookTransition::Display(HookBehavior::Error) => {
                session.turn_active = false;
                HookPhase::Error
            }
        };
        session.phase = next;
        session.last_seen_at = observed_at;
        phase_decision(previous, self.aggregate_phase())
    }

    fn apply_session_end(
        &mut self,
        session_key: String,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        if self
            .sessions
            .get(&session_key)
            .is_some_and(|session| session.ended)
        {
            return HookEventDecision::Ignore;
        }

        self.ensure_capacity_for(&session_key);
        self.sessions.insert(
            session_key,
            HookSessionState {
                phase: HookPhase::Released,
                turn_active: false,
                turn_id: None,
                ended: true,
                last_seen_at: observed_at,
            },
        );
        let next = self.aggregate_phase();
        // 即使 Monitor 在会话开始后才启动，也要让首次结束事件向目标设备
        // 幂等释放一次。墓碑保证同一结束事件重放时不会形成请求风暴。
        if next == HookPhase::Released {
            return HookEventDecision::Forward(HookTransition::Release);
        }
        phase_decision(previous, next)
    }

    fn apply_session_start(
        &mut self,
        session_key: String,
        has_session_id: bool,
        observed_at: Duration,
        previous: HookPhase,
    ) -> HookEventDecision {
        // Cursor 的 workspaceOpen 不带 conversation_id，先以默认占位展示空闲；
        // 真正的 sessionStart 到来后必须替换该占位，否则 SessionEnd 后会残留
        // 一个永远无法释放的“工作区会话”。
        if has_session_id {
            self.sessions.remove("__default__");
        }
        // 同一会话的重复或迟到 SessionStart 只算作存活信号，不能把已经
        // Running/Asking/Error 的状态倒退回 Idle；真正结束后的 tombstone
        // 仍允许由显式 SessionStart 覆盖，支持会话 ID 被上游重新使用。
        if let Some(existing) = self.sessions.get_mut(&session_key)
            && !existing.ended
        {
            existing.last_seen_at = observed_at;
            return phase_decision(previous, self.aggregate_phase());
        }
        self.ensure_capacity_for(&session_key);
        self.sessions.insert(
            session_key,
            HookSessionState {
                phase: HookPhase::Idle,
                turn_active: false,
                turn_id: None,
                ended: false,
                last_seen_at: observed_at,
            },
        );
        phase_decision(previous, self.aggregate_phase())
    }

    /// 一次性清理所有到期记录，并只根据清理前后的最终聚合状态返回一个决定。
    /// 使用 `saturating_sub` 防御调用方意外传入较小时间值，避免下溢。
    pub(crate) fn expire_inactive_sessions(
        &mut self,
        observed_at: Duration,
        timeout: Duration,
    ) -> HookEventDecision {
        let previous = self.aggregate_phase();
        self.sessions
            .retain(|_, session| observed_at.saturating_sub(session.last_seen_at) < timeout);
        phase_decision(previous, self.aggregate_phase())
    }

    /// 为新会话腾出一个位置：优先淘汰最旧墓碑，其次最旧非活跃会话，
    /// 最后才淘汰最旧活跃会话。相同时间以会话键排序，保证行为可复现。
    fn ensure_capacity_for(&mut self, session_key: &str) {
        if self.sessions.contains_key(session_key) {
            return;
        }
        while self.sessions.len() >= MAX_TRACKED_HOOK_SESSIONS {
            let Some(eviction_key) = self
                .sessions
                .iter()
                .min_by(|(left_key, left), (right_key, right)| {
                    session_eviction_priority(left)
                        .cmp(&session_eviction_priority(right))
                        .then_with(|| left.last_seen_at.cmp(&right.last_seen_at))
                        .then_with(|| left_key.cmp(right_key))
                })
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.sessions.remove(&eviction_key);
        }
    }

    fn aggregate_phase(&self) -> HookPhase {
        [
            HookPhase::Asking,
            HookPhase::Error,
            HookPhase::Running,
            HookPhase::Idle,
        ]
        .into_iter()
        .find(|phase| {
            self.sessions
                .values()
                .any(|session| !session.ended && session.phase == *phase)
        })
        .unwrap_or(HookPhase::Released)
    }

    #[cfg(test)]
    fn tracked_session_count(&self) -> usize {
        self.sessions.len()
    }
}

fn session_eviction_priority(session: &HookSessionState) -> u8 {
    if session.ended {
        0
    } else if !session.turn_active {
        1
    } else {
        2
    }
}

fn turn_is_stale(session: &HookSessionState, incoming_turn_id: Option<&str>) -> bool {
    incoming_turn_id.is_some_and(|incoming| {
        session
            .turn_id
            .as_deref()
            .is_some_and(|current| current != incoming)
    })
}

fn phase_decision(previous: HookPhase, next: HookPhase) -> HookEventDecision {
    if previous == next {
        return HookEventDecision::Ignore;
    }
    HookEventDecision::Forward(match next {
        HookPhase::Released => HookTransition::Release,
        HookPhase::Idle => HookTransition::Display(HookBehavior::Idle),
        HookPhase::Running => HookTransition::Display(HookBehavior::Running),
        HookPhase::Asking => HookTransition::Display(HookBehavior::Asking),
        HookPhase::Error => HookTransition::Display(HookBehavior::Error),
    })
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

/// 长边超过该像素数的上传图片会被等比缩小到该尺寸以内。
pub const MAX_UPLOAD_IMAGE_EDGE: u32 = 800;
// 重新编码 JPEG 时使用的固定质量参数（0-100，值越大质量越高、体积越大）。
const UPLOAD_JPEG_QUALITY: u8 = 82;

/// 上传前处理完成的图片。BMP 与 WebP 会转换为目标端兼容性更好的格式，因此
/// 文件名和 MIME 类型可能与用户选择的原文件不同。
pub struct ProcessedUploadImage {
    pub filename: String,
    pub mime_type: &'static str,
    pub bytes: Vec<u8>,
}

/// 处理待上传图片，降低展示屏（Android 端）解码大图的压力：
///
/// - JPEG/PNG 会在长边超过 [`MAX_UPLOAD_IMAGE_EDGE`] 时等比缩小并重新编码；
/// - GIF 校验格式后原样透传，避免丢失动画；
/// - BMP 转成 PNG；
/// - 静态 WebP 转成 PNG，动画 WebP 保留帧和延时并转成 GIF。
pub fn process_image_upload(
    filename: &str,
    bytes: &[u8],
    mime_type: &str,
) -> Result<ProcessedUploadImage, String> {
    match mime_type {
        "image/gif" => {
            image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes))
                .map_err(|error| format!("图片解码失败：{error}"))?;
            Ok(ProcessedUploadImage {
                filename: filename.to_owned(),
                mime_type: "image/gif",
                bytes: bytes.to_vec(),
            })
        }
        "image/webp" => process_webp_upload(filename, bytes),
        "image/jpeg" => process_static_upload(
            filename,
            bytes,
            image::ImageFormat::Jpeg,
            "image/jpeg",
            false,
        ),
        "image/png" => {
            process_static_upload(filename, bytes, image::ImageFormat::Png, "image/png", false)
        }
        "image/bmp" | "image/x-ms-bmp" => {
            process_static_upload(filename, bytes, image::ImageFormat::Bmp, "image/png", true)
        }
        _ => Err("不支持的图片类型".to_owned()),
    }
}

fn process_static_upload(
    filename: &str,
    bytes: &[u8],
    source_format: image::ImageFormat,
    output_mime_type: &'static str,
    convert_to_png: bool,
) -> Result<ProcessedUploadImage, String> {
    let output_format = if convert_to_png {
        image::ImageFormat::Png
    } else {
        source_format
    };

    // 按声明的格式解码图片；解码失败说明数据损坏或格式不匹配。
    let decoded = image::load_from_memory_with_format(bytes, source_format)
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
    match output_format {
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
        // output_format 在调用方已被限定为 Jpeg 或 Png 之一。
        _ => unreachable!("输出格式只能是 Jpeg 或 Png"),
    }

    // 没有缩放且重新编码后体积没有变小，则保留原始字节，避免做无意义的替换。
    let output_bytes = if !convert_to_png && !needs_resize && output.len() >= bytes.len() {
        bytes.to_vec()
    } else {
        output
    };
    let output_filename = if convert_to_png {
        replace_image_extension(filename, "png")
    } else {
        filename.to_owned()
    };
    Ok(ProcessedUploadImage {
        filename: output_filename,
        mime_type: output_mime_type,
        bytes: output_bytes,
    })
}

fn process_webp_upload(filename: &str, bytes: &[u8]) -> Result<ProcessedUploadImage, String> {
    use image::AnimationDecoder as _;

    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|error| format!("图片解码失败：{error}"))?;
    if !decoder.has_animation() {
        return process_static_upload(filename, bytes, image::ImageFormat::WebP, "image/png", true);
    }

    let repeat = match decoder.loop_count() {
        image::metadata::LoopCount::Infinite => image::codecs::gif::Repeat::Infinite,
        image::metadata::LoopCount::Finite(count) => {
            image::codecs::gif::Repeat::Finite(u16::try_from(count.get()).unwrap_or(u16::MAX))
        }
    };
    let frames = decoder
        .into_frames()
        .map(|frame| {
            let frame = frame?;
            let delay = frame.delay();
            let buffer = frame.into_buffer();
            let resized = if buffer.width() > MAX_UPLOAD_IMAGE_EDGE
                || buffer.height() > MAX_UPLOAD_IMAGE_EDGE
            {
                image::DynamicImage::ImageRgba8(buffer)
                    .resize(
                        MAX_UPLOAD_IMAGE_EDGE,
                        MAX_UPLOAD_IMAGE_EDGE,
                        image::imageops::FilterType::Lanczos3,
                    )
                    .to_rgba8()
            } else {
                buffer
            };
            Ok(image::Frame::from_parts(resized, 0, 0, delay))
        })
        .collect::<image::ImageResult<Vec<_>>>()
        .map_err(|error| format!("图片解码失败：{error}"))?;

    let mut output = Vec::new();
    {
        let mut encoder = image::codecs::gif::GifEncoder::new_with_speed(&mut output, 10);
        encoder
            .set_repeat(repeat)
            .and_then(|()| encoder.encode_frames(frames))
            .map_err(|error| format!("WebP 动图转 GIF 失败：{error}"))?;
    }
    Ok(ProcessedUploadImage {
        filename: replace_image_extension(filename, "gif"),
        mime_type: "image/gif",
        bytes: output,
    })
}

fn replace_image_extension(filename: &str, extension: &str) -> String {
    std::path::Path::new(filename)
        .with_extension(extension)
        .to_string_lossy()
        .into_owned()
}

// 仅在测试构建中编译的单元测试模块，覆盖本文件内的纯业务逻辑。
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;

    // 引入被测的类型与函数（父模块中的公开/私有项）。
    use super::{
        AiProfile, AiTool, DEFAULT_BASE_URL, DEFAULT_DISCOVERY_INTERVAL_MINUTES, HookBehavior,
        HookConfigDirectories, HookConfigPreview, HookContent, HookEventDecision, HookPhase,
        HookStateMachine, HookTransition, MANAGED_HOOK_PREFIX, MAX_DISCOVERY_INTERVAL_MINUTES,
        MAX_TRACKED_HOOK_SESSIONS, MAX_UPLOAD_IMAGE_EDGE, MonitorDeviceRoute, MonitorSettings,
        SavedMonitorData, command_has_marker, decoded_hook_command, default_enabled_ai_tools,
        generate_hook_auxiliary_configs, generate_hook_config, hook_transition,
        managed_hook_marker, merge_hook_config, normalize_base_url, normalize_enabled_ai_tools,
        process_image_upload, validate_discovery_interval_minutes, validate_profile,
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

    #[test]
    fn ai_client_selection_defaults_to_primary_tools_and_is_normalized() {
        assert_eq!(
            default_enabled_ai_tools(),
            vec![AiTool::Codex, AiTool::ClaudeCode, AiTool::Cursor]
        );
        assert_eq!(
            normalize_enabled_ai_tools(&[
                AiTool::Cursor,
                AiTool::Codex,
                AiTool::Cursor,
                AiTool::OpenClaw,
            ]),
            vec![AiTool::Codex, AiTool::Cursor, AiTool::OpenClaw]
        );
    }

    #[test]
    fn legacy_harness_values_load_as_hermes_and_serialize_canonically() {
        let tool: AiTool = serde_json::from_str(r#""harness""#).unwrap();
        assert_eq!(tool, AiTool::Hermes);
        assert_eq!(serde_json::to_string(&tool).unwrap(), r#""hermes""#);

        let directories: HookConfigDirectories =
            serde_json::from_str(r#"{"harness":"/legacy/harness"}"#).unwrap();
        assert_eq!(directories.hermes, "/legacy/harness");
        let serialized = serde_json::to_value(directories).unwrap();
        assert_eq!(serialized["hermes"], "/legacy/harness");
        assert!(serialized.get("harness").is_none());
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
        let preview = generate_hook_config(AiTool::Cursor).unwrap();

        assert_eq!(preview.filename, ".cursor/hooks.json");
        assert!(preview.content.contains("\"beforeSubmitPrompt\""));
        assert!(preview.content.contains("\"beforeShellExecution\""));
        assert!(preview.content.contains("\"beforeMCPExecution\""));
        assert!(preview.content.contains("\"afterFileEdit\""));
        assert!(preview.content.contains("\"workspaceOpen\""));
        assert!(preview.content.contains("\"postToolUse\""));
        assert!(preview.content.contains("\"subagentStart\""));
        assert!(preview.content.contains("\"subagentStop\""));
        assert!(preview.content.contains("\"preCompact\""));
        assert!(preview.content.contains("\"afterAgentResponse\""));
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
        let preview = generate_hook_config(AiTool::ClaudeCode).unwrap();

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
        let preview = generate_hook_config(AiTool::Codex).unwrap();

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
        assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
        // 取出 Stop 的 Windows 命令（编码后的 PowerShell），解码后验证内容正确。
        let windows = value["hooks"]["Stop"][0]["hooks"][0]["commandWindows"]
            .as_str()
            .unwrap();
        let decoded = decoded_hook_command(windows).unwrap();
        assert!(decoded.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(decoded.contains(MANAGED_HOOK_PREFIX));
        assert!(decoded.contains("while ($true)"));
        assert!(decoded.contains("Start-Sleep -Seconds 1"));
        // command_has_marker 应能在未解码的编码命令上直接识别出托管标记。
        assert!(command_has_marker(
            windows,
            &managed_hook_marker(AiTool::Codex)
        ));
    }

    #[test]
    fn work_buddy_preview_targets_its_independent_settings_file() {
        let preview = generate_hook_config(AiTool::WorkBuddy).unwrap();

        assert_eq!(preview.filename, ".workbuddy/settings.json");
        assert!(preview.content.contains("\"SessionStart\""));
        assert!(preview.content.contains("\"PermissionRequest\""));
        assert!(preview.content.contains("AIMonitor|tool=workbuddy"));
    }

    #[test]
    fn open_code_preview_is_a_managed_global_plugin() {
        let preview = generate_hook_config(AiTool::OpenCode).unwrap();

        assert_eq!(preview.filename, ".config/opencode/plugins/aimonitor.js");
        assert!(preview.content.contains("AIMonitor|tool=opencode"));
        assert!(preview.content.contains("session.status"));
        assert!(preview.content.contains("permission.asked"));
        assert!(preview.content.contains("/api/hooks/opencode"));

        let merged = merge_hook_config(None, &preview, AiTool::OpenCode).unwrap();
        assert_eq!(merged.content, preview.content);
        let merged_again =
            merge_hook_config(Some(&merged.content), &preview, AiTool::OpenCode).unwrap();
        assert_eq!(merged_again.content, preview.content);
        assert!(
            merge_hook_config(
                Some("export const unrelated = true\n"),
                &preview,
                AiTool::OpenCode,
            )
            .is_err()
        );
    }

    #[test]
    fn code_buddy_preview_uses_its_native_config_and_posix_hook_command() {
        let preview = generate_hook_config(AiTool::CodeBuddy).unwrap();

        assert_eq!(preview.filename, ".codebuddy/settings.json");
        assert!(preview.content.contains("\"PermissionRequest\""));
        assert!(preview.content.contains("AIMonitor|tool=codebuddy"));
        assert!(preview.content.contains("/api/hooks/codebuddy"));
        assert!(!preview.content.contains("powershell.exe"));
    }

    #[test]
    fn hermes_preview_contains_a_complete_managed_plugin() {
        let preview = generate_hook_config(AiTool::Hermes).unwrap();
        let auxiliary = generate_hook_auxiliary_configs(AiTool::Hermes);

        assert_eq!(preview.filename, ".hermes/plugins/aimonitor/__init__.py");
        assert!(preview.content.contains("AIMonitor|tool=hermes"));
        assert!(preview.content.contains("ctx.register_hook"));
        assert!(preview.content.contains("pre_approval_request"));
        assert!(preview.content.contains("api_request_error"));
        assert!(preview.content.contains("/api/hooks/hermes"));
        assert_eq!(auxiliary.len(), 1);
        assert_eq!(auxiliary[0].filename, "plugins/aimonitor/plugin.yaml");
        assert!(auxiliary[0].content.contains("name: aimonitor"));

        let merged = merge_hook_config(None, &preview, AiTool::Hermes).unwrap();
        assert_eq!(merged.content, preview.content);
        assert!(
            merge_hook_config(
                Some("# unrelated Hermes plugin\n"),
                &preview,
                AiTool::Hermes,
            )
            .is_err()
        );
    }

    #[test]
    fn open_claw_preview_contains_a_complete_managed_plugin() {
        let preview = generate_hook_config(AiTool::OpenClaw).unwrap();
        let auxiliary = generate_hook_auxiliary_configs(AiTool::OpenClaw);

        assert!(preview.content.contains("AIMonitor|tool=openclaw"));
        assert!(preview.content.contains("before_agent_run"));
        assert!(preview.content.contains("agent_end"));
        assert!(preview.content.contains("/api/hooks/openclaw"));
        assert_eq!(auxiliary.len(), 2);
        assert!(
            auxiliary
                .iter()
                .any(|file| file.filename.ends_with("openclaw.plugin.json"))
        );
        assert!(
            auxiliary
                .iter()
                .all(|file| file.content.contains("AIMonitor|tool=openclaw"))
        );
    }

    // 验证 Codex 的合并逻辑是幂等的：重复合并不会让托管条目累积，
    // 且用户手工添加的其他命令、其他顶层字段（如 permissions）会被保留。
    #[test]
    fn codex_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(AiTool::Codex).unwrap();
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
        assert_eq!(
            serialized
                .matches(&managed_hook_marker(AiTool::Codex))
                .count(),
            1
        );
        // 用户手工添加的 permissions 字段应被完整保留。
        assert_eq!(value["permissions"]["allow"][0], "Bash");
    }

    // 验证 Cursor 的合并逻辑同样幂等，且能正确保留用户命令、去重已有的托管条目。
    #[test]
    fn cursor_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(AiTool::Cursor).unwrap();
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

    // 验证生成的命令直接请求稳定的本机中继地址，带有覆盖冷启动竞态的有界重试，
    // 且不包含历史上淘汰的独立脚本、默认设备地址或 behavior 业务字段。
    #[test]
    fn hook_commands_post_directly_to_the_stable_local_relay() {
        let preview = generate_hook_config(AiTool::Codex).unwrap();

        assert!(preview.content.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(
            preview
                .content
                .contains("X-AIMonitor-Hook-Type: SessionStart")
        );
        assert!(preview.content.contains("--data-binary @-"));
        assert!(preview.content.contains("--retry 5"));
        assert!(preview.content.contains("--retry-connrefused"));
        assert!(preview.content.contains("--retry-all-errors"));
        assert!(!preview.content.contains("aimonitor-hook.sh"));
        assert!(!preview.content.contains("aimonitor-hook.ps1"));
        assert!(!preview.content.contains(DEFAULT_BASE_URL));
        assert!(!preview.content.contains("\"behavior\":\"running\""));
    }

    // 验证生成的 Hooks 配置只依赖工具类型，不需要预先保存设备展示 Profile。
    #[test]
    fn hook_config_is_identical_when_display_content_changes() {
        assert_eq!(
            generate_hook_config(AiTool::Codex).unwrap().content,
            generate_hook_config(AiTool::Codex).unwrap().content
        );
    }

    // 验证 hook_transition 对不同工具、不同事件名能返回正确的状态迁移：
    // Claude 的 Notification 对应 Idle 展示，Codex 的 PermissionRequest 对应 Asking 展示，
    // Cursor 的 sessionEnd 对应释放展示位，OpenCode/WorkBuddy 的原生事件也由
    // 各自协议归一化，未知事件返回 None。
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
        assert_eq!(
            hook_transition(AiTool::OpenCode, "session.busy"),
            Some(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            hook_transition(AiTool::WorkBuddy, "PermissionRequest"),
            Some(HookTransition::Display(HookBehavior::Asking))
        );
        assert_eq!(hook_transition(AiTool::Codex, "Unknown"), None);
    }

    #[test]
    fn status_driven_protocols_map_native_states() {
        let mut hermes = HookStateMachine::default();
        assert_eq!(
            hermes.apply_event(
                AiTool::Hermes,
                "pre_llm_call",
                Some("session-1"),
                Some("turn-1"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            hermes.apply_event(
                AiTool::Hermes,
                "pre_approval_request",
                Some("session-1"),
                Some("turn-1"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
        );

        let mut open_claw = HookStateMachine::default();
        open_claw.apply_event(AiTool::OpenClaw, "session_start", Some("s1"), None);
        open_claw.apply_event(AiTool::OpenClaw, "before_agent_run", Some("s1"), Some("r1"));
        assert_eq!(
            open_claw.apply_event_with_status(
                AiTool::OpenClaw,
                "agent_end",
                Some("s1"),
                Some("r1"),
                Some("failed"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );
    }

    #[test]
    fn codex_state_machine_covers_open_interrupt_late_completion_and_exit() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply(AiTool::Codex, "SessionStart"),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.apply(AiTool::Codex, "UserPromptSubmit"),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            machine.apply(AiTool::Codex, "Stop"),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        // Stop 之后不再依赖两秒窗口：无论迟到多久，完成类事件都不能重新进入运行态。
        assert_eq!(
            machine.apply(AiTool::Codex, "SubagentStop"),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply(AiTool::Codex, "PostToolUse"),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply(AiTool::Codex, "SessionEnd"),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }

    #[test]
    fn state_machine_only_resumes_after_a_real_work_start() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply(AiTool::ClaudeCode, "Stop"),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.apply(AiTool::ClaudeCode, "PostCompact"),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply(AiTool::ClaudeCode, "UserPromptSubmit"),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        // 活跃轮次中的完成事件保持 Running；由于展示状态未变化，无需重复打设备请求。
        assert_eq!(
            machine.apply(AiTool::ClaudeCode, "PostToolUse"),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply(AiTool::ClaudeCode, "PermissionRequest"),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Asking))
        );
    }

    #[test]
    fn duplicate_session_start_does_not_regress_an_active_session() {
        let mut machine = HookStateMachine::default();
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionStart",
            Some("s1"),
            None,
            None,
            Duration::from_secs(1),
        );
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("s1"),
            Some("t1"),
            None,
            Duration::from_secs(2),
        );

        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "SessionStart",
                Some("s1"),
                None,
                None,
                Duration::from_secs(3),
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(machine.sessions["s1"].phase, HookPhase::Running);
        assert!(machine.sessions["s1"].turn_active);
        assert_eq!(machine.sessions["s1"].last_seen_at, Duration::from_secs(3));
    }

    #[test]
    fn cursor_stop_status_distinguishes_failure_from_completion() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply_event(
                AiTool::Cursor,
                "beforeSubmitPrompt",
                Some("conversation-1"),
                Some("generation-1"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(
            machine.apply_event_with_status(
                AiTool::Cursor,
                "stop",
                Some("conversation-1"),
                Some("generation-1"),
                Some("error"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Error))
        );

        let mut completed = HookStateMachine::default();
        completed.apply_event(
            AiTool::Cursor,
            "beforeSubmitPrompt",
            Some("conversation-2"),
            Some("generation-2"),
        );
        assert_eq!(
            completed.apply_event_with_status(
                AiTool::Cursor,
                "stop",
                Some("conversation-2"),
                Some("generation-2"),
                Some("completed"),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
    }

    #[test]
    fn cursor_real_session_replaces_workspace_placeholder() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply_event(AiTool::Cursor, "workspaceOpen", None, None),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.apply_event(AiTool::Cursor, "sessionStart", Some("conversation-1"), None,),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply_event(AiTool::Cursor, "sessionEnd", Some("conversation-1"), None,),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }

    #[test]
    fn state_machine_aggregates_multiple_sessions_without_cross_talk() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply_event(AiTool::Codex, "SessionStart", Some("s1"), None),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.apply_event(AiTool::Codex, "UserPromptSubmit", Some("s1"), Some("t1")),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        // 第二个空闲会话出现时，第一个会话仍在工作，聚合状态保持 Running。
        assert_eq!(
            machine.apply_event(AiTool::Codex, "SessionStart", Some("s2"), None),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("t1")),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(
            machine.apply_event(AiTool::Codex, "UserPromptSubmit", Some("s2"), Some("t2")),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        // 关闭 s1 不得释放仍在运行的 s2。
        assert_eq!(
            machine.apply_event(AiTool::Codex, "SessionEnd", Some("s1"), None),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply_event(AiTool::Codex, "SessionEnd", Some("s2"), None),
            HookEventDecision::Forward(HookTransition::Release)
        );
    }

    #[test]
    fn state_machine_rejects_events_from_an_older_turn() {
        let mut machine = HookStateMachine::default();
        machine.apply_event(AiTool::Codex, "SessionStart", Some("s1"), None);
        machine.apply_event(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("s1"),
            Some("new-turn"),
        );

        assert_eq!(
            machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("old-turn")),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.apply_event(AiTool::Codex, "Stop", Some("s1"), Some("new-turn")),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
    }

    #[test]
    fn orphan_completion_is_ignored_without_leaving_a_ghost_session() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "PostToolUse",
                Some("late-session"),
                Some("turn-1"),
                None,
                Duration::from_secs(10),
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(machine.tracked_session_count(), 0);

        // 与完成事件不同，真实工作进展可以作为 Monitor 中途启动后的首个事件。
        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "PreToolUse",
                Some("live-session"),
                Some("turn-1"),
                None,
                Duration::from_secs(11),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Running))
        );
        assert_eq!(machine.tracked_session_count(), 1);
    }

    #[test]
    fn ended_tombstone_rejects_late_events_until_explicit_restart() {
        let mut machine = HookStateMachine::default();
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("s1"),
            Some("t1"),
            None,
            Duration::from_secs(1),
        );
        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "SessionEnd",
                Some("s1"),
                None,
                None,
                Duration::from_secs(2),
            ),
            HookEventDecision::Forward(HookTransition::Release)
        );

        for event in [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "Stop",
        ] {
            assert_eq!(
                machine.apply_event_with_status_at(
                    AiTool::Codex,
                    event,
                    Some("s1"),
                    Some("t1"),
                    None,
                    Duration::from_secs(100),
                ),
                HookEventDecision::Ignore,
                "墓碑应拒绝迟到事件 {event}"
            );
        }
        assert_eq!(machine.sessions["s1"].last_seen_at, Duration::from_secs(2));

        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "SessionStart",
                Some("s1"),
                None,
                None,
                Duration::from_secs(101),
            ),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert!(!machine.sessions["s1"].ended);
    }

    #[test]
    fn unknown_session_end_releases_once_without_overriding_other_live_sessions() {
        let mut machine = HookStateMachine::default();

        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "SessionEnd",
                Some("unknown"),
                None,
                None,
                Duration::from_secs(1),
            ),
            HookEventDecision::Forward(HookTransition::Release)
        );
        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "SessionEnd",
                Some("unknown"),
                None,
                None,
                Duration::from_secs(2),
            ),
            HookEventDecision::Ignore
        );
        assert_eq!(
            machine.sessions["unknown"].last_seen_at,
            Duration::from_secs(1)
        );

        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("live"),
            Some("turn"),
            None,
            Duration::from_secs(3),
        );
        assert_eq!(
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "SessionEnd",
                Some("another-unknown"),
                None,
                None,
                Duration::from_secs(4),
            ),
            HookEventDecision::Ignore
        );
    }

    #[test]
    fn expiring_sessions_batches_changes_into_one_final_aggregate_transition() {
        let mut machine = HookStateMachine::default();
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("asking"),
            Some("t1"),
            None,
            Duration::ZERO,
        );
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "PermissionRequest",
            Some("asking"),
            Some("t1"),
            None,
            Duration::ZERO,
        );
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("running"),
            Some("t2"),
            None,
            Duration::from_secs(5),
        );
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionStart",
            Some("idle"),
            None,
            None,
            Duration::from_secs(15),
        );

        // Asking 和 Running 同批到期；对外只暴露最终仍存活的 Idle 聚合态。
        assert_eq!(
            machine.expire_inactive_sessions(Duration::from_secs(20), Duration::from_secs(10),),
            HookEventDecision::Forward(HookTransition::Display(HookBehavior::Idle))
        );
        assert_eq!(machine.tracked_session_count(), 1);
        assert_eq!(
            machine.expire_inactive_sessions(Duration::from_secs(26), Duration::from_secs(10),),
            HookEventDecision::Forward(HookTransition::Release)
        );
        assert_eq!(machine.tracked_session_count(), 0);
    }

    #[test]
    fn session_tracking_stays_bounded_and_uses_eviction_priority() {
        let mut machine = HookStateMachine::default();
        for index in 0..MAX_TRACKED_HOOK_SESSIONS {
            let session_id = format!("session-{index:03}");
            machine.apply_event_with_status_at(
                AiTool::Codex,
                "UserPromptSubmit",
                Some(&session_id),
                Some("turn"),
                None,
                Duration::from_secs(index as u64),
            );
        }
        assert_eq!(machine.tracked_session_count(), MAX_TRACKED_HOOK_SESSIONS);

        // 即使墓碑较新，也应先于任何活跃会话淘汰。
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "SessionEnd",
            Some("session-000"),
            None,
            None,
            Duration::from_mins(5),
        );
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("overflow-1"),
            Some("turn"),
            None,
            Duration::from_secs(301),
        );
        assert!(!machine.sessions.contains_key("session-000"));

        // 非活跃会话其次；只有两类都不存在时才淘汰最旧活跃会话。
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "Stop",
            Some("session-001"),
            Some("turn"),
            None,
            Duration::from_secs(302),
        );
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("overflow-2"),
            Some("turn"),
            None,
            Duration::from_secs(303),
        );
        assert!(!machine.sessions.contains_key("session-001"));
        machine.apply_event_with_status_at(
            AiTool::Codex,
            "UserPromptSubmit",
            Some("overflow-3"),
            Some("turn"),
            None,
            Duration::from_secs(304),
        );
        assert!(!machine.sessions.contains_key("session-002"));
        assert_eq!(machine.tracked_session_count(), MAX_TRACKED_HOOK_SESSIONS);
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

        let processed = process_image_upload("wide.jpg", &source, "image/jpeg").unwrap();

        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Jpeg)
                .unwrap();
        assert_eq!(decoded.width(), MAX_UPLOAD_IMAGE_EDGE);
        assert_eq!(decoded.height(), 200);
        assert_eq!(processed.filename, "wide.jpg");
        assert_eq!(processed.mime_type, "image/jpeg");
    }

    // 验证未超限的小尺寸 PNG 图片在处理后尺寸保持不变。
    #[test]
    fn small_png_upload_keeps_its_dimensions() {
        let source = encode_test_image(100, 50, image::ImageFormat::Png);

        let processed = process_image_upload("small.png", &source, "image/png").unwrap();

        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (100, 50));
    }

    // 验证 GIF 图片原样透传，不做任何解码/重新编码（避免丢失动画帧）。
    #[test]
    fn gif_upload_passes_through_unchanged() {
        let source = encode_test_image(2, 2, image::ImageFormat::Gif);

        let processed = process_image_upload("moving.gif", &source, "image/gif").unwrap();

        assert_eq!(processed.bytes, source);
        assert_eq!(processed.filename, "moving.gif");
        assert_eq!(processed.mime_type, "image/gif");
    }

    #[test]
    fn bmp_upload_is_converted_to_png() {
        let source = encode_test_image(16, 8, image::ImageFormat::Bmp);

        let processed = process_image_upload("legacy.bmp", &source, "image/bmp").unwrap();

        assert_eq!(processed.filename, "legacy.png");
        assert_eq!(processed.mime_type, "image/png");
        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 8));
    }

    #[test]
    fn static_webp_upload_is_converted_to_png() {
        let source = encode_test_image(12, 6, image::ImageFormat::WebP);

        let processed = process_image_upload("still.webp", &source, "image/webp").unwrap();

        assert_eq!(processed.filename, "still.png");
        assert_eq!(processed.mime_type, "image/png");
        let decoded =
            image::load_from_memory_with_format(&processed.bytes, image::ImageFormat::Png).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (12, 6));
    }

    #[test]
    fn animated_webp_upload_is_converted_to_animated_gif() {
        use image::AnimationDecoder as _;

        // 2x2、两帧、无限循环的 WebP：红帧 80ms，蓝帧 120ms。
        const ANIMATED_WEBP: &[u8] = &[
            0x52, 0x49, 0x46, 0x46, 0x84, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50,
            0x38, 0x58, 0x0a, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x41, 0x4e, 0x49, 0x4d, 0x06, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            0x00, 0x00, 0x41, 0x4e, 0x4d, 0x46, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x50, 0x00, 0x00, 0x02, 0x56, 0x50,
            0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01, 0x40, 0x00, 0x00, 0x07, 0x10, 0xe5,
            0x8f, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00, 0x41, 0x4e, 0x4d, 0x46, 0x28, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00,
            0x78, 0x00, 0x00, 0x00, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x01,
            0x40, 0x00, 0x00, 0x07, 0x10, 0xd1, 0xfe, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00,
        ];

        let processed =
            process_image_upload("animation.webp", ANIMATED_WEBP, "image/webp").unwrap();

        assert_eq!(processed.filename, "animation.gif");
        assert_eq!(processed.mime_type, "image/gif");
        let decoder =
            image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&processed.bytes)).unwrap();
        assert_eq!(decoder.into_frames().count(), 2);
    }

    // 验证损坏的 JPEG 数据在解码阶段会被拒绝并返回错误，而不是 panic。
    #[test]
    fn corrupt_jpeg_upload_is_rejected() {
        let result = process_image_upload("broken.jpg", b"not a jpeg", "image/jpeg");

        assert!(result.is_err());
    }
}
