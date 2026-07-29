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
pub use generation::{generate_hook_config, generate_wsl_hook_config, merge_hook_config};

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

    /// 默认返回 `Null`：走 `standalone_config` 独立文件路线的工具无需实现。
    fn handler(&self, _event: &HookEvent, _commands: &ManagedCommands) -> Value {
        Value::Null
    }

    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "hooks": Value::Object(hooks) })
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

    /// 写入后是否需要用户在工具自身 UI/CLI 中审核或显式信任新增 Hook/插件。
    fn requires_review(&self) -> bool {
        false
    }

    /// 写入后是否需要重启工具进程/守护进程才能重新加载配置。
    fn restart_required(&self) -> bool {
        false
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

// 返回该工具随主配置一同写入的辅助文件（如插件清单），未实现的工具返回空列表。
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
    // 只有具备稳定会话/轮次语义并经过状态机适配验证的四个工具执行抑制；
    // 其他协议按事件到达顺序直通，避免公共状态机误丢上游事件。
    !matches!(
        tool,
        AiTool::Codex | AiTool::ClaudeCode | AiTool::Cursor | AiTool::OpenCode
    )
}

// 判断一条 hooks 配置条目是否携带该工具的 AIMonitor 管理标识，
// 供 `remove_managed_entries` 默认实现筛选出可安全移除的条目。
fn entry_is_managed<P: HookProtocol + ?Sized>(entry: &Value, protocol: &P) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        .any(|command| contains_command_marker(command, protocol.tool()))
}

// 生成写入配置命令中的 `--managed-by` 标识：relay 子进程与 `entry_is_managed`
// 都据此判断一条 hooks 记录是否由本应用生成/管理。
pub(crate) fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}:tool={}", protocol(tool).slug())
}

fn contains_managed_marker(content: &str, tool: AiTool) -> bool {
    content.contains(&managed_hook_marker(tool))
}

fn contains_command_marker(command: &str, tool: AiTool) -> bool {
    command_has_marker(command, &managed_hook_marker(tool))
}

// 按 POSIX shell 单引号规则转义一个参数，供拼接进生成的托管命令字符串。
pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
