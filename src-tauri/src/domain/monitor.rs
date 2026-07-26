use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const DEFAULT_BASE_URL: &str = "http://192.168.1.100:8080";
const MANAGED_HOOK_PREFIX: &str = "AIMonitor";
pub const DEFAULT_HOOK_RELAY_PORT: u16 = 10_240;
/// 在线设备自动检查的默认间隔：启动后立即检查一次，之后每分钟刷新。
pub const DEFAULT_DISCOVERY_INTERVAL_MINUTES: u64 = 1;
pub const MIN_DISCOVERY_INTERVAL_MINUTES: u64 = 1;
pub const MAX_DISCOVERY_INTERVAL_MINUTES: u64 = 60;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    /// 所有设备共享的显示用户名。
    #[serde(default)]
    pub username: String,
    /// 当前 UI 选中的设备。仅用于页面上下文，不决定 Hook 转发目标。
    pub base_url: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
    /// 在线设备自动检查间隔（分钟）。修改后由后台发现循环下一次轮询立即生效。
    #[serde(default = "default_discovery_interval_minutes")]
    pub discovery_interval_minutes: u64,
}

fn default_discovery_interval_minutes() -> u64 {
    DEFAULT_DISCOVERY_INTERVAL_MINUTES
}

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
    if !(MIN_DISCOVERY_INTERVAL_MINUTES..=MAX_DISCOVERY_INTERVAL_MINUTES).contains(&minutes) {
        return Err(format!(
            "自动检查间隔必须在 {MIN_DISCOVERY_INTERVAL_MINUTES} 到 {MAX_DISCOVERY_INTERVAL_MINUTES} 分钟之间"
        ));
    }
    Ok(minutes)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorDeviceRoute {
    pub base_url: String,
    pub device_id: String,
    pub device_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredMonitorDevice {
    pub id: String,
    pub name: String,
    pub api_version: String,
    pub base_url: String,
    pub path: String,
    #[serde(default)]
    pub discovery_source: DiscoverySource,
}

/// 设备是如何被找到的；决定发现流程的信任优先级：mDNS 优先，
/// 失败后回退到 UDP 广播，再回退到已保存地址。
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DiscoverySource {
    #[default]
    Mdns,
    UdpBroadcast,
    SavedAddress,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum AiTool {
    Codex,
    ClaudeCode,
    Cursor,
}

impl AiTool {
    pub const ALL: [Self; 3] = [Self::Codex, Self::ClaudeCode, Self::Cursor];
}

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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HookContent {
    pub behavior: HookBehavior,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub image: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProfile {
    /// Profile 所属的 `AIMonitor` 设备。
    #[serde(default)]
    pub device_id: String,
    pub tool: AiTool,
    /// 在展示屏上的显示位置，取值范围 1-25（校验见 `validate_profile`）。
    pub slot: u8,
    #[serde(default)]
    pub hooks: Vec<HookContent>,
}

#[derive(Clone, Debug)]
pub struct HookConfigPreview {
    pub filename: String,
    pub content: String,
}

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
    pub fn get(&self, tool: AiTool) -> &str {
        match tool {
            AiTool::Codex => &self.codex,
            AiTool::ClaudeCode => &self.claude_code,
            AiTool::Cursor => &self.cursor,
        }
    }

    pub fn set(&mut self, tool: AiTool, directory: String) {
        match tool {
            AiTool::Codex => self.codex = directory,
            AiTool::ClaudeCode => self.claude_code = directory,
            AiTool::Cursor => self.cursor = directory,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigLocation {
    pub tool: AiTool,
    pub directory: String,
    pub config_path: String,
    pub is_custom: bool,
}

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
    normalize_base_url(&data.settings.base_url)?;
    validate_discovery_interval_minutes(data.settings.discovery_interval_minutes)?;
    if !data.settings.username.is_empty() {
        validate_username(&data.settings.username)?;
    }

    let mut device_ids = HashSet::new();
    for device in &data.devices {
        if device.device_id.trim().is_empty() || device.device_name.trim().is_empty() {
            return Err("持久化设备路由缺少设备 ID 或名称".to_owned());
        }
        normalize_base_url(&device.base_url)?;
        if !device_ids.insert(device.device_id.as_str()) {
            return Err(format!("设备路由重复：{}", device.device_id));
        }
    }

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

    let mut profile_keys = HashSet::new();
    for profile in &data.profiles {
        if profile.device_id.trim().is_empty() || !device_ids.contains(profile.device_id.as_str()) {
            return Err("AI 配置关联了不存在的设备".to_owned());
        }
        validate_profile(profile.clone())?;
        if !profile_keys.insert((profile.device_id.as_str(), profile.tool)) {
            return Err(format!(
                "设备 {} 的 {} AI 配置重复",
                profile.device_id,
                ai_tool_name(profile.tool)
            ));
        }
    }

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

pub fn hook_config_filename(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex | AiTool::Cursor => "hooks.json",
        AiTool::ClaudeCode => "settings.json",
    }
}

pub fn normalize_base_url(value: &str) -> Result<String, String> {
    let normalized = value.trim().trim_end_matches('/');
    let has_supported_scheme =
        normalized.starts_with("http://") || normalized.starts_with("https://");

    if !has_supported_scheme || normalized.contains(char::is_whitespace) {
        return Err("基地址必须是以 http:// 或 https:// 开头的有效地址".to_owned());
    }

    let authority = normalized
        .split_once("://")
        .map_or("", |(_, authority)| authority);
    if authority.is_empty() || authority.starts_with(':') {
        return Err("基地址缺少有效的主机名或 IP".to_owned());
    }

    Ok(normalized.to_owned())
}

pub fn validate_device_route(
    device: &DiscoveredMonitorDevice,
) -> Result<MonitorDeviceRoute, String> {
    let device_id = device.id.trim();
    let device_name = device.name.trim();
    if device_id.is_empty() || device_name.is_empty() {
        return Err("请选择发现的 AIMonitor 设备".to_owned());
    }

    Ok(MonitorDeviceRoute {
        base_url: normalize_base_url(&device.base_url)?,
        device_id: device_id.to_owned(),
        device_name: device_name.to_owned(),
    })
}

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
    let device_id = profile.device_id.trim().to_owned();
    profile.device_id = device_id;
    if !(1..=25).contains(&profile.slot) {
        return Err("显示位置必须在 1 到 25 之间".to_owned());
    }
    if profile.hooks.len() != 4 {
        return Err("必须配置空闲、运行中、询问和异常四种行为".to_owned());
    }

    let mut behaviors = HashSet::new();
    for hook in &mut profile.hooks {
        hook.content = hook.content.trim().to_owned();
        hook.image = hook.image.trim().to_owned();
        if hook.image.is_empty() {
            return Err("每个行为都必须选择图片".to_owned());
        }
        if !behaviors.insert(hook.behavior) {
            return Err("同一行为不能重复配置".to_owned());
        }
    }
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
    let profile = validate_profile(profile)?;
    let mut hooks = Map::new();

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
    let session_end_event = native_session_end_event(profile.tool);
    let session_end_commands = managed_commands(profile.tool, session_end_event);
    insert_handler(
        &mut hooks,
        profile.tool,
        session_end_event,
        None,
        &session_end_commands,
    );

    let config = if profile.tool == AiTool::Cursor {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    } else {
        json!({ "hooks": Value::Object(hooks) })
    };
    let filename = match profile.tool {
        AiTool::Codex => ".codex/hooks.json",
        AiTool::ClaudeCode => ".claude/settings.json",
        AiTool::Cursor => ".cursor/hooks.json",
    };

    Ok(HookConfigPreview {
        filename: filename.to_owned(),
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
    let mut existing = match existing_content {
        Some(content) => serde_json::from_str::<Value>(content)
            .map_err(|error| format!("现有 Hooks 配置格式错误：{error}"))?,
        None => json!({}),
    };
    let generated_value = serde_json::from_str::<Value>(&generated.content)
        .map_err(|error| format!("生成的 Hooks 配置格式错误：{error}"))?;
    let existing_root = existing
        .as_object_mut()
        .ok_or_else(|| "现有 Hooks 配置的根节点必须是对象".to_owned())?;
    let generated_root = generated_value
        .as_object()
        .ok_or_else(|| "生成的 Hooks 配置的根节点必须是对象".to_owned())?;

    for (key, value) in generated_root {
        if key != "hooks" {
            existing_root
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    let existing_hooks = existing_root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "现有配置中的 hooks 必须是对象".to_owned())?;
    let generated_hooks = generated_root
        .get("hooks")
        .and_then(Value::as_object)
        .ok_or_else(|| "生成的配置缺少 hooks 对象".to_owned())?;
    let existing_events: Vec<_> = existing_hooks.keys().cloned().collect();
    for event in existing_events {
        let should_remove = existing_hooks.get_mut(&event).is_some_and(|entries| {
            let Some(entries) = entries.as_array_mut() else {
                return false;
            };
            remove_managed_entries(entries, tool);
            entries.is_empty()
        });
        if should_remove {
            existing_hooks.remove(&event);
        }
    }

    for (event, generated_entries) in generated_hooks {
        let generated_entries = generated_entries
            .as_array()
            .ok_or_else(|| format!("生成的 {event} 配置必须是数组"))?;
        let existing_entries = existing_hooks
            .entry(event.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("现有配置中的 {event} 必须是数组"))?;

        existing_entries.extend(generated_entries.iter().cloned());
    }

    Ok(HookConfigPreview {
        filename: generated.filename.clone(),
        content: serde_json::to_string_pretty(&existing)
            .map_err(|error| format!("无法生成合并后的 Hooks 配置：{error}"))?,
    })
}

impl HookBehavior {
    const DISPLAY_BEHAVIORS: [Self; 4] = [Self::Idle, Self::Running, Self::Asking, Self::Error];
}

struct NativeStateEvent {
    name: &'static str,
    matcher: Option<&'static str>,
    behavior: HookBehavior,
}

fn native_state_events(tool: AiTool) -> Vec<NativeStateEvent> {
    match tool {
        AiTool::Cursor => cursor_state_events(),
        AiTool::ClaudeCode => claude_state_events(),
        AiTool::Codex => codex_state_events(),
    }
}

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
    events.push(NativeStateEvent {
        name: "Notification",
        matcher: Some("idle_prompt"),
        behavior: HookBehavior::Idle,
    });
    events
}

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

fn native_session_end_event(tool: AiTool) -> &'static str {
    if tool == AiTool::Cursor {
        "sessionEnd"
    } else {
        "SessionEnd"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookTransition {
    Display(HookBehavior),
    Release,
}

pub fn hook_transition(tool: AiTool, event: &str) -> Option<HookTransition> {
    if event == native_session_end_event(tool) {
        return Some(if tool == AiTool::Codex {
            HookTransition::Display(HookBehavior::Idle)
        } else {
            HookTransition::Release
        });
    }

    native_state_events(tool)
        .into_iter()
        .find(|candidate| candidate.name == event)
        .map(|candidate| HookTransition::Display(candidate.behavior))
}

pub fn is_authoritative_terminal_event(tool: AiTool, event: &str) -> bool {
    event == native_session_end_event(tool)
        || matches!(event, "Stop" | "stop" | "Notification" | "StopFailure")
}

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

struct ManagedCommands {
    posix: String,
    windows: String,
}

fn insert_handler(
    hooks: &mut Map<String, Value>,
    tool: AiTool,
    event: &str,
    matcher: Option<&str>,
    commands: &ManagedCommands,
) {
    let handler = match tool {
        AiTool::Cursor => json!([{ "command": platform_command(commands) }]),
        AiTool::ClaudeCode => {
            let mut group = json!({
                "hooks": [{
                    "type": "command",
                    "command": platform_command(commands)
                }]
            });
            if let Some(matcher) = matcher {
                group["matcher"] = Value::String(matcher.to_owned());
            }
            Value::Array(vec![group])
        }
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

#[cfg(windows)]
fn platform_command(commands: &ManagedCommands) -> &str {
    &commands.windows
}

#[cfg(not(windows))]
fn platform_command(commands: &ManagedCommands) -> &str {
    &commands.posix
}

fn remove_managed_entries(entries: &mut Vec<Value>, tool: AiTool) {
    if tool == AiTool::Cursor {
        entries.retain(|entry| !entry_is_managed(entry, tool));
        return;
    }

    entries.retain_mut(|group| {
        let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        handlers.retain(|handler| !entry_is_managed(handler, tool));
        !handlers.is_empty()
    });
}

fn entry_is_managed(entry: &Value, tool: AiTool) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .any(|command| command_has_marker(command, &managed_hook_marker(tool)))
}

fn managed_commands(tool: AiTool, event: &str) -> ManagedCommands {
    let marker = managed_hook_marker(tool);
    let payload = serde_json::to_string(&json!({ "type": event }))
        .expect("fixed Hook event payload must serialize");
    let url = format!(
        "http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/{}",
        ai_tool_slug(tool)
    );
    let posix_marked = format!(
        ": {}; curl --silent --show-error --fail --connect-timeout 1 --max-time 3 \
         --request POST --header 'Content-Type: application/json' --data-binary {} {}",
        shell_quote(&marker),
        shell_quote(&payload),
        shell_quote(&url),
    );
    let posix = match tool {
        AiTool::Cursor => format!("{posix_marked} >/dev/null && printf '{{}}'"),
        // Codex Desktop and Claude Code interpret hook stdout as protocol
        // output. The monitor response is transport data, not hook feedback.
        AiTool::Codex | AiTool::ClaudeCode => format!("{posix_marked} >/dev/null"),
    };
    let mut windows_script = format!(
        "$null = '{}'; $ProgressPreference = 'SilentlyContinue'; \
         Invoke-RestMethod -Uri '{}' -Method Post -ContentType 'application/json' \
         -Body '{}' -TimeoutSec 3 | Out-Null",
        powershell_quote(&marker),
        powershell_quote(&url),
        powershell_quote(&payload),
    );
    if tool == AiTool::Cursor {
        windows_script.push_str("; Write-Output '{}'");
    }
    let windows = format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encode_powershell_command(&windows_script)
    );
    ManagedCommands { posix, windows }
}

fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}|tool={}", ai_tool_slug(tool))
}

const fn ai_tool_slug(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex => "codex",
        AiTool::ClaudeCode => "claude-code",
        AiTool::Cursor => "cursor",
    }
}

pub const fn ai_tool_name(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex => "Codex",
        AiTool::ClaudeCode => "Claude Code",
        AiTool::Cursor => "Cursor",
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn powershell_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn command_has_marker(command: &str, marker: &str) -> bool {
    decoded_hook_command(command).is_some_and(|decoded| {
        decoded
            .match_indices(marker)
            .any(|(start, _)| decoded[start + marker.len()..].starts_with('\''))
    })
}

fn decoded_hook_command(command: &str) -> Option<String> {
    if command.contains(MANAGED_HOOK_PREFIX) {
        return Some(command.to_owned());
    }
    let encoded = command
        .split_once("-EncodedCommand ")
        .map(|(_, encoded)| encoded.split_whitespace().next().unwrap_or(""))?;
    let bytes = decode_base64(encoded)?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let utf16: Vec<_> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&utf16).ok()
}

fn encode_powershell_command(script: &str) -> String {
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    encode_base64(&bytes)
}

pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0b11) << 4) | (second >> 4))],
        ));
        encoded.push(if chunk.len() > 1 {
            char::from(ALPHABET[usize::from(((second & 0b1111) << 2) | (third >> 6))])
        } else {
            '='
        });
        encoded.push(if chunk.len() > 2 {
            char::from(ALPHABET[usize::from(third & 0b11_1111)])
        } else {
            '='
        });
    }
    encoded
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
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
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks_exact(4) {
        let a = sextet(chunk[0])?;
        let b = sextet(chunk[1])?;
        let c = (chunk[2] != b'=').then(|| sextet(chunk[2])).flatten();
        let d = (chunk[3] != b'=').then(|| sextet(chunk[3])).flatten();
        decoded.push((a << 2) | (b >> 4));
        if let Some(c) = c {
            decoded.push((b << 4) | (c >> 2));
            if let Some(d) = d {
                decoded.push((c << 6) | d);
            } else if chunk[3] != b'=' {
                return None;
            }
        } else if chunk[2] != b'=' || chunk[3] != b'=' {
            return None;
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{
        AiProfile, AiTool, DEFAULT_BASE_URL, DEFAULT_DISCOVERY_INTERVAL_MINUTES, HookBehavior,
        HookConfigDirectories, HookConfigPreview, HookContent, HookTransition, MANAGED_HOOK_PREFIX,
        MAX_DISCOVERY_INTERVAL_MINUTES, MonitorDeviceRoute, MonitorSettings, SavedMonitorData,
        command_has_marker, decoded_hook_command, generate_hook_config, hook_transition,
        managed_hook_marker, merge_hook_config, normalize_base_url,
        validate_discovery_interval_minutes, validate_profile, validate_saved_monitor_data,
        validate_username,
    };

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

    #[test]
    fn base_url_is_trimmed_and_trailing_slashes_are_removed() {
        assert_eq!(
            normalize_base_url(" http://192.168.1.10:8080/ ").unwrap(),
            "http://192.168.1.10:8080"
        );
    }

    #[test]
    fn settings_require_a_username() {
        assert!(validate_username(" ").is_err());
        assert_eq!(validate_username(" Manon ").unwrap(), "Manon");
    }

    #[test]
    fn discovery_interval_defaults_to_one_minute_and_rejects_out_of_range_values() {
        assert_eq!(
            MonitorSettings::default().discovery_interval_minutes,
            DEFAULT_DISCOVERY_INTERVAL_MINUTES
        );
        assert_eq!(DEFAULT_DISCOVERY_INTERVAL_MINUTES, 1);
        assert!(validate_discovery_interval_minutes(0).is_err());
        assert!(validate_discovery_interval_minutes(MAX_DISCOVERY_INTERVAL_MINUTES + 1).is_err());
        assert_eq!(
            validate_discovery_interval_minutes(DEFAULT_DISCOVERY_INTERVAL_MINUTES).unwrap(),
            DEFAULT_DISCOVERY_INTERVAL_MINUTES
        );
    }

    #[test]
    fn profile_allows_empty_content_but_requires_an_image() {
        let mut invalid = profile(AiTool::Codex);
        invalid.hooks[0].image.clear();
        assert!(validate_profile(invalid).is_err());

        let mut valid = profile(AiTool::Codex);
        valid.hooks[0].content.clear();
        assert!(validate_profile(valid).is_ok());
    }

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
        assert!(validate_saved_monitor_data(&valid).is_ok());

        let mut duplicate = valid.clone();
        duplicate.profiles.push(profile(AiTool::Codex));
        assert!(validate_saved_monitor_data(&duplicate).is_err());

        let mut orphaned = valid;
        orphaned.profiles[0].device_id = "unknown-device".to_owned();
        assert!(validate_saved_monitor_data(&orphaned).is_err());
    }

    #[test]
    fn cursor_preview_uses_cursor_event_names_and_shape() {
        let preview = generate_hook_config(profile(AiTool::Cursor)).unwrap();

        assert_eq!(preview.filename, ".cursor/hooks.json");
        assert!(preview.content.contains("\"beforeSubmitPrompt\""));
        assert!(preview.content.contains("\"beforeShellExecution\""));
        assert!(preview.content.contains("\"beforeMCPExecution\""));
        assert!(preview.content.contains("\"afterFileEdit\""));
        assert!(preview.content.contains("\"workspaceOpen\""));
        assert!(!preview.content.contains("\"postToolUse\""));
        assert!(preview.content.contains("\"sessionEnd\""));
        assert!(preview.content.contains("127.0.0.1:10240/api/hooks/cursor"));
        assert!(preview.content.contains("AIMonitor|tool=cursor"));
        assert!(preview.content.contains("printf '{}'"));
        assert!(!preview.content.contains("\"notification\""));
        assert!(!preview.content.contains("\"type\": \"command\""));
    }

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
        assert!(preview.content.contains(">/dev/null"));
        let value: Value = serde_json::from_str(&preview.content).unwrap();
        assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
        assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
    }

    #[test]
    fn codex_preview_uses_pascal_case_and_nested_handlers() {
        let preview = generate_hook_config(profile(AiTool::Codex)).unwrap();

        assert_eq!(preview.filename, ".codex/hooks.json");
        assert!(preview.content.contains("\"SessionStart\""));
        assert!(preview.content.contains("\"UserPromptSubmit\""));
        assert!(preview.content.contains("\"PermissionRequest\""));
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
        let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(session_end.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(session_end.contains("SessionEnd"));
        let windows = value["hooks"]["Stop"][0]["hooks"][0]["commandWindows"]
            .as_str()
            .unwrap();
        let decoded = decoded_hook_command(windows).unwrap();
        assert!(decoded.contains("127.0.0.1:10240/api/hooks/codex"));
        assert!(decoded.contains(MANAGED_HOOK_PREFIX));
        assert!(command_has_marker(
            windows,
            &managed_hook_marker(AiTool::Codex)
        ));
    }

    #[test]
    fn codex_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(profile(AiTool::Codex)).unwrap();
        let first = merge_hook_config(None, &generated, AiTool::Codex).unwrap();
        let mut value: Value = serde_json::from_str(&first.content).unwrap();
        value["permissions"] = serde_json::json!({ "allow": ["Bash"] });
        value["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "hooks": [{ "type": "command", "command": "other-app notify" }]
            }));
        let existing = serde_json::to_string_pretty(&value).unwrap();
        let merged = merge_hook_config(Some(&existing), &generated, AiTool::Codex).unwrap();
        let value: Value = serde_json::from_str(&merged.content).unwrap();
        let stop = value["hooks"]["Stop"].as_array().unwrap();
        let serialized = serde_json::to_string(stop).unwrap();

        assert_eq!(serialized.matches("other-app notify").count(), 1);
        assert_eq!(serialized.matches(MANAGED_HOOK_PREFIX).count(), 1);
        assert_eq!(value["permissions"]["allow"][0], "Bash");
    }

    #[test]
    fn cursor_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(profile(AiTool::Cursor)).unwrap();
        let existing = r#"{
          "version": 1,
          "hooks": {
            "stop": [
              { "command": "other-app stop" },
              { "command": ": 'AIMonitor|tool=cursor'; curl current" }
            ]
          }
        }"#;

        let first = merge_hook_config(Some(existing), &generated, AiTool::Cursor).unwrap();
        let second = merge_hook_config(Some(&first.content), &generated, AiTool::Cursor).unwrap();
        let value: Value = serde_json::from_str(&second.content).unwrap();
        let stop = serde_json::to_string(&value["hooks"]["stop"]).unwrap();

        assert_eq!(stop.matches("other-app stop").count(), 1);
        assert_eq!(stop.matches("AIMonitor|tool=cursor").count(), 1);
    }

    #[test]
    fn merge_rejects_an_invalid_existing_config() {
        let generated = HookConfigPreview {
            filename: ".cursor/hooks.json".to_owned(),
            content: r#"{"version":1,"hooks":{}}"#.to_owned(),
        };

        assert!(merge_hook_config(Some(r#"{"hooks":[]}"#), &generated, AiTool::Cursor).is_err());
    }

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
}
