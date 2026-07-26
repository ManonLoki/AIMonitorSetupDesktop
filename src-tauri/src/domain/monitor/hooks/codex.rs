use serde_json::{Value, json};

use super::{HookEvent, HookEventKind, HookProtocol, ManagedCommands};
use crate::domain::monitor::{AiTool, HookBehavior};

pub(super) static CODEX: CodexProtocol = CodexProtocol;

pub(super) struct CodexProtocol;

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
        "PermissionRequest",
        HookEventKind::State(HookBehavior::Asking),
    ),
    HookEvent::new("Stop", HookEventKind::Stop),
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
    HookEvent::new("SessionEnd", HookEventKind::SessionEnd),
];

impl HookProtocol for CodexProtocol {
    fn tool(&self) -> AiTool {
        AiTool::Codex
    }
    fn name(&self) -> &'static str {
        "Codex"
    }
    fn slug(&self) -> &'static str {
        "codex"
    }
    fn config_filename(&self) -> &'static str {
        "hooks.json"
    }
    fn preview_filename(&self) -> &'static str {
        ".codex/hooks.json"
    }
    fn events(&self) -> &'static [HookEvent] {
        EVENTS
    }

    fn handler(&self, event: &HookEvent, commands: &ManagedCommands) -> Value {
        let mut command = json!({
            "type": "command",
            "command": commands.posix,
            "commandWindows": commands.windows,
        });
        if event.name == "SessionEnd" {
            command["timeout"] = json!(3);
        }
        json!([{ "hooks": [command] }])
    }
}
