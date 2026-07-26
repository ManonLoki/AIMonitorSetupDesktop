// 标准库：HashMap/HashSet 用于生命周期聚合与去重校验，Path 用于校验目录。
use std::collections::{HashMap, HashSet};
use std::path::Path;

// serde：结构体/枚举的序列化与反序列化派生宏。
use serde::{Deserialize, Serialize};
mod hooks;

use hooks::{HookEventKind, event_kind};
#[cfg(test)]
use hooks::{
    MANAGED_HOOK_PREFIX, command_has_marker, decoded_hook_command, hook_transition,
    managed_hook_marker,
};
pub use hooks::{ai_tool_name, generate_hook_config, hook_config_filename, merge_hook_config};

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
    pub fn apply_event_with_status(
        &mut self,
        tool: AiTool,
        event: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        status: Option<&str>,
    ) -> HookEventDecision {
        let Some(event_kind) = event_kind(tool, event, status) else {
            return HookEventDecision::Unsupported;
        };
        let transition = event_kind.transition();
        let previous = self.aggregate_phase();
        let session_key = session_id.unwrap_or("__default__").to_owned();

        if event_kind == HookEventKind::SessionEnd {
            self.sessions.remove(&session_key);
            return phase_decision(previous, self.aggregate_phase());
        }

        if event_kind == HookEventKind::SessionStart {
            // Cursor 的 workspaceOpen 不带 conversation_id，先以默认占位展示空闲；
            // 真正的 sessionStart 到来后必须替换该占位，否则 sessionEnd 后会残留
            // 一个永远无法释放的“工作区会话”。
            if session_id.is_some() {
                self.sessions.remove("__default__");
            }
            self.sessions.insert(
                session_key,
                HookSessionState {
                    phase: HookPhase::Idle,
                    turn_active: false,
                    turn_id: None,
                },
            );
            return phase_decision(previous, self.aggregate_phase());
        }

        let session = self.sessions.entry(session_key).or_default();

        if event_kind == HookEventKind::WorkStart {
            session.turn_active = true;
            session.turn_id = turn_id.map(str::to_owned);
            session.phase = HookPhase::Running;
            return phase_decision(previous, self.aggregate_phase());
        }

        if event_kind == HookEventKind::Stop {
            if turn_is_stale(session, turn_id) {
                return HookEventDecision::Ignore;
            }
            session.turn_active = false;
            session.phase = HookPhase::Idle;
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
        phase_decision(previous, self.aggregate_phase())
    }

    fn aggregate_phase(&self) -> HookPhase {
        if self.sessions.is_empty() {
            return HookPhase::Released;
        }
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
                .any(|session| session.phase == *phase)
        })
        .unwrap_or(HookPhase::Idle)
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
        HookConfigDirectories, HookConfigPreview, HookContent, HookEventDecision, HookStateMachine,
        HookTransition, MANAGED_HOOK_PREFIX, MAX_DISCOVERY_INTERVAL_MINUTES, MAX_UPLOAD_IMAGE_EDGE,
        MonitorDeviceRoute, MonitorSettings, SavedMonitorData, command_has_marker,
        decoded_hook_command, generate_hook_config, hook_transition, managed_hook_marker,
        merge_hook_config, normalize_base_url, resize_and_compress_image,
        validate_discovery_interval_minutes, validate_profile, validate_saved_monitor_data,
        validate_username,
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
        assert_eq!(value["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"], 3);
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
        assert!(
            preview
                .content
                .contains("X-AIMonitor-Hook-Type: SessionStart")
        );
        assert!(preview.content.contains("--data-binary @-"));
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
