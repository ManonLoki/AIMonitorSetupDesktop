//! AI 编程工具 Hook 协议适配层。
//!
//! 公共流程只依赖 [`HookProtocol`]；事件名、配置 JSON 结构、命令输出约定和
//! 托管条目布局由各工具的独立实现负责。

mod claude_code;
mod code_buddy;
mod codex;
mod cursor;
mod generation;
mod hermes;
mod open_claw;
mod open_code;
mod work_buddy;

use serde_json::{Map, Value, json};

use generation::command_has_marker;
pub use generation::{contains_managed_hook_config, generate_hook_config, merge_hook_config};

use super::{AiTool, HookBehavior, HookConfigPreview, HookTransition};

pub(super) const MANAGED_HOOK_PREFIX: &str = "AIMonitor";

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
    pub windows_powershell_host: String,
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
    /// 用户内容共存的独立格式可自行覆盖。
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
        AiTool::Hermes => &hermes::HERMES,
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
    #[cfg(target_os = "windows")]
    {
        &commands.windows
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        &commands.posix
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    compile_error!("AIMonitor Hook command generation only supports Windows, macOS, and Linux");
}

pub(crate) fn forwards_every_event(tool: AiTool) -> bool {
    // 只有具备稳定会话/轮次语义并经过状态机适配验证的四个工具执行抑制；
    // 其他协议按事件到达顺序直通，避免公共状态机误丢上游事件。
    !matches!(
        tool,
        AiTool::Codex | AiTool::ClaudeCode | AiTool::Cursor | AiTool::OpenCode
    )
}

fn entry_is_managed<P: HookProtocol + ?Sized>(entry: &Value, protocol: &P) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .any(|command| command_has_marker(command, &managed_hook_marker(protocol.tool())))
}

pub(crate) fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}|tool={}", protocol(tool).slug())
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
