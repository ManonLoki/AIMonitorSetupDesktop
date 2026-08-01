use serde_json::Value;

use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group, platform_command,
};
use crate::domain::monitor::{AiTool, HookBehavior, HookWriteOutcome};

pub(super) static GEMINI_CLI: GeminiCliProtocol = GeminiCliProtocol;

pub(super) struct GeminiCliProtocol;

// Gemini CLI 的用户级 ~/.gemini/settings.json 使用 Claude-compatible command
// groups。Notification 当前公开的确定类型只有 ToolPermission，适合映射为询问态。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    HookEvent::new("BeforeAgent", HookEventKind::WorkStart),
    HookEvent::new(
        "BeforeModel",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "AfterModel",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "BeforeToolSelection",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "BeforeTool",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "AfterTool",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PreCompress",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::with_matcher(
        "Notification",
        "ToolPermission",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("AfterAgent", HookEventKind::Stop),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for GeminiCliProtocol {
    fn tool(&self) -> AiTool {
        AiTool::GeminiCli
    }

    fn name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn slug(&self) -> &'static str {
        "gemini-cli"
    }

    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    fn preview_filename(&self) -> &'static str {
        ".gemini/settings.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // Gemini 严格解析 command Hook 的 stdout；轻量 relay 对本工具成功时统一
        // 输出唯一的 `{}`，这里不能再追加任何 shell 输出。
        command_group(platform_command(commands), event.matcher)
    }

    // 新会话重新读取用户设置，确保当前进程中的 Hook 快照不会继续沿用旧配置。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::RestartRequired
    }
}
