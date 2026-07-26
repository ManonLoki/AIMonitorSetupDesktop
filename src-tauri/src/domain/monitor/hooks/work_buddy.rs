use serde_json::Value;

use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, command_group, platform_command,
};
use crate::domain::monitor::{AiTool, HookBehavior};

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
        command_group(platform_command(commands), event.matcher)
    }

    // WorkBuddy 需要在 Hooks 配置面板中审核并信任新增规则。
    fn requires_review(&self) -> bool {
        true
    }
}
