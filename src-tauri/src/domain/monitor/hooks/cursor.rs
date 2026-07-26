use serde_json::{Map, Value, json};

use super::{
    HookEvent, HookEventKind, HookProtocol, ManagedCommands, entry_is_managed, platform_command,
};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static CURSOR: CursorProtocol = CursorProtocol;

pub(super) struct CursorProtocol;

const EVENTS: &[HookEvent] = &[
    HookEvent::new("workspaceOpen", HookEventKind::SessionStart),
    HookEvent::new("sessionStart", HookEventKind::SessionStart),
    HookEvent::new("beforeSubmitPrompt", HookEventKind::WorkStart),
    HookEvent::new(
        "afterFileEdit",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterShellExecution",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterMCPExecution",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "beforeShellExecution",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "beforeMCPExecution",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new(
        "preToolUse",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "postToolUse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "postToolUseFailure",
        HookEventKind::State(HookBehavior::Error),
    ),
    HookEvent::new(
        "subagentStart",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "subagentStop",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "preCompact",
        HookEventKind::WorkProgress(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterAgentResponse",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new(
        "afterAgentThought",
        HookEventKind::WorkCompletion(HookBehavior::Running),
    ),
    HookEvent::new("stop", HookEventKind::Stop),
    HookEvent::new("sessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CursorProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Cursor
    }
    fn name(&self) -> &'static str {
        "Cursor"
    }
    fn slug(&self) -> &'static str {
        "cursor"
    }
    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }
    fn preview_filename(&self) -> &'static str {
        ".cursor/hooks.json"
    }
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn event_kind(&self, event: &HookEvent, status: Option<&str>) -> HookEventKind {
        if event.name == "stop" && status == Some("error") {
            return HookEventKind::State(HookBehavior::Error);
        }
        event.kind
    }

    fn handler(&self, _event: &HookEvent, commands: &ManagedCommands) -> Value {
        json!([{ "command": platform_command(commands) }])
    }

    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "version": 1, "hooks": Value::Object(hooks) })
    }

    fn posix_command(&self, command: String) -> String {
        format!("{command} >/dev/null && printf '{{}}'")
    }

    fn windows_script_suffix(&self) -> &'static str {
        "; Write-Output '{}'"
    }

    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain(|entry| !entry_is_managed(entry, self));
    }
}
