use std::{collections::HashSet, fmt::Write as _};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const DEFAULT_BASE_URL: &str = "http://192.168.1.100:8080";
const LEGACY_MANAGED_HOOK_PREFIX: &str = "aimonitor-managed-hook:v1|target=";
const MANAGED_HOOK_PREFIX: &str = "aimonitor-managed-hook:v2|tool=";
const CODEX_HOOK_EVENTS: [&str; 11] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "SubagentStart",
    "SubagentStop",
    "Stop",
];
const CURSOR_HOOK_EVENTS: [&str; 21] = [
    "beforeShellExecution",
    "beforeMCPExecution",
    "afterShellExecution",
    "afterMCPExecution",
    "beforeReadFile",
    "afterFileEdit",
    "beforeTabFileRead",
    "afterTabFileEdit",
    "stop",
    "beforeSubmitPrompt",
    "afterAgentResponse",
    "afterAgentThought",
    "sessionStart",
    "sessionEnd",
    "preCompact",
    "subagentStart",
    "subagentStop",
    "preToolUse",
    "postToolUse",
    "postToolUseFailure",
    "workspaceOpen",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorSettings {
    pub base_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub device_name: String,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            username: String::new(),
            device_id: String::new(),
            device_name: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
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
    // Legacy variants are accepted so persisted v1 profiles can be migrated.
    SessionStart,
    SessionEnd,
    BeforePrompt,
    BeforeTool,
    AfterTool,
    BeforeCompact,
    SubagentStart,
    SubagentStop,
    Stop,
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
    pub tool: AiTool,
    /// 在展示屏上的显示位置，取值范围 1-25（校验见 `validate_profile`）。
    pub slot: u8,
    #[serde(default)]
    pub hooks: Vec<HookContent>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigPreview {
    pub filename: String,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct HookRunnerPaths {
    pub posix: String,
    pub windows: String,
}

#[derive(Clone, Debug)]
pub struct HookRunnerScripts {
    pub posix: String,
    pub windows: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookConfigWriteResult {
    pub profile: AiProfile,
    pub filename: String,
    pub config_changed: bool,
    /// 仅当写入的是 Codex 且配置发生变化时为真：Codex 不会热加载
    /// hooks.json，需要提示用户手动确认写入内容。
    pub requires_review: bool,
    /// 仅当写入的是 Codex 且配置发生变化时为真：需要提示用户重启 Codex
    /// 才能使新的 hooks 配置生效。
    pub restart_required: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalHookConfig {
    pub tool: AiTool,
    pub filename: String,
    pub exists: bool,
    pub valid: bool,
    /// 现有配置里的托管条目是否已指向当前版本的 runner 脚本路径
    /// （即是否已是最新版，无需迁移/重写）。
    pub stable_runner: bool,
    pub error: String,
    /// 从现有配置中解析出的、由本工具托管写入的事件名列表（去重排序）。
    pub managed_targets: Vec<String>,
    pub content: String,
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
    #[serde(default)]
    pub profiles: Vec<AiProfile>,
    #[serde(default)]
    pub hook_config_directories: HookConfigDirectories,
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

pub fn validate_settings(
    device: &DiscoveredMonitorDevice,
    username: &str,
) -> Result<MonitorSettings, String> {
    let username = username.trim();
    if username.is_empty() {
        return Err("用户名不能为空".to_owned());
    }
    let device_id = device.id.trim();
    let device_name = device.name.trim();
    if device_id.is_empty() || device_name.is_empty() {
        return Err("请选择发现的 AIMonitor 设备".to_owned());
    }

    Ok(MonitorSettings {
        base_url: normalize_base_url(&device.base_url)?,
        username: username.to_owned(),
        device_id: device_id.to_owned(),
        device_name: device_name.to_owned(),
    })
}

/// 校验 Profile 是否可用于生成 Hooks 配置：位置在 1-25 之间，且四种展示
/// 行为（空闲/运行中/询问/异常）各配置一次、都选择了图片。
pub fn validate_profile(mut profile: AiProfile) -> Result<AiProfile, String> {
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
        if !hook.behavior.is_display_behavior() {
            return Err("配置中包含已停用的旧行为".to_owned());
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
/// 文件内容：为每个原生事件写入调用 runner 脚本更新对应展示位置的命令。
pub fn generate_hook_config(
    profile: AiProfile,
    runner_paths: &HookRunnerPaths,
) -> Result<HookConfigPreview, String> {
    let profile = validate_profile(profile)?;
    let mut hooks = Map::new();

    for event in native_state_events(profile.tool) {
        let commands = managed_commands(profile.tool, event.name, runner_paths);
        insert_handler(
            &mut hooks,
            profile.tool,
            event.name,
            event.matcher,
            &commands,
        );
    }
    let session_end_event = native_session_end_event(profile.tool);
    let session_end_commands = managed_commands(profile.tool, session_end_event, runner_paths);
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

/// 生成 hooks 实际调用的 runner 脚本（POSIX sh 与 Windows PowerShell 各一份），
/// 按 `工具:事件` 分支，把每个 Profile 的展示状态更新为对应设备图片。
pub fn generate_hook_runner_scripts(
    settings: &MonitorSettings,
    profiles: &[AiProfile],
) -> Result<HookRunnerScripts, String> {
    let mut posix_cases = String::new();
    let mut windows_cases = String::new();

    for profile in profiles.iter().cloned().map(validate_profile) {
        let profile = profile?;
        for event in native_state_events(profile.tool) {
            let state = profile
                .hooks
                .iter()
                .find(|hook| hook.behavior == event.behavior)
                .ok_or_else(|| "行为配置不完整".to_owned())?;
            let retry = requires_delivery_retry(event.name);
            append_runner_case(
                &mut posix_cases,
                &mut windows_cases,
                profile.tool,
                event.name,
                &update_slot_command(settings, profile.tool, profile.slot, state, retry)?,
                &update_slot_command_windows(settings, profile.tool, profile.slot, state, retry)?,
            );
        }

        let session_end_event = native_session_end_event(profile.tool);
        let (posix, windows) = if profile.tool == AiTool::Codex {
            let idle = profile
                .hooks
                .iter()
                .find(|hook| hook.behavior == HookBehavior::Idle)
                .ok_or_else(|| "空闲行为配置不完整".to_owned())?;
            (
                update_slot_command(settings, profile.tool, profile.slot, idle, true)?,
                update_slot_command_windows(settings, profile.tool, profile.slot, idle, true)?,
            )
        } else {
            (
                release_slot_command(settings, profile.slot),
                release_slot_command_windows(settings, profile.slot),
            )
        };
        append_runner_case(
            &mut posix_cases,
            &mut windows_cases,
            profile.tool,
            session_end_event,
            &posix,
            &windows,
        );
    }

    Ok(HookRunnerScripts {
        posix: format!("#!/bin/sh\nset -eu\ncase \"$1:$2\" in\n{posix_cases}esac\n"),
        windows: format!(
            "param([string]$Tool, [string]$Event)\n\
             $ErrorActionPreference = 'Stop'\n\
             switch (\"${{Tool}}:${{Event}}\") {{\n\
             {windows_cases}}}\n"
        ),
    })
}

impl HookBehavior {
    const DISPLAY_BEHAVIORS: [Self; 4] = [Self::Idle, Self::Running, Self::Asking, Self::Error];

    const fn is_display_behavior(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Running | Self::Asking | Self::Error
        )
    }
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

/// 这些事件对应的通知在网络抖动时容易丢失（用户正等待反馈的关键节点），
/// 因此 runner 脚本需要对它们做请求重试，其余事件失败可直接忽略。
fn requires_delivery_retry(event: &str) -> bool {
    matches!(
        event,
        "UserPromptSubmit"
            | "beforeSubmitPrompt"
            | "Stop"
            | "stop"
            | "StopFailure"
            | "Notification"
    )
}

fn native_session_end_event(tool: AiTool) -> &'static str {
    if tool == AiTool::Cursor {
        "sessionEnd"
    } else {
        "SessionEnd"
    }
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
        .any(|command| {
            command_has_marker(command, &managed_hook_marker(tool))
                || decoded_hook_command(command)
                    .is_some_and(|decoded| decoded.contains(LEGACY_MANAGED_HOOK_PREFIX))
        })
}

fn managed_commands(tool: AiTool, event: &str, runner_paths: &HookRunnerPaths) -> ManagedCommands {
    let marker = managed_hook_marker(tool);
    let posix_marked = format!(
        ": {}; /bin/sh {} {} {}",
        shell_quote(&marker),
        shell_quote(&runner_paths.posix),
        shell_quote(ai_tool_slug(tool)),
        shell_quote(event),
    );
    let posix = match tool {
        AiTool::Cursor => format!("{posix_marked} >/dev/null && printf '{{}}'"),
        // Codex Desktop and Claude Code interpret hook stdout as protocol
        // output. The monitor response is transport data, not hook feedback.
        AiTool::Codex | AiTool::ClaudeCode => format!("{posix_marked} >/dev/null"),
    };
    let mut windows_script = format!(
        "$null = '{}'; & '{}' -NoProfile -NonInteractive -ExecutionPolicy Bypass -File '{}' '{}' '{}'",
        powershell_quote(&marker),
        powershell_quote("powershell.exe"),
        powershell_quote(&runner_paths.windows),
        powershell_quote(ai_tool_slug(tool)),
        powershell_quote(event),
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
    format!("{MANAGED_HOOK_PREFIX}{}", ai_tool_slug(tool))
}

const fn ai_tool_slug(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex => "codex",
        AiTool::ClaudeCode => "claude-code",
        AiTool::Cursor => "cursor",
    }
}

fn append_runner_case(
    posix_cases: &mut String,
    windows_cases: &mut String,
    tool: AiTool,
    event: &str,
    posix_command: &str,
    windows_command: &str,
) {
    let key = format!("{}:{event}", ai_tool_slug(tool));
    let _ = write!(
        posix_cases,
        "  {})\n    {posix_command} >/dev/null\n    ;;\n",
        shell_quote(&key)
    );
    let _ = writeln!(
        windows_cases,
        "  '{}' {{ {windows_command}; break }}",
        powershell_quote(&key)
    );
}

pub fn inspect_local_hook_config(
    tool: AiTool,
    filename: String,
    content: Option<String>,
) -> LocalHookConfig {
    let Some(content) = content else {
        return LocalHookConfig {
            tool,
            filename,
            exists: false,
            valid: true,
            stable_runner: false,
            error: String::new(),
            managed_targets: Vec::new(),
            content: String::new(),
        };
    };

    match serde_json::from_str::<Value>(&content) {
        Ok(value) => {
            let mut managed_targets = Vec::new();
            collect_managed_targets(&value, &mut managed_targets);
            let stable_runner = contains_stable_runner(&value);
            managed_targets.sort();
            managed_targets.dedup();
            let error = validate_local_hook_shape(tool, &value)
                .err()
                .unwrap_or_default();
            LocalHookConfig {
                tool,
                filename,
                exists: true,
                valid: error.is_empty(),
                stable_runner,
                error,
                managed_targets,
                content,
            }
        }
        Err(error) => LocalHookConfig {
            tool,
            filename,
            exists: true,
            valid: false,
            stable_runner: false,
            error: format!("JSON 格式错误：{error}"),
            managed_targets: Vec::new(),
            content,
        },
    }
}

fn contains_stable_runner(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            ["command", "commandWindows"]
                .into_iter()
                .filter_map(|key| object.get(key).and_then(Value::as_str))
                .any(|command| {
                    decoded_hook_command(command)
                        .is_some_and(|decoded| decoded.contains(MANAGED_HOOK_PREFIX))
                })
                || object.values().any(contains_stable_runner)
        }
        Value::Array(items) => items.iter().any(contains_stable_runner),
        _ => false,
    }
}

fn validate_local_hook_shape(tool: AiTool, value: &Value) -> Result<(), String> {
    let root = value
        .as_object()
        .ok_or_else(|| "配置根节点必须是对象".to_owned())?;
    let Some(hooks) = root.get("hooks") else {
        return Ok(());
    };
    let hooks = hooks
        .as_object()
        .ok_or_else(|| "hooks 必须是对象".to_owned())?;

    for (event, entries) in hooks {
        let entries = entries
            .as_array()
            .ok_or_else(|| format!("{event} 必须是数组"))?;
        if tool == AiTool::Codex && !CODEX_HOOK_EVENTS.contains(&event.as_str()) {
            return Err(format!("当前 Codex Desktop 不支持 Hook 事件：{event}"));
        }
        if tool == AiTool::Cursor && !CURSOR_HOOK_EVENTS.contains(&event.as_str()) {
            return Err(format!("不支持的 Cursor Hook 事件：{event}"));
        }
        validate_hook_entries(tool, event, entries)?;
    }
    Ok(())
}

fn validate_hook_entries(tool: AiTool, event: &str, entries: &[Value]) -> Result<(), String> {
    for (index, entry) in entries.iter().enumerate() {
        let entry = entry
            .as_object()
            .ok_or_else(|| format!("{event}[{index}] 必须是对象"))?;
        if tool == AiTool::Cursor {
            let command = entry.get("command").and_then(Value::as_str).unwrap_or("");
            if command.trim().is_empty() {
                return Err(format!("{event}[{index}].command 必须是非空字符串"));
            }
            continue;
        }

        let handlers = entry
            .get("hooks")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{event}[{index}].hooks 必须是数组"))?;
        if handlers.is_empty() {
            return Err(format!("{event}[{index}].hooks 不能为空"));
        }
        for (handler_index, handler) in handlers.iter().enumerate() {
            let handler = handler
                .as_object()
                .ok_or_else(|| format!("{event}[{index}].hooks[{handler_index}] 必须是对象"))?;
            let handler_type = handler.get("type").and_then(Value::as_str).unwrap_or("");
            if handler_type.trim().is_empty() {
                return Err(format!(
                    "{event}[{index}].hooks[{handler_index}].type 必须是非空字符串"
                ));
            }
            if handler_type == "command" {
                let command = handler.get("command").and_then(Value::as_str).unwrap_or("");
                if command.trim().is_empty() {
                    return Err(format!(
                        "{event}[{index}].hooks[{handler_index}].command 必须是非空字符串"
                    ));
                }
                if let Some(command_windows) = handler.get("commandWindows")
                    && command_windows.as_str().is_none_or(str::is_empty)
                {
                    return Err(format!(
                        "{event}[{index}].hooks[{handler_index}].commandWindows 必须是非空字符串"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn collect_managed_targets(value: &Value, targets: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for key in ["command", "commandWindows"] {
                if let Some(command) = object.get(key).and_then(Value::as_str)
                    && let Some(decoded) = decoded_hook_command(command)
                    && let Some(marker_start) = decoded.find(LEGACY_MANAGED_HOOK_PREFIX)
                {
                    let target_start = marker_start + LEGACY_MANAGED_HOOK_PREFIX.len();
                    let target = decoded[target_start..]
                        .split_once('\'')
                        .map_or(&decoded[target_start..], |(target, _)| target);
                    if !target.is_empty() {
                        targets.push(target.to_owned());
                    }
                }
            }
            for nested in object.values() {
                collect_managed_targets(nested, targets);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_managed_targets(item, targets);
            }
        }
        _ => {}
    }
}

fn update_slot_command(
    settings: &MonitorSettings,
    tool: AiTool,
    slot: u8,
    state: &HookContent,
    retry: bool,
) -> Result<String, String> {
    let payload = serde_json::to_string(&json!({
        "username": settings.username,
        "aiName": ai_tool_name(tool),
        "behavior": state.behavior,
        "content": state.content,
        "image": state.image,
    }))
    .map_err(|error| format!("无法生成状态请求：{error}"))?;
    let url = format!("{}/api/slots/{slot}", settings.base_url);
    let retry_options = if retry {
        " --retry 2 --retry-delay 1 --retry-all-errors"
    } else {
        ""
    };
    Ok(format!(
        "curl --silent --show-error --fail --connect-timeout 2 --max-time 5{retry_options} \
         --request POST --header 'Content-Type: application/json' --data-binary {} {}",
        shell_quote(&payload),
        shell_quote(&url)
    ))
}

fn update_slot_command_windows(
    settings: &MonitorSettings,
    tool: AiTool,
    slot: u8,
    state: &HookContent,
    retry: bool,
) -> Result<String, String> {
    let payload = serde_json::to_string(&json!({
        "username": settings.username,
        "aiName": ai_tool_name(tool),
        "behavior": state.behavior,
        "content": state.content,
        "image": state.image,
    }))
    .map_err(|error| format!("无法生成 Windows 状态请求：{error}"))?;
    let url = format!("{}/api/slots/{slot}", settings.base_url);
    let request = format!(
        "Invoke-RestMethod -Uri '{}' -Method Post -ContentType 'application/json' \
         -Body '{}' -TimeoutSec 5 | Out-Null",
        powershell_quote(&url),
        powershell_quote(&payload)
    );
    if retry {
        Ok(format!(
            "$ProgressPreference = 'SilentlyContinue'; $lastError = $null; \
         for ($attempt = 1; $attempt -le 3; $attempt++) {{ \
         try {{ {request}; $lastError = $null; break }} \
         catch {{ $lastError = $_; if ($attempt -lt 3) {{ Start-Sleep -Seconds 1 }} }} }}; \
         if ($null -ne $lastError) {{ throw $lastError }}"
        ))
    } else {
        Ok(format!(
            "$ProgressPreference = 'SilentlyContinue'; {request}"
        ))
    }
}

const fn ai_tool_name(tool: AiTool) -> &'static str {
    match tool {
        AiTool::Codex => "Codex",
        AiTool::ClaudeCode => "Claude Code",
        AiTool::Cursor => "Cursor",
    }
}

fn release_slot_command(settings: &MonitorSettings, slot: u8) -> String {
    let url = format!("{}/api/slots/{slot}", settings.base_url);
    format!(
        "curl --silent --show-error --fail --connect-timeout 2 --max-time 5 \
         --retry 2 --retry-delay 1 --retry-all-errors --request DELETE {}",
        shell_quote(&url)
    )
}

fn release_slot_command_windows(settings: &MonitorSettings, slot: u8) -> String {
    let url = format!("{}/api/slots/{slot}", settings.base_url);
    format!(
        "$ProgressPreference = 'SilentlyContinue'; $lastError = $null; \
         for ($attempt = 1; $attempt -le 3; $attempt++) {{ \
         try {{ Invoke-RestMethod -Uri '{}' -Method Delete -TimeoutSec 5 | Out-Null; \
         $lastError = $null; break }} catch {{ $lastError = $_; \
         if ($attempt -lt 3) {{ Start-Sleep -Seconds 1 }} }} }}; \
         if ($null -ne $lastError) {{ throw $lastError }}",
        powershell_quote(&url)
    )
}

pub fn migrate_legacy_profile(profile: &mut AiProfile) {
    profile.hooks = HookBehavior::DISPLAY_BEHAVIORS
        .into_iter()
        .map(|behavior| HookContent {
            behavior,
            content: find_migration_source(&profile.hooks, behavior)
                .map_or_else(String::new, |hook| hook.content.clone()),
            image: find_migration_source(&profile.hooks, behavior)
                .map_or_else(String::new, |hook| hook.image.clone()),
        })
        .collect();
}

/// 为旧版（v1）细粒度行为找到对应的新版展示行为迁移来源，按优先级取第一个
/// 已配置的旧行为内容；找不到则该展示行为迁移后为空，需要用户重新配置。
fn find_migration_source(hooks: &[HookContent], behavior: HookBehavior) -> Option<&HookContent> {
    let fallbacks: &[HookBehavior] = match behavior {
        HookBehavior::Idle => &[
            HookBehavior::Idle,
            HookBehavior::Stop,
            HookBehavior::SessionEnd,
            HookBehavior::SessionStart,
        ],
        HookBehavior::Running => &[
            HookBehavior::Running,
            HookBehavior::BeforePrompt,
            HookBehavior::BeforeTool,
            HookBehavior::AfterTool,
            HookBehavior::BeforeCompact,
            HookBehavior::SubagentStart,
            HookBehavior::SubagentStop,
        ],
        HookBehavior::Asking => &[HookBehavior::Asking],
        HookBehavior::Error => &[HookBehavior::Error],
        _ => &[],
    };
    fallbacks
        .iter()
        .find_map(|candidate| hooks.iter().find(|hook| hook.behavior == *candidate))
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
    if command.contains(MANAGED_HOOK_PREFIX) || command.contains(LEGACY_MANAGED_HOOK_PREFIX) {
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
        AiProfile, AiTool, DEFAULT_BASE_URL, DiscoveredMonitorDevice, DiscoverySource,
        HookBehavior, HookConfigPreview, HookContent, HookRunnerPaths, LEGACY_MANAGED_HOOK_PREFIX,
        MANAGED_HOOK_PREFIX, MonitorSettings, command_has_marker, decoded_hook_command,
        generate_hook_config, generate_hook_runner_scripts, inspect_local_hook_config,
        managed_hook_marker, merge_hook_config, migrate_legacy_profile, normalize_base_url,
        validate_profile, validate_settings,
    };

    fn profile(tool: AiTool) -> AiProfile {
        AiProfile {
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

    fn runner_paths() -> HookRunnerPaths {
        HookRunnerPaths {
            posix: "/tmp/ai monitor/aimonitor-hook.sh".to_owned(),
            windows: r"C:\AIMonitor Data\aimonitor-hook.ps1".to_owned(),
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
        assert!(
            validate_settings(
                &DiscoveredMonitorDevice {
                    id: "device-1".to_owned(),
                    name: "Desk Monitor".to_owned(),
                    api_version: "1".to_owned(),
                    base_url: DEFAULT_BASE_URL.to_owned(),
                    path: "/api/device".to_owned(),
                    discovery_source: DiscoverySource::Mdns,
                },
                " "
            )
            .is_err()
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
    fn cursor_preview_uses_cursor_event_names_and_shape() {
        let preview = generate_hook_config(profile(AiTool::Cursor), &runner_paths()).unwrap();

        assert_eq!(preview.filename, ".cursor/hooks.json");
        assert!(preview.content.contains("\"beforeSubmitPrompt\""));
        assert!(preview.content.contains("\"beforeShellExecution\""));
        assert!(preview.content.contains("\"beforeMCPExecution\""));
        assert!(preview.content.contains("\"afterFileEdit\""));
        assert!(preview.content.contains("\"workspaceOpen\""));
        assert!(!preview.content.contains("\"postToolUse\""));
        assert!(preview.content.contains("\"sessionEnd\""));
        assert!(
            preview
                .content
                .contains("/tmp/ai monitor/aimonitor-hook.sh")
        );
        assert!(
            preview
                .content
                .contains("aimonitor-managed-hook:v2|tool=cursor")
        );
        assert!(preview.content.contains("printf '{}'"));
        assert!(!preview.content.contains("\"notification\""));
        assert!(!preview.content.contains("\"type\": \"command\""));
    }

    #[test]
    fn claude_preview_covers_permission_and_lifecycle_events() {
        let preview = generate_hook_config(profile(AiTool::ClaudeCode), &runner_paths()).unwrap();

        assert_eq!(preview.filename, ".claude/settings.json");
        assert!(preview.content.contains("\"SessionStart\""));
        assert!(preview.content.contains("\"PermissionRequest\""));
        assert!(preview.content.contains("\"Elicitation\""));
        assert!(preview.content.contains("\"PostToolUse\""));
        assert!(preview.content.contains("\"PostToolUseFailure\""));
        assert!(preview.content.contains("\"StopFailure\""));
        assert!(preview.content.contains("\"SessionEnd\""));
        assert!(preview.content.contains("\"Notification\""));
        assert!(
            preview
                .content
                .contains("aimonitor-managed-hook:v2|tool=claude-code")
        );
        assert!(preview.content.contains(">/dev/null"));
        let value: Value = serde_json::from_str(&preview.content).unwrap();
        assert_eq!(value["hooks"]["Notification"][0]["matcher"], "idle_prompt");
        assert!(value["hooks"]["SessionStart"][0].get("matcher").is_none());
    }

    #[test]
    fn codex_preview_uses_pascal_case_and_nested_handlers() {
        let preview = generate_hook_config(profile(AiTool::Codex), &runner_paths()).unwrap();

        assert_eq!(preview.filename, ".codex/hooks.json");
        assert!(preview.content.contains("\"SessionStart\""));
        assert!(preview.content.contains("\"UserPromptSubmit\""));
        assert!(preview.content.contains("\"PermissionRequest\""));
        assert!(!preview.content.contains("\"Error\""));
        assert!(preview.content.contains("\"PostToolUse\""));
        assert!(preview.content.contains("\"SessionEnd\""));
        assert!(preview.content.contains(">/dev/null"));
        assert!(
            preview
                .content
                .contains("aimonitor-managed-hook:v2|tool=codex")
        );
        assert!(preview.content.contains("\"type\": \"command\""));
        assert!(preview.content.contains("\"commandWindows\""));
        assert!(preview.content.contains("powershell.exe"));
        assert!(preview.content.contains("-EncodedCommand"));
        let value: Value = serde_json::from_str(&preview.content).unwrap();
        let session_end = value["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(session_end.contains("aimonitor-hook.sh"));
        assert!(session_end.contains("SessionEnd"));
        let windows = value["hooks"]["Stop"][0]["hooks"][0]["commandWindows"]
            .as_str()
            .unwrap();
        let decoded = decoded_hook_command(windows).unwrap();
        assert!(decoded.contains("aimonitor-hook.ps1"));
        assert!(decoded.contains(MANAGED_HOOK_PREFIX));
        assert!(command_has_marker(
            windows,
            &managed_hook_marker(AiTool::Codex)
        ));
        assert!(!command_has_marker(
            windows,
            "aimonitor-managed-hook:v1|target=http://192.168.1.100:80800"
        ));
    }

    #[test]
    fn codex_local_config_rejects_events_not_supported_by_desktop() {
        let inspected = inspect_local_hook_config(
            AiTool::Codex,
            ".codex/hooks.json".to_owned(),
            Some(r#"{"hooks":{"Error":[]}}"#.to_owned()),
        );

        assert!(inspected.exists);
        assert!(!inspected.valid);
        assert!(inspected.error.contains("Codex Desktop"));
    }

    #[test]
    fn merge_replaces_all_legacy_entries_for_the_same_tool() {
        let generated = generate_hook_config(profile(AiTool::Codex), &runner_paths()).unwrap();
        let existing = r#"{
          "permissions": { "allow": ["Bash"] },
          "hooks": {
            "Stop": [
              { "hooks": [{ "type": "command", "command": "other-app notify" }] },
              { "hooks": [{ "type": "command", "command": ": 'aimonitor-managed-hook:v1|target=http://192.168.1.100:8080'; curl old" }] },
              { "hooks": [{ "type": "command", "command": ": 'aimonitor-managed-hook:v1|target=http://10.0.0.5:8080'; curl other-network" }] },
              { "hooks": [{ "type": "command", "command": ": 'aimonitor-managed-hook:v1|target=http://192.168.1.100:80800'; curl prefix-network" }] }
            ],
            "CustomEvent": [
              { "hooks": [{ "type": "command", "command": "keep custom" }] }
            ]
          }
        }"#;

        let merged = merge_hook_config(Some(existing), &generated, AiTool::Codex).unwrap();
        let value: Value = serde_json::from_str(&merged.content).unwrap();
        let stop = value["hooks"]["Stop"].as_array().unwrap();
        let serialized = serde_json::to_string(stop).unwrap();

        assert_eq!(serialized.matches("other-app notify").count(), 1);
        assert_eq!(serialized.matches("curl old").count(), 0);
        assert_eq!(serialized.matches("curl other-network").count(), 0);
        assert_eq!(serialized.matches("curl prefix-network").count(), 0);
        assert_eq!(serialized.matches(MANAGED_HOOK_PREFIX).count(), 1);
        assert_eq!(serialized.matches(LEGACY_MANAGED_HOOK_PREFIX).count(), 0);
        assert_eq!(value["permissions"]["allow"][0], "Bash");
        assert_eq!(
            value["hooks"]["CustomEvent"][0]["hooks"][0]["command"],
            "keep custom"
        );
    }

    #[test]
    fn cursor_merge_is_idempotent_and_preserves_other_commands() {
        let generated = generate_hook_config(profile(AiTool::Cursor), &runner_paths()).unwrap();
        let existing = r#"{
          "version": 1,
          "hooks": {
            "stop": [{ "command": "other-app stop" }],
            "notification": [
              { "command": ": 'aimonitor-managed-hook:v1|target=http://192.168.1.100:8080'; curl old-invalid-event" }
            ]
          }
        }"#;

        let first = merge_hook_config(Some(existing), &generated, AiTool::Cursor).unwrap();
        let second = merge_hook_config(Some(&first.content), &generated, AiTool::Cursor).unwrap();
        let value: Value = serde_json::from_str(&second.content).unwrap();
        let stop = serde_json::to_string(&value["hooks"]["stop"]).unwrap();

        assert_eq!(stop.matches("other-app stop").count(), 1);
        assert_eq!(
            stop.matches("aimonitor-managed-hook:v2|tool=cursor")
                .count(),
            1
        );
        assert!(value["hooks"].get("notification").is_none());
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
    fn runner_script_contains_mutable_payload_while_hook_definition_stays_stable() {
        let settings = MonitorSettings {
            base_url: DEFAULT_BASE_URL.to_owned(),
            username: "Manon".to_owned(),
            device_id: "device-1".to_owned(),
            device_name: "Desk Monitor".to_owned(),
        };
        let scripts = generate_hook_runner_scripts(&settings, &[profile(AiTool::Codex)]).unwrap();
        let preview = generate_hook_config(profile(AiTool::Codex), &runner_paths()).unwrap();

        assert!(scripts.posix.contains("/api/slots/4"));
        assert!(scripts.posix.contains("\"behavior\":\"running\""));
        assert!(scripts.posix.contains("codex:SessionEnd"));
        assert!(scripts.posix.contains("--retry-all-errors"));
        assert!(scripts.windows.contains("Invoke-RestMethod"));
        assert!(scripts.windows.contains("$attempt -le 3"));
        assert!(!preview.content.contains(DEFAULT_BASE_URL));
        assert!(!preview.content.contains("\"behavior\":\"running\""));
    }

    #[test]
    fn claude_runner_uses_idle_notification_as_terminal_fallback() {
        let settings = MonitorSettings {
            base_url: DEFAULT_BASE_URL.to_owned(),
            username: "Manon".to_owned(),
            device_id: "device-1".to_owned(),
            device_name: "Desk Monitor".to_owned(),
        };
        let scripts =
            generate_hook_runner_scripts(&settings, &[profile(AiTool::ClaudeCode)]).unwrap();

        let notification_case = scripts
            .posix
            .split("'claude-code:Notification')")
            .nth(1)
            .unwrap()
            .split(";;")
            .next()
            .unwrap();
        assert!(notification_case.contains("\"behavior\":\"idle\""));
        assert!(notification_case.contains("--retry-all-errors"));

        let pre_tool_case = scripts
            .posix
            .split("'claude-code:PreToolUse')")
            .nth(1)
            .unwrap()
            .split(";;")
            .next()
            .unwrap();
        assert!(!pre_tool_case.contains("--retry"));
    }

    #[test]
    fn local_config_inspection_lists_managed_targets() {
        let inspected = inspect_local_hook_config(
            AiTool::Cursor,
            ".cursor/hooks.json".to_owned(),
            Some(
                r#"{
                  "hooks": {
                    "stop": [
                      { "command": ": 'aimonitor-managed-hook:v1|target=http://10.0.0.5:8080'; curl one" },
                      { "command": ": 'aimonitor-managed-hook:v1|target=http://192.168.1.8:8080'; curl two" },
                      { "command": "other-app" }
                    ]
                  }
                }"#
                .to_owned(),
            ),
        );

        assert!(inspected.exists);
        assert!(inspected.valid);
        assert_eq!(
            inspected.managed_targets,
            vec![
                "http://10.0.0.5:8080".to_owned(),
                "http://192.168.1.8:8080".to_owned()
            ]
        );
    }

    #[test]
    fn local_cursor_config_reports_unknown_events() {
        let inspected = inspect_local_hook_config(
            AiTool::Cursor,
            ".cursor/hooks.json".to_owned(),
            Some(r#"{"version":1,"hooks":{"notification":[]}}"#.to_owned()),
        );

        assert!(!inspected.valid);
        assert_eq!(inspected.error, "不支持的 Cursor Hook 事件：notification");
    }

    #[test]
    fn local_config_inspection_rejects_malformed_handler_shapes() {
        let cursor = inspect_local_hook_config(
            AiTool::Cursor,
            ".cursor/hooks.json".to_owned(),
            Some(r#"{"version":1,"hooks":{"stop":[{"command":""}]}}"#.to_owned()),
        );
        assert!(!cursor.valid);
        assert!(cursor.error.contains("command"));

        let codex = inspect_local_hook_config(
            AiTool::Codex,
            ".codex/hooks.json".to_owned(),
            Some(r#"{"hooks":{"Stop":[{"hooks":[{"type":"command"}]}]}}"#.to_owned()),
        );
        assert!(!codex.valid);
        assert!(codex.error.contains("command"));

        let claude = inspect_local_hook_config(
            AiTool::ClaudeCode,
            ".claude/settings.json".to_owned(),
            Some(r#"{"hooks":{"Stop":[42]}}"#.to_owned()),
        );
        assert!(!claude.valid);
        assert!(claude.error.contains("必须是对象"));
    }

    #[test]
    fn legacy_profile_migration_preserves_representative_content_and_images() {
        let mut legacy = AiProfile {
            tool: AiTool::Codex,
            slot: 2,
            hooks: vec![
                HookContent {
                    behavior: HookBehavior::Stop,
                    content: "已完成".to_owned(),
                    image: "idle-old.png".to_owned(),
                },
                HookContent {
                    behavior: HookBehavior::BeforePrompt,
                    content: "处理中".to_owned(),
                    image: "running-old.gif".to_owned(),
                },
                HookContent {
                    behavior: HookBehavior::Asking,
                    content: "请确认".to_owned(),
                    image: "asking-old.png".to_owned(),
                },
                HookContent {
                    behavior: HookBehavior::Error,
                    content: "出错了".to_owned(),
                    image: "error-old.png".to_owned(),
                },
            ],
        };

        migrate_legacy_profile(&mut legacy);

        assert_eq!(legacy.hooks.len(), 4);
        assert_eq!(legacy.hooks[0].behavior, HookBehavior::Idle);
        assert_eq!(legacy.hooks[0].image, "idle-old.png");
        assert_eq!(legacy.hooks[1].behavior, HookBehavior::Running);
        assert_eq!(legacy.hooks[1].content, "处理中");
        assert_eq!(legacy.hooks[2].image, "asking-old.png");
        assert_eq!(legacy.hooks[3].image, "error-old.png");
    }
}
