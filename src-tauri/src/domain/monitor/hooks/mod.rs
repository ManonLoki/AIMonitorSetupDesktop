//! AI 编程工具 Hook 协议适配层。
//!
//! 公共流程只依赖 [`HookProtocol`]；事件名、配置 JSON 结构、命令输出约定和
//! 托管条目布局由各工具的独立实现负责。

mod claude_code;
mod code_buddy;
mod codex;
mod cursor;
mod harness;
mod open_claw;
mod open_code;
mod work_buddy;

use serde_json::{Map, Value, json};

use super::{
    AiProfile, AiTool, DEFAULT_HOOK_RELAY_PORT, HookBehavior, HookConfigPreview, HookTransition,
    validate_profile,
};

pub(super) const MANAGED_HOOK_PREFIX: &str = "AIMonitor";
// AI 客户端与 AIMonitor 可能同时由登录项冷启动。Hook 进程通常会先于
// Tauri setup 完成，此时本机端口会短暂拒绝连接；允许额外重试 5 次，既覆盖
// 启动竞态，又保持失败有界，不把远端设备投递的“一次转换只发送一次”改成重试。
const LOCAL_RELAY_RETRY_COUNT: u8 = 5;
const LOCAL_RELAY_RETRY_DELAY_SECONDS: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookEventKind {
    SessionStart,
    WorkStart,
    WorkProgress(HookBehavior),
    WorkCompletion(HookBehavior),
    Stop,
    State(HookBehavior),
    SessionEnd,
}

impl HookEventKind {
    pub(super) const fn transition(self) -> HookTransition {
        match self {
            Self::SessionEnd => HookTransition::Release,
            Self::SessionStart | Self::Stop => HookTransition::Display(HookBehavior::Idle),
            Self::WorkStart => HookTransition::Display(HookBehavior::Running),
            Self::WorkProgress(behavior)
            | Self::WorkCompletion(behavior)
            | Self::State(behavior) => HookTransition::Display(behavior),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HookEvent {
    pub name: &'static str,
    pub matcher: Option<&'static str>,
    pub kind: HookEventKind,
}

impl HookEvent {
    pub const fn new(name: &'static str, kind: HookEventKind) -> Self {
        Self {
            name,
            matcher: None,
            kind,
        }
    }

    pub const fn with_matcher(
        name: &'static str,
        matcher: &'static str,
        kind: HookEventKind,
    ) -> Self {
        Self {
            name,
            matcher: Some(matcher),
            kind,
        }
    }
}

pub(super) struct ManagedCommands {
    pub posix: String,
    pub windows: String,
}

/// 单个工具必须实现的完整 Hook 协议契约。
pub(super) trait HookProtocol: Sync {
    fn tool(&self) -> AiTool;
    fn name(&self) -> &'static str;
    fn slug(&self) -> &'static str;
    fn config_filename(&self) -> &'static str;
    fn preview_filename(&self) -> &'static str;
    fn events(&self) -> &'static [HookEvent];

    /// 返回独立配置文件内容时，公共 JSON hooks 生成/合并流程会被跳过。
    /// 用于 `OpenCode` 这类以自动发现插件文件作为公开扩展入口的工具。
    fn standalone_config(&self) -> Option<String> {
        None
    }

    /// 返回与主配置文件一同写入的受管文件。所有文件都会先完成冲突校验，
    /// 再由 application 层统一落盘，避免只安装半套插件。
    fn auxiliary_configs(&self) -> Vec<HookConfigPreview> {
        Vec::new()
    }

    /// 合并独立文件。默认只覆盖带当前工具 `AIMonitor` 标识的受管文件；需要与
    /// 用户内容共存的独立格式（例如 Harness hooks.json 数组）可自行覆盖。
    fn merge_standalone(
        &self,
        existing_content: Option<&str>,
        generated: &HookConfigPreview,
    ) -> Result<String, String> {
        let marker = managed_hook_marker(self.tool());
        if existing_content.is_some_and(|content| !content.contains(&marker)) {
            return Err(format!(
                "现有 {} 不是 AIMonitor 管理的文件，已拒绝覆盖",
                generated.filename
            ));
        }
        Ok(generated.content.clone())
    }

    fn event_kind(&self, event: &HookEvent, _status: Option<&str>) -> HookEventKind {
        event.kind
    }

    /// 默认返回 `Null`：走 `standalone_config` 独立文件路线的工具无需实现。
    fn handler(&self, _event: &HookEvent, _commands: &ManagedCommands) -> Value {
        Value::Null
    }

    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "hooks": Value::Object(hooks) })
    }

    fn posix_command(&self, command: String) -> String {
        format!("{command} >/dev/null")
    }

    fn windows_script_suffix(&self) -> &'static str {
        ""
    }

    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            handlers.retain(|handler| !entry_is_managed(handler, self));
            !handlers.is_empty()
        });
    }

    /// 写入后是否需要用户在工具自身 UI/CLI 中审核或显式信任新增 Hook/插件。
    fn requires_review(&self) -> bool {
        false
    }

    /// 写入后是否需要重启工具进程/守护进程才能重新加载配置。
    fn restart_required(&self) -> bool {
        false
    }
}

pub(super) fn protocol(tool: AiTool) -> &'static dyn HookProtocol {
    match tool {
        AiTool::Codex => &codex::CODEX,
        AiTool::ClaudeCode => &claude_code::CLAUDE_CODE,
        AiTool::Cursor => &cursor::CURSOR,
        AiTool::OpenCode => &open_code::OPEN_CODE,
        AiTool::WorkBuddy => &work_buddy::WORK_BUDDY,
        AiTool::Harness => &harness::HARNESS,
        AiTool::OpenClaw => &open_claw::OPEN_CLAW,
        AiTool::CodeBuddy => &code_buddy::CODE_BUDDY,
    }
}

pub fn hook_config_filename(tool: AiTool) -> &'static str {
    protocol(tool).config_filename()
}

pub fn ai_tool_name(tool: AiTool) -> &'static str {
    protocol(tool).name()
}

/// 写入配置后该工具是否需要用户审核/信任新增 Hook，供 application 层构造写入结果。
pub fn hook_requires_review(tool: AiTool) -> bool {
    protocol(tool).requires_review()
}

/// 写入配置后该工具是否需要重启才能加载，供 application 层构造写入结果。
pub fn hook_restart_required(tool: AiTool) -> bool {
    protocol(tool).restart_required()
}

/// 按 Hook 请求路径中的 slug 反查对应工具，避免与各协议自身的 `slug()` 重复维护映射表。
pub fn tool_from_slug(slug: &str) -> Option<AiTool> {
    AiTool::ALL
        .into_iter()
        .find(|tool| protocol(*tool).slug() == slug)
}

pub fn generate_hook_auxiliary_configs(tool: AiTool) -> Vec<HookConfigPreview> {
    protocol(tool).auxiliary_configs()
}

pub(super) fn event_definition(tool: AiTool, event: &str) -> Option<HookEvent> {
    protocol(tool)
        .events()
        .iter()
        .copied()
        .find(|candidate| candidate.name == event)
}

pub(super) fn event_kind(tool: AiTool, event: &str, status: Option<&str>) -> Option<HookEventKind> {
    let protocol = protocol(tool);
    event_definition(tool, event).map(|definition| protocol.event_kind(&definition, status))
}

#[cfg(test)]
pub(super) fn hook_transition(tool: AiTool, event: &str) -> Option<HookTransition> {
    event_definition(tool, event).map(|definition| definition.kind.transition())
}

pub fn generate_hook_config(profile: AiProfile) -> Result<HookConfigPreview, String> {
    let profile = validate_profile(profile)?;
    let protocol = protocol(profile.tool);
    if let Some(content) = protocol.standalone_config() {
        return Ok(HookConfigPreview {
            filename: protocol.preview_filename().to_owned(),
            content,
        });
    }
    let mut hooks = Map::new();

    for event in protocol.events() {
        let commands = managed_commands(protocol, event.name);
        hooks.insert(event.name.to_owned(), protocol.handler(event, &commands));
    }

    let config = protocol.config_root(hooks);
    Ok(HookConfigPreview {
        filename: protocol.preview_filename().to_owned(),
        content: serde_json::to_string_pretty(&config)
            .map_err(|error| format!("无法生成 Hooks 配置：{error}"))?,
    })
}

pub fn merge_hook_config(
    existing_content: Option<&str>,
    generated: &HookConfigPreview,
    tool: AiTool,
) -> Result<HookConfigPreview, String> {
    let protocol = protocol(tool);
    if protocol.standalone_config().is_some() {
        return Ok(HookConfigPreview {
            filename: generated.filename.clone(),
            content: protocol.merge_standalone(existing_content, generated)?,
        });
    }
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
    for event in existing_hooks.keys().cloned().collect::<Vec<_>>() {
        let should_remove = existing_hooks.get_mut(&event).is_some_and(|entries| {
            let Some(entries) = entries.as_array_mut() else {
                return false;
            };
            protocol.remove_managed_entries(entries);
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

fn managed_commands(protocol: &dyn HookProtocol, event: &str) -> ManagedCommands {
    let marker = managed_hook_marker(protocol.tool());
    let url = format!(
        "http://127.0.0.1:{DEFAULT_HOOK_RELAY_PORT}/api/hooks/{}",
        protocol.slug()
    );
    let posix_marked = format!(
        ": {}; curl --silent --show-error --fail --connect-timeout 1 --max-time 3 \
         --retry {LOCAL_RELAY_RETRY_COUNT} --retry-delay {LOCAL_RELAY_RETRY_DELAY_SECONDS} \
         --retry-connrefused --retry-all-errors \
         --request POST --header 'Content-Type: application/json' --header {} \
         --data-binary @- {}",
        shell_quote(&marker),
        shell_quote(&format!("X-AIMonitor-Hook-Type: {event}")),
        shell_quote(&url),
    );
    let posix = protocol.posix_command(posix_marked);

    let windows_script = format!(
        "$null = '{}'; $ProgressPreference = 'SilentlyContinue'; \
         $body = [Console]::In.ReadToEnd(); \
         $attempt = 0; while ($true) {{ try {{ \
         Invoke-RestMethod -Uri '{}' -Method Post -ContentType 'application/json' \
         -Headers @{{'X-AIMonitor-Hook-Type'='{}'}} -Body $body -TimeoutSec 3 | Out-Null; break \
         }} catch {{ if ($attempt -ge {LOCAL_RELAY_RETRY_COUNT}) {{ throw }}; \
         $attempt++; Start-Sleep -Seconds {LOCAL_RELAY_RETRY_DELAY_SECONDS} }} }}{}",
        powershell_quote(&marker),
        powershell_quote(&url),
        powershell_quote(event),
        protocol.windows_script_suffix(),
    );
    let windows = format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        encode_powershell_command(&windows_script)
    );
    ManagedCommands { posix, windows }
}

/// 构造 Claude-Code 兼容协议共用的 `{ hooks: [{ type, command, matcher? }] }` 条目。
pub(super) fn command_group(command: &str, matcher: Option<&str>) -> Value {
    let mut group = json!({
        "hooks": [{
            "type": "command",
            "command": command,
        }]
    });
    if let Some(matcher) = matcher {
        group["matcher"] = Value::String(matcher.to_owned());
    }
    Value::Array(vec![group])
}

pub(super) fn platform_command(commands: &ManagedCommands) -> &str {
    #[cfg(windows)]
    {
        &commands.windows
    }
    #[cfg(not(windows))]
    {
        &commands.posix
    }
}

fn entry_is_managed<P: HookProtocol + ?Sized>(entry: &Value, protocol: &P) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .any(|command| command_has_marker(command, &managed_hook_marker(protocol.tool())))
}

pub(super) fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}|tool={}", protocol(tool).slug())
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn powershell_quote(value: &str) -> String {
    value.replace('\'', "''")
}

pub(super) fn command_has_marker(command: &str, marker: &str) -> bool {
    let contains_marker = |value: &str| {
        value.contains(&format!("{marker}'")) || value.contains(&format!("{marker}\""))
    };
    contains_marker(command)
        || decoded_hook_command(command)
            .as_deref()
            .is_some_and(contains_marker)
}

pub(super) fn decoded_hook_command(command: &str) -> Option<String> {
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
    let utf16 = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&utf16).ok()
}

fn encode_powershell_command(script: &str) -> String {
    let bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    encode_base64(&bytes)
}

fn encode_base64(bytes: &[u8]) -> String {
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
            }
        }
    }
    Some(decoded)
}
