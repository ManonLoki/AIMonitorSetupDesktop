use serde_json::Value;

use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group};
use crate::domain::monitor::{AiTool, HookBehavior, HookWriteOutcome};

pub(super) static WORK_BUDDY: WorkBuddyProtocol = WorkBuddyProtocol;

pub(super) struct WorkBuddyProtocol;

// WorkBuddy 内置 CodeBuddy Agent 引擎，并从 v2.48 起使用独立的
// ~/.workbuddy/settings.json；其 hooks 结构与 CodeBuddy/Claude Code 兼容。
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

impl HookProtocol for WorkBuddyProtocol {
    fn tool(&self) -> AiTool {
        AiTool::WorkBuddy
    }
    fn name(&self) -> &'static str {
        "WorkBuddy"
    }
    fn slug(&self) -> &'static str {
        "workbuddy"
    }
    fn config_filename(&self) -> &'static str {
        "settings.json"
    }
    fn preview_filename(&self) -> &'static str {
        ".workbuddy/settings.json"
    }
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        // WorkBuddy 的内置 CodeBuddy 引擎在 Windows 上也固定使用 Git Bash
        // 执行 command Hook；cmd.exe 包装命令会被 Bash 错误解析。
        command_group(&commands.posix, event.matcher)
    }

    // WorkBuddy 需要在 Hooks 面板审核，并重启或新建会话加载新规则。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        HookWriteOutcome::WorkBuddyReviewRequired
    }
}
