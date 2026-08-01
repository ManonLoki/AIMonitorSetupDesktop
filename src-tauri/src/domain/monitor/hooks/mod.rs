//! AI 编程工具 Hook 协议适配层。
//!
//! 公共流程只依赖 [`HookProtocol`]；事件名、配置 JSON 结构、命令输出约定和
//! 托管条目布局由各工具的独立实现负责。

use std::time::Duration;

mod claude_code;
mod code_buddy;
mod codex;
mod cursor;
mod gemini_cli;
mod generation;
mod github_copilot;
mod hermes;
mod kimi_code;
mod open_claw;
mod open_code;
mod qoder;
mod qwen_code;
mod work_buddy;

#[cfg(test)]
mod tests_contract;

use serde_json::{Map, Value, json};

use generation::command_has_marker;
pub use generation::{generate_hook_config, generate_wsl_hook_config, merge_hook_config};

use super::{
    AiTool, AiToolDescriptor, HookBehavior, HookConfigPreview, HookConfigWriteResult,
    HookTransition, HookWriteOutcome,
};

pub(super) const MANAGED_HOOK_PREFIX: &str = "AIMonitor";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookEventKind {
    WorkspaceStart,
    SessionStart,
    WorkStart,
    UnscopedWorkStart(HookBehavior),
    UnscopedWorkCompletion,
    WorkProgress(HookBehavior),
    WorkCompletion(HookBehavior),
    Stop,
    TerminalState(HookBehavior),
    State(HookBehavior),
    SessionEnd,
}

impl HookEventKind {
    pub(super) const fn transition(self) -> HookTransition {
        match self {
            Self::SessionEnd => HookTransition::Release,
            Self::WorkspaceStart | Self::SessionStart | Self::Stop => {
                HookTransition::Display(HookBehavior::Idle)
            }
            Self::WorkStart | Self::UnscopedWorkCompletion => {
                HookTransition::Display(HookBehavior::Running)
            }
            Self::UnscopedWorkStart(behavior)
            | Self::WorkProgress(behavior)
            | Self::WorkCompletion(behavior)
            | Self::TerminalState(behavior)
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
    // WSL 配置必须选择 POSIX 命令；普通 Windows 配置仍保持现有 CMD 分支。
    pub is_wsl: bool,
    // 这两个字段只在 `#[cfg(target_os = "windows")]` 分支中被读取
    // （见 `platform_command` 与 `codex.rs` 的 `handler`），非 Windows 平台
    // 编译时会被 dead_code 检查误判为未使用，因此显式允许。
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub windows: String,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
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
        if existing_content.is_some_and(|content| !contains_managed_marker(content, self.tool())) {
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

    /// 生命周期交接缓冲；用于消歧无 ID End，也用于延后物理 Release。
    fn release_settle_delay(&self) -> Duration {
        Duration::ZERO
    }

    /// 是否允许显式 `SessionStart` 覆盖同 ID 墓碑；新的 `WorkStart` 始终可建新 epoch。
    fn session_start_revives_tombstone(&self) -> bool {
        true
    }

    /// 默认返回 `Null`：走 `standalone_config` 独立文件路线的工具无需实现。
    fn handler(&self, _event: &HookEvent, _commands: &ManagedCommands) -> Value {
        Value::Null
    }

    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "hooks": Value::Object(hooks) })
    }

    /// 把逐事件生成的 handler 序列化为最终配置。默认输出 JSON；若未来出现公开
    /// 配置格式不是 JSON 的 command Hook，可覆盖该步骤。
    fn render_config(&self, hooks: Map<String, Value>) -> Result<String, String> {
        serde_json::to_string_pretty(&self.config_root(hooks))
            .map_err(|error| format!("无法生成 Hooks 配置：{error}"))
    }

    /// 是否绕过公共 JSON 合并器。独立插件默认需要自定义合并；共享 TOML 等
    /// 非 JSON 配置也可显式启用，同时仍保留 command Hook 的 WSL 能力。
    fn uses_custom_merge(&self) -> bool {
        self.standalone_config().is_some()
    }

    /// 从一组 hooks 事件条目中过滤掉本工具的受管处理器；一个条目的处理器
    /// 全部被移除后，连同这个条目本身一起丢弃，避免留下空壳分组。
    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            handlers.retain(|handler| !entry_is_managed(handler, self));
            !handlers.is_empty()
        });
    }

    /// 配置实际变化后需要执行的激活流程。工具专属流程必须在协议内唯一声明，
    /// application 与前端不得再根据 `AiTool` 组合布尔值或推断指引。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::Active
    }
}

// 按 AI 工具类型分发到对应的静态协议实现，是本模块内所有分发函数的唯一入口。
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
        AiTool::QwenCode => &qwen_code::QWEN_CODE,
        AiTool::KimiCode => &kimi_code::KIMI_CODE,
        AiTool::Qoder => &qoder::QODER,
        AiTool::GeminiCli => &gemini_cli::GEMINI_CLI,
        AiTool::GitHubCopilot => &github_copilot::GITHUB_COPILOT,
    }
}

// 返回该工具主配置文件的文件名，供 application 层拼出完整配置路径。
pub fn hook_config_filename(tool: AiTool) -> &'static str {
    protocol(tool).config_filename()
}

/// WSL 内目前只托管 command Hook。原生插件直接从 Linux 进程访问 listener，
/// 其 Windows/WSL 网络边界与 command relay 不同，不能复用本分支。
pub fn hook_supports_wsl(tool: AiTool) -> bool {
    protocol(tool).standalone_config().is_none()
}

// 返回该工具的展示名称，用于转发请求体的 `aiName` 字段等面向用户的场景。
pub fn ai_tool_name(tool: AiTool) -> &'static str {
    protocol(tool).name()
}

/// 返回全部 AI 工具的稳定展示目录；顺序与 `AiTool::ALL` 保持一致，展示名称
/// 直接来自每个工具的 `HookProtocol`。
pub fn ai_tool_descriptors() -> Vec<AiToolDescriptor> {
    AiTool::ALL
        .into_iter()
        .map(|tool| AiToolDescriptor {
            tool,
            name: protocol(tool).name().to_owned(),
        })
        .collect()
}

/// 配置发生变化时，返回该工具协议唯一声明的写入结果。
#[cfg(test)]
pub(super) fn hook_changed_write_outcome(tool: AiTool) -> HookWriteOutcome {
    protocol(tool).changed_write_outcome()
}

/// 根据实际内容变化构造写入结果；未变化时统一返回 `Unchanged`。
pub(crate) fn hook_config_write_result(
    tool: AiTool,
    filename: String,
    config_changed: bool,
) -> HookConfigWriteResult {
    HookConfigWriteResult::from_changed_outcome(
        tool,
        filename,
        config_changed,
        protocol(tool).changed_write_outcome(),
    )
}

/// 兼容旧调用方：配置变化后该工具是否需要用户审核/信任。
#[cfg(test)]
pub(super) fn hook_requires_review(tool: AiTool) -> bool {
    protocol(tool).changed_write_outcome().requires_review()
}

/// 兼容旧调用方：配置变化后该工具是否需要重启才能加载。
#[cfg(test)]
pub(super) fn hook_restart_required(tool: AiTool) -> bool {
    protocol(tool).changed_write_outcome().restart_required()
}

/// 判断配置内容中是否已包含当前工具的 `AIMonitor` 管理标识。
pub fn hook_config_has_managed_marker(content: &str, tool: AiTool) -> bool {
    contains_managed_marker(content, tool)
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

pub(crate) fn release_settle_delay(tool: AiTool) -> Duration {
    protocol(tool).release_settle_delay()
}

pub(crate) fn session_start_revives_tombstone(tool: AiTool) -> bool {
    protocol(tool).session_start_revives_tombstone()
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

// 按编译目标平台选取应写入配置的命令变体（Windows 用 PowerShell 包装，POSIX 直接执行）。
pub(super) fn platform_command(commands: &ManagedCommands) -> &str {
    if commands.is_wsl {
        return &commands.posix;
    }
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
    // 只有具备稳定会话/工作开始语义并经过状态机适配验证的工具执行抑制；
    // 其他协议按事件到达顺序直通，避免公共状态机误丢上游事件。
    !matches!(
        tool,
        AiTool::Codex
            | AiTool::ClaudeCode
            | AiTool::Cursor
            | AiTool::OpenCode
            | AiTool::QwenCode
            | AiTool::KimiCode
            | AiTool::Qoder
            | AiTool::GeminiCli
            | AiTool::GitHubCopilot
    )
}

// 判断 hooks 配置条目是否携带该工具的 AIMonitor 管理标识。
fn entry_is_managed<P: HookProtocol + ?Sized>(entry: &Value, protocol: &P) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .any(|command| contains_command_marker(command, protocol.tool()))
}

// 生成 relay 与配置清理共同识别的 `--managed-by` 标识。
pub(crate) fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}:tool={}", protocol(tool).slug())
}

fn contains_managed_marker(content: &str, tool: AiTool) -> bool {
    content.contains(&managed_hook_marker(tool))
}

fn contains_command_marker(command: &str, tool: AiTool) -> bool {
    command_has_marker(command, &managed_hook_marker(tool))
}

pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
