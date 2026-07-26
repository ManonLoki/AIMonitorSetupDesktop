use serde_json::Value;

use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static CODE_BUDDY: CodeBuddyProtocol = CodeBuddyProtocol;

pub(super) struct CodeBuddyProtocol;

// CodeBuddy Code v1.16+ 的公开 Hook 生命周期。只订阅影响展示状态的事件，
// 避免 FileChanged/ConfigChange 等旁路事件产生无意义的重复转发。
const EVENTS: &[HookEvent] = &[
    HookEvent::new("SessionStart", HookEventKind::SessionStart),
    HookEvent::new("UserPromptSubmit", HookEventKind::WorkStart),
    HookEvent::new(
        "PreToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "PermissionDenied",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new("Elicitation", HookEventKind::State(HookBehavior::Asking)),
    HookEvent::new("Stop", HookEventKind::Stop),
    HookEvent::new("StopFailure", HookEventKind::State(HookBehavior::Error)),
    HookEvent::new(
        "SubagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "SubagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "PreCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "PostCompact",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::with_matcher("Notification", "idle_prompt", HookEventKind::Stop),
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CodeBuddyProtocol {
    fn tool(&self) -> AiTool {
        AiTool::CodeBuddy
    }

    fn name(&self) -> &'static str {
        "CodeBuddy"
    }

    fn slug(&self) -> &'static str {
        "codebuddy"
    }

    fn config_filename(&self) -> &'static str {
        "settings.json"
    }

    fn preview_filename(&self) -> &'static str {
        ".codebuddy/settings.json"
    }

    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // CodeBuddy 在 Windows 上也强制通过 Git Bash 执行 command Hook，明确不支持
        // PowerShell，因此始终写入公共生成器的 POSIX/curl 命令。
        command_group(&commands.posix, event.matcher)
    }

    // CodeBuddy 需要运行 /hooks 审核并信任新增规则。
    fn requires_review(&self) -> bool {
        true
    }

    // CodeBuddy 启动时会快照 Hooks 配置，需重启或新建会话才能生效。
    fn restart_required(&self) -> bool {
        true
    }
}
